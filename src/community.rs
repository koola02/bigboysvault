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

/// `max_age` 0 means "until the browser closes" (no Max-Age); a negative value removes the cookie.
fn session_cookie(h: &HeaderMap, raw: &str, max_age: i64) -> String {
    let https = h.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https");
    let age = if max_age == 0 && !raw.is_empty() { String::new() } else { format!("; Max-Age={}", max_age.max(0)) };
    format!("{COOKIE}={raw}; Path=/; HttpOnly; SameSite=Lax{age}{}", if https { "; Secure" } else { "" })
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

/// Cloudflare's own addresses (cloudflare.com/ips). Only a request that really came from one of these may name the visitor.
const CLOUDFLARE: [&str; 22] = [
    "173.245.48.0/20", "103.21.244.0/22", "103.22.200.0/22", "103.31.4.0/22", "141.101.64.0/18", "108.162.192.0/18",
    "190.93.240.0/20", "188.114.96.0/20", "197.234.240.0/22", "198.41.128.0/17", "162.158.0.0/15", "104.16.0.0/13",
    "104.24.0.0/14", "172.64.0.0/13", "131.0.72.0/22",
    "2400:cb00::/32", "2606:4700::/32", "2803:f800::/32", "2405:b500::/32", "2405:8100::/32", "2a06:98c0::/29", "2c0f:f248::/32",
];

/// True when `ip` sits inside one of Cloudflare's ranges.
fn from_cloudflare(ip: &str) -> bool {
    use std::net::IpAddr;
    let Ok(ip) = ip.parse::<IpAddr>() else { return false };
    CLOUDFLARE.iter().any(|range| {
        let Some((net, bits)) = range.split_once('/') else { return false };
        let (Ok(net), Ok(bits)) = (net.parse::<IpAddr>(), bits.parse::<u32>()) else { return false };
        match (ip, net) {
            (IpAddr::V4(a), IpAddr::V4(n)) => bits <= 32 && (u32::from(a) ^ u32::from(n)).checked_shr(32 - bits).unwrap_or(0) == 0,
            (IpAddr::V6(a), IpAddr::V6(n)) => bits <= 128 && (u128::from(a) ^ u128::from(n)).checked_shr(128 - bits).unwrap_or(0) == 0,
            _ => false,
        }
    })
}

/// The visitor's address. Fly's proxy sets `fly-client-ip` itself (a visitor can't fake it): that is the machine that
/// connected. When that machine is Cloudflare, Cloudflare's `cf-connecting-ip` names the real visitor. Anyone else
/// could write any `cf-connecting-ip` they like (the fly.dev address skips Cloudflare), so it is ignored for them.
pub(crate) fn client_ip(h: &HeaderMap) -> String {
    let header = |k: &str| h.get(k).and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty());
    match header("fly-client-ip") {
        Some(peer) if from_cloudflare(peer) => header("cf-connecting-ip").unwrap_or(peer).to_string(),
        Some(peer) => peer.to_string(),
        None => "local".into(),
    }
}

/// Counts attempts for a while. True means the limit has been reached.
pub(crate) fn over_limit(app: &App, key: String, max: u32) -> bool {
    let n = app.limits.get(&key).unwrap_or(0) + 1;
    app.limits.insert(key, n);
    n > max
}

/// True when this count has reached `max`. Only looks: it doesn't add one or restart the wait.
fn limit_hit(app: &App, key: &str, max: u32) -> bool {
    app.limits.get(key).unwrap_or(0) >= max
}

/// Adds one to a count of failed tries.
fn count_fail(app: &App, key: &str) {
    let n = app.limits.get(key).unwrap_or(0) + 1;
    app.limits.insert(key.to_string(), n);
}

/// Password work at the same time. Each one needs about 19 MB, and the server is small.
static HASH_GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
const BUSY: &str = "The vault is busy right now. Please try again in a moment.";

/// Runs a password hash or check, at most two at once, so a burst of sign-ins can't use up the memory.
/// Ok(None): it waited a few seconds and it still wasn't its turn.
pub(crate) async fn hashing<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<Option<T>, (StatusCode, String)> {
    let Ok(Ok(permit)) = tokio::time::timeout(std::time::Duration::from_secs(4), HASH_GATE.acquire()).await else { return Ok(None) };
    let out = tokio::task::spawn_blocking(f).await.map_err(ise)?;
    drop(permit);
    Ok(Some(out))
}

/// Where to go after logging in, when the page asked: only a path on this site ("/..."), never another site.
fn safe_next(next: &str) -> Option<&str> {
    let n = next.trim();
    // Plain visible letters only (a browser sends a path %-coded), so it is always a safe address to send people to.
    let ok = n.starts_with('/') && !n.starts_with("//") && !n.contains('\\') && n.len() <= 300 && n.bytes().all(|b| b.is_ascii_graphic());
    ok.then_some(n)
}

