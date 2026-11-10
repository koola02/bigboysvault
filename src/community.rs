//! Trainers: sign-up and sign-in, the "My trainer" page, public profiles, vouches and replies, and the owner's controls.
use crate::{admin, chrome, ise, with_db, App, Chrome, Res};
use askama::Template;
use axum::{
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    Form, Json,
};
use marcuru_rs::members::{self, Added, Award, ChatMessage, Member, MemberCard, Posted, Vouch};
use marcuru_rs::market;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

const COOKIE: &str = "bbv_trainer";

// ---------- small helpers ----------
pub(crate) fn html(s: String) -> Response {
    ([(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))], Html(s)).into_response()
}
pub(crate) fn ok(v: Value) -> Response {
    ([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], v.to_string()).into_response()
}
pub(crate) fn fail(code: StatusCode, msg: &str) -> Response {
    (code, [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], json!({ "error": msg }).to_string()).into_response()
}
pub(crate) fn render<T: Template>(t: T) -> Result<String, (StatusCode, String)> {
    t.render().map_err(ise)
}

fn cookie_value(h: &HeaderMap) -> Option<String> {
    h.get(header::COOKIE)?.to_str().ok()?.split(';').find_map(|p| p.trim().strip_prefix(&format!("{COOKIE}=")).map(str::to_owned))
}

fn session_cookie(h: &HeaderMap, raw: &str, max_age: i64) -> String {
    let https = h.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https");
    format!("{COOKIE}={raw}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{}", if https { "; Secure" } else { "" })
}

/// Who is signed in, if anyone.
pub(crate) async fn current(app: &App, h: &HeaderMap) -> Result<Option<Member>, (StatusCode, String)> {
    let Some(raw) = cookie_value(h) else { return Ok(None) };
    with_db(app, move |c| members::from_session(c, &raw)).await
}

/// Forms and buttons must come from this site, not from another page trying to act for a signed-in visitor.
pub(crate) fn same_site(h: &HeaderMap) -> bool {
    let Some(origin) = h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else { return true };
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    origin.split_once("://").map(|(_, o)| o) == Some(host)
}

/// The visitor's address: Cloudflare's header when the site sits behind it, else Fly's.
pub(crate) fn client_ip(h: &HeaderMap) -> String {
    ["cf-connecting-ip", "fly-client-ip"]
        .iter()
        .find_map(|k| h.get(*k).and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty()))
        .unwrap_or("local")
        .to_string()
}

/// Counts attempts for a while. True means the limit has been reached.
pub(crate) fn over_limit(app: &App, key: String, max: u32) -> bool {
    let n = app.limits.get(&key).unwrap_or(0) + 1;
    app.limits.insert(key, n);
    n > max
}

fn known_picture(url: &str) -> bool {
    market::image_host_ok(url)
}

/// What the market says a card is worth, in pesos. 0 if it doesn't say.
fn market_php(info: &Value) -> f64 {
    info["finishes"].as_array().and_then(|f| f.first()).and_then(|f| f["php"].as_f64()).unwrap_or(0.0)
}

/// Looks up prices for trainers' cards a few at a time, politely. Runs in the background.
pub async fn refresh_values(app: &App) {
    let now = crate::unix_now();
    let Ok(ids) = with_db(app, move |c| members::cards_to_price(c, now - 5 * 86_400, 400)).await else { return };
    let mut changed = false;
    for id in ids {
        let info = market::by_id_lang(&app.http, &app.fx, &id).await.ok().flatten();
        let market = info.as_ref().map_or(0.0, market_php);
        let key = id.clone();
        let done = with_db(app, move |c| {
            let value = if market > 0.0 { market } else { members::vault_price(c, &key)? };
            members::set_card_value(c, &key, value, crate::unix_now())?;
            Ok(value)
        })
        .await;
        changed |= done.is_ok_and(|v| v > 0.0);
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    }
    if changed {
        app.forget();
    }
}

// ---------- pages ----------
#[derive(Template)]
#[template(path = "login.html")]
struct LoginPage {
    nav: Chrome,
    error: String,
    name: String,
}

#[derive(Template)]
#[template(path = "join.html")]
struct JoinPage {
    nav: Chrome,
    error: String,
    name: String,
    mode: &'static str,
}

#[derive(Template)]
#[template(path = "trainers.html")]
struct TrainersPage {
    nav: Chrome,
    trainers: Vec<Member>,
    me: Option<Member>,
}

#[derive(Template)]
#[template(path = "trainer.html")]
struct TrainerPage {
    nav: Chrome,
    t: Member,
    cards: Vec<MemberCard>,
    highlight: Option<MemberCard>,
    is_me: bool,
    full: bool,
    vouches: Vec<Vouch>,
    awards: Vec<Award>,
}

#[derive(Template)]
#[template(path = "trainer_part.html")]
struct TrainerPart {
    t: Member,
    cards: Vec<MemberCard>,
    highlight: Option<MemberCard>,
    is_me: bool,
    full: bool,
    vouches: Vec<Vouch>,
    awards: Vec<Award>,
}

#[derive(Template)]
#[template(path = "vouches.html")]
struct WallPage {
    nav: Chrome,
    vouches: Vec<Vouch>,
    next: Option<i64>,
    me: Option<Member>,
}

#[derive(Template)]
#[template(path = "vouches_part.html")]
struct WallPart {
    vouches: Vec<Vouch>,
    next: Option<i64>,
}

#[derive(Template)]
#[template(path = "vouch_one.html")]
struct VouchOne {
    v: Vouch,
}

#[derive(Template)]
#[template(path = "vouch_reply_one.html")]
struct ReplyOne {
    r: Vouch,
}

#[derive(Template)]
#[template(path = "member_card.html")]
struct MemberCardPage {
    nav: Chrome,
    t: Member,
    c: MemberCard,
}

#[derive(Template)]
#[template(path = "member_card_part.html")]
struct MemberCardPart {
    t: Member,
    c: MemberCard,
}

#[derive(Template)]
#[template(path = "me.html")]
struct MePage {
    nav: Chrome,
    m: Member,
    cards: Vec<MemberCard>,
    binders: Vec<members::MemberBinder>,
    stats: members::SellerStats,
    max: i64,
    conditions: [&'static str; 10],
}

#[derive(Template)]
#[template(path = "admin_trainers.html")]
struct AdminPage {
    nav: Chrome,
    tab: &'static str,
    rows: Vec<AdminRow>,
    feed: Vec<Vouch>,
    showing_vouches: bool,
    photo_mb: i64,
    photo_cap_mb: i64,
    hidden_vouches: usize,
    /// Can do everything, so sees the site's other tabs and can hand out roles.
    full: bool,
}

struct AdminRow {
    m: Member,
    highlight: Option<MemberCard>,
    profile_waiting: bool,
    awards: Vec<Award>,
    /// This viewer can act on this trainer. Nobody acts on themselves, and moderators don't act on other staff.
    can_act: bool,
    is_self: bool,
}

// ---------- sign up, sign in, sign out ----------
#[derive(Deserialize, Default)]
pub struct JoinQs {
    signin: Option<String>,
}

pub async fn join_page(State(app): State<App>, h: HeaderMap, Query(q): Query<JoinQs>) -> Res {
    if current(&app, &h).await?.is_some() {
        return Ok(Redirect::to("/me").into_response());
    }
    let nav = with_db(&app, chrome).await?;
    Ok(html(render(JoinPage { nav, error: String::new(), name: String::new(), mode: if q.signin.is_some() { "signin" } else { "join" } })?))
}

/// `GET /login`: the sign-in landing for trainers, with the vault owner's admin password tucked underneath.
pub async fn login_page(State(app): State<App>, h: HeaderMap, Query(q): Query<LoginQs>) -> Res {
    if current(&app, &h).await?.is_some() {
        return Ok(Redirect::to("/me").into_response());
    }
    if admin::authed(&app, &h) && !app.admin_token.is_empty() {
        return Ok(Redirect::to("/admin").into_response());
    }
    let nav = with_db(&app, chrome).await?;
    let error = if q.wrong.is_some() { "That password isn't right.".to_string() } else { String::new() };
    Ok(html(render(LoginPage { nav, error, name: String::new() })?))
}

#[derive(Deserialize)]
pub struct LoginQs {
    wrong: Option<String>,
}

async fn join_again(app: &App, code: StatusCode, mode: &'static str, name: String, error: &str) -> Res {
    let nav = with_db(app, chrome).await?;
    if mode == "signin" {
        return Ok((code, html(render(LoginPage { nav, error: error.to_string(), name })?)).into_response());
    }
    Ok((code, html(render(JoinPage { nav, error: error.to_string(), name, mode })?)).into_response())
}

#[derive(Deserialize)]
pub struct JoinForm {
    name: String,
    password: String,
    /// A field people never see. Only bots fill it in.
    #[serde(default)]
    website: String,
}

async fn signed_in(app: &App, h: &HeaderMap, id: i64) -> Res {
    let raw = with_db(app, move |c| members::new_session(c, id)).await?;
    Ok(([(header::SET_COOKIE, session_cookie(h, &raw, members::SESSION_SECS))], Redirect::to("/me")).into_response())
}

pub async fn join(State(app): State<App>, h: HeaderMap, Form(f): Form<JoinForm>) -> Res {
    if !same_site(&h) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }
    let typed = members::clean_line(&f.name, 40);
    if !f.website.is_empty() {
        return Ok(Redirect::to("/join").into_response());
    }
    // Typos don't count against anyone. New accounts are limited, and so is the number of tries overall.
    let ip = client_ip(&h);
    if app.limits.get(&format!("joined|{ip}")).unwrap_or(0) >= 5 || over_limit(&app, format!("join|{ip}"), 40) {
        return join_again(&app, StatusCode::TOO_MANY_REQUESTS, "join", typed, "Too many new trainers from here. Please try again in a little while.").await;
    }
    let name = match members::valid_name(&f.name) {
        Ok(n) => n,
        Err(e) => return join_again(&app, StatusCode::BAD_REQUEST, "join", typed, e).await,
    };
    if let Err(e) = members::valid_password(&f.password) {
        return join_again(&app, StatusCode::BAD_REQUEST, "join", typed, e).await;
    }
    if with_db(&app, members::unapproved_count).await? >= members::MAX_PENDING {
        return join_again(&app, StatusCode::SERVICE_UNAVAILABLE, "join", typed, "New sign-ups are paused for a moment. Please try again later.").await;
    }
    let pass = f.password;
    let hash = tokio::task::spawn_blocking(move || members::hash_password(&pass)).await.map_err(ise)?.map_err(ise)?;
    let n2 = name.clone();
    let Some(id) = with_db(&app, move |c| members::create(c, &n2, &hash)).await? else {
        return join_again(&app, StatusCode::CONFLICT, "join", typed, "That trainer name is taken. Please pick another.").await;
    };
    over_limit(&app, format!("joined|{ip}"), u32::MAX);
    signed_in(&app, &h, id).await
}

