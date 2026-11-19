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
use marcuru_rs::sellers::Like;
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
    /// Copies a buyer can still order (those in open orders are spoken for).
    left: i64,
    /// "More like this": cards for sale in the vault and the open stores (`sellers::related`).
    related: Vec<Like>,
    /// How the market price of this card moved lately, as on a vault card's page.
    trend: Option<marcuru_rs::history::Trend>,
    /// Who is looking (0 for a visitor): a seller gets no "+" or Message on their own store's cards in "More like this".
    viewer: i64,
}

/// The same, without the site around it, for the card window.
#[derive(Template)]
#[template(path = "member_card_part.html")]
struct MemberCardPart {
    t: Member,
    c: MemberCard,
    left: i64,
    related: Vec<Like>,
    trend: Option<marcuru_rs::history::Trend>,
    viewer: i64,
}

#[derive(Template)]
#[template(path = "me.html")]
struct MePage {
    nav: Chrome,
    /// The trainer, inside the Manager's frame (store_tabs.html).
    p: crate::store::Panel,
    cards: Vec<MemberCard>,
    binders: Vec<members::MemberBinder>,
    stats: members::SellerStats,
    max: i64,
    conditions: [&'static str; 10],
    /// A seller's cards by what they are: for sale, highlights, on the wishlist (their My cards shows the numbers).
    kinds: (usize, usize, usize),
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
    /// The vault's team hid this trainer's page.
    page_hidden: bool,
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

/// The page to land on after logging in: where they were, or Home (the owner's plan, 18 Nov 2026: a tap goes to the
/// thing itself, never to the settings).
fn landing(next: &str) -> String {
    safe_next(next).unwrap_or("/").to_string()
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
    Ok(html(render(JoinPage { nav: titled(nav, "Register"), error: String::new(), name: String::new(), next })?))
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
    Ok((code, html(render(JoinPage { nav: titled(nav, "Register"), error: error.to_string(), name, next })?)).into_response())
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
    // Nobody waits for approval, so what stops a bot filling the disk is a limit on new trainers in a day, on top of
    // the limit for each address above.
    if with_db(&app, |c| members::joined_since(c, members::now() - 86_400)).await? >= members::MAX_JOINS_PER_DAY {
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
    let n3 = name.clone();
    with_db(&app, move |c| tell_staff_joined(c, id, &n3)).await?;
    // A new trainer lands on their new page, unless they came to finish something (an order, a message).
    let to = safe_next(&f.next).map(str::to_string).unwrap_or_else(|| format!("/trainers/{id}"));
    signed_in(&app, &h, id, members::REMEMBER_SECS, &to).await
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
    // A store its seller hid shows none of its cards for sale to anyone else.
    let cards: Vec<MemberCard> = cards.into_iter().filter(|c| is_me || owner || (c.on_show() && !(m.store_hidden && c.is_sale()))).collect();
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
    // A page the vault's team hid: the public sees the name and vouches only. Its trainer and the staff see all of it.
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
    // A store its seller hid shows none of its cards for sale to anyone else.
    let cards: Vec<MemberCard> = cards.into_iter().filter(|c| is_me || owner || (c.on_show() && !(t.store_hidden && c.is_sale()))).collect();
    let highlight = t.highlight.and_then(|h| cards.iter().find(|c| c.id == h).cloned());
    let (cards, wishes, selling) = split_kinds(cards);
    let mut nav = titled(nav, &t.name);
    // A trainer's page is worth finding (unless the vault's team hid it): a profile with their picture and binder.
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
    // A hidden or ended listing is off the site, as in the store. A sold one stays, so order links still work. A store its
    // seller hid shows none of its cards for sale.
    if !staff_view && c.is_sale() && (matches!(c.status.as_str(), "Hidden" | "Expired") || t.store_hidden) {
        return not_found(&app).await;
    }
    // A shared link shows the card, its picture and price; listings for sale in an open store are indexed.
    let mut nav = nav;
    nav.seo = crate::store::listing_seo(&nav, &t, &c);
    // A look at a card in a store counts for its seller (once a day per viewer, never the owner). Under the card, as
    // under a vault card: similar cards for sale anywhere on the site, and how its market price moved.
    let (cid2, who, viewer, card) = (c.id, crate::store::viewer_hash(&h), me.as_ref().map(|m| m.id), c.clone());
    let (related, trend, held) = with_db(&app, move |db| {
        marcuru_rs::sellers::count_view(db, cid2, &who, viewer).ok();
        Ok((marcuru_rs::sellers::related(db, &card, 12)?, marcuru_rs::history::listing_trend(db, &card.card_id, &card.finish)?, marcuru_rs::sellers::held(db, cid2)?))
    })
    .await?;
    let left = (c.qty - held).max(0);
    Ok(html(if q.part.is_some() { render(MemberCardPart { t, c, left, related, trend, viewer: viewer.unwrap_or(0) })? } else { render(MemberCardPage { nav, t, c, left, related, trend, viewer: viewer.unwrap_or(0) })? }))
}

// ---------- my trainer ----------
pub async fn me(State(app): State<App>, h: HeaderMap) -> Res {
    // Signed out (or the sign-in ran out): log in again, not Register.
    let Some(m) = current(&app, &h).await? else { return Ok(Redirect::to("/login").into_response()) };
    let id = m.id;
    let (nav, p, cards, binders, stats, max) = with_db(&app, move |c| {
        Ok((chrome(c)?, crate::store::panel(c, m, "me")?, members::cards_of(c, id)?, members::binders_of(c, id)?, members::seller_stats(c, id)?, marcuru_rs::sellers::max_cards(c)))
    })
    .await?;
    let kinds = (cards.iter().filter(|c| c.is_sale()).count(), cards.iter().filter(|c| c.is_own()).count(), cards.iter().filter(|c| c.is_wish()).count());
    Ok(html(render(MePage { nav: titled(nav, "Account Settings"), p, cards, binders, stats, max, conditions: members::CONDITIONS, kinds })?))
}

/// GET /me/page: your own trainer page. The menu's first line (your name) opens this: the page itself, not its
/// settings (the owner, 18 Nov 2026: "it should lead directly to your page, not the settings"). "Account Settings" on
/// that page opens `/me`. Signed out: log in, then come back here.
pub async fn my_page(State(app): State<App>, h: HeaderMap) -> Res {
    Ok(match current(&app, &h).await? {
        Some(m) => Redirect::to(&format!("/trainers/{}", m.id)).into_response(),
        None => Redirect::to("/login?next=/me/page").into_response(),
    })
}

/// GET /trainers/{id}/follow: whether the visitor follows this trainer (the same switch as their store's Follow), and
/// how many do. The trainer's page asks once it is open (static/people.js).
pub async fn trainer_follow_state(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    let me = current(&app, &h).await?.map(|m| m.id);
    let (following, n) = with_db(&app, move |c| Ok((match me { Some(v) => marcuru_rs::sellers::is_following(c, v, id)?, None => false }, marcuru_rs::sellers::followers(c, id)?))).await?;
    Ok(ok(json!({ "ok": true, "following": following, "followers": n, "me": me == Some(id) })))
}

/// POST /trainers/{id}/follow: follow a trainer, or stop. Following a seller and following their store are one switch;
/// they hear when the trainer posts or puts up cards.
pub async fn trainer_follow(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let me = m.id;
    if id == me {
        return Ok(fail(StatusCode::BAD_REQUEST, "That's you."));
    }
    if over_limit(&app, format!("follow|{me}"), 30) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of following for now. Please try again in a while."));
    }
    let out = with_db(&app, move |c| {
        if !members::get(c, id)?.is_some_and(|t| t.profile_ok && !t.blocked) {
            return Ok(None);
        }
        let on = marcuru_rs::sellers::toggle_follow(c, me, id)?;
        Ok(Some((on, marcuru_rs::sellers::followers(c, id)?)))
    })
    .await?;
    let Some((following, n)) = out else { return Ok(fail(StatusCode::NOT_FOUND, "That trainer isn't here.")) };
    app.forget();
    Ok(ok(json!({ "ok": true, "following": following, "followers": n })))
}

/// GET /trainers/{id}/followers: who follows this trainer (and their store), for the window behind the count.
pub async fn trainer_followers(State(app): State<App>, Path(id): Path<i64>) -> Res {
    let list = with_db(&app, move |c| {
        if !members::get(c, id)?.is_some_and(|t| t.profile_ok && !t.blocked) {
            return Ok(vec![]);
        }
        marcuru_rs::sellers::follower_list(c, id, 300)
    })
    .await?;
    Ok(ok(json!({ "people": people_json(list) })))
}

/// People for a window that lists them: their page, picture and name.
pub(crate) fn people_json(list: Vec<(i64, String, String)>) -> Vec<serde_json::Value> {
    list.into_iter().map(|(id, name, avatar)| json!({ "id": id, "initial": members::initial(&name), "name": name, "avatar": avatar, "url": format!("/trainers/{id}") })).collect()
}

/// GET /me/store/page: your own store, the page itself (the menu's "My store"; the Store Manager is a button on it).
/// Someone who doesn't sell yet goes to Apply as a seller; signed out, log in first.
pub async fn my_store_page(State(app): State<App>, h: HeaderMap) -> Res {
    Ok(match current(&app, &h).await? {
        Some(m) if m.seller => Redirect::to(&m.store_url()).into_response(),
        Some(_) => Redirect::to("/me/sell").into_response(),
        None => Redirect::to("/login?next=/me/store/page").into_response(),
    })
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
pub(crate) fn check_size(bytes: &[u8]) -> Result<(), String> {
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?.into_dimensions().map_err(|e| e.to_string())?;
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(TOO_BIG.into());
    }
    Ok(())
}

/// A profile picture: square, sharp enough to enlarge, and re-made from the pixels, so nothing hidden in the upload survives.
/// It is made small first, then cut square, so no second full-size copy is ever held in memory.
pub(crate) fn store_avatar(bytes: Vec<u8>, dir: PathBuf, stem: String) -> Result<String, String> {
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
    let (mut name, mut bio, mut photo, mut cover_file, mut drop_cover) = (m.name.clone(), m.bio.clone(), None::<Vec<u8>>, None::<Vec<u8>>, false);
    // The page's own links, never the store's (those are in Store settings). What is there stays unless the form sends it.
    let (mut fb_url, mut ig_url, mut tiktok_url, mut links) = (m.page_fb_url.clone(), m.page_ig_url.clone(), m.page_tiktok_url.clone(), m.page_links.clone());
    while let Some(f) = mp.next_field().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))? {
        match f.name().unwrap_or_default() {
            "avatar" => {
                let b = f.bytes().await.map_err(|_| (StatusCode::BAD_REQUEST, "That picture didn't upload.".to_string()))?;
                if !b.is_empty() {
                    photo = Some(b.to_vec());
                }
            }
            "cover" => {
                let b = f.bytes().await.map_err(|_| (StatusCode::BAD_REQUEST, "That cover photo didn't upload.".to_string()))?;
                if !b.is_empty() {
                    cover_file = Some(b.to_vec());
                }
            }
            "drop_cover" => {
                let v = f.text().await.unwrap_or_default();
                drop_cover = v == "1" || v == "on";
            }
            "name" => name = f.text().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?,
            "bio" => bio = f.text().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?,
            "fb_url" => fb_url = f.text().await.unwrap_or_default(),
            "ig_url" => ig_url = f.text().await.unwrap_or_default(),
            "tiktok_url" => tiktok_url = f.text().await.unwrap_or_default(),
            "links" => links = f.text().await.unwrap_or_default(),
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
    // The page's own cover photo: a new one, taken off, or left as it is.
    let cover = match cover_file {
        Some(bytes) => {
            let dir = PathBuf::from(&*app.uploads);
            let stem = format!("pcover{}-{}", m.id, members::temp_password());
            let made = gated(move || store_photo(bytes, dir, stem)).await?;
            match made {
                Ok((large, small, _, _, _)) => {
                    admin::remove_upload(&app.uploads, &small);
                    Some(large)
                }
                Err(e) => {
                    if let Some(a) = &avatar {
                        admin::remove_upload(&app.uploads, a);
                    }
                    return Ok(fail(StatusCode::BAD_REQUEST, if e == TOO_BIG { TOO_BIG_TEXT } else { "That cover photo couldn't be read. Try a JPEG or PNG." }));
                }
            }
        }
        None if drop_cover => Some(String::new()),
        None => None,
    };
    let (m2, n2, b2, a2, k2) = (m.clone(), name.clone(), bio.clone(), avatar.clone(), cover.clone());
    let saved = with_db(&app, move |c| {
        members::save_page(c, &m2, &members::PageSave { name: &n2, bio: &b2, avatar: a2.as_deref(), cover: k2.as_deref(), fb_url: &fb_url, ig_url: &ig_url, tiktok_url: &tiktok_url, links: &links })
    })
    .await?;
    if !saved {
        for f in [&avatar, &cover].into_iter().flatten().filter(|f| !f.is_empty()) {
            admin::remove_upload(&app.uploads, f);
        }
        return Ok(fail(StatusCode::CONFLICT, "That trainer name is taken. Please pick another."));
    }
    if avatar.is_some() && m.has_avatar() {
        admin::remove_upload(&app.uploads, &m.avatar);
    }
    if cover.is_some() && m.has_page_cover() {
        admin::remove_upload(&app.uploads, &m.page_cover);
    }
    app.forget();
    let id = m.id;
    let fresh = with_db(&app, move |c| members::get(c, id)).await?;
    Ok(ok(json!({ "ok": true, "name": name, "avatar": avatar, "cover": fresh.as_ref().map(|f| f.page_cover.clone()).unwrap_or_default() })))
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
    // The number beside Bookmarks in the menus (static/bookmarks.js).
    let mid = m.as_ref().map(|m| m.id);
    let bookmarks = match mid {
        Some(id) => with_db(&app, move |c| marcuru_rs::sellers::bookmark_count(c, id)).await?,
        None => 0,
    };
    Ok(ok(json!({
        "admin": p != admin::Power::Nothing,
        "full": matches!(p, admin::Power::Admin(_)),
        // "store": a seller's own store, where the menu's "My store" goes.
        "member": m.map(|m| json!({ "id": m.id, "name": m.name, "locked": m.locked(), "lock_left": m.lock_left(), "avatar": m.avatar, "initial": m.initial(), "seller": m.seller, "role": m.role, "page_hidden": !m.profile_ok, "store": if m.seller { m.store_url() } else { String::new() } })),
        // The key a browser turns alerts on with (static/alerts.js).
        "push_key": crate::push::public_key(),
        "bookmarks": bookmarks,
    })))
}

#[derive(Deserialize)]
pub struct PostReq {
    #[serde(default)]
    text: String,
    parent: Option<i64>,
    #[serde(default)]
    photos: Vec<i64>,
    /// The store it vouches for: a seller's id, or 0 / nothing for the vault's (the floating Vouch button).
    #[serde(default)]
    store: i64,
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
    let (parent, photos, store) = (req.parent, req.photos, req.store);
    // A vouch is for an open store, and never for your own (a reply may go anywhere: it joins its vouch's store).
    if parent.is_none() {
        if store == id {
            return Ok(fail(StatusCode::BAD_REQUEST, "That's your own store. Vouches come from the people you sold to."));
        }
        if !with_db(&app, move |c| members::vouchable_store(c, store)).await? {
            return Ok(fail(StatusCode::NOT_FOUND, "That store isn't open right now."));
        }
    }
    match with_db(&app, move |c| members::post_vouch(c, id, parent, store, &text, &photos)).await? {
        Posted::Yes(vid) => {
            let name = m.name.clone();
            with_db(&app, move |c| tell_about_vouch(c, id, &name, parent, store)).await?;
            app.forget();
            Ok(ok(json!({ "ok": true, "id": vid })))
        }
        Posted::Empty => Ok(fail(StatusCode::BAD_REQUEST, "Write something or add a photo first.")),
        Posted::NoParent => Ok(fail(StatusCode::NOT_FOUND, "That vouch isn't there any more.")),
    }
}

/// A vouch was written: the store it is for hears (its seller, or the vault's admins for the vault's), and a reply is
/// heard by whoever wrote the vouch it answers. The line opens the store's Vouches window.
fn tell_about_vouch(c: &rusqlite::Connection, writer: i64, name: &str, parent: Option<i64>, store: i64) -> rusqlite::Result<()> {
    let store_of = |s: i64| -> rusqlite::Result<String> {
        Ok(if s == 0 { "/store/vault#vouches".to_string() } else { members::get(c, s)?.map(|m| format!("{}#vouches", m.store_url())).unwrap_or_else(|| "/vouches".into()) })
    };
    match parent {
        None => {
            let owners = if store == 0 { marcuru_rs::dm::admins(c)? } else { vec![store] };
            let whose = if store == 0 { "the vault" } else { "your store" };
            let link = store_of(store)?;
            for o in owners.into_iter().filter(|o| *o != writer) {
                marcuru_rs::sellers::notify_from(c, o, writer, "review", "", &format!("{name} vouched for {whose}."), &link)?;
            }
        }
        Some(p) => {
            let at: Option<(i64, i64, Option<i64>)> = rusqlite::OptionalExtension::optional(c.query_row("SELECT member_id, store_id, parent_id FROM vouches WHERE id=?1", [p], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))))?;
            if let Some((author, s, up)) = at.filter(|(a, _, _)| *a != writer) {
                let what = if up.is_none() { "your vouch" } else { "you on a vouch" };
                marcuru_rs::sellers::notify_from(c, author, writer, "social", "", &format!("{name} replied to {what}."), &store_of(s)?)?;
            }
        }
    }
    Ok(())
}

