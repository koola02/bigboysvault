//! Private admin: manage cards (add / edit / delete / change photo) and the home page's sections.
//! Open by default; set ADMIN_TOKEN to require a password.
use super::{chrome, ise, safe_link, with_db, App, Chrome};
use askama::Template;
use axum::{
    extract::{Json, Multipart, Path, Query as Qs, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use marcuru_rs::blocks::{self, Block};
use marcuru_rs::{bulk, market};
use marcuru_rs::db::{self, Entry, Product, Query};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{io::Cursor, path::PathBuf};

const COOKIE: &str = "marcuru_admin";
type Res = Result<Response, (StatusCode, String)>;

/// Empty string = admin is open (no password). Set ADMIN_TOKEN to require one.
pub fn make_token() -> String {
    std::env::var("ADMIN_TOKEN").unwrap_or_default()
}

fn eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// What someone can do. The owner (the admin password) and trainers made admins can do everything.
/// Moderators get everything to do with trainers and vouches, and nothing to do with the cards, binders or home page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    Nothing,
    Moderator(i64),
    /// Holds the trainer's id, or nothing for the owner.
    Admin(Option<i64>),
}

pub fn power(app: &App, h: &HeaderMap) -> Power {
    if app.admin_token.is_empty() {
        return Power::Admin(None);
    }
    let cookie = |name: &str| -> Option<String> {
        h.get(header::COOKIE)?.to_str().ok()?.split(';').find_map(|p| p.trim().strip_prefix(&format!("{name}=")).map(str::to_owned))
    };
    if cookie(COOKIE).is_some_and(|t| eq(&t, &app.admin_token)) {
        return Power::Admin(None);
    }
    // A trainer's powers only ever count for requests made from this site itself.
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    let from_here = h.get(header::ORIGIN).and_then(|v| v.to_str().ok()).is_none_or(|o| o.split_once("://").map(|(_, x)| x) == Some(host));
    let Some(raw) = cookie("bbv_trainer").filter(|_| from_here) else { return Power::Nothing };
    let Ok(Some((id, role))) = db::open(&app.db_path).and_then(|c| crate::members::staff_from_session(&c, &raw)) else { return Power::Nothing };
    match role.as_str() {
        "admin" => Power::Admin(Some(id)),
        "mod" => Power::Moderator(id),
        _ => Power::Nothing,
    }
}

/// Everything, including editing the site.
pub fn authed(app: &App, h: &HeaderMap) -> bool {
    matches!(power(app, h), Power::Admin(_))
}

/// Trainers and vouches.
pub fn can_moderate(app: &App, h: &HeaderMap) -> bool {
    power(app, h) != Power::Nothing
}

fn wants_json(h: &HeaderMap) -> bool {
    h.get(header::ACCEPT).and_then(|v| v.to_str().ok()).map_or(false, |a| a.contains("application/json"))
}

fn json_resp(code: StatusCode, v: Value) -> Response {
    (code, [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], v.to_string()).into_response()
}
fn ok_json(v: Value) -> Response {
    json_resp(StatusCode::OK, v)
}
fn err_json(code: StatusCode, msg: &str) -> Response {
    json_resp(code, json!({ "error": msg }))
}
fn html(s: String) -> Response {
    let mut h = HeaderMap::new();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    (h, Html(s)).into_response()
}

/// JSON endpoints answer 401 as JSON; page endpoints fall back to the sign-in page.
fn deny(h: &HeaderMap) -> Response {
    if wants_json(h) {
        err_json(StatusCode::UNAUTHORIZED, "Your session expired — sign in again.")
    } else {
        Redirect::to("/admin").into_response()
    }
}

// ---------- templates ----------
#[derive(Template)]
#[template(path = "admin_login.html")]
struct Login {
    nav: Chrome,
    error: bool,
}

struct BinderOpt {
    handle: String,
    title: String,
    on: bool,
}

#[derive(Template)]
#[template(path = "admin.html")]
struct CardForm {
    nav: Chrome,
    tab: &'static str,
    editing: bool,
    id: String,
    title: String,
    variant: String,
    price: String,
    notes: String,
    tags: String,
    card_no: String,
    card_set: String,
    api_id: String,
    finish: String,
    price_updated: String,
    /// Where the price came from: '' (typed), 'market' or 'suggested'. Keeps a saved card's suggestion following its condition.
    price_src: String,
    image: String,
    binders: Vec<BinderOpt>,
    saved: String,
    total: i64,
}

#[derive(Template)]
#[template(path = "admin_cards.html")]
struct CardsPage {
    nav: Chrome,
    tab: &'static str,
    q: String,
    binder: String,
    sort: String,
    /// Different cards matching (copies of one card count once).
    total: i64,
    shown: usize,
    items: Vec<Product>,
    next: Option<usize>,
    binders: Vec<BinderOpt>,
    /// Whole collection, for the header plates.
    copies: String,
    distinct: String,
    value: String,
}

struct BlockRow {
    id: i64,
    kind: String,
    label: &'static str,
    summary: String,
    visible: bool,
    editable: bool,
    first: bool,
    last: bool,
    json: String,
}

#[derive(Template)]
#[template(path = "admin_page.html")]
struct PagePage {
    nav: Chrome,
    tab: &'static str,
    brand: String,
    blocks: Vec<BlockRow>,
    collections: String,
}

#[derive(Template)]
#[template(path = "admin_blocks.html")]
struct BlocksFrag {
    blocks: Vec<BlockRow>,
}

fn rd<T: Template>(t: T) -> Res {
    Ok(html(t.render().map_err(ise)?))
}

async fn login_page(app: &App, error: bool) -> Res {
    let nav = with_db(app, |c| chrome(c)).await?;
    rd(Login { nav, error })
}

// ---------- auth ----------
#[derive(Deserialize)]
pub struct LoginForm {
    token: String,
}

pub async fn login(State(app): State<App>, h: HeaderMap, Form(f): Form<LoginForm>) -> Res {
    if !eq(f.token.trim(), &app.admin_token) {
        return login_page(&app, true).await;
    }
    let https = h.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https");
    let cookie = format!("{COOKIE}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000{}", app.admin_token, if https { "; Secure" } else { "" });
    Ok(([(header::SET_COOKIE, cookie)], Redirect::to("/admin")).into_response())
}

pub async fn logout() -> impl IntoResponse {
    ([(header::SET_COOKIE, format!("{COOKIE}=; Path=/; Max-Age=0"))], Redirect::to("/"))
}

// ---------- cards: list ----------
#[derive(Deserialize, Default)]
pub struct ListQs {
    q: Option<String>,
    binder: Option<String>,
    page: Option<usize>,
    sort: Option<String>,
}

/// Binder checkboxes/pickers, in the owner's order and with the owner's names.
fn binder_opts(links: &[db::Collection], selected: &[String]) -> Vec<BinderOpt> {
    links.iter().map(|b| BinderOpt { handle: b.handle.clone(), title: b.title.clone(), on: selected.iter().any(|s| *s == b.handle) }).collect()
}

pub async fn cards(State(app): State<App>, h: HeaderMap, Qs(qs): Qs<ListQs>) -> Res {
    // The Admin button takes a moderator to the part they can use.
    if matches!(power(&app, &h), Power::Moderator(_)) {
        return Ok(Redirect::to("/admin/trainers").into_response());
    }
    if !authed(&app, &h) {
        return login_page(&app, false).await;
    }
    let q = qs.q.unwrap_or_default().trim().chars().take(80).collect::<String>();
    let binder = qs.binder.filter(|b| db::handle_ok(b)).unwrap_or_default();
    let page = qs.page.unwrap_or(0).min(500);
    let sort = qs.sort.filter(|s| ["price-desc", "price-asc", "title", "newest"].contains(&s.as_str())).unwrap_or_default();
    let query = Query { collection: (!binder.is_empty()).then(|| binder.clone()), q: q.clone(), sort: sort.clone(), page: 0, ..Default::default() };
    let (nav, items, total, copies, distinct, value) = with_db(&app, move |c| {
        // Pages accumulate so "Show more" simply extends the list.
        let mut items = vec![];
        let mut total = 0;
        for p in 0..=page {
            let (rows, t) = db::list(c, &Query { page: p, ..query.clone() })?;
            items.extend(rows);
            total = t;
        }
        Ok((chrome(c)?, items, total, db::total(c)?, db::distinct_cards(c)?, db::peso(db::total_value(c)?)))
    })
    .await?;
    let shown = items.len();
    let next = (shown < total as usize).then_some(page + 1);
    let binders = binder_opts(&nav.links, &[binder.clone()]);
    rd(CardsPage { nav, tab: "cards", q, binders, binder, sort, total, shown, items, next, copies: db::thousands(copies), distinct: db::thousands(distinct), value })
}

// ---------- cards: form ----------
#[derive(Deserialize, Default)]
pub struct FormQs {
    binders: Option<String>,
    saved: Option<String>,
}

async fn form_page(app: &App, edit: Option<Product>, qs: FormQs) -> Res {
    let id = edit.as_ref().map(|p| p.id);
    let (nav, total, current) = with_db(app, move |c| {
        let cur = match id {
            Some(id) => db::binders_of(c, id)?,
            None => vec![],
        };
        Ok((chrome(c)?, db::total(c)?, cur))
    })
    .await?;
    let preset: Vec<String> = qs.binders.unwrap_or_default().split(',').map(str::to_owned).collect();
    let selected = if edit.is_some() { current } else { preset };
    let p = edit.as_ref();
    let binders = binder_opts(&nav.links, &selected);
    rd(CardForm {
        nav,
        tab: "cards",
        editing: p.is_some(),
        id: p.map(|p| p.id.to_string()).unwrap_or_default(),
        title: p.map(|p| p.title.clone()).unwrap_or_default(),
        variant: p.map(|p| p.variant.clone()).unwrap_or_default(),
        price: p.filter(|p| p.price > 0.0).map(|p| format!("{}", p.price.round() as i64)).unwrap_or_default(),
        notes: p.map(|p| p.notes()).unwrap_or_default(),
        tags: p.map(|p| p.tags.clone()).unwrap_or_default(),
        card_no: p.map(|p| p.card_no.clone()).unwrap_or_default(),
        card_set: p.map(|p| p.card_set.clone()).unwrap_or_default(),
        api_id: p.map(|p| p.api_id.clone()).unwrap_or_default(),
        finish: p.map(|p| p.finish.clone()).unwrap_or_default(),
        price_updated: p.map(|p| p.price_updated.clone()).unwrap_or_default(),
        price_src: p.map(|p| p.price_src.clone()).unwrap_or_default(),
        image: p.filter(|p| p.has_image()).map(|p| p.img(360)).unwrap_or_default(),
        binders,
        saved: qs.saved.unwrap_or_default(),
        total,
    })
}

pub async fn new_card(State(app): State<App>, h: HeaderMap, Qs(qs): Qs<FormQs>) -> Res {
    if !authed(&app, &h) {
        return login_page(&app, false).await;
    }
    form_page(&app, None, qs).await
}

pub async fn edit(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    if !authed(&app, &h) {
        return login_page(&app, false).await;
    }
    match with_db(&app, move |c| db::product_by_id(c, id)).await? {
        Some(p) => form_page(&app, Some(p), FormQs::default()).await,
        None => Ok(Redirect::to("/admin").into_response()),
    }
}

fn parse_price(s: &str) -> f64 {
    s.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect::<String>().parse().unwrap_or(0.0)
}

pub fn open_image(bytes: Vec<u8>) -> Result<image::DynamicImage, String> {
    use image::{DynamicImage, ImageDecoder, ImageReader};
    let mut dec = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_decoder()
        .map_err(|e| format!("unsupported image: {e}"))?;
    let orient = dec.orientation().ok();
    let mut img = DynamicImage::from_decoder(dec).map_err(|e| e.to_string())?;
    if let Some(o) = orient {
        img.apply_orientation(o); // honour iPhone EXIF rotation
    }
    Ok(img)
}

/// Card photo: large + thumbnail JPEGs.
fn store_image(bytes: Vec<u8>, dir: PathBuf, stem: String) -> Result<String, String> {
    use image::{codecs::jpeg::JpegEncoder, imageops::FilterType};
    let img = open_image(bytes)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (suffix, max) in [("_l", 1400u32), ("_s", 480u32)] {
        let out = if img.width().max(img.height()) > max { img.resize(max, max, FilterType::Lanczos3) } else { img.clone() };
        let file = std::fs::File::create(dir.join(format!("{stem}{suffix}.jpg"))).map_err(|e| e.to_string())?;
        let mut w = std::io::BufWriter::new(file);
        JpegEncoder::new_with_quality(&mut w, 86).encode_image(&out.to_rgb8()).map_err(|e| e.to_string())?;
    }
    Ok(format!("/uploads/{stem}_l.jpg"))
}

/// Page-section image: keeps transparency (PNG) when the source has it, otherwise JPEG. Returns (path, w, h).
fn store_block_image(bytes: Vec<u8>, dir: PathBuf, stem: String) -> Result<(String, u32, u32), String> {
    use image::{imageops::FilterType, DynamicImage};
    let mut img = open_image(bytes)?;
    if img.width().max(img.height()) > 1200 {
        img = img.resize(1200, 1200, FilterType::Lanczos3);
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (name, out) = if img.color().has_alpha() { (format!("{stem}.png"), img.clone()) } else { (format!("{stem}.jpg"), DynamicImage::ImageRgb8(img.to_rgb8())) };
    out.save(dir.join(&name)).map_err(|e| e.to_string())?;
    Ok((format!("/uploads/{name}"), img.width(), img.height()))
}

/// Delete a picture only when nothing uses it any more: copies of a card share one file.
async fn remove_if_unused(app: &App, path: &str) -> Result<(), (StatusCode, String)> {
    let p = path.to_string();
    if !with_db(app, move |c| db::image_in_use(c, &p)).await? {
        remove_upload(&app.uploads, path);
    }
    Ok(())
}

/// Puts back pictures whose file has gone missing, from the card catalogue. Runs once at startup.
pub async fn heal_photos(app: &App) {
    let Ok(list) = with_db(app, db::catalogue_uploads).await else { return };
    let dir = PathBuf::from(&*app.uploads);
    let mut healed = 0;
    for (image, api_id) in list {
        let Some(name) = image.strip_prefix("/uploads/") else { continue };
        if name.contains('/') || name.contains("..") || dir.join(name).exists() {
            continue;
        }
        let Ok(Some(info)) = market::by_id_lang(&app.http, &app.fx, &api_id).await else { continue };
        let url = info["image_large"].as_str().unwrap_or("").to_string();
        if !market::image_host_ok(&url) {
            continue;
        }
        let Ok(r) = app.http.get(&url).send().await else { continue };
        if !r.status().is_success() {
            continue;
        }
        let Ok(bytes) = r.bytes().await else { continue };
        let (d, stem, data) = (dir.clone(), format!("c0-{}", nanos()), bytes.to_vec());
        let Ok(Ok(new)) = tokio::task::spawn_blocking(move || store_image(data, d, stem)).await else { continue };
        let old = image.clone();
        if with_db(app, move |c| db::replace_image(c, &old, &new)).await.is_ok() {
            healed += 1;
        }
    }
    // Catalogue cards that never got a picture take the catalogue's.
    for (id, api_id) in with_db(app, db::cards_without_picture).await.unwrap_or_default() {
        let Ok(Some(info)) = market::by_id_lang(&app.http, &app.fx, &api_id).await else { continue };
        let url = info["image_large"].as_str().or(info["image"].as_str()).unwrap_or("").to_string();
        if !market::image_host_ok(&url) {
            continue;
        }
        let Ok(r) = app.http.get(&url).send().await else { continue };
        if !r.status().is_success() {
            continue;
        }
        let Ok(bytes) = r.bytes().await else { continue };
        let (d, stem, data) = (dir.clone(), format!("c{id}-{}", nanos()), bytes.to_vec());
        let Ok(Ok(new)) = tokio::task::spawn_blocking(move || store_image(data, d, stem)).await else { continue };
        if with_db(app, move |c| db::set_picture(c, id, &new)).await.is_ok() {
            healed += 1;
        }
    }
    if healed > 0 {
        println!("Restored {healed} missing card picture(s) from the catalogue.");
        app.forget();
    }
}

/// Delete a replaced/removed upload (and its thumbnail). Only ever touches files inside the uploads folder.
pub fn remove_upload(dir: &str, path: &str) {
    let Some(name) = path.strip_prefix("/uploads/") else { return };
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return;
    }
    let base = PathBuf::from(dir);
    let _ = std::fs::remove_file(base.join(name));
    if let Some(stem) = name.strip_suffix("_l.jpg") {
        let _ = std::fs::remove_file(base.join(format!("{stem}_s.jpg")));
    }
}

/// Today's date as YYYY-MM-DD (UTC), without pulling in a date crate.
fn today() -> String {
    let days = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() / 86400) as i64;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

fn nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
}

