//! Keeping the site fast and safe: picture formats, cache headers, a health check, and the routes the VM's
//! nightly jobs call (database snapshot, uploads list, price refresh, search warm-up). See docs/OPERATIONS.md.
use crate::{admin, with_db, App, Res};
use axum::{
    extract::{Query, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use marcuru_rs::{blocks, market, members, pricing, sellers};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

// ---------- pictures and cache headers ----------

/// `/uploads`: a browser that takes AVIF or WebP gets that file when it exists beside the JPEG (the VM's image job
/// makes them). Pictures keep for a day in the browser, and a stale one may be shown for a week while a fresh one is fetched.
/// A copy only counts while its original is still there, so a deleted photo stays deleted.
/// One address can send different formats, and a shared cache in front (Cloudflare) ignores `Vary: Accept`: so a picture
/// that has copies is never kept by the CDN, only by the browser.
pub async fn negotiate_uploads(State(app): State<App>, mut req: Request, next: Next) -> Response {
    let accept = req.headers().get(header::ACCEPT).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let path = req.uri().path().to_string();
    let lower = path.to_lowercase();
    let mut has_copies = false;
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".png") {
        let file = PathBuf::from(&*app.uploads).join(path.trim_start_matches('/'));
        if file.is_file() {
            has_copies = ["avif", "webp"].iter().any(|ext| file.with_extension(ext).is_file());
            for (mime, ext) in [("image/avif", "avif"), ("image/webp", "webp")] {
                if accept.contains(mime) && file.with_extension(ext).is_file() {
                    let stem = path.rsplit_once('.').map(|(a, _)| a).unwrap_or(&path);
                    if let Ok(uri) = format!("{stem}.{ext}").parse::<axum::http::Uri>() {
                        *req.uri_mut() = uri;
                    }
                    break;
                }
            }
        }
    }
    let mut res = next.run(req).await;
    let kept = res.status().is_success() || res.status() == StatusCode::NOT_MODIFIED;
    let h = res.headers_mut();
    h.insert(header::VARY, HeaderValue::from_static("Accept"));
    if kept {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=86400, stale-while-revalidate=604800"));
        if has_copies {
            h.insert(header::HeaderName::from_static("cdn-cache-control"), HeaderValue::from_static("no-store"));
            h.insert(header::HeaderName::from_static("cloudflare-cdn-cache-control"), HeaderValue::from_static("no-store"));
        }
    }
    res
}

/// `/static`: files asked for with a version (`?v=N`) never change, so browsers and the CDN keep them for a year.
/// Anything without a version revalidates on every load (a cheap 304).
pub async fn static_cache(req: Request, next: Next) -> Response {
    let versioned = req.uri().query().is_some_and(|q| q.split('&').any(|p| p.starts_with("v=")));
    let mut res = next.run(req).await;
    // "Not modified" is a good answer too: it must keep the same rule, or the browser throws its copy away.
    let ok = res.status().is_success() || res.status() == StatusCode::NOT_MODIFIED;
    let value = if !ok { "no-store" } else if versioned { "public, max-age=31536000, immutable" } else { "no-cache" };
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    res
}

static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Call once at boot, so `/healthz` can say how long this process has been up.
pub fn mark_started() {
    STARTED.get_or_init(std::time::Instant::now);
}

/// `GET /healthz`: is the site up and is the database answering? For the uptime watcher.
pub async fn healthz(State(app): State<App>) -> Response {
    let up = STARTED.get().map(|s| s.elapsed().as_secs()).unwrap_or(0);
    let t = std::time::Instant::now();
    let counts = with_db(&app, |c| {
        let cards: i64 = c.query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0))?;
        let members: i64 = c.query_row("SELECT COUNT(*) FROM members", [], |r| r.get(0))?;
        Ok((cards, members))
    })
    .await;
    let ms = t.elapsed().as_millis();
    match counts {
        Ok((cards, members)) => (
            [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")],
            json!({ "ok": true, "cards": cards, "members": members, "db_ms": ms, "uptime_s": up }).to_string(),
        )
            .into_response(),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], json!({ "ok": false, "db_ms": ms }).to_string()).into_response(),
    }
}

/// How many cards a catalogue search sends back. The pages show this many; `total` says how many matched.
pub const CATALOG_SHOWN: usize = 120;

/// The card catalogue search, remembered for a day. The catalogue changes rarely and the service is slow.
/// Only the first cards are kept and sent, so a short word like "pi" can't fill the memory.
pub async fn catalog_cached(app: &App, q: &str, lang: &str) -> Value {
    let key = format!("{lang}|{}", q.trim().to_lowercase());
    if let Some(hit) = app.catalog.get(&key) {
        return (*hit).clone();
    }
    match market::catalog(&app.http, q, lang).await {
        Ok(mut v) => {
            let total = trim_catalog(&mut v);
            if total > 0 {
                app.catalog.insert(key, Arc::new(v.clone()));
            }
            v
        }
        Err(_) => json!({ "cards": [], "total": 0, "error": "The card list isn't answering right now." }),
    }
}

/// Keeps the first `CATALOG_SHOWN` cards and writes how many matched in `total`. Returns that number.
fn trim_catalog(v: &mut Value) -> usize {
    let total = v["cards"].as_array().map_or(0, |a| a.len());
    if let Some(a) = v.get_mut("cards").and_then(|c| c.as_array_mut()) {
        a.truncate(CATALOG_SHOWN);
    }
    if let Some(o) = v.as_object_mut() {
        o.insert("total".into(), json!(total));
    }
    total
}

// ---------- set release dates, for sorting cards by set ----------

