//! Private messages, the web side (marcuru_rs::dm is the database): the inbox, one conversation, sending text, cards,
//! photos and voice notes, "seen", who's online, and voice calls. A call's audio goes straight between the two
//! browsers (WebRTC); this only passes the calling messages along, and Cloudflare's relay (TURN) carries the audio
//! when two phones can't reach each other directly.
//!
//! Everything live waits on one long request per open page (`GET /dm/poll`), woken the moment something arrives,
//! like the vault chat. Admins can read every conversation from Admin › Messages; that never marks anything seen.
use crate::community::{acting, current, fail, gated, html, ok, photo_cap_mb, render, store_photo, TOO_BIG};
use crate::{admin, with_db, App, Chrome, Res};
use askama::Template;
use axum::{
    body::Bytes,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use marcuru_rs::{db, dm, members};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

const VAULT_NAME: &str = "Big Boys Vault";
const VAULT_AVATAR: &str = "/static/img/logo-sm-144.png?v=1";
const VOICE_MAX_BYTES: usize = 3 * 1024 * 1024;
/// A call that nobody picked up stops ringing after this long.
const RING_SECS: i64 = 45;

#[derive(Clone, Debug)]
struct Call {
    id: String,
    caller_side: i64,
    caller: i64,
    callee: Option<i64>,
    answered: i64,
}

pub struct DmHub {
    versions: moka::sync::Cache<i64, Arc<watch::Sender<u64>>>,
    signals: moka::sync::Cache<i64, Arc<Mutex<Vec<Value>>>>,
    online: moka::sync::Cache<i64, i64>,
    calls: moka::sync::Cache<i64, Arc<Mutex<Option<Call>>>>,
    waiting: tokio::sync::Semaphore,
    ice: Mutex<Option<(i64, Value)>>,
}

impl DmHub {
    pub fn new() -> Self {
        Self {
            versions: moka::sync::Cache::builder().max_capacity(20_000).time_to_idle(Duration::from_secs(3600)).build(),
            signals: moka::sync::Cache::builder().max_capacity(20_000).time_to_idle(Duration::from_secs(120)).build(),
            online: moka::sync::Cache::builder().max_capacity(20_000).time_to_live(Duration::from_secs(60)).build(),
            calls: moka::sync::Cache::builder().max_capacity(5_000).time_to_idle(Duration::from_secs(4 * 3600)).build(),
            waiting: tokio::sync::Semaphore::new(150),
            ice: Mutex::new(None),
        }
    }
    fn channel(&self, member: i64) -> Arc<watch::Sender<u64>> {
        self.versions.get_with(member, || Arc::new(watch::channel(1).0))
    }
    /// Wakes every open page of these members.
    fn bump(&self, members: &[i64]) {
        for m in members {
            self.channel(*m).send_modify(|v| *v += 1);
        }
    }
    fn signal(&self, members: &[i64], v: Value) {
        for m in members {
            let queue = self.signals.get_with(*m, || Arc::new(Mutex::new(vec![])));
            let mut q = match queue.lock() {
                Ok(q) => q,
                Err(p) => p.into_inner(),
            };
            q.push(v.clone());
            if q.len() > 200 {
                q.drain(..100);
            }
        }
        self.bump(members);
    }
    fn take_signals(&self, member: i64) -> Vec<Value> {
        self.signals.get(&member).and_then(|q| q.lock().ok().map(|mut q| std::mem::take(&mut *q))).unwrap_or_default()
    }
    fn is_online(&self, member: i64) -> bool {
        self.online.contains_key(&member)
    }
    fn call(&self, thread: i64) -> Arc<Mutex<Option<Call>>> {
        self.calls.get_with(thread, || Arc::new(Mutex::new(None)))
    }
}

/// How one side of a conversation looks: a trainer, or the vault.
fn person(c: &rusqlite::Connection, side: i64, hub: &DmHub, admins: &[i64]) -> rusqlite::Result<Value> {
    if side == dm::VAULT {
        return Ok(json!({ "id": dm::VAULT, "name": VAULT_NAME, "avatar": VAULT_AVATAR, "initial": "B", "online": admins.iter().any(|a| hub.is_online(*a)), "url": "/" }));
    }
    Ok(match members::get(c, side)? {
        Some(m) => json!({ "id": m.id, "name": m.store_title(), "avatar": m.avatar, "initial": m.initial(), "online": hub.is_online(m.id), "url": format!("/trainers/{}", m.id) }),
        None => json!({ "id": side, "name": "A trainer who left", "avatar": "", "initial": "?", "online": false, "url": "" }),
    })
}

/// The members behind a side: the trainer, or every full admin for the vault.
fn people(side: i64, admins: &[i64]) -> Vec<i64> {
    if side == dm::VAULT {
        admins.to_vec()
    } else {
        vec![side]
    }
}

/// Which side this member is on in a conversation, if any: themselves, or the vault for a full admin.
fn side_of(t: &dm::Thread, m: &members::Member) -> Option<i64> {
    if t.has(m.id) {
        Some(m.id)
    } else if t.is_vault() && m.role == "admin" {
        Some(dm::VAULT)
    } else {
        None
    }
}

fn msg_json(m: &dm::Message, my_side: i64) -> Value {
    let card: Value = if m.card.is_empty() { Value::Null } else { serde_json::from_str(&m.card).unwrap_or(Value::Null) };
    json!({ "id": m.id, "side": m.side, "mine": m.side == my_side, "kind": m.kind, "text": m.text, "card": card, "media": m.media, "thumb": m.media_thumb, "secs": m.secs, "at": m.created_at })
}

/// A copy of a card for a message, taken now, so the message keeps showing it as it was.
fn card_copy(c: &rusqlite::Connection, r: &CardRef) -> rusqlite::Result<Option<(i64, Value)>> {
    if let Some(id) = r.vault {
        let Some(p) = db::product_by_id(c, id)? else { return Ok(None) };
        return Ok(Some((
            dm::VAULT,
            json!({ "src": "vault", "id": p.id, "title": p.short_title(), "set": p.set_text(), "number": p.card_no, "cond": p.variant, "price": if p.price > 0.0 { vault_search::peso(p.price) } else { String::new() }, "image": p.img(360), "url": format!("/products/{}", p.handle), "sold": !p.available }),
        )));
    }
    if let Some(id) = r.card {
        let owner: Option<i64> = rusqlite::OptionalExtension::optional(c.query_row("SELECT member_id FROM member_cards WHERE id=?1", [id], |r| r.get(0)))?;
        let Some(owner) = owner else { return Ok(None) };
        let Some(mc) = members::card(c, owner, id)? else { return Ok(None) };
        return Ok(Some((
            owner,
            json!({ "src": "store", "id": mc.id, "title": mc.title, "set": mc.card_set, "number": mc.number, "cond": mc.cond, "price": if mc.has_price() { mc.price_text() } else { String::new() }, "image": mc.image, "url": format!("/trainers/{owner}/cards/{}", mc.id), "sold": mc.status == "Sold" }),
        )));
    }
    Ok(None)
}

#[derive(Deserialize, Default, Clone)]
pub struct CardRef {
    vault: Option<i64>,
    card: Option<i64>,
}

fn signed_out() -> Response {
    fail(StatusCode::UNAUTHORIZED, "Log in to send messages.")
}

// ---------- pages ----------

#[derive(Template)]
#[template(path = "messages.html")]
struct MessagesPage {
    nav: Chrome,
    admin: bool,
}

/// GET /me/messages: the inbox. The conversations are drawn by static/dm.js.
pub async fn page(State(app): State<App>, h: HeaderMap) -> Res {
    if current(&app, &h).await?.is_none() {
        return Ok(axum::response::Redirect::to("/login").into_response());
    }
    let mut nav = with_db(&app, |c| crate::chrome(c)).await?;
    nav.seo.title = format!("Messages · {}", nav.brand);
    Ok(html(render(MessagesPage { nav, admin: false })?))
}

/// GET /admin/messages: every conversation, for the admins.
pub async fn admin_page(State(app): State<App>, h: HeaderMap) -> Res {
    if !matches!(admin::power(&app, &h), admin::Power::Admin(_)) {
        return Ok(fail(StatusCode::NOT_FOUND, "Not found"));
    }
    let mut nav = with_db(&app, |c| crate::chrome(c)).await?;
    nav.seo.title = format!("All messages · {}", nav.brand);
    Ok(html(render(MessagesPage { nav, admin: true })?))
}

// ---------- reading ----------

/// GET /dm/inbox
pub async fn inbox(State(app): State<App>, h: HeaderMap) -> Res {
    let Some(m) = current(&app, &h).await? else { return Ok(signed_out()) };
    let hub = app.dm.clone();
    let (id, admin) = (m.id, m.role == "admin");
    let rows = with_db(&app, move |c| {
        let admins = dm::admins(c)?;
        let list = dm::inbox(c, id, admin, 100)?;
        list.iter()
            .map(|r| Ok(json!({ "id": r.id, "me": r.me, "other": person(c, r.other, &hub, &admins)?, "as_vault": r.me == dm::VAULT, "last": r.last_text, "last_mine": r.last_from == r.me, "at": r.last_at, "unread": r.unread })))
            .collect::<rusqlite::Result<Vec<Value>>>()
    })
    .await?;
    let unread: i64 = rows.iter().map(|r| r["unread"].as_i64().unwrap_or(0)).sum();
    Ok(ok(json!({ "threads": rows, "unread": unread, "me": id })))
}

#[derive(Deserialize)]
pub struct PageQs {
    before: Option<i64>,
}

/// GET /dm/{thread}: the latest messages (or older ones, with `before`).
pub async fn thread(State(app): State<App>, h: HeaderMap, Path(tid): Path<i64>, Query(q): Query<PageQs>) -> Res {
    let Some(m) = current(&app, &h).await? else { return Ok(signed_out()) };
    let hub = app.dm.clone();
    let out = with_db(&app, move |c| {
        let Some(t) = dm::thread(c, tid)? else { return Ok(None) };
        let Some(side) = side_of(&t, &m) else { return Ok(None) };
        let admins = dm::admins(c)?;
        let list = dm::messages(c, tid, None, q.before, 40)?;
        let other = t.other(side);
        Ok(Some(json!({
            "id": t.id, "me": side, "as_vault": side == dm::VAULT, "other": person(c, other, &hub, &admins)?,
            "seen": t.read_by(other), "messages": list.iter().map(|x| msg_json(x, side)).collect::<Vec<_>>(), "more": list.len() == 40,
        })))
    })
    .await?;
    Ok(match out {
        Some(v) => ok(v),
        None => fail(StatusCode::NOT_FOUND, "That conversation isn't yours."),
    })
}

#[derive(Deserialize)]
pub struct PollQs {
    v: Option<u64>,
    t: Option<i64>,
    after: Option<i64>,
}

/// GET /dm/poll: waits up to 25 seconds for something new for this member, then says what: unread count, new
/// messages in the open conversation, "seen", and call signals.
pub async fn poll(State(app): State<App>, h: HeaderMap, Query(q): Query<PollQs>) -> Res {
    let Some(m) = current(&app, &h).await? else { return Ok(signed_out()) };
    let hub = app.dm.clone();
    hub.online.insert(m.id, crate::unix_now());
    let mut retry = 0;
    if let Some(seen) = q.v {
        let mut rx = hub.channel(m.id).subscribe();
        if *rx.borrow_and_update() == seen {
            match hub.waiting.try_acquire() {
                Ok(_slot) => {
                    let _ = tokio::time::timeout(Duration::from_secs(25), rx.changed()).await;
                }
                Err(_) => retry = 6000,
            }
        }
    }
    hub.online.insert(m.id, crate::unix_now());
    let version = *hub.channel(m.id).borrow();
    let signals = hub.take_signals(m.id);
    let (id, admin) = (m.id, m.role == "admin");
    let hub2 = hub.clone();
    let (unread, open) = with_db(&app, move |c| {
        let unread: i64 = dm::inbox(c, id, admin, 200)?.iter().map(|r| r.unread).sum();
        let mut open = Value::Null;
        if let Some(tid) = q.t {
            if let Some(t) = dm::thread(c, tid)? {
                if let Some(side) = side_of(&t, &m) {
                    let admins = dm::admins(c)?;
                    let other = t.other(side);
                    let list = dm::messages(c, tid, Some(q.after.unwrap_or(0)), None, 100)?;
                    let online = if other == dm::VAULT { admins.iter().any(|a| hub2.is_online(*a)) } else { hub2.is_online(other) };
                    open = json!({ "id": tid, "messages": list.iter().map(|x| msg_json(x, side)).collect::<Vec<_>>(), "seen": t.read_by(other), "online": online });
                }
            }
        }
        Ok((unread, open))
    })
    .await?;
    Ok(ok(json!({ "v": version, "unread": unread, "signals": signals, "open": open, "retry": retry })))
}

// ---------- writing ----------

#[derive(Deserialize)]
pub struct OpenReq {
    #[serde(default)]
    vault: Option<i64>,
    #[serde(default)]
    card: Option<i64>,
    /// A member's id, or 0 for the vault.
    #[serde(default)]
    to: Option<i64>,
}

/// POST /dm/open: the conversation with the seller of a card (or with a trainer, or the vault), made if new,
/// and a copy of the card to send with the first words.
pub async fn open(State(app): State<App>, h: HeaderMap, Json(req): Json<OpenReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(_) if current(&app, &h).await?.is_none() => return Ok(signed_out()),
        Err(r) => return Ok(r),
    };
    let hub = app.dm.clone();
    let now = crate::unix_now();
    let out = with_db(&app, move |c| {
        let r = CardRef { vault: req.vault, card: req.card };
        let (to, card) = if r.vault.is_some() || r.card.is_some() {
            match card_copy(c, &r)? {
                Some((owner, copy)) => (owner, copy),
                None => return Ok(Err("That card isn't here any more.")),
            }
        } else {
            match req.to {
                Some(t) => (t, Value::Null),
                None => return Ok(Err("Who to message?")),
            }
        };
        if to == m.id {
            return Ok(Err("That's your own card."));
        }
        if to == dm::VAULT && m.role == "admin" {
            return Ok(Err("This is the vault's own card: you answer for the vault."));
        }
        if to != dm::VAULT {
            let Some(other) = members::get(c, to)? else { return Ok(Err("That trainer isn't here any more.")) };
            if other.blocked {
                return Ok(Err("That trainer can't get messages right now."));
            }
            // Anyone may write to a seller or an admin; anyone else only once a conversation exists (they wrote first).
            let known = dm::find_between(c, m.id, to)?.is_some();
            if !(other.seller || other.role == "admin" || known || m.role == "admin") {
                return Ok(Err("You can message sellers and the vault."));
            }
        }
        let t = dm::thread_between(c, m.id, to, now)?;
        let admins = dm::admins(c)?;
        Ok(Ok(json!({ "thread": t.id, "me": m.id, "other": person(c, to, &hub, &admins)?, "card": card })))
    })
    .await?;
    Ok(match out {
        Ok(v) => ok(v),
        Err(e) => fail(StatusCode::BAD_REQUEST, e),
    })
}