struct Saved {
    id: i64,
    handle: String,
    title: String,
    price: f64,
    again: bool,
    binders: Vec<String>,
}

async fn save_inner(app: &App, mut mp: Multipart) -> Result<Saved, (StatusCode, String)> {
    let (mut id, mut title, mut variant, mut price, mut notes, mut tags) = (None::<i64>, String::new(), String::new(), String::new(), String::new(), String::new());
    let (mut binders, mut photo, mut again) = (Vec::<String>::new(), None::<Vec<u8>>, false);
    let (mut card_no, mut card_set, mut api_id, mut finish, mut photo_url, mut price_src) = (String::new(), String::new(), String::new(), String::new(), String::new(), String::new());
    let (mut rarity, mut solo, mut image_from) = (String::new(), false, None::<i64>);
    while let Some(f) = mp.next_field().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))? {
        let name = f.name().unwrap_or_default().to_string();
        if name == "photo" {
            let b = f.bytes().await.map_err(|e| (StatusCode::BAD_REQUEST, format!("Photo upload failed: {e}")))?;
            if !b.is_empty() {
                photo = Some(b.to_vec());
            }
            continue;
        }
        let t = f.text().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        match name.as_str() {
            "id" => id = t.trim().parse().ok(),
            "title" => title = t.trim().chars().take(200).collect(),
            "variant" => variant = t.trim().chars().take(80).collect(),
            "price" => price = t,
            "notes" => notes = t.chars().take(4000).collect(),
            "tags" => tags = t.trim().chars().take(300).collect(),
            "binder" => binders.push(t),
            "again" => again = t == "1",
            "card_no" => card_no = t.trim().chars().take(24).collect(),
            "card_set" => card_set = t.trim().chars().take(80).collect(),
            "api_id" => api_id = t.trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.' || *c == ':').take(40).collect(),
            "finish" => finish = t.trim().chars().take(40).collect(),
            "rarity" => rarity = t.trim().chars().take(40).collect(),
            "solo" => solo = t == "1",
            "image_from" => image_from = t.trim().parse().ok(),
            "photo_url" => photo_url = t.trim().to_string(),
            "price_src" => price_src = t.trim().to_string(),
            _ => {}
        }
    }
    // A vintage card's Unlimited picture is made here from the 1st Edition scan (the seal painted out).
    if photo.is_none() {
        if let Some(src) = unlimited_src(&photo_url) {
            if let Ok(b) = unlimited_bytes(app, &src, false).await {
                photo = Some(b);
            }
        }
    }
    // "Use the official image": fetched server-side, and only from known card-image hosts.
    if photo.is_none() && market::image_host_ok(&photo_url) {
        if let Ok(r) = app.http.get(&photo_url).send().await {
            if r.status().is_success() {
                if let Ok(b) = r.bytes().await {
                    if b.len() < 8_000_000 {
                        photo = Some(b.to_vec());
                    }
                }
            }
        }
    }
    if title.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Please enter the card's name.".into()));
    }
    let dir = PathBuf::from(&*app.uploads);
    let image = match photo {
        Some(bytes) => {
            let stem = format!("c{}-{}", id.unwrap_or(0), nanos());
            let r = tokio::task::spawn_blocking(move || store_image(bytes, dir, stem)).await.map_err(ise)?;
            Some(r.map_err(|e| (StatusCode::BAD_REQUEST, format!("That photo couldn't be read ({e}). Try a JPEG or PNG.")))?)
        }
        None => None,
    };
    let value = parse_price(&price);
    let price_updated = ((price_src == "market" || price_src == "suggested") && !api_id.is_empty()).then(today);
    let entry = Entry { id, title: title.clone(), variant, price: value, body: notes, tags, image, binders: binders.clone(), card_no, card_set, api_id, finish, price_updated, price_src, rarity, solo };
    let (id, handle) = with_db(app, move |c| db::save_entry(c, &entry)).await?;
    if let Some(from) = image_from {
        let old = with_db(app, move |c| db::product_by_id(c, id)).await?.map(|p| p.image).unwrap_or_default();
        with_db(app, move |c| db::share_image(c, id, from)).await?;
        remove_if_unused(app, &old).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}")))?;
    }
    app.forget();
    Ok(Saved { id, handle, title, price: value, again, binders })
}