pub async fn signin(State(app): State<App>, h: HeaderMap, Form(f): Form<JoinForm>) -> Res {
    if !same_site(&h) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }
    let typed = members::clean_line(&f.name, 40);
    let by_ip = over_limit(&app, format!("in|{}", client_ip(&h)), 20);
    let by_name = over_limit(&app, format!("in|{}", members::name_key(&typed)), 8);
    if by_ip || by_name {
        return join_again(&app, StatusCode::TOO_MANY_REQUESTS, "signin", typed, "Too many tries. Please wait 15 minutes and try again.").await;
    }
    // An admin trainer signs in with "name/secret": that opens the admin and their own trainer at once.
    if admin::admin_password_ok(&app, &typed, &f.password) {
        let n3 = typed.clone();
        let who = with_db(&app, move |c| members::for_signin(c, &n3)).await?;
        let Some((id, _)) = who else { return join_again(&app, StatusCode::UNAUTHORIZED, "signin", typed, "There's no trainer with that name. Make one first, then an admin can make it staff.").await };
        let is_admin = with_db(&app, move |c| members::get(c, id)).await?.is_some_and(|m| m.role == "admin");
        if !is_admin {
            return join_again(&app, StatusCode::UNAUTHORIZED, "signin", typed, "That trainer isn't an admin.").await;
        }
        let raw = with_db(&app, move |c| members::new_session(c, id)).await?;
        // Two cookies: AppendHeaders keeps both (a plain array would keep only the last Set-Cookie).
        return Ok((axum::response::AppendHeaders([(header::SET_COOKIE, session_cookie(&h, &raw, members::SESSION_SECS)), (header::SET_COOKIE, admin::admin_cookie_text(&app, &h))]), Redirect::to("/admin")).into_response());
    }
    let n2 = typed.clone();
    let found = with_db(&app, move |c| members::for_signin(c, &n2)).await?;
    let pass = f.password;
    let good = tokio::task::spawn_blocking(move || match &found {
        Some((id, stored)) => members::verify_password(&pass, stored).then_some(*id),
        None => {
            members::waste_time(&pass);
            None
        }
    })
    .await
    .map_err(ise)?;
    match good {
        Some(id) if with_db(&app, move |c| members::is_blocked(c, id)).await? => {
            join_again(&app, StatusCode::FORBIDDEN, "signin", typed, "This trainer has been blocked by the vault owner.").await
        }
        Some(id) => signed_in(&app, &h, id).await,
        None => join_again(&app, StatusCode::UNAUTHORIZED, "signin", typed, "That name and password don't match.").await,
    }
}

pub async fn signout(State(app): State<App>, h: HeaderMap) -> Res {
    if !same_site(&h) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }
    if let Some(raw) = cookie_value(&h) {
        with_db(&app, move |c| members::end_session(c, &raw)).await?;
    }
    // Both cookies go: an admin signed in with name/secret holds the owner's too.
    Ok((axum::response::AppendHeaders([(header::SET_COOKIE, session_cookie(&h, "", 0)), (header::SET_COOKIE, "marcuru_admin=; Path=/; Max-Age=0".to_string())]), Redirect::to("/")).into_response())
}

// ---------- public pages ----------
pub async fn trainers(State(app): State<App>, h: HeaderMap) -> Res {
    let me = current(&app, &h).await?;
    let (mut nav, list) = with_db(&app, |c| Ok((chrome(c)?, members::approved(c)?))).await?;
    let site = nav.site.clone();
    let people: Vec<serde_json::Value> = list.iter().filter(|t| !t.blocked).take(50).enumerate().map(|(i, t)| json!({ "@type": "ListItem", "position": i + 1, "url": format!("{site}/trainers/{}", t.id), "name": t.name })).collect();
    nav.seo = crate::Seo {
        title: format!("Trainers · {}", nav.brand),
        desc: format!("Meet the {} trainers of {}: their Pokémon card binders, star points and vouches from other collectors.", list.len(), nav.brand),
        path: "/trainers".into(),
        kind: "website",
        ld: crate::ld(json!({ "@context": "https://schema.org", "@type": "CollectionPage", "url": format!("{site}/trainers"), "name": "Trainers", "mainEntity": { "@type": "ItemList", "itemListElement": people } })),
        ..Default::default()
    };
    Ok(html(render(TrainersPage { nav, trainers: list, me })?))
}

pub async fn trainer_by_slug(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>, Query(q): Query<PartQs>) -> Res {
    let s = slug.clone();
    let member = with_db(&app, move |c| members::by_slug(c, &s)).await?;
    if let Some(m) = member {
        return trainer(State(app), h, Path(m.id), Query(q)).await;
    }
    if let Ok(id) = slug.parse::<i64>() {
        return trainer(State(app), h, Path(id), Query(q)).await;
    }
    not_found(&app).await
}

pub async fn seller_binder(State(app): State<App>, h: HeaderMap, Path((slug, b_slug)): Path<(String, String)>, Query(q): Query<PartQs>) -> Res {
    let s = slug.clone();
    let member = with_db(&app, move |c| members::by_slug(c, &s)).await?;
    let Some(m) = member else {
        if let Ok(id) = slug.parse::<i64>() {
            return trainer(State(app), h, Path(id), Query(q)).await;
        }
        return not_found(&app).await;
    };
    let mid = m.id;
    let bs = b_slug.clone();
    let binder = with_db(&app, move |c| members::binder_by_slug(c, mid, &bs)).await?;
    let Some(b) = binder else { return not_found(&app).await };
    let bid = b.id;
    let me = current(&app, &h).await?;
    let is_me = me.as_ref().is_some_and(|u| u.id == m.id);
    let owner = admin::can_moderate(&app, &h);
    // Same rules as the trainer's own page, plus: a private binder is for its owner (and the vault's staff) only.
    if (m.blocked || b.visibility.eq_ignore_ascii_case("private")) && !is_me && !owner {
        return not_found(&app).await;
    }
    let full = m.profile_ok || is_me || owner;
    if !full {
        return not_found(&app).await;
    }
    let (nav, cards, vouches, awards) = with_db(&app, move |c| {
        Ok((chrome(c)?, members::cards_of_binder(c, mid, Some(bid))?, members::public_vouches(c, Some(mid), None, 20)?, members::awards_of(c, mid, 10)?))
    }).await?;
    let awards: Vec<Award> = awards.into_iter().filter(|a| !a.reason.is_empty()).collect();
    let highlight = m.highlight.and_then(|h| cards.iter().find(|c| c.id == h).cloned());
    Ok(html(if q.part.is_some() {
        render(TrainerPart { t: m, cards, highlight, is_me, full, vouches, awards })?
    } else {
        render(TrainerPage { nav, t: m, cards, highlight, is_me, full, vouches, awards })?
    }))
}