/// The page's name in the tab, in links people share, and for search engines: one title everywhere.
fn titled(mut nav: Chrome, title: &str) -> Chrome {
    nav.seo.title = format!("{title} · {}", nav.brand);
    nav
}

fn known_picture(url: &str) -> bool {
    market::image_host_ok(url)
}

/// Looks up prices for trainers' cards a few at a time, politely. Runs in the background.
pub async fn refresh_values(app: &App) {
    let now = crate::unix_now();
    let Ok(ids) = with_db(app, move |c| members::cards_to_price(c, now - 5 * 86_400, 400)).await else { return };
    let mut changed = false;
    for id in ids {
        // The price service didn't answer: try this card again next time, and keep the price it has.
        let Ok(info) = market::by_id_lang(&app.http, &app.fx, &id).await else {
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
            continue;
        };
        // Finish by finish, as the nightly job does: a Reverse Holo listing never takes the Holofoil price.
        let done = with_db(app, move |c| members::price_listings(c, &id, info.as_ref(), crate::unix_now())).await;
        changed |= done.unwrap_or(false);
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
    /// Where to go after logging in (a path on this site), or empty for "My trainer".
    next: String,
}

#[derive(Template)]
#[template(path = "join.html")]
struct JoinPage {
    nav: Chrome,
    error: String,
    name: String,
    /// Where to go after joining (a path on this site), or empty for "My trainer".
    next: String,
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
    wishes: Vec<MemberCard>,
    selling: Vec<MemberCard>,
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
    /// Cards they're after.
    wishes: Vec<MemberCard>,
    /// Cards in their store (a seller).
    selling: Vec<MemberCard>,
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
    #[serde(default)]
    next: String,
}

/// The page to land on after logging in or joining.
fn landing(next: &str) -> String {
    safe_next(next).unwrap_or("/me").to_string()
}

/// Makes a path safe to put after "?next=": letters, digits and "/-_.~" stay, everything else is %-coded.
fn encode_next(p: &str) -> String {
    p.bytes().map(|b| if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

pub async fn join_page(State(app): State<App>, h: HeaderMap, Query(q): Query<JoinQs>) -> Res {
    if current(&app, &h).await?.is_some() {
        return Ok(Redirect::to(&landing(&q.next)).into_response());
    }
    // There's one log-in page, /login. Old "/join?signin=1" links go there.
    if q.signin.is_some() {
        let to = match safe_next(&q.next) {
            Some(n) => format!("/login?next={}", encode_next(n)),
            None => "/login".to_string(),
        };
        return Ok(Redirect::to(&to).into_response());
    }
    let nav = with_db(&app, chrome).await?;
    let next = safe_next(&q.next).unwrap_or("").to_string();
    Ok(html(render(JoinPage { nav: titled(nav, "Join"), error: String::new(), name: String::new(), next })?))
}

/// `GET /login`: where trainers log in. The vault owner's own way in is /admin.
pub async fn login_page(State(app): State<App>, h: HeaderMap, Query(q): Query<LoginQs>) -> Res {
    if current(&app, &h).await?.is_some() {
        return Ok(Redirect::to(&landing(&q.next)).into_response());
    }
    if admin::authed(&app, &h) && !app.admin_token.is_empty() {
        return Ok(Redirect::to("/admin").into_response());
    }
    let nav = with_db(&app, chrome).await?;
    let error = if q.wrong.is_some() { "That password isn't right.".to_string() } else { String::new() };
    let next = safe_next(&q.next).unwrap_or("").to_string();
    Ok(html(render(LoginPage { nav: titled(nav, "Log in"), error, name: String::new(), next })?))
}

#[derive(Deserialize)]
pub struct LoginQs {
    wrong: Option<String>,
    #[serde(default)]
    next: String,
}

/// Shows the form again with a message. `mode` is "signin" (the log-in page) or "join".
async fn join_again(app: &App, code: StatusCode, mode: &'static str, name: String, error: &str, next: &str) -> Res {
    let nav = with_db(app, chrome).await?;
    let next = safe_next(next).unwrap_or("").to_string();
    if mode == "signin" {
        return Ok((code, html(render(LoginPage { nav: titled(nav, "Log in"), error: error.to_string(), name, next })?)).into_response());
    }
    Ok((code, html(render(JoinPage { nav: titled(nav, "Join"), error: error.to_string(), name, next })?)).into_response())
}

#[derive(Deserialize)]
pub struct JoinForm {
    name: String,
    password: String,
    /// A field people never see. Only bots fill it in.
    #[serde(default)]
    website: String,
    /// "Remember me": stay signed in for a year instead of until the browser closes.
    #[serde(default)]
    remember: String,
    /// Where to go afterwards, say back to the cart. Only a path on this site is followed.
    #[serde(default)]
    next: String,
}

/// How long the browser keeps a sign-in: a year when "remember me" is ticked, else until it closes (0).
fn session_age(remember: &str) -> i64 {
    if remember == "1" || remember == "on" { members::REMEMBER_SECS } else { 0 }
}

/// Starts a session and goes on to `to`. The server keeps it as long as the cookie says (a month for "until the browser closes").
async fn signed_in(app: &App, h: &HeaderMap, id: i64, age: i64, to: &str) -> Res {
    let keep = if age > 0 { age } else { members::SESSION_SECS };
    let raw = with_db(app, move |c| members::new_session(c, id, keep)).await?;
    Ok(([(header::SET_COOKIE, session_cookie(h, &raw, age))], Redirect::to(to)).into_response())
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
        return join_again(&app, StatusCode::TOO_MANY_REQUESTS, "join", typed, "Too many new trainers from here. Please try again in a little while.", &f.next).await;
    }
    let name = match members::valid_name(&f.name) {
        Ok(n) => n,
        Err(e) => return join_again(&app, StatusCode::BAD_REQUEST, "join", typed, e, &f.next).await,
    };
    if let Err(e) = members::valid_password(&f.password) {
        return join_again(&app, StatusCode::BAD_REQUEST, "join", typed, e, &f.next).await;
    }
    if with_db(&app, members::unapproved_count).await? >= members::MAX_PENDING {
        return join_again(&app, StatusCode::SERVICE_UNAVAILABLE, "join", typed, "New sign-ups are paused for a moment. Please try again later.", &f.next).await;
    }
    // A taken name is turned away before the slow password work.
    const TAKEN: &str = "That trainer name is taken. Please pick another.";
    let n0 = name.clone();
    if with_db(&app, move |c| members::for_signin(c, &n0)).await?.is_some() {
        return join_again(&app, StatusCode::CONFLICT, "join", typed, TAKEN, &f.next).await;
    }
    let pass = f.password;
    let Some(hash) = hashing(move || members::hash_password(&pass)).await? else {
        return join_again(&app, StatusCode::SERVICE_UNAVAILABLE, "join", typed, BUSY, &f.next).await;
    };
    let hash = hash.map_err(ise)?;
    let n2 = name.clone();
    let Some(id) = with_db(&app, move |c| members::create(c, &n2, &hash)).await? else {
        return join_again(&app, StatusCode::CONFLICT, "join", typed, TAKEN, &f.next).await;
    };
    over_limit(&app, format!("joined|{ip}"), u32::MAX);
    signed_in(&app, &h, id, members::REMEMBER_SECS, &landing(&f.next)).await
}

pub async fn signin(State(app): State<App>, h: HeaderMap, Form(f): Form<JoinForm>) -> Res {
    if !same_site(&h) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }
    let typed = members::clean_line(&f.name, 40);
    let next = f.next.clone();
    // Only wrong passwords count, per name and device: a stranger's wrong tries lock out only the stranger.
    // A much higher count per name, from every device together, stops guessing spread over many addresses.
    let ip = client_ip(&h);
    let nk = members::name_key(&typed);
    let (pair, by_name) = (format!("fail|{nk}|{ip}"), format!("fail|{nk}"));
    let by_ip = over_limit(&app, format!("in|{ip}"), 20);
    if by_ip || limit_hit(&app, &pair, 8) || limit_hit(&app, &by_name, 100) {
        return join_again(&app, StatusCode::TOO_MANY_REQUESTS, "signin", typed, "Too many tries. Please wait 15 minutes and try again.", &next).await;
    }
    let wrong = |app: &App| {
        count_fail(app, &pair);
        count_fail(app, &by_name);
    };
    // An admin may still log in with "name/secret". It opens their own trainer only, never the owner's cookie:
    // admin powers come from their role, checked on every request, so taking the role away or blocking them ends it at once.
    if admin::admin_password_ok(&app, &typed, &f.password) {
        let n3 = typed.clone();
        let who = with_db(&app, move |c| match members::for_signin(c, &n3)? {
            Some((id, _)) => members::get(c, id),
            None => Ok(None),
        })
        .await?;
        let Some(m) = who else {
            wrong(&app);
            return join_again(&app, StatusCode::UNAUTHORIZED, "signin", typed, "There's no trainer with that name. Make one first, then an admin can make it staff.", &next).await;
        };
        if m.role != "admin" {
            wrong(&app);
            return join_again(&app, StatusCode::UNAUTHORIZED, "signin", typed, "That trainer isn't an admin.", &next).await;
        }
        if m.blocked {
            return join_again(&app, StatusCode::FORBIDDEN, "signin", typed, "This trainer has been blocked by the vault owner.", &next).await;
        }
        app.limits.invalidate(&pair);
        let to = safe_next(&next).unwrap_or("/admin").to_string();
        return signed_in(&app, &h, m.id, session_age(&f.remember), &to).await;
    }
    let n2 = typed.clone();
    let found = with_db(&app, move |c| members::for_signin(c, &n2)).await?;
    let pass = f.password;
    let checked = hashing(move || match &found {
        Some((id, stored)) => members::verify_password(&pass, stored).then_some(*id),
        None => {
            members::waste_time(&pass);
            None
        }
    })
    .await?;
    let Some(good) = checked else {
        return join_again(&app, StatusCode::SERVICE_UNAVAILABLE, "signin", typed, BUSY, &next).await;
    };
    match good {
        Some(id) if with_db(&app, move |c| members::is_blocked(c, id)).await? => {
            join_again(&app, StatusCode::FORBIDDEN, "signin", typed, "This trainer has been blocked by the vault owner.", &next).await
        }
        Some(id) => {
            app.limits.invalidate(&pair);
            signed_in(&app, &h, id, session_age(&f.remember), &landing(&next)).await
        }
        None => {
            wrong(&app);
            join_again(&app, StatusCode::UNAUTHORIZED, "signin", typed, "That name and password don't match.", &next).await
        }
    }
}

pub async fn signout(State(app): State<App>, h: HeaderMap) -> Res {
    if !same_site(&h) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }
    if let Some(raw) = cookie_value(&h) {
        with_db(&app, move |c| members::end_session(c, &raw)).await?;
    }
    // The owner's cookie goes too, in case this browser still holds one from an older sign-in.
    Ok((axum::response::AppendHeaders([(header::SET_COOKIE, session_cookie(&h, "", -1)), (header::SET_COOKIE, "marcuru_admin=; Path=/; Max-Age=0".to_string())]), Redirect::to("/")).into_response())
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
    let nav = titled(nav, &format!("{} · {}", b.title, m.name));
    let awards: Vec<Award> = awards.into_iter().filter(|a| !a.reason.is_empty()).collect();
    // Hidden, sold or ended listings stay off the public page, as in the store. The trainer and staff see everything.
    let cards: Vec<MemberCard> = cards.into_iter().filter(|c| is_me || owner || c.on_show()).collect();
    let highlight = m.highlight.and_then(|h| cards.iter().find(|c| c.id == h).cloned());
    let (cards, wishes, selling) = split_kinds(cards);
    Ok(html(if q.part.is_some() {
        render(TrainerPart { t: m, cards, wishes, selling, highlight, is_me, full, vouches, awards })?
    } else {
        render(TrainerPage { nav, t: m, cards, wishes, selling, highlight, is_me, full, vouches, awards })?
    }))
}