/// Who to wake and tell about a new message, and the admins who watch. `quiet`: wake pages, tell nobody (a call
/// that was answered is only written in the chat).
async fn after_post(app: &App, t: dm::Thread, side: i64, author: i64, preview: String, quiet: bool) {
    let hub = app.dm.clone();
    let now = crate::unix_now();
    let r = with_db(app, move |c| {
        let admins = dm::admins(c)?;
        let other = t.other(side);
        let sender = person(c, side, &hub, &admins)?["name"].as_str().unwrap_or("Someone").to_string();
        let receiver = person(c, other, &hub, &admins)?["name"].as_str().unwrap_or("someone").to_string();
        let words = format!("“{preview}”");
        let link = format!("/me/messages?t={}", t.id);
        // Writing in a chat means it was read: the writer's side (every admin, for the vault) has nothing waiting there.
        for who in people(side, &admins) {
            c.execute("UPDATE notifications SET is_read=1 WHERE member_id=?1 AND kind='message' AND link=?2 AND is_read=0", rusqlite::params![who, link])?;
        }
        let recipients: Vec<i64> = if quiet { vec![] } else { people(other, &admins).into_iter().filter(|x| *x != author).collect() };
        for r in &recipients {
            dm::notify(c, *r, "message", &sender, &words, &link, now)?;
        }
        // Every message, for the admins who aren't in it (the people in it are never told).
        if !t.is_vault() && !quiet {
            for a in admins.iter().filter(|a| !t.has(**a) && **a != author) {
                dm::notify(c, *a, "dm_watch", &format!("{sender} → {receiver}"), &words, &format!("/admin/messages?t={}", t.id), now)?;
            }
        }
        let mut wake: Vec<i64> = people(t.a, &admins);
        wake.extend(people(t.b, &admins));
        wake.extend(admins.iter().copied());
        wake.sort_unstable();
        wake.dedup();
        Ok(wake)
    })
    .await;
    if let Ok(wake) = r {
        app.dm.bump(&wake);
    }
}