/// Looks up the sets the vault's cards use that we have no date for yet. Returns how many were saved.
pub async fn refresh_sets(app: &App) -> usize {
    let Ok(codes) = with_db(app, |c| marcuru_rs::db::sets_to_fetch(c)).await else { return 0 };
    let Ok(names) = with_db(app, |c| marcuru_rs::db::set_names_to_fetch(c)).await else { return 0 };
    let mut n = 0;
    for code in codes {
        let found = market::set_info(&app.http, &code, "en").await;
        let (name, series, released) = found.unwrap_or_default();
        let k = code.clone();
        if with_db(app, move |c| marcuru_rs::db::save_set(c, &k, &name, &series, &released)).await.is_ok() {
            n += 1;
        }
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    }
    for name in names {
        let Some(code) = market::set_code_for_name(&app.http, &name, "en").await else { continue };
        let Some((set_name, series, released)) = market::set_info(&app.http, &code, "en").await else { continue };
        let set_name = if set_name.is_empty() { name.clone() } else { set_name };
        if with_db(app, move |c| marcuru_rs::db::save_set(c, &code, &set_name, &series, &released)).await.is_ok() {
            n += 1;
        }
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    }
    if n > 0 {
        app.forget();
    }
    n
}

/// `POST /admin/sets/refresh`: the same, on demand.
pub async fn refresh_sets_route(State(app): State<App>, h: HeaderMap) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(deny());
    }
    let n = refresh_sets(&app).await;
    let known: i64 = with_db(&app, |c| c.query_row("SELECT COUNT(*) FROM card_sets WHERE released<>''", [], |r| r.get(0))).await?;
    Ok(([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], json!({ "ok": true, "fetched": n, "known": known }).to_string()).into_response())
}

// ---------- routes for the VM's jobs (owner's token or a full admin) ----------

fn deny() -> Response {
    (StatusCode::NOT_FOUND, "Not found").into_response()
}

/// `GET /admin/backup/db`: a consistent copy of the database, as a file. `VACUUM INTO` makes a clean snapshot
/// even while the site is busy writing.
pub async fn backup_db(State(app): State<App>, h: HeaderMap) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(deny());
    }
    let tmp = std::env::temp_dir().join(format!("bbv-backup-{}.db", members::temp_password()));
    let path = tmp.to_string_lossy().replace('\'', "''");
    with_db(&app, move |c| c.execute_batch(&format!("VACUUM INTO '{path}'"))).await?;
    let bytes = tokio::fs::read(&tmp).await.map_err(crate::ise)?;
    let _ = tokio::fs::remove_file(&tmp).await;
    let name = format!("attachment; filename=\"data-{}.db\"", admin::today());
    Ok((
        [(header::CONTENT_TYPE, "application/vnd.sqlite3".to_string()), (header::CONTENT_DISPOSITION, name), (header::CACHE_CONTROL, "no-store".to_string())],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct SinceQs {
    since: Option<i64>,
}

/// `GET /admin/backup/uploads?since=<unix time>`: every picture file changed since then, so a backup can fetch
/// only what's new from `/uploads/...`.
pub async fn backup_uploads(State(app): State<App>, h: HeaderMap, Query(q): Query<SinceQs>) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(deny());
    }
    let root = PathBuf::from(&*app.uploads);
    let since = q.since.unwrap_or(0);
    let files = tokio::task::spawn_blocking(move || {
        let mut out = vec![];
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let Ok(md) = e.metadata() else { continue };
                let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
                if mtime >= since {
                    let rel = p.strip_prefix(&root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                    out.push(json!({ "path": format!("/uploads/{rel}"), "size": md.len(), "mtime": mtime }));
                }
            }
        }
        out
    })
    .await
    .map_err(crate::ise)?;
    Ok(([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], json!({ "ok": true, "now": crate::unix_now(), "files": files }).to_string()).into_response())
}

#[derive(Deserialize)]
pub struct LimitQs {
    limit: Option<i64>,
}

/// `POST /admin/prices/refresh?limit=50`: today's market price for the stalest cards, the vault's and the sellers',
/// a few at a time (the price service is free: be polite). Sellers whose own price is far from the market are told.
/// Run it in a loop until `remaining` is 0: see ops/prices.sh.
pub async fn refresh_prices(State(app): State<App>, h: HeaderMap, Query(q): Query<LimitQs>) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(deny());
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let out = refresh_prices_batch(&app, limit).await?;
    Ok(([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], out.to_string()).into_response())
}

/// How many catalogue cards (one per finish) have never had their market price looked up (the SRP shows "no market price yet").
pub async fn market_missing(app: &App) -> i64 {
    with_db(app, |c| c.query_row("SELECT COUNT(DISTINCT api_id || '|' || finish) FROM products WHERE api_id<>'' AND market_at=0", [], |r| r.get(0))).await.unwrap_or(0)
}

use market::finish_price;
#[cfg(test)]
use market::member_finish_price;