pub(crate) async fn not_found(app: &App) -> Res {
    Ok(crate::not_found(State(app.clone())).await)
}

/// A binder's cards by what they are: (highlights, wishlist, for sale).
fn split_kinds(cards: Vec<MemberCard>) -> (Vec<MemberCard>, Vec<MemberCard>, Vec<MemberCard>) {
    let (mut own, mut wish, mut sale) = (vec![], vec![], vec![]);
    for c in cards {
        match c.kind.as_str() {
            "wish" => wish.push(c),
            "sale" => sale.push(c),
            _ => own.push(c),
        }
    }
    (own, wish, sale)
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
    // Not approved yet: the public sees the name and vouches. The picture, description, links and binder wait for the owner.
    let full = t.profile_ok || is_me || owner;
    if !full {
        t.avatar.clear();
        t.bio.clear();
        t.fb_url.clear();
        t.tiktok_url.clear();
        t.links.clear();
        t.fb_group_url.clear();
        t.ig_url.clear();
        t.shopee_url.clear();
    }
    let cards = if full { cards } else { vec![] };
    // Hidden, sold or ended listings stay off the public page, as in the store. The trainer and staff see everything.
    let cards: Vec<MemberCard> = cards.into_iter().filter(|c| is_me || owner || c.on_show()).collect();
    let highlight = t.highlight.and_then(|h| cards.iter().find(|c| c.id == h).cloned());
    let (cards, wishes, selling) = split_kinds(cards);
    let mut nav = titled(nav, &t.name);
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
        render(TrainerPart { t, cards, wishes, selling, highlight, is_me, full, vouches, awards })?
    } else {
        render(TrainerPage { nav, t, cards, wishes, selling, highlight, is_me, full, vouches, awards })?
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
    let staff_view = me.as_ref().is_some_and(|m| m.id == t.id) || admin::can_moderate(&app, &h);
    if (!t.profile_ok || t.blocked) && !staff_view {
        return not_found(&app).await;
    }
    // A hidden or ended listing is off the site, as in the store. A sold one stays, so order links still work.
    if !staff_view && c.is_sale() && matches!(c.status.as_str(), "Hidden" | "Expired") {
        return not_found(&app).await;
    }
    // A shared link shows the card, its picture and price; listings for sale in an open store are indexed.
    let mut nav = nav;
    nav.seo = crate::store::listing_seo(&nav, &t, &c);
    // A look at a card in a store counts for its seller (once a day per viewer, never the owner).
    let (cid2, who, viewer) = (c.id, crate::store::viewer_hash(&h), me.as_ref().map(|m| m.id));
    with_db(&app, move |c| marcuru_rs::sellers::count_view(c, cid2, &who, viewer)).await.ok();
    Ok(html(if q.part.is_some() { render(MemberCardPart { t, c })? } else { render(MemberCardPage { nav, t, c })? }))
}