pub(crate) async fn not_found(app: &App) -> Res {
    Ok(crate::not_found(State(app.clone())).await)
}

pub async fn trainer(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Query(q): Query<PartQs>) -> Res {
    let me = current(&app, &h).await?;
    let (nav, t, cards, vouches, awards) = with_db(&app, move |c| {
        Ok((chrome(c)?, members::get(c, id)?, members::cards_of(c, id)?, members::public_vouches(c, Some(id), None, 20)?, members::awards_of(c, id, 10)?))
    })
    .await?;
    let Some(mut t) = t else { return not_found(&app).await };
    // The page lists what the stars were for. Ones given without a reason just count towards the total.
    let awards: Vec<Award> = awards.into_iter().filter(|a| !a.reason.is_empty()).collect();
    let is_me = me.as_ref().is_some_and(|m| m.id == t.id);
    let owner = admin::can_moderate(&app, &h);
    if t.blocked && !owner {
        return not_found(&app).await;
    }
    // Not approved yet: the public sees the name and vouches. The picture, description and binder wait for the owner.
    let full = t.profile_ok || is_me || owner;
    if !full {
        t.avatar.clear();
        t.bio.clear();
    }
    let cards = if full { cards } else { vec![] };
    let highlight = t.highlight.and_then(|h| cards.iter().find(|c| c.id == h).cloned());
    let mut nav = nav;
    // An approved trainer's page is worth finding: a profile with their picture and binder.
    if t.profile_ok && !t.blocked {
        let site = nav.site.clone();
        let url = format!("{site}/trainers/{}", t.id);
        let about = if t.bio.trim().is_empty() { String::new() } else { format!(" {}", crate::clip(&t.bio, 90)) };
        let mut person = json!({ "@type": "Person", "name": t.name, "url": url });
        if t.has_avatar() {
            person["image"] = json!(nav.abs(&t.avatar));
        }
        if !t.bio.trim().is_empty() {
            person["description"] = json!(t.bio.trim());
        }
        nav.seo = crate::Seo {
            title: format!("{}'s Pokémon card binder · {}", t.name, nav.brand),
            desc: crate::clip(&format!("{}'s binder on {}: {} card{}.{about}", t.name, nav.brand, t.cards, if t.cards == 1 { "" } else { "s" }), 165),
            path: format!("/trainers/{}", t.id),
            image: if t.has_avatar() { t.avatar.clone() } else { highlight.as_ref().map(|c| c.big().to_string()).unwrap_or_default() },
            kind: "profile",
            ld: crate::ld(json!({ "@context": "https://schema.org", "@type": "ProfilePage", "url": url, "mainEntity": person })),
        };
    }
    Ok(html(if q.part.is_some() {
        render(TrainerPart { t, cards, highlight, is_me, full, vouches, awards })?
    } else {
        render(TrainerPage { nav, t, cards, highlight, is_me, full, vouches, awards })?
    }))
}

#[derive(Deserialize, Default)]
pub struct PartQs {
    part: Option<String>,
}

pub async fn member_card(State(app): State<App>, h: HeaderMap, Path((id, cid)): Path<(i64, i64)>, Query(q): Query<PartQs>) -> Res {
    let me = current(&app, &h).await?;
    let (nav, t, card) = with_db(&app, move |c| Ok((chrome(c)?, members::get(c, id)?, members::card(c, id, cid)?))).await?;
    let (Some(t), Some(c)) = (t, card) else { return not_found(&app).await };
    if (!t.profile_ok || t.blocked) && me.as_ref().is_none_or(|m| m.id != t.id) && !admin::can_moderate(&app, &h) {
        return not_found(&app).await;
    }
    // A look at a card in a store counts for its seller (once a day per viewer, never the owner).
    let (cid2, who, viewer) = (c.id, crate::store::viewer_hash(&h), me.as_ref().map(|m| m.id));
    with_db(&app, move |c| marcuru_rs::sellers::count_view(c, cid2, &who, viewer)).await.ok();
    Ok(html(if q.part.is_some() { render(MemberCardPart { t, c })? } else { render(MemberCardPage { nav, t, c })? }))
}

// ---------- my trainer ----------
pub async fn me(State(app): State<App>, h: HeaderMap) -> Res {
    let Some(m) = current(&app, &h).await? else { return Ok(Redirect::to("/join").into_response()) };
    let id = m.id;
    let (nav, cards, binders, stats) = with_db(&app, move |c| Ok((chrome(c)?, members::cards_of(c, id)?, members::binders_of(c, id)?, members::seller_stats(c, id)?))).await?;
    Ok(html(render(MePage { nav, m, cards, binders, stats, max: members::MAX_CARDS, conditions: members::CONDITIONS })?))
}

/// Signed in and really from this site, whatever their lock says. For things a locked trainer may still do: their password, deleting what's theirs.
pub(crate) async fn acting_any(app: &App, h: &HeaderMap) -> Result<Result<Member, Response>, (StatusCode, String)> {
    if !same_site(h) {
        return Ok(Err(fail(StatusCode::FORBIDDEN, "That request didn't come from this site.")));
    }
    Ok(current(app, h).await?.ok_or_else(|| fail(StatusCode::UNAUTHORIZED, "Please sign in again.")))
}

/// Every action on "My trainer" and every post starts here: signed in, really from this site, and not locked.
pub(crate) async fn acting(app: &App, h: &HeaderMap) -> Result<Result<Member, Response>, (StatusCode, String)> {
    Ok(match acting_any(app, h).await? {
        Ok(m) if m.locked() => Err(fail(StatusCode::FORBIDDEN, &format!("Your trainer is locked for another {}. You can't post or change anything until then.", m.lock_left()))),
        other => other,
    })
}

/// A profile picture: square, sharp enough to enlarge, and re-made from the pixels, so nothing hidden in the upload survives.
fn store_avatar(bytes: Vec<u8>, dir: PathBuf, stem: String) -> Result<String, String> {
    use image::{codecs::jpeg::JpegEncoder, imageops::FilterType};
    let img = admin::open_image(bytes)?;
    let side = img.width().min(img.height());
    if side < 16 {
        return Err("too small".into());
    }
    let square = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side).resize_exact(side.clamp(400, 640), side.clamp(400, 640), FilterType::Lanczos3);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = format!("{stem}.jpg");
    let file = std::fs::File::create(dir.join(&name)).map_err(|e| e.to_string())?;
    let mut w = std::io::BufWriter::new(file);
    JpegEncoder::new_with_quality(&mut w, 86).encode_image(&square.to_rgb8()).map_err(|e| e.to_string())?;
    Ok(format!("/uploads/{name}"))
}

/// One photo for a vouch: a full size and a small one, both re-made from the pixels. The server is small, so very large pictures are refused before they're opened.
/// Returns (full, small, width, height, bytes on disk).
pub(crate) fn store_photo(bytes: Vec<u8>, dir: PathBuf, stem: String) -> Result<(String, String, i64, i64, i64), String> {
    use image::{codecs::jpeg::JpegEncoder, imageops::FilterType, DynamicImage, ImageReader};
    let (w, h) = ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().map_err(|e| e.to_string())?.into_dimensions().map_err(|e| e.to_string())?;
    if u64::from(w) * u64::from(h) > 24_000_000 {
        return Err("too large".into());
    }
    let img = admin::open_image(bytes)?;
    let big = if img.width().max(img.height()) > 1600 { img.resize(1600, 1600, FilterType::Triangle) } else { img };
    let small = if big.width().max(big.height()) > 480 { big.resize(480, 480, FilterType::Triangle) } else { big.clone() };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let write = |pic: &DynamicImage, name: &str, quality: u8| -> Result<i64, String> {
        let file = std::fs::File::create(dir.join(name)).map_err(|e| e.to_string())?;
        let mut out = std::io::BufWriter::new(file);
        JpegEncoder::new_with_quality(&mut out, quality).encode_image(&pic.to_rgb8()).map_err(|e| e.to_string())?;
        drop(out);
        Ok(std::fs::metadata(dir.join(name)).map(|m| m.len() as i64).unwrap_or(0))
    };
    let (l, s) = (format!("{stem}_l.jpg"), format!("{stem}_s.jpg"));
    let size = write(&big, &l, 82)? + write(&small, &s, 78)?;
    Ok((format!("/uploads/{l}"), format!("/uploads/{s}"), i64::from(big.width()), i64::from(big.height()), size))
}