/// The admins hear that a trainer joined (nobody waits for approval, so this is for knowing, not for doing): one line
/// while it is unread, brought up to date with how many joined in the last day. It opens the newest one's page.
fn tell_staff_joined(c: &rusqlite::Connection, who: i64, name: &str) -> rusqlite::Result<()> {
    let n = members::joined_since(c, members::now() - 86_400)?;
    let line = if n > 1 { format!("{name} joined. {n} new trainers in the last day.") } else { format!("{name} joined.") };
    for a in marcuru_rs::dm::admins(c)?.into_iter().filter(|a| *a != who) {
        marcuru_rs::sellers::notify_from(c, a, who, "staff", "staff:joined", &line, &format!("/trainers/{who}"))?;
    }
    Ok(())
}

/// The vault's team hides a trainer's page, or shows it again. Its trainer is told, once for each change.
fn show_page(c: &rusqlite::Connection, id: i64, show: bool) -> rusqlite::Result<()> {
    let was = members::get(c, id)?.is_some_and(|m| m.profile_ok);
    members::set_profile_ok(c, id, show)?;
    if was != show {
        let words = if show { "Your page is showing again: everyone can see it." } else { "The vault's team hid your page. Only you can see it for now." };
        marcuru_rs::sellers::notify(c, id, "announcement", words, &format!("/trainers/{id}"))?;
    }
    Ok(())
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
    /// "delete" (its writer or the owner), "hide" or "show" (the owner), "edit" (its writer, with `text`)
    action: String,
    #[serde(default)]
    text: String,
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
        // Its writer changes their own words; nobody else's, staff or not.
        "edit" => {
            let m = match acting(&app, &h).await? {
                Ok(m) => m,
                Err(r) => return Ok(r),
            };
            let (me, text) = (m.id, members::clean_text(&req.text, members::TEXT_MAX));
            let words = text.clone();
            if !with_db(&app, move |c| members::edit_vouch(c, vid, me, &words)).await? {
                return Ok(fail(StatusCode::BAD_REQUEST, if text.is_empty() { "Write something first." } else { "That isn't yours to change." }));
            }
            app.forget();
            return Ok(ok(json!({ "ok": true, "text": text })));
        }
        "hide" | "show" if owner => {
            let hide = req.action == "hide";
            let by = current(&app, &h).await?.map(|m| m.id).unwrap_or(0);
            with_db(&app, move |c| {
                members::set_vouch_hidden(c, vid, hide)?;
                // Hidden by the vault's team: its writer is told (they still see it themselves, so they would wonder).
                if hide {
                    tell_vouch_writer(c, vid, by, "hid")?;
                }
                Ok(())
            })
            .await?;
        }
        "delete" => {
            if !owner {
                let Some(m) = current(&app, &h).await? else { return Ok(fail(StatusCode::UNAUTHORIZED, "Please log in again.")) };
                if with_db(&app, move |c| members::vouch_owner(c, vid)).await? != Some(m.id) {
                    return Ok(fail(StatusCode::FORBIDDEN, "That isn't yours to delete."));
                }
            }
            let by = current(&app, &h).await?.map(|m| m.id).unwrap_or(0);
            let files = with_db(&app, move |c| {
                if owner {
                    tell_vouch_writer(c, vid, by, "took down")?;
                }
                members::delete_vouch(c, vid)
            })
            .await?;
            for f in files {
                admin::remove_upload(&app.uploads, &f);
            }
        }
        _ => return Ok(fail(StatusCode::FORBIDDEN, "Only the vault owner can do that.")),
    }
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