// ---------- my trainer ----------
pub async fn me(State(app): State<App>, h: HeaderMap) -> Res {
    // Signed out (or the sign-in ran out): log in again, not "Become a trainer".
    let Some(m) = current(&app, &h).await? else { return Ok(Redirect::to("/login").into_response()) };
    let id = m.id;
    let (nav, cards, binders, stats, max) = with_db(&app, move |c| {
        Ok((chrome(c)?, members::cards_of(c, id)?, members::binders_of(c, id)?, members::seller_stats(c, id)?, marcuru_rs::sellers::max_cards(c)))
    })
    .await?;
    Ok(html(render(MePage { nav: titled(nav, "My trainer"), m, cards, binders, stats, max, conditions: members::CONDITIONS })?))
}

/// Signed in and really from this site, whatever their lock says. For things a locked trainer may still do: their password, deleting what's theirs.
pub(crate) async fn acting_any(app: &App, h: &HeaderMap) -> Result<Result<Member, Response>, (StatusCode, String)> {
    if !same_site(h) {
        return Ok(Err(fail(StatusCode::FORBIDDEN, "That request didn't come from this site.")));
    }
    Ok(current(app, h).await?.ok_or_else(|| fail(StatusCode::UNAUTHORIZED, "Please log in again.")))
}