/// Catalogue cards whose market price is the oldest (or never looked up), one row per card and finish.
fn stale_finishes(c: &rusqlite::Connection, limit: i64, older_than: i64) -> rusqlite::Result<Vec<(String, String)>> {
    let mut st = c.prepare(
        "SELECT api_id, finish FROM products WHERE api_id<>'' AND market_at<?2
         GROUP BY api_id, finish ORDER BY MIN(market_at), api_id, finish LIMIT ?1",
    )?;
    let v = st.query_map(rusqlite::params![limit, older_than], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// Saves today's market price for one finish of a card, and moves every copy of that finish whose price follows the
/// market to the suggested price for its condition. Only prices that came from the market ('market' or 'suggested')
/// are ever changed: a price a person typed is never touched. Returns how many prices changed.
pub(crate) fn save_market(
    c: &rusqlite::Connection,
    api_id: &str,
    finish: &str,
    nm: Option<f64>,
    now: i64,
    today: &str,
    shares: &std::collections::HashMap<String, f64>,
) -> rusqlite::Result<usize> {
    // Remembered even when nothing was found (0), so the card isn't asked for again today.
    c.execute("UPDATE products SET market_php=?3, market_at=?4 WHERE api_id=?1 AND finish=?2", rusqlite::params![api_id, finish, nm.unwrap_or(0.0), now])?;
    let Some(nm) = nm else { return Ok(0) };
    // One price a day per card and finish, for the "up 12% this month" line on card pages.
    marcuru_rs::history::record(c, api_id, finish, nm, today)?;
    let mut st = c.prepare("SELECT id, variant FROM products WHERE api_id=?1 AND finish=?2 AND price_src IN ('market','suggested')")?;
    let rows: Vec<(i64, String)> = st.query_map([api_id, finish], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    let mut n = 0;
    for (id, cond) in rows {
        let cond = if cond.trim().is_empty() { "Near Mint".to_string() } else { cond };
        if let Some(price) = pricing::suggest_with_shares(shares, nm, &cond) {
            c.execute("UPDATE products SET price=?2, price_updated=?3 WHERE id=?1", rusqlite::params![id, price, today])?;
            n += 1;
        }
    }
    Ok(n)
}

/// The price service didn't answer for this card: keep yesterday's market price and try again in about an hour.
fn retry_market_soon(c: &rusqlite::Connection, api_id: &str, finish: &str, now: i64) -> rusqlite::Result<usize> {
    c.execute("UPDATE products SET market_at=MAX(market_at, ?3) WHERE api_id=?1 AND finish=?2", rusqlite::params![api_id, finish, now - 19 * 3600])
}

/// One batch of the price refresh. Returns the JSON the route answers with.
pub async fn refresh_prices_batch(app: &App, limit: i64) -> Result<Value, (StatusCode, String)> {
    let app = app.clone();
    let today = admin::today();
    let now = crate::unix_now();
    // Every catalogue card gets today's Near Mint market price for each of its finishes (the SRP comes from it), oldest
    // first. Copies whose price follows the market get the suggested price too.
    let stale: Vec<(String, String)> = with_db(&app, move |c| stale_finishes(c, limit, now - 20 * 3600)).await?;
    let mut updated_products = 0usize;
    let mut failed = 0usize;
    // One lookup per card, shared by its finishes. A card whose lookup failed isn't asked again in this batch.
    let mut cache: std::collections::HashMap<String, Option<Value>> = std::collections::HashMap::new();
    let mut down: std::collections::HashSet<String> = std::collections::HashSet::new();
    let shares = pricing::current();
    for (api_id, finish) in &stale {
        let looked: Result<Option<Value>, ()> = if let Some(v) = cache.get(api_id) {
            Ok(v.clone())
        } else if down.contains(api_id) {
            Err(())
        } else {
            let r = market::by_id_lang(&app.http, &app.fx, api_id).await;
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            match r {
                Ok(v) => {
                    cache.insert(api_id.clone(), v.clone());
                    Ok(v)
                }
                Err(_) => {
                    failed += 1;
                    down.insert(api_id.clone());
                    Err(())
                }
            }
        };
        let (key, fin) = (api_id.clone(), finish.clone());
        let Ok(card) = looked else {
            // The service didn't answer (down or slow): that isn't "no price". Keep the old price.
            with_db(&app, move |c| retry_market_soon(c, &key, &fin, now).map(|_| ())).await?;
            continue;
        };
        let nm = card.as_ref().and_then(|c| finish_price(c, finish));
        let (today, shares2) = (today.clone(), shares.clone());
        let n = with_db(&app, move |c| save_market(c, &key, &fin, nm, now, &today, &shares2)).await?;
        updated_products += n;
    }
    // Sellers' cards: one lookup per catalogue card, oldest first.
    let keys: Vec<String> = with_db(&app, move |c| {
        let mut st = c.prepare(
            "SELECT k FROM (SELECT CASE WHEN instr(card_id,'#')>0 THEN substr(card_id,1,instr(card_id,'#')-1) ELSE card_id END AS k, MIN(value_at) AS at
             FROM member_cards WHERE card_id NOT LIKE 'own:%' GROUP BY k) ORDER BY at ASC LIMIT ?1",
        )?;
        let v = st.query_map([limit], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(v)
    })
    .await?;
    let mut updated_cards = 0usize;
    for key in &keys {
        let card: Result<Option<Value>, ()> = match cache.get(key).cloned().flatten() {
            Some(c) => Ok(Some(c)),
            None => {
                let r = market::by_id_lang(&app.http, &app.fx, key).await;
                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                match r {
                    Ok(v) => {
                        cache.insert(key.clone(), v.clone());
                        Ok(v)
                    }
                    Err(_) => {
                        failed += 1;
                        Err(())
                    }
                }
            }
        };
        // Each finish a seller listed gets that finish's price (the same matching as the vault's copies).
        let k = key.clone();
        let priced = match card {
            Ok(card) => with_db(&app, move |c| members::price_listings(c, &k, card.as_ref(), now)).await?,
            Err(()) => with_db(&app, move |c| members::hold_listing_values(c, &k, now).map(|_| false)).await?,
        };
        if priced {
            updated_cards += 1;
        }
    }
    // A seller's own price far from today's market: tell them, once a week per card.
    let flagged = with_db(&app, move |c| {
        let mut st = c.prepare("SELECT id, member_id, title, cond, value, value_own FROM member_cards WHERE value_own>0 AND value>0 AND status IN ('Active','Reserved') AND card_id NOT LIKE 'own:%'")?;
        let rows: Vec<(i64, i64, String, String, f64, f64)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?.collect::<Result<_, _>>()?;
        let mut n = 0;
        for (id, owner, title, cond, value, own) in rows {
            let Some(suggested) = pricing::suggest(value, &cond) else { continue };
            if suggested <= 0.0 || (own - suggested).abs() / suggested <= 0.5 {
                continue;
            }
            let link = format!("/me/listings?id={id}");
            let recent: i64 = c.query_row("SELECT COUNT(*) FROM notifications WHERE member_id=?1 AND kind='price' AND link=?2 AND created_at>?3", rusqlite::params![owner, link, now - 7 * 86400], |r| r.get(0))?;
            if recent > 0 {
                continue;
            }
            let word = if own > suggested { "above" } else { "below" };
            sellers::notify(c, owner, "price", &format!("Your price for {} ({}) is far {word} today's market: you ask {}, the market says {}.", title.split(" - ").next().unwrap_or(&title), cond, vault_search::peso(own), vault_search::peso(suggested)), &link)?;
            n += 1;
        }
        Ok(n)
    })
    .await?;
    if updated_products > 0 {
        app.forget();
    }
    let (remaining_products, remaining_cards): (i64, i64) = with_db(&app, move |c| {
        Ok((
            c.query_row("SELECT COUNT(DISTINCT api_id || '|' || finish) FROM products WHERE api_id<>'' AND market_at<?1", [now - 20 * 3600], |r| r.get(0))?,
            c.query_row("SELECT COUNT(DISTINCT CASE WHEN instr(card_id,'#')>0 THEN substr(card_id,1,instr(card_id,'#')-1) ELSE card_id END) FROM member_cards WHERE card_id NOT LIKE 'own:%' AND value_at<?1", [now - 86400], |r| r.get(0))?,
        ))
    })
    .await?;
    Ok(json!({ "ok": true, "products": updated_products, "cards": updated_cards, "flagged": flagged, "failed": failed, "remaining_products": remaining_products, "remaining_cards": remaining_cards, "remaining": remaining_products + remaining_cards }))
}

/// `POST /admin/catalog/warm?limit=40`: looks up the names people search for most, so the first search of the
/// day is as quick as the second.
pub async fn warm_catalog(State(app): State<App>, h: HeaderMap, Query(q): Query<LimitQs>) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(deny());
    }
    let limit = q.limit.unwrap_or(40).clamp(1, 200);
    let terms: Vec<String> = with_db(&app, move |c| {
        let mut st = c.prepare("SELECT term FROM search_log ORDER BY n DESC, last DESC LIMIT ?1")?;
        let v = st.query_map([limit], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(v)
    })
    .await?;
    let mut warmed = 0usize;
    for t in &terms {
        let key = format!("en|{}", t.trim().to_lowercase());
        if app.catalog.contains_key(&key) {
            continue;
        }
        let v = catalog_cached(&app, t, "en").await;
        if v["cards"].as_array().is_some_and(|a| !a.is_empty()) {
            warmed += 1;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    Ok(([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], json!({ "ok": true, "terms": terms.len(), "warmed": warmed }).to_string()).into_response())
}

// ---------- what the VM's jobs say ----------

/// The jobs the VM runs, and after how many hours without a good report each one counts as late (0: never late).
pub const OPS_JOBS: [(&str, i64); 9] = [
    ("backup", 30),
    ("prices", 30),
    ("images", 50),
    ("checks", 1),
    ("drill", 30),
    ("photos", 0),
    ("similar", 50),
    ("uptime", 0),
    ("update", 0),
];

/// Hours after which a job with no good report counts as late, for the admin dashboard (0: it is never late).
pub fn late_after_hours(job: &str) -> i64 {
    OPS_JOBS.iter().find(|j| j.0 == job).map(|j| j.1).unwrap_or(0)
}

/// The last thing one job said: when, good or not, its note, when it last went well, and when it first reported.
#[derive(Default, Clone, Debug, PartialEq)]
struct JobState {
    at: i64,
    ok: bool,
    note: String,
    last_ok: i64,
    since: i64,
}

fn read_job(c: &rusqlite::Connection, job: &str) -> rusqlite::Result<Option<JobState>> {
    let Some(raw) = blocks::setting(c, &format!("ops_job_{job}"))? else { return Ok(None) };
    let v: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    let at = v["at"].as_i64().unwrap_or(0);
    Ok(Some(JobState {
        at,
        ok: v["ok"].as_bool().unwrap_or(false),
        note: v["note"].as_str().unwrap_or("").to_string(),
        last_ok: v["last_ok"].as_i64().unwrap_or(0),
        since: v["since"].as_i64().unwrap_or(at),
    }))
}

/// Every admin trainer gets the same note in the bell. Returns how many were told.
fn tell_admins(c: &rusqlite::Connection, message: &str) -> rusqlite::Result<usize> {
    let mut st = c.prepare("SELECT id FROM members WHERE role='admin' AND blocked=0")?;
    let ids: Vec<i64> = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for id in &ids {
        sellers::notify(c, *id, "announcement", message, "/admin")?;
    }
    Ok(ids.len())
}

/// A short note, on one line, at most 300 characters.
fn clean_note(note: &str) -> String {
    let flat: String = note.chars().map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
    flat.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(300).collect()
}

/// "Nov 12, 6:15 AM" in Manila time (UTC+8, no daylight saving), for notes people read.
fn manila_stamp(t: i64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let local = t + 8 * 3600;
    let (days, secs) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    // Days since 1970 to a calendar date (the usual civil-date arithmetic).
    let z = days + 719_468;
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    format!("{} {}, {}:{:02} {}", MONTHS[(month - 1) as usize], day, if h % 12 == 0 { 12 } else { h % 12 }, m, if h < 12 { "AM" } else { "PM" })
}

/// A job that had failed is working again: its warnings in the admins' bell say so, instead of reading as if
/// something were still broken. Only warnings not marked yet are touched. Returns how many notes changed.
fn mark_fixed(c: &rusqlite::Connection, job: &str, good_at: i64) -> rusqlite::Result<usize> {
    let lead = format!("Fixed, working again (last good run {}). ", manila_stamp(good_at));
    c.execute(
        "UPDATE notifications SET message = ?1 || message WHERE kind='announcement' AND (message LIKE ?2 OR message LIKE ?3)",
        rusqlite::params![lead, format!("VM: {job} failed:%"), format!("VM: the {job} job %")],
    )
}

/// Saves a job's report. A failure tells the admins, at most once per job every 6 hours. Returns true when they were told.
pub fn record_report(c: &rusqlite::Connection, job: &str, ok: bool, note: &str, now: i64) -> rusqlite::Result<bool> {
    let note = clean_note(note);
    let before = read_job(c, job)?;
    let last_ok = if ok { now } else { before.as_ref().map(|b| b.last_ok).unwrap_or(0) };
    let since = before.as_ref().map(|b| b.since).filter(|s| *s > 0).unwrap_or(now);
    let saved = json!({ "at": now, "ok": ok, "note": note, "last_ok": last_ok, "since": since });
    blocks::set_setting(c, &format!("ops_job_{job}"), &saved.to_string())?;
    if ok {
        // A good run after a bad one: the warnings about it stop looking like something is still broken.
        if before.as_ref().is_some_and(|b| !b.ok) {
            mark_fixed(c, job, now)?;
        }
        return Ok(false);
    }
    let key = format!("ops_alert_{job}");
    let told: i64 = blocks::setting(c, &key)?.and_then(|s| s.parse().ok()).unwrap_or(0);
    if now - told < 6 * 3600 {
        return Ok(false);
    }
    blocks::set_setting(c, &key, &now.to_string())?;
    let why = if note.is_empty() { "no details".to_string() } else { note };
    Ok(tell_admins(c, &format!("VM: {job} failed: {why}"))? > 0)
}

/// Every VM job, with its last report: (job, when (0: never), good or not, note). For the admin dashboard.
pub fn ops_status(c: &rusqlite::Connection) -> rusqlite::Result<Vec<(String, i64, bool, String)>> {
    let mut out = vec![];
    for (job, _) in OPS_JOBS {
        out.push(match read_job(c, job)? {
            Some(s) => (job.to_string(), s.at, s.ok, s.note),
            None => (job.to_string(), 0, false, String::new()),
        });
    }
    Ok(out)
}

/// Run every hour: when a watched VM job (backup, pictures, page checks, drill, similar cards; `OPS_JOBS` hours) hasn't gone well for too long, the admins
/// are told, once a day. A job that has never reported is skipped: the VM isn't set up yet.
/// This also catches a VM that stopped, because then nothing reports at all. Returns how many jobs are late.
pub fn ops_watchdog(c: &rusqlite::Connection, now: i64) -> rusqlite::Result<usize> {
    // Warnings about jobs that are fine now are marked fixed (this also tidies warnings from before a job was mended).
    for (job, hours) in OPS_JOBS {
        if let Some(s) = read_job(c, job)? {
            if s.ok && s.last_ok > 0 && (hours == 0 || now - s.last_ok <= hours * 3600) {
                mark_fixed(c, job, s.last_ok)?;
            }
        }
    }
    let mut late = 0;
    for job in ["backup", "images", "checks", "drill", "similar"] {
        let Some(s) = read_job(c, job)? else { continue };
        let hours = late_after_hours(job);
        let good = if s.last_ok > 0 { s.last_ok } else { s.since };
        if good <= 0 || now - good <= hours * 3600 {
            continue;
        }
        late += 1;
        let key = format!("ops_late_{job}");
        let told: i64 = blocks::setting(c, &key)?.and_then(|v| v.parse().ok()).unwrap_or(0);
        if now - told < 86_400 {
            continue;
        }
        blocks::set_setting(c, &key, &now.to_string())?;
        let what = if s.last_ok > 0 { format!("hasn't worked for {} hours", (now - s.last_ok) / 3600) } else { "hasn't worked yet".to_string() };
        tell_admins(c, &format!("VM: the {job} job {what}. Please check the VM."))?;
    }
    Ok(late)
}

#[derive(Deserialize)]
struct ReportReq {
    job: String,
    ok: bool,
    #[serde(default)]
    note: String,
}

fn json_reply(status: StatusCode, v: Value) -> Response {
    (status, [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], v.to_string()).into_response()
}

/// `POST /admin/ops/report` with `{"job": "backup", "ok": true, "note": "..."}`: a VM job says how it went.
pub async fn ops_report(State(app): State<App>, h: HeaderMap, body: axum::body::Bytes) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(deny());
    }
    let Ok(r) = serde_json::from_slice::<ReportReq>(&body) else {
        return Ok(json_reply(StatusCode::BAD_REQUEST, json!({ "ok": false, "error": "Send JSON with job, ok and note." })));
    };
    let job = r.job.trim().to_lowercase();
    if !OPS_JOBS.iter().any(|j| j.0 == job) {
        return Ok(json_reply(StatusCode::BAD_REQUEST, json!({ "ok": false, "error": "Unknown job." })));
    }
    let now = crate::unix_now();
    let (ok, note) = (r.ok, r.note);
    let told = with_db(&app, move |c| record_report(c, &job, ok, &note, now)).await?;
    Ok(json_reply(StatusCode::OK, json!({ "ok": true, "told": told })))
}

// ---------- Work the VM does for the site ----------

#[derive(Deserialize)]
pub struct PendingQs {
    limit: Option<i64>,
}

/// `GET /admin/photos/pending`: trainers' own card photos the VM hasn't tidied yet (id and the large picture).
pub async fn photos_pending(State(app): State<App>, h: HeaderMap, Query(q): Query<PendingQs>) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(deny());
    }
    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    let rows = with_db(&app, move |c| {
        let mut st = c.prepare(
            "SELECT id, image_large FROM member_cards WHERE card_id LIKE 'own:%' AND photo_clean=0 AND image_large LIKE '/uploads/%' ORDER BY id LIMIT ?1",
        )?;
        let v: Vec<Value> = st.query_map([limit], |r| Ok(json!({ "id": r.get::<_, i64>(0)?, "image": r.get::<_, String>(1)? })))?.collect::<Result<_, _>>()?;
        Ok(v)
    })
    .await?;
    Ok(json_reply(StatusCode::OK, json!({ "photos": rows })))
}

