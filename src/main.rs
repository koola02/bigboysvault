mod admin;
mod api;
mod community;
mod feed;
mod look;
mod messages;
mod ops;
mod store;
mod vault_ai;

use askama::Template;
use axum::{
    extract::{Path, Query as Qs, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use marcuru_rs::blocks::{self, Block};
use marcuru_rs::db::{self, Collection, Product, Query, PAGE_SIZE};
use marcuru_rs::{market, members, suggest};
use moka::sync::Cache;
use serde::Deserialize;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tower_http::{compression::CompressionLayer, services::ServeDir, set_header::SetResponseHeaderLayer};

/// Shared state. The render cache is bounded by *bytes* (weigher) and expires on a TTL,
/// so memory use is capped no matter how many distinct queries arrive. Any admin write clears it.
#[derive(Clone)]
struct App {
    db_path: Arc<String>,
    uploads: Arc<String>,
    admin_token: Arc<String>,
    cache: Cache<String, Arc<String>>,
    http: reqwest::Client,
    fx: Arc<market::FxCache>,
    prices: Cache<String, Arc<Value>>,
    /// Card catalogue searches, kept for a day (the catalogue changes rarely and the service is slow).
    catalog: Cache<String, Arc<Value>>,
    sindex: Cache<u8, Arc<suggest::Index>>,
    /// Recent attempts per visitor or name, for sign-ins, sign-ups and searches. Forgotten after 15 minutes.
    limits: Cache<String, u32>,
    chat: Arc<community::ChatHub>,
    /// Private messages: who's waiting for news, call signals, who's online (messages.rs).
    dm: Arc<messages::DmHub>,
    /// Goes up on every change. A page or catalogue worked out before the latest change is never kept,
    /// so a slow request that started just before a save can't put old cards back into the cache.
    changes: Arc<std::sync::atomic::AtomicU64>,
}

impl App {
    /// Something changed: everything cached is out of date.
    fn forget(&self) {
        self.changes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.cache.invalidate_all();
    }
    fn change_count(&self) -> u64 {
        self.changes.load(std::sync::atomic::Ordering::SeqCst)
    }
    /// Keeps a freshly worked-out entry, unless something changed while it was being made.
    fn keep(&self, key: String, value: Arc<String>, since: u64) {
        if self.change_count() == since {
            self.cache.insert(key, value);
        }
    }
}

/// Site chrome shared by every page: the editable site name and the (automatic) category links.
#[derive(Clone)]
struct Chrome {
    brand: String,
    links: Vec<Collection>,
    /// Copyright years for the footer: "2026", or "2026–2028" once the site is older.
    years: String,
    /// The top menu: binders the owner put there, in the owner's order.
    menu: Vec<Collection>,
    /// Chrome origin-trial tokens (the ORIGIN_TRIALS setting, comma-separated). They switch on trial-only browser features.
    trials: Vec<String>,
    /// Trainers waiting for the owner's approval (shown on the admin tab).
    pending: i64,
    /// Orders of the vault's cards waiting for an answer (shown on the admin tab).
    orders: i64,
    /// The site's public address, for links that leave the page (search engines, link previews).
    site: String,
    /// What search engines and link previews read about this page. A page without a canonical path isn't indexed.
    seo: Seo,
}

/// What search engines and link previews read about one page.
#[derive(Default, Clone)]
struct Seo {
    /// The whole <title>. Empty: the template's own.
    title: String,
    desc: String,
    /// The page's one true address, e.g. "/products/pikachu". Empty means "don't index this page".
    path: String,
    image: String,
    /// og:type: "website", "product", "profile".
    kind: &'static str,
    /// Structured data (JSON-LD), ready to print.
    ld: String,
}

/// JSON-LD that's safe inside a <script> tag.
fn ld(v: serde_json::Value) -> String {
    v.to_string().replace("</", "<\\/")
}

/// The site's address: PUBLIC_URL, or the live domain.
fn site() -> String {
    std::env::var("PUBLIC_URL").ok().filter(|u| u.starts_with("http")).unwrap_or_else(|| "https://www.bigboysvault.com".into()).trim_end_matches('/').to_string()
}

/// Cut text to about `n` characters at a word, for descriptions.
fn clip(t: &str, n: usize) -> String {
    let t: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= n {
        return t;
    }
    let mut out: String = t.chars().take(n).collect();
    if let Some(i) = out.rfind(' ') {
        out.truncate(i);
    }
    format!("{}…", out.trim_end_matches([',', '.', ';', ':', '-', '·']))
}

impl Chrome {
    fn indexed(&self) -> bool {
        !self.seo.path.is_empty()
    }
    fn abs(&self, u: &str) -> String {
        if u.starts_with("http://") || u.starts_with("https://") {
            u.to_string()
        } else if u.starts_with("//") {
            format!("https:{u}")
        } else {
            format!("{}{}", self.site, u)
        }
    }
    fn canonical(&self) -> String {
        self.abs(&self.seo.path)
    }
    fn page_title(&self) -> String {
        if self.seo.title.is_empty() { format!("{} · Pokémon cards, graded slabs & collectibles", self.brand) } else { self.seo.title.clone() }
    }
    fn description(&self) -> String {
        if self.seo.desc.is_empty() {
            format!("{}: a Pokémon card and collectibles vault in the Philippines. Graded slabs, Japanese cards, sealed product and figures, priced in pesos.", self.brand)
        } else {
            self.seo.desc.clone()
        }
    }
    fn og_image(&self) -> String {
        self.abs(if self.seo.image.is_empty() { "/static/img/icon-512.png" } else { &self.seo.image })
    }
    fn og_type(&self) -> &'static str {
        if self.seo.kind.is_empty() { "website" } else { self.seo.kind }
    }
    /// The name is drawn as a two-colour lockup: everything before the last word, then the last word ("Big Boys" + "Vault").
    /// One-word names are drawn plain.
    fn split(&self) -> bool {
        self.lockup().is_some()
    }
    fn lockup(&self) -> Option<(String, String)> {
        let b = self.brand.trim();
        let i = b.rfind(char::is_whitespace)?;
        let (l, r) = (b[..i].trim(), b[i..].trim());
        (!l.is_empty() && !r.is_empty()).then(|| (l.to_string(), r.to_string()))
    }
    fn left(&self) -> String {
        self.lockup().map(|l| l.0).unwrap_or_default()
    }
    fn right(&self) -> String {
        self.lockup().map(|l| l.1).unwrap_or_default()
    }
}

fn chrome(c: &rusqlite::Connection) -> rusqlite::Result<Chrome> {
    Ok(Chrome {
        brand: blocks::setting(c, "brand")?.filter(|b| !b.trim().is_empty()).unwrap_or_else(|| "Big Boys Vault".into()),
        links: db::collections(c)?,
        years: copyright_years(),
        menu: vec![],
        pending: members::pending_count(c)?,
        orders: marcuru_rs::sellers::vault_orders_open(c).unwrap_or(0),
        trials: std::env::var("ORIGIN_TRIALS").unwrap_or_default().split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
        site: site(),
        seo: Seo::default(),
    })
    .map(|mut ch| {
        let mut m: Vec<Collection> = ch.links.iter().filter(|x| x.in_menu).cloned().collect();
        m.sort_by_key(|x| x.menu_pos);
        ch.menu = m;
        ch
    })
}

/// The site went live in 2026. Later years show as a range, the way professional sites do.
fn copyright_years() -> String {
    const LAUNCHED: i64 = 2026;
    let days = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) / 86_400) as i64;
    let year = 1970 + ((days as f64 + 0.5) / 365.2425) as i64;
    if year > LAUNCHED { format!("{LAUNCHED}–{year}") } else { LAUNCHED.to_string() }
}

type Res = Result<Response, (StatusCode, String)>;
/// Something broke on the server. The real reason goes to the log; the visitor gets plain words, never technical text.
pub(crate) fn ise<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    tracing::error!("{e}");
    (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong on our side. Please try again in a moment.".into())
}

/// Run blocking SQLite work off the async reactor, on a fresh connection.
async fn with_db<T: Send + 'static>(
    app: &App,
    f: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T> + Send + 'static,
) -> Result<T, (StatusCode, String)> {
    let path = app.db_path.clone();
    // Pages rendered on the database thread need to know the look too (see look.rs).
    let new_look = look::is_new();
    tokio::task::spawn_blocking(move || {
        look::in_thread(new_look);
        db::open(&path).and_then(|c| f(&c))
    })
        .await
        .map_err(ise)?
        .map_err(ise)
}

fn render<T: Template>(t: T) -> Result<String, (StatusCode, String)> {
    t.render().map_err(ise)
}