/// Checks the sender may write here now: their side, and not too many messages lately.
async fn may_send(app: &App, h: &HeaderMap, tid: i64) -> Result<Result<(members::Member, dm::Thread, i64), Response>, (StatusCode, String)> {
    let m = match acting(app, h).await? {
        Ok(m) => m,
        Err(r) => return Ok(Err(r)),
    };
    let now = crate::unix_now();
    let id = m.id;
    let (t, minute, hour) = with_db(app, move |c| Ok((dm::thread(c, tid)?, dm::sent_since(c, id, now - 60)?, dm::sent_since(c, id, now - 3600)?))).await?;
    let Some(t) = t else { return Ok(Err(fail(StatusCode::NOT_FOUND, "That conversation isn't here."))) };
    let Some(side) = side_of(&t, &m) else { return Ok(Err(fail(StatusCode::FORBIDDEN, "That conversation isn't yours."))) };
    if minute >= dm::PER_MINUTE || hour >= dm::PER_HOUR {
        return Ok(Err(fail(StatusCode::TOO_MANY_REQUESTS, "You're sending a lot right now. Wait a moment and try again.")));
    }
    Ok(Ok((m, t, side)))
}

#[derive(Deserialize)]
pub struct SendReq {
    #[serde(default)]
    text: String,
    #[serde(default)]
    card: Option<CardRef>,
}