/// Every action on "My trainer" and every post starts here: signed in, really from this site, and not locked.
pub(crate) async fn acting(app: &App, h: &HeaderMap) -> Result<Result<Member, Response>, (StatusCode, String)> {
    Ok(match acting_any(app, h).await? {
        Ok(m) if m.locked() => Err(fail(StatusCode::FORBIDDEN, &format!("Your trainer is locked for another {}. You can't post or change anything until then.", m.lock_left()))),
        other => other,
    })
}

/// Pictures bigger than this are turned away before they're opened: the server is small.
const MAX_PIXELS: u64 = 16_000_000;
/// The error a too-big picture gives, so the page can say so in plain words.
pub(crate) const TOO_BIG: &str = admin::TOO_BIG; // one word for "too big", whichever check finds it
const TOO_BIG_TEXT: &str = "That photo is too big. Please use a smaller one (under 16 megapixels).";

/// Reads only the picture's size from its first bytes, and refuses one that is too big to open safely.
fn check_size(bytes: &[u8]) -> Result<(), String> {
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?.into_dimensions().map_err(|e| e.to_string())?;
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(TOO_BIG.into());
    }
    Ok(())
}

/// A profile picture: square, sharp enough to enlarge, and re-made from the pixels, so nothing hidden in the upload survives.
/// It is made small first, then cut square, so no second full-size copy is ever held in memory.
fn store_avatar(bytes: Vec<u8>, dir: PathBuf, stem: String) -> Result<String, String> {
    use image::{codecs::jpeg::JpegEncoder, imageops::FilterType};
    check_size(&bytes)?;
    let img = admin::open_image(bytes)?;
    let (w, h) = (u64::from(img.width()), u64::from(img.height()));
    let short = w.min(h);
    if short < 16 {
        return Err("too small".into());
    }
    let target = short.clamp(400, 640);
    // Shrink first, keeping the shape, so the short side is `target`.
    let img = if short > target { img.thumbnail((w * target).div_ceil(short) as u32, (h * target).div_ceil(short) as u32) } else { img };
    let side = img.width().min(img.height());
    let target = target as u32;
    let square = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side).resize_exact(target, target, FilterType::Triangle);
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
    use image::{codecs::jpeg::JpegEncoder, imageops::FilterType, DynamicImage};
    check_size(&bytes)?;
    let img = admin::open_image(bytes)?;
    // `thumbnail` shrinks without the big working copy a full resize needs.
    let big = if img.width().max(img.height()) > 1600 { img.thumbnail(1600, 1600) } else { img };
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