/// Saves a card. JS clients (Accept: application/json) get JSON; plain form posts get a redirect.
pub async fn save(State(app): State<App>, h: HeaderMap, mp: Multipart) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let json = wants_json(&h);
    match save_inner(&app, mp).await {
        Ok(s) if json => Ok(ok_json(json!({ "ok": true, "id": s.id, "handle": s.handle, "title": s.title, "price": db::peso(s.price) }))),
        Ok(s) if s.again => {
            let t: String = s.title.chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
            Ok(Redirect::to(&format!("/admin/new?binders={}&saved={}", s.binders.join(","), t.replace(' ', "+"))).into_response())
        }
        Ok(s) => Ok(Redirect::to(&format!("/products/{}", s.handle)).into_response()),
        Err((code, msg)) if json => Ok(err_json(code, &msg)),
        Err(e) => Err(e),
    }
}

#[derive(Deserialize)]
pub struct CheckQs {
    title: String,
    id: Option<i64>,
}

/// Duplicate guard: lists existing cards whose name matches what is being typed.
pub async fn check(State(app): State<App>, h: HeaderMap, Qs(q): Qs<CheckQs>) -> Res {
    if !authed(&app, &h) {
        return Ok(StatusCode::UNAUTHORIZED.into_response());
    }
    let t = q.title.trim().to_string();
    let hits = if t.len() < 3 { vec![] } else { with_db(&app, move |c| db::similar(c, &t, q.id.unwrap_or(0))).await? };
    let items: Vec<_> = hits.into_iter().map(|(id, title)| json!({ "id": id, "title": title })).collect();
    Ok(ok_json(json!({ "similar": items })))
}

pub async fn delete(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let old = with_db(&app, move |c| db::product_by_id(c, id)).await?.map(|p| p.image).unwrap_or_default();
    with_db(&app, move |c| db::delete_entry(c, id)).await?;
    remove_if_unused(&app, &old).await?;
    app.forget();
    Ok(if wants_json(&h) { ok_json(json!({ "ok": true })) } else { Redirect::to("/admin").into_response() })
}

/// Replace a card's photo from the list (no need to open the edit form).
pub async fn photo(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, mut mp: Multipart) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let mut bytes = None;
    while let Some(f) = mp.next_field().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))? {
        if f.name() == Some("photo") {
            bytes = Some(f.bytes().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?.to_vec());
        }
    }
    let Some(bytes) = bytes.filter(|b| !b.is_empty()) else {
        return Ok(err_json(StatusCode::BAD_REQUEST, "No photo received."));
    };
    let dir = PathBuf::from(&*app.uploads);
    let stem = format!("c{id}-{}", nanos());
    let old = with_db(&app, move |c| db::product_by_id(c, id)).await?.map(|p| p.image).unwrap_or_default();
    match tokio::task::spawn_blocking(move || store_image(bytes, dir, stem)).await.map_err(ise)? {
        Ok(path) => {
            let p2 = path.clone();
            with_db(&app, move |c| blocks::set_product_image(c, id, Some(&p2))).await?;
            remove_if_unused(&app, &old).await?;
            app.forget();
            Ok(ok_json(json!({ "ok": true, "src": path.replace("_l.jpg", "_s.jpg") })))
        }
        Err(e) => Ok(err_json(StatusCode::BAD_REQUEST, &format!("That photo couldn't be read ({e}). Try a JPEG or PNG."))),
    }
}

pub async fn photo_remove(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let old = with_db(&app, move |c| db::product_by_id(c, id)).await?.map(|p| p.image).unwrap_or_default();
    with_db(&app, move |c| blocks::set_product_image(c, id, None)).await?;
    remove_if_unused(&app, &old).await?;
    app.forget();
    Ok(ok_json(json!({ "ok": true })))
}

// ---------- page sections ----------
pub fn label(kind: &str) -> &'static str {
    match kind {
        "hero" => "Big banner at the top",
        "ticker" => "Scrolling words",
        "stats" => "Number boxes",
        "binders" => "Binder pictures",
        "row" => "Row of cards",
        "feature" => "Picture banner",
        "text" => "Text",
        _ => "Section",
    }
}

fn block_rows(bs: &[Block], cols: &[marcuru_rs::db::Collection]) -> Vec<BlockRow> {
    let n = bs.len();
    bs.iter()
        .enumerate()
        .map(|(i, b)| {
            let t = |k: &str| b.data[k].as_str().unwrap_or("").to_string();
            let summary = match b.kind.as_str() {
                "hero" => t("title"),
                "ticker" => t("phrases").lines().next().unwrap_or("").to_string(),
                "binders" => format!("{} · automatic", t("title")),
                "stats" => "Cards · Graded · Binders · Value — automatic".into(),
                "row" => {
                    let h = t("handle");
                    let name = cols.iter().find(|c| c.handle == h).map(|c| c.title.clone()).unwrap_or_else(|| "All cards".into());
                    let title = t("title");
                    format!("{}{}", if title.is_empty() { name } else { title }, if b.data["spotlight"].as_bool().unwrap_or(false) { " · highlighted" } else { "" })
                }
                "feature" => t("title"),
                "text" => t("title"),
                _ => String::new(),
            };
            BlockRow {
                id: b.id,
                kind: b.kind.clone(),
                label: label(&b.kind),
                summary,
                visible: b.visible,
                editable: b.kind != "stats",
                first: i == 0,
                last: i + 1 == n,
                json: b.data.to_string(),
            }
        })
        .collect()
}

#[derive(Deserialize, Default)]
pub struct PageQs {
    frag: Option<String>,
}