/// POST /dm/{thread}/send: words, or a card with words.
pub async fn send(State(app): State<App>, h: HeaderMap, Path(tid): Path<i64>, Json(req): Json<SendReq>) -> Res {
    let (m, t, side) = match may_send(&app, &h, tid).await? {
        Ok(x) => x,
        Err(r) => return Ok(r),
    };
    let text = members::clean_text(&req.text, dm::TEXT_MAX);
    let now = crate::unix_now();
    let (t2, author) = (t.clone(), m.id);
    let out = with_db(&app, move |c| {
        let mut new = dm::New { kind: "text".into(), text, ..Default::default() };
        if let Some(r) = req.card.as_ref().filter(|r| r.vault.is_some() || r.card.is_some()) {
            let Some((owner, copy)) = card_copy(c, r)? else { return Ok(Err("That card isn't here any more.")) };
            // A card in a conversation belongs to one of the two sides.
            if !t2.has(owner) {
                return Ok(Err("That card isn't from this conversation."));
            }
            new.kind = "card".into();
            new.card = copy.to_string();
        } else if new.text.is_empty() {
            return Ok(Err("Write something first."));
        }
        let id = dm::post(c, &t2, side, author, &new, now)?;
        Ok(Ok((id, dm::preview(&new), dm::messages(c, t2.id, Some(id - 1), None, 1)?)))
    })
    .await?;
    let (_, preview, list) = match out {
        Ok(x) => x,
        Err(e) => return Ok(fail(StatusCode::BAD_REQUEST, e)),
    };
    after_post(&app, t, side, m.id, preview, false).await;
    Ok(ok(json!({ "ok": true, "message": list.first().map(|x| msg_json(x, side)) })))
}