#[derive(Deserialize)]
pub struct CleanQs {
    skip: Option<u8>,
}

/// `POST /admin/photos/{id}`: the VM sends back the tidied photo (the JPEG is the request body), or `?skip=1`
/// when it found no card to straighten. The original stays on disk, so the trainer can put it back.
pub async fn photo_clean(State(app): State<App>, h: HeaderMap, axum::extract::Path(id): axum::extract::Path<i64>, Query(q): Query<CleanQs>, body: axum::body::Bytes) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(deny());
    }
    let row: Option<(i64, i64)> = with_db(&app, move |c| {
        Ok(c.query_row("SELECT member_id, photo_clean FROM member_cards WHERE id=?1 AND card_id LIKE 'own:%'", [id], |r| Ok((r.get(0)?, r.get(1)?))).ok())
    })
    .await?;
    let Some((member, state)) = row else { return Ok(json_reply(StatusCode::NOT_FOUND, json!({ "ok": false, "error": "No such photo." }))) };
    if state != 0 {
        return Ok(json_reply(StatusCode::CONFLICT, json!({ "ok": false, "error": "Already done." })));
    }
    if q.skip.unwrap_or(0) == 1 || body.is_empty() {
        with_db(&app, move |c| c.execute("UPDATE member_cards SET photo_clean=2 WHERE id=?1", [id]).map(|_| ())).await?;
        return Ok(json_reply(StatusCode::OK, json!({ "ok": true, "skipped": true })));
    }
    let dir = PathBuf::from(&*app.uploads);
    let stem = format!("m{member}-{}-t", members::temp_password());
    let bytes = body.to_vec();
    let made = crate::community::gated(move || crate::community::store_photo(bytes, dir, stem)).await?;
    let Ok((large, small, _, _, size)) = made else {
        with_db(&app, move |c| c.execute("UPDATE member_cards SET photo_clean=2 WHERE id=?1", [id]).map(|_| ())).await?;
        return Ok(json_reply(StatusCode::BAD_REQUEST, json!({ "ok": false, "error": "That picture couldn't be read." })));
    };
    with_db(&app, move |c| {
        c.execute(
            "UPDATE member_cards SET photo_original=image || '|' || image_large, image=?2, image_large=?3, photo_clean=1, photo_bytes=photo_bytes+?4 WHERE id=?1 AND photo_clean=0",
            rusqlite::params![id, small, large, size],
        )
        .map(|_| ())
    })
    .await?;
    app.forget();
    Ok(json_reply(StatusCode::OK, json!({ "ok": true })))
}