/// A vouch (or a reply under one) the vault's team hid or took down tells its writer, unless they did it themselves:
/// nobody is left wondering where their words went (the owner, 19 Nov 2026).
fn tell_vouch_writer(c: &rusqlite::Connection, vid: i64, by: i64, did: &str) -> rusqlite::Result<()> {
    let found: Option<(i64, String, Option<i64>)> = rusqlite::OptionalExtension::optional(c.query_row("SELECT member_id, text, parent_id FROM vouches WHERE id=?1", [vid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))))?;
    let Some((writer, text, parent)) = found else { return Ok(()) };
    if writer == by || writer == 0 {
        return Ok(());
    }
    let words: String = if text.chars().count() > 80 { format!("{}…", text.chars().take(79).collect::<String>()) } else { text };
    let what = if parent.is_some() { "reply to a vouch" } else { "vouch" };
    let said = if words.trim().is_empty() { String::new() } else { format!(": “{words}”") };
    marcuru_rs::sellers::notify_from(c, writer, 0, "announcement", "", &format!("The vault's team {did} your {what}{said}"), "/vouches")
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
pub struct SetsQs {
    #[serde(default)]
    q: String,
    #[serde(default)]
    lang: String,
}

/// GET /me/sets/search?q=&lang=: the card list's sets, newest first, for "By set" on Add cards (anyone signed in).
pub async fn my_set_search(State(app): State<App>, h: HeaderMap, Query(q): Query<SetsQs>) -> Res {
    let Some(m) = current(&app, &h).await? else { return Ok(fail(StatusCode::UNAUTHORIZED, "Please log in again.")) };
    if over_limit(&app, format!("search|{}", m.id), 120) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of searching. Please wait a few minutes."));
    }
    let typed: String = q.q.chars().take(60).collect();
    Ok(ok(market::set_search(&app.http, &typed, &q.lang).await))
}

