//! Read-only data for the card finder (`/cards.tsv`) and for AI assistants using the site through WebMCP (`/api/*`).
//! Everything here is public information that the pages already show. Nothing here can change anything.
use crate::{admin, unix_now, with_db, App, Res};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use marcuru_rs::{ask, db, market, members, suggest, translate, pricing};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use vault_search::{parse, peso, read_words, search, Sort};

async fn check_auth(app: &App, h: &HeaderMap) -> bool {
    if admin::power(app, h) != admin::Power::Nothing {
        return true;
    }
    let cookie = |name: &str| -> Option<String> {
        h.get(header::COOKIE)?.to_str().ok()?.split(';').find_map(|p| p.trim().strip_prefix(&format!("{name}=")).map(str::to_owned))
    };
    if let Some(raw) = cookie("bbv_trainer") {
        if let Ok(Some(_)) = with_db(app, move |c| members::from_session(c, &raw)).await {
            return true;
        }
    }
    false
}

pub async fn condition_shares(State(app): State<App>, h: HeaderMap) -> Res {
    if !check_auth(&app, &h).await {
        return Ok((StatusCode::NOT_FOUND, "Not found").into_response());
    }
    let shares: HashMap<String, f64> = pricing::current();
    Ok(json(json!(shares)))
}

#[derive(Deserialize)]
pub struct PriceSuggestQs {
    id: String,
    #[serde(default)]
    finish: String,
    #[serde(default)]
    cond: String,
}

pub async fn price_suggest(State(app): State<App>, h: HeaderMap, Query(q): Query<PriceSuggestQs>) -> Res {
    if !check_auth(&app, &h).await {
        return Ok((StatusCode::NOT_FOUND, "Not found").into_response());
    }
    let who = crate::community::client_ip(&h);
    let key_limit = format!("price_suggest|{}", who);
    let n = app.limits.get(&key_limit).unwrap_or(0) + 1;
    app.limits.insert(key_limit, n);
    if n > 600 {
        return Ok((StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response());
    }

    let key = format!("id|{}", q.id);
    let cached = app.prices.get(&key);
    let m = if let Some(hit) = cached {
        hit
    } else {
        match market::by_id_lang(&app.http, &app.fx, &q.id).await {
            Ok(Some(v)) => {
                let rate = market::fx(&app.http, &app.fx).await.map(|f| f.usd_php).unwrap_or(0.0);
                let hit = Arc::new(json!({ "matches": [v], "rate": rate, "exact": true }));
                app.prices.insert(key.clone(), hit.clone());
                hit
            }
            Ok(None) => Arc::new(json!({ "matches": [], "exact": true })),
            Err(_) => return Ok((StatusCode::INTERNAL_SERVER_ERROR, "Market lookup failed").into_response()),
        }
    };

    let Some(card) = m["matches"].as_array().and_then(|arr| arr.first()) else {
        return Ok(json(json!({
            "near_mint": 0.0, "finish": "", "finish_label": "", "condition": q.cond,
            "share": serde_json::Value::Null, "suggested": serde_json::Value::Null,
            "graded": pricing::is_graded(&q.cond), "source": "", "updated": "", "finishes": []
        })));
    };

    let source = card["source"].as_str().unwrap_or("TCGplayer");
    let updated = card["updated"].as_str().unwrap_or("");
    let finishes_arr = card["finishes"].as_array();
    
    let target_finish = finishes_arr.and_then(|arr| {
        arr.iter().find(|f| f["key"].as_str().unwrap_or("") == q.finish)
            .or_else(|| arr.first())
    });

    let (near_mint, finish_key, finish_label) = match target_finish {
        Some(f) => (f["php"].as_f64().unwrap_or(0.0), f["key"].as_str().unwrap_or(""), f["label"].as_str().unwrap_or("")),
        None => (0.0, "", ""),
    };

    let shares = pricing::current();
    let is_graded = pricing::is_graded(&q.cond);
    let is_sealed = pricing::is_sealed(&q.cond);
    let share = if !is_graded && !is_sealed { pricing::factor_with_shares(&shares, &q.cond) } else { None };
    let suggested = if !is_graded && !is_sealed { pricing::suggest_with_shares(&shares, near_mint, &q.cond) } else { None };

    let finishes: Vec<serde_json::Value> = finishes_arr.map(|arr| {
        arr.iter().map(|f| {
            let nm = f["php"].as_f64().unwrap_or(0.0);
            let sug = if !is_graded && !is_sealed { pricing::suggest_with_shares(&shares, nm, &q.cond) } else { None };
            json!({ "key": f["key"], "label": f["label"], "near_mint": nm, "suggested": sug })
        }).collect()
    }).unwrap_or_default();

    Ok(json(json!({
        "near_mint": near_mint,
        "finish": finish_key,
        "finish_label": finish_label,
        "condition": q.cond,
        "share": share,
        "suggested": suggested,
        "graded": is_graded,
        "source": source,
        "updated": updated,
        "finishes": finishes,
    })))
}

async fn catalogue(app: &App) -> Result<Arc<String>, (StatusCode, String)> {
    const KEY: &str = "cards.tsv";
    if let Some(hit) = app.cache.get(KEY) {
        return Ok(hit);
    }
    let since = app.change_count();
    let body = Arc::new(with_db(app, db::catalogue).await?);
    app.keep(KEY.to_string(), body.clone(), since);
    Ok(body)
}

fn json(v: serde_json::Value) -> Response {
    ([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-cache")], v.to_string()).into_response()
}

/// FNV-1a: only used to tell whether the catalogue has changed.
fn fingerprint(s: &str) -> String {
    let h = s.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
    format!("\"{h:016x}\"")
}

/// The whole catalogue for the card finder. Unchanged since the visitor's last download? They get a tiny "not modified" reply.
pub async fn cards_file(State(app): State<App>, headers: HeaderMap) -> Res {
    let body = catalogue(&app).await?;
    let tag = fingerprint(&body);
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(tag.as_str()) {
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response());
    }
    // Browsers revalidate every time (the ETag makes that a cheap 304); the CDN keeps it for half a minute.
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()), (header::CACHE_CONTROL, "public, max-age=0, s-maxage=30, stale-while-revalidate=120".to_string()), (header::ETAG, tag)], (*body).clone()).into_response())
}