/// `GET /admin/similar/cards`: every card in the vault once (copies grouped), with what the VM needs to work out
/// which cards are alike: the catalogue id, name, set, finish, condition and price.
pub async fn similar_cards(State(app): State<App>, h: HeaderMap) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(deny());
    }
    let rows = with_db(&app, |c| {
        let sql = format!(
            "SELECT MIN(p.id), group_concat(p.id), p.api_id, p.title, p.card_set, p.finish, p.variant, MIN(p.price), MAX(p.available)
             FROM products p GROUP BY {} ORDER BY MIN(p.id)",
            marcuru_rs::db::DUP_KEY
        );
        let mut st = c.prepare(&sql)?;
        let v: Vec<Value> = st
            .query_map([], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?, "ids": r.get::<_, String>(1)?, "api_id": r.get::<_, String>(2)?,
                    "title": r.get::<_, String>(3)?, "set": r.get::<_, String>(4)?, "finish": r.get::<_, String>(5)?,
                    "cond": r.get::<_, String>(6)?, "price": r.get::<_, f64>(7)?, "available": r.get::<_, i64>(8)? != 0,
                }))
            })?
            .collect::<Result<_, _>>()?;
        Ok(v)
    })
    .await?;
    Ok(json_reply(StatusCode::OK, json!({ "cards": rows })))
}