pub async fn page_editor(State(app): State<App>, h: HeaderMap, Qs(qs): Qs<PageQs>) -> Res {
    if !authed(&app, &h) {
        return login_page(&app, false).await;
    }
    let (nav, mut bs, brand) = with_db(&app, |c| Ok((chrome(c)?, blocks::blocks(c)?, blocks::setting(c, "brand")?))).await?;
    bs.retain(|b| b.kind != "stats");
    let rows = block_rows(&bs, &nav.links);
    if qs.frag.is_some() {
        return rd(BlocksFrag { blocks: rows });
    }
    let mut cols: Vec<Value> = vec![json!({ "handle": "all", "title": "All cards" })];
    cols.extend(nav.links.iter().map(|c| json!({ "handle": c.handle, "title": c.title })));
    let brand = brand.filter(|b| !b.trim().is_empty()).unwrap_or_else(|| "Big Boys Vault".into());
    rd(PagePage { nav, tab: "page", brand, blocks: rows, collections: Value::Array(cols).to_string() })
}

#[derive(Deserialize)]
pub struct SettingsForm {
    brand: String,
}

pub async fn settings(State(app): State<App>, h: HeaderMap, Form(f): Form<SettingsForm>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let brand: String = f.brand.trim().chars().take(40).collect();
    if brand.is_empty() {
        return Ok(err_json(StatusCode::BAD_REQUEST, "The site name can't be empty."));
    }
    with_db(&app, move |c| blocks::set_setting(c, "brand", &brand)).await?;
    app.forget();
    Ok(ok_json(json!({ "ok": true })))
}

/// The "Condition prices" panel: `{ "shares": { "Lightly Played": "80", ... } }` in percent, or `{ "reset": true }`.
#[derive(Deserialize)]
pub struct ConditionSharesReq {
    #[serde(default)]
    shares: std::collections::HashMap<String, serde_json::Value>,
    #[serde(default)]
    reset: bool,
}

/// Only the owner and full admins can change how much each condition is worth.
pub async fn settings_condition_shares(State(app): State<App>, h: HeaderMap, Json(f): Json<ConditionSharesReq>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    if f.reset {
        let shares = with_db(&app, move |c| marcuru_rs::pricing::save(c, None)).await?;
        app.forget();
        return Ok(ok_json(json!({ "ok": true, "shares": shares })));
    }
    let mut map = std::collections::HashMap::new();
    for (cond, _) in marcuru_rs::pricing::DEFAULT_SHARES {
        let Some(v) = f.shares.get(cond) else { continue };
        let pct = match v {
            serde_json::Value::Number(n) => n.as_f64(),
            serde_json::Value::String(s) => s.trim().trim_end_matches('%').trim().parse::<f64>().ok(),
            _ => None,
        };
        match pct {
            Some(p) if p.is_finite() && (0.0..=200.0).contains(&p) => {
                map.insert(cond.to_string(), (p * 10.0).round() / 1000.0);
            }
            _ => return Ok(json_resp(StatusCode::BAD_REQUEST, json!({ "error": format!("{cond}: type a number from 0 to 200.") }))),
        }
    }
    if map.is_empty() {
        return Ok(json_resp(StatusCode::BAD_REQUEST, json!({ "error": "Nothing to save." })));
    }
    let shares = with_db(&app, move |c| marcuru_rs::pricing::save(c, Some(&map))).await?;
    app.forget();
    Ok(ok_json(json!({ "ok": true, "shares": shares })))
}

#[derive(Deserialize)]
pub struct AddForm {
    kind: String,
    /// Put the new section right after this one (else at the end).
    after: Option<i64>,
    /// For a card row: which binder it shows.
    handle: Option<String>,
}

pub async fn block_add(State(app): State<App>, h: HeaderMap, Form(f): Form<AddForm>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    if !blocks::KINDS.contains(&f.kind.as_str()) || f.kind == "stats" {
        return Ok(err_json(StatusCode::BAD_REQUEST, "Unknown section type."));
    }
    let id = with_db(&app, move |c| {
        let mut data = blocks::default_data(&f.kind);
        if f.kind == "row" {
            if let Some(h) = f.handle.as_deref().filter(|h| *h == "all" || db::binder_exists(c, h)) {
                data["handle"] = json!(h);
                data["title"] = json!("");
            }
        }
        blocks::add_block_at(c, &f.kind, &data, f.after)
    })
    .await?;
    app.forget();
    Ok(ok_json(json!({ "ok": true, "id": id })))
}

#[derive(Deserialize)]
pub struct MoveForm {
    dir: String,
}

pub async fn block_move(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): Form<MoveForm>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    with_db(&app, move |c| blocks::move_block(c, id, f.dir == "up")).await?;
    app.forget();
    Ok(ok_json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct VisibleForm {
    on: String,
}

pub async fn block_visible(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Form(f): Form<VisibleForm>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    with_db(&app, move |c| blocks::set_visible(c, id, f.on == "1")).await?;
    app.forget();
    Ok(ok_json(json!({ "ok": true })))
}

pub async fn block_delete(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    with_db(&app, move |c| blocks::delete_block(c, id)).await?;
    app.forget();
    Ok(ok_json(json!({ "ok": true })))
}

/// Which keys each kind of block may edit (anything else posted is ignored).
fn text_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        "hero" => &["eyebrow", "title", "subtitle", "button_label", "button_link", "sticker"],
        "ticker" => &["phrases"],
        "binders" => &["title"],
        "row" => &["handle", "title", "eyebrow", "sort"],
        "feature" => &["eyebrow", "title", "subtitle", "button_label", "button_link"],
        "text" => &["title", "body"],
        "showcase" | "highlights" | "vouches" => &["title", "eyebrow"],
        _ => &[],
    }
}
fn image_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        "hero" => &["img_a", "img_b"],
        "feature" => &["img"],
        _ => &[],
    }
}

pub async fn block_save(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, mut mp: Multipart) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let Some(blk) = with_db(&app, move |c| blocks::block(c, id)).await? else {
        return Ok(err_json(StatusCode::NOT_FOUND, "That section no longer exists."));
    };
    let mut fields = std::collections::HashMap::<String, String>::new();
    let mut files = std::collections::HashMap::<String, Vec<u8>>::new();
    while let Some(f) = mp.next_field().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))? {
        let name = f.name().unwrap_or_default().to_string();
        if image_keys(&blk.kind).contains(&name.as_str()) {
            let b = f.bytes().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            if !b.is_empty() {
                files.insert(name, b.to_vec());
            }
        } else {
            fields.insert(name, f.text().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?);
        }
    }
    let mut data = blk.data.clone();
    for k in text_keys(&blk.kind) {
        if let Some(v) = fields.get(*k) {
            let v = v.trim();
            let v = match *k {
                "button_link" => safe_link(v),
                "body" => v.chars().take(2000).collect(),
                "phrases" => v.chars().take(1500).collect(),
                _ => v.chars().take(200).collect(),
            };
            data[*k] = json!(v);
        }
    }
    if blk.kind == "stats" {
        const KEYS: [&str; 4] = ["cards", "graded", "binders", "value"];
        if let Some(v) = fields.get("items") {
            let mut items: Vec<&str> = vec![];
            for k in v.split(',').map(str::trim) {
                if KEYS.contains(&k) && !items.contains(&k) {
                    items.push(k);
                }
            }
            data["items"] = json!(items);
        }
        for k in KEYS {
            if let Some(l) = fields.get(&format!("label_{k}")) {
                if !data["labels"].is_object() {
                    data["labels"] = json!({});
                }
                data["labels"][k] = json!(l.trim().chars().take(40).collect::<String>());
            }
        }
    }
    if blk.kind == "showcase" {
        if let Some(v) = fields.get("ids") {
            let ids: Vec<i64> = v.split(',').filter_map(|x| x.trim().parse().ok()).take(24).collect();
            data["ids"] = json!(ids);
        }
    }
    if blk.kind == "highlights" {
        if let Some(v) = fields.get("order") {
            let ids: Vec<i64> = v.split(',').filter_map(|x| x.trim().parse().ok()).take(60).collect();
            data["order"] = json!(ids);
        }
    }
    if blk.kind == "row" {
        if let Some(v) = fields.get("spotlight") {
            data["spotlight"] = json!(v == "1");
        }
    }
    let mut trash: Vec<String> = vec![];
    for k in image_keys(&blk.kind) {
        let (kw, kh) = (format!("{k}_w"), format!("{k}_h"));
        let old = data[*k].as_str().unwrap_or("").to_string();
        if fields.get(&format!("rm_{k}")).map(String::as_str) == Some("1") {
            trash.push(old.clone());
            data[*k] = json!("");
            data[&kw] = json!(0);
            data[&kh] = json!(0);
        }
        if let Some(bytes) = files.remove(*k) {
            let dir = PathBuf::from(&*app.uploads);
            let stem = format!("blk{id}-{k}-{}", nanos());
            match tokio::task::spawn_blocking(move || store_block_image(bytes, dir, stem)).await.map_err(ise)? {
                Ok((path, w, h)) => {
                    // Hero characters are fitted into the same 250×286 box so any picture lands at a consistent scale.
                    let (dw, dh) = if blk.kind == "hero" {
                        let f = (250.0 / w as f64).min(286.0 / h as f64);
                        ((w as f64 * f).round() as i64, (h as f64 * f).round() as i64)
                    } else {
                        (w as i64, h as i64)
                    };
                    trash.push(old.clone());
                    data[*k] = json!(path);
                    data[&kw] = json!(dw);
                    data[&kh] = json!(dh);
                }
                Err(e) => return Ok(err_json(StatusCode::BAD_REQUEST, &format!("That picture couldn't be read ({e}). Try a PNG or JPEG."))),
            }
        }
    }
    let d2 = data.clone();
    with_db(&app, move |c| blocks::save_block(c, id, &d2)).await?;
    for t in trash {
        remove_upload(&app.uploads, &t);
    }
    app.forget();
    Ok(ok_json(json!({ "ok": true })))
}