/// Room for vouch photos, so they can never fill the disk the whole site lives on. Raise it after growing the volume.
pub(crate) fn photo_cap_mb() -> i64 {
    std::env::var("PHOTO_SPACE_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(650)
}

pub(crate) static PHOTO_GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

pub async fn save_profile(State(app): State<App>, h: HeaderMap, mut mp: Multipart) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if over_limit(&app, format!("profile|{}", m.id), 30) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "Too many saves. Please wait a few minutes."));
    }
    let (mut name, mut bio, mut photo) = (m.name.clone(), m.bio.clone(), None::<Vec<u8>>);
    let (mut fb_url, mut tiktok_url, mut links) = (m.fb_url.clone(), m.tiktok_url.clone(), m.links.clone());
    while let Some(f) = mp.next_field().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))? {
        match f.name().unwrap_or_default() {
            "avatar" => {
                let b = f.bytes().await.map_err(|_| (StatusCode::BAD_REQUEST, "That picture didn't upload.".to_string()))?;
                if !b.is_empty() {
                    photo = Some(b.to_vec());
                }
            }
            "name" => name = f.text().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?,
            "bio" => bio = f.text().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?,
            "fb_url" => fb_url = members::clean_https_url(&f.text().await.unwrap_or_default()),
            "tiktok_url" => tiktok_url = members::clean_https_url(&f.text().await.unwrap_or_default()),
            "links" => links = members::clean_link_lines(&f.text().await.unwrap_or_default()),
            _ => {}
        }
    }
    let name = match members::valid_name(&name) {
        Ok(n) => n,
        Err(e) => return Ok(fail(StatusCode::BAD_REQUEST, e)),
    };
    let bio = members::clean_text(&bio, 300);
    let avatar = match photo {
        Some(bytes) => {
            let dir = PathBuf::from(&*app.uploads);
            let stem = format!("t{}-{}", m.id, members::temp_password());
            match tokio::task::spawn_blocking(move || store_avatar(bytes, dir, stem)).await.map_err(ise)? {
                Ok(p) => Some(p),
                Err(_) => return Ok(fail(StatusCode::BAD_REQUEST, "That picture couldn't be read. Try a JPEG or PNG.")),
            }
        }
        None => None,
    };
    let (m2, n2, b2, a2) = (m.clone(), name.clone(), bio.clone(), avatar.clone());
    let (fb, tt, lk) = (fb_url.clone(), tiktok_url.clone(), links.clone());
    let saved = with_db(&app, move |c| members::save_profile_seller(c, &m2, &n2, &b2, a2.as_deref(), &fb, &tt, &lk)).await?;
    if !saved {
        if let Some(a) = &avatar {
            admin::remove_upload(&app.uploads, a);
        }
        return Ok(fail(StatusCode::CONFLICT, "That trainer name is taken. Please pick another."));
    }
    if avatar.is_some() && m.has_avatar() {
        admin::remove_upload(&app.uploads, &m.avatar);
    }
    app.forget();
    let id = m.id;
    let fresh = with_db(&app, move |c| members::get(c, id)).await?;
    Ok(ok(json!({ "ok": true, "name": name, "avatar": avatar, "approved": fresh.is_some_and(|f| f.profile_ok) })))
}

// ---------- vouches and replies ----------
const WALL_PAGE: i64 = 24;

#[derive(Deserialize, Default)]
pub struct WallQs {
    part: Option<String>,
    before: Option<i64>,
}

pub async fn wall(State(app): State<App>, h: HeaderMap, Query(q): Query<WallQs>) -> Res {
    let me = current(&app, &h).await?;
    let before = q.before;
    let (nav, mut vouches) = with_db(&app, move |c| Ok((chrome(c)?, members::public_vouches(c, None, before, WALL_PAGE + 1)?))).await?;
    let more = vouches.len() as i64 > WALL_PAGE;
    vouches.truncate(WALL_PAGE as usize);
    let next = if more { vouches.last().map(|v| v.id) } else { None };
    let mut nav = nav;
    if before.is_none() {
        nav.seo = crate::Seo {
            title: format!("Vouches · {}", nav.brand),
            desc: format!("What collectors say about {}: real vouches and photos from trainers who traded, bought and visited the vault.", nav.brand),
            path: "/vouches".into(),
            kind: "website",
            ..Default::default()
        };
    }
    Ok(html(if q.part.is_some() { render(WallPart { vouches, next })? } else { render(WallPage { nav, vouches, next, me })? }))
}

/// One vouch or reply, ready to drop into the page just after posting it.
pub async fn vouch_one(State(app): State<App>, Path(id): Path<i64>) -> Res {
    let Some(v) = with_db(&app, move |c| members::public_vouch(c, id)).await? else { return not_found(&app).await };
    Ok(html(if v.parent_id.is_some() { render(ReplyOne { r: v })? } else { render(VouchOne { v })? }))
}

/// Who's looking. Pages are shared and saved, so the buttons that depend on the visitor are added in the browser.
pub async fn who(State(app): State<App>, h: HeaderMap) -> Res {
    let m = current(&app, &h).await?;
    Ok(ok(json!({
        "admin": admin::can_moderate(&app, &h),
        "full": admin::authed(&app, &h),
        "member": m.map(|m| json!({ "id": m.id, "name": m.name, "locked": m.locked(), "lock_left": m.lock_left(), "avatar": m.avatar, "initial": m.initial(), "seller": m.seller, "role": m.role })),
    })))
}

#[derive(Deserialize)]
pub struct PostReq {
    #[serde(default)]
    text: String,
    parent: Option<i64>,
    #[serde(default)]
    photos: Vec<i64>,
}

pub async fn post_vouch(State(app): State<App>, h: HeaderMap, Json(req): Json<PostReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let (id, replying) = (m.id, req.parent.is_some());
    let used = with_db(&app, move |c| members::posts_last_hour(c, id, replying)).await?;
    if used >= if replying { members::REPLIES_PER_HOUR } else { members::VOUCHES_PER_HOUR } {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "You've posted a lot this hour. Please take a short break and try again."));
    }
    let text = members::clean_text(&req.text, members::TEXT_MAX);
    let (parent, photos) = (req.parent, req.photos);
    match with_db(&app, move |c| members::post_vouch(c, id, parent, &text, &photos)).await? {
        Posted::Yes(vid) => {
            app.forget();
            Ok(ok(json!({ "ok": true, "id": vid })))
        }
        Posted::Empty => Ok(fail(StatusCode::BAD_REQUEST, "Write something or add a photo first.")),
        Posted::NoParent => Ok(fail(StatusCode::NOT_FOUND, "That vouch isn't there any more.")),
    }
}

/// A photo goes up as soon as it's picked and waits for its vouch to be posted.
pub async fn upload_photo(State(app): State<App>, h: HeaderMap, mut mp: Multipart) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let (used, room, old) = with_db(&app, move |c| Ok((members::photos_last_hour(c, id)?, members::photo_bytes(c)?, members::sweep_waiting_photos(c)?))).await?;
    for f in &old {
        admin::remove_upload(&app.uploads, f);
    }
    if used >= members::PHOTOS_PER_HOUR {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of photos for one hour. Please take a short break and try again."));
    }
    if room >= photo_cap_mb() * 1024 * 1024 {
        return Ok(fail(StatusCode::INSUFFICIENT_STORAGE, "The vault's photo space is full right now. The owner has to make more room. Your words can still be posted."));
    }
    let mut bytes = None;
    while let Some(f) = mp.next_field().await.map_err(|_| (StatusCode::BAD_REQUEST, "That photo didn't upload.".to_string()))? {
        if f.name() == Some("photo") {
            bytes = Some(f.bytes().await.map_err(|_| (StatusCode::BAD_REQUEST, "That photo didn't upload.".to_string()))?.to_vec());
        }
    }
    let Some(bytes) = bytes.filter(|b| !b.is_empty()) else { return Ok(fail(StatusCode::BAD_REQUEST, "That photo didn't upload.")) };
    let gate = PHOTO_GATE.acquire().await.map_err(ise)?;
    let dir = PathBuf::from(&*app.uploads);
    let stem = format!("v{id}-{}", members::temp_password());
    let made = tokio::task::spawn_blocking(move || store_photo(bytes, dir, stem)).await.map_err(ise)?;
    drop(gate);
    let (path, thumb, w, h2, size) = match made {
        Ok(x) => x,
        Err(_) => return Ok(fail(StatusCode::BAD_REQUEST, "That photo couldn't be read. Try a JPEG or PNG.")),
    };
    let (p2, t2) = (path.clone(), thumb.clone());
    let pid = with_db(&app, move |c| members::add_photo(c, id, &p2, &t2, w, h2, size)).await?;
    Ok(ok(json!({ "ok": true, "id": pid, "thumb": thumb })))
}