/// POST /dm/{thread}/photo (a "photo" form field): a photo, made smaller and re-made from the pixels.
pub async fn photo(State(app): State<App>, h: HeaderMap, Path(tid): Path<i64>, mut mp: Multipart) -> Res {
    let (m, t, side) = match may_send(&app, &h, tid).await? {
        Ok(x) => x,
        Err(r) => return Ok(r),
    };
    let room = with_db(&app, |c| Ok(members::photo_bytes(c)? + dm::media_bytes(c)?)).await?;
    if room >= photo_cap_mb() * 1024 * 1024 {
        return Ok(fail(StatusCode::INSUFFICIENT_STORAGE, "The vault's photo space is full right now. Words still work."));
    }
    let mut bytes = None;
    while let Some(f) = mp.next_field().await.map_err(|_| (StatusCode::BAD_REQUEST, "That photo didn't upload.".to_string()))? {
        if f.name() == Some("photo") {
            bytes = Some(f.bytes().await.map_err(|_| (StatusCode::BAD_REQUEST, "That photo didn't upload.".to_string()))?.to_vec());
        }
    }
    let Some(bytes) = bytes.filter(|b| !b.is_empty()) else { return Ok(fail(StatusCode::BAD_REQUEST, "That photo didn't upload.")) };
    let dir = PathBuf::from(&*app.uploads);
    let stem = format!("dm{}-{}{}", t.id, members::temp_password(), members::temp_password());
    let (path, thumb, size) = match gated(move || store_photo(bytes, dir, stem)).await? {
        Ok((l, s, _, _, size)) => (l, s, size),
        Err(e) if e == TOO_BIG => return Ok(fail(StatusCode::BAD_REQUEST, "That photo is too big. Please use a smaller one.")),
        Err(_) => return Ok(fail(StatusCode::BAD_REQUEST, "That photo couldn't be read. Try a JPEG or PNG.")),
    };
    let now = crate::unix_now();
    let (t2, author) = (t.clone(), m.id);
    let (id, list) = with_db(&app, move |c| {
        let new = dm::New { kind: "photo".into(), media: path, media_thumb: thumb, bytes: size, ..Default::default() };
        let id = dm::post(c, &t2, side, author, &new, now)?;
        Ok((id, dm::messages(c, t2.id, Some(id - 1), None, 1)?))
    })
    .await?;
    let _ = id;
    after_post(&app, t, side, m.id, "📷 Photo".into(), false).await;
    Ok(ok(json!({ "ok": true, "message": list.first().map(|x| msg_json(x, side)) })))
}