// ---------- market price lookup ----------
#[derive(Deserialize)]
pub struct PriceQs {
    #[serde(default)]
    name: String,
    #[serde(default)]
    number: String,
    /// An exact card ("tcgdex:sv08-057"), e.g. one tapped in quick add.
    id: Option<String>,
}

/// Looks up today's market value (in pesos) for a card by name + number, or by exact card id.
/// Never blocks saving: failures come back as a hint.
pub async fn price(State(app): State<App>, h: HeaderMap, Qs(q): Qs<PriceQs>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let key = match &q.id {
        Some(id) => format!("id|{id}"),
        None => format!("{}|{}", q.name.trim().to_lowercase(), q.number.trim().to_lowercase()),
    };
    if let Some(hit) = app.prices.get(&key) {
        return Ok(ok_json((*hit).clone()));
    }
    let found = match &q.id {
        Some(id) => match market::by_id_lang(&app.http, &app.fx, id).await {
            Ok(m) => {
                let rate = market::fx(&app.http, &app.fx).await.map(|f| f.usd_php).unwrap_or(0.0);
                Ok(json!({ "matches": m.into_iter().collect::<Vec<_>>(), "rate": rate, "exact": true }))
            }
            Err(e) => Err(e),
        },
        None => market::search(&app.http, &app.fx, &q.name, &q.number).await,
    };
    match found {
        Ok(v) => {
            if v["matches"].as_array().map_or(false, |m| !m.is_empty()) {
                app.prices.insert(key, std::sync::Arc::new(v.clone()));
            }
            Ok(ok_json(v))
        }
        Err(e) => Ok(ok_json(json!({ "matches": [], "error": e }))),
    }
}

// ---------- quick add: search by picture, tap into a tray, save all ----------
#[derive(Template)]
#[template(path = "admin_quick.html")]
struct QuickPage {
    nav: Chrome,
    tab: &'static str,
    /// "card": search all printings by name. "set": pick a set, then tap cards from its full list.
    mode: &'static str,
    total: i64,
    binders: Vec<BinderOpt>,
}

async fn quick_page(app: &App, h: &HeaderMap, mode: &'static str) -> Res {
    if !authed(app, h) {
        return login_page(app, false).await;
    }
    let (nav, total) = with_db(app, |c| Ok((chrome(c)?, db::total(c)?))).await?;
    let binders = binder_opts(&nav.links, &[]);
    rd(QuickPage { nav, tab: if mode == "set" { "sets" } else { "quick" }, mode, total, binders })
}

pub async fn quick(State(app): State<App>, h: HeaderMap) -> Res {
    quick_page(&app, &h, "card").await
}

pub async fn by_set(State(app): State<App>, h: HeaderMap) -> Res {
    quick_page(&app, &h, "set").await
}

/// Adds `owned`: how many of each card the collection already holds. A vintage card split into printings
/// (1st Edition, Unlimited...) counts each printing on its own: by the printing named in the card's title, or for cards
/// saved before printings were told apart, by its finish (a 1st Edition finish, or not).
async fn mark_owned(app: &App, v: &mut Value) -> Result<(), (StatusCode, String)> {
    let api_of = |c: &Value| c["api"].as_str().or(c["id"].as_str()).map(str::to_string);
    let mut ids: Vec<String> = v["cards"].as_array().map(|a| a.iter().filter_map(api_of).collect()).unwrap_or_default();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Ok(());
    }
    let rows = with_db(app, move |c| {
        let marks = vec!["?"; ids.len()].join(",");
        let mut st = c.prepare(&format!("SELECT api_id, COALESCE(finish, ''), title FROM products WHERE api_id IN ({marks})"))?;
        let rows = st.query_map(rusqlite::params_from_iter(ids.iter()), |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })
    .await?;
    const NAMED: [&str; 3] = ["(1st Edition", "(Unlimited", "(Shadowless"];
    if let Some(arr) = v["cards"].as_array_mut() {
        for c in arr {
            let Some(api) = api_of(c) else { continue };
            let edition = c["edition"].as_str().unwrap_or("").to_string();
            let n = rows.iter().filter(|(a, finish, title)| {
                if *a != api {
                    return false;
                }
                if edition.is_empty() {
                    return true;
                }
                if title.contains(&format!("({edition})")) {
                    return true;
                }
                let named = NAMED.iter().any(|w| title.contains(w));
                !named && ((edition == "1st Edition" && finish.starts_with("1stEdition")) || (edition == "Unlimited" && !finish.starts_with("1stEdition")))
            }).count();
            c["owned"] = json!(n);
        }
    }
    Ok(())
}

/// The scan an Unlimited picture is made from, out of its address on this site.
fn unlimited_src(url: &str) -> Option<String> {
    let q = url.strip_prefix("/admin/cardimg/unlimited?")?;
    let src = q.split('&').find_map(|kv| kv.strip_prefix("src="))?.replace("%3A", ":").replace("%2F", "/");
    (src.starts_with("https://assets.tcgdex.net/") && src.ends_with("/high.png") && !src.contains("..")).then_some(src)
}

/// A vintage card's Unlimited picture: the 1st Edition scan with its seal painted out, made once and kept
/// (full size and a small one for the set's grid).
pub async fn unlimited_bytes(app: &App, src: &str, small: bool) -> Result<Vec<u8>, String> {
    use sha2::Digest;
    if !(src.starts_with("https://assets.tcgdex.net/") && src.ends_with("/high.png") && !src.contains("..")) {
        return Err("not a card picture".into());
    }
    let name = format!("{:x}", sha2::Sha256::digest(src.as_bytes()))[..24].to_string();
    let dir = PathBuf::from(&*app.uploads).join("unl");
    let file = |s: bool| dir.join(format!("{name}{}.jpg", if s { "_s" } else { "" }));
    if let Ok(b) = tokio::fs::read(file(small)).await {
        return Ok(b);
    }
    let r = app.http.get(src).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("status {}", r.status()));
    }
    let bytes = r.bytes().await.map_err(|e| e.to_string())?.to_vec();
    let (d, n) = (dir.clone(), name.clone());
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        use image::{codecs::jpeg::JpegEncoder, imageops::FilterType};
        let mut img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?.to_rgba8();
        marcuru_rs::stamp::erase(&mut img);
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
        let thumb = image::imageops::resize(&rgb, 300, (300 * rgb.height()) / rgb.width().max(1), FilterType::Lanczos3);
        for (img, suffix) in [(&rgb, ""), (&thumb, "_s")] {
            // Written aside and moved into place, so a half-written file is never served.
            let tmp = d.join(format!("{n}{suffix}.tmp{}", std::process::id()));
            let mut out = std::io::BufWriter::new(std::fs::File::create(&tmp).map_err(|e| e.to_string())?);
            JpegEncoder::new_with_quality(&mut out, 90).encode_image(img).map_err(|e| e.to_string())?;
            drop(out);
            std::fs::rename(&tmp, d.join(format!("{n}{suffix}.jpg"))).map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    tokio::fs::read(file(small)).await.map_err(|e| e.to_string())
}

#[derive(Deserialize)]
pub struct UnlimitedQs {
    src: String,
    #[serde(default)]
    size: String,
}

/// Admin › Add by set: the Unlimited picture of a vintage card.
pub async fn cardimg_unlimited(State(app): State<App>, h: HeaderMap, Qs(q): Qs<UnlimitedQs>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let src = q.src.replace("%3A", ":").replace("%2F", "/");
    match unlimited_bytes(&app, &src, q.size == "low").await {
        Ok(b) => Ok(([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "private, max-age=604800")], b).into_response()),
        Err(e) => Ok((StatusCode::NOT_FOUND, e).into_response()),
    }
}

#[derive(Deserialize)]
pub struct CatalogQs {
    #[serde(default)]
    q: String,
    /// en (default), ja, zh-tw, zh-cn
    #[serde(default)]
    lang: String,
}