/// Taken out of the vouch being written.
pub async fn drop_photo(State(app): State<App>, h: HeaderMap, Path(pid): Path<i64>) -> Res {
    let m = match acting_any(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    if let Some(f) = with_db(&app, move |c| members::drop_waiting_photo(c, id, pid)).await? {
        admin::remove_upload(&app.uploads, &f);
    }
    Ok(ok(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct VouchAct {
    /// "delete" (its writer or the owner), "hide" or "show" (the owner)
    action: String,
}

pub async fn vouch_act(State(app): State<App>, h: HeaderMap, Path(vid): Path<i64>, Json(req): Json<VouchAct>) -> Res {
    if !same_site(&h) {
        return Ok(fail(StatusCode::FORBIDDEN, "That request didn't come from this site."));
    }
    let power = admin::power(&app, &h);
    let owner = power != admin::Power::Nothing;
    // A moderator doesn't overrule other staff.
    if let admin::Power::Moderator(me) = power {
        let author = with_db(&app, move |c| members::vouch_author(c, vid)).await?;
        if author.is_some_and(|(id, role)| id != me && !role.is_empty()) {
            return Ok(fail(StatusCode::FORBIDDEN, "That's another staff member's post, so only an admin can change it."));
        }
    }
    match req.action.as_str() {
        "hide" | "show" if owner => {
            let hide = req.action == "hide";
            with_db(&app, move |c| members::set_vouch_hidden(c, vid, hide)).await?;
        }
        "delete" => {
            if !owner {
                let Some(m) = current(&app, &h).await? else { return Ok(fail(StatusCode::UNAUTHORIZED, "Please sign in again.")) };
                if with_db(&app, move |c| members::vouch_owner(c, vid)).await? != Some(m.id) {
                    return Ok(fail(StatusCode::FORBIDDEN, "That isn't yours to delete."));
                }
            }
            for f in with_db(&app, move |c| members::delete_vouch(c, vid)).await? {
                admin::remove_upload(&app.uploads, &f);
            }
        }
        _ => return Ok(fail(StatusCode::FORBIDDEN, "Only the vault owner can do that.")),
    }
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

/// The owner takes a single photo off a vouch.
pub async fn admin_photo_delete(State(app): State<App>, h: HeaderMap, Path(pid): Path<i64>) -> Res {
    let power = admin::power(&app, &h);
    if power == admin::Power::Nothing || !same_site(&h) {
        return Ok(fail(StatusCode::UNAUTHORIZED, "Your session expired. Sign in again."));
    }
    if let admin::Power::Moderator(me) = power {
        let author = with_db(&app, move |c| members::photo_author(c, pid)).await?;
        if author.is_some_and(|(id, role)| id != me && !role.is_empty()) {
            return Ok(fail(StatusCode::FORBIDDEN, "That's another staff member's photo, so only an admin can change it."));
        }
    }
    if let Some(f) = with_db(&app, move |c| members::delete_photo(c, pid)).await? {
        admin::remove_upload(&app.uploads, &f);
    }
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct SearchQs {
    #[serde(default)]
    q: String,
    #[serde(default)]
    lang: String,
}

/// The same card catalogue the owner uses, so everyone picks from real card pictures.
pub async fn search_cards(State(app): State<App>, h: HeaderMap, Query(q): Query<SearchQs>) -> Res {
    let Some(m) = current(&app, &h).await? else { return Ok(fail(StatusCode::UNAUTHORIZED, "Please sign in again.")) };
    if over_limit(&app, format!("search|{}", m.id), 120) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of searching. Please wait a few minutes."));
    }
    let typed: String = q.q.chars().take(60).collect();
    Ok(ok(crate::ops::catalog_cached(&app, &typed, &q.lang).await))
}

#[derive(Deserialize)]
pub struct AddReq {
    id: String,
    /// Pictures from the search result, used only when the catalogue has none and only from known card-picture sites.
    image: Option<String>,
    image_large: Option<String>,
    en: Option<String>,
}

pub async fn add_card(State(app): State<App>, h: HeaderMap, Json(req): Json<AddReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if m.cards >= with_db(&app, |c| Ok(marcuru_rs::sellers::max_cards(c))).await? {
        return Ok(fail(StatusCode::CONFLICT, "Your store is full. Take a card out first."));
    }
    // The details come from the catalogue, not from what the browser says.
    let info = match market::by_id_lang(&app.http, &app.fx, &req.id).await {
        Ok(Some(v)) => v,
        _ => return Ok(fail(StatusCode::BAD_REQUEST, "That card couldn't be found. Please try again.")),
    };
    let text = |k: &str| info[k].as_str().unwrap_or("").to_string();
    let name = text("name");
    if name.is_empty() {
        return Ok(fail(StatusCode::BAD_REQUEST, "That card couldn't be found. Please try again."));
    }
    let en = req.en.as_deref().map(|e| members::clean_line(e, 60)).filter(|e| !e.is_empty() && *e != name);
    let title = members::clean_line(&en.map(|e| format!("{name} ({e})")).unwrap_or_else(|| name.clone()), 120);
    let number = match (info["number"].as_str(), info["total"].as_i64()) {
        (Some(n), Some(t)) if t > 0 => format!("{n}/{t}"),
        (Some(n), _) => n.to_string(),
        _ => String::new(),
    };
    let pick = |own: String, given: Option<String>| if known_picture(&own) { own } else { given.filter(|g| known_picture(g)).unwrap_or_default() };
    let image = pick(text("image"), req.image);
    let image_large = pick(text("image_large"), req.image_large);
    let (id, card_id, set) = (m.id, req.id.chars().take(60).collect::<String>(), members::clean_line(&text("set"), 80));
    let (t2, n2, s2, i2, l2) = (title.clone(), number.clone(), set.clone(), image.clone(), image_large.clone());
    // A store may list the same card more than once (say Near Mint and Lightly Played): add_listing numbers the copies.
    let added = with_db(&app, move |c| marcuru_rs::sellers::add_listing(c, id, &card_id, &t2, &n2, &s2, &i2, &l2)).await?;
    match added {
        Added::Full => Ok(fail(StatusCode::CONFLICT, "Your binder is full. Take a card out first.")),
        Added::Duplicate => Ok(fail(StatusCode::CONFLICT, "That card is already in your store.")),
        Added::Yes(cid) => {
            let (market, key) = (market_php(&info), req.id.chars().take(60).collect::<String>());
            let value = with_db(&app, move |c| {
                let value = if market > 0.0 { market } else { members::vault_price(c, &key)? };
                members::set_card_value(c, &key, value, crate::unix_now())?;
                Ok(value)
            })
            .await
            .unwrap_or(0.0);
            if m.profile_ok {
                app.forget();
            }
            let (worth, card) = with_db(&app, move |c| Ok((members::get(c, id)?.map_or(0.0, |f| f.worth), members::card(c, id, cid)?))).await?;
            let mut j = json!({ "id": cid, "title": title, "number": number, "set": set, "image": image, "cond": "Near Mint", "value": vault_search::peso(value * members::condition_factor("Near Mint")), "market": value, "highlight": m.highlight.is_none() });
            if let Some(c) = card {
                let is_graded = c.is_graded();
                let is_sealed = c.is_sealed();
                let suggested = c.suggested_price();
                let share = c.condition_share();
                j["is_graded"] = json!(is_graded);
                j["is_sealed"] = json!(is_sealed);
                j["suggested"] = json!(suggested);
                j["share"] = json!(share);
                j["value_own"] = json!(c.value_own);
            }
            Ok(ok(json!({ "ok": true, "worth": vault_search::peso(worth), "card": j })))
        }
    }
}

#[derive(Deserialize)]
pub struct CardAction {
    /// "remove", "highlight", "condition", "value" or "binder"
    action: String,
    cond: Option<String>,
    value: Option<f64>,
    binder_id: Option<i64>,
}

pub async fn card_action(State(app): State<App>, h: HeaderMap, Path(cid): Path<i64>, Json(req): Json<CardAction>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let cond = req.cond.filter(|c| members::CONDITIONS.contains(&c.as_str()));
    match req.action.as_str() {
        "remove" => {
            let gone = with_db(&app, move |c| { let k = members::card(c, id, cid)?; members::remove_card(c, id, cid)?; Ok(k) }).await?;
            if let Some(k) = gone.filter(MemberCard::own_photo) {
                admin::remove_upload(&app.uploads, &k.image);
                admin::remove_upload(&app.uploads, &k.image_large);
            }
        }
        "highlight" => {
            if !with_db(&app, move |c| members::set_highlight(c, id, cid)).await? {
                return Ok(fail(StatusCode::NOT_FOUND, "That card isn't in your binder."));
            }
        }
        "condition" => {
            let Some(cond) = cond else { return Ok(fail(StatusCode::BAD_REQUEST, "Please pick a condition.")) };
            with_db(&app, move |c| members::set_condition(c, id, cid, &cond)).await?
        }
        "value" => {
            let value = req.value.unwrap_or(0.0);
            if !(0.0..=100_000_000.0).contains(&value) {
                return Ok(fail(StatusCode::BAD_REQUEST, "Please type a value in pesos."));
            }
            with_db(&app, move |c| members::set_own_value(c, id, cid, value.round())).await?;
        }
        "binder" => {
            let bid = req.binder_id;
            let ok = with_db(&app, move |c| members::move_card_to_binder(c, id, cid, bid)).await?;
            if !ok {
                return Ok(fail(StatusCode::NOT_FOUND, "Could not move card to that binder."));
            }
        }
        _ => return Ok(fail(StatusCode::BAD_REQUEST, "Unknown action.")),
    }
    if m.profile_ok {
        app.forget();
    }
    let me = with_db(&app, move |c| members::get(c, id)).await?;
    let card = with_db(&app, move |c| members::card(c, id, cid)).await?;
    let mut out = json!({ "ok": true, "highlight": me.as_ref().and_then(|f| f.highlight), "worth": vault_search::peso(me.as_ref().map_or(0.0, |f| f.worth)), "card_worth": card.as_ref().map(members::MemberCard::worth_text), "card_market": card.as_ref().map(members::MemberCard::market_text) });
    if let Some(c) = card {
        out["is_graded"] = json!(c.is_graded());
        out["is_sealed"] = json!(c.is_sealed());
        out["suggested"] = json!(c.suggested_price());
        out["share"] = json!(c.condition_share());
        out["value_own"] = json!(c.value_own);
        out["market"] = json!(c.value);
        out["cond"] = json!(c.cond);
        out["own_photo"] = json!(c.own_photo());
        out["binder_id"] = json!(c.binder_id);
    }
    Ok(ok(out))
}

#[derive(Deserialize)]
pub struct BinderReq {
    title: Option<String>,
    card_id: Option<i64>,
}

pub async fn me_create_binder(State(app): State<App>, h: HeaderMap, Json(req): Json<BinderReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let title = req.title.unwrap_or_default();
    let binder = with_db(&app, move |c| members::create_member_binder(c, id, &title)).await?;
    app.forget();
    Ok(ok(json!({ "ok": true, "binder": binder })))
}

pub async fn me_rename_binder(State(app): State<App>, h: HeaderMap, Path(bid): Path<i64>, Json(req): Json<BinderReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let title = req.title.unwrap_or_default();
    let ok_flag = with_db(&app, move |c| members::rename_member_binder(c, id, bid, &title)).await?;
    if !ok_flag {
        return Ok(fail(StatusCode::NOT_FOUND, "Binder not found."));
    }
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

pub async fn me_delete_binder(State(app): State<App>, h: HeaderMap, Path(bid): Path<i64>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let ok_flag = with_db(&app, move |c| members::delete_member_binder(c, id, bid)).await?;
    if !ok_flag {
        return Ok(fail(StatusCode::NOT_FOUND, "Binder not found."));
    }
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

pub async fn me_binder_cover(State(app): State<App>, h: HeaderMap, Path(bid): Path<i64>, Json(req): Json<BinderReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let ok_flag = with_db(&app, move |c| members::set_member_binder_cover(c, id, bid, req.card_id)).await?;
    if !ok_flag {
        return Ok(fail(StatusCode::NOT_FOUND, "Could not set binder cover."));
    }
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct NotificationQuery {
    unread_only: Option<bool>,
}

pub async fn get_notifications(State(app): State<App>, h: HeaderMap, Query(q): Query<NotificationQuery>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let unread_only = q.unread_only.unwrap_or(false);
    let id = m.id;
    let notifs = with_db(&app, move |c| members::notifications(c, id, unread_only)).await?;
    let unread_count = with_db(&app, move |c| members::unread_notification_count(c, id)).await?;
    Ok(ok(json!({ "ok": true, "notifications": notifs, "unread_count": unread_count })))
}

#[derive(Deserialize)]
pub struct ReadNotificationReq {
    before_id: Option<i64>,
}

pub async fn read_notifications(State(app): State<App>, h: HeaderMap, Json(req): Json<ReadNotificationReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    with_db(&app, move |c| {
        members::mark_notifications_read(c, id, req.before_id)?;
        Ok(())
    }).await?;
    Ok(ok(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct OrderReq {
    ids: Vec<i64>,
}

pub async fn order_cards(State(app): State<App>, h: HeaderMap, Json(req): Json<OrderReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let ids: Vec<i64> = req.ids.into_iter().take(members::MAX_CARDS as usize).collect();
    with_db(&app, move |c| members::reorder_cards(c, id, &ids)).await?;
    Ok(ok(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct PasswordReq {
    current: String,
    new: String,
}

pub async fn change_password(State(app): State<App>, h: HeaderMap, Json(req): Json<PasswordReq>) -> Res {
    let m = match acting_any(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if over_limit(&app, format!("in|{}", members::name_key(&m.name)), 8) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "Too many tries. Please wait 15 minutes."));
    }
    if let Err(e) = members::valid_password(&req.new) {
        return Ok(fail(StatusCode::BAD_REQUEST, e));
    }
    let id = m.id;
    let stored = with_db(&app, move |c| members::password_of(c, id)).await?.unwrap_or_default();
    let (cur, new) = (req.current, req.new);
    let hash = tokio::task::spawn_blocking(move || members::verify_password(&cur, &stored).then(|| members::hash_password(&new))).await.map_err(ise)?;
    let Some(hash) = hash else { return Ok(fail(StatusCode::UNAUTHORIZED, "Your current password isn't right.")) };
    let hash = hash.map_err(ise)?;
    let raw = with_db(&app, move |c| {
        members::set_password(c, id, &hash)?;
        members::new_session(c, id)
    })
    .await?;
    let mut r = ok(json!({ "ok": true }));
    r.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&session_cookie(&h, &raw, members::SESSION_SECS)).map_err(ise)?);
    Ok(r)
}

pub async fn delete_me(State(app): State<App>, h: HeaderMap) -> Res {
    let m = match acting_any(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    remove_member(&app, m.id).await?;
    let mut r = ok(json!({ "ok": true }));
    r.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&session_cookie(&h, "", 0)).map_err(ise)?);
    Ok(r)
}

async fn remove_member(app: &App, id: i64) -> Result<(), (StatusCode, String)> {
    for f in with_db(app, move |c| members::delete(c, id)).await? {
        admin::remove_upload(&app.uploads, &f);
    }
    app.forget();
    Ok(())
}

// ---------- the owner's screen ----------
#[derive(Deserialize, Default)]
pub struct AdminQs {
    view: Option<String>,
}

pub async fn admin_page(State(app): State<App>, h: HeaderMap, Query(q): Query<AdminQs>) -> Res {
    let power = admin::power(&app, &h);
    if power == admin::Power::Nothing {
        return Ok(Redirect::to("/admin").into_response());
    }
    let (full, me) = match power {
        admin::Power::Admin(me) => (true, me),
        admin::Power::Moderator(me) => (false, Some(me)),
        admin::Power::Nothing => (false, None),
    };
    // An admin signed in with name/secret carries the owner's cookie too: their own row is still "you".
    let me = match me {
        Some(m) => Some(m),
        None => current(&app, &h).await?.map(|m| m.id),
    };
    let showing_vouches = q.view.as_deref() == Some("vouches");
    let (nav, rows, feed, bytes) = with_db(&app, move |c| {
        let nav = chrome(c)?;
        let mut rows = vec![];
        if !showing_vouches {
            for m in members::all_for_admin(c)? {
                let highlight = match m.highlight {
                    Some(cid) => members::card(c, m.id, cid)?,
                    None => None,
                };
                let awards = members::awards_of(c, m.id, 20)?;
                let is_self = me == Some(m.id);
                rows.push(AdminRow { profile_waiting: !m.profile_ok, highlight, awards, can_act: full || !m.is_staff(), is_self, m });
            }
        }
        Ok((nav, rows, members::admin_feed(c, 200)?, members::photo_bytes(c)?))
    })
    .await?;
    let hidden_vouches = feed.iter().filter(|v| v.hidden).count();
    Ok(html(render(AdminPage { nav, tab: "trainers", rows, feed, showing_vouches, photo_mb: bytes / (1024 * 1024), photo_cap_mb: photo_cap_mb(), hidden_vouches, full })?))
}

#[derive(Deserialize)]
pub struct AdminAction {
    action: String,
    hours: Option<i64>,
    points: Option<i64>,
    reason: Option<String>,
    award: Option<i64>,
    role: Option<String>,
    /// For "seller": on or off.
    seller: Option<bool>,
    /// For "tags": the admin's tags for this trainer, shown to everyone.
    tags: Option<Vec<String>>,
}

/// What a moderator can do to a trainer. Vouches and their photos are handled by their own routes.
fn moderator_may(action: &str) -> bool {
    matches!(action, "block" | "unblock" | "delete")
}

pub async fn admin_action(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<AdminAction>) -> Res {
    let power = admin::power(&app, &h);
    if power == admin::Power::Nothing || !same_site(&h) {
        return Ok(fail(StatusCode::UNAUTHORIZED, "Your session expired. Sign in again."));
    }
    let (full, me) = match power {
        admin::Power::Admin(me) => (true, me),
        admin::Power::Moderator(me) => (false, Some(me)),
        admin::Power::Nothing => (false, None),
    };
    // The owner's cookie and a trainer's session can sit side by side (an admin signed in with name/secret): the trainer is who's acting.
    let me = match me {
        Some(m) => Some(m),
        None => current(&app, &h).await?.map(|m| m.id),
    };
    // An admin runs their own trainer like any other, but can't lock, block, remove or demote themselves (another admin can).
    if me == Some(id) && matches!(req.action.as_str(), "lock" | "block" | "delete" | "role") {
        return Ok(fail(StatusCode::FORBIDDEN, "You can't do that to your own trainer. Another admin can."));
    }
    if !full {
        if !moderator_may(&req.action) {
            return Ok(fail(StatusCode::FORBIDDEN, "Only an admin can do that."));
        }
        let target = with_db(&app, move |c| members::role_of(c, id)).await?;
        if !target.is_empty() {
            return Ok(fail(StatusCode::FORBIDDEN, "That's another staff member, so only an admin can do that."));
        }
    }
    let action = req.action.clone();
    match action.as_str() {
        "approve" => with_db(&app, move |c| members::set_profile_ok(c, id, true)).await?,
        "hide" => with_db(&app, move |c| members::set_profile_ok(c, id, false)).await?,
        "lock" => {
            let hours = req.hours.unwrap_or(0);
            if !(1..=87_600).contains(&hours) {
                return Ok(fail(StatusCode::BAD_REQUEST, "Pick how long to lock this trainer for."));
            }
            with_db(&app, move |c| members::lock(c, id, hours)).await?
        }
        "unlock" => with_db(&app, move |c| members::unlock(c, id)).await?,
        "role" => {
            let role = req.role.clone().unwrap_or_default();
            if !matches!(role.as_str(), "" | "mod" | "admin") {
                return Ok(fail(StatusCode::BAD_REQUEST, "Unknown role."));
            }
            with_db(&app, move |c| members::set_role(c, id, &role)).await?
        }
        "tags" => {
            let tags: Vec<String> = crate::db::clean_tags(&req.tags.clone().unwrap_or_default()).into_iter().take(6).collect();
            let joined = tags.join(",");
            with_db(&app, move |c| members::set_tags(c, id, &joined)).await?
        }
        "seller" => {
            let on = req.seller.unwrap_or(false);
            with_db(&app, move |c| members::set_seller(c, id, on)).await?
        }
        "block" => with_db(&app, move |c| members::set_blocked(c, id, true)).await?,
        "unblock" => with_db(&app, move |c| members::set_blocked(c, id, false)).await?,
        "stars" => {
            let points = req.points.unwrap_or(0);
            if !(1..=1_000_000).contains(&points) {
                return Ok(fail(StatusCode::BAD_REQUEST, "Type how many star points to give."));
            }
            let reason = members::clean_line(req.reason.as_deref().unwrap_or(""), 120);
            with_db(&app, move |c| members::give_stars(c, id, points, &reason)).await?
        }
        "stars-undo" => {
            let Some(award) = req.award else { return Ok(fail(StatusCode::BAD_REQUEST, "Unknown award.")) };
            with_db(&app, move |c| members::undo_award(c, id, award)).await?
        }
        "delete" => remove_member(&app, id).await?,
        "password" => {
            let temp = members::temp_password();
            let t2 = temp.clone();
            let hash = tokio::task::spawn_blocking(move || members::hash_password(&t2)).await.map_err(ise)?.map_err(ise)?;
            with_db(&app, move |c| members::set_password(c, id, &hash)).await?;
            return Ok(ok(json!({ "ok": true, "password": temp })));
        }
        _ => return Ok(fail(StatusCode::BAD_REQUEST, "Unknown action.")),
    }
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

// ---------- the chat ----------
/// Open chat windows wait here for something new, so a message appears for everyone straight away.
pub struct ChatHub {
    version: tokio::sync::watch::Sender<u64>,
    online: moka::sync::Cache<String, ()>,
    waiting: tokio::sync::Semaphore,
}

impl ChatHub {
    pub fn new() -> Self {
        Self {
            version: tokio::sync::watch::channel(1).0,
            online: moka::sync::Cache::builder().max_capacity(5000).time_to_live(std::time::Duration::from_secs(45)).build(),
            waiting: tokio::sync::Semaphore::new(120),
        }
    }
    fn changed(&self) {
        self.version.send_modify(|v| *v += 1);
    }
    fn here(&self) -> u64 {
        self.online.run_pending_tasks();
        self.online.entry_count()
    }
}

#[derive(Template)]
#[template(path = "chat.html")]
struct ChatPage {
    nav: Chrome,
}
#[derive(Template)]
#[template(path = "chat_part.html")]
struct ChatPart;
#[derive(Template)]
#[template(path = "ask.html")]
struct AskPage {
    nav: Chrome,
}
#[derive(Template)]
#[template(path = "ask_part.html")]
struct AskPart;

pub async fn chat_page(State(app): State<App>, Query(q): Query<PartQs>) -> Res {
    if q.part.is_some() {
        return Ok(html(render(ChatPart)?));
    }
    let nav = with_db(&app, |c| chrome(c)).await?;
    Ok(html(render(ChatPage { nav })?))
}

pub async fn ask_page(State(app): State<App>, Query(q): Query<PartQs>) -> Res {
    if q.part.is_some() {
        return Ok(html(render(AskPart)?));
    }
    let nav = with_db(&app, |c| chrome(c)).await?;
    Ok(html(render(AskPage { nav })?))
}

fn chat_json(m: &ChatMessage) -> Value {
    json!({ "id": m.id, "member": m.member_id, "name": m.name, "avatar": m.avatar, "role": m.role, "stars": m.stars, "worth": if m.worth > 0.0 { vault_search::peso(m.worth) } else { String::new() }, "text": m.text, "at": m.created_at, "tags": members::tag_list(&m.tags) })
}

#[derive(Deserialize)]
pub struct ChatQs {
    after: Option<i64>,
    before: Option<i64>,
    /// The last version this window saw. It waits here until there's a newer one.
    v: Option<u64>,
    /// A random id for this open window, for the count of who's here.
    c: Option<String>,
}

pub async fn chat_messages(State(app): State<App>, Query(q): Query<ChatQs>) -> Res {
    if let Some(window) = q.c.as_deref().filter(|c| !c.is_empty() && c.len() <= 32) {
        app.chat.online.insert(window.to_string(), ());
    }
    let mut retry = 0;
    if let Some(seen) = q.v {
        let mut rx = app.chat.version.subscribe();
        if *rx.borrow_and_update() == seen {
            match app.chat.waiting.try_acquire() {
                Ok(_slot) => {
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(25), rx.changed()).await;
                }
                // Too many waiting at once: this window checks back shortly instead.
                Err(_) => retry = 8000,
            }
        }
    }
    let version = *app.chat.version.borrow();
    let (after, before) = (q.after, q.before);
    let (list, gone) = with_db(&app, move |c| {
        let list = members::chat_read(c, after, before, 60)?;
        let gone = match after {
            Some(a) => members::chat_gone(c, a - 300)?,
            None => vec![],
        };
        Ok((list, gone))
    })
    .await?;
    Ok(ok(json!({ "messages": list.iter().map(chat_json).collect::<Vec<_>>(), "gone": gone, "v": version, "here": app.chat.here(), "retry": retry })))
}

#[derive(Deserialize)]
pub struct ChatPost {
    text: String,
}

pub async fn chat_post(State(app): State<App>, h: HeaderMap, Json(req): Json<ChatPost>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let text = members::clean_text(&req.text, members::CHAT_MAX);
    if text.is_empty() {
        return Ok(fail(StatusCode::BAD_REQUEST, "Write something first."));
    }
    let (id, now, t2) = (m.id, crate::unix_now(), text.clone());
    let (minute, hour, last) = with_db(&app, move |c| Ok((members::chat_recent_count(c, id, now - 30)?, members::chat_recent_count(c, id, now - 3600)?, members::chat_last_text(c, id, now - 60)?))).await?;
    if minute >= 12 || hour >= 300 {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "You're typing faster than the vault can read. Wait a few seconds and try again."));
    }
    if last.as_deref() == Some(text.as_str()) {
        return Ok(fail(StatusCode::CONFLICT, "You just said that."));
    }
    let new_id = with_db(&app, move |c| members::chat_post(c, id, &t2, now)).await?;
    app.chat.changed();
    Ok(ok(json!({ "ok": true, "id": new_id })))
}

/// Your own messages are yours to delete. Staff can delete anyone's, and moderators don't overrule other staff.
pub async fn chat_delete(State(app): State<App>, h: HeaderMap, Path(mid): Path<i64>) -> Res {
    if !same_site(&h) {
        return Ok(fail(StatusCode::FORBIDDEN, "That request didn't come from this site."));
    }
    let Some((author, role)) = with_db(&app, move |c| members::chat_author(c, mid)).await? else {
        return Ok(ok(json!({ "ok": true })));
    };
    let me = current(&app, &h).await?.map(|m| m.id);
    let allowed = me == Some(author)
        || match admin::power(&app, &h) {
            admin::Power::Admin(_) => true,
            admin::Power::Moderator(_) => role.is_empty(),
            admin::Power::Nothing => false,
        };
    if !allowed {
        return Ok(fail(StatusCode::FORBIDDEN, "That's not yours to delete."));
    }
    with_db(&app, move |c| members::chat_hide(c, mid)).await?;
    app.chat.changed();
    Ok(ok(json!({ "ok": true })))
}

// ---------- trainers' own photos ----------
/// A card the catalogue doesn't have: the trainer's own photo, a name, and the rest if they like. It only shows on their page.
pub async fn upload_card(State(app): State<App>, h: HeaderMap, mut mp: Multipart) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if m.cards >= with_db(&app, |c| Ok(marcuru_rs::sellers::max_cards(c))).await? {
        return Ok(fail(StatusCode::CONFLICT, "Your store is full. Take a card out first."));
    }
    if over_limit(&app, format!("cardup|{}", m.id), 40) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of photos in a short time. Please take a short break and try again."));
    }
    let room = with_db(&app, members::photo_bytes).await?;
    if room >= photo_cap_mb() * 1024 * 1024 {
        return Ok(fail(StatusCode::INSUFFICIENT_STORAGE, "The vault's photo space is full right now. Please try again later."));
    }
    let (mut bytes, mut name, mut set, mut cond, mut value) = (None, String::new(), String::new(), String::new(), 0.0f64);
    while let Some(f) = mp.next_field().await.map_err(|_| (StatusCode::BAD_REQUEST, "That photo didn't upload.".to_string()))? {
        let key = f.name().unwrap_or("").to_string();
        if key == "photo" {
            bytes = Some(f.bytes().await.map_err(|_| (StatusCode::BAD_REQUEST, "That photo didn't upload.".to_string()))?.to_vec());
            continue;
        }
        let text = f.text().await.unwrap_or_default();
        match key.as_str() {
            "name" => name = members::clean_line(&text, 120),
            "set" => set = members::clean_line(&text, 80),
            "cond" => cond = text.trim().to_string(),
            "value" => value = text.trim().parse::<f64>().ok().filter(|v| v.is_finite() && (0.0..=100_000_000.0).contains(v)).unwrap_or(0.0),
            _ => {}
        }
    }
    if name.is_empty() {
        return Ok(fail(StatusCode::BAD_REQUEST, "Give the card a name."));
    }
    let Some(bytes) = bytes.filter(|b| !b.is_empty()) else { return Ok(fail(StatusCode::BAD_REQUEST, "Add a photo of the card.")) };
    let cond = if members::CONDITIONS.contains(&cond.as_str()) { cond } else { "Near Mint".to_string() };
    let gate = PHOTO_GATE.acquire().await.map_err(ise)?;
    let dir = PathBuf::from(&*app.uploads);
    let stem = format!("m{}-{}", m.id, members::temp_password());
    let made = tokio::task::spawn_blocking(move || store_photo(bytes, dir, stem)).await.map_err(ise)?;
    drop(gate);
    let Ok((path, thumb, _, _, _)) = made else { return Ok(fail(StatusCode::BAD_REQUEST, "That photo couldn't be read. Try a JPEG or PNG.")) };
    let (id, key) = (m.id, format!("own:{}", members::temp_password()));
    let (n2, s2, t2, p2, c2) = (name.clone(), set.clone(), thumb.clone(), path.clone(), cond.clone());
    let added = with_db(&app, move |c| {
        let a = members::add_card(c, id, &key, &n2, "", &s2, &t2, &p2)?;
        if let Added::Yes(cid) = a {
            members::set_condition(c, id, cid, &c2)?;
            if value > 0.0 {
                members::set_own_value(c, id, cid, value.round())?;
            }
        }
        Ok(a)
    })
    .await?;
    let Added::Yes(cid) = added else {
        admin::remove_upload(&app.uploads, &path);
        admin::remove_upload(&app.uploads, &thumb);
        return Ok(fail(StatusCode::CONFLICT, "Your binder is full. Take a card out first."));
    };
    if m.profile_ok {
        app.forget();
    }
    let worth = with_db(&app, move |c| members::get(c, id)).await?.map_or(0.0, |f| f.worth);
    Ok(ok(json!({ "ok": true, "worth": vault_search::peso(worth), "card": { "id": cid, "title": name, "number": "", "set": set, "image": thumb, "cond": cond, "value": if value > 0.0 { vault_search::peso(value) } else { String::new() }, "highlight": m.highlight.is_none() } })))
}

/// Staff take a card off a trainer's page. Moderators don't overrule other staff.
pub async fn staff_remove_card(State(app): State<App>, h: HeaderMap, Path(cid): Path<i64>) -> Res {
    let power = admin::power(&app, &h);
    if power == admin::Power::Nothing || !same_site(&h) {
        return Ok(fail(StatusCode::UNAUTHORIZED, "Your session expired. Sign in again."));
    }
    let Some((owner, role, k)) = with_db(&app, move |c| members::card_owner(c, cid)).await? else { return Ok(ok(json!({ "ok": true }))) };
    if matches!(power, admin::Power::Moderator(_)) && !role.is_empty() {
        return Ok(fail(StatusCode::FORBIDDEN, "That's another staff member's card, so only an admin can take it down."));
    }
    with_db(&app, move |c| members::remove_card(c, owner, cid)).await?;
    if k.own_photo() {
        admin::remove_upload(&app.uploads, &k.image);
        admin::remove_upload(&app.uploads, &k.image_large);
    }
    app.forget();
    Ok(ok(json!({ "ok": true })))
}