#[derive(Deserialize)]
pub struct SearchQs {
    q: Option<String>,
    min_price: Option<f64>,
    max_price: Option<f64>,
    binder: Option<String>,
    graded: Option<bool>,
    sort: Option<String>,
    limit: Option<usize>,
}

/// Same search as the card finder: both are the `vault-search` crate.
pub async fn search_cards(State(app): State<App>, Query(p): Query<SearchQs>) -> Res {
    let body = catalogue(&app).await?;
    let (binders, cards) = parse(&body);
    let typed: String = p.q.unwrap_or_default().chars().take(120).collect();
    let mut f = read_words(&typed);
    f.min_price = f.min_price.or(p.min_price);
    f.max_price = f.max_price.or(p.max_price);
    f.graded |= p.graded.unwrap_or(false);
    f.binder = match p.binder.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => {
            let want = suggest::norm(b);
            match binders.iter().position(|x| x.handle == b || suggest::norm(&x.title) == want) {
                Some(i) => Some(i as u16),
                None => return Ok(json(serde_json::json!({ "error": format!("There is no binder called {b}. Use list_binders to see them."), "binders": binders.iter().map(|x| &x.title).collect::<Vec<_>>() }))),
            }
        }
        None => None,
    };
    let found = search(&cards, &f, Sort::parse(p.sort.as_deref().unwrap_or("")));
    // A card with every copy sold is only a record now, so assistants aren't offered it.
    let hits: Vec<usize> = found.hits.into_iter().filter(|&i| cards[i].qty > 0).collect();
    let limit = p.limit.unwrap_or(12).clamp(1, 50);
    let rows: Vec<_> = hits
        .iter()
        .take(limit)
        .map(|&i| {
            let c = &cards[i];
            serde_json::json!({
                "title": c.title, "handle": c.handle, "url": format!("/products/{}", c.handle), "price": c.price, "price_text": peso(c.price),
                "set": c.set, "number": c.no, "condition": c.variant, "grade": c.grade, "copies": c.qty, "image": c.img,
            })
        })
        .collect();
    Ok(json(serde_json::json!({ "total_matches": hits.len(), "returned": rows.len(), "closest_spelling": found.typo, "currency": "PHP", "cards": rows })))
}