/// Card pictures matching a name (all printings), plus how many of each the collection already holds.
pub async fn catalog(State(app): State<App>, h: HeaderMap, Qs(q): Qs<CatalogQs>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let mut v = match market::catalog(&app.http, &q.q, &q.lang).await {
        Ok(v) => v,
        Err(e) => return Ok(ok_json(json!({ "cards": [], "error": e }))),
    };
    mark_owned(&app, &mut v).await?;
    Ok(ok_json(v))
}

/// Sets matching a name ("30th", "ascended", "sv08"), newest first.
pub async fn set_search(State(app): State<App>, h: HeaderMap, Qs(q): Qs<CatalogQs>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    Ok(ok_json(market::set_search(&app.http, &q.q, &q.lang).await))
}

#[derive(Deserialize)]
pub struct SetQs {
    id: String,
    #[serde(default)]
    lang: String,
}

/// Every card in a set, with how many of each the collection already holds.
pub async fn set_cards(State(app): State<App>, h: HeaderMap, Qs(q): Qs<SetQs>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let mut v = match market::set_cards(&app.http, &q.id, &q.lang).await {
        Ok(v) => v,
        Err(e) => return Ok(ok_json(json!({ "cards": [], "error": e }))),
    };
    mark_owned(&app, &mut v).await?;
    // A vintage set's Unlimited pictures start being made now, a few at a time, so they're ready as the grid scrolls.
    let mut srcs: Vec<String> = v["cards"].as_array().map(|a| a.iter().filter_map(|c| c["image_large"].as_str().and_then(unlimited_src)).collect()).unwrap_or_default();
    srcs.sort();
    srcs.dedup();
    if !srcs.is_empty() {
        let app = app.clone();
        tokio::spawn(async move {
            let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(4));
            let mut jobs = tokio::task::JoinSet::new();
            for src in srcs {
                let (app, gate) = (app.clone(), gate.clone());
                jobs.spawn(async move {
                    let _p = gate.acquire_owned().await;
                    let _ = unlimited_bytes(&app, &src, true).await;
                });
            }
            while jobs.join_next().await.is_some() {}
        });
    }
    Ok(ok_json(v))
}

// ---------- quick edits right on My cards ----------
#[derive(Deserialize)]
pub struct QuickEdit {
    ids: Vec<i64>,
    price: Option<f64>,
    price_src: Option<String>,
    notes: Option<String>,
    condition: Option<String>,
    tags: Option<Vec<String>>,
}

/// Price, description or condition, for every copy of a card, without opening the edit form.
pub async fn quick_edit(State(app): State<App>, h: HeaderMap, Json(req): Json<QuickEdit>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    if req.ids.is_empty() || req.ids.len() > 200 || req.price.is_some_and(|v| !v.is_finite() || !(0.0..=1_000_000_000.0).contains(&v)) {
        return Ok(err_json(StatusCode::BAD_REQUEST, "That price doesn't look right."));
    }
    let notes: Option<String> = req.notes.map(|n| n.trim().chars().take(4000).collect());
    let condition: Option<String> = req.condition.map(|v| v.trim().chars().take(80).collect());
    if condition.as_deref().is_some_and(str::is_empty) {
        return Ok(err_json(StatusCode::BAD_REQUEST, "Please pick a condition."));
    }
    let tags = req.tags.map(|t| db::clean_tags(&t));
    let shown = tags.clone();
    let (ids, price, price_src) = (req.ids, req.price, req.price_src);
    // The tile's grade badge follows the condition, so a card set to PSA 10 shows it straight away.
    let grade = with_db(&app, move |c| {
        db::quick_edit(c, &ids, price, price_src.as_deref(), notes.as_deref(), condition.as_deref(), tags.as_deref())?;
        Ok(db::product_by_id(c, ids[0])?.and_then(|p| p.grade()))
    })
    .await?;
    app.forget();
    Ok(ok_json(json!({ "ok": true, "price": price.map(db::peso), "grade": grade, "tags": shown })))
}

// ---------- bulk edits ----------
#[derive(Deserialize)]
pub struct Scope {
    q: Option<String>,
    binder: Option<String>,
}

#[derive(Deserialize)]
pub struct BulkReq {
    action: String,
    ids: Option<Vec<i64>>,
    scope: Option<Scope>,
    binders: Option<Vec<String>>,
    mode: Option<String>,
    text: Option<String>,
    number: Option<f64>,
    snapshot: Option<Vec<bulk::Snap>>,
}