fn page(html: String) -> Response {
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    // Always revalidate: edits made in /admin must show up immediately. The server-side cache keeps it fast.
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-cache"));
    h.insert(header::VARY, HeaderValue::from_static("HX-Request"));
    (h, Html(html)).into_response()
}

/// Cache-aside: hit returns the stored HTML; miss renders once.
async fn cached<F, Fut>(app: &App, key: String, f: F) -> Res
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<String, (StatusCode, String)>>,
{
    // The two looks never share a kept page.
    let key = if look::is_new() { format!("fb|{key}") } else { key };
    if let Some(hit) = app.cache.get(&key) {
        return Ok(page((*hit).clone()));
    }
    let since = app.change_count();
    match f().await {
        Ok(html) => {
            app.keep(key, Arc::new(html.clone()), since);
            Ok(page(html))
        }
        // Not-found pages are real HTML with a 404 status, and are never cached.
        Err((StatusCode::NOT_FOUND, html)) => Ok((StatusCode::NOT_FOUND, page(html)).into_response()),
        Err(e) => Err(e),
    }
}

// ---------- templates ----------
/// One renderable section of the home page. Fields are a superset; each kind uses its own.
#[derive(Default)]
struct BlockView {
    kind: String,
    /// Editor only: which section this is, its saved settings (JSON) and a plain-words name.
    id: i64,
    visible: bool,
    json: String,
    label: String,
    eyebrow: String,
    title: String,
    subtitle: String,
    body: String,
    button_label: String,
    button_link: String,
    img_a: String,
    img_a_w: i64,
    img_a_h: i64,
    img_b: String,
    img_b_w: i64,
    img_b_h: i64,
    img: String,
    img_w: i64,
    img_h: i64,
    link: String,
    spotlight: bool,
    count: i64,
    items: Vec<Product>,
    binders: Vec<Collection>,
    /// The binders shelf: each binder with the photos of its first page of cards (up to nine pockets).
    shelf: Vec<(Collection, Vec<String>)>,
    nbinders: usize,
    total: i64,
    graded: i64,
    value: String,
    total_fmt: String,
    count_fmt: String,
    phrases: Vec<String>,
    sticker: String,
    /// Number boxes: (key, label, value) in the owner's order; removed boxes aren't listed.
    stats: Vec<(String, String, String)>,
    /// Community sections: only what the owner has approved.
    highlights: Vec<members::Highlight>,
    insights: Option<marcuru_rs::insights::Insights>,
    trainer_count: i64,
    trainer_faces: Vec<members::Member>,
    vouches: Vec<members::Vouch>,
}

#[derive(Template)]
#[template(path = "home.html")]
struct Home {
    nav: Chrome,
    blocks: Vec<BlockView>,
    /// The owner's editor: the real home page with every part editable in place.
    edit: bool,
    collections: String,
    tab: &'static str,
}

#[derive(Template)]
#[template(path = "binder_part.html")]
struct BinderPart {
    title: String,
    handle: String,
    total: i64,
    /// Different cards (one tile each), for counting pages of nine.
    tiles: i64,
    tone: u8,
    fan: Vec<Product>,
    items: Vec<Product>,
    next: Option<usize>,
}

#[derive(Template)]
#[template(path = "binder_more.html")]
struct BinderMore {
    handle: String,
    items: Vec<Product>,
    next: Option<usize>,
}

#[derive(Template)]
#[template(path = "collection.html")]
struct CollectionPage {
    title: String,
    base: String,
    q: String,
    sort: String,
    in_stock: bool,
    total: i64,
    items: Vec<Product>,
    next: Option<usize>,
    nav: Chrome,
}

#[derive(Template)]
#[template(path = "grid.html")]
struct Grid {
    base: String,
    q: String,
    sort: String,
    in_stock: bool,
    items: Vec<Product>,
    next: Option<usize>,
}

#[derive(Template)]
#[template(path = "product.html")]
struct ProductPage {
    p: Product,
    related: Vec<Product>,
    nav: Chrome,
    /// How many copies of this card are in the vault.
    copies: i64,
    versions: Vec<db::Version>,
    /// The share of the vault that costs less (0–100).
    standing: u8,
    /// How the market price moved lately, once there are two days of history.
    trend: Option<marcuru_rs::history::Trend>,
}

/// The same card details, without the site around them, for the floating card window.
#[derive(Template)]
#[template(path = "product_part.html")]
struct ProductPart {
    p: Product,
    related: Vec<Product>,
    copies: i64,
    versions: Vec<db::Version>,
    standing: u8,
    trend: Option<marcuru_rs::history::Trend>,
}

#[derive(Template)]
#[template(path = "404.html")]
struct NotFound {
    nav: Chrome,
}

fn st(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}
fn num(v: &Value, k: &str) -> i64 {
    v[k].as_i64().unwrap_or(0)
}
fn fill(s: &str, total: i64) -> String {
    s.replace("{count}", &db::thousands(total))
}
/// Only same-site paths or http(s) links are allowed for buttons.
fn safe_link(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('/') && !s.starts_with("//") || s.starts_with("https://") || s.starts_with("http://") {
        s.to_string()
    } else {
        "/all".into()
    }
}