#[derive(Deserialize)]
pub struct VoiceQs {
    secs: Option<i64>,
}

/// What kind of recording this is, from its first bytes (never from what the browser says).
fn audio_ext(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        Some("webm")
    } else if b.starts_with(b"OggS") {
        Some("ogg")
    } else if b.len() > 12 && &b[4..8] == b"ftyp" {
        Some("mp4")
    } else if b.starts_with(b"ID3") || (b.len() > 2 && b[0] == 0xFF && b[1] & 0xE0 == 0xE0) {
        Some("mp3")
    } else {
        None
    }
}

/// POST /dm/{thread}/voice?secs=: a voice note, the recording itself as the body.
pub async fn voice(State(app): State<App>, h: HeaderMap, Path(tid): Path<i64>, Query(q): Query<VoiceQs>, body: Bytes) -> Res {
    let (m, t, side) = match may_send(&app, &h, tid).await? {
        Ok(x) => x,
        Err(r) => return Ok(r),
    };
    if body.len() < 200 || body.len() > VOICE_MAX_BYTES {
        return Ok(fail(StatusCode::BAD_REQUEST, "That recording is empty or too long (3 minutes at most)."));
    }
    let Some(ext) = audio_ext(&body) else { return Ok(fail(StatusCode::BAD_REQUEST, "That recording couldn't be read.")) };
    let room = with_db(&app, |c| Ok(members::photo_bytes(c)? + dm::media_bytes(c)?)).await?;
    if room >= photo_cap_mb() * 1024 * 1024 {
        return Ok(fail(StatusCode::INSUFFICIENT_STORAGE, "The vault's space is full right now. Words still work."));
    }
    let name = format!("dm{}-v{}{}.{ext}", t.id, members::temp_password(), members::temp_password());
    let file = PathBuf::from(&*app.uploads).join(&name);
    let size = body.len() as i64;
    tokio::fs::write(&file, &body).await.map_err(crate::ise)?;
    let secs = q.secs.unwrap_or(0).clamp(1, 180);
    let now = crate::unix_now();
    let (t2, author) = (t.clone(), m.id);
    let list = with_db(&app, move |c| {
        let new = dm::New { kind: "voice".into(), media: format!("/uploads/{name}"), secs, bytes: size, ..Default::default() };
        let id = dm::post(c, &t2, side, author, &new, now)?;
        dm::messages(c, t2.id, Some(id - 1), None, 1)
    })
    .await?;
    after_post(&app, t, side, m.id, "🎤 Voice message".into(), false).await;
    Ok(ok(json!({ "ok": true, "message": list.first().map(|x| msg_json(x, side)) })))
}

#[derive(Deserialize)]
pub struct ReadReq {
    upto: i64,
}

/// POST /dm/{thread}/read: seen up to a message (the other side sees "Seen").
pub async fn read(State(app): State<App>, h: HeaderMap, Path(tid): Path<i64>, Json(req): Json<ReadReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let wake = with_db(&app, move |c| {
        let Some(t) = dm::thread(c, tid)? else { return Ok(vec![]) };
        let Some(side) = side_of(&t, &m) else { return Ok(vec![]) };
        let admins = dm::admins(c)?;
        let mut wake = people(t.other(side), &admins);
        if side == dm::VAULT {
            wake.extend(admins.iter().copied()); // the other admins' badges go down too
        }
        // Reading the vault's mail also clears its notifications for every admin.
        if dm::mark_read(c, &t, side, req.upto)? {
            let link = format!("/me/messages?t={}", t.id);
            for who in people(side, &admins) {
                c.execute("UPDATE notifications SET is_read=1 WHERE member_id=?1 AND kind='message' AND link=?2", rusqlite::params![who, link])?;
            }
            Ok(wake)
        } else {
            Ok(vec![])
        }
    })
    .await?;
    app.dm.bump(&wake);
    Ok(ok(json!({ "ok": true })))
}

// ---------- calls ----------

#[derive(Deserialize)]
pub struct CallReq {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    call: String,
    #[serde(default)]
    data: Value,
}