pub async fn bulk(State(app): State<App>, h: HeaderMap, Json(req): Json<BulkReq>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    // Resolve the selection: explicit ids, or "everything matching this search".
    let mut ids: Vec<i64> = match (&req.scope, &req.ids) {
        (Some(sc), _) => {
            let binder = sc.binder.clone().filter(|b| db::handle_ok(b));
            let query = Query { collection: binder, q: sc.q.clone().unwrap_or_default().chars().take(80).collect(), ..Default::default() };
            with_db(&app, move |c| db::ids(c, &query)).await?
        }
        (None, Some(v)) => v.iter().copied().take(5000).collect(),
        _ => vec![],
    };
    ids.sort_unstable();
    ids.dedup();
    let n = ids.len();
    if req.action == "restore" {
        let snaps = req.snapshot.unwrap_or_default();
        let k = with_db(&app, move |c| bulk::restore(c, &snaps)).await?;
        app.forget();
        return Ok(ok_json(json!({ "ok": true, "n": k })));
    }
    if ids.is_empty() {
        return Ok(err_json(StatusCode::BAD_REQUEST, "Select at least one card first."));
    }
    if req.action == "delete" {
        let ids2 = ids.clone();
        let images = with_db(&app, move |c| bulk::delete_many(c, &ids2)).await?;
        for img in images {
            remove_if_unused(&app, &img).await?;
        }
        app.forget();
        return Ok(ok_json(json!({ "ok": true, "n": n })));
    }
    // Everything else is undoable: remember how the cards looked first.
    let ids2 = ids.clone();
    let undo = with_db(&app, move |c| bulk::snapshot(c, &ids2)).await?;
    let mut skipped = 0usize;
    let mut capped = false;
    match req.action.as_str() {
        "binder" => {
            let (bs, mode) = (req.binders.unwrap_or_default(), req.mode.unwrap_or_else(|| "add".into()));
            if bs.is_empty() {
                return Ok(err_json(StatusCode::BAD_REQUEST, "Pick at least one binder."));
            }
            with_db(&app, move |c| bulk::binder_op(c, &ids, &bs, &mode)).await?;
        }
        "grade" => {
            let t: String = req.text.unwrap_or_default().trim().chars().take(80).collect();
            with_db(&app, move |c| bulk::set_variant(c, &ids, &t)).await?;
        }
        "set_value" => {
            let v = req.number.filter(|v| v.is_finite() && *v >= 0.0).ok_or((StatusCode::BAD_REQUEST, "Enter a value.".to_string()))?;
            with_db(&app, move |c| bulk::set_price(c, &ids, v)).await?;
        }
        "adjust_value" => {
            let pct = req.number.filter(|v| v.is_finite()).ok_or((StatusCode::BAD_REQUEST, "Enter a percentage.".to_string()))?.clamp(-99.0, 1000.0);
            with_db(&app, move |c| bulk::adjust_price(c, &ids, pct)).await?;
        }
        "market" => {
            let ids2 = ids.clone();
            let mut cards = with_db(&app, move |c| bulk::priced_cards(c, &ids2)).await?;
            skipped = n - cards.len();
            if cards.len() > 40 {
                capped = true;
                cards.truncate(40); // be polite to the free price service; run again for the rest
            }
            let mut updates: Vec<(i64, f64, String)> = vec![];
            let mut failed = 0usize;
            for (id, api_id, finish) in cards {
                match market::by_id_lang(&app.http, &app.fx, &api_id).await {
                    Ok(Some(card)) => {
                        let fin = card["finishes"].as_array().cloned().unwrap_or_default();
                        let pick = fin.iter().find(|f| f["key"].as_str() == Some(finish.as_str())).or_else(|| fin.first());
                        if let Some(f) = pick {
                            updates.push((id, f["php"].as_f64().unwrap_or(0.0), f["key"].as_str().unwrap_or("").to_string()));
                        } else {
                            skipped += 1;
                        }
                    }
                    Ok(None) => skipped += 1,
                    Err(_) => {
                        skipped += 1;
                        failed += 1;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            }
            if updates.is_empty() && failed > 0 {
                return Ok(err_json(StatusCode::BAD_GATEWAY, "The price service didn't answer. Try again in a moment."));
            }
            let k = updates.len();
            let day = today();
            with_db(&app, move |c| bulk::apply_market(c, &updates, &day)).await?;
            app.forget();
            return Ok(ok_json(json!({ "ok": true, "n": k, "skipped": skipped, "capped": capped, "undo": undo })));
        }
        _ => return Ok(err_json(StatusCode::BAD_REQUEST, "Unknown action.")),
    }
    app.forget();
    Ok(ok_json(json!({ "ok": true, "n": n, "skipped": skipped, "undo": undo })))
}

// ---------- binders: name, cover, home page ----------
#[derive(Template)]
#[template(path = "admin_binders.html")]
struct BindersPage {
    nav: Chrome,
    tab: &'static str,
    /// Every binder as JSON; the page draws the two grids (home page / hidden) from it.
    data: String,
}

fn binder_json(c: &db::Collection) -> Value {
    json!({ "handle": c.handle, "title": c.title, "image": c.img(480), "count": c.count, "on_home": c.on_home, "custom_cover": c.custom_cover })
}

pub async fn binders_page(State(app): State<App>, h: HeaderMap) -> Res {
    if !authed(&app, &h) {
        return login_page(&app, false).await;
    }
    let nav = with_db(&app, |c| chrome(c)).await?;
    let data = Value::Array(nav.links.iter().map(binder_json).collect()).to_string();
    rd(BindersPage { nav, tab: "binders", data })
}

async fn known_binder(app: &App, handle: &str) -> Result<bool, (StatusCode, String)> {
    let h = handle.to_string();
    with_db(app, move |c| Ok(db::binder_exists(c, &h))).await
}

/// A binder cover the site uploaded for the owner (never a card's own photo), so it can be cleaned up when replaced.
fn own_cover(path: &str) -> bool {
    path.starts_with("/uploads/b-")
}

#[derive(Deserialize)]
pub struct BinderReq {
    title: Option<String>,
    on_home: Option<bool>,
    /// -1 = up, +1 = down
    #[serde(rename = "move")]
    move_by: Option<i64>,
    /// "auto" = the most valuable card; otherwise the id of a card in the collection whose photo becomes the cover.
    cover: Option<String>,
    /// Remove the binder (its cards stay in the collection).
    #[serde(default)]
    delete: bool,
}

/// Rename, show/hide on the home page, reorder, or pick a cover. Returns every binder so the page can redraw.
pub async fn binder_update(State(app): State<App>, h: HeaderMap, Path(handle): Path<String>, Json(req): Json<BinderReq>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    if !known_binder(&app, &handle).await? {
        return Ok(err_json(StatusCode::NOT_FOUND, "That binder doesn't exist."));
    }
    let title = match req.title.as_deref().map(str::trim) {
        Some("") => return Ok(err_json(StatusCode::BAD_REQUEST, "A binder needs a name.")),
        Some(t) => Some(t.chars().take(40).collect::<String>()),
        None => None,
    };
    let uploads = app.uploads.to_string();
    let (links, old) = with_db(&app, move |c| {
        if req.delete {
            let old = db::delete_binder(c, &handle)?;
            return Ok((db::collections(c)?, Some(old)));
        }
        if let Some(t) = &title {
            db::rename_binder(c, &handle, t)?;
        }
        if let Some(on) = req.on_home {
            db::set_binder_home(c, &handle, on)?;
        }
        if let Some(d) = req.move_by {
            db::move_binder(c, &handle, d.signum())?;
        }
        let mut old = None;
        match req.cover.as_deref() {
            Some("auto") => old = Some(db::set_binder_cover(c, &handle, "")?),
            Some(id) => {
                if let Some(p) = id.parse::<i64>().ok().and_then(|id| db::product_by_id(c, id).ok().flatten()).filter(|p| p.has_image()) {
                    old = Some(db::set_binder_cover(c, &handle, &p.image)?);
                }
            }
            None => {}
        }
        Ok((db::collections(c)?, old))
    })
    .await?;
    if let Some(old) = old.filter(|o| own_cover(o)) {
        remove_upload(&uploads, &old);
    }
    app.forget();
    Ok(ok_json(json!({ "ok": true, "binders": links.iter().map(binder_json).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
pub struct NewBinder {
    title: String,
}

/// Add a binder (at the end of the list, shown on the home page). Returns every binder plus the new one's handle.
pub async fn binder_create(State(app): State<App>, h: HeaderMap, Json(req): Json<NewBinder>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let title: String = req.title.trim().chars().take(40).collect();
    if title.is_empty() {
        return Ok(err_json(StatusCode::BAD_REQUEST, "Give the new binder a name."));
    }
    let (handle, links) = with_db(&app, move |c| {
        let h = db::create_binder(c, &title)?;
        Ok((h, db::collections(c)?))
    })
    .await?;
    app.forget();
    Ok(ok_json(json!({ "ok": true, "handle": handle, "binders": links.iter().map(binder_json).collect::<Vec<_>>() })))
}

/// Photos of the cards in a binder, most valuable first, to choose a cover from.
pub async fn binder_covers(State(app): State<App>, h: HeaderMap, Path(handle): Path<String>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    if !known_binder(&app, &handle).await? {
        return Ok(ok_json(json!({ "cards": [] })));
    }
    let rows = with_db(&app, move |c| db::binder_cover_choices(c, &handle, i64::MAX)).await?;
    let cards: Vec<Value> = rows.iter().map(|(id, title, image)| json!({ "id": id, "title": title, "image": db::cdn(image, 360) })).collect();
    Ok(ok_json(json!({ "cards": cards })))
}

/// Upload a cover photo for a binder (any photo from the phone, not only a card).
pub async fn binder_cover_upload(State(app): State<App>, h: HeaderMap, Path(handle): Path<String>, mut mp: Multipart) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    if !known_binder(&app, &handle).await? {
        return Ok(err_json(StatusCode::NOT_FOUND, "That binder doesn't exist."));
    }
    let mut bytes = None;
    while let Some(f) = mp.next_field().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))? {
        if f.name() == Some("photo") {
            let b = f.bytes().await.map_err(|e| (StatusCode::BAD_REQUEST, format!("Photo upload failed: {e}")))?;
            if !b.is_empty() {
                bytes = Some(b.to_vec());
            }
        }
    }
    let Some(bytes) = bytes else { return Ok(err_json(StatusCode::BAD_REQUEST, "Choose a photo first.")) };
    let dir = PathBuf::from(&*app.uploads);
    let stem = format!("b-{}-{}", handle.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>(), nanos());
    let path = match tokio::task::spawn_blocking(move || store_image(bytes, dir, stem)).await.map_err(ise)? {
        Ok(p) => p,
        Err(e) => return Ok(err_json(StatusCode::BAD_REQUEST, &format!("That photo couldn't be read ({e}). Try a JPEG or PNG."))),
    };
    let (links, old) = with_db(&app, move |c| {
        let old = db::set_binder_cover(c, &handle, &path)?;
        Ok((db::collections(c)?, old))
    })
    .await?;
    if own_cover(&old) {
        remove_upload(&app.uploads, &old);
    }
    app.forget();
    Ok(ok_json(json!({ "ok": true, "binders": links.iter().map(binder_json).collect::<Vec<_>>() })))
}

// ---------- drag-and-drop ordering ----------
#[derive(Deserialize)]
pub struct BlockOrder {
    ids: Vec<i64>,
}

pub async fn blocks_order(State(app): State<App>, h: HeaderMap, Json(req): Json<BlockOrder>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let ids: Vec<i64> = req.ids.into_iter().take(500).collect();
    with_db(&app, move |c| blocks::reorder(c, &ids)).await?;
    app.forget();
    Ok(ok_json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct BinderOrder {
    handles: Vec<String>,
    /// When given: exactly these binders are shown on the home page (drag between "home page" and "hidden").
    home: Option<Vec<String>>,
}

pub async fn binders_order(State(app): State<App>, h: HeaderMap, Json(req): Json<BinderOrder>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let hs: Vec<String> = req.handles.into_iter().filter(|h| db::handle_ok(h)).take(500).collect();
    let home = req.home;
    let links = with_db(&app, move |c| {
        db::reorder_binders(c, &hs)?;
        if let Some(home) = &home {
            for h in &hs {
                db::set_binder_home(c, h, home.contains(h))?;
            }
        }
        db::collections(c)
    })
    .await?;
    app.forget();
    Ok(ok_json(json!({ "ok": true, "binders": links.iter().map(binder_json).collect::<Vec<_>>() })))
}

// ---------- card picker (showcase) ----------
#[derive(Deserialize)]
pub struct PickQs {
    #[serde(default)]
    q: String,
    /// Newest first (Quick add's "recently added").
    #[serde(default)]
    recent: bool,
    /// Exact cards to show (the ones already picked), comma-separated ids.
    #[serde(default)]
    ids: String,
}

/// Cards from the owner's own collection: search results, or the given ids in order.
pub async fn pick(State(app): State<App>, h: HeaderMap, Qs(q): Qs<PickQs>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let ids: Vec<i64> = q.ids.split(',').filter_map(|x| x.trim().parse().ok()).take(48).collect();
    let text: String = q.q.trim().chars().take(80).collect();
    let recent = q.recent;
    let items = with_db(&app, move |c| {
        if !ids.is_empty() {
            return Ok(ids.iter().filter_map(|id| db::product_by_id(c, *id).ok().flatten()).collect::<Vec<_>>());
        }
        let (rows, _) = db::list(c, &Query { q: text.clone(), sort: if recent || !text.is_empty() { String::new() } else { "price-desc".into() }, ..Default::default() })?;
        Ok(rows)
    })
    .await?;
    let cards: Vec<Value> = items.iter().map(|p| json!({ "id": p.id, "title": p.title, "image": p.img(360), "price": p.price_text(), "qty": p.qty, "api_id": p.api_id })).collect();
    Ok(ok_json(json!({ "cards": cards })))
}

// ---------- the site's top menu ----------
#[derive(Deserialize)]
pub struct MenuReq {
    handles: Vec<String>,
}

/// Which binders show in the top menu, and in what order (drag and drop in the Home page editor).
pub async fn menu_save(State(app): State<App>, h: HeaderMap, Json(req): Json<MenuReq>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let hs: Vec<String> = req.handles.into_iter().filter(|h| db::handle_ok(h)).take(60).collect();
    with_db(&app, move |c| db::set_menu(c, &hs)).await?;
    app.forget();
    Ok(ok_json(json!({ "ok": true })))
}

/// One more copy of a card (the "+" on a card in My cards).
pub async fn dup(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    match with_db(&app, move |c| db::duplicate(c, id)).await? {
        Some(nid) => {
            app.forget();
            Ok(ok_json(json!({ "ok": true, "id": nid })))
        }
        None => Ok(err_json(StatusCode::NOT_FOUND, "That card no longer exists.")),
    }
}

// ---------- the dashboard ----------
#[derive(Template)]
#[template(path = "admin_dashboard.html")]
struct DashboardPage {
    nav: Chrome,
    tab: &'static str,
    value: String,
    copies: String,
    distinct: String,
    average: String,
    median: String,
    top10: u32,
    week_n: i64,
    week_value: String,
    month_n: i64,
    month_value: String,
    insights: Option<marcuru_rs::insights::Insights>,
    /// (day, cards, bar height in %)
    days: Vec<(String, i64, u32)>,
    days_first: String,
    /// (title, handle, cards, value, bar width in %)
    binders: Vec<(String, String, i64, String, u32)>,
    viewed: Vec<(Product, i64)>,
    /// (term, times, bar width in %)
    searches: Vec<(String, i64, u32)>,
    missing_searches: Vec<(String, i64)>,
    no_photo_n: i64,
    no_photo: Vec<Product>,
    no_price_n: i64,
    no_price: Vec<Product>,
    cond_notes_n: usize,
    cond_notes: Vec<(i64, String, String, String)>,
    trainers: i64,
    pending: i64,
    vouches_week: i64,
    chat_week: i64,
    /// (id, name, value, stars, bar width in %)
    top_trainers: Vec<(i64, String, String, i64, u32)>,
    /// Array of condition shares: (condition_name, percentage_as_number)
    condition_shares: Vec<(String, u32)>,
}

fn share(part: f64, whole: f64) -> u32 {
    if whole <= 0.0 { 0 } else { ((part / whole) * 100.0).round().clamp(2.0, 100.0) as u32 }
}

pub async fn dashboard(State(app): State<App>, h: HeaderMap) -> Res {
    if matches!(power(&app, &h), Power::Moderator(_)) {
        return Ok(Redirect::to("/admin/trainers").into_response());
    }
    if !authed(&app, &h) {
        return login_page(&app, false).await;
    }
    let week_ago = crate::unix_now() - 7 * 86_400;
    let page = with_db(&app, move |c| {
        let nav = chrome(c)?;
        let (_, cards) = vault_search::parse(&db::catalogue(c)?);
        let insights = marcuru_rs::insights::read(&cards);
        let total_value: f64 = cards.iter().map(|x| x.price * f64::from(x.qty.max(1))).sum();
        let copies: u32 = cards.iter().map(|x| x.qty.max(1)).sum();
        let mut each: Vec<f64> = cards.iter().flat_map(|x| std::iter::repeat_n(x.price, x.qty.max(1) as usize)).collect();
        each.sort_by(|a, b| b.total_cmp(a));
        let top10 = share(each.iter().take(10).sum(), total_value);
        let (week_n, week_v) = db::added_since(c, "-7 days")?;
        let (month_n, month_v) = db::added_since(c, "-30 days")?;
        let per_day = db::added_per_day(c, 30)?;
        let busiest = per_day.iter().map(|d| d.1).max().unwrap_or(0).max(1) as f64;
        let days_first = per_day.first().map(|d| d.0.clone()).unwrap_or_default();
        let days = per_day.into_iter().map(|(d, n)| { let h = if n == 0 { 0 } else { ((n as f64 / busiest) * 100.0).max(6.0) as u32 }; (d, n, h) }).collect();
        let bvals = db::binder_values(c)?;
        let top_b = bvals.first().map_or(1.0, |b| b.3);
        let binders = bvals.into_iter().map(|(t, hd, n, v)| (t, hd, n, db::peso(v), share(v, top_b))).collect();
        let viewed = db::most_viewed(c, 8)?;
        let logged = db::top_searches(c, 40)?;
        let most = logged.first().map_or(1.0, |s| s.1 as f64);
        // Searches that find nothing in the vault right now: worth stocking.
        let missing_searches: Vec<(String, i64)> = logged.iter().filter(|(term, _)| vault_search::search(&cards, &vault_search::read_words(term), vault_search::Sort::Newest).hits.is_empty()).take(12).cloned().collect();
        let searches = logged.into_iter().take(10).map(|(t, n)| (t, n, share(n as f64, most))).collect();
        let (no_photo_n, no_photo) = db::missing(c, "photo", 8)?;
        let (no_price_n, no_price) = db::missing(c, "price", 8)?;
        let all_notes = db::condition_notes(c)?;
        let cond_notes_n = all_notes.len();
        let cond_notes = all_notes.into_iter().take(8).collect();
        let (trainers, pending, vouches_week, chat_week) = marcuru_rs::members::community_numbers(c, week_ago)?;
        let mut people = marcuru_rs::members::approved(c)?;
        people.sort_by(|a, b| b.worth.total_cmp(&a.worth));
        let richest = people.first().map_or(1.0, |m| m.worth);
        let top_trainers = people.into_iter().filter(|m| m.worth > 0.0).take(6).map(|m| (m.id, m.name.clone(), db::peso(m.worth), m.stars, share(m.worth, richest))).collect();
        let average = if copies > 0 { db::peso(total_value / f64::from(copies)) } else { db::peso(0.0) };
        let median = insights.as_ref().map(|i| i.median.clone()).unwrap_or_else(|| db::peso(0.0));
        let mut condition_shares = Vec::new();
        let shares = marcuru_rs::pricing::load_shares(c);
        for (cond, _) in marcuru_rs::pricing::DEFAULT_SHARES {
            let pct = shares.get(cond).copied().unwrap_or(1.0);
            condition_shares.push((cond.to_string(), (pct * 100.0).round() as u32));
        }
        Ok(DashboardPage {
            nav, tab: "dash", value: db::peso(total_value), copies: db::thousands(i64::from(copies)), distinct: db::thousands(cards.len() as i64), average, median, top10,
            week_n, week_value: db::peso(week_v), month_n, month_value: db::peso(month_v), insights, days, days_first, binders, viewed, searches, missing_searches,
            no_photo_n, no_photo, no_price_n, no_price, cond_notes_n, cond_notes, trainers, pending, vouches_week, chat_week, top_trainers, condition_shares,
        })
    })
    .await?;
    rd(page)
}

/// The dashboard's one-tap fix: conditions typed into descriptions move into the condition.
pub async fn fix_conditions(State(app): State<App>, h: HeaderMap) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    let moved = with_db(&app, db::move_condition_notes).await?;
    app.forget();
    Ok(ok_json(serde_json::json!({ "ok": true, "moved": moved })))
}

#[derive(Deserialize)]
pub struct OrderReq {
    groups: Vec<Vec<i64>>,
    /// A home page row that was sorted another way: these cards go to the very top of your order, in this order.
    #[serde(default)]
    top: bool,
}

/// My cards: the owner dragged cards into a new order.
pub async fn order(State(app): State<App>, h: HeaderMap, Json(req): Json<OrderReq>) -> Res {
    if !authed(&app, &h) {
        return Ok(deny(&h));
    }
    if req.groups.is_empty() || req.groups.len() > 5000 || req.groups.iter().any(|g| g.is_empty() || g.len() > 500) {
        return Ok(err_json(StatusCode::BAD_REQUEST, "That order didn't come through. Please try again."));
    }
    let (groups, top) = (req.groups, req.top);
    with_db(&app, move |c| if top { db::groups_to_top(c, &groups) } else { db::reorder_groups(c, &groups) }).await?;
    app.forget();
    Ok(ok_json(json!({ "ok": true })))
}