fn view(c: &rusqlite::Connection, b: &Block, nav: &Chrome, binders: &[Collection], total: i64, graded: i64, value: &str) -> rusqlite::Result<BlockView> {
    let d = &b.data;
    let mut v = BlockView { kind: b.kind.clone(), total, total_fmt: db::thousands(total), graded, value: value.into(), nbinders: binders.len(), ..Default::default() };
    v.id = b.id;
    v.visible = b.visible;
    v.json = d.to_string();
    v.label = admin::label(&b.kind).to_string();
    match b.kind.as_str() {
        "hero" => {
            v.eyebrow = st(d, "eyebrow");
            v.title = fill(&st(d, "title"), total);
            v.subtitle = fill(&st(d, "subtitle"), total);
            v.button_label = fill(&st(d, "button_label"), total);
            v.button_link = safe_link(&st(d, "button_link"));
            v.sticker = st(d, "sticker");
            v.img_a = st(d, "img_a");
            v.img_a_w = num(d, "img_a_w");
            v.img_a_h = num(d, "img_a_h");
            v.img_b = st(d, "img_b");
            v.img_b_w = num(d, "img_b_w");
            v.img_b_h = num(d, "img_b_h");
            v.items = db::orbit_cards(c, 14)?;
        }
        "ticker" => {
            v.phrases = st(d, "phrases").lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).take(24).collect();
        }
        "binders" => {
            v.title = st(d, "title");
            v.binders = binders.to_vec();
            for b in binders {
                let (items, _) = db::list(c, &Query { collection: Some(b.handle.clone()), ..Default::default() })?;
                v.shelf.push((b.clone(), items.iter().filter(|p| p.has_image()).take(9).map(|p| p.img(240)).collect()));
            }
        }
        "row" => {
            let h = st(d, "handle");
            let col = (!h.is_empty() && h != "all").then(|| h.clone());
            let sort = match st(d, "sort").as_str() {
                "price-desc" => "price-desc",
                "price-asc" => "price-asc",
                "title" => "title",
                "newest" => "newest",
                _ => "",
            };
            let (items, count) = db::list(c, &Query { collection: col.clone(), sort: sort.into(), ..Default::default() })?;
            v.items = items.into_iter().take(12).collect();
            let _ = count;
            let count = db::cards_in(c, col.as_deref())?;
            v.count = count;
            v.count_fmt = db::thousands(count);
            let t = st(d, "title");
            v.title = if !t.is_empty() {
                t
            } else {
                col.as_ref().and_then(|h| nav.links.iter().find(|x| &x.handle == h)).map(|x| x.title.clone()).unwrap_or_else(|| "All cards".into())
            };
            v.link = col.map(|h| format!("/collections/{h}")).unwrap_or_else(|| "/all".into());
            v.eyebrow = st(d, "eyebrow");
            v.spotlight = d["spotlight"].as_bool().unwrap_or(false);
        }
        "feature" => {
            v.eyebrow = st(d, "eyebrow");
            v.title = fill(&st(d, "title"), total);
            v.subtitle = fill(&st(d, "subtitle"), total);
            v.button_label = fill(&st(d, "button_label"), total);
            v.button_link = safe_link(&st(d, "button_link"));
            v.img = st(d, "img");
            v.img_w = num(d, "img_w");
            v.img_h = num(d, "img_h");
        }
        "text" => {
            v.title = st(d, "title");
            v.body = st(d, "body");
        }
        "stats" => {
            // Only what the vault still holds: sold copies stay in the table as a record but aren't counted.
            let cards = admin::vault_cards(c)?;
            let mut i = marcuru_rs::insights::read(&cards);
            if let Some(x) = i.as_mut() {
                x.newest = admin::newest_arrival(c)?.as_ref().map(marcuru_rs::insights::spot_of);
            }
            v.insights = i;
            // Which boxes show, in what order, with what words: all owner-editable.
            const ALL: [(&str, &str); 4] = [("cards", "Cards"), ("graded", "Graded"), ("binders", "Binders"), ("value", "Collection value")];
            let order: Vec<String> = d["items"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_else(|| ALL.iter().map(|x| x.0.to_string()).collect());
            for k in order.iter().filter(|k| ALL.iter().any(|a| a.0 == k.as_str())) {
                let default = ALL.iter().find(|a| a.0 == k.as_str()).map(|a| a.1).unwrap_or("");
                let label = d["labels"][k.as_str()].as_str().filter(|l| !l.trim().is_empty()).unwrap_or(default).to_string();
                let val = match k.as_str() {
                    "cards" => v.total_fmt.clone(),
                    "graded" => graded.to_string(),
                    "binders" => v.nbinders.to_string(),
                    _ => v.value.clone(),
                };
                v.stats.push((k.clone(), label, val));
            }
        }
        "showcase" => {
            v.title = st(d, "title");
            v.eyebrow = st(d, "eyebrow");
            let ids: Vec<i64> = d["ids"].as_array().map(|a| a.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default();
            v.items = ids.iter().filter_map(|id| db::product_by_id(c, *id).ok().flatten()).collect();
            v.count = v.items.len() as i64;
        }
        "highlights" => {
            v.title = st(d, "title");
            v.eyebrow = st(d, "eyebrow");
            // The owner's order from the home page editor first; trainers not placed yet follow, newest first.
            let order: Vec<i64> = d["order"].as_array().map(|a| a.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default();
            let mut hl = members::approved_highlights(c, 60)?;
            hl.sort_by_key(|h| order.iter().position(|id| *id == h.member.id).unwrap_or(usize::MAX));
            hl.truncate(12);
            v.highlights = hl;
            v.trainer_count = members::approved_count(c)?;
            v.trainer_faces = members::approved_faces(c, 5)?;
        }
        "vouches" => {
            v.title = st(d, "title");
            v.eyebrow = st(d, "eyebrow");
            v.vouches = members::public_vouches(c, None, None, 6)?;
        }
        _ => {}
    }
    // The editor names each part by what it shows ("Binder · Playables"), not by its type.
    let short = |t: &str| -> String { let t: String = t.chars().take(36).collect(); t };
    v.label = match b.kind.as_str() {
        "hero" => format!("Top banner · {}", short(&st(d, "title"))),
        "row" => format!("Binder · {}", v.title),
        "binders" => "Binders slider".to_string(),
        "feature" => format!("Picture banner · {}", short(&st(d, "title"))),
        "stats" => "Numbers".to_string(),
        "ticker" => "Moving text".to_string(),
        "text" => format!("Text · {}", short(&v.title)),
        "showcase" => format!("Showcase · {} card{}", v.count, if v.count == 1 { "" } else { "s" }),
        "highlights" => "Trainers' picks".to_string(),
        "vouches" => "Vouches".to_string(),
        _ => v.label.clone(),
    };
    Ok(v)
}

// ---------- handlers ----------
#[derive(Deserialize, Default)]
struct Params {
    q: Option<String>,
    sort: Option<String>,
    stock: Option<String>,
    /// Text, so a bad page number (?page=abc) opens the first page instead of an error.
    page: Option<String>,
    part: Option<String>,
}

impl Params {
    fn page_no(&self) -> usize {
        self.page.as_deref().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
    }
}

fn is_htmx(h: &HeaderMap) -> bool {
    h.get("hx-request").is_some() && h.get("hx-boosted").is_none()
}

async fn home(State(app): State<App>, h: HeaderMap) -> Res {
    // The new look's home is the feed (feed.rs).
    if look::is_new() {
        return feed::home(app, h).await;
    }
    let a = app.clone();
    cached(&app, "home".into(), || async move {
        let t = with_db(&a, |c| {
            let nav = chrome(c)?;
            // What the vault still holds (copies for sale and their value), not the sold copies kept as a record.
            let (total, value, _) = admin::in_vault(c)?;
            let value = db::peso(value);
            let graded = nav.links.iter().find(|x| x.handle == "graded-cards").map(|x| x.count).unwrap_or(0);
            // The owner picks which binders appear on the home page, and their order, in Admin › Binders.
            let binders = nav.links.iter().filter(|x| x.on_home).cloned().collect::<Vec<_>>();
            let mut views = vec![];
            // The numbers live on the admin dashboard now, not on the public page.
            for b in blocks::blocks(c)?.iter().filter(|b| b.visible && b.kind != "stats") {
                let v = view(c, b, &nav, &binders, total, graded, &value)?;
                // A collection row with no cards yet isn't shown to visitors.
                if ((v.kind == "row" || v.kind == "showcase") && v.items.is_empty()) || (v.kind == "stats" && v.stats.is_empty()) || (v.kind == "highlights" && v.highlights.is_empty()) || (v.kind == "vouches" && v.vouches.is_empty()) {
                    continue;
                }
                views.push(v);
            }
            let mut nav = nav;
            let site = nav.site.clone();
            nav.seo = Seo {
                title: format!("{} · Pokémon cards, graded slabs & collectibles in the Philippines", nav.brand),
                desc: clip(&format!("{} Pokémon cards and collectibles worth {} in one vault: graded slabs, Japanese cards, sealed product and figures, priced in pesos. Search them, browse the binders and meet the trainers.", db::thousands(total), value), 165),
                path: "/".into(),
                kind: "website",
                ld: ld(serde_json::json!({ "@context": "https://schema.org", "@graph": [
                    { "@type": "Organization", "@id": format!("{site}/#org"), "name": nav.brand, "url": format!("{site}/"), "logo": format!("{site}/static/img/icon-512.png") },
                    { "@type": "WebSite", "@id": format!("{site}/#site"), "name": nav.brand, "url": format!("{site}/"), "inLanguage": "en", "publisher": { "@id": format!("{site}/#org") },
                      "potentialAction": { "@type": "SearchAction", "target": { "@type": "EntryPoint", "urlTemplate": format!("{site}/search?q={{search_term_string}}") }, "query-input": "required name=search_term_string" } }
                ] })),
                ..Default::default()
            };
            Ok(Home { nav, blocks: views, edit: false, collections: String::new(), tab: "page" })
        })
        .await?;
        render(t)
    })
    .await
}

async fn browse(app: App, headers: HeaderMap, collection: Option<String>, p: Params) -> Res {
    let q = Query {
        collection: collection.clone(),
        q: p.q.clone().unwrap_or_default().trim().chars().take(80).collect(),
        sort: p.sort.clone().unwrap_or_default(),
        in_stock: p.stock.as_deref() == Some("1"),
        page: p.page_no().min(500),
    };
    let htmx = is_htmx(&headers);
    // A binder opened in the floating window.
    let window = p.part.is_some() && collection.is_some();
    let key = format!("{htmx}|{window}|{q:?}");
    let a = app.clone();
    cached(&app, key, || async move {
        let q2 = q.clone();
        let (nav, title, items, (tiles, total)) = with_db(&a, move |c| {
            let nav = chrome(c)?;
            let title = match &q2.collection {
                Some(h) => nav.links.iter().find(|x| &x.handle == h).map(|x| x.title.clone()),
                None if q2.q.is_empty() => Some("All cards".into()),
                None => Some(format!("Results for “{}”", q2.q)),
            };
            let (items, tiles) = db::list(c, &q2)?;
            // A whole binder says how many cards it holds, the same number as everywhere else. A search says how many it found.
            let shown = if q2.q.is_empty() && !q2.in_stock { db::cards_in(c, q2.collection.as_deref())? } else { tiles };
            Ok((nav, title, items, (tiles, shown)))
        })
        .await?;
        let Some(title) = title else {
            return Err((StatusCode::NOT_FOUND, render(NotFound { nav })?));
        };
        let base = match &q.collection {
            Some(h) => format!("/collections/{h}"),
            None => "/search".into(),
        };
        let next = ((q.page + 1) * PAGE_SIZE < tiles as usize).then_some(q.page + 1);
        if window {
            let handle = q.collection.clone().unwrap_or_default();
            if q.page > 0 {
                return render(BinderMore { handle, items, next });
            }
            let fan: Vec<Product> = items.iter().filter(|p| p.has_image()).take(7).cloned().collect();
            let tone = db::tone(&handle);
            return render(BinderPart { title, handle, total, tiles, tone, fan, items, next });
        }
        if htmx && q.page > 0 {
            render(Grid { base, q: q.q, sort: q.sort, in_stock: q.in_stock, items, next })
        } else {
            let mut nav = nav;
            // A binder and All cards are pages worth finding. A typed search isn't; its cards are found through these.
            let path = match &q.collection {
                Some(h) => format!("/collections/{h}"),
                None if q.q.is_empty() => "/all".into(),
                None => String::new(),
            };
            if !path.is_empty() {
                let site = nav.site.clone();
                let url = format!("{site}{path}");
                let list: Vec<serde_json::Value> = items.iter().take(24).enumerate().map(|(i, p)| serde_json::json!({ "@type": "ListItem", "position": i + 1, "url": format!("{site}/products/{}", p.handle), "name": p.title })).collect();
                let low = items.iter().filter(|p| p.price > 0.0).map(|p| p.price).fold(f64::INFINITY, f64::min);
                let range = if low.is_finite() { format!(" from {}", db::peso(low)) } else { String::new() };
                // A binder's title says it holds cards, so a binder called "Trainers" isn't confused with the Trainers page.
                let page_title = if q.collection.is_some() { format!("{title} · Pokémon cards · {}", nav.brand) } else { format!("{title} · {}", nav.brand) };
                nav.seo = Seo {
                    title: page_title,
                    desc: clip(&format!("{} Pokémon cards in {title} at {}{range}: real photos, condition and prices in pesos. Tap any card to see it up close.", db::thousands(total), nav.brand), 165),
                    image: items.iter().find(|p| p.has_image()).map(|p| p.img(900)).unwrap_or_default(),
                    kind: "website",
                    ld: ld(serde_json::json!({ "@context": "https://schema.org", "@graph": [
                        { "@type": "CollectionPage", "@id": url, "url": url, "name": title, "isPartOf": { "@id": format!("{site}/#site") }, "mainEntity": { "@type": "ItemList", "numberOfItems": total, "itemListElement": list } },
                        { "@type": "BreadcrumbList", "itemListElement": [
                            { "@type": "ListItem", "position": 1, "name": nav.brand, "item": format!("{site}/") },
                            { "@type": "ListItem", "position": 2, "name": title, "item": url } ] }
                    ] })),
                    path,
                };
            }
            render(CollectionPage { title, base, q: q.q, sort: q.sort, in_stock: q.in_stock, total, items, next, nav })
        }
    })
    .await
}

async fn collection(State(app): State<App>, headers: HeaderMap, Path(h): Path<String>, Qs(p): Qs<Params>) -> Res {
    browse(app, headers, Some(h), p).await
}
pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Remember what was searched, so future suggestions learn which cards matter most.
/// Pages the browser fetches ahead of time ("Sec-Purpose: prefetch") aren't real searches.
fn log_search(app: &App, headers: &HeaderMap, p: &Params) {
    let term = p.q.clone().unwrap_or_default();
    if term.trim().len() < 2 || p.page_no() > 0 || headers.contains_key("sec-purpose") {
        return;
    }
    // The same allowance as /api/searched, so nobody can fill the log by loading search pages.
    if crate::community::over_limit(app, format!("slog|{}", crate::community::client_ip(headers)), 60) {
        return;
    }
    let path = app.db_path.clone();
    tokio::task::spawn_blocking(move || {
        if let Ok(c) = db::open(&path) {
            let _ = suggest::log_search(&c, &term, unix_now());
        }
    });
}

async fn all(State(app): State<App>, headers: HeaderMap, Qs(p): Qs<Params>) -> Res {
    log_search(&app, &headers, &p);
    browse(app, headers, None, p).await
}
async fn search(State(app): State<App>, headers: HeaderMap, Qs(p): Qs<Params>) -> Res {
    log_search(&app, &headers, &p);
    browse(app, headers, None, p).await
}

#[derive(Deserialize)]
struct SuggestQs {
    q: String,
}

/// Probability-ranked suggestions for the search box (see src/suggest.rs for the model).
async fn suggest_api(State(app): State<App>, Qs(qs): Qs<SuggestQs>) -> Res {
    let typed: String = qs.q.chars().take(60).collect();
    let ql = suggest::norm(&typed);
    let empty = || serde_json::json!({ "items": [], "cards": [], "ghost": "" });
    let body = if ql.is_empty() {
        empty()
    } else {
        let ix = match app.sindex.get(&0) {
            Some(ix) => ix,
            None => {
                let ix = Arc::new(with_db(&app, |c| suggest::build(c)).await?);
                app.sindex.insert(0, ix.clone());
                ix
            }
        };
        let hits = suggest::suggest(&ix, &typed, unix_now());
        let ghost = hits.first().filter(|h| h.full_prefix && !h.typo && h.p >= 0.30 && h.href.is_none()).map(|h| h.text.clone()).unwrap_or_default();
        let q2 = ql.clone();
        let cards = with_db(&app, move |c| Ok(db::list(c, &Query { q: q2, ..Default::default() })?.0)).await?;
        serde_json::json!({
            "items": hits.iter().map(|h| serde_json::json!({ "text": h.text, "p": h.p, "kind": h.kind, "href": h.href, "typo": h.typo })).collect::<Vec<_>>(),
            "cards": cards.iter().take(3).map(|p| serde_json::json!({ "title": p.title, "handle": p.handle, "price": p.price_text(), "img": p.img(120) })).collect::<Vec<_>>(),
            "ghost": ghost,
        })
    };
    Ok(([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "private, max-age=10")], body.to_string()).into_response())
}

/// A browser couldn't start the search. Only ever written to the log, and only so many times, so nobody can fill it.
async fn oops(body: String) -> StatusCode {
    static LEFT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(300);
    if LEFT.fetch_update(std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed, |n| n.checked_sub(1)).is_ok() {
        let line: String = body.chars().filter(|c| !c.is_control()).take(340).collect();
        eprintln!("browser problem: {line}");
    }
    StatusCode::NO_CONTENT
}

/// A card page was opened: counts toward how likely that card is to be wanted next.
/// Each visitor counts once per card every 15 minutes, and only so many cards in all, so nobody can push a card up.
async fn hit(State(app): State<App>, headers: HeaderMap, Path(id): Path<i64>) -> StatusCode {
    let ip = crate::community::client_ip(&headers);
    if crate::community::over_limit(&app, format!("hit|{ip}|{id}"), 1) || crate::community::over_limit(&app, format!("hits|{ip}"), 300) {
        return StatusCode::NO_CONTENT;
    }
    let _ = with_db(&app, move |c| c.execute("UPDATE products SET hits=hits+1 WHERE id=?1", [id]).map(|_| ())).await;
    StatusCode::NO_CONTENT
}

#[derive(Deserialize, Default)]
struct PartQs {
    part: Option<String>,
}

async fn product(State(app): State<App>, Path(h): Path<String>, Qs(q): Qs<PartQs>) -> Res {
    let a = app.clone();
    let part = q.part.is_some();
    cached(&app, format!("p|{part}|{h}"), || async move {
        let r = with_db(&a, move |c| {
            let nav = chrome(c)?;
            Ok(match db::product(c, &h)? {
                Some(p) => {
                    let related = db::related(c, &p, 12)?;
                    let copies = db::copies(c, &p)?;
                    let versions = db::versions(c, &p)?;
                    let standing = db::standing(c, p.price)?;
                    let trend = marcuru_rs::history::trend(c, &p.api_id, &p.finish)?;
                    let mut nav = nav;
                    nav.seo = product_seo(&nav, &p, &db::canonical_handle(c, &p)?, db::total(c)?);
                    Ok((p, related, copies, versions, standing, trend, nav))
                }
                None => Err(NotFound { nav }),
            })
        })
        .await?;
        match r {
            Ok((p, related, copies, versions, standing, trend, _)) if part => render(ProductPart { p, related, copies, versions, standing, trend }),
            Ok((p, related, copies, versions, standing, trend, nav)) => render(ProductPage { p, related, nav, copies, versions, standing, trend }),
            Err(t) => Err((StatusCode::NOT_FOUND, render(t)?)),
        }
    })
    .await
}

/// A card's page for search engines: a clear title and description, and Product data with its price in pesos.
/// Copies of a card share one address (the first copy's), so search engines see one page per card.
fn product_seo(nav: &Chrome, p: &Product, canonical: &str, total: i64) -> Seo {
    let site = &nav.site;
    let url = format!("{site}/products/{canonical}");
    let set = p.set_text();
    let cond = p.condition_text();
    let mut what = p.short_title();
    if !set.is_empty() {
        what.push_str(&format!(" from {set}"));
    }
    let mut bits: Vec<String> = vec![];
    // The condition's words already name the finish; a grade doesn't, so a graded card adds it.
    if let Some(g) = p.grade() {
        bits.push(g);
        if !p.finish_shown().is_empty() {
            bits.push(p.finish_shown());
        }
    } else if !cond.is_empty() {
        bits.push(cond.clone());
    }
    if p.price > 0.0 {
        bits.push(p.price_text());
    }
    let notes = p.notes();
    let desc = clip(
        &format!(
            "{what}{}. {}See real photos of it in the {} collection, one of {} cards in the vault.",
            if bits.is_empty() { String::new() } else { format!(", {}", bits.join(", ")) },
            if notes.is_empty() { String::new() } else { format!("{}. ", clip(&notes, 70).trim_end_matches('.')) },
            nav.brand,
            db::thousands(total)
        ),
        165,
    );
    let image = if p.has_image() { nav.abs(&p.img(900)) } else { String::new() };
    let condition = if p.cond_class() == "sl" { "https://schema.org/NewCondition" } else { "https://schema.org/UsedCondition" };
    let mut product = serde_json::json!({
        "@type": "Product", "@id": format!("{url}#product"), "name": p.title, "url": url, "sku": p.id.to_string(),
        "description": desc, "category": "Collectible trading cards", "brand": { "@type": "Brand", "name": "Pokémon" },
    });
    if !image.is_empty() {
        product["image"] = serde_json::json!([image]);
    }
    if !p.card_no.is_empty() {
        product["mpn"] = serde_json::json!(p.card_no);
    }
    if p.price > 0.0 {
        product["offers"] = serde_json::json!({ "@type": "Offer", "url": url, "price": format!("{:.2}", p.price), "priceCurrency": "PHP",
            "availability": if p.available { "https://schema.org/InStock" } else { "https://schema.org/OutOfStock" },
            "itemCondition": condition, "seller": { "@type": "Organization", "name": nav.brand, "url": format!("{site}/") } });
    }
    Seo {
        title: format!("{} · {}{}", p.title, if p.price > 0.0 { format!("{} · ", p.price_text()) } else { String::new() }, nav.brand),
        desc,
        path: format!("/products/{canonical}"),
        image,
        kind: "product",
        ld: ld(serde_json::json!({ "@context": "https://schema.org", "@graph": [ product,
            { "@type": "BreadcrumbList", "itemListElement": [
                { "@type": "ListItem", "position": 1, "name": nav.brand, "item": format!("{site}/") },
                { "@type": "ListItem", "position": 2, "name": "All cards", "item": format!("{site}/all") },
                { "@type": "ListItem", "position": 3, "name": p.title, "item": url } ] } ] })),
    }
}

/// Where search engines may go, and the map of every page worth finding.
async fn robots() -> impl IntoResponse {
    let site = site();
    let body = format!("User-agent: *\nAllow: /\nDisallow: /admin\nDisallow: /me\nDisallow: /join\nDisallow: /signin\nDisallow: /signout\nDisallow: /api/\nDisallow: /suggest\nDisallow: /hit/\nDisallow: /oops\nDisallow: /*?part=\nDisallow: /*&part=\n\nSitemap: {site}/sitemap.xml\n");
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=3600")], body)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// Every page worth finding: the home page, All cards, each binder, one page per card (with its photo), the trainers and their stores.
async fn sitemap(State(app): State<App>) -> Res {
    let key = "sitemap.xml".to_string();
    let xml = if let Some(hit) = app.cache.get(&key) {
        (*hit).clone()
    } else {
        let site = site();
        // A change saved while this is being worked out means it may be out of date: then it isn't kept.
        let since = app.change_count();
        let (links, cards, trainers) = with_db(&app, |c| Ok((db::collections(c)?, db::sitemap_cards(c)?, members::approved(c)?))).await?;
        let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" xmlns:image=\"http://www.google.com/schemas/sitemap-image/1.1\">\n");
        let newest = cards.iter().map(|c| c.2.clone()).max().unwrap_or_default();
        let lm = |d: &str| if d.len() >= 10 { format!("<lastmod>{}</lastmod>", &d[..10]) } else { String::new() };
        for (path, when) in [("/", newest.as_str()), ("/all", newest.as_str()), ("/trainers", ""), ("/stores", ""), ("/vouches", "")] {
            x.push_str(&format!("<url><loc>{site}{path}</loc>{}</url>\n", lm(when)));
        }
        for l in links.iter().filter(|l| l.count > 0) {
            x.push_str(&format!("<url><loc>{site}/collections/{}</loc>{}</url>\n", xml_escape(&l.handle), lm(&newest)));
        }
        for (handle, title, when, img) in &cards {
            let img = if img.is_empty() {
                String::new()
            } else {
                let u = if img.starts_with("http") { img.clone() } else if img.starts_with("//") { format!("https:{img}") } else { format!("{site}{img}") };
                format!("<image:image><image:loc>{}</image:loc><image:title>{}</image:title></image:image>", xml_escape(&u), xml_escape(title))
            };
            x.push_str(&format!("<url><loc>{site}/products/{}</loc>{}{img}</url>\n", xml_escape(handle), lm(when)));
        }
        for t in trainers.iter().filter(|t| !t.blocked) {
            x.push_str(&format!("<url><loc>{site}/trainers/{}</loc></url>\n", t.id));
        }
        // Each open store with its own address. members::approved already leaves out trainers who are blocked or not approved;
        // a seller with no store address uses the trainer page above.
        for t in trainers.iter().filter(|t| !t.blocked && t.seller && !t.slug.is_empty()) {
            x.push_str(&format!("<url><loc>{site}/store/{}</loc></url>\n", xml_escape(&t.slug)));
        }
        x.push_str("</urlset>\n");
        app.keep(key, Arc::new(x.clone()), since);
        x
    };
    Ok(([(header::CONTENT_TYPE, "application/xml; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=1800")], xml).into_response())
}

/// Window pieces, data and private pages aren't for search engines: the header asks them to leave these out.
async fn robots_header(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let path = req.uri().path().to_string();
    let part = req.uri().query().is_some_and(|q| q.split('&').any(|kv| kv == "part" || kv.starts_with("part=")));
    let mut res = next.run(req).await;
    if part || path.starts_with("/api/") || path.starts_with("/admin") || path == "/suggest" || path == "/cards.tsv" || path.starts_with("/me") {
        res.headers_mut().insert(header::HeaderName::from_static("x-robots-tag"), header::HeaderValue::from_static("noindex, nofollow"));
    }
    res
}

async fn manifest(State(app): State<App>) -> impl IntoResponse {
    let brand = with_db(&app, |c| blocks::setting(c, "brand")).await.ok().flatten().filter(|b| !b.trim().is_empty()).unwrap_or_else(|| "Big Boys Vault".into());
    let m = serde_json::json!({
        "name": brand, "short_name": brand, "start_url": "/", "display": "standalone",
        "background_color": "#0b0d12", "theme_color": "#0b0d12",
        "icons": [{ "src": "/static/img/icon-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any" }, { "src": "/static/img/icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any" }]
    });
    ([(header::CONTENT_TYPE, "application/manifest+json")], m.to_string())
}

/// Admin › Home page: the real home page, every part editable in place (hidden parts and empty rows included, greyed).
async fn home_editor(State(app): State<App>, h: HeaderMap) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(Redirect::to("/admin").into_response());
    }
    let t = with_db(&app, |c| {
        let nav = chrome(c)?;
        let (total, value, _) = admin::in_vault(c)?;
        let value = db::peso(value);
        let graded = nav.links.iter().find(|x| x.handle == "graded-cards").map(|x| x.count).unwrap_or(0);
        let binders = nav.links.iter().filter(|x| x.on_home).cloned().collect::<Vec<_>>();
        let mut views = vec![];
        for b in blocks::blocks(c)?.iter().filter(|b| b.kind != "stats") {
            views.push(view(c, b, &nav, &binders, total, graded, &value)?);
        }
        let mut cols: Vec<serde_json::Value> = vec![serde_json::json!({ "handle": "all", "title": "All cards" })];
        cols.extend(nav.links.iter().map(|c| serde_json::json!({ "handle": c.handle, "title": c.title, "image": c.img(360), "count": c.count, "in_menu": c.in_menu })));
        Ok(Home { nav, blocks: views, edit: true, collections: serde_json::Value::Array(cols).to_string(), tab: "page" })
    })
    .await?;
    let mut r = Html(t.render().map_err(ise)?).into_response();
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(r)
}