pub async fn card(State(app): State<App>, Path(handle): Path<String>) -> Res {
    let h = handle.clone();
    let r = with_db(&app, move |c| {
        let Some(p) = db::product(c, &h)? else { return Ok(None) };
        // Only copies still for sale: sold ones stay in the records but can't be bought.
        let copies = db::copies_for_sale(c, p.id)?;
        let mut st = c.prepare("SELECT co.title FROM collections co JOIN collection_products cp ON cp.collection=co.handle WHERE cp.product_id=?1 ORDER BY co.position")?;
        let binders = st.query_map([p.id], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok(Some((p, copies, binders)))
    })
    .await?;
    Ok(json(match r {
        Some((p, copies, binders)) => serde_json::json!({
            "title": p.title, "handle": p.handle, "url": format!("/products/{}", p.handle), "price": p.price, "price_text": p.price_text(), "currency": "PHP",
            "price_updated": p.price_updated, "set": p.card_set, "number": p.card_no, "condition": p.variant, "grade": p.grade(), "copies": copies, "for_sale": copies > 0,
            "binders": binders, "notes": p.notes(), "image": p.img(900),
        }),
        None => serde_json::json!({ "error": format!("No card with the handle {handle}. Use search_cards to find it.") }),
    }))
}

#[derive(Deserialize)]
pub struct TextQs {
    to: String,
}

/// A card page's text in English, Japanese or Chinese, for the language switch on every card page.
pub async fn card_text(State(app): State<App>, Path(handle): Path<String>, Query(p): Query<TextQs>) -> Res {
    let Some(to) = translate::LANGS.iter().find(|l| **l == p.to).copied() else {
        return Ok((StatusCode::BAD_REQUEST, "Language must be en, ja or zh").into_response());
    };
    let key = format!("text|{to}|{handle}");
    if let Some(hit) = app.cache.get(&key) {
        return Ok(([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-cache")], (*hit).clone()).into_response());
    }
    let since = app.change_count();
    let h = handle.clone();
    let found = with_db(&app, move |c| {
        let Some(p) = db::product(c, &h)? else { return Ok(None) };
        let related = db::related(c, &p, 12)?;
        let versions = db::versions(c, &p)?;
        Ok(Some((p, related, versions)))
    })
    .await?;
    let Some((p, related, versions)) = found else {
        return Ok((StatusCode::NOT_FOUND, "No such card").into_response());
    };
    let names = market::name_table(&app.http).await;
    let notes = p.notes_shown().lines().map(|l| translate::note_line(l, to)).collect::<Vec<_>>().join("\n");
    let body = serde_json::json!({
        "lang": to,
        "title": translate::title(&p.title, to, &names),
        "variant": translate::variant(&p.condition_text().replace(" · ", " / "), to).replace(" / ", " · "),
        "versions": versions.iter().map(|v| {
            let parts: Vec<serde_json::Value> = [&v.condition, &v.finish, &v.rarity].iter().filter(|x| !x.is_empty()).map(|x| serde_json::Value::String(translate::variant(x, to))).collect();
            (v.handle.clone(), serde_json::Value::Array(parts))
        }).collect::<serde_json::Map<_, _>>(),
        "set": translate::set_name(&p.set_text(), to),
        "cond": translate::variant(&p.plate_condition(), to),
        "finish": translate::word(&p.plate_finish(), to),
        "rarity": translate::word(&p.plate_rarity(), to),
        "notes": notes,
        "tags": p.tag_list().iter().map(|t| translate::word(t, to)).collect::<Vec<_>>(),
        "labels": translate::labels(to),
        "related": related.iter().map(|r| (format!("/products/{}", r.handle), serde_json::Value::String(translate::title(&r.short_title(), to, &names)))).collect::<serde_json::Map<_, _>>(),
    })
    .to_string();
    app.keep(key, Arc::new(body.clone()), since);
    Ok(([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-cache")], body).into_response())
}

pub async fn binders(State(app): State<App>) -> Res {
    let (list, total) = with_db(&app, |c| Ok((db::collections(c)?, db::total(c)?))).await?;
    Ok(json(serde_json::json!({
        "total_cards": total,
        "binders": list.iter().filter(|b| b.count > 0).map(|b| serde_json::json!({ "title": b.title, "handle": b.handle, "cards": b.count, "url": format!("/collections/{}", b.handle) })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct SearchedQs {
    q: String,
}

/// The finder runs on the visitor's device, so it reports finished searches here to keep the suggestions learning.
/// Each visitor can add a few at a time (the search pages share the same limit), so nobody can flood the log.
pub async fn searched(State(app): State<App>, h: HeaderMap, Query(p): Query<SearchedQs>) -> StatusCode {
    if crate::community::over_limit(&app, format!("slog|{}", crate::community::client_ip(&h)), 60) {
        return StatusCode::NO_CONTENT;
    }
    let term: String = p.q.chars().take(60).collect();
    let _ = with_db(&app, move |c| suggest::log_search(c, &term, unix_now())).await;
    StatusCode::NO_CONTENT
}

/// Ask the Vault: a plain question in, an answer and some cards out, worked out here from the live catalogue.
#[derive(Deserialize)]
pub struct AskReq {
    q: String,
    ctx: Option<String>,
}

/// Ask the Vault: a plain question in, an answer and some cards out, worked out here from the live catalogue.
pub async fn ask_vault(State(app): State<App>, h: HeaderMap, axum::Json(req): axum::Json<AskReq>) -> Res {
    let who = crate::community::client_ip(&h);
    let key = format!("ask|{who}");
    let n = app.limits.get(&key).unwrap_or(0) + 1;
    app.limits.insert(key, n);
    if n > 240 {
        return Ok(json(serde_json::json!({ "say": "You've asked a lot in a short time. Give me a few minutes and try again.", "cards": [], "chips": [] })));
    }
    let body = catalogue(&app).await?;
    let question: String = req.q.chars().take(200).collect();
    let ctx: Option<String> = req.ctx.map(|c| c.chars().take(200).collect());
    let trainers = with_db(&app, |c| members::approved(c)).await?;
    let trainers: Vec<ask::Trainer> = trainers.into_iter().map(|t| ask::Trainer { name: t.name, stars: t.stars, worth: t.worth }).collect();
    let (mut out, handles) = tokio::task::spawn_blocking(move || {
        let (binders, cards) = parse(&body);
        let a = ask::answer(&question, ctx.as_deref(), &cards, &binders, &trainers);
        let handles: Vec<String> = a.picks.iter().map(|i| cards[*i].handle.clone()).collect();
        let tiles: Vec<serde_json::Value> = a
            .picks
            .iter()
            .map(|i| {
                let c = &cards[*i];
                serde_json::json!({ "handle": c.handle, "title": vault_search::title_and_set(&c.title, &c.set).0, "full": c.title, "img": c.img, "price": peso(c.price), "c": vault_search::cond_class(&c.variant), "cond": vault_search::tile_condition(&c.variant), "set": c.set, "sym": c.sym, "foil": vault_search::foil_class(&format!("{} {}", c.title, c.variant)), "grade": c.grade, "notes": c.notes, "tags": c.tags.split(',').map(str::trim).filter(|t| !t.is_empty() && db::tag_for_people(t)).collect::<Vec<_>>() })
            })
            .collect();
        (serde_json::json!({ "say": a.say, "cards": tiles, "chips": a.chips, "link": a.link.map(|(t, u)| serde_json::json!({ "text": t, "href": u })), "ctx": a.ctx }), handles)
    })
    .await
    .map_err(crate::ise)?;
    // The "+" on a tile needs the vault's own number for the card and its real price.
    // The handle is not that number, so look the card up. A sold card, or one with no price, gets no "+".
    if !handles.is_empty() {
        let buy = with_db(&app, move |c| {
            handles
                .iter()
                .map(|h| db::product(c, h).map(|p| p.filter(|p| p.available && p.price > 0.0).map(|p| (p.id, p.price))))
                .collect::<rusqlite::Result<Vec<Option<(i64, f64)>>>>()
        })
        .await?;
        if let Some(tiles) = out["cards"].as_array_mut() {
            for (t, b) in tiles.iter_mut().zip(buy) {
                if let Some((id, price)) = b {
                    t["id"] = serde_json::json!(id);
                    t["amount"] = serde_json::json!(price);
                }
            }
        }
    }
    Ok(json(out))
}