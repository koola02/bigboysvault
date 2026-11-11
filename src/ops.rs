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
pub const OPS_JOBS: [(&str, i64); 5] = [("backup", 30), ("prices", 30), ("images", 50), ("uptime", 0), ("update", 0)];

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

/// Saves a job's report. A failure tells the admins, at most once per job every 6 hours. Returns true when they were told.
pub fn record_report(c: &rusqlite::Connection, job: &str, ok: bool, note: &str, now: i64) -> rusqlite::Result<bool> {
    let note = clean_note(note);
    let before = read_job(c, job)?;
    let last_ok = if ok { now } else { before.as_ref().map(|b| b.last_ok).unwrap_or(0) };
    let since = before.as_ref().map(|b| b.since).filter(|s| *s > 0).unwrap_or(now);
    let saved = json!({ "at": now, "ok": ok, "note": note, "last_ok": last_ok, "since": since });
    blocks::set_setting(c, &format!("ops_job_{job}"), &saved.to_string())?;
    if ok {
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

/// Run every hour: when the backup (30 hours) or the picture job (50 hours) hasn't gone well for too long, the admins
/// are told, once a day. A job that has never reported is skipped: the VM isn't set up yet.
/// This also catches a VM that stopped, because then nothing reports at all. Returns how many jobs are late.
pub fn ops_watchdog(c: &rusqlite::Connection, now: i64) -> rusqlite::Result<usize> {
    let mut late = 0;
    for job in ["backup", "images"] {
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
        assert_eq!(s.len(), 5);
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
        let shares = pricing::current();
        let n = save_market(&c, "x-1", "normal", Some(200.0), 10, "2026-11-11", &shares).unwrap();
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