/// POST /dm/{thread}/call: ring, accept, decline, end, and the browsers' connection details (ice), passed along.
pub async fn call(State(app): State<App>, h: HeaderMap, Path(tid): Path<i64>, Json(req): Json<CallReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if req.data.to_string().len() > 60_000 {
        return Ok(fail(StatusCode::BAD_REQUEST, "Too much."));
    }
    let hub = app.dm.clone();
    let me = m.clone();
    let info = with_db(&app, move |c| {
        let Some(t) = dm::thread(c, tid)? else { return Ok(None) };
        let Some(side) = side_of(&t, &me) else { return Ok(None) };
        let admins = dm::admins(c)?;
        let who = person(c, side, &hub, &admins)?;
        Ok(Some((t, side, admins, who)))
    })
    .await?;
    let Some((t, side, admins, who)) = info else { return Ok(fail(StatusCode::NOT_FOUND, "That conversation isn't yours.")) };
    let other_people: Vec<i64> = people(t.other(side), &admins).into_iter().filter(|x| *x != m.id).collect();
    let now = crate::unix_now();
    let slot = app.dm.call(t.id);
    let mut log: Option<(i64, i64, String)> = None;
    let reply = {
        let mut cur = slot.lock().map_err(crate::ise)?;
        // A call left ringing too long is over.
        if let Some(c) = cur.as_ref() {
            if c.callee.is_none() && now - c.answered > RING_SECS && req.kind == "ring" {
                *cur = None;
            }
        }
        match req.kind.as_str() {
            "ring" => {
                if cur.is_some() {
                    return Ok(fail(StatusCode::CONFLICT, "There's already a call in this conversation."));
                }
                let id = members::temp_password();
                *cur = Some(Call { id: id.clone(), caller_side: side, caller: m.id, callee: None, answered: now });
                app.dm.signal(&other_people, json!({ "type": "ring", "thread": t.id, "call": id, "from": who, "data": req.data }));
                json!({ "ok": true, "call": id })
            }
            kind => {
                let Some(c) = cur.clone().filter(|c| c.id == req.call) else { return Ok(ok(json!({ "ok": false, "gone": true }))) };
                let from_caller = m.id == c.caller;
                let base = json!({ "thread": t.id, "call": c.id, "data": req.data });
                let with = |k: &str| {
                    let mut v = base.clone();
                    v["type"] = json!(k);
                    v
                };
                match kind {
                    "accept" if !from_caller && c.callee.is_none() => {
                        let mut c2 = c.clone();
                        c2.callee = Some(m.id);
                        c2.answered = now;
                        *cur = Some(c2);
                        app.dm.signal(&[c.caller], with("answer"));
                        // Answered for the vault: the other admins' phones stop ringing.
                        let others: Vec<i64> = people(side, &admins).into_iter().filter(|x| *x != m.id).collect();
                        app.dm.signal(&others, with("taken"));
                    }
                    "ice" => {
                        let to: Vec<i64> = if from_caller { c.callee.map(|x| vec![x]).unwrap_or_else(|| people(t.other(c.caller_side), &admins)) } else { vec![c.caller] };
                        app.dm.signal(&to, with("ice"));
                    }
                    "decline" if !from_caller => {
                        app.dm.signal(&[c.caller], with("declined"));
                        *cur = None;
                        log = Some((c.caller_side, c.caller, "Missed voice call".into()));
                    }
                    "end" => {
                        let mut to: Vec<i64> = if from_caller { c.callee.map(|x| vec![x]).unwrap_or_else(|| people(t.other(c.caller_side), &admins)) } else { vec![c.caller] };
                        to.retain(|x| *x != m.id);
                        app.dm.signal(&to, with("ended"));
                        *cur = None;
                        let text = match c.callee {
                            Some(_) => {
                                let s = (now - c.answered).max(0);
                                format!("Voice call · {}:{:02}", s / 60, s % 60)
                            }
                            None => "Missed voice call".into(),
                        };
                        log = Some((c.caller_side, c.caller, text));
                    }
                    _ => return Ok(fail(StatusCode::BAD_REQUEST, "That call step doesn't fit.")),
                }
                json!({ "ok": true })
            }
        }
    };
    if let Some((caller_side, caller, text)) = log {
        let t2 = t.clone();
        let txt = text.clone();
        with_db(&app, move |c| {
            let new = dm::New { kind: "call".into(), text: txt, ..Default::default() };
            dm::post(c, &t2, caller_side, caller, &new, now)
        })
        .await?;
        let answered = !text.starts_with("Missed");
        after_post(&app, t, caller_side, caller, text, answered).await;
    }
    Ok(ok(reply))
}