/// Room for trainers' photos (vouch photos and their own card photos), so they can never fill the disk the whole site
/// lives on. Raise it after growing the volume.
pub(crate) fn photo_cap_mb() -> i64 {
    std::env::var("PHOTO_SPACE_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(650)
}

/// One picture is opened at a time, everywhere on the site: opening a big photo takes a lot of the small server's memory.
pub(crate) static PHOTO_GATE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// Opens a picture behind PHOTO_GATE. The permit moves into the blocking work and is let go only when that work
/// ends, so a request that is cancelled mid-way can't free the slot while its picture is still being opened.
pub(crate) async fn gated<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, (StatusCode, String)> {
    let gate = PHOTO_GATE.acquire().await.map_err(ise)?;
    tokio::task::spawn_blocking(move || {
        let _gate = gate;
        work()
    })
    .await
    .map_err(ise)
}

/// Card photos added before their size was kept are measured once, so the photo-space limit counts them too.
async fn measure_old_card_photos(app: &App) -> Result<(), (StatusCode, String)> {
    let dir = PathBuf::from(&*app.uploads);
    with_db(app, move |c| {
        for (id, small, big) in members::unmeasured_card_photos(c, 500)? {
            let size: i64 = [small, big]
                .iter()
                .filter_map(|u| u.strip_prefix("/uploads/"))
                .filter(|f| !f.is_empty() && !f.contains('/') && !f.contains('\\') && !f.contains(".."))
                .map(|f| std::fs::metadata(dir.join(f)).map_or(0, |m| m.len() as i64))
                .sum();
            // At least 1, so a photo whose files are gone isn't measured again and again.
            members::set_card_photo_bytes(c, id, size.max(1))?;
        }
        Ok(())
    })
    .await
}

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
            let made = gated(move || store_avatar(bytes, dir, stem)).await?;
            match made {
                Ok(p) => Some(p),
                Err(e) if e == TOO_BIG => return Ok(fail(StatusCode::BAD_REQUEST, TOO_BIG_TEXT)),
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
    let p = admin::power(&app, &h);
    Ok(ok(json!({
        "admin": p != admin::Power::Nothing,
        "full": matches!(p, admin::Power::Admin(_)),
        "member": m.map(|m| json!({ "id": m.id, "name": m.name, "locked": m.locked(), "lock_left": m.lock_left(), "avatar": m.avatar, "initial": m.initial(), "seller": m.seller, "role": m.role })),
        // The key a browser turns alerts on with (static/alerts.js).
        "push_key": crate::push::public_key(),
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
    measure_old_card_photos(&app).await?;
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
    let dir = PathBuf::from(&*app.uploads);
    let stem = format!("v{id}-{}", members::temp_password());
    let made = gated(move || store_photo(bytes, dir, stem)).await?;
    let (path, thumb, w, h2, size) = match made {
        Ok(x) => x,
        Err(e) if e == TOO_BIG => return Ok(fail(StatusCode::BAD_REQUEST, TOO_BIG_TEXT)),
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
                let Some(m) = current(&app, &h).await? else { return Ok(fail(StatusCode::UNAUTHORIZED, "Please log in again.")) };
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
        return Ok(fail(StatusCode::UNAUTHORIZED, "Your session expired. Please log in again."));
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
    let Some(m) = current(&app, &h).await? else { return Ok(fail(StatusCode::UNAUTHORIZED, "Please log in again.")) };
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
    /// "own", "sale" or "wish". A seller's card is for sale unless told otherwise; a collector's is a highlight.
    kind: Option<String>,
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
            if let Some(k) = req.kind.filter(|k| k == "own" || k == "wish" || k == "sale") {
                with_db(&app, move |c| marcuru_rs::sellers::set_kind(c, id, cid, &k)).await?.ok();
            }
            // Only the new listing gets a value (its finish is still empty, so the card's default finish). Other sellers'
            // listings of this card keep their own finish's price from the nightly job.
            let market = market::member_finish_price(&info, "").unwrap_or(0.0);
            let key = req.id.chars().take(60).collect::<String>();
            let value = with_db(&app, move |c| {
                let value = if market > 0.0 { market } else { members::vault_price(c, &key)? };
                members::set_listing_value(c, id, cid, value, crate::unix_now())?;
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
                j["kind"] = json!(c.kind);
            }
            Ok(ok(json!({ "ok": true, "worth": vault_search::peso(worth), "card": j })))
        }
    }
}

#[derive(Deserialize)]
pub struct CardAction {
    /// "remove", "highlight", "condition", "value", "binder" or "kind"
    action: String,
    cond: Option<String>,
    value: Option<f64>,
    binder_id: Option<i64>,
    /// For "kind": "own" (a highlight), "sale" or "wish".
    kind: Option<String>,
}

/// `POST /me/cards/{card}/original-photo`: the photo the VM tidied goes back to the one the trainer took.
/// A plain form post: it goes back to the page it came from.
pub async fn original_photo(State(app): State<App>, h: HeaderMap, Path(cid): Path<i64>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let tidied: Option<(String, String)> = with_db(&app, move |c| {
        let row: Option<(String, String, String)> = c
            .query_row(
                "SELECT photo_original, image, image_large FROM member_cards WHERE id=?1 AND member_id=?2 AND photo_clean=1",
                [cid, id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        let Some((orig, small, large)) = row else { return Ok(None) };
        let Some((o_small, o_large)) = orig.split_once('|') else { return Ok(None) };
        c.execute(
            "UPDATE member_cards SET image=?3, image_large=?4, photo_original='', photo_clean=3 WHERE id=?1 AND member_id=?2",
            rusqlite::params![cid, id, o_small, o_large],
        )?;
        Ok(Some((small, large)))
    })
    .await?;
    if let Some((_, large)) = tidied {
        admin::remove_upload(&app.uploads, &large); // the tidied copy (both sizes)
        app.forget();
    }
    let back = h
        .get(axum::http::header::REFERER)
        .and_then(|v| v.to_str().ok())
        .and_then(|r| r.find("://").map(|i| &r[i + 3..]).and_then(|r| r.find('/').map(|j| r[j..].to_string())))
        .filter(|p| p.starts_with('/') && !p.starts_with("//"))
        .unwrap_or_else(|| "/me".to_string());
    Ok(axum::response::Redirect::to(&back).into_response())
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
        "kind" => {
            let kind = req.kind.unwrap_or_default();
            if let Err(e) = with_db(&app, move |c| marcuru_rs::sellers::set_kind(c, id, cid, &kind)).await? {
                return Ok(fail(StatusCode::BAD_REQUEST, e));
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
        out["kind"] = json!(c.kind);
        out["status"] = json!(c.status);
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
    // A sensible ceiling: a store of binders, not a wall of them.
    let binder = with_db(&app, move |c| {
        if members::binder_count(c, id)? >= members::MAX_BINDERS {
            return Ok(None);
        }
        members::create_member_binder(c, id, &title).map(Some)
    })
    .await?;
    let Some(binder) = binder else {
        return Ok(fail(StatusCode::BAD_REQUEST, &format!("You have {} binders, the most one trainer can have. Delete one to make another.", members::MAX_BINDERS)));
    };
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
    // A locked trainer still sees and clears their own bell.
    let m = match acting_any(&app, &h).await? {
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
    let m = match acting_any(&app, &h).await? {
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
    // Its own count, separate from logging in: only wrong current passwords add to it, and only this trainer can.
    let k = format!("pw|{}", m.id);
    if limit_hit(&app, &k, 8) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "Too many tries. Please wait 15 minutes."));
    }
    if let Err(e) = members::valid_password(&req.new) {
        return Ok(fail(StatusCode::BAD_REQUEST, e));
    }
    let id = m.id;
    let stored = with_db(&app, move |c| members::password_of(c, id)).await?.unwrap_or_default();
    let (cur, new) = (req.current, req.new);
    let Some(hash) = hashing(move || members::verify_password(&cur, &stored).then(|| members::hash_password(&new))).await? else {
        return Ok(fail(StatusCode::SERVICE_UNAVAILABLE, BUSY));
    };
    let Some(hash) = hash else {
        count_fail(&app, &k);
        return Ok(fail(StatusCode::UNAUTHORIZED, "Your current password isn't right."));
    };
    let hash = hash.map_err(ise)?;
    // The new sign-in lasts as long as the old one was meant to: "remember me" stays a year, "this visit only" stays that way.
    let old = cookie_value(&h).unwrap_or_default();
    let (raw, remembered) = with_db(&app, move |c| {
        let remembered = members::session_remembered(c, &old)?.unwrap_or(false);
        members::set_password(c, id, &hash)?;
        let keep = if remembered { members::REMEMBER_SECS } else { members::SESSION_SECS };
        Ok((members::new_session(c, id, keep)?, remembered))
    })
    .await?;
    let age = if remembered { members::REMEMBER_SECS } else { 0 };
    let mut r = ok(json!({ "ok": true }));
    r.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&session_cookie(&h, &raw, age)).map_err(ise)?);
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
    // The owner's cookie and a trainer's session can sit side by side in one browser: their own row is still "you".
    let me = match me {
        Some(m) => Some(m),
        None => current(&app, &h).await?.map(|m| m.id),
    };
    let showing_vouches = q.view.as_deref() == Some("vouches");
    measure_old_card_photos(&app).await?;
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
        return Ok(fail(StatusCode::UNAUTHORIZED, "Your session expired. Please log in again."));
    }
    let (full, me) = match power {
        admin::Power::Admin(me) => (true, me),
        admin::Power::Moderator(me) => (false, Some(me)),
        admin::Power::Nothing => (false, None),
    };
    // The owner's cookie and a trainer's session can sit side by side in one browser: the trainer is who's acting.
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
            let Some(hash) = hashing(move || members::hash_password(&t2)).await? else { return Ok(fail(StatusCode::SERVICE_UNAVAILABLE, BUSY)) };
            let hash = hash.map_err(ise)?;
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
    ai: bool,
}
#[derive(Template)]
#[template(path = "ask_part.html")]
struct AskPart {
    ai: bool,
}

pub async fn chat_page(State(app): State<App>, Query(q): Query<PartQs>) -> Res {
    if q.part.is_some() {
        return Ok(html(render(ChatPart)?));
    }
    let nav = with_db(&app, |c| chrome(c)).await?;
    Ok(html(render(ChatPage { nav: titled(nav, "Vault chat") })?))
}

pub async fn ask_page(State(app): State<App>, Query(q): Query<PartQs>) -> Res {
    if q.part.is_some() {
        return Ok(html(render(AskPart { ai: crate::vault_ai::enabled() })?));
    }
    let nav = with_db(&app, |c| chrome(c)).await?;
    Ok(html(render(AskPage { nav: titled(nav, "Ask the Vault"), ai: crate::vault_ai::enabled() })?))
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
/// How many of their own card photos a trainer can add before the vault owner approves their page.
const UNAPPROVED_PHOTOS: i64 = 20;

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
    measure_old_card_photos(&app).await?;
    let id = m.id;
    let (room, own) = with_db(&app, move |c| Ok((members::photo_bytes(c)?, members::own_photo_count(c, id)?))).await?;
    if room >= photo_cap_mb() * 1024 * 1024 {
        return Ok(fail(StatusCode::INSUFFICIENT_STORAGE, "The vault's photo space is full right now. Please try again later."));
    }
    // Until the vault owner approves a new trainer, they can add only a few of their own photos.
    if !m.profile_ok && own >= UNAPPROVED_PHOTOS {
        return Ok(fail(StatusCode::FORBIDDEN, "You can add more of your own photos once the vault owner approves your page."));
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
    let dir = PathBuf::from(&*app.uploads);
    let stem = format!("m{}-{}", m.id, members::temp_password());
    let made = gated(move || store_photo(bytes, dir, stem)).await?;
    let (path, thumb, size) = match made {
        Ok((path, thumb, _, _, size)) => (path, thumb, size),
        Err(e) if e == TOO_BIG => return Ok(fail(StatusCode::BAD_REQUEST, TOO_BIG_TEXT)),
        Err(_) => return Ok(fail(StatusCode::BAD_REQUEST, "That photo couldn't be read. Try a JPEG or PNG.")),
    };
    let key = format!("own:{}", members::temp_password());
    let (n2, s2, t2, p2, c2) = (name.clone(), set.clone(), thumb.clone(), path.clone(), cond.clone());
    let added = with_db(&app, move |c| {
        let a = members::add_card(c, id, &key, &n2, "", &s2, &t2, &p2)?;
        if let Added::Yes(cid) = a {
            members::set_card_photo_bytes(c, cid, size.max(1))?;
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
        return Ok(fail(StatusCode::UNAUTHORIZED, "Your session expired. Please log in again."));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, HeaderValue::from_static(v));
        }
        h
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_cancelled_upload_keeps_the_photo_gate_until_its_picture_is_done() {
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let job = tokio::spawn(gated(move || {
            let _ = wait.recv();
        }));
        let busy = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while PHOTO_GATE.available_permits() > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        });
        busy.await.expect("the picture work took the gate");
        // The request goes away (the visitor closed the page): the picture is still being opened, so the gate stays shut.
        job.abort();
        let _ = job.await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(PHOTO_GATE.available_permits(), 0);
        // Once the work ends, the gate opens again.
        go.send(()).unwrap();
        let open = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while PHOTO_GATE.available_permits() == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        });
        open.await.expect("the gate opens when the work is done");
    }

    #[test]
    fn the_visitor_address_cant_be_faked() {
        // Straight to Fly (the fly.dev address): a made-up Cloudflare header is ignored.
        assert_eq!(client_ip(&headers(&[("fly-client-ip", "203.0.113.9"), ("cf-connecting-ip", "1.2.3.4")])), "203.0.113.9");
        // Through Cloudflare: Fly sees a Cloudflare machine, and Cloudflare names the visitor.
        assert_eq!(client_ip(&headers(&[("fly-client-ip", "172.70.1.2"), ("cf-connecting-ip", "198.51.100.7")])), "198.51.100.7");
        assert_eq!(client_ip(&headers(&[("fly-client-ip", "2606:4700:10::1"), ("cf-connecting-ip", "198.51.100.7")])), "198.51.100.7");
        assert_eq!(client_ip(&headers(&[("fly-client-ip", "172.70.1.2")])), "172.70.1.2");
        assert_eq!(client_ip(&headers(&[("cf-connecting-ip", "1.2.3.4")])), "local");
        assert!(from_cloudflare("104.16.0.1"));
        assert!(!from_cloudflare("104.32.0.1"));
        assert!(!from_cloudflare("not an address"));
    }

    #[test]
    fn only_paths_on_this_site_are_followed_after_logging_in() {
        assert_eq!(safe_next("/?cart=1"), Some("/?cart=1"));
        assert_eq!(safe_next("/store/ash?cart=1"), Some("/store/ash?cart=1"));
        assert_eq!(safe_next("//evil.example"), None);
        assert_eq!(safe_next("/\\evil.example"), None);
        assert_eq!(safe_next("https://evil.example"), None);
        assert_eq!(safe_next(""), None);
        assert_eq!(safe_next("/a\nb"), None);
        assert_eq!(encode_next("/?cart=1"), "/%3Fcart%3D1");
    }
}