#[derive(Deserialize)]
struct SimilarRow {
    id: i64,
    picks: Vec<i64>,
}
#[derive(Deserialize)]
struct SimilarReq {
    cards: Vec<SimilarRow>,
}

/// `POST /admin/similar` with `{"cards": [{"id": 12, "picks": [40, 7, ...]}, ...]}`: tonight's "More like this".
pub async fn similar_save(State(app): State<App>, h: HeaderMap, body: axum::body::Bytes) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(deny());
    }
    let Ok(r) = serde_json::from_slice::<SimilarReq>(&body) else {
        return Ok(json_reply(StatusCode::BAD_REQUEST, json!({ "ok": false, "error": "Send JSON with cards." })));
    };
    let rows: Vec<(i64, Vec<i64>)> = r.cards.into_iter().map(|x| (x.id, x.picks)).collect();
    let now = crate::unix_now();
    let n = with_db(&app, move |c| marcuru_rs::similar::save(c, &rows, now)).await?;
    app.forget();
    Ok(json_reply(StatusCode::OK, json!({ "ok": true, "saved": n })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> rusqlite::Connection {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE members(id INTEGER PRIMARY KEY, role TEXT NOT NULL DEFAULT '', blocked INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE notifications(id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, kind TEXT NOT NULL, message TEXT NOT NULL, link TEXT NOT NULL, is_read INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
             CREATE TABLE notify_prefs(member_id INTEGER NOT NULL, event TEXT NOT NULL, site INTEGER NOT NULL DEFAULT 1, email INTEGER NOT NULL DEFAULT 0, push INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(member_id, event));
             INSERT INTO members(id, role, blocked) VALUES (1, 'admin', 0), (2, 'admin', 1), (3, '', 0);",
        )
        .unwrap();
        c
    }
    fn told(c: &rusqlite::Connection) -> i64 {
        c.query_row("SELECT COUNT(*) FROM notifications", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn a_good_run_after_a_bad_one_marks_the_warning_fixed() {
        let c = db();
        let msg = |c: &rusqlite::Connection| -> String { c.query_row("SELECT message FROM notifications ORDER BY id LIMIT 1", [], |r| r.get(0)).unwrap() };
        // 22:10 UTC on 11 Nov 2026 is 6:10 in the morning of the 12th in Manila.
        assert!(record_report(&c, "uptime", false, "the site is DOWN", 1_794_435_037).unwrap());
        assert_eq!(msg(&c), "VM: uptime failed: the site is DOWN");
        assert!(!record_report(&c, "uptime", true, "Back up", 1_794_435_303).unwrap());
        assert_eq!(msg(&c), "Fixed, working again (last good run Nov 12, 6:15 AM). VM: uptime failed: the site is DOWN");
        // Another good run leaves it as it is, and a warning about another job is not touched.
        assert!(record_report(&c, "drill", false, "missing tables", 1_794_435_400).unwrap());
        assert!(!record_report(&c, "uptime", true, "fine", 1_794_435_600).unwrap());
        assert_eq!(msg(&c), "Fixed, working again (last good run Nov 12, 6:15 AM). VM: uptime failed: the site is DOWN");
        let drill: String = c.query_row("SELECT message FROM notifications ORDER BY id DESC LIMIT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(drill, "VM: drill failed: missing tables");
        assert_eq!(told(&c), 2, "marking a warning fixed never adds a note");
    }

    #[test]
    fn the_hourly_look_tidies_old_warnings_only_for_jobs_that_are_fine_now() {
        let c = db();
        let all = |c: &rusqlite::Connection| -> Vec<String> { c.prepare("SELECT message FROM notifications ORDER BY id").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect() };
        // Warnings left from before: the jobs were mended since, but nothing marked the notes.
        for m in ["VM: checks failed: exit 141", "VM: backup failed: disk full", "VM: the images job hasn't worked for 60 hours. Please check the VM."] {
            c.execute("INSERT INTO notifications(member_id, kind, message, link, created_at) VALUES(1, 'announcement', ?1, '/admin', 1)", [m]).unwrap();
        }
        let now = 1_794_465_906; // 6:45 AM on 12 Nov in Manila, plus a few hours
        c.execute("INSERT INTO settings(key, value) VALUES('ops_job_checks', ?1)", [json!({ "at": now - 60, "ok": true, "note": "8 of 8 pages fine", "last_ok": now - 60, "since": 1 }).to_string()]).unwrap();
        c.execute("INSERT INTO settings(key, value) VALUES('ops_job_backup', ?1)", [json!({ "at": now - 60, "ok": false, "note": "disk full", "last_ok": now - 90_000, "since": 1 }).to_string()]).unwrap();
        // The pictures job said "fine" three days ago and nothing since: it is late, so its warning stays.
        c.execute("INSERT INTO settings(key, value) VALUES('ops_job_images', ?1)", [json!({ "at": now - 3 * 86_400, "ok": true, "note": "ok", "last_ok": now - 3 * 86_400, "since": 1 }).to_string()]).unwrap();
        ops_watchdog(&c, now).unwrap();
        let notes = all(&c);
        assert!(notes[0].starts_with("Fixed, working again (last good run Nov 12, ") && notes[0].ends_with("VM: checks failed: exit 141"), "{}", notes[0]);
        assert_eq!(notes[1], "VM: backup failed: disk full", "still failing: left alone");
        assert_eq!(notes[2], "VM: the images job hasn't worked for 60 hours. Please check the VM.", "still late: left alone");
        ops_watchdog(&c, now + 3600).unwrap();
        assert_eq!(all(&c)[0], notes[0], "never marked twice");
    }

    #[test]
    fn manila_time_reads_like_a_clock_on_the_wall() {
        assert_eq!(manila_stamp(1_794_435_303), "Nov 12, 6:15 AM");
        assert_eq!(manila_stamp(0), "Jan 1, 8:00 AM");
        assert_eq!(manila_stamp(1_794_435_303 + 6 * 3600), "Nov 12, 12:15 PM");
        assert_eq!(manila_stamp(1_709_222_400 - 8 * 3600), "Feb 29, 4:00 PM"); // a leap day
    }

    #[test]
    fn a_failure_tells_the_admins_once_every_six_hours() {
        let c = db();
        assert!(!record_report(&c, "backup", true, "all good", 1_000_000).unwrap());
        assert_eq!(told(&c), 0, "a good report tells nobody");
        assert!(record_report(&c, "backup", false, "disk full", 1_000_100).unwrap());
        assert_eq!(told(&c), 1, "only the admin who isn't blocked");
        let msg: String = c.query_row("SELECT message FROM notifications", [], |r| r.get(0)).unwrap();
        assert_eq!(msg, "VM: backup failed: disk full");
        assert!(!record_report(&c, "backup", false, "disk full", 1_000_200).unwrap());
        assert_eq!(told(&c), 1, "not again within 6 hours");
        assert!(record_report(&c, "backup", false, "still full", 1_000_100 + 6 * 3600).unwrap());
        assert_eq!(told(&c), 2);
        let s = ops_status(&c).unwrap();
        assert_eq!(s.len(), OPS_JOBS.len(), "every VM job has a row");
        assert_eq!(s[0], ("backup".to_string(), 1_000_100 + 6 * 3600, false, "still full".to_string()));
        assert_eq!(s[1], ("prices".to_string(), 0, false, String::new()), "never reported");
        assert_eq!(read_job(&c, "backup").unwrap().unwrap().last_ok, 1_000_000, "a failure keeps the last good time");
    }

    #[test]
    fn a_late_backup_is_noticed_once_a_day() {
        let c = db();
        let t = 2_000_000;
        assert_eq!(ops_watchdog(&c, t).unwrap(), 0, "nothing reported yet: the VM isn't set up");
        record_report(&c, "backup", true, "", t).unwrap();
        record_report(&c, "images", true, "", t).unwrap();
        assert_eq!(ops_watchdog(&c, t + 29 * 3600).unwrap(), 0);
        assert_eq!(ops_watchdog(&c, t + 31 * 3600).unwrap(), 1, "the backup is late, the pictures aren't yet");
        assert_eq!(told(&c), 1);
        ops_watchdog(&c, t + 32 * 3600).unwrap();
        assert_eq!(told(&c), 1, "once a day");
        assert_eq!(ops_watchdog(&c, t + 51 * 3600).unwrap(), 2);
        assert_eq!(told(&c), 2, "the picture job is told about now; the backup waits for its day");
    }

    #[test]
    fn notes_are_one_short_line() {
        assert_eq!(clean_note("a\nb\t c"), "a b c");
        assert_eq!(clean_note(&"x".repeat(400)).chars().count(), 300);
        assert_eq!(late_after_hours("images"), 50);
        assert_eq!(late_after_hours("uptime"), 0);
    }

    #[test]
    fn each_finish_gets_its_own_price() {
        let card = json!({ "finishes": [{ "key": "holofoil", "php": 900.0 }, { "key": "normal", "php": 200.0 }, { "key": "reverseHolofoil", "php": 750.0 }] });
        assert_eq!(finish_price(&card, "reverseHolofoil"), Some(750.0));
        assert_eq!(finish_price(&card, "normal"), Some(200.0));
        assert_eq!(finish_price(&card, ""), Some(900.0), "no finish: the first one");
        let wotc = json!({ "finishes": [{ "key": "1st-edition", "php": 5000.0 }, { "key": "unlimited", "php": 800.0 }] });
        assert_eq!(finish_price(&wotc, "normal"), Some(800.0), "Unlimited is TCGdex's name for normal");
        assert_eq!(finish_price(&wotc, "holofoil"), None, "a known finish that isn't listed never borrows the 1st Edition price");
        assert_eq!(finish_price(&wotc, "someNewFinish"), Some(800.0), "a finish this site doesn't know takes the common printing");
        assert_eq!(finish_price(&wotc, ""), Some(800.0), "no finish: Unlimited, never the 1st Edition price");
        assert_eq!(member_finish_price(&wotc, "Normal"), Some(800.0), "a seller's 'Normal' is the Unlimited price");
        assert_eq!(member_finish_price(&wotc, ""), Some(800.0));
        assert_eq!(member_finish_price(&wotc, "1st Edition"), Some(5000.0), "typed words find the dashed key");
        let wotc_holo = json!({ "finishes": [{ "key": "1st-edition-holofoil", "php": 9000.0 }, { "key": "unlimited-holofoil", "php": 1500.0 }] });
        assert_eq!(finish_price(&wotc_holo, ""), Some(1500.0));
        assert_eq!(finish_price(&wotc_holo, "holofoil"), Some(1500.0));
        assert_eq!(member_finish_price(&wotc_holo, "HOLOFOIL"), Some(1500.0), "any letter case");
        let labelled = json!({ "finishes": [{ "key": "holofoil", "label": "Holo", "php": 900.0 }, { "key": "reverseHolofoil", "label": "Reverse Holo", "php": 750.0 }] });
        assert_eq!(member_finish_price(&labelled, "reverse holo"), Some(750.0));
        assert_eq!(member_finish_price(&labelled, "Something else"), Some(900.0), "a seller's unknown finish keeps the first, as before");
        assert_eq!(member_finish_price(&card, "reverseHolofoil"), Some(750.0));
        assert_eq!(member_finish_price(&card, ""), Some(900.0));
        assert_eq!(finish_price(&json!({ "finishes": [] }), "normal"), None);
        assert_eq!(finish_price(&json!({ "finishes": [{ "key": "normal", "php": 0.0 }] }), "normal"), None);
    }

    #[test]
    fn the_nightly_job_only_moves_market_prices_of_that_finish() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE products(id INTEGER PRIMARY KEY, api_id TEXT NOT NULL DEFAULT '', finish TEXT NOT NULL DEFAULT '', variant TEXT NOT NULL DEFAULT '',
               price REAL NOT NULL DEFAULT 0, price_src TEXT NOT NULL DEFAULT '', price_updated TEXT NOT NULL DEFAULT '', market_php REAL NOT NULL DEFAULT 0, market_at INTEGER NOT NULL DEFAULT 0);
             INSERT INTO products(id, api_id, finish, variant, price, price_src, market_php) VALUES
               (1, 'x-1', 'normal', 'Near Mint', 100, 'suggested', 100),
               (2, 'x-1', 'reverseHolofoil', 'Near Mint', 750, 'suggested', 700),
               (3, 'x-1', 'normal', 'Near Mint', 555, 'own', 100),
               (4, 'x-1', 'normal', 'Near Mint', 444, '', 100);",
        )
        .unwrap();
        c.execute_batch(marcuru_rs::history::SCHEMA).unwrap();
        let shares = pricing::current();
        let n = save_market(&c, "x-1", "normal", Some(200.0), 10, "2026-11-11", &shares).unwrap();
        let kept: f64 = c.query_row("SELECT php FROM price_history WHERE api_id='x-1' AND finish='normal' AND day='2026-11-11'", [], |r| r.get(0)).unwrap();
        assert_eq!(kept, 200.0, "today's market price goes into the history");
        assert_eq!(n, 1);
        let price = |id: i64| -> f64 { c.query_row("SELECT price FROM products WHERE id=?1", [id], |r| r.get(0)).unwrap() };
        let market = |id: i64| -> f64 { c.query_row("SELECT market_php FROM products WHERE id=?1", [id], |r| r.get(0)).unwrap() };
        assert_eq!(price(1), pricing::suggest_with_shares(&shares, 200.0, "Near Mint").unwrap());
        assert_eq!(price(2), 750.0, "the Reverse Holo copy keeps its own market");
        assert_eq!(market(2), 700.0);
        assert_eq!(price(3), 555.0, "a typed price is never touched");
        assert_eq!(price(4), 444.0, "an old price that didn't say where it came from isn't touched");
        // The service didn't answer: the old market price stays, and it's asked again in about an hour.
        retry_market_soon(&c, "x-1", "reverseHolofoil", 100_000).unwrap();
        assert_eq!(market(2), 700.0);
        let at: i64 = c.query_row("SELECT market_at FROM products WHERE id=2", [], |r| r.get(0)).unwrap();
        assert_eq!(at, 100_000 - 19 * 3600);
    }

    #[test]
    fn the_catalogue_keeps_the_first_cards_and_the_count() {
        let mut v = json!({ "cards": (0..300).collect::<Vec<i32>>() });
        assert_eq!(trim_catalog(&mut v), 300);
        assert_eq!(v["cards"].as_array().unwrap().len(), CATALOG_SHOWN);
        assert_eq!(v["total"], 300);
        let mut odd = Value::Null;
        assert_eq!(trim_catalog(&mut odd), 0);
    }
}