#[derive(Deserialize)]
pub struct SetCardsQs {
    #[serde(default)]
    id: String,
    #[serde(default)]
    lang: String,
}

/// GET /me/sets/cards?id=&lang=: every card in one set (a vintage set split into its printings), for "By set".
pub async fn my_set_cards(State(app): State<App>, h: HeaderMap, Query(q): Query<SetCardsQs>) -> Res {
    let Some(m) = current(&app, &h).await? else { return Ok(fail(StatusCode::UNAUTHORIZED, "Please log in again.")) };
    if over_limit(&app, format!("search|{}", m.id), 120) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of searching. Please wait a few minutes."));
    }
    let id: String = q.id.chars().take(40).collect();
    Ok(ok(match market::set_cards(&app.http, &id, &q.lang).await {
        Ok(v) => v,
        Err(e) => json!({ "cards": [], "error": e }),
    }))
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
            // Cards for sale are added through "Add to your store" (add_card_full), where the seller sets the price.
            // Here a card goes in as theirs (or a wish), never for sale at an automatic price.
            let k = req.kind.filter(|k| k == "own" || k == "wish").unwrap_or_else(|| "own".into());
            with_db(&app, move |c| marcuru_rs::sellers::set_kind(c, id, cid, &k)).await?.ok();
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
            if let Some(k) = gone.filter(MemberCard::uploaded_photo) {
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
    let (ok_flag, photo) = with_db(&app, move |c| {
        let photo = members::binder_photo(c, id, bid)?;
        Ok((members::delete_member_binder(c, id, bid)?, photo))
    })
    .await?;
    if !ok_flag {
        return Ok(fail(StatusCode::NOT_FOUND, "Binder not found."));
    }
    if photo.starts_with("/uploads/") {
        admin::remove_upload(&app.uploads, &photo);
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
    // A card for the cover, or the cover picking itself again: a photo uploaded before lets go (one cover at a time).
    let done = with_db(&app, move |c| {
        if !members::set_member_binder_cover(c, id, bid, req.card_id)? {
            return Ok(None);
        }
        members::set_member_binder_photo(c, id, bid, "")
    })
    .await?;
    let Some(old) = done else { return Ok(fail(StatusCode::NOT_FOUND, "Could not set binder cover.")) };
    if old.starts_with("/uploads/") {
        admin::remove_upload(&app.uploads, &old);
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
    /// One line, tapped in the bell.
    id: Option<i64>,
}

pub async fn read_notifications(State(app): State<App>, h: HeaderMap, Json(req): Json<ReadNotificationReq>) -> Res {
    let m = match acting_any(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    with_db(&app, move |c| {
        match req.id {
            Some(one) => members::mark_one_read(c, id, one)?,
            None => members::mark_notifications_read(c, id, req.before_id)?,
        };
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
                rows.push(AdminRow { page_hidden: !m.profile_ok, highlight, awards, can_act: full || !m.is_staff(), is_self, m });
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
        // A page is live from the start. The vault's team can hide one ("hide") and show it again ("approve", the name
        // it had when pages waited for approval).
        "approve" => with_db(&app, move |c| show_page(c, id, true)).await?,
        "hide" => with_db(&app, move |c| show_page(c, id, false)).await?,
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
            with_db(&app, move |c| {
                let was = members::get(c, id)?.is_some_and(|m| m.seller);
                members::set_seller(c, id, on)?;
                if on && !was {
                    marcuru_rs::sellers::notify(c, id, "announcement", "Your store is open. Add your first cards.", "/me/add")?;
                }
                Ok(())
            })
            .await?
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
    json!({ "id": m.id, "member": m.member_id, "name": m.name, "avatar": m.avatar, "role": m.role, "text": m.text, "at": m.created_at, "tags": members::tag_list(&m.tags) })
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
/// How many of their own card photos a trainer whose page the vault's team hid can hold: past it they add no more
/// until the page shows again.
const HIDDEN_PAGE_PHOTOS: i64 = 20;

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
    if !m.profile_ok && own >= HIDDEN_PAGE_PHOTOS {
        return Ok(fail(StatusCode::FORBIDDEN, "The vault's team has hidden your page, so you can't add more of your own photos for now."));
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
    if k.uploaded_photo() {
        admin::remove_upload(&app.uploads, &k.image);
        admin::remove_upload(&app.uploads, &k.image_large);
    }
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

/// GET /me/cards/price?id=: what the card list's market says a card is worth, for a seller's "Add a card" window
/// (static/add-card.js). It is only shown as a hint there: the seller types their own price.
pub async fn seller_price(State(app): State<App>, h: HeaderMap, Query(q): Query<SellerPriceQs>) -> Res {
    let Some(m) = current(&app, &h).await? else { return Ok(fail(StatusCode::UNAUTHORIZED, "Please log in again.")) };
    if !m.seller {
        return Ok(fail(StatusCode::FORBIDDEN, "Only sellers add cards to a store."));
    }
    if over_limit(&app, format!("sprice|{}", m.id), 240) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of looking up. Please wait a few minutes."));
    }
    let id: String = q.id.chars().take(60).collect();
    Ok(ok(crate::admin::price_for_id(&app, &id).await))
}

#[derive(Deserialize)]
pub struct SellerPriceQs {
    id: String,
}

/// The most copies one "Add a card" can put up at once.
const ADD_QTY_MAX: i64 = 99;

/// POST /me/cards/new (a form, with an optional photo): a seller puts a card from the card list up for sale with
/// everything chosen by them first: its condition, their own price, how many, a note, the binder, and either the card
/// list's picture or a photo of their own card. Nothing is filled in for them (the owner, 18 Nov 2026: "adding a card
/// shouldn't automatically give it a price"). The listing is made only when everything checks out.
pub async fn add_card_full(State(app): State<App>, h: HeaderMap, mut mp: Multipart) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if !m.seller {
        return Ok(fail(StatusCode::FORBIDDEN, "Only sellers add cards to a store. Apply as a seller from the menu."));
    }
    if m.cards >= with_db(&app, |c| Ok(marcuru_rs::sellers::max_cards(c))).await? {
        return Ok(fail(StatusCode::CONFLICT, "Your store is full. Take a card out first."));
    }
    if over_limit(&app, format!("addfull|{}", m.id), 120) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of cards in a short time. Please take a short break and try again."));
    }
    let (mut card_id, mut image, mut image_large, mut en) = (String::new(), None::<String>, None::<String>, None::<String>);
    let (mut cond, mut price, mut qty, mut notes, mut binder, mut photo) = (String::new(), String::new(), String::from("1"), String::new(), String::new(), None::<Vec<u8>>);
    let mut finish = String::new();
    while let Some(f) = mp.next_field().await.map_err(|_| (StatusCode::BAD_REQUEST, "That didn't upload. Please try again.".to_string()))? {
        let key = f.name().unwrap_or("").to_string();
        if key == "photo" {
            let b = f.bytes().await.map_err(|_| (StatusCode::BAD_REQUEST, "The photo didn't upload. Please try again.".to_string()))?;
            if !b.is_empty() {
                photo = Some(b.to_vec());
            }
            continue;
        }
        let text = f.text().await.unwrap_or_default();
        match key.as_str() {
            "id" => card_id = text.trim().chars().take(60).collect(),
            "image" => image = Some(text),
            "image_large" => image_large = Some(text),
            "en" => en = Some(text),
            "cond" => cond = text.trim().to_string(),
            "price" => price = text.trim().to_string(),
            "qty" => qty = text.trim().to_string(),
            "notes" => notes = text,
            "binder_id" => binder = text.trim().to_string(),
            "finish" => finish = text.trim().chars().take(40).collect(),
            _ => {}
        }
    }
    // Everything is checked before anything is made.
    if card_id.is_empty() {
        return Ok(fail(StatusCode::BAD_REQUEST, "Pick a card first."));
    }
    // The version (Normal, Holo, Reverse Holo...) when the window offered a choice (a scanned card's tiles, scan.rs): a
    // key from the card list's prices. Empty, the listing is the card's usual version, as before. "market" is a price
    // with no version, never a version.
    if !finish.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == ' ') {
        return Ok(fail(StatusCode::BAD_REQUEST, "That version isn't one the card list knows. Please try again."));
    }
    if finish.eq_ignore_ascii_case("market") {
        finish.clear();
    }
    if !members::CONDITIONS.contains(&cond.as_str()) {
        return Ok(fail(StatusCode::BAD_REQUEST, "Choose the card's condition."));
    }
    let Some(price) = price.replace(',', "").parse::<f64>().ok().filter(|p| p.is_finite() && *p >= 1.0 && *p <= 100_000_000.0) else {
        return Ok(fail(StatusCode::BAD_REQUEST, "Type your price for one card, in pesos."));
    };
    let Some(qty) = qty.parse::<i64>().ok().filter(|q| (1..=ADD_QTY_MAX).contains(q)) else {
        return Ok(fail(StatusCode::BAD_REQUEST, "How many: from 1 to 99."));
    };
    let notes = members::clean_text(&notes, 300);
    let id = m.id;
    let binder_id = match binder.parse::<i64>().ok().filter(|b| *b > 0) {
        Some(b) => {
            let mine = with_db(&app, move |c| Ok(members::binders_of(c, id)?.iter().any(|x| x.id == b))).await?;
            if !mine {
                return Ok(fail(StatusCode::BAD_REQUEST, "That binder isn't yours."));
            }
            Some(b)
        }
        None => None,
    };
    if photo.is_some() {
        let room = with_db(&app, |c| Ok(members::photo_bytes(c)?)).await?;
        if room >= photo_cap_mb() * 1024 * 1024 {
            return Ok(fail(StatusCode::INSUFFICIENT_STORAGE, "The vault's photo space is full right now. Use the card's own picture for now."));
        }
    }
    // The card's details come from the card list, not from what the browser says.
    let info = match market::by_id_lang(&app.http, &app.fx, &card_id).await {
        Ok(Some(v)) => v,
        _ => return Ok(fail(StatusCode::BAD_REQUEST, "That card couldn't be found. Please try again.")),
    };
    let text = |k: &str| info[k].as_str().unwrap_or("").to_string();
    let name = text("name");
    if name.is_empty() {
        return Ok(fail(StatusCode::BAD_REQUEST, "That card couldn't be found. Please try again."));
    }
    let en = en.as_deref().map(|e| members::clean_line(e, 60)).filter(|e| !e.is_empty() && *e != name);
    let title = members::clean_line(&en.map(|e| format!("{name} ({e})")).unwrap_or_else(|| name.clone()), 120);
    let number = match (info["number"].as_str(), info["total"].as_i64()) {
        (Some(n), Some(t)) if t > 0 => format!("{n}/{t}"),
        (Some(n), _) => n.to_string(),
        _ => String::new(),
    };
    let pick = |own: String, given: Option<String>| if known_picture(&own) { own } else { given.filter(|g| known_picture(g)).unwrap_or_default() };
    let (mut pic, mut pic_large) = (pick(text("image"), image), pick(text("image_large"), image_large));
    // A photo of the seller's own card replaces the card list's picture.
    let mut own_bytes = 0i64;
    if let Some(bytes) = photo {
        let dir = PathBuf::from(&*app.uploads);
        let stem = format!("m{}-{}", m.id, members::temp_password());
        match gated(move || store_photo(bytes, dir, stem)).await? {
            Ok((large, small, _, _, size)) => {
                pic = small;
                pic_large = large;
                own_bytes = size.max(1);
            }
            Err(e) if e == TOO_BIG => return Ok(fail(StatusCode::BAD_REQUEST, TOO_BIG_TEXT)),
            Err(_) => return Ok(fail(StatusCode::BAD_REQUEST, "That photo couldn't be read. Try a JPEG or PNG.")),
        }
    }
    let set = members::clean_line(&text("set"), 80);
    let market = market::member_finish_price(&info, &finish).unwrap_or(0.0);
    let (t2, n2, s2, i2, l2, key) = (title.clone(), number.clone(), set.clone(), pic.clone(), pic_large.clone(), card_id.clone());
    let made = with_db(&app, move |c| {
        let added = marcuru_rs::sellers::add_listing(c, id, &key, &t2, &n2, &s2, &i2, &l2)?;
        let Added::Yes(cid) = added else { return Ok(Err(added)) };
        marcuru_rs::sellers::set_kind(c, id, cid, "sale")?.ok();
        // The market price is kept for the seller's dashboard and hints; the card sells at the seller's own price.
        let value = if market > 0.0 { market } else { members::vault_price(c, &key)? };
        members::set_listing_value(c, id, cid, value, crate::unix_now())?;
        if own_bytes > 0 {
            members::set_card_photo_bytes(c, cid, own_bytes)?;
        }
        let patch = marcuru_rs::sellers::Patch {
            price: Some(price),
            cond: Some(cond),
            qty: Some(qty),
            notes: Some(notes),
            binder_id: binder_id.map(Some),
            finish: (!finish.is_empty()).then_some(finish),
            ..Default::default()
        };
        let card = marcuru_rs::sellers::update_listing(c, id, cid, &patch)?;
        Ok(Ok((cid, card)))
    })
    .await?;
    let (cid, card) = match made {
        Ok(x) => x,
        Err(a) => {
            if own_bytes > 0 {
                admin::remove_upload(&app.uploads, &pic);
                admin::remove_upload(&app.uploads, &pic_large);
            }
            return Ok(fail(StatusCode::CONFLICT, if matches!(a, Added::Full) { "Your store is full. Take a card out first." } else { "That card is already in your store." }));
        }
    };
    if m.profile_ok {
        app.forget();
    }
    let shown = card.as_ref().map(|k| k.price_text()).unwrap_or_default();
    Ok(ok(json!({ "ok": true, "card": { "id": cid, "title": title, "number": number, "set": set, "image": pic, "price": shown, "binder_id": binder_id } })))
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

    fn db() -> rusqlite::Connection {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        marcuru_rs::db::register_set_code(&c).unwrap();
        c.execute_batch(marcuru_rs::db::SCHEMA).unwrap();
        c.execute_batch(marcuru_rs::blocks::SCHEMA).unwrap();
        c.execute_batch(members::SCHEMA).unwrap();
        members::migrate(&c).unwrap();
        marcuru_rs::db::migrate(&c).unwrap();
        c
    }

    /// A trainer's own page and their store have their own settings (the owner, 19 Nov 2026: "when i add a link in
    /// store, it shows up in my personal page too"): the page shows only what Account Settings holds, the store only
    /// what Store settings holds, and the store can have a picture of its own.
    #[test]
    fn a_trainers_page_and_their_store_keep_their_own_settings() {
        let c = db();
        let s = members::create(&c, "Ash", "h").unwrap().unwrap();
        members::set_profile_ok(&c, s, true).unwrap();
        members::set_seller(&c, s, true).unwrap();
        c.execute("UPDATE members SET avatar='/uploads/t-ash.jpg', bio='I collect Pikachu' WHERE id=?1", [s]).unwrap();
        // Store settings: the store's links, cover, words and picture.
        let store = marcuru_rs::sellers::StoreSettings { store_name: "Pallet Cards".into(), fb_url: "https://facebook.com/palletcards".into(), links: "Carousell | https://carousell.ph/u/pallet".into(), status: "Active".into(), store_bio: "Singles and sealed, shipped in toploaders.".into(), ..Default::default() };
        marcuru_rs::sellers::save_store(&c, s, &store).unwrap();
        c.execute("UPDATE members SET cover_photo='/uploads/cover-store_l.jpg', profile_ok=1 WHERE id=?1", [s]).unwrap();
        let m = members::get(&c, s).unwrap().unwrap();
        assert_eq!(m.store_pic(), "/uploads/t-ash.jpg", "the store shows its owner's picture until it has its own");
        let page = |c: &rusqlite::Connection, is_me: bool| {
            let t = members::get(c, s).unwrap().unwrap();
            render(TrainerPage { nav: chrome(c).unwrap(), t, cards: vec![], wishes: vec![], selling: vec![], highlight: None, is_me, full: true, vouches: vec![], awards: vec![] }).unwrap()
        };
        let html = page(&c, false);
        assert!(!html.contains("palletcards") && !html.contains("carousell") && !html.contains("cover-store") && !html.contains("Find me on"), "nothing of the store's on the trainer's own page: {html}");
        // Account Settings: the page's own cover and links (a bad link is dropped). The store's stay as they are.
        assert!(members::save_page(&c, &m, &members::PageSave { name: "Ash", bio: "I collect Pikachu", avatar: None, cover: Some("/uploads/pcover-ash_l.jpg"), fb_url: "https://facebook.com/ash.ketchum", ig_url: "javascript:alert(1)", tiktok_url: "", links: "YouTube | https://youtube.com/@ash" }).unwrap());
        let m = members::get(&c, s).unwrap().unwrap();
        assert_eq!((m.page_cover.as_str(), m.page_fb_url.as_str(), m.page_ig_url.as_str(), m.fb_url.as_str(), m.cover_photo.as_str()), ("/uploads/pcover-ash_l.jpg", "https://facebook.com/ash.ketchum", "", "https://facebook.com/palletcards", "/uploads/cover-store_l.jpg"));
        assert_eq!(m.page_link_list().iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), vec!["Facebook", "YouTube"]);
        assert_eq!(m.social_links().iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), vec!["Facebook", "Carousell"], "the store's links are untouched");
        assert!(m.profile_ok, "a changed page stays live: nothing waits for approval");
        members::set_profile_ok(&c, s, true).unwrap();
        let m = members::get(&c, s).unwrap().unwrap();
        assert!(members::save_page(&c, &m, &members::PageSave { name: "Ash", bio: "I collect Pikachu", avatar: None, cover: None, fb_url: "https://facebook.com/ash.ketchum", ig_url: "", tiktok_url: "", links: "YouTube | https://youtube.com/@ash" }).unwrap());
        assert!(members::get(&c, s).unwrap().unwrap().profile_ok, "saved again with nothing changed: still live");
        let html = page(&c, true);
        assert!(html.contains("src=\"/uploads/pcover-ash_l.jpg\"") && html.contains("href=\"https://facebook.com/ash.ketchum\"") && html.contains(">YouTube <") && !html.contains("palletcards") && !html.contains("cover-store"), "{html}");
        assert!(html.contains("href=\"/me\"") && html.contains("Account Settings"), "the page's own settings button");
        // Taking the page's cover off leaves the store's alone.
        let m = members::get(&c, s).unwrap().unwrap();
        members::save_page(&c, &m, &members::PageSave { name: "Ash", bio: "I collect Pikachu", avatar: None, cover: Some(""), fb_url: "", ig_url: "", tiktok_url: "", links: "" }).unwrap();
        let m = members::get(&c, s).unwrap().unwrap();
        assert_eq!((m.page_cover.as_str(), m.cover_photo.as_str(), m.page_link_list().len(), m.social_links().len()), ("", "/uploads/cover-store_l.jpg", 0, 2));
        // The store's own picture.
        c.execute("UPDATE members SET store_avatar='/uploads/s-pallet.jpg' WHERE id=?1", [s]).unwrap();
        let m = members::get(&c, s).unwrap().unwrap();
        assert_eq!((m.store_pic(), m.avatar.as_str(), m.store_bio.as_str()), ("/uploads/s-pallet.jpg", "/uploads/t-ash.jpg", "Singles and sealed, shipped in toploaders."));
    }

    #[test]
    fn a_seller_changes_cards_in_listings_and_a_collector_here() {
        let c = db();
        let s = members::create(&c, "Ash", "h").unwrap().unwrap();
        members::set_profile_ok(&c, s, true).unwrap();
        let members::Added::Yes(id) = marcuru_rs::sellers::add_listing(&c, s, "tcgdex:base1-58", "Pikachu", "58/102", "Base Set", "/p/pika.webp", "").unwrap() else { panic!() };
        c.execute("UPDATE member_cards SET kind='own' WHERE id=?1", [id]).unwrap();
        let page = |c: &rusqlite::Connection| {
            let m = members::get(c, s).unwrap().unwrap();
            let cards = members::cards_of(c, s).unwrap();
            let kinds = (cards.iter().filter(|k| k.is_sale()).count(), cards.iter().filter(|k| k.is_own()).count(), cards.iter().filter(|k| k.is_wish()).count());
            render(MePage { nav: chrome(c).unwrap(), p: crate::store::panel(c, m, "me").unwrap(), cards, binders: vec![], stats: members::seller_stats(c, s).unwrap(), max: 100, conditions: members::CONDITIONS, kinds }).unwrap()
        };
        // A trainer without a store changes their cards right here, as before.
        let html = page(&c);
        assert!(html.contains("data-kind") && html.contains("data-cond") && !html.contains("Manage them in Listings"));
        // A seller: the pictures, ★ and their order here; everything else in Listings.
        members::set_seller(&c, s, true).unwrap();
        let html = page(&c);
        assert!(html.contains("<a class=\"btn\" href=\"/me/listings\">Manage them in Listings</a>") && html.contains("data-star") && !html.contains("data-kind") && !html.contains("data-cond"), "{html}");
        assert!(html.contains("<span><b>0</b> for sale</span><span><b>1</b> highlight</span>"));
        // One menu from a tablet up: the account menu has the cart, the header has no ☰.
        assert!(html.contains("data-cart-open") && html.contains("class=\"hd-ib hd-cart\"") && !html.contains("hd-menu-btn"));
    }

    #[test]
    fn a_store_card_says_who_sells_it_and_shows_finish_note_and_copies_left() {
        let c = db();
        let s = members::create(&c, "Ash", "h").unwrap().unwrap();
        members::set_seller(&c, s, true).unwrap();
        members::set_profile_ok(&c, s, true).unwrap();
        c.execute("UPDATE members SET store_name='Pallet Cards' WHERE id=?1", [s]).unwrap();
        let members::Added::Yes(id) = marcuru_rs::sellers::add_listing(&c, s, "tcgdex:base1-58", "Pikachu - 058/102 - Base Set", "58/102", "Base Set", "", "").unwrap() else { panic!() };
        c.execute("UPDATE member_cards SET kind='sale', status='Active', qty=3, value_own=500, finish='reverseHolofoil', notes='Light whitening on the back.' WHERE id=?1", [id]).unwrap();
        let t = members::get(&c, s).unwrap().unwrap();
        let card = members::card(&c, s, id).unwrap().unwrap();
        let html = render(MemberCardPart { t: t.clone(), c: card.clone(), left: 2, related: vec![], trend: None, viewer: 0 }).unwrap();
        assert!(html.contains("<small>Sold by</small><b class=\"mco-name\">Pallet Cards</b>") && html.contains(&format!("href=\"{}\"", t.store_url())), "{html}");
        assert!(html.contains("<dt>Finish</dt><dd>Reverse Holo</dd>") && html.contains("<dd>2 left</dd>") && html.contains("<dt>Seller's note</dt><dd>Light whitening on the back.</dd>"), "{html}");
        assert!(!html.contains("Ask in the store"), "asking is Message");
        // A card that isn't for sale belongs to its trainer: "Owned by", and the box opens their page.
        c.execute("UPDATE member_cards SET kind='own' WHERE id=?1", [id]).unwrap();
        let html = render(MemberCardPart { t, c: members::card(&c, s, id).unwrap().unwrap(), left: 0, related: vec![], trend: None, viewer: 0 }).unwrap();
        assert!(html.contains("<small>Owned by</small><b class=\"mco-name\">Ash</b>") && html.contains(&format!("href=\"/trainers/{s}\"")) && !html.contains("left</dd>"), "{html}");
    }

    #[test]
    fn a_vouch_tells_its_store_a_reply_tells_its_writer_and_admins_hear_who_joined() {
        let c = db();
        let ash = members::create(&c, "Ash", "h").unwrap().unwrap();
        members::set_seller(&c, ash, true).unwrap();
        members::set_profile_ok(&c, ash, true).unwrap();
        let misty = members::create(&c, "Misty", "h").unwrap().unwrap();
        members::set_profile_ok(&c, misty, true).unwrap();
        let boss = members::create(&c, "Boss", "h").unwrap().unwrap();
        members::set_profile_ok(&c, boss, true).unwrap();
        c.execute("UPDATE members SET role='admin' WHERE id=?1", [boss]).unwrap();
        let last = |m: i64| members::notifications(&c, m, true).unwrap().into_iter().next().unwrap();
        // Misty vouches for Ash's store: Ash hears, with Misty's picture, and the line opens the store's vouches.
        let members::Posted::Yes(v) = members::post_vouch(&c, misty, None, ash, "Great trade", &[]).unwrap() else { panic!() };
        tell_about_vouch(&c, misty, "Misty", None, ash).unwrap();
        let n = last(ash);
        assert_eq!((n.kind.as_str(), n.actor_id, n.message.as_str()), ("review", misty, "Misty vouched for your store."));
        assert!(n.link.ends_with("#vouches") && n.link.starts_with("/store/"), "{}", n.link);
        // A vouch for the vault: its admins hear.
        tell_about_vouch(&c, misty, "Misty", None, 0).unwrap();
        assert_eq!(last(boss).link, "/store/vault#vouches");
        // Ash answers Misty's vouch: Misty hears. Nobody hears about their own words.
        let members::Posted::Yes(reply) = members::post_vouch(&c, ash, Some(v), ash, "Thanks!", &[]).unwrap() else { panic!() };
        tell_about_vouch(&c, ash, "Ash", Some(v), ash).unwrap();
        assert_eq!((last(misty).message.as_str(), last(misty).actor_id), ("Ash replied to your vouch.", ash));
        tell_about_vouch(&c, misty, "Misty", Some(reply), ash).unwrap();
        assert_eq!(last(ash).message, "Misty replied to you on a vouch.");
        tell_about_vouch(&c, ash, "Ash", None, ash).unwrap();
        assert_eq!(last(ash).message, "Misty replied to you on a vouch.", "not told about their own vouch");
        // New trainers: one line in the admins' bell while it is unread, kept up to date, opening the newest one's page.
        // Nobody waits for approval: their pages are live already.
        let brock = members::create(&c, "Brock", "h").unwrap().unwrap();
        tell_staff_joined(&c, brock, "Brock").unwrap();
        let gary = members::create(&c, "Gary", "h").unwrap().unwrap();
        tell_staff_joined(&c, gary, "Gary").unwrap();
        let staff: Vec<(String, String)> = members::notifications(&c, boss, true).unwrap().into_iter().filter(|n| n.kind == "staff").map(|n| (n.message, n.link)).collect();
        assert_eq!(staff, vec![("Gary joined. 5 new trainers in the last day.".to_string(), format!("/trainers/{gary}"))]);
        assert!(members::get(&c, gary).unwrap().unwrap().profile_ok && members::approved(&c).unwrap().iter().any(|m| m.id == gary));
        // The vault's team hides a page and shows it again: its trainer is told each time, and only when it changed.
        show_page(&c, gary, false).unwrap();
        show_page(&c, gary, false).unwrap();
        assert_eq!(last(gary).message, "The vault's team hid your page. Only you can see it for now.");
        assert_eq!(members::notifications(&c, gary, true).unwrap().len(), 1);
        show_page(&c, gary, true).unwrap();
        assert_eq!((last(gary).message.as_str(), members::get(&c, gary).unwrap().unwrap().profile_ok), ("Your page is showing again: everyone can see it.", true));
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