/// GET /dm/ice: how the browsers reach each other for a call. With CF_TURN_KEY_ID and CF_TURN_API_TOKEN set,
/// Cloudflare's relay too (credentials good for a day, made again every 12 hours); otherwise public STUN only.
pub async fn ice(State(app): State<App>, h: HeaderMap) -> Res {
    if current(&app, &h).await?.is_none() {
        return Ok(signed_out());
    }
    let now = crate::unix_now();
    if let Some((at, v)) = app.dm.ice.lock().map_err(crate::ise)?.clone() {
        if now - at < 12 * 3600 {
            return Ok(ok(v));
        }
    }
    let stun = json!({ "iceServers": [{ "urls": ["stun:stun.cloudflare.com:3478", "stun:stun.l.google.com:19302"] }] });
    let (Ok(key), Ok(token)) = (std::env::var("CF_TURN_KEY_ID"), std::env::var("CF_TURN_API_TOKEN")) else { return Ok(ok(stun)) };
    let url = format!("https://rtc.live.cloudflare.com/v1/turn/keys/{}/credentials/generate-ice-servers", key.trim());
    let r = app.http.post(url).bearer_auth(token.trim()).timeout(Duration::from_secs(8)).json(&json!({ "ttl": 86_400 })).send().await;
    let v = match r {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.ok(),
        Ok(r) => {
            tracing::warn!("Cloudflare TURN: {}", r.status());
            None
        }
        Err(e) => {
            tracing::warn!("Cloudflare TURN: {e}");
            None
        }
    };
    let Some(mut v) = v.filter(|v| v["iceServers"].is_array()) else { return Ok(ok(stun)) };
    // Browsers refuse port 53; those addresses would only make the call wait.
    if let Some(list) = v["iceServers"].as_array_mut() {
        for s in list.iter_mut() {
            if let Some(urls) = s["urls"].as_array_mut() {
                urls.retain(|u| !u.as_str().unwrap_or("").contains(":53"));
            }
        }
    }
    *app.dm.ice.lock().map_err(crate::ise)? = Some((now, v.clone()));
    Ok(ok(v))
}

// ---------- admins ----------

/// GET /admin/dm: every conversation, newest first.
pub async fn admin_list(State(app): State<App>, h: HeaderMap) -> Res {
    if !matches!(admin::power(&app, &h), admin::Power::Admin(_)) {
        return Ok(fail(StatusCode::NOT_FOUND, "Not found"));
    }
    let hub = app.dm.clone();
    let rows = with_db(&app, move |c| {
        let admins = dm::admins(c)?;
        dm::all(c, 300)?
            .iter()
            .map(|t| Ok(json!({ "id": t.id, "a": person(c, t.a, &hub, &admins)?, "b": person(c, t.b, &hub, &admins)?, "last": t.last_text, "at": t.last_at, "vault": t.is_vault() })))
            .collect::<rusqlite::Result<Vec<Value>>>()
    })
    .await?;
    Ok(ok(json!({ "threads": rows })))
}

/// GET /admin/dm/{thread}: one conversation, read without marking anything seen.
pub async fn admin_thread(State(app): State<App>, h: HeaderMap, Path(tid): Path<i64>, Query(q): Query<PageQs>) -> Res {
    if !matches!(admin::power(&app, &h), admin::Power::Admin(_)) {
        return Ok(fail(StatusCode::NOT_FOUND, "Not found"));
    }
    let hub = app.dm.clone();
    let out = with_db(&app, move |c| {
        let Some(t) = dm::thread(c, tid)? else { return Ok(None) };
        let admins = dm::admins(c)?;
        let list = dm::messages(c, tid, None, q.before, 60)?;
        // Shown from the first side's point of view (the vault's, in a vault conversation).
        Ok(Some(json!({
            "id": t.id, "me": t.a, "watch": true, "as_vault": t.is_vault(), "a": person(c, t.a, &hub, &admins)?, "other": person(c, t.b, &hub, &admins)?,
            "seen": t.b_read, "messages": list.iter().map(|x| { let mut v = msg_json(x, t.a); v["author"] = json!(x.author); v }).collect::<Vec<_>>(), "more": list.len() == 60,
        })))
    })
    .await?;
    Ok(match out {
        Some(v) => ok(v),
        None => fail(StatusCode::NOT_FOUND, "Not found"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recordings_are_known_by_their_first_bytes() {
        assert_eq!(audio_ext(&[0x1A, 0x45, 0xDF, 0xA3, 1, 2]), Some("webm"));
        assert_eq!(audio_ext(b"OggS\0\0\0\0"), Some("ogg"));
        assert_eq!(audio_ext(b"\0\0\0\x1cftypM4A \0\0\0\0"), Some("mp4"));
        assert_eq!(audio_ext(b"ID3\x04\0\0"), Some("mp3"));
        assert_eq!(audio_ext(b"<html>hello</html>"), None);
    }
}
