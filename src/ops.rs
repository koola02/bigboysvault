//! Keeping the site fast and safe: picture formats, cache headers, a health check, and the routes the VM's
//! nightly jobs call (database snapshot, uploads list, price refresh, search warm-up). See docs/OPERATIONS.md.
use crate::{admin, with_db, App, Res};
use axum::{
    extract::{Query, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use marcuru_rs::{market, members, pricing, sellers};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

// ---------- pictures and cache headers ----------

/// `/uploads`: a browser that takes AVIF or WebP gets that file when it exists beside the JPEG (the VM's image job
/// makes them). Pictures keep for a day, and a stale one may be shown for a week while a fresh one is fetched.
pub async fn negotiate_uploads(State(app): State<App>, mut req: Request, next: Next) -> Response {
    let accept = req.headers().get(header::ACCEPT).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let path = req.uri().path().to_string();
    let lower = path.to_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".png") {
        let file = PathBuf::from(&*app.uploads).join(path.trim_start_matches('/'));
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
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::VARY, HeaderValue::from_static("Accept"));
    if res.status().is_success() {
        res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=86400, stale-while-revalidate=604800"));
    }
    res
}

/// `/static`: files asked for with a version (`?v=N`) never change, so browsers and the CDN keep them for a year.
/// Anything without a version revalidates on every load (a cheap 304).
pub async fn static_cache(req: Request, next: Next) -> Response {
    let versioned = req.uri().query().is_some_and(|q| q.split('&').any(|p| p.starts_with("v=")));
    let mut res = next.run(req).await;
    let value = if !res.status().is_success() { "no-store" } else if versioned { "public, max-age=31536000, immutable" } else { "no-cache" };
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

/// The card catalogue search, remembered for a day. The catalogue changes rarely and the service is slow.
pub async fn catalog_cached(app: &App, q: &str, lang: &str) -> Value {
    let key = format!("{lang}|{}", q.trim().to_lowercase());
    if let Some(hit) = app.catalog.get(&key) {
        return (*hit).clone();
    }
    match market::catalog(&app.http, q, lang).await {
        Ok(v) => {
            if v["cards"].as_array().is_some_and(|a| !a.is_empty()) {
                app.catalog.insert(key, Arc::new(v.clone()));
            }
            v
        }
        Err(_) => json!({ "cards": [], "error": "The card list isn't answering right now." }),
    }
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
    let today = admin::today();
    let now = crate::unix_now();
    // The vault's cards whose price came from the market, oldest first. Cards saved before `price_src` existed
    // show it by a price date.
    let products: Vec<(i64, String, String, String)> = with_db(&app, move |c| {
        let mut st = c.prepare("SELECT id, api_id, finish, variant FROM products WHERE api_id<>'' AND (price_src IN ('market','suggested') OR (price_src='' AND price_updated<>'')) ORDER BY price_updated ASC, id LIMIT ?1")?;
        let v = st.query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
        Ok(v)
    })
    .await?;
    let mut updated_products = 0usize;
    let mut failed = 0usize;
    // One lookup per catalogue card, however many copies the vault and the sellers hold.
    let mut cache: std::collections::HashMap<String, Option<Value>> = std::collections::HashMap::new();
    let shares = pricing::current();
    for (id, api_id, finish, cond) in &products {
        let card = match cache.get(api_id).cloned().flatten() {
            Some(c) => Some(c),
            None => {
                let r = market::by_id_lang(&app.http, &app.fx, api_id).await;
                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                match r {
                    Ok(v) => {
                        cache.insert(api_id.clone(), v.clone());
                        v
                    }
                    Err(_) => {
                        failed += 1;
                        None
                    }
                }
            }
        };
        let (id, today) = (*id, today.clone());
        // No condition written means Near Mint. Graded and sealed cards keep the owner's price: the raw price isn't theirs.
        let cond = if cond.trim().is_empty() { "Near Mint" } else { cond.as_str() };
        let price = card.as_ref().and_then(|c| {
            let fin = c["finishes"].as_array().cloned().unwrap_or_default();
            let pick = fin.iter().find(|f| f["key"].as_str() == Some(finish.as_str())).or_else(|| fin.first());
            let nm = pick.and_then(|f| f["php"].as_f64()).filter(|v| *v > 0.0)?;
            pricing::suggest_with_shares(&shares, nm, cond)
        });
        match price {
            Some(price) => {
                with_db(&app, move |c| c.execute("UPDATE products SET price=?2, price_updated=?3 WHERE id=?1", rusqlite::params![id, price, today]).map(|_| ())).await?;
                updated_products += 1;
            }
            // Not found, no price, graded or no answer: to the back of the queue, so the rest get their turn.
            None => with_db(&app, move |c| c.execute("UPDATE products SET price_updated=?2 WHERE id=?1 AND price_updated<?2", rusqlite::params![id, today]).map(|_| ())).await?,
        }
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
        let card = match cache.get(key).cloned().flatten() {
            Some(c) => Some(c),
            None => {
                let r = market::by_id_lang(&app.http, &app.fx, key).await;
                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                match r {
                    Ok(v) => {
                        cache.insert(key.clone(), v.clone());
                        v
                    }
                    Err(_) => {
                        failed += 1;
                        None
                    }
                }
            }
        };
        let php = card.as_ref().and_then(|c| c["finishes"].as_array()).and_then(|f| f.first()).and_then(|f| f["php"].as_f64()).unwrap_or(0.0);
        let k = key.clone();
        if php > 0.0 {
            with_db(&app, move |c| members::set_card_value(c, &k, php, now)).await?;
            updated_cards += 1;
        } else {
            with_db(&app, move |c| c.execute("UPDATE member_cards SET value_at=?2 WHERE (card_id=?1 OR card_id LIKE ?1 || '#%') AND value_at<?2", rusqlite::params![k, now]).map(|_| ())).await?;
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
            c.query_row("SELECT COUNT(*) FROM products WHERE api_id<>'' AND (price_src IN ('market','suggested') OR (price_src='' AND price_updated<>'')) AND price_updated<?1", [today], |r| r.get(0))?,
            c.query_row("SELECT COUNT(DISTINCT CASE WHEN instr(card_id,'#')>0 THEN substr(card_id,1,instr(card_id,'#')-1) ELSE card_id END) FROM member_cards WHERE card_id NOT LIKE 'own:%' AND value_at<?1", [now - 86400], |r| r.get(0))?,
        ))
    })
    .await?;
    Ok((
        [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")],
        json!({ "ok": true, "products": updated_products, "cards": updated_cards, "flagged": flagged, "failed": failed, "remaining_products": remaining_products, "remaining_cards": remaining_cards, "remaining": remaining_products + remaining_cards }).to_string(),
    )
        .into_response())
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