/// Any address that isn't a page: the site's own "not found" page (with the menu and a way back), not a blank browser error.
pub(crate) async fn not_found(State(app): State<App>) -> Response {
    let html = match with_db(&app, |c| chrome(c)).await {
        Ok(nav) => NotFound { nav }.render().unwrap_or_else(|_| PLAIN_NOT_FOUND.to_string()),
        Err(_) => PLAIN_NOT_FOUND.to_string(),
    };
    (StatusCode::NOT_FOUND, Html(html)).into_response()
}

/// The "not found" page when even the database can't answer: still words and a way home, never a blank page.
const PLAIN_NOT_FOUND: &str = "<!doctype html><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Not found</title><body style=\"background:#03040a;color:#f3efe4;font-family:sans-serif;padding:24px\"><p>That page isn't here. <a href=\"/\" style=\"color:#efc873\">Back to the vault</a></p></body>";

/// An address that only takes a form being sent, opened like a page (say, by reloading it): the site's own page, not a blank one.
async fn wrong_method(State(app): State<App>) -> Response {
    let mut r = not_found(State(app)).await;
    *r.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
    r
}

/// The address without "www" is the same site: send visitors to the one true address, so sign-ins and carts aren't split in two.
/// Only that exact name is moved; the Fly address and localhost are left alone.
async fn www_redirect(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let host = req.uri().host().map(str::to_string).or_else(|| req.headers().get(header::HOST).and_then(|v| v.to_str().ok()).map(str::to_string)).unwrap_or_default();
    if let Some(to) = www_target(&host, req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/")) {
        return (StatusCode::MOVED_PERMANENTLY, [(header::LOCATION, to), (header::CACHE_CONTROL, "public, max-age=86400".to_string())]).into_response();
    }
    next.run(req).await
}

/// Where a request to the bare domain goes, keeping the path and the query. None for every other host.
fn www_target(host: &str, path_and_query: &str) -> Option<String> {
    let name = host.rsplit_once(':').filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit())).map(|(h, _)| h).unwrap_or(host);
    if !name.eq_ignore_ascii_case("bigboysvault.com") {
        return None;
    }
    let pq = if path_and_query.starts_with('/') { path_and_query } else { "/" };
    Some(format!("https://www.bigboysvault.com{pq}"))
}

/// `/favicon.ico`: browsers and link previews ask for it even though the pages name a PNG icon.
async fn favicon() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "public, max-age=604800")], &include_bytes!("../static/img/favicon-32.png")[..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restore_copy_replaces_the_database_only_when_it_checks_out() {
        let dir = std::env::temp_dir().join(format!("bbv-restore-{}-{}", std::process::id(), unix_now()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("data.db");
        let db_s = db.to_str().unwrap().to_string();
        let make = |p: &std::path::Path, v: i64| {
            let c = rusqlite::Connection::open(p).unwrap();
            c.execute_batch("CREATE TABLE t(v INTEGER); CREATE TABLE products(id INTEGER); CREATE TABLE members(id INTEGER);").unwrap();
            c.execute("INSERT INTO t VALUES(?1)", [v]).unwrap();
        };
        let read = |p: &std::path::Path| -> i64 { rusqlite::Connection::open(p).unwrap().query_row("SELECT v FROM t", [], |r| r.get(0)).unwrap() };
        make(&db, 1);
        // A broken copy is set aside and the database stays as it was.
        std::fs::write(dir.join("restore.db"), b"not a database at all, just some bytes that are long enough").unwrap();
        restore_if_present(&db_s);
        assert_eq!(read(&db), 1);
        assert!(!dir.join("restore.db").exists());
        let names = || std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect::<Vec<_>>();
        assert!(names().iter().any(|n| n.starts_with("restore.db.bad-")));
        let bad = || names().iter().filter(|n| n.starts_with("restore.db.bad-")).count();
        // An empty file (an upload that stopped at once) is set aside too: SQLite would open it as an empty database.
        for n in names().into_iter().filter(|n| n.starts_with("restore.db.bad-")) {
            std::fs::remove_file(dir.join(n)).unwrap();
        }
        std::fs::write(dir.join("restore.db"), b"").unwrap();
        restore_if_present(&db_s);
        assert_eq!(read(&db), 1);
        assert!(!dir.join("restore.db").exists());
        assert_eq!(bad(), 1);
        // So is a sound SQLite file that isn't this site's database (a test copy, say).
        std::thread::sleep(std::time::Duration::from_millis(1100)); // a new stamp for the new .bad- name
        {
            let c = rusqlite::Connection::open(dir.join("restore.db")).unwrap();
            c.execute_batch("CREATE TABLE t(v INTEGER); INSERT INTO t VALUES(7);").unwrap();
        }
        restore_if_present(&db_s);
        assert_eq!(read(&db), 1);
        assert!(!dir.join("restore.db").exists());
        assert_eq!(bad(), 2);
        // A good copy becomes the database, and the old one is kept beside it.
        make(&dir.join("restore.db"), 2);
        restore_if_present(&db_s);
        assert_eq!(read(&db), 2);
        assert!(!dir.join("restore.db").exists());
        assert!(names().iter().any(|n| n.starts_with("data.db.before-restore-")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_bare_domain_moves_to_www() {
        assert_eq!(www_target("bigboysvault.com", "/all?q=pika"), Some("https://www.bigboysvault.com/all?q=pika".into()));
        assert_eq!(www_target("BigBoysVault.com:443", "/"), Some("https://www.bigboysvault.com/".into()));
        assert_eq!(www_target("www.bigboysvault.com", "/all"), None);
        assert_eq!(www_target("bigboysvault.fly.dev", "/all"), None);
        assert_eq!(www_target("localhost:3000", "/"), None);
        assert_eq!(www_target("", "/"), None);
    }

    #[test]
    fn a_bad_page_number_opens_the_first_page() {
        let p = |s: Option<&str>| Params { page: s.map(str::to_string), ..Default::default() }.page_no();
        assert_eq!(p(Some("3")), 3);
        assert_eq!(p(Some("abc")), 0);
        assert_eq!(p(Some("-1")), 0);
        assert_eq!(p(None), 0);
    }
}

/// Whether `restore.db` really is a copy of this site's database: not empty (SQLite opens an empty or half-uploaded
/// file as a valid empty database), not the live file itself, intact, and holding the site's cards and trainers tables.
fn restore_copy_ok(restore: &std::path::Path, live: &std::path::Path) -> bool {
    if std::fs::metadata(restore).map(|m| m.len()).unwrap_or(0) == 0 {
        return false;
    }
    if let (Ok(a), Ok(b)) = (std::fs::canonicalize(restore), std::fs::canonicalize(live)) {
        if a == b {
            return false;
        }
    }
    let Ok(c) = rusqlite::Connection::open_with_flags(restore, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) else { return false };
    let intact = c.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)).is_ok_and(|s| s == "ok");
    intact && c.query_row("SELECT (SELECT COUNT(*) FROM products) + (SELECT COUNT(*) FROM members)", [], |r| r.get::<_, i64>(0)).is_ok()
}

/// A database copy put at `<folder>/restore.db` (docs/OPERATIONS.md › Restore) becomes the live one at start,
/// before anything opens it. A copy that fails its check is set aside and the current database is kept.
fn restore_if_present(db_path: &str) {
    use std::path::Path;
    let db = Path::new(db_path);
    let dir = db.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let restore = dir.join("restore.db");
    if !restore.is_file() {
        return;
    }
    let stamp = unix_now();
    let ok = restore_copy_ok(&restore, db);
    // the connection is closed by now, before any rename
    let _ = std::fs::remove_file(dir.join("restore.db-wal"));
    let _ = std::fs::remove_file(dir.join("restore.db-shm"));
    if !ok {
        let bad = dir.join(format!("restore.db.bad-{stamp}"));
        let _ = std::fs::rename(&restore, &bad);
        tracing::error!("restore.db failed its check: kept the current database, moved the copy to {}", bad.display());
        return;
    }
    let name = db.file_name().and_then(|n| n.to_str()).unwrap_or("data.db").to_string();
    for suffix in ["", "-wal", "-shm"] {
        let from = dir.join(format!("{name}{suffix}"));
        if from.exists() {
            let _ = std::fs::rename(&from, dir.join(format!("{name}.before-restore-{stamp}{suffix}")));
        }
    }
    match std::fs::rename(&restore, db) {
        Ok(()) => tracing::warn!("database restored from restore.db; the old one is {name}.before-restore-{stamp}"),
        Err(e) => {
            tracing::error!("could not move restore.db into place: {e}; putting the old database back");
            let _ = std::fs::rename(dir.join(format!("{name}.before-restore-{stamp}")), db);
            for suffix in ["-wal", "-shm"] {
                let _ = std::fs::rename(dir.join(format!("{name}.before-restore-{stamp}{suffix}")), dir.join(format!("{name}{suffix}")));
            }
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let db_path = std::env::var("DATABASE").unwrap_or_else(|_| "data.db".into());
    restore_if_present(&db_path);
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000);
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let uploads = std::env::var("UPLOADS").unwrap_or_else(|_| "uploads".into());
    let admin_token = admin::make_token();
    {
        let c = db::open(&db_path).expect("open database"); // creates schema on first run
        db::ensure_binders(&c).expect("binders");
        db::menu_hide_empty_once(&c).expect("menu");
        blocks::rebrand_once(&c).expect("rebrand");
        blocks::ensure_blocks(&c).expect("blocks");
        blocks::refresh_hero_art(&c).expect("hero art");
        blocks::community_once(&c).expect("community");
        look::load(&c).expect("look");
        // Old prices that didn't say where they came from are sorted once in db::migrate (price_src_v).
    }
    if admin_token.is_empty() {
        println!("Admin is OPEN (no password). Set ADMIN_TOKEN before putting this online.");
    } else if admin_token.chars().count() < 24 {
        eprintln!("WARNING: ADMIN_TOKEN is only {} characters long. Anyone who guesses it owns the site: set a long random one (openssl rand -hex 24).", admin_token.chars().count());
        tracing::warn!("ADMIN_TOKEN is shorter than 24 characters");
    }

    let app = App {
        db_path: Arc::new(db_path),
        uploads: Arc::new(uploads.clone()),
        admin_token: Arc::new(admin_token),
        cache: Cache::builder()
            .weigher(|k: &String, v: &Arc<String>| (k.len() + v.len()).try_into().unwrap_or(u32::MAX))
            .max_capacity(32 * 1024 * 1024)
            .time_to_live(Duration::from_secs(300))
            .build(),
        http: reqwest::Client::builder().user_agent("bigboysvault collection").timeout(Duration::from_secs(10)).build().expect("http client"),
        fx: Arc::new(tokio::sync::Mutex::new(None)),
        // Market lookups are remembered for an hour: instant repeats, and polite to the free price service.
        prices: Cache::builder().max_capacity(500).time_to_live(Duration::from_secs(3600)).build(),
        // Kept by size (16 MB at most), not by count: each search keeps only its first cards (ops::CATALOG_SHOWN).
        catalog: Cache::builder()
            .weigher(|k: &String, v: &Arc<Value>| (k.len() + serde_json::to_vec(&**v).map(|b| b.len()).unwrap_or(0)).try_into().unwrap_or(u32::MAX))
            .max_capacity(16 * 1024 * 1024)
            .time_to_live(Duration::from_secs(24 * 3600))
            .build(),
        // The suggestion model is rebuilt at most once a minute, so new cards and searches are learned quickly.
        sindex: Cache::builder().max_capacity(1).time_to_live(Duration::from_secs(60)).build(),
        limits: Cache::builder().max_capacity(20_000).time_to_live(Duration::from_secs(900)).build(),
        chat: Arc::new(community::ChatHub::new()),
        dm: Arc::new(messages::DmHub::new()),
        changes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
    };

    // Trainers' cards get their market prices in the background, again every few days.
    {
        let app = app.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(20)).await;
            loop {
                community::refresh_values(&app).await;
                tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
            }
        });
    }

    // Every hour: has the VM's backup or picture job gone quiet for too long? Then the admins are told (see ops.rs).
    {
        let app = app.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(300)).await;
            loop {
                let now = unix_now();
                let _ = with_db(&app, move |c| ops::ops_watchdog(c, now)).await;
                // Wishlist alerts: a wanted card that is for sale somewhere new rings its trainer's bell once.
                let _ = with_db(&app, move |c| marcuru_rs::wishes::run(c, now)).await;
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        });
    }

    // Cards saved before rarity was recorded get theirs from the catalogue, gently, in the background.
    {
        let app = app.clone();
        tokio::spawn(async move {
            admin::heal_photos(&app).await;
            let Ok(ids) = with_db(&app, |c| db::missing_rarity(c, 400)).await else { return };
            let found = !ids.is_empty();
            for id in ids {
                if let Ok(Some(info)) = market::by_id_lang(&app.http, &app.fx, &id).await {
                    let rarity = info["rarity"].as_str().filter(|r| !r.trim().is_empty()).unwrap_or("none").to_string();
                    let _ = with_db(&app, move |c| db::set_rarity(c, &id, &rarity)).await;
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            if found {
                app.forget();
            }
        });
    }

    // Pictures and other files keep for a day. Redirects and errors never do: they change with who's asking.
    let immutable = SetResponseHeaderLayer::if_not_present(header::CACHE_CONTROL, |r: &axum::response::Response| {
        let keeps = r.status().is_success() || r.status() == StatusCode::NOT_MODIFIED;
        Some(HeaderValue::from_static(if keeps { "public, max-age=86400" } else { "no-store" }))
    });
    let big = || axum::extract::DefaultBodyLimit::max(30 * 1024 * 1024);
    let router = Router::new()
        .route("/", get(home))
        .route("/collections/{handle}", get(collection))
        .route("/search", get(search))
        .route("/all", get(all))
        .route("/suggest", get(suggest_api))
        .route("/hit/{id}", post(hit))
        .route("/products/{handle}", get(product))
        .route("/manifest.webmanifest", get(manifest))
        .route("/robots.txt", get(robots))
        .route("/favicon.ico", get(favicon))
        .route("/healthz", get(ops::healthz))
        .route("/admin/ops/report", post(ops::ops_report))
        .route("/admin/photos/pending", get(ops::photos_pending))
        .route("/admin/photos/{id}", post(ops::photo_clean).layer(axum::extract::DefaultBodyLimit::max(12 * 1024 * 1024)))
        .route("/admin/similar/cards", get(ops::similar_cards))
        .route("/admin/similar", post(ops::similar_save).layer(axum::extract::DefaultBodyLimit::max(4 * 1024 * 1024)))
        .route("/admin/backup/db", get(ops::backup_db))
        .route("/admin/backup/uploads", get(ops::backup_uploads))
        .route("/admin/prices/refresh", post(ops::refresh_prices))
        .route("/admin/catalog/warm", post(ops::warm_catalog))
        .route("/admin/sets/refresh", post(ops::refresh_sets_route))
        .route("/sitemap.xml", get(sitemap))
        .route("/cards.tsv", get(api::cards_file))
        .route("/oops", post(oops).layer(axum::extract::DefaultBodyLimit::max(1024)))
        .route("/api/search", get(api::search_cards))
        .route("/api/card/{handle}", get(api::card))
        .route("/api/card/{handle}/text", get(api::card_text))
        .route("/api/binders", get(api::binders))
        .route("/api/searched", post(api::searched))
        .route("/api/condition-shares", get(api::condition_shares))
        .route("/api/price-suggest", get(api::price_suggest))
        .route("/api/notifications", get(community::get_notifications))
        .route("/api/notifications/read", post(community::read_notifications))
        // trainers
        .route("/join", get(community::join_page).post(community::join))
        .route("/login", get(community::login_page))
        // Opening these addresses like a page (a reload after a wrong password) goes somewhere useful.
        .route("/signin", get(|| async { Redirect::to("/login") }).post(community::signin))
        .route("/signout", get(|| async { Redirect::to("/") }).post(community::signout))
        .route("/trainers", get(community::trainers))
        .route("/trainers/{id}", get(community::trainer))
        .route("/trainer/{slug}", get(community::trainer_by_slug))
        .route("/trainer/{slug}/binders/{binder_slug}", get(community::seller_binder))
        .route("/sellers", get(store::directory))
        .route("/trainers/{id}/cards/{card}", get(community::member_card))
        .route("/me", get(community::me))
        // the seller panel
        .route("/me/store", get(store::dash))
        .route("/me/listings", get(store::listings))
        .route("/me/listings/bulk", post(store::bulk_listings))
        .route("/me/listings/move", post(store::move_cards))
        .route("/me/listings/{id}", post(store::edit_listing))
        .route("/me/listings/{id}/renew", post(store::renew_listing))
        .route("/me/binders", get(store::binders).post(community::me_create_binder))
        .route("/me/binders/{id}/rename", post(community::me_rename_binder))
        .route("/me/binders/{id}/delete", post(community::me_delete_binder))
        .route("/me/binders/{id}/cover", post(community::me_binder_cover))
        .route("/me/binders/{id}/edit", post(store::edit_binder))
        .route("/me/binders/{id}/order", post(store::order_binder))
        .route("/me/orders", get(store::orders))
        .route("/me/orders/reservations/{id}", post(store::answer_reservation))
        .route("/me/orders/offers/{id}", post(store::answer_offer))
        .route("/me/orders/questions/{id}/reply", post(store::reply_question))
        .route("/me/reviews", post(store::add_review))
        .route("/me/reviews/{id}/reply", post(store::reply_review))
        .route("/me/settings", get(store::settings).post(store::save_settings).layer(axum::extract::DefaultBodyLimit::max(12 * 1024 * 1024)))
        .route("/me/settings/slug", post(store::save_slug))
        .route("/me/notifications", get(store::notifications))
        .route("/me/notifications/prefs", post(store::save_pref))
        .route("/api/notifications/push", post(store::push_subscribe))
        .route("/api/cards/{id}/save", post(store::save_card))
        // the cart
        .route("/cart/lines", get(store::cart_lines))
        .route("/cart/checkout", post(store::checkout))
        .route("/me/orders/vault/{id}", post(store::cancel_vault_order))
        // the public storefront
        .route("/stores", get(store::directory))
        .route("/store/{slug}", get(store::storefront))
        .route("/store/{slug}/binders/{binder_slug}", get(store::store_binder))
        .route("/store/{slug}/follow", post(store::follow))
        .route("/store/{slug}/reserve", post(store::reserve))
        .route("/store/{slug}/offer", post(store::make_offer))
        .route("/store/{slug}/ask", post(store::ask_seller))
        .route("/me/profile", post(community::save_profile).layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024)))
        .route("/me/who", get(community::who))
        .route("/chat", get(community::chat_page))
        .route("/chat/messages", get(community::chat_messages).post(community::chat_post))
        .route("/chat/messages/{id}/delete", post(community::chat_delete))
        .route("/ask", get(community::ask_page))
        .route("/api/ask", post(api::ask_vault))
        .route("/admin/ask/status", get(api::ask_status))
        .route("/feed", get(feed::more))
        .route("/posts", post(feed::create).layer(axum::extract::DefaultBodyLimit::max(40 * 1024 * 1024)))
        .route("/posts/{id}", get(feed::one))
        .route("/posts/{id}/react", post(feed::react))
        .route("/posts/{id}/comments", get(feed::comments).post(feed::comment))
        .route("/posts/{id}/delete", post(feed::remove))
        .route("/posts/{id}/edit", post(feed::edit).layer(axum::extract::DefaultBodyLimit::max(40 * 1024 * 1024)))
        .route("/comments/{id}/edit", post(feed::comment_edit))
        .route("/posts/{id}/pin", post(feed::pin))
        .route("/comments/{id}/delete", post(feed::comment_remove))
        .route("/admin/feed/auto", post(feed::auto))
        .route("/admin/look", get(look::guide))
        .route("/admin/look/all", post(look::for_all))
        .route("/me/look", post(look::pick))
        .route("/me/messages", get(messages::page))
        .route("/admin/messages", get(messages::admin_page))
        .route("/admin/dm", get(messages::admin_list))
        .route("/admin/dm/{id}", get(messages::admin_thread))
        .route("/dm/inbox", get(messages::inbox))
        .route("/dm/poll", get(messages::poll))
        .route("/dm/ice", get(messages::ice))
        .route("/dm/open", post(messages::open))
        .route("/dm/{id}", get(messages::thread))
        .route("/dm/{id}/send", post(messages::send))
        .route("/dm/{id}/read", post(messages::read))
        .route("/dm/{id}/call", post(messages::call).layer(axum::extract::DefaultBodyLimit::max(96 * 1024)))
        .route("/dm/{id}/photo", post(messages::photo).layer(axum::extract::DefaultBodyLimit::max(12 * 1024 * 1024)))
        .route("/dm/{id}/voice", post(messages::voice).layer(axum::extract::DefaultBodyLimit::max(3 * 1024 * 1024 + 4096)))
        .route("/me/photos", post(community::upload_photo).layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024)))
        .route("/me/photos/{id}/drop", post(community::drop_photo))
        .route("/vouches", get(community::wall).post(community::post_vouch))
        .route("/vouches/{id}", get(community::vouch_one))
        .route("/vouches/{id}/act", post(community::vouch_act))
        .route("/me/search", get(community::search_cards))
        .route("/me/cards", post(community::add_card))
        .route("/me/cards/upload", post(community::upload_card).layer(axum::extract::DefaultBodyLimit::max(12 * 1024 * 1024)))
        .route("/admin/member-cards/{id}/delete", post(community::staff_remove_card))
        .route("/me/cards/order", post(community::order_cards))
        .route("/me/cards/{card}", post(community::card_action))
        .route("/me/cards/{card}/original-photo", post(community::original_photo))
        .route("/me/password", post(community::change_password))
        .route("/me/delete", post(community::delete_me))
        .route("/admin/trainers", get(community::admin_page))
        .route("/admin/dashboard", get(admin::dashboard))
        .route("/admin/dashboard/fix-conditions", post(admin::fix_conditions))
        .route("/admin/trainers/{id}", post(community::admin_action))
        .route("/admin/vouch-photos/{id}/delete", post(community::admin_photo_delete))
        // admin: cards
        .route("/admin", get(admin::cards))
        .route("/admin/new", get(admin::new_card))
        .route("/admin/edit/{id}", get(admin::edit))
        .route("/admin/login", get(|| async { Redirect::to("/admin") }).post(admin::login))
        .route("/admin/logout", get(|| async { Redirect::to("/admin") }).post(admin::logout))
        .route("/admin/check", get(admin::check))
        .route("/admin/save", post(admin::save).layer(big()))
        .route("/admin/delete/{id}", post(admin::delete))
        .route("/admin/dup/{id}", post(admin::dup))
        .route("/admin/quick-edit", post(admin::quick_edit))
        .route("/admin/order", post(admin::order))
        .route("/admin/price", get(admin::price))
        .route("/admin/quick", get(admin::quick))
        .route("/admin/catalog", get(admin::catalog))
        .route("/admin/sets", get(admin::by_set))
        .route("/admin/binders", get(admin::binders_page).post(admin::binder_create))
        .route("/admin/binders/order", post(admin::binders_order))
        .route("/admin/binders/{handle}", post(admin::binder_update))
        .route("/admin/binders/{handle}/covers", get(admin::binder_covers))
        .route("/admin/binders/{handle}/cover", post(admin::binder_cover_upload).layer(big()))
        .route("/admin/sets/search", get(admin::set_search))
        .route("/admin/sets/cards", get(admin::set_cards))
        .route("/admin/cardimg/unlimited", get(admin::cardimg_unlimited))
        .route("/admin/bulk", post(admin::bulk))
        .route("/admin/photo/{id}", post(admin::photo).layer(big()))
        .route("/admin/photo/{id}/remove", post(admin::photo_remove))
        // admin: page sections
        .route("/admin/page", get(home_editor))
        .route("/admin/page/list", get(admin::page_editor))
        .route("/admin/settings", post(admin::settings))
        .route("/admin/settings/condition-shares", post(admin::settings_condition_shares))
        .route("/admin/sellers", get(store::admin_sellers))
        .route("/admin/orders", get(store::admin_orders))
        .route("/admin/orders/pay", post(store::admin_vault_pay))
        .route("/admin/orders/{id}", post(store::admin_answer_order))
        .route("/admin/sellers/announce", post(store::announce))
        .route("/admin/sellers/reviews/{id}", post(store::admin_review))
        .route("/admin/sellers/flag/{id}/clear", post(store::admin_clear_flag))
        .route("/admin/blocks/add", post(admin::block_add))
        .route("/admin/blocks/order", post(admin::blocks_order))
        .route("/admin/pick", get(admin::pick))
        .route("/admin/menu", post(admin::menu_save))
        .route("/admin/blocks/{id}/save", post(admin::block_save).layer(big()))
        .route("/admin/blocks/{id}/move", post(admin::block_move))
        .route("/admin/blocks/{id}/visible", post(admin::block_visible))
        .route("/admin/blocks/{id}/delete", post(admin::block_delete))
        // Pictures: AVIF or WebP when the browser takes it and the file exists; kept for a day.
        .nest("/uploads", Router::new().fallback_service(ServeDir::new(uploads)).layer(axum::middleware::from_fn_with_state(app.clone(), ops::negotiate_uploads)))
        // App code with a version (?v=N) keeps for a year; without one it revalidates on every load (cheap 304).
        .nest("/static", Router::new().fallback_service(ServeDir::new("static")).layer(axum::middleware::from_fn(ops::static_cache)))
        .fallback(not_found)
        .method_not_allowed_fallback(wrong_method)
        .layer(immutable)
        .layer(SetResponseHeaderLayer::if_not_present(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::if_not_present(header::REFERRER_POLICY, HeaderValue::from_static("same-origin")))
        .layer(SetResponseHeaderLayer::if_not_present(header::X_FRAME_OPTIONS, HeaderValue::from_static("SAMEORIGIN")))
        // Browsers that have seen the site once only ever ask for it over https (they ignore this on plain http).
        .layer(SetResponseHeaderLayer::if_not_present(header::STRICT_TRANSPORT_SECURITY, HeaderValue::from_static("max-age=15552000")))
        .layer(axum::middleware::from_fn(look::layer))
        .layer(axum::middleware::from_fn(robots_header))
        .layer(CompressionLayer::new())
        .layer(axum::middleware::from_fn(www_redirect))
        .with_state(app.clone());
    // Mail goes out and old listings come off sale in the background.
    store::start_housekeeping(app);
    ops::mark_started();

    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    println!("Big Boys Vault listening on http://{addr}");
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .unwrap();
}
