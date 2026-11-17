//! Each seller's store: the seller panel under /me (dashboard, listings, binders, orders, settings,
//! notifications) and the public storefront at /store/<name>. The database side is `sellers.rs`.
use crate::community::{acting, acting_any, client_ip, fail, html, not_found, ok, over_limit, render, store_photo, gated};
use crate::{admin, chrome, with_db, App, Chrome, Res};
use askama::Template;
use axum::{
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use marcuru_rs::binder_sort;
use marcuru_rs::db::{self, Collection, Product};
use marcuru_rs::dm;
use marcuru_rs::members::{self, Member, MemberBinder, MemberCard, Notification};
use marcuru_rs::sellers::{self, Dashboard, ListFilter, Order, Orders, Patch, Pref, Review, Sale, StoreStats};
use std::collections::HashMap;
use serde::Deserialize;
use rusqlite::OptionalExtension;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// A viewer, for counting looks at a card once a day: a hash of their address and browser, never the address itself.
pub fn viewer_hash(h: &HeaderMap) -> String {
    let ua = h.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    let mut hasher = Sha256::new();
    hasher.update(client_ip(h).as_bytes());
    hasher.update(b"|");
    hasher.update(ua.as_bytes());
    let out = hasher.finalize();
    out.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// The tabs across the top of the seller panel, with what's waiting.
pub struct Panel {
    pub m: Member,
    pub tab: &'static str,
    pub todo: i64,
    pub unread: i64,
}

pub(crate) fn panel(c: &rusqlite::Connection, m: Member, tab: &'static str) -> rusqlite::Result<Panel> {
    let (r, o, q) = sellers::pending(c, m.id)?;
    let unread = members::unread_notification_count(c, m.id)?;
    Ok(Panel { m, tab, todo: r + o + q, unread })
}

/// Signed in, or sent to sign in. Pages, not JSON.
async fn signed_in(app: &App, h: &HeaderMap) -> Result<Result<Member, Response>, (StatusCode, String)> {
    Ok(match crate::community::current(app, h).await? {
        Some(m) => Ok(m),
        None => Err(Redirect::to("/login").into_response()),
    })
}

#[derive(Template)]
#[template(path = "store_off.html")]
struct StoreOffPage {
    nav: Chrome,
    m: Member,
}

/// A signed-in seller, or the page that says selling isn't on for them yet.
async fn selling(app: &App, h: &HeaderMap) -> Result<Result<Member, Response>, (StatusCode, String)> {
    let m = match signed_in(app, h).await? {
        Ok(m) => m,
        Err(r) => return Ok(Err(r)),
    };
    if m.seller {
        return Ok(Ok(m));
    }
    let nav = with_db(app, |c| chrome(c)).await?;
    Ok(Err(html(render(StoreOffPage { nav, m })?)))
}

/// JSON routes only a seller may use.
fn seller_only(m: &Member) -> Result<(), Response> {
    if m.seller { Ok(()) } else { Err(fail(StatusCode::FORBIDDEN, "Selling isn't switched on for your trainer. Ask the vault owner.")) }
}

// ---------- the seller panel ----------

#[derive(Template)]
#[template(path = "store_dash.html")]
struct DashPage {
    nav: Chrome,
    p: Panel,
    d: Dashboard,
}

pub async fn dash(State(app): State<App>, h: HeaderMap) -> Res {
    let m = match selling(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let (nav, p, d) = with_db(&app, move |c| {
        sellers::expire_due(c)?;
        Ok((chrome(c)?, panel(c, m, "dash")?, sellers::dashboard(c, id)?))
    })
    .await?;
    Ok(html(render(DashPage { nav, p, d })?))
}

/// A number in the address that may be left empty. The filters are a form, and a form sends "binder=" when no binder
/// is chosen: that means none, not a mistake. (It used to be refused, so every filter failed until a binder was picked.)
fn empty_or_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Ok(Option::<String>::deserialize(d)?.and_then(|s| s.trim().parse().ok()))
}

#[derive(Deserialize)]
pub struct ListQs {
    #[serde(default)]
    q: String,
    #[serde(default)]
    status: String,
    #[serde(default, deserialize_with = "empty_or_number")]
    binder: Option<i64>,
    #[serde(default)]
    sort: String,
    #[serde(default)]
    kind: String,
    /// Open this listing's editor straight away (links from notifications).
    #[serde(default, deserialize_with = "empty_or_number")]
    id: Option<i64>,
}

#[derive(Template)]
#[template(path = "store_listings.html")]
struct ListingsPage {
    nav: Chrome,
    p: Panel,
    cards: Vec<MemberCard>,
    binders: Vec<MemberBinder>,
    f: ListFilter,
    conditions: [&'static str; 10],
    statuses: [&'static str; 6],
    max: i64,
    total: i64,
    focus: i64,
}

pub async fn listings(State(app): State<App>, h: HeaderMap, Query(q): Query<ListQs>) -> Res {
    let m = match selling(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    // The table's own columns ("price:desc,card:asc") go back into the page cleaned; a named order goes through as it is.
    let by_column = marcuru_rs::db::clean_column_sort(&q.sort, &sellers::LISTING_SORT_COLUMNS);
    let sort = if by_column.is_empty() { q.sort.chars().take(24).collect() } else { by_column };
    let f = ListFilter { q: q.q.chars().take(60).collect(), status: q.status, binder: q.binder, sort, kind: q.kind };
    let f2 = f.clone();
    let (nav, p, cards, binders, max, total) = with_db(&app, move |c| {
        sellers::expire_due(c)?;
        let total: i64 = c.query_row("SELECT COUNT(*) FROM member_cards WHERE member_id=?1", [id], |r| r.get(0))?;
        Ok((chrome(c)?, panel(c, m, "listings")?, sellers::listings(c, id, &f2)?, members::binders_of(c, id)?, sellers::max_cards(c), total))
    })
    .await?;
    Ok(html(render(ListingsPage { nav, p, cards, binders, f, conditions: members::CONDITIONS, statuses: sellers::STATUSES, max, total, focus: q.id.unwrap_or(0) })?))
}

/// One listing's JSON, for the page to repaint a tile.
fn card_json(c: &MemberCard) -> serde_json::Value {
    json!({
        "id": c.id, "title": c.title, "number": c.number, "set": c.card_set, "image": c.image, "cond": c.cond, "finish": c.finish, "notes": c.notes,
        "qty": c.qty, "sold": c.sold, "status": c.status, "expires_at": c.expires_at, "expires_text": c.expires_text(), "binder_id": c.binder_id,
        "market": c.value, "value_own": c.value_own, "price": c.worth(), "price_text": c.price_text(), "own_photo": c.own_photo(), "is_custom": c.is_custom,
        "is_graded": c.is_graded(), "is_sealed": c.is_sealed(), "suggested": c.suggested_price(), "share": c.condition_share(), "views": c.view_n, "saves": c.save_n,
        "flagged": c.flagged, "when": c.when(), "kind": c.kind
    })
}

#[derive(Deserialize, Default)]
pub struct PatchReq {
    price: Option<f64>,
    cond: Option<String>,
    qty: Option<i64>,
    status: Option<String>,
    finish: Option<String>,
    notes: Option<String>,
    /// Present and null: take it out of its binder.
    #[serde(default, deserialize_with = "double_option")]
    binder_id: Option<Option<i64>>,
    /// Days from now (0 = no end date).
    expires_days: Option<i64>,
    title: Option<String>,
    set: Option<String>,
    number: Option<String>,
    flag: Option<bool>,
    /// "own", "sale" or "wish".
    kind: Option<String>,
}

fn double_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<i64>>, D::Error> {
    Option::<i64>::deserialize(d).map(Some)
}

impl PatchReq {
    fn patch(&self) -> Patch {
        Patch {
            price: self.price.filter(|v| v.is_finite() && (0.0..=100_000_000.0).contains(v)),
            cond: self.cond.clone(),
            qty: self.qty,
            status: self.status.clone(),
            finish: self.finish.clone(),
            notes: self.notes.clone(),
            binder_id: self.binder_id,
            expires_at: self.expires_days.map(|d| if d <= 0 { 0 } else { members::now() + d.min(365) * 86400 }),
            title: self.title.clone(),
            card_set: self.set.clone(),
            number: self.number.clone(),
        }
    }
}

pub async fn edit_listing(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<PatchReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if let Err(r) = seller_only(&m) {
        return Ok(r);
    }
    let mid = m.id;
    let (patch, flag, kind) = (req.patch(), req.flag, req.kind.clone());
    let card = with_db(&app, move |c| {
        if let Some(f) = flag {
            sellers::flag_card(c, mid, id, f)?;
        }
        if let Some(k) = kind {
            if let Err(e) = sellers::set_kind(c, mid, id, &k)? {
                return Ok(Err(e));
            }
        }
        Ok(sellers::update_listing(c, mid, id, &patch)?.ok_or("That card isn't in your store."))
    })
    .await?;
    let card = match card {
        Ok(c) => c,
        Err(e) => return Ok(fail(StatusCode::BAD_REQUEST, e)),
    };
    if m.profile_ok {
        app.forget();
    }
    let worth = with_db(&app, move |c| members::get(c, mid)).await?.map_or(0.0, |f| f.worth);
    Ok(ok(json!({ "ok": true, "card": card_json(&card), "worth": vault_search::peso(worth) })))
}

#[derive(Deserialize)]
pub struct BulkReq {
    ids: Vec<i64>,
    /// "edit" (with the same fields as one listing), "reprice" (percent), "delete".
    action: String,
    percent: Option<f64>,
    #[serde(flatten)]
    fields: PatchReq,
}

pub async fn bulk_listings(State(app): State<App>, h: HeaderMap, Json(req): Json<BulkReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if let Err(r) = seller_only(&m) {
        return Ok(r);
    }
    if req.ids.is_empty() {
        return Ok(fail(StatusCode::BAD_REQUEST, "Pick some cards first."));
    }
    let (mid, ids, action, percent, patch, kind) = (m.id, req.ids, req.action, req.percent.unwrap_or(0.0), req.fields.patch(), req.fields.kind.clone());
    if let Some(k) = kind.filter(|_| action == "edit") {
        let ids2 = ids.clone();
        with_db(&app, move |c| {
            for id in ids2.iter().take(500) {
                sellers::set_kind(c, mid, *id, &k)?.ok();
            }
            Ok(())
        })
        .await?;
    }
    let n = match action.as_str() {
        "edit" => with_db(&app, move |c| sellers::bulk_update(c, mid, &ids, &patch)).await?,
        "reprice" => {
            if !percent.is_finite() {
                return Ok(fail(StatusCode::BAD_REQUEST, "Type a percentage."));
            }
            with_db(&app, move |c| sellers::bulk_reprice(c, mid, &ids, percent)).await?
        }
        "delete" => {
            let gone = with_db(&app, move |c| sellers::bulk_delete(c, mid, &ids)).await?;
            for k in gone.iter().filter(|k| k.uploaded_photo()) {
                admin::remove_upload(&app.uploads, &k.image);
                admin::remove_upload(&app.uploads, &k.image_large);
            }
            gone.len()
        }
        _ => return Ok(fail(StatusCode::BAD_REQUEST, "Unknown action.")),
    };
    if m.profile_ok {
        app.forget();
    }
    Ok(ok(json!({ "ok": true, "n": n })))
}

#[derive(Deserialize)]
pub struct RenewReq {
    days: Option<i64>,
}

pub async fn renew_listing(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<RenewReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if let Err(r) = seller_only(&m) {
        return Ok(r);
    }
    let (mid, days) = (m.id, req.days.unwrap_or(30));
    let Some(card) = with_db(&app, move |c| sellers::renew(c, mid, id, days)).await? else { return Ok(fail(StatusCode::NOT_FOUND, "That card isn't in your store.")) };
    if m.profile_ok {
        app.forget();
    }
    Ok(ok(json!({ "ok": true, "card": card_json(&card) })))
}

// ---------- binders ----------

#[derive(Template)]
#[template(path = "store_binders.html")]
struct BindersPage {
    nav: Chrome,
    p: Panel,
    binders: Vec<MemberBinder>,
    cards: Vec<MemberCard>,
}

/// The binder tools (`static/binder-tools.js`) ask for JSON; they get JSON back, errors included, never a page.
fn wants_json(h: &HeaderMap) -> bool {
    h.get(header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("application/json"))
}

/// The seller using the binder tools: signed in, from this site, not locked, selling. Only their own binders and cards.
async fn binder_owner(app: &App, h: &HeaderMap) -> Result<Result<Member, Response>, (StatusCode, String)> {
    let m = match acting(app, h).await? {
        Ok(m) => m,
        Err(r) => return Ok(Err(r)),
    };
    Ok(seller_only(&m).map(|_| m))
}

/// GET /me/binders: the Store Manager's Binders page. Asked for JSON (the binder tools on the store page), the seller's
/// binders as they arrange them: every one, in their order, as tiles.
pub async fn binders(State(app): State<App>, h: HeaderMap) -> Res {
    if wants_json(&h) {
        let m = match binder_owner(&app, &h).await? {
            Ok(m) => m,
            Err(r) => return Ok(r),
        };
        let id = m.id;
        let tiles = with_db(&app, move |c| Ok(seller_tiles(&m, &members::binders_of(c, id)?, &sellers::storefront_listings(c, id)?, true))).await?;
        return Ok(ok(json!({ "ok": true, "binders": tiles.iter().map(BinderTile::json).collect::<Vec<_>>(), "max": members::MAX_BINDERS })));
    }
    let m = match selling(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let (nav, p, binders, cards) = with_db(&app, move |c| Ok((chrome(c)?, panel(c, m, "binders")?, members::binders_of(c, id)?, members::cards_of(c, id)?))).await?;
    Ok(html(render(BindersPage { nav, p, binders, cards })?))
}

#[derive(Deserialize)]
pub struct BinderEdit {
    title: Option<String>,
    description: Option<String>,
    visibility: Option<String>,
}

pub async fn edit_binder(State(app): State<App>, h: HeaderMap, Path(bid): Path<i64>, Json(req): Json<BinderEdit>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if let Err(r) = seller_only(&m) {
        return Ok(r);
    }
    let id = m.id;
    let okay = with_db(&app, move |c| {
        let mut changed = false;
        if let Some(t) = &req.title {
            changed |= members::rename_member_binder(c, id, bid, t)?;
        }
        if let Some(d) = &req.description {
            changed |= c.execute("UPDATE member_binders SET description=?3 WHERE id=?1 AND member_id=?2", rusqlite::params![bid, id, members::clean_text(d, 300)])? > 0;
        }
        if let Some(v) = &req.visibility {
            if ["Public", "Private", "Unlisted"].contains(&v.as_str()) {
                changed |= c.execute("UPDATE member_binders SET visibility=?3 WHERE id=?1 AND member_id=?2", rusqlite::params![bid, id, v])? > 0;
            }
        }
        Ok(changed)
    })
    .await?;
    if !okay {
        return Ok(fail(StatusCode::NOT_FOUND, "Binder not found."));
    }
    app.forget();
    let b = with_db(&app, move |c| members::binders_of(c, id)).await?.into_iter().find(|b| b.id == bid);
    Ok(ok(json!({ "ok": true, "binder": b })))
}

#[derive(Deserialize)]
pub struct OrderReq {
    ids: Vec<i64>,
}

/// The order of cards inside one binder (drag and drop). Cards not in the binder are ignored.
pub async fn order_binder(State(app): State<App>, h: HeaderMap, Path(bid): Path<i64>, Json(req): Json<OrderReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if let Err(r) = seller_only(&m) {
        return Ok(r);
    }
    let id = m.id;
    let ids: Vec<i64> = req.ids.into_iter().take(1000).collect();
    with_db(&app, move |c| members::order_binder_cards(c, id, bid, &ids)).await?;
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct SortReq {
    by: String,
}

/// The order a binder's cards take when sorted with one tap, and how many cards couldn't be looked up just now (they go
/// last; sorting again later places them). By evolution, cards not looked up before are asked about first.
pub(crate) async fn sort_order(app: &App, cards: Vec<binder_sort::SortCard>, by: binder_sort::By) -> Result<(Vec<i64>, usize), (StatusCode, String)> {
    let keys: Vec<String> = cards.iter().map(|k| binder_sort::dex_key(&k.api_id)).collect();
    let mut missing = 0;
    if by == binder_sort::By::Evolution {
        let now = crate::unix_now();
        let ask = keys.clone();
        let need = with_db(app, move |c| binder_sort::to_learn(c, &ask, now)).await?;
        if !need.is_empty() {
            let got = binder_sort::learn(&app.http, need.clone()).await;
            missing = need.len() - got.len();
            with_db(app, move |c| {
                for (k, d) in &got {
                    binder_sort::remember(c, k, d.as_ref(), now)?;
                }
                Ok(())
            })
            .await?;
        }
    }
    let dex = with_db(app, move |c| binder_sort::known(c, &keys)).await?;
    Ok((binder_sort::order(&cards, by, &dex), missing))
}

/// The cards a seller's binder editor shows: those on sale, reserved, out of stock, or hidden by the seller.
fn editor_cards(c: &rusqlite::Connection, member_id: i64, binder_id: i64) -> rusqlite::Result<Vec<MemberCard>> {
    Ok(members::cards_of_binder(c, member_id, Some(binder_id))?.into_iter().filter(|k| k.is_sale() && matches!(k.status.as_str(), "Active" | "Reserved" | "Out of stock" | "Hidden")).collect())
}

/// POST /me/binders/{id}/sort {by: "az" | "za" | "set" | "number" | "evolution"}: puts the cards of one of the seller's
/// binders in that order and saves it, as dragging does. Says the new order, first to last.
pub async fn sort_binder(State(app): State<App>, h: HeaderMap, Path(bid): Path<i64>, Json(req): Json<SortReq>) -> Res {
    let m = match binder_owner(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let Some(by) = binder_sort::By::parse(&req.by) else { return Ok(fail(StatusCode::BAD_REQUEST, "Choose how to sort the cards.")) };
    let id = m.id;
    let found = with_db(&app, move |c| {
        if !members::binders_of(c, id)?.iter().any(|b| b.id == bid) {
            return Ok(None);
        }
        let mut cards = vec![];
        for k in editor_cards(c, id, bid)? {
            let released = binder_sort::released(c, &k.card_id, &k.card_set)?;
            cards.push(binder_sort::SortCard { id: k.id, name: k.title, set: k.card_set, released, number: k.number, api_id: k.card_id });
        }
        Ok(Some(cards))
    })
    .await?;
    let Some(cards) = found else { return Ok(fail(StatusCode::NOT_FOUND, "Binder not found.")) };
    let (ids, missing) = sort_order(&app, cards, by).await?;
    let save = ids.clone();
    with_db(&app, move |c| members::order_binder_cards(c, id, bid, &save)).await?;
    app.forget();
    Ok(ok(json!({ "ok": true, "ids": ids, "missing": missing })))
}

#[derive(Deserialize)]
pub struct MoveReq {
    ids: Vec<i64>,
    binder_id: Option<i64>,
}

pub async fn move_cards(State(app): State<App>, h: HeaderMap, Json(req): Json<MoveReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if let Err(r) = seller_only(&m) {
        return Ok(r);
    }
    let (id, ids, bid) = (m.id, req.ids, req.binder_id);
    let n = with_db(&app, move |c| {
        let mut n = 0;
        for cid in ids.iter().take(500) {
            if members::move_card_to_binder(c, id, *cid, bid)? {
                n += 1;
            }
        }
        Ok(n)
    })
    .await?;
    app.forget();
    Ok(ok(json!({ "ok": true, "n": n })))
}

/// POST /me/binders/order {ids}: the seller's binders in this order (dragged on their store or in the Store Manager).
pub async fn order_binders(State(app): State<App>, h: HeaderMap, Json(req): Json<OrderReq>) -> Res {
    let m = match binder_owner(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let ids: Vec<i64> = req.ids.into_iter().take(200).collect();
    with_db(&app, move |c| members::reorder_member_binders(c, id, &ids)).await?;
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

/// A seller's card as the binder tools show it: picture, name, what it is, price, and whether it is the binder's cover.
/// `inside` names the binder it is in now (the "Add cards" picker says it will move).
/// `hidden`: the seller hid it (status Hidden): nobody else sees it, the editor shows it faded with Show.
fn binder_card_json(k: &MemberCard, cover: bool, inside: &str) -> serde_json::Value {
    let sub = [k.card_set.trim(), k.number.trim(), k.cond.trim()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
    let hidden = k.status == "Hidden";
    let note = if k.status == "Active" || hidden { String::new() } else { k.status.clone() };
    json!({ "id": k.id, "title": k.title, "sub": sub, "image": k.image, "price": if k.has_price() { k.price_text() } else { "Ask for a price".to_string() }, "cover": cover, "note": note, "inside": inside, "hidden": hidden, "qty": k.qty })
}

/// GET /me/binders/{id}/cards: the cards of one of the seller's binders as its page shows them (on sale, reserved or out
/// of stock), and the ones the seller hid, in the binder's order, for the binder's editor.
pub async fn binder_cards(State(app): State<App>, h: HeaderMap, Path(bid): Path<i64>) -> Res {
    let m = match binder_owner(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let found = with_db(&app, move |c| {
        let Some(b) = members::binders_of(c, id)?.into_iter().find(|b| b.id == bid) else { return Ok(None) };
        Ok(Some((b, editor_cards(c, id, bid)?)))
    })
    .await?;
    let Some((b, cards)) = found else { return Ok(fail(StatusCode::NOT_FOUND, "Binder not found.")) };
    Ok(ok(json!({ "ok": true, "title": b.title, "cards": cards.iter().map(|k| binder_card_json(k, b.cover_card == Some(k.id), "")).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
pub struct BinderCardsReq {
    #[serde(default)]
    add: Vec<i64>,
    #[serde(default)]
    remove: Vec<i64>,
}

/// POST /me/binders/{id}/cards {add, remove}: cards into one of the seller's binders (a card in another binder moves
/// here) or out of it (it stays in the store, in no binder). Says how many moved from another binder.
pub async fn change_binder_cards(State(app): State<App>, h: HeaderMap, Path(bid): Path<i64>, Json(req): Json<BinderCardsReq>) -> Res {
    let m = match binder_owner(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let (add, remove): (Vec<i64>, Vec<i64>) = (req.add.into_iter().take(500).collect(), req.remove.into_iter().take(500).collect());
    let done = with_db(&app, move |c| {
        let Some(a) = members::add_to_member_binder(c, id, bid, &add)? else { return Ok(None) };
        Ok(Some((a, members::remove_from_member_binder(c, id, bid, &remove)?)))
    })
    .await?;
    let Some((a, removed)) = done else { return Ok(fail(StatusCode::NOT_FOUND, "Binder not found.")) };
    app.forget();
    Ok(ok(json!({ "ok": true, "added": a.added, "moved": a.moved, "removed": removed })))
}

#[derive(Deserialize)]
pub struct ChoiceQs {
    #[serde(default)]
    q: String,
}

/// GET /me/binders/{id}/choices?q=: the seller's cards on sale not in this binder yet, for its "Add cards" picker.
pub async fn binder_choices(State(app): State<App>, h: HeaderMap, Path(bid): Path<i64>, Query(qs): Query<ChoiceQs>) -> Res {
    let m = match binder_owner(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let q: String = qs.q.trim().chars().take(80).collect();
    let found = with_db(&app, move |c| {
        if !members::binders_of(c, id)?.iter().any(|b| b.id == bid) {
            return Ok(None);
        }
        Ok(Some(members::binder_choices(c, id, bid, &q, 60)?))
    })
    .await?;
    let Some(rows) = found else { return Ok(fail(StatusCode::NOT_FOUND, "Binder not found.")) };
    Ok(ok(json!({ "ok": true, "cards": rows.iter().map(|(k, inside)| binder_card_json(k, false, inside)).collect::<Vec<_>>() })))
}

// ---------- orders ----------

#[derive(Deserialize)]
pub struct TabQs {
    #[serde(default)]
    tab: String,
}

#[derive(Template)]
#[template(path = "store_orders.html")]
struct OrdersPage {
    nav: Chrome,
    p: Panel,
    tab: String,
    selling: Orders,
    buying: Orders,
    sales: Vec<Sale>,
    purchases: Vec<Sale>,
    /// (reservation id, ways to pay, how to pay) for the buyer's accepted reservations.
    pay: Vec<(i64, String, String)>,
    methods: [&'static str; 6],
    /// What this trainer ordered from the vault itself.
    vault_orders: Vec<sellers::VaultOrder>,
    /// (ways to pay, how to pay) the vault.
    vault_pay: (String, String),
}

pub async fn orders(State(app): State<App>, h: HeaderMap, Query(q): Query<TabQs>) -> Res {
    let m = match signed_in(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    // A member who doesn't sell only has the buying side.
    let tab = match q.tab.as_str() {
        "buying" => "buying".to_string(),
        "questions" | "sales" if m.seller => q.tab.clone(),
        _ if m.seller => "selling".to_string(),
        _ => "buying".to_string(),
    };
    let (nav, p, selling, buying, sales, purchases, pay) = with_db(&app, move |c| {
        sellers::expire_due(c)?;
        let buying = sellers::orders_for_buyer(c, id)?;
        let mut pay = vec![];
        for r in buying.reservations.iter().filter(|r| r.status == "Confirmed") {
            if let Some((methods, info)) = sellers::pay_info_for(c, id, r.id)? {
                pay.push((r.id, methods, info));
            }
        }
        Ok((chrome(c)?, panel(c, m, "orders")?, sellers::orders_for_seller(c, id)?, buying, sellers::sales_of(c, id, 100)?, sellers::purchases_of(c, id, 100)?, pay))
    })
    .await?;
    let (vault_orders, vault_pay) = with_db(&app, move |c| Ok((sellers::vault_orders_of_buyer(c, id)?, sellers::vault_pay(c)?))).await?;
    Ok(html(render(OrdersPage { nav, p, tab, selling, buying, sales, purchases, pay, methods: sellers::PAY_METHODS, vault_orders, vault_pay })?))
}

fn order_json(o: &Order) -> serde_json::Value {
    json!({ "id": o.id, "kind": o.kind, "status": o.status, "price": o.price, "counter": o.counter, "qty": o.qty, "total": o.total_text(), "line": o.buyer_line() })
}

#[derive(Deserialize)]
pub struct ReservationReq {
    /// Seller: "Confirmed", "Paid", "Declined". Buyer: "Cancelled".
    status: String,
    #[serde(default)]
    paid_via: String,
}

pub async fn answer_reservation(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<ReservationReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let mid = m.id;
    if req.status == "Cancelled" {
        let done = with_db(&app, move |c| sellers::cancel_reservation(c, mid, id)).await?;
        return Ok(if done { ok(json!({ "ok": true })) } else { fail(StatusCode::BAD_REQUEST, "That reservation can't be cancelled.") });
    }
    if let Err(r) = seller_only(&m) {
        return Ok(r);
    }
    let (status, via) = (req.status, req.paid_via);
    match with_db(&app, move |c| sellers::answer_reservation(c, mid, id, &status, &via)).await? {
        Ok(o) => {
            app.forget();
            Ok(ok(json!({ "ok": true, "order": order_json(&o) })))
        }
        Err(e) => Ok(fail(StatusCode::BAD_REQUEST, e)),
    }
}

#[derive(Deserialize)]
pub struct OfferAnswer {
    /// Seller: "Accepted", "Declined", "Countered" (+ counter). Buyer: "accept" (a counter), "withdraw".
    status: String,
    counter: Option<f64>,
}

pub async fn answer_offer(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<OfferAnswer>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let mid = m.id;
    let res = match req.status.as_str() {
        "accept" | "withdraw" => {
            let accept = req.status == "accept";
            with_db(&app, move |c| sellers::answer_counter(c, mid, id, accept)).await?.map(|_| None)
        }
        _ => {
            if let Err(r) = seller_only(&m) {
                return Ok(r);
            }
            let (status, counter) = (req.status, req.counter.unwrap_or(0.0));
            with_db(&app, move |c| sellers::answer_offer(c, mid, id, &status, counter)).await?.map(Some)
        }
    };
    match res {
        Ok(o) => {
            app.forget();
            Ok(ok(json!({ "ok": true, "order": o.map(|o| order_json(&o)) })))
        }
        Err(e) => Ok(fail(StatusCode::BAD_REQUEST, e)),
    }
}

#[derive(Deserialize)]
pub struct TextReq {
    text: String,
}

pub async fn reply_question(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<TextReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if let Err(r) = seller_only(&m) {
        return Ok(r);
    }
    let (mid, text) = (m.id, req.text);
    match with_db(&app, move |c| sellers::reply(c, mid, id, &text)).await? {
        Ok(()) => Ok(ok(json!({ "ok": true }))),
        Err(e) => Ok(fail(StatusCode::BAD_REQUEST, e)),
    }
}

#[derive(Deserialize)]
pub struct ReviewReq {
    transaction_id: i64,
    rating: i64,
    #[serde(default)]
    text: String,
}

pub async fn add_review(State(app): State<App>, h: HeaderMap, Json(req): Json<ReviewReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let mid = m.id;
    match with_db(&app, move |c| sellers::add_review(c, mid, req.transaction_id, req.rating, &req.text)).await? {
        Ok(id) => {
            app.forget();
            Ok(ok(json!({ "ok": true, "id": id })))
        }
        Err(e) => Ok(fail(StatusCode::BAD_REQUEST, e)),
    }
}

pub async fn reply_review(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<TextReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if let Err(r) = seller_only(&m) {
        return Ok(r);
    }
    let (mid, text) = (m.id, req.text);
    if !with_db(&app, move |c| sellers::reply_review(c, mid, id, &text)).await? {
        return Ok(fail(StatusCode::NOT_FOUND, "That review isn't on your store."));
    }
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

// ---------- settings ----------

#[derive(Template)]
#[template(path = "store_settings.html")]
struct SettingsPage {
    nav: Chrome,
    p: Panel,
    methods: [&'static str; 6],
    site: String,
}

pub async fn settings(State(app): State<App>, h: HeaderMap) -> Res {
    let m = match selling(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let (nav, p) = with_db(&app, move |c| Ok((chrome(c)?, panel(c, m, "settings")?))).await?;
    let site = nav.site.clone();
    Ok(html(render(SettingsPage { nav, p, methods: sellers::PAY_METHODS, site })?))
}

pub async fn save_settings(State(app): State<App>, h: HeaderMap, mut mp: Multipart) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if let Err(r) = seller_only(&m) {
        return Ok(r);
    }
    if over_limit(&app, format!("store|{}", m.id), 60) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "Too many saves. Please wait a few minutes."));
    }
    let mut s = sellers::StoreSettings {
        store_name: m.store_name.clone(),
        location: m.location.clone(),
        contact_info: m.contact_info.clone(),
        contact_public: m.contact_public,
        status: m.status.clone(),
        fb_url: m.fb_url.clone(),
        fb_group_url: m.fb_group_url.clone(),
        ig_url: m.ig_url.clone(),
        tiktok_url: m.tiktok_url.clone(),
        shopee_url: m.shopee_url.clone(),
        links: m.links.clone(),
        pay_methods: m.pay_methods.clone(),
        pay_info: m.pay_info.clone(),
        email: m.email.clone(),
        notify_views: m.notify_views,
        pay_public: m.pay_public,
    };
    let (mut cover, mut methods, mut drop_cover, mut border) = (None::<Vec<u8>>, Vec::<String>::new(), false, None::<String>);
    // An unticked box isn't sent at all. The pay_methods_sent marker says "the whole form came in",
    // so a missing box then means "off"; without the marker it means "leave it as it is".
    // The boxes are applied after the loop, so the order the fields arrive in doesn't matter.
    let (mut sent, mut contact_public, mut pay_public, mut notify_views) = (false, None::<bool>, None::<bool>, None::<bool>);
    while let Some(f) = mp.next_field().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))? {
        let key = f.name().unwrap_or("").to_string();
        if key == "cover" {
            let b = f.bytes().await.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            if !b.is_empty() {
                cover = Some(b.to_vec());
            }
            continue;
        }
        let v = f.text().await.unwrap_or_default();
        match key.as_str() {
            "store_name" => s.store_name = v,
            "location" => s.location = v,
            "contact_info" => s.contact_info = v,
            "contact_public" => contact_public = Some(v == "1" || v == "on"),
            "status" => s.status = v,
            "fb_url" => s.fb_url = v,
            "fb_group_url" => s.fb_group_url = v,
            "ig_url" => s.ig_url = v,
            "tiktok_url" => s.tiktok_url = v,
            "shopee_url" => s.shopee_url = v,
            "links" => s.links = v,
            "pay_method" => methods.push(v),
            "pay_methods_sent" => sent = true,
            "pay_info" => s.pay_info = v,
            "email" => s.email = v,
            "notify_views" => notify_views = Some(v == "1" || v == "on"),
            "pay_public" => pay_public = Some(v == "1" || v == "on"),
            "drop_cover" => drop_cover = v == "1" || v == "on",
            "store_border" => border = Some(v),
            _ => {}
        }
    }
    s.contact_public = contact_public.unwrap_or(s.contact_public && !sent);
    s.pay_public = pay_public.unwrap_or(s.pay_public && !sent);
    s.notify_views = notify_views.unwrap_or(s.notify_views && !sent);
    if sent || !methods.is_empty() {
        s.pay_methods = methods.join(",");
    }
    let border = match border.map(|b| sellers::border_ok(&b)) {
        Some(None) => return Ok(fail(StatusCode::BAD_REQUEST, BORDER_BAD)),
        Some(Some(b)) => Some(b),
        None => None,
    };
    let mut new_cover = None;
    if let Some(bytes) = cover {
        let dir = PathBuf::from(&*app.uploads);
        let stem = format!("cover{}-{}", m.id, members::temp_password());
        let made = gated(move || store_photo(bytes, dir, stem)).await?;
        match made {
            Ok((large, small, _, _, _)) => {
                admin::remove_upload(&app.uploads, &small);
                new_cover = Some(large);
            }
            Err(e) if e == crate::community::TOO_BIG => return Ok(fail(StatusCode::BAD_REQUEST, "That photo is too big. Please use a smaller one (under 16 megapixels).")),
            Err(_) => return Ok(fail(StatusCode::BAD_REQUEST, "That cover photo couldn't be read. Try a JPEG or PNG.")),
        }
    }
    // A store can't call itself Big Boys Vault (or the owner's current site name), staff or official: its cards sit
    // next to the vault's on The Vault page, each saying whose it is.
    let wanted = members::clean_line(&s.store_name, 40);
    if !wanted.is_empty() && wanted != m.store_name.trim() {
        let key = members::name_key(&wanted);
        let brand = with_db(&app, |c| marcuru_rs::blocks::setting(c, "brand")).await?.unwrap_or_default();
        let brand_key = members::name_key(&brand);
        if members::pretends_to_be_site(&key) || (!brand_key.is_empty() && key.contains(&brand_key)) {
            return Ok(fail(StatusCode::BAD_REQUEST, "That store name is reserved. Please pick another."));
        }
    }
    let (id, old_cover) = (m.id, m.cover_photo.clone());
    let cover_now = new_cover.clone();
    with_db(&app, move |c| {
        sellers::save_store(c, id, &s)?;
        if let Some(b) = &border {
            sellers::set_border(c, id, b)?;
        }
        if let Some(p) = &cover_now {
            c.execute("UPDATE members SET cover_photo=?2 WHERE id=?1", rusqlite::params![id, p])?;
        } else if drop_cover {
            c.execute("UPDATE members SET cover_photo='' WHERE id=?1", [id])?;
        }
        Ok(())
    })
    .await?;
    if (new_cover.is_some() || drop_cover) && old_cover.starts_with("/uploads/") {
        admin::remove_upload(&app.uploads, &old_cover);
    }
    app.forget();
    let me = with_db(&app, move |c| members::get(c, id)).await?;
    Ok(ok(json!({ "ok": true, "cover": me.as_ref().map(|m| m.cover_photo.clone()), "status": me.as_ref().map(|m| m.status.clone()) })))
}

#[derive(Deserialize)]
pub struct SlugReq {
    slug: String,
}

pub async fn save_slug(State(app): State<App>, h: HeaderMap, Json(req): Json<SlugReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if let Err(r) = seller_only(&m) {
        return Ok(r);
    }
    let (id, slug) = (m.id, req.slug);
    match with_db(&app, move |c| sellers::set_slug(c, id, &slug)).await? {
        Ok(true) => {
            app.forget();
            let me = with_db(&app, move |c| members::get(c, id)).await?;
            Ok(ok(json!({ "ok": true, "slug": me.as_ref().map(|m| m.slug.clone()), "url": me.as_ref().map(|m| m.store_url()) })))
        }
        Ok(false) => Ok(fail(StatusCode::CONFLICT, "Someone already has that store address.")),
        Err(e) => Ok(fail(StatusCode::BAD_REQUEST, e)),
    }
}

// ---------- notifications ----------

#[derive(Deserialize)]
pub struct PageQs {
    page: Option<i64>,
}

#[derive(Template)]
#[template(path = "store_notifications.html")]
struct NotificationsPage {
    nav: Chrome,
    p: Panel,
    notes: Vec<Notification>,
    page: i64,
    pages: i64,
    prefs: Vec<Pref>,
    push_key: String,
    has_email: bool,
}

pub async fn notifications(State(app): State<App>, h: HeaderMap, Query(q): Query<PageQs>) -> Res {
    let m = match signed_in(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let (id, page, has_email) = (m.id, q.page.unwrap_or(0).max(0), !m.email.is_empty());
    const PER: i64 = 30;
    let (nav, p, notes, total, prefs) = with_db(&app, move |c| {
        let (notes, total) = sellers::notifications_page(c, id, page, PER)?;
        let prefs = sellers::prefs_of(c, id)?;
        // Opening the page reads what's on it.
        members::mark_notifications_read(c, id, notes.first().map(|n| n.id))?;
        Ok((chrome(c)?, panel(c, m, "notifications")?, notes, total, prefs))
    })
    .await?;
    let pages = (total + PER - 1) / PER;
    Ok(html(render(NotificationsPage { nav, p, notes, page, pages, prefs, push_key: crate::push::public_key().to_string(), has_email })?))
}

#[derive(Deserialize)]
pub struct PrefReq {
    #[serde(default)]
    event: String,
    #[serde(default)]
    site: bool,
    #[serde(default)]
    email: bool,
    #[serde(default)]
    push: bool,
    /// Sent instead of a switch: the member's email address for notifications (buyers have no Store settings page).
    address: Option<String>,
}

pub async fn save_pref(State(app): State<App>, h: HeaderMap, Json(req): Json<PrefReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    if let Some(address) = req.address {
        if over_limit(&app, format!("email|{id}"), 20) {
            return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "Too many changes. Please wait a few minutes."));
        }
        let typed = !address.trim().is_empty();
        let saved = with_db(&app, move |c| sellers::set_email(c, id, &address)).await?;
        if typed && saved.is_empty() {
            return Ok(fail(StatusCode::BAD_REQUEST, "That doesn't look like an email address."));
        }
        return Ok(ok(json!({ "ok": true, "email": saved })));
    }
    if !with_db(&app, move |c| sellers::set_pref(c, id, &req.event, req.site, req.email, req.push)).await? {
        return Ok(fail(StatusCode::BAD_REQUEST, "Unknown kind of notification."));
    }
    Ok(ok(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct PushReq {
    endpoint: String,
    #[serde(default)]
    keys: PushKeys,
    #[serde(default)]
    remove: bool,
}

#[derive(Deserialize, Default)]
pub struct PushKeys {
    #[serde(default)]
    p256dh: String,
    #[serde(default)]
    auth: String,
}

/// Saves a browser's push subscription (or forgets it, with `remove`). Sending is push.rs.
pub async fn push_subscribe(State(app): State<App>, h: HeaderMap, Json(req): Json<PushReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if over_limit(&app, format!("push|{}", m.id), 30) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "Too many tries. Please wait a few minutes."));
    }
    let id = m.id;
    if req.remove {
        with_db(&app, move |c| sellers::push_forget(c, &req.endpoint).map(|_| ())).await?;
        return Ok(ok(json!({ "ok": true })));
    }
    // Only the browsers' own push services, and keys that are really keys: anything else is turned away.
    if !crate::push::allowed_endpoint(&req.endpoint) || !crate::push::valid_keys(&req.keys.p256dh, &req.keys.auth) {
        return Ok(fail(StatusCode::BAD_REQUEST, "This browser's alert address isn't one we can use."));
    }
    with_db(&app, move |c| sellers::push_subscribe(c, id, &req.endpoint, &req.keys.p256dh, &req.keys.auth)).await?;
    Ok(ok(json!({ "ok": true, "enabled": true })))
}

#[derive(Deserialize)]
pub struct ForgetReq {
    endpoint: String,
}

/// POST /api/notifications/push/forget {endpoint}: this browser stops getting alerts, whoever turned them on (no
/// sign-in needed: only the browser itself knows its address). Used on signing out, and when someone else signs in.
pub async fn push_forget(State(app): State<App>, h: HeaderMap, Json(req): Json<ForgetReq>) -> Res {
    if !crate::community::same_site(&h) {
        return Ok(fail(StatusCode::FORBIDDEN, "Not from this site."));
    }
    if req.endpoint.len() > 1000 {
        return Ok(fail(StatusCode::BAD_REQUEST, "Too long."));
    }
    let n = with_db(&app, move |c| sellers::push_forget(c, &req.endpoint)).await?;
    Ok(ok(json!({ "ok": true, "forgotten": n })))
}

// ---------- the public storefront ----------

#[derive(Template)]
#[template(path = "store.html")]
struct StorePage {
    nav: Chrome,
    m: Member,
    stats: StoreStats,
    /// Each card with whether the viewer saved it.
    cards: Vec<(MemberCard, bool)>,
    /// The binders as tiles (`seller_tiles`); empty on a binder's own page.
    tiles: Vec<BinderTile>,
    /// How many of them visitors see (its owner also sees the ones that are off).
    binders_n: usize,
    binder: Option<MemberBinder>,
    /// Its Vouches count: vouches written for it plus the reviews from its finished sales (the Vouches window lists both).
    vouches: i64,
    me: Option<Member>,
    following: bool,
    is_me: bool,
    links: Vec<(String, String, String)>,
    conditions: [&'static str; 10],
}

async fn store_for(app: &App, h: &HeaderMap, slug: String, binder_slug: Option<String>) -> Res {
    let me = crate::community::current(app, h).await?;
    let s = slug.clone();
    let Some(m) = with_db(app, move |c| members::by_slug(c, &s)).await? else { return not_found(app).await };
    let is_me = me.as_ref().is_some_and(|u| u.id == m.id);
    let staff = admin::can_moderate(app, h);
    // Same rules as the trainer's page: blocked and not-yet-approved stores are for their owner and the vault's staff.
    if (m.blocked || !m.profile_ok) && !is_me && !staff {
        return not_found(app).await;
    }
    if !m.seller {
        return if is_me { Ok(Redirect::to("/me/store").into_response()) } else { not_found(app).await };
    }
    let (id, viewer) = (m.id, me.as_ref().map(|u| u.id));
    let bslug = binder_slug.clone();
    let (nav, stats, binders, binder, vouches, following, saved) = with_db(app, move |c| {
        sellers::expire_due(c)?;
        let all = members::binders_of(c, id)?;
        let binder = match &bslug {
            Some(bs) => Some(members::binder_by_slug(c, id, bs)?),
            None => None,
        };
        let binders: Vec<MemberBinder> = all.into_iter().filter(|b| b.visibility == "Public" || is_me || staff).collect();
        let following = match viewer {
            Some(v) => sellers::is_following(c, v, id)?,
            None => false,
        };
        let saved = match viewer {
            Some(v) => sellers::saved_ids(c, v)?,
            None => vec![],
        };
        let stats = sellers::store_stats(c, id)?;
        let vouches = members::store_vouch_count(c, id)? + stats.reviews;
        Ok((chrome(c)?, stats, binders, binder, vouches, following, saved))
    })
    .await?;
    let binder = match binder {
        Some(Some(b)) if b.visibility != "Private" || is_me || staff => Some(b),
        Some(_) => return not_found(app).await,
        None => None,
    };
    let bid = binder.as_ref().map(|b| b.id);
    let cards: Vec<MemberCard> = with_db(app, move |c| match bid {
        // The same cards the whole store shows (for sale: on sale, reserved or out of stock), just this binder's.
        // Highlights and wishlist cards in the binder are never offered for sale.
        Some(b) => Ok(members::cards_of_binder(c, id, Some(b))?.into_iter().filter(|k| k.is_sale() && matches!(k.status.as_str(), "Active" | "Reserved" | "Out of stock")).collect()),
        None => sellers::storefront_listings(c, id),
    })
    .await?;
    // The store page lists the binders as tiles, worked out from the cards above.
    let tiles = if bid.is_none() { seller_tiles(&m, &binders, &cards, is_me || staff) } else { vec![] };
    let cards: Vec<(MemberCard, bool)> = cards.into_iter().map(|k| { let s = saved.contains(&k.id); (k, s) }).collect();
    let mut nav = nav;
    // "Hayme's store", so the store page and the trainer page don't share one title. A store with its own name keeps it.
    let name = store_page_name(&m);
    let title = match &binder {
        Some(b) => format!("{} · {} · {}", b.title, name, nav.brand),
        None => format!("{} · {}", name, nav.brand),
    };
    // Say what the page is first, then a little of the seller's own words.
    let about = if m.bio.trim().is_empty() { String::new() } else { format!(" {}", crate::clip(&m.bio, 90)) };
    let desc = match &binder {
        Some(b) => crate::clip(&format!("{}, a Pokémon card binder from {} on {}: {} on sale.{about}", b.title, m.store_title(), nav.brand, cards.iter().filter(|(k, _)| k.on_sale()).count()), 165),
        None => crate::clip(&format!("{} sells Pokémon cards on {}: {} on sale.{about}", m.store_title(), nav.brand, stats.on_sale), 165),
    };
    nav.seo = crate::Seo {
        title,
        desc,
        path: match &binder {
            Some(b) => format!("/store/{}/binders/{}", m.slug, b.slug),
            None => m.store_url(),
        },
        image: if m.has_cover() { nav.abs(&m.cover_photo) } else if m.has_avatar() { nav.abs(&m.avatar) } else { String::new() },
        kind: "profile",
        ..Default::default()
    };
    if !m.profile_ok {
        nav.seo.path.clear();
    }
    let links = m.social_links();
    let binders_n = tiles.iter().filter(|t| !t.off).count();
    Ok(html(render(StorePage { nav, m, stats, cards, tiles, binders_n, binder, vouches, me, following, is_me, links, conditions: members::CONDITIONS })?))
}

/// One binder on a store page: two or three to a row, each with its pictures, its name and how many
/// cards its own page shows. Sellers' binders and the vault's own (`/store/vault`) share it, and `binder_tile.html`.
pub(crate) struct BinderTile {
    /// What the binder tools call it: a seller's binder by its number, a vault binder by its handle.
    pub key: String,
    pub href: String,
    pub title: String,
    /// Up to three card pictures, the cover first. Or one photo the owner uploaded (`photo`), which fills the tile.
    pub pics: Vec<String>,
    pub photo: bool,
    /// "12 cards", and for the store's owner what visitors don't see ("· private").
    pub words: String,
    /// Visitors don't see this tile on the store page; its owner (and staff) do, faded.
    pub off: bool,
    /// Every card in the binder, on sale or not (the binder tools say how many a deleted binder lets go).
    pub count: i64,
}

impl BinderTile {
    /// For the binder tools (`binder-tools.js`): the tiles as the owner sees them while arranging them.
    fn json(&self) -> serde_json::Value {
        json!({ "key": self.key, "href": self.href, "title": self.title, "pics": self.pics, "photo": self.photo, "words": self.words, "off": self.off, "count": self.count })
    }
}

/// "1 card", "1,117 cards".
fn cards_word(n: i64) -> String {
    format!("{} card{}", db::thousands(n), if n == 1 { "" } else { "s" })
}

/// A seller's binders as tiles. A tile counts the binder's cards for sale (`listings`, the store's cards on sale,
/// reserved or out of stock), which are exactly the cards its own page lists, and shows them peeking out.
/// A binder with nothing for sale has nothing to open to, so visitors don't see it; `owner_view` (the seller, or staff)
/// still does, marked, along with the binders visitors can't see at all.
/// A tile's pictures are the cover the seller picked, then the binder's cards on sale (`sellers::binder_peeks` says the
/// same for one binder alone); across the page, cards an earlier tile already shows wait their turn (`db::fresh_pics`).
fn seller_tiles(m: &Member, binders: &[MemberBinder], listings: &[MemberCard], owner_view: bool) -> Vec<BinderTile> {
    let mut seen = std::collections::HashSet::new();
    binders
        .iter()
        .filter_map(|b| {
            let n = listings.iter().filter(|k| k.binder_id == Some(b.id)).count() as i64;
            if n == 0 && !owner_view {
                return None;
            }
            let mut words = if n == 0 { "No cards for sale".to_string() } else { cards_word(n) };
            match b.visibility.as_str() {
                "Unlisted" => words.push_str(" · unlisted"),
                "Private" => words.push_str(" · private"),
                _ if n == 0 => words.push_str(" · hidden from visitors"),
                _ => {}
            }
            // A cover the seller picked leads while that card is on sale here. Everything else is the binder's cards on
            // sale, in its order: never a sold, hidden, highlight or wishlist card.
            let picked = sellers::picked_cover(b, listings);
            let cards: Vec<String> = listings.iter().filter(|k| k.binder_id == Some(b.id) && Some(k.id) != b.cover_card && !k.image.is_empty()).map(|k| k.image.clone()).collect();
            Some(BinderTile {
                key: b.id.to_string(),
                href: format!("{}/binders/{}", m.store_url(), b.slug),
                title: b.title.clone(),
                pics: db::fresh_pics(picked.map(|k| k.image.as_str()), &cards, 3, &mut seen),
                photo: false,
                words,
                off: n == 0 || b.visibility != "Public",
                count: b.count,
            })
        })
        .collect()
}

/// The vault's own binders as tiles: every binder with cards for sale, in the owner's order (Manager › Binders), whether
/// or not it is on today's home page or was ever in the menu. `peeks` is `db::binder_peeks`.
/// A tile says the binder's own count, the same number its page shows. The owner arranging them (`owner_view`) also sees
/// the binders with nothing for sale, faded, so a new binder can be filled. Pictures repeat as little as they can.
pub(crate) fn vault_tiles(links: &[Collection], peeks: &HashMap<String, (i64, Vec<String>)>, owner_view: bool) -> Vec<BinderTile> {
    let mut seen = std::collections::HashSet::new();
    let none: Vec<String> = vec![];
    links
        .iter()
        .filter_map(|b| {
            let pics = match peeks.get(&b.handle) {
                Some((_, pics)) => pics,
                None if owner_view => &none,
                None => return None,
            };
            let off = !peeks.contains_key(&b.handle);
            let words = match (off, b.count) {
                (false, n) => cards_word(n),
                (true, 0) => "No cards yet · hidden from visitors".to_string(),
                (true, n) => format!("{} · none for sale · hidden from visitors", cards_word(n)),
            };
            Some(BinderTile {
                key: b.handle.clone(),
                href: format!("/collections/{}", b.handle),
                title: b.title.clone(),
                pics: b.tile_pics_among(pics, 3, &mut seen),
                photo: b.photo_cover(),
                words,
                off,
                count: b.count,
            })
        })
        .collect()
}

/// How many pictures each vault binder offers its tile to choose from, so tiles side by side can show different cards.
const PEEK_CHOICES: usize = 12;

/// GET /admin/binders as JSON: the vault's binders as the owner arranges them (every one, in order).
pub(crate) fn vault_tiles_json(c: &rusqlite::Connection) -> rusqlite::Result<serde_json::Value> {
    let tiles = vault_tiles(&db::collections(c)?, &db::binder_peeks(c, PEEK_CHOICES)?, true);
    Ok(json!({ "ok": true, "binders": tiles.iter().map(BinderTile::json).collect::<Vec<_>>() }))
}

/// The numbers the vault's store shows, the same four every store shows: copies of its cards for sale, binders with cards
/// for sale, followers and vouches. Never what the cards are worth: the owner keeps that off the public pages.
struct VaultNumbers {
    for_sale: i64,
    binders: usize,
    followers: i64,
    vouches: i64,
}

impl VaultNumbers {
    fn for_sale_text(&self) -> String {
        db::thousands(self.for_sale)
    }
}

fn vault_numbers(c: &rusqlite::Connection, tiles: &[BinderTile]) -> rusqlite::Result<VaultNumbers> {
    let (for_sale, _, _) = admin::in_vault(c)?;
    Ok(VaultNumbers { for_sale, binders: tiles.len(), followers: sellers::followers(c, 0)?, vouches: members::store_vouch_count(c, 0)? })
}

/// "Nov 2026": the month the vault put up its first card, as a seller's store says when it opened.
fn vault_since(c: &rusqlite::Connection) -> rusqlite::Result<String> {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let first: Option<String> = c.query_row("SELECT MIN(created_at) FROM products WHERE created_at<>''", [], |r| r.get(0))?;
    let first = first.unwrap_or_default();
    let (y, m) = (first.get(..4).unwrap_or(""), first.get(5..7).and_then(|m| m.parse::<usize>().ok()).unwrap_or(0));
    Ok(if y.len() == 4 && (1..=12).contains(&m) { format!("{} {y}", M[m - 1]) } else { String::new() })
}

#[derive(Template)]
#[template(path = "store_vault.html")]
struct VaultStorePage {
    nav: Chrome,
    tiles: Vec<BinderTile>,
    n: VaultNumbers,
    /// What a seller's store header shows too: when it opened, its words, how to pay it and its border colour.
    since: String,
    bio: &'static str,
    pay: Vec<String>,
    border: String,
    /// "Everything in the vault": its cards for sale, as `grid.html` draws them.
    base: String,
    q: String,
    sort: String,
    in_stock: bool,
    items: Vec<Product>,
    next: Option<usize>,
}

/// GET /store/vault: Big Boys Vault's own store, one of the stores on /stores. Its header, its binders as tiles
/// (each opens its page at /collections/{handle}, the address everything else links to), then every card for sale.
/// Every visitor gets the same page (the staff's "Manage binders" shows in the browser), so it is kept like the binders.
pub async fn vault_store(State(app): State<App>, h: HeaderMap, Query(p): Query<crate::Params>) -> Res {
    let q = db::Query { collection: None, q: p.q.clone().unwrap_or_default().trim().chars().take(80).collect(), sort: p.sort.clone().unwrap_or_default(), in_stock: true, page: p.page_no().min(500) };
    // Scrolling down only needs the next cards.
    let more = crate::is_htmx(&h) && q.page > 0;
    let key = format!("store/vault|{more}|{q:?}");
    let a = app.clone();
    crate::cached(&app, key, || async move {
        if more {
            let q2 = q.clone();
            let (items, found) = with_db(&a, move |c| db::list(c, &q2)).await?;
            let next = next_page(&q, found);
            return render(crate::Grid { base: VAULT_STORE.into(), q: q.q, sort: q.sort, in_stock: true, items, next });
        }
        render(with_db(&a, move |c| vault_page(c, q)).await?)
    })
    .await
}

const VAULT_STORE: &str = "/store/vault";

fn next_page(q: &db::Query, found: i64) -> Option<usize> {
    ((q.page + 1) * db::PAGE_SIZE < found as usize).then_some(q.page + 1)
}

/// The vault's store page for a search of its cards for sale (`q.in_stock` is always on), with what search engines read.
fn vault_page(c: &rusqlite::Connection, q: db::Query) -> rusqlite::Result<VaultStorePage> {
    let mut nav = chrome(c)?;
    let (items, found) = db::list(c, &q)?;
    let tiles = vault_tiles(&nav.links, &db::binder_peeks(c, PEEK_CHOICES)?, false);
    let n = vault_numbers(c, &tiles)?;
    let (since, border) = (vault_since(c)?, sellers::vault_border(c)?);
    let pay: Vec<String> = sellers::vault_pay(c)?.0.split(',').map(str::trim).filter(|m| !m.is_empty()).map(String::from).collect();
    let next = next_page(&q, found);
    let site = nav.site.clone();
    let url = format!("{site}{VAULT_STORE}");
    let title = format!("{} · Store", nav.brand);
    let names = tiles.iter().take(4).map(|t| t.title.as_str()).collect::<Vec<_>>().join(", ");
    let binders = if tiles.is_empty() { String::new() } else { format!(" in {} binder{} ({names})", tiles.len(), if tiles.len() == 1 { "" } else { "s" }) };
    let list: Vec<serde_json::Value> = tiles.iter().enumerate().map(|(i, t)| json!({ "@type": "ListItem", "position": i + 1, "url": format!("{site}{}", t.href), "name": t.title })).collect();
    nav.seo = crate::Seo {
        desc: crate::clip(&format!("{}'s store: {} Pokémon cards for sale{binders}. Real photos, condition and prices in pesos.", nav.brand, db::thousands(n.for_sale)), 165),
        // A typed search here isn't a page of its own.
        path: if q.q.is_empty() { VAULT_STORE.into() } else { String::new() },
        kind: "website",
        ld: crate::ld(json!({ "@context": "https://schema.org", "@graph": [
            { "@type": "OnlineStore", "@id": format!("{site}/#org"), "name": nav.brand, "url": format!("{site}/"), "logo": format!("{site}/static/img/icon-512.png") },
            { "@type": "CollectionPage", "@id": url, "url": url, "name": title, "isPartOf": { "@id": format!("{site}/#site") }, "about": { "@id": format!("{site}/#org") },
              "mainEntity": { "@type": "ItemList", "name": "Binders", "numberOfItems": tiles.len(), "itemListElement": list } },
            { "@type": "BreadcrumbList", "itemListElement": [
                { "@type": "ListItem", "position": 1, "name": nav.brand, "item": format!("{site}/") },
                { "@type": "ListItem", "position": 2, "name": "Stores", "item": format!("{site}/stores") },
                { "@type": "ListItem", "position": 3, "name": nav.brand, "item": url } ] }
        ] })),
        title,
        ..Default::default()
    };
    Ok(VaultStorePage { nav, tiles, n, since, bio: VAULT_BIO, pay, border, base: VAULT_STORE.into(), q: q.q, sort: q.sort, in_stock: true, items, next })
}

/// GET /store/vault/binders/{handle}: a vault binder's address, should anyone write it like a seller's. Its page is /collections/{handle}.
pub async fn vault_binder(State(app): State<App>, Path(handle): Path<String>) -> Res {
    if handle.is_empty() || !handle.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return not_found(&app).await;
    }
    Ok(Redirect::permanent(&format!("/collections/{handle}")).into_response())
}

/// The store's name for titles: what the seller called it, or "Hayme's store".
fn store_page_name(m: &Member) -> String {
    if m.store_name.trim().is_empty() { format!("{}'s store", m.name) } else { m.store_name.trim().to_string() }
}

/// What a shared link to one listing shows (title, words, picture, price), for `/trainers/{id}/cards/{card}`.
/// Only for an open store (approved and not blocked); a closed one keeps the plain, unindexed page.
/// Listings for sale are indexed; highlights and wishlist cards are shared nicely but kept out of search.
pub(crate) fn listing_seo(nav: &Chrome, m: &Member, k: &MemberCard) -> crate::Seo {
    let page = format!("/trainers/{}/cards/{}", m.id, k.id);
    let url = nav.abs(&page);
    let price = if k.has_price() { k.price_text() } else { String::new() };
    let for_sale = k.is_sale() && matches!(k.status.as_str(), "Active" | "Reserved" | "Out of stock");
    let mut bits: Vec<String> = vec![];
    for b in [&k.card_set, &k.cond] {
        if !b.trim().is_empty() {
            bits.push(b.trim().to_string());
        }
    }
    if for_sale && !price.is_empty() {
        bits.push(price.clone());
    }
    let store = m.store_title();
    let title = if for_sale && !price.is_empty() { format!("{} · {} · {} · {}", k.title, price, store, nav.brand) } else { format!("{} · {} · {}", k.title, m.name, nav.brand) };
    let what = if for_sale { format!("for sale by {store}") } else if k.is_wish() { format!("wanted by {}", m.name) } else { format!("in the binder of {}", m.name) };
    let desc = crate::clip(&format!("{} {what} on {}{}.", k.title, nav.brand, if bits.is_empty() { String::new() } else { format!(": {}", bits.join(", ")) }), 165);
    let img = k.big().to_string();
    let mut ld = json!({ "@context": "https://schema.org", "@type": "Product", "name": k.title, "url": url, "sku": format!("m{}", k.id), "category": "Collectible trading cards" });
    if !img.is_empty() {
        ld["image"] = json!([nav.abs(&img)]);
    }
    if for_sale && k.has_price() {
        let stock = if k.status == "Active" && k.qty > 0 { "https://schema.org/InStock" } else { "https://schema.org/OutOfStock" };
        ld["offers"] = json!({ "@type": "Offer", "price": format!("{:.2}", k.worth()), "priceCurrency": "PHP", "url": url, "availability": stock, "seller": { "@type": "Person", "name": store } });
    }
    let open = m.profile_ok && !m.blocked;
    crate::Seo {
        title,
        desc,
        path: if open && for_sale { page } else { String::new() },
        image: if open { img } else { String::new() },
        kind: "product",
        ld: if open && for_sale { crate::ld(ld) } else { String::new() },
    }
}

pub async fn storefront(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>) -> Res {
    store_for(&app, &h, slug, None).await
}

pub async fn store_binder(State(app): State<App>, h: HeaderMap, Path((slug, bslug)): Path<(String, String)>) -> Res {
    store_for(&app, &h, slug, Some(bslug)).await
}

/// One store on the Stores page, the vault's or a trainer's. Both are worked out the same way and drawn by the same card
/// (`stores_card.html`): the same numbers, the same gold Visit button, the binders and cards its owner chose to show.
/// Every store is listed by one rule: the one that last put up or changed a card for sale comes first.
pub(crate) struct StoreCard {
    pub name: String,
    pub url: String,
    /// Its picture (empty: `initial` shows instead).
    pub avatar: String,
    pub initial: String,
    /// The vault's picture is its round logo, shown whole rather than cropped to fill the circle.
    pub logo: bool,
    pub cover: String,
    pub bio: String,
    pub on_sale: i64,
    pub sold: i64,
    pub followers: i64,
    pub location: String,
    pub pay: Vec<String>,
    /// "On vacation", "Away", or nothing.
    pub badge: &'static str,
    /// Its border colour: '' is gold (`sellers::border_ok`).
    pub border: String,
    /// Who chooses what it shows here: the seller's id, or 0 for the vault (its full admins).
    pub owner: i64,
    pub binders: Vec<BinderTile>,
    pub cards: Vec<Highlight>,
    /// When it last put up or changed a card for sale (`sellers::last_active`): the order on the page.
    pub last: i64,
}

impl StoreCard {
    pub fn on_sale_text(&self) -> String {
        db::thousands(self.on_sale)
    }
}

/// A card a store highlights on Stores, the vault's and a seller's drawn alike. It opens in the card window.
pub(crate) struct Highlight {
    pub href: String,
    pub image: String,
    pub title: String,
    pub set: String,
    pub symbol: String,
    pub cond: String,
    pub cond_class: &'static str,
    /// Empty when the seller asks for offers instead of naming a price.
    pub price: String,
}

#[derive(Template)]
#[template(path = "stores.html")]
struct StoresPage {
    nav: Chrome,
    stores: Vec<StoreCard>,
    me: Option<Member>,
}

/// The vault's own words under its name, where a trainer's store shows its owner's.
pub(crate) const VAULT_BIO: &str = "The vault's own cards, sorted into binders. Real photos, prices in pesos, and Ask the Vault for anything else.";

/// Keeps the binders the owner chose to feature, in the store's own order (Edit binders arranges that order).
fn chosen_binders(tiles: Vec<BinderTile>, keys: &[String]) -> Vec<BinderTile> {
    tiles.into_iter().filter(|t| keys.contains(&t.key)).collect()
}

/// The vault's highlighted cards, in the owner's order: only those still for sale.
fn vault_highlights(c: &rusqlite::Connection, keys: &[String]) -> rusqlite::Result<Vec<Product>> {
    let mut out = vec![];
    for id in keys.iter().filter_map(|k| k.parse::<i64>().ok()) {
        if let Some(p) = db::product_by_id(c, id)?.filter(|p| p.available) {
            out.push(p);
        }
    }
    Ok(out)
}

/// The vault's card on Stores. It shows what a full admin chose; nothing chosen yet, every binder with cards for sale
/// and no highlighted cards: the same rule as a trainer's store.
fn vault_card(c: &rusqlite::Connection, brand: &str) -> rusqlite::Result<StoreCard> {
    let tiles = vault_tiles(&db::collections(c)?, &db::binder_peeks(c, PEEK_CHOICES)?, false);
    let (binders, cards) = match sellers::featured(c, 0)? {
        None => (tiles, vec![]),
        Some(f) => (chosen_binders(tiles, &f.binders), vault_highlights(c, &f.cards)?),
    };
    let (on_sale, _, _) = admin::in_vault(c)?;
    let (methods, _) = sellers::vault_pay(c)?;
    let cards = cards
        .iter()
        .map(|p| Highlight {
            href: format!("/products/{}", p.handle),
            image: if p.has_image() { p.img(360) } else { String::new() },
            title: p.short_title(),
            set: p.set_text(),
            symbol: p.symbol(),
            cond: p.tile_cond(),
            cond_class: p.cond_class(),
            price: p.price_text(),
        })
        .collect();
    Ok(StoreCard {
        name: brand.to_string(),
        url: VAULT_STORE.into(),
        avatar: "/static/img/logo-sm-144.png?v=1".into(),
        initial: String::new(),
        logo: true,
        cover: String::new(),
        bio: VAULT_BIO.into(),
        on_sale,
        sold: sellers::vault_sold(c)?,
        followers: sellers::followers(c, 0)?,
        location: String::new(),
        pay: methods.split(',').map(str::trim).filter(|m| !m.is_empty()).map(String::from).collect(),
        badge: "",
        border: sellers::vault_border(c)?,
        owner: 0,
        binders,
        cards,
        last: sellers::last_active(c, 0)?,
    })
}

/// A seller's binders that may show off their store: public ones with cards for sale, in the store's order.
fn public_tiles(c: &rusqlite::Connection, m: &Member, listings: &[MemberCard]) -> rusqlite::Result<Vec<BinderTile>> {
    let public: Vec<MemberBinder> = members::binders_of(c, m.id)?.into_iter().filter(|b| b.visibility == "Public").collect();
    Ok(seller_tiles(m, &public, listings, false))
}

/// A seller's highlighted cards, in their order: only cards on sale right now (not reserved, not out of stock).
fn seller_highlights(keys: &[String], listings: &[MemberCard]) -> Vec<MemberCard> {
    keys.iter().filter_map(|k| listings.iter().find(|x| x.id.to_string() == *k && x.status == "Active")).cloned().collect()
}

/// A trainer's card on Stores: its binders, then its highlighted cards, as its owner chose.
/// Nothing chosen yet: every public binder with cards for sale, and no highlighted cards.
fn seller_card(c: &rusqlite::Connection, m: Member, s: StoreStats) -> rusqlite::Result<StoreCard> {
    let listings = sellers::storefront_listings(c, m.id)?;
    let tiles = public_tiles(c, &m, &listings)?;
    let (binders, cards) = match sellers::featured(c, m.id)? {
        None => (tiles, vec![]),
        Some(f) => (chosen_binders(tiles, &f.binders), seller_highlights(&f.cards, &listings)),
    };
    let cards = cards
        .iter()
        .map(|k| Highlight {
            href: format!("/trainers/{}/cards/{}", m.id, k.id),
            image: k.image.clone(),
            title: k.title.clone(),
            set: k.card_set.clone(),
            symbol: k.symbol(),
            cond: k.cond.clone(),
            cond_class: k.cond_class(),
            price: if k.has_price() { k.price_text() } else { String::new() },
        })
        .collect();
    Ok(StoreCard {
        name: m.store_title(),
        url: m.store_url(),
        avatar: if m.has_avatar() { m.avatar.clone() } else { String::new() },
        initial: m.initial(),
        logo: false,
        cover: if m.has_cover() { m.cover_photo.clone() } else { String::new() },
        bio: m.bio.clone(),
        on_sale: s.on_sale,
        sold: s.sold,
        followers: s.followers,
        location: m.location.clone(),
        pay: m.pay_list(),
        badge: if m.on_vacation() { "On vacation" } else if m.is_away() { "Away" } else { "" },
        border: sellers::border_ok(&m.store_border).unwrap_or_default(),
        owner: m.id,
        binders,
        cards,
        last: sellers::last_active(c, m.id)?,
    })
}

/// Every store with cards for sale, the vault's among them, the most recently active first.
fn store_cards(c: &rusqlite::Connection, brand: &str) -> rusqlite::Result<Vec<StoreCard>> {
    let mut out = vec![];
    let vault = vault_card(c, brand)?;
    if vault.on_sale > 0 {
        out.push(vault);
    }
    for (m, s, _) in sellers::directory(c)? {
        out.push(seller_card(c, m, s)?);
    }
    out.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

pub async fn directory(State(app): State<App>, h: HeaderMap) -> Res {
    let me = crate::community::current(&app, &h).await?;
    let (mut nav, stores) = with_db(&app, move |c| {
        let nav = chrome(c)?;
        let stores = store_cards(c, &nav.brand)?;
        Ok((nav, stores))
    })
    .await?;
    nav.seo = crate::Seo {
        title: format!("Stores · {}", nav.brand),
        desc: format!("The Pokémon card stores on {}: browse each store's binders and cards for sale, then reserve a card or add it to your cart.", nav.brand),
        path: "/stores".into(),
        kind: "website",
        ..Default::default()
    };
    Ok(html(render(StoresPage { nav, stores, me })?))
}

// ---------- what a store features on the Stores page: chosen where it shows (static/binder-tools.js) ----------

/// Who is choosing: a full admin for the vault's store, or a seller for their own.
enum Featurer {
    Vault,
    Seller(Member),
}

async fn featurer(app: &App, h: &HeaderMap, vault: bool) -> Result<Result<Featurer, Response>, (StatusCode, String)> {
    if !vault {
        return Ok(binder_owner(app, h).await?.map(Featurer::Seller));
    }
    if !crate::community::same_site(h) {
        return Ok(Err(fail(StatusCode::FORBIDDEN, "That request didn't come from this site.")));
    }
    if !admin::authed(app, h) {
        return Ok(Err(fail(StatusCode::UNAUTHORIZED, "Only a full admin can choose what the vault's store shows. Please sign in again.")));
    }
    Ok(Ok(Featurer::Vault))
}

/// A vault card as the picker shows it.
fn vault_pick_json(p: &Product) -> serde_json::Value {
    let sub = [p.set_text(), p.card_no.trim().to_string(), p.tile_cond()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
    json!({ "id": p.id, "title": p.short_title(), "sub": sub, "image": p.img(360), "price": p.price_text(), "note": "", "inside": "" })
}

/// The binders that can show and the highlighted cards, as the tile shows them now (nothing chosen: every binder).
fn feature_state(c: &rusqlite::Connection, who: &Featurer) -> rusqlite::Result<serde_json::Value> {
    let f = sellers::featured(c, match who { Featurer::Vault => 0, Featurer::Seller(m) => m.id })?;
    let keys: &[String] = f.as_ref().map_or(&[][..], |f| &f.cards);
    let (tiles, cards) = match who {
        Featurer::Vault => {
            let tiles = vault_tiles(&db::collections(c)?, &db::binder_peeks(c, PEEK_CHOICES)?, false);
            (tiles, vault_highlights(c, keys)?.iter().map(vault_pick_json).collect::<Vec<_>>())
        }
        Featurer::Seller(m) => {
            let listings = sellers::storefront_listings(c, m.id)?;
            let tiles = public_tiles(c, m, &listings)?;
            (tiles, seller_highlights(keys, &listings).iter().map(|k| binder_card_json(k, false, "")).collect::<Vec<_>>())
        }
    };
    let binders: Vec<serde_json::Value> = tiles
        .iter()
        .map(|t| {
            let mut v = t.json();
            v["on"] = json!(f.as_ref().is_none_or(|f| f.binders.contains(&t.key)));
            v
        })
        .collect();
    Ok(json!({ "ok": true, "saved": f.is_some(), "max": sellers::FEATURE_MAX, "binders": binders, "cards": cards }))
}

async fn featured_get(app: App, h: HeaderMap, vault: bool) -> Res {
    let who = match featurer(&app, &h, vault).await? {
        Ok(w) => w,
        Err(r) => return Ok(r),
    };
    let v = with_db(&app, move |c| feature_state(c, &who)).await?;
    Ok(ok(v))
}

/// GET /me/featured: what the seller's store shows on the Stores page, for the chooser.
pub async fn featured_me(State(app): State<App>, h: HeaderMap) -> Res {
    featured_get(app, h, false).await
}

/// GET /admin/featured: the same for the vault's store (full admins).
pub async fn featured_vault(State(app): State<App>, h: HeaderMap) -> Res {
    featured_get(app, h, true).await
}

async fn feature_choices(app: App, h: HeaderMap, vault: bool, q: String) -> Res {
    let who = match featurer(&app, &h, vault).await? {
        Ok(w) => w,
        Err(r) => return Ok(r),
    };
    let q: String = q.trim().chars().take(80).collect();
    let rows = with_db(&app, move |c| -> rusqlite::Result<Vec<serde_json::Value>> {
        Ok(match &who {
            Featurer::Vault => {
                let sort = if q.is_empty() { "price-desc".to_string() } else { String::new() };
                let (rows, _) = db::list(c, &db::Query { q: q.clone(), sort, in_stock: true, ..Default::default() })?;
                rows.iter().map(vault_pick_json).collect()
            }
            Featurer::Seller(m) => {
                let words: Vec<String> = q.split_whitespace().take(6).map(|w| w.to_lowercase()).collect();
                sellers::storefront_listings(c, m.id)?
                    .iter()
                    .filter(|k| k.status == "Active")
                    .filter(|k| {
                        let hay = format!("{} {} {}", k.title, k.number, k.card_set).to_lowercase();
                        words.iter().all(|w| hay.contains(w.as_str()))
                    })
                    .take(60)
                    .map(|k| binder_card_json(k, false, ""))
                    .collect()
            }
        })
    })
    .await?;
    Ok(ok(json!({ "ok": true, "cards": rows })))
}

/// GET /me/featured/choices?q=: the seller's cards on sale that can be highlighted.
pub async fn featured_me_choices(State(app): State<App>, h: HeaderMap, Query(qs): Query<ChoiceQs>) -> Res {
    feature_choices(app, h, false, qs.q).await
}

/// GET /admin/featured/choices?q=: the vault's cards for sale (its dearest first until something is typed).
pub async fn featured_vault_choices(State(app): State<App>, h: HeaderMap, Query(qs): Query<ChoiceQs>) -> Res {
    feature_choices(app, h, true, qs.q).await
}

#[derive(Deserialize)]
pub struct FeatureReq {
    #[serde(default)]
    binders: Vec<serde_json::Value>,
    #[serde(default)]
    cards: Vec<serde_json::Value>,
}

/// Numbers or words, as text: a seller's binders and cards go by number, the vault's binders by handle.
fn as_keys(v: &[serde_json::Value]) -> Vec<String> {
    v.iter()
        .take(200)
        .filter_map(|x| match x {
            serde_json::Value::String(s) => Some(s.trim().to_string()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .filter(|s| !s.is_empty() && s.len() <= 120)
        .collect()
}

async fn featured_post(app: App, h: HeaderMap, vault: bool, req: FeatureReq) -> Res {
    let who = match featurer(&app, &h, vault).await? {
        Ok(w) => w,
        Err(r) => return Ok(r),
    };
    let (binders, cards) = (as_keys(&req.binders), as_keys(&req.cards));
    if binders.len() > sellers::FEATURE_MAX || cards.len() > sellers::FEATURE_MAX {
        return Ok(fail(StatusCode::BAD_REQUEST, &format!("Choose up to {} binders and {} cards.", sellers::FEATURE_MAX, sellers::FEATURE_MAX)));
    }
    let saved = with_db(&app, move |c| {
        // Only the store's own binders and cards: anything else is left out.
        let (store, f) = match &who {
            Featurer::Vault => {
                let handles: Vec<String> = db::collections(c)?.into_iter().map(|b| b.handle).collect();
                let mut keep = vec![];
                for k in &cards {
                    if let Ok(id) = k.parse::<i64>() {
                        if db::product_by_id(c, id)?.is_some() {
                            keep.push(id.to_string());
                        }
                    }
                }
                (0, sellers::Featured { binders: binders.into_iter().filter(|k| handles.contains(k)).collect(), cards: keep })
            }
            Featurer::Seller(m) => {
                let mine: Vec<String> = members::binders_of(c, m.id)?.into_iter().map(|b| b.id.to_string()).collect();
                let listings: Vec<String> = sellers::storefront_listings(c, m.id)?.into_iter().map(|k| k.id.to_string()).collect();
                (m.id, sellers::Featured { binders: binders.into_iter().filter(|k| mine.contains(k)).collect(), cards: cards.into_iter().filter(|k| listings.contains(k)).collect() })
            }
        };
        sellers::set_featured(c, store, &f)?;
        Ok(sellers::featured(c, store)?.unwrap_or_default())
    })
    .await?;
    app.forget();
    Ok(ok(json!({ "ok": true, "binders": saved.binders.len(), "cards": saved.cards.len() })))
}

/// POST /me/featured {binders, cards}: what the seller's store shows on the Stores page.
pub async fn featured_me_save(State(app): State<App>, h: HeaderMap, Json(req): Json<FeatureReq>) -> Res {
    featured_post(app, h, false, req).await
}

/// POST /admin/featured {binders, cards}: what the vault's store shows on the Stores page (full admins).
pub async fn featured_vault_save(State(app): State<App>, h: HeaderMap, Json(req): Json<FeatureReq>) -> Res {
    featured_post(app, h, true, req).await
}

async fn seller_by_slug(app: &App, slug: String) -> Result<Option<Member>, (StatusCode, String)> {
    with_db(app, move |c| members::by_slug(c, &slug)).await
}

/// What a buyer is told when a store is blocked or not approved yet.
const CLOSED: &str = "This store is closed.";

/// A blocked store, or one the vault hasn't approved yet, takes no orders, offers or questions.
fn closed(seller: &Member) -> bool {
    seller.blocked || !seller.profile_ok
}

/// The store a `/store/{slug}/...` address names, as the number its follows and vouches are kept under: 0 for the
/// vault's ("vault"), otherwise the seller's member id. None when there is no such open store.
async fn store_id(app: &App, slug: String) -> Result<Option<i64>, (StatusCode, String)> {
    if slug == sellers::VAULT_SLUG {
        return Ok(Some(0));
    }
    Ok(seller_by_slug(app, slug).await?.filter(|m| m.seller && m.profile_ok && !m.blocked).map(|m| m.id))
}

/// POST /store/{slug}/follow: follow the store, or stop. The vault's store ("vault") is followed like any other.
pub async fn follow(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let Some(sid) = store_id(&app, slug).await? else { return Ok(fail(StatusCode::NOT_FOUND, "No such store.")) };
    let me = m.id;
    if over_limit(&app, format!("follow|{me}"), 30) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of following for now. Please try again in a while."));
    }
    let following = with_db(&app, move |c| sellers::toggle_follow(c, me, sid)).await?;
    let n = with_db(&app, move |c| sellers::followers(c, sid)).await?;
    // The vault's store page is kept for everyone, with its follower count on it.
    app.forget();
    Ok(ok(json!({ "ok": true, "following": following, "followers": n })))
}

/// GET /store/{slug}/follow: whether the visitor follows the store, and how many do. The vault's store page is the same
/// for every visitor, so its Follow button asks this once the page is open (static/store-head.js).
pub async fn follow_state(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>) -> Res {
    let Some(sid) = store_id(&app, slug).await? else { return Ok(fail(StatusCode::NOT_FOUND, "No such store.")) };
    let me = crate::community::current(&app, &h).await?.map(|m| m.id);
    let (following, n) = with_db(&app, move |c| Ok((match me { Some(v) => sellers::is_following(c, v, sid)?, None => false }, sellers::followers(c, sid)?))).await?;
    Ok(ok(json!({ "ok": true, "following": following, "followers": n })))
}

/// One entry in a store's Vouches window: a vouch someone wrote for the store, or a review from one of its finished sales.
pub(crate) enum VouchItem {
    Vouch(members::Vouch),
    Review(Review),
}

impl VouchItem {
    fn at(&self) -> i64 {
        match self {
            VouchItem::Vouch(v) => v.created_at,
            VouchItem::Review(r) => r.created_at,
        }
    }
}

#[derive(Template)]
#[template(path = "store_vouches.html")]
struct StoreVouchesPart {
    /// The store: 0 for the vault's, otherwise the seller's member id.
    store: i64,
    name: String,
    items: Vec<VouchItem>,
    /// The visitor runs this store: they may answer its reviews here.
    is_me: bool,
}

/// GET /store/{slug}/vouches: a store's Vouches window, newest first (static/store-head.js opens it over the page).
/// The vault's store lists the vouches written for it; a seller's, those and the reviews from its finished sales.
pub async fn store_vouches(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>) -> Res {
    let me = crate::community::current(&app, &h).await?;
    let Some(store) = store_id(&app, slug).await? else { return not_found(&app).await };
    let is_me = me.as_ref().is_some_and(|u| u.id == store && store != 0);
    let (name, items) = with_db(&app, move |c| {
        let name = if store == 0 { chrome(c)?.brand } else { members::get(c, store)?.map(|m| m.store_title()).unwrap_or_default() };
        let mut items: Vec<VouchItem> = members::store_vouches(c, store, 100)?.into_iter().map(VouchItem::Vouch).collect();
        if store != 0 {
            items.extend(sellers::reviews_of(c, store, false)?.into_iter().map(VouchItem::Review));
        }
        items.sort_by_key(|i| std::cmp::Reverse(i.at()));
        Ok((name, items))
    })
    .await?;
    Ok(html(render(StoreVouchesPart { store, name, items, is_me })?))
}

#[derive(Deserialize)]
pub struct BorderReq {
    border: String,
}

/// POST /admin/store/border: the vault's store border colour (a full admin, from "Edit store" on the vault's page).
/// Sellers set theirs in their own "Edit store" (`save_settings`, field store_border).
pub async fn vault_border_save(State(app): State<App>, h: HeaderMap, Json(req): Json<BorderReq>) -> Res {
    if !matches!(admin::power(&app, &h), admin::Power::Admin(_)) {
        return Ok(fail(StatusCode::FORBIDDEN, "Only the vault's admins can change its store."));
    }
    let Some(border) = sellers::border_ok(&req.border) else { return Ok(fail(StatusCode::BAD_REQUEST, BORDER_BAD)) };
    let b = border.clone();
    with_db(&app, move |c| sellers::set_vault_border(c, &b)).await?;
    app.forget();
    Ok(ok(json!({ "ok": true, "border": border })))
}

const BORDER_BAD: &str = "That colour is too dark to see on the page. Pick a lighter one.";

#[derive(Deserialize)]
pub struct BuyReq {
    card_id: i64,
    #[serde(default = "one")]
    qty: i64,
    #[serde(default)]
    message: String,
    amount: Option<f64>,
}

fn one() -> i64 {
    1
}

pub async fn reserve(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>, Json(req): Json<BuyReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if over_limit(&app, format!("buy|{}", m.id), 60) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of orders in a short time. Please wait a few minutes."));
    }
    let Some(seller) = seller_by_slug(&app, slug).await? else { return Ok(fail(StatusCode::NOT_FOUND, "No such store.")) };
    if closed(&seller) {
        return Ok(fail(StatusCode::BAD_REQUEST, CLOSED));
    }
    let (me, sid) = (m.id, seller.id);
    match with_db(&app, move |c| sellers::reserve(c, me, sid, req.card_id, req.qty, &req.message)).await? {
        Ok(id) => {
            app.forget();
            Ok(ok(json!({ "ok": true, "id": id })))
        }
        Err(e) => Ok(fail(StatusCode::BAD_REQUEST, e)),
    }
}

pub async fn make_offer(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>, Json(req): Json<BuyReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if over_limit(&app, format!("buy|{}", m.id), 60) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of offers in a short time. Please wait a few minutes."));
    }
    let Some(seller) = seller_by_slug(&app, slug).await? else { return Ok(fail(StatusCode::NOT_FOUND, "No such store.")) };
    if closed(&seller) {
        return Ok(fail(StatusCode::BAD_REQUEST, CLOSED));
    }
    let (me, sid, amount) = (m.id, seller.id, req.amount.unwrap_or(0.0));
    match with_db(&app, move |c| sellers::make_offer(c, me, sid, req.card_id, amount, req.qty, &req.message)).await? {
        Ok(id) => Ok(ok(json!({ "ok": true, "id": id }))),
        Err(e) => Ok(fail(StatusCode::BAD_REQUEST, e)),
    }
}

#[derive(Deserialize)]
pub struct AskReq {
    card_id: Option<i64>,
    message: String,
}

pub async fn ask_seller(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>, Json(req): Json<AskReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if over_limit(&app, format!("ask|{}", m.id), 30) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of questions in a short time. Please wait a few minutes."));
    }
    let Some(seller) = seller_by_slug(&app, slug).await? else { return Ok(fail(StatusCode::NOT_FOUND, "No such store.")) };
    if closed(&seller) {
        return Ok(fail(StatusCode::BAD_REQUEST, CLOSED));
    }
    let (me, sid) = (m.id, seller.id);
    match with_db(&app, move |c| sellers::ask(c, me, sid, req.card_id, &req.message)).await? {
        Ok(id) => Ok(ok(json!({ "ok": true, "id": id }))),
        Err(e) => Ok(fail(StatusCode::BAD_REQUEST, e)),
    }
}

pub async fn save_card(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    let m = match acting_any(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let me = m.id;
    let saved = with_db(&app, move |c| sellers::toggle_save(c, me, id)).await?;
    Ok(ok(json!({ "ok": true, "saved": saved })))
}

// ---------- the cart ----------

#[derive(Deserialize)]
pub struct CartQs {
    #[serde(default)]
    v: String,
    #[serde(default)]
    c: String,
}

fn ids_of(s: &str) -> Vec<i64> {
    s.split(',').filter_map(|x| x.trim().parse().ok()).take(60).collect()
}

/// `GET /cart/lines?v=1,2&c=3`: what's in the cart as it stands now (today's prices, still for sale?).
pub async fn cart_lines(State(app): State<App>, Query(q): Query<CartQs>) -> Res {
    let (v, c) = (ids_of(&q.v), ids_of(&q.c));
    let lines = with_db(&app, move |db| sellers::cart_lines(db, &v, &c)).await?;
    Ok(ok(json!({ "ok": true, "lines": lines })))
}

#[derive(Deserialize)]
pub struct CartItem {
    kind: String,
    id: i64,
    #[serde(default = "one")]
    qty: i64,
}

#[derive(Deserialize)]
pub struct CheckoutReq {
    items: Vec<CartItem>,
    #[serde(default)]
    message: String,
}

/// `POST /cart/checkout`: the vault's cards become one order for the admins; each seller's cards become reservations.
/// Lines that can't be placed come back with why, and the rest go through.
pub async fn checkout(State(app): State<App>, h: HeaderMap, Json(req): Json<CheckoutReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if over_limit(&app, format!("buy|{}", m.id), 60) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of orders in a short time. Please wait a few minutes."));
    }
    if req.items.is_empty() {
        return Ok(fail(StatusCode::BAD_REQUEST, "Your cart is empty."));
    }
    let me = m.id;
    let message = req.message.clone();
    let vault: Vec<(i64, i64)> = req.items.iter().filter(|i| i.kind == "vault").map(|i| (i.id, i.qty.clamp(1, 99))).collect();
    let cards: Vec<(i64, i64)> = req.items.iter().filter(|i| i.kind == "card").map(|i| (i.id, i.qty.clamp(1, 99))).collect();
    let mut placed: Vec<serde_json::Value> = vec![];
    let mut failed: Vec<serde_json::Value> = vec![];
    if !vault.is_empty() {
        let (v2, msg) = (vault.clone(), message.clone());
        // The whole vault order goes through or none of it. When it can't, only the lines that are really short
        // come back in "ids" (with why for each in "lines"), so the cart greys out just those. "ids" is empty when
        // the trouble is the order as a whole (too many cards, or three orders open already).
        let res = with_db(&app, move |c| {
            Ok(match sellers::place_vault_order(c, me, &v2, &msg)? {
                Ok(id) => Ok(id),
                Err(e) => Err((e, sellers::vault_short_lines(c, &v2)?)),
            })
        })
        .await?;
        match res {
            Ok(id) => placed.push(json!({ "kind": "vault", "order": id, "ids": vault.iter().map(|x| x.0).collect::<Vec<_>>() })),
            Err((e, short)) => failed.push(json!({
                "kind": "vault",
                "ids": short.iter().map(|x| x.0).collect::<Vec<_>>(),
                "lines": short.iter().map(|(id, left, why)| json!({ "id": id, "left": left, "why": why })).collect::<Vec<_>>(),
                "error": e,
            })),
        }
    }
    for (cid, qty) in cards {
        let msg = message.clone();
        // Whose card it is, and whether their store is open (not blocked, and approved).
        let seller: Option<(i64, bool)> = with_db(&app, move |c| {
            c.query_row("SELECT x.member_id, (m.blocked=0 AND m.profile_ok=1) FROM member_cards x JOIN members m ON m.id=x.member_id WHERE x.id=?1", [cid], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0))).optional()
        })
        .await?;
        let Some((sid, open)) = seller else {
            failed.push(json!({ "kind": "card", "id": cid, "error": "No longer listed" }));
            continue;
        };
        if !open {
            failed.push(json!({ "kind": "card", "id": cid, "error": CLOSED }));
            continue;
        }
        match with_db(&app, move |c| sellers::reserve(c, me, sid, cid, qty, &msg)).await? {
            Ok(id) => placed.push(json!({ "kind": "card", "id": cid, "reservation": id })),
            Err(e) => failed.push(json!({ "kind": "card", "id": cid, "error": e })),
        }
    }
    if !placed.is_empty() {
        app.forget();
    }
    Ok(ok(json!({ "ok": true, "placed": placed, "failed": failed })))
}

#[derive(Deserialize)]
pub struct CancelReq {
    #[serde(default)]
    status: String,
}

/// `POST /me/orders/vault/{id}`: the buyer cancels an order of the vault's cards.
pub async fn cancel_vault_order(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<CancelReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if req.status != "Cancelled" {
        return Ok(fail(StatusCode::BAD_REQUEST, "Only cancelling is yours to do here."));
    }
    let me = m.id;
    let done = with_db(&app, move |c| sellers::cancel_vault_order(c, me, id)).await?;
    Ok(if done { ok(json!({ "ok": true })) } else { fail(StatusCode::BAD_REQUEST, "That order can't be cancelled any more.") })
}

#[derive(Template)]
#[template(path = "admin_orders.html")]
struct AdminOrdersPage {
    nav: Chrome,
    tab: &'static str,
    orders: Vec<sellers::VaultOrder>,
    open: i64,
    methods: [&'static str; 6],
    pay_methods: String,
    pay_info: String,
}

impl AdminOrdersPage {
    fn has_pay(&self, m: &str) -> bool {
        self.pay_methods.split(',').any(|x| x.trim() == m)
    }
}

/// `GET /admin/orders`: orders for the vault's own cards, and how buyers pay the vault.
pub async fn admin_orders(State(app): State<App>, h: HeaderMap) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(Redirect::to("/login").into_response());
    }
    let (nav, orders, open, (pay_methods, pay_info)) = with_db(&app, |c| Ok((chrome(c)?, sellers::vault_orders_all(c)?, sellers::vault_orders_open(c)?, sellers::vault_pay(c)?))).await?;
    Ok(html(render(AdminOrdersPage { nav, tab: "orders", orders, open, methods: sellers::PAY_METHODS, pay_methods, pay_info })?))
}

#[derive(Deserialize)]
pub struct VaultAnswer {
    status: String,
    #[serde(default)]
    paid_via: String,
}

pub async fn admin_answer_order(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<VaultAnswer>) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(fail(StatusCode::FORBIDDEN, "Only the vault's admins answer orders."));
    }
    let (status, via) = (req.status, req.paid_via);
    match with_db(&app, move |c| sellers::answer_vault_order(c, id, &status, &via)).await? {
        Ok(o) => {
            app.forget();
            Ok(ok(json!({ "ok": true, "status": o.status })))
        }
        Err(e) => Ok(fail(StatusCode::BAD_REQUEST, &e)),
    }
}

#[derive(Deserialize)]
pub struct VaultPayReq {
    #[serde(default)]
    methods: Vec<String>,
    #[serde(default)]
    info: String,
}

pub async fn admin_vault_pay(State(app): State<App>, h: HeaderMap, Json(req): Json<VaultPayReq>) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(fail(StatusCode::FORBIDDEN, "Only the vault owner and admins can change this."));
    }
    let (methods, info) = (req.methods.join(","), req.info);
    with_db(&app, move |c| sellers::set_vault_pay(c, &methods, &info)).await?;
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

// ---------- admin ----------

#[derive(Template)]
#[template(path = "admin_sellers.html")]
struct AdminSellersPage {
    nav: Chrome,
    tab: &'static str,
    stores: Vec<(Member, StoreStats, Vec<MemberCard>)>,
    flagged: Vec<(MemberCard, String)>,
    reviews: Vec<Review>,
    sellers_n: i64,
    /// Trainers asking to sell: every one still waiting, then the latest answered.
    waiting: Vec<sellers::Application>,
    answered: Vec<sellers::Application>,
}

pub async fn admin_sellers(State(app): State<App>, h: HeaderMap) -> Res {
    if !admin::can_moderate(&app, &h) {
        return Ok(Redirect::to("/admin").into_response());
    }
    let (nav, stores, flagged, reviews, sellers_n, (waiting, answered)) = with_db(&app, |c| {
        let n: i64 = c.query_row("SELECT COUNT(*) FROM members WHERE blocked=0 AND seller=1", [], |r| r.get(0))?;
        Ok((chrome(c)?, sellers::directory(c)?, sellers::flagged_cards(c)?, sellers::all_reviews(c, 50)?, n, sellers::applications(c, 20)?))
    })
    .await?;
    Ok(html(render(AdminSellersPage { nav, tab: "sellers", stores, flagged, reviews, sellers_n, waiting, answered })?))
}

#[derive(Deserialize)]
pub struct AnnounceReq {
    message: String,
    #[serde(default)]
    link: String,
}

pub async fn announce(State(app): State<App>, h: HeaderMap, Json(req): Json<AnnounceReq>) -> Res {
    if !admin::authed(&app, &h) {
        return Ok(fail(StatusCode::FORBIDDEN, "Only the vault owner and admins can send announcements."));
    }
    let n = with_db(&app, move |c| sellers::announce(c, &req.message, &req.link)).await?;
    Ok(ok(json!({ "ok": true, "n": n })))
}

#[derive(Deserialize)]
pub struct ModReq {
    hidden: Option<bool>,
}

pub async fn admin_review(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<ModReq>) -> Res {
    if !admin::can_moderate(&app, &h) {
        return Ok(fail(StatusCode::FORBIDDEN, "Not allowed."));
    }
    let hidden = req.hidden.unwrap_or(true);
    with_db(&app, move |c| sellers::hide_review(c, id, hidden)).await?;
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

pub async fn admin_clear_flag(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    if !admin::can_moderate(&app, &h) {
        return Ok(fail(StatusCode::FORBIDDEN, "Not allowed."));
    }
    with_db(&app, move |c| sellers::clear_flag(c, id)).await?;
    Ok(ok(json!({ "ok": true })))
}

// ---------- mail and housekeeping ----------

/// Every minute: mail in the outbox goes out (when a mailer is configured), and listings past their date come off sale.
/// Mail: set RESEND_API_KEY and MAIL_FROM. Without them, mail is logged once and dropped.
pub fn start_housekeeping(app: App) {
    tokio::spawn(async move {
        let key = std::env::var("RESEND_API_KEY").ok().filter(|k| !k.is_empty());
        let from = std::env::var("MAIL_FROM").unwrap_or_else(|_| "Big Boys Vault <no-reply@bigboysvault.com>".into());
        let site = crate::site();
        let mut warned = false;
        let mut tick: u64 = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            // Release dates for any set the vault's cards use: soon after boot, then every six hours.
            if tick % 360 == 1 {
                crate::ops::refresh_sets(&app).await;
            }
            // Cards that never had a market price looked up (the SRP needs it): a batch every minute until every card
            // has one (about an hour for the whole vault, politely spaced). The nightly job keeps them fresh after that.
            if tick >= 1 && crate::ops::market_missing(&app).await > 0 {
                crate::ops::refresh_prices_batch(&app, 40).await.ok();
            }
            tick += 1;
            let swept = with_db(&app, |c| sellers::expire_due(c)).await;
            if let Ok(n) = swept {
                if n > 0 {
                    app.forget();
                }
            }
            let Ok(batch) = with_db(&app, |c| sellers::outbox(c, 20)).await else { continue };
            if batch.is_empty() {
                continue;
            }
            let Some(key) = &key else {
                if !warned {
                    eprintln!("mail: {} message(s) waiting, but RESEND_API_KEY isn't set, so they're dropped.", batch.len());
                    warned = true;
                }
                for (id, ..) in batch {
                    with_db(&app, move |c| sellers::outbox_done(c, id, true)).await.ok();
                }
                continue;
            };
            for (id, to, kind, message, link) in batch {
                if to.is_empty() {
                    with_db(&app, move |c| sellers::outbox_done(c, id, true)).await.ok();
                    continue;
                }
                let subject = match kind.as_str() {
                    "my_order" => "An update on your order",
                    "order" => "A new vault order",
                    "reservation" => "Someone reserved a card",
                    "offer" => "You have an offer",
                    "sale" => "A sale is done",
                    "inquiry" => "A question from a buyer",
                    "review" => "A new review",
                    "follow" => "A new follower",
                    "expiry" => "A listing expired",
                    "announcement" => "News from the vault",
                    "wish" => "A card from your wishlist is here",
                    "message" => "You have a new message",
                    _ => "Something happened in your store",
                };
                let url = if link.starts_with('/') { format!("{site}{link}") } else { link };
                let body = format!("{message}\n\nOpen it: {url}\n\nYou get this because you turned on email in your notification settings: {site}/me/notifications");
                let sent = app
                    .http
                    .post("https://api.resend.com/emails")
                    .bearer_auth(key)
                    .json(&json!({ "from": from, "to": [to], "subject": subject, "text": body }))
                    .send()
                    .await
                    .map(|r| r.status().is_success())
                    .unwrap_or(false);
                with_db(&app, move |c| sellers::outbox_done(c, id, sent)).await.ok();
            }
        }
    });
}

// ---------- applying to sell ----------

/// How many photos an application can carry (screenshots of past sales, buyers' feedback, a selling group).
const PROOF_MAX: usize = 8;

#[derive(Template)]
#[template(path = "apply.html")]
struct ApplyPage {
    nav: Chrome,
    me: Member,
    /// Their newest application, if they sent one: waiting, approved or declined.
    sent: Option<sellers::Application>,
    /// It is still waiting for an answer: no form, just where the answer will come.
    waiting: bool,
}

/// GET /me/sell: "Apply as a seller" (the account menu). A trainer who sells already goes to their store.
pub async fn apply_page(State(app): State<App>, h: HeaderMap) -> Res {
    let Some(me) = crate::community::current(&app, &h).await? else { return Ok(Redirect::to("/login").into_response()) };
    if me.seller {
        return Ok(Redirect::to("/me/store").into_response());
    }
    let id = me.id;
    let (mut nav, sent) = with_db(&app, move |c| Ok((chrome(c)?, sellers::application_of(c, id)?))).await?;
    nav.seo.title = format!("Apply as a seller · {}", nav.brand);
    let waiting = sent.as_ref().is_some_and(|a| a.waiting());
    Ok(html(render(ApplyPage { nav, me, sent, waiting })?))
}

/// POST /me/sell: a trainer applies to sell. Their words and photos (both may be left out) open the chat between them
/// and the vault's team, and the whole application goes on there: the admins' questions, then their answer. The admins
/// also see it under Admin › Sellers, with a number on the tab.
pub async fn apply_send(State(app): State<App>, h: HeaderMap, mut mp: Multipart) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if m.seller {
        return Ok(fail(StatusCode::BAD_REQUEST, "You can sell already: open My store."));
    }
    let id = m.id;
    if with_db(&app, move |c| sellers::application_of(c, id)).await?.is_some_and(|a| a.waiting()) {
        return Ok(fail(StatusCode::CONFLICT, "Your application is with the admins already. Their answer comes in your messages."));
    }
    if over_limit(&app, format!("apply|{id}"), 3) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a few tries already. Please try again tomorrow."));
    }
    let (mut note, mut files) = (String::new(), Vec::<Vec<u8>>::new());
    while let Some(f) = mp.next_field().await.map_err(|_| (StatusCode::BAD_REQUEST, "That didn't upload. Please try again.".to_string()))? {
        match f.name().unwrap_or("") {
            "note" => note = f.text().await.unwrap_or_default(),
            "proof" => {
                let b = f.bytes().await.map_err(|_| (StatusCode::BAD_REQUEST, "A photo didn't upload. Please try again.".to_string()))?;
                if !b.is_empty() && files.len() < PROOF_MAX {
                    files.push(b.to_vec());
                }
            }
            _ => {}
        }
    }
    let note = members::clean_text(&note, 1000);
    if !files.is_empty() {
        let room = with_db(&app, |c| Ok(members::photo_bytes(c)? + dm::media_bytes(c)?)).await?;
        if room >= crate::community::photo_cap_mb() * 1024 * 1024 {
            return Ok(fail(StatusCode::INSUFFICIENT_STORAGE, "The vault's photo space is full right now. Send your application without photos, and share them in the chat later."));
        }
    }
    let now = crate::unix_now();
    // The chat first: its photos are named after it, like every photo sent in a chat.
    let t = with_db(&app, move |c| dm::thread_between(c, id, dm::VAULT, now)).await?;
    let mut pics: Vec<(String, String, i64)> = vec![];
    for bytes in files {
        let dir = PathBuf::from(&*app.uploads);
        let stem = format!("dm{}-{}{}", t.id, members::temp_password(), members::temp_password());
        match gated(move || store_photo(bytes, dir, stem)).await? {
            Ok((large, small, _, _, size)) => pics.push((large, small, size)),
            Err(e) => {
                for (l, s, _) in &pics {
                    admin::remove_upload(&app.uploads, l);
                    admin::remove_upload(&app.uploads, s);
                }
                let why = if e == crate::community::TOO_BIG { "One of the photos is too big. Please use a smaller one." } else { "One of the photos couldn't be read. Try a JPEG or PNG." };
                return Ok(fail(StatusCode::BAD_REQUEST, why));
            }
        }
    }
    let t2 = t.clone();
    let words = if note.is_empty() { "No note.".to_string() } else { note.clone() };
    let n = pics.len();
    let text = format!("📝 Seller application\n\n{words}{}", if n > 0 { format!("\n\n📷 {n} photo{} below.", if n == 1 { "" } else { "s" }) } else { String::new() });
    let pics2 = pics.clone();
    let preview = with_db(&app, move |c| {
        let kept: Vec<(String, String)> = pics2.iter().map(|(l, s, _)| (l.clone(), s.clone())).collect();
        if sellers::add_application(c, id, &note, &kept, t2.id, now).is_err() {
            return Ok(None);
        }
        let first = dm::New { kind: "text".into(), text, ..Default::default() };
        dm::post(c, &t2, id, id, &first, now)?;
        for (large, small, size) in &pics2 {
            dm::post(c, &t2, id, id, &dm::New { kind: "photo".into(), media: large.clone(), media_thumb: small.clone(), bytes: *size, ..Default::default() }, now)?;
        }
        Ok(Some(dm::preview(&first)))
    })
    .await?;
    let Some(preview) = preview else {
        for (l, s, _) in &pics {
            admin::remove_upload(&app.uploads, l);
            admin::remove_upload(&app.uploads, s);
        }
        return Ok(fail(StatusCode::CONFLICT, "Your application is with the admins already. Their answer comes in your messages."));
    };
    crate::messages::after_post(&app, t.clone(), id, id, preview, false, true).await;
    app.forget();
    Ok(ok(json!({ "ok": true, "thread": t.id })))
}

#[derive(Deserialize)]
pub struct DecideReq {
    action: String,
    #[serde(default)]
    reason: String,
}

/// POST /admin/applications/{id} with `{"action": "approve" | "decline", "reason": "..."}`: a full admin answers. The
/// answer is written into the application's chat, from the vault, so the trainer reads it where they applied.
pub async fn apply_decide(State(app): State<App>, h: HeaderMap, Path(aid): Path<i64>, Json(req): Json<DecideReq>) -> Res {
    let by = match admin::power(&app, &h) {
        admin::Power::Admin(me) => me.unwrap_or(0),
        _ => return Ok(fail(StatusCode::FORBIDDEN, "Only an admin can answer applications.")),
    };
    let approve = match req.action.as_str() {
        "approve" => true,
        "decline" => false,
        _ => return Ok(fail(StatusCode::BAD_REQUEST, "Approve or decline?")),
    };
    let reason = members::clean_line(&req.reason, 300);
    let now = crate::unix_now();
    let done = with_db(&app, move |c| {
        let Some(a) = sellers::decide_application(c, aid, approve, &reason, by, now)? else { return Ok(None) };
        let text = if approve {
            "✅ Your seller application is approved! Your store is open: open the menu and tap My store to put up your first cards.".to_string()
        } else if reason.is_empty() {
            "Your seller application wasn't approved this time. You can apply again later from your menu.".to_string()
        } else {
            format!("Your seller application wasn't approved this time: {reason} You can apply again later from your menu.")
        };
        let t = match dm::thread(c, a.thread_id)? {
            Some(t) => t,
            None => dm::thread_between(c, a.member_id, dm::VAULT, now)?,
        };
        let msg = dm::New { kind: "text".into(), text, ..Default::default() };
        dm::post(c, &t, dm::VAULT, by, &msg, now)?;
        Ok(Some((a, t, dm::preview(&msg))))
    })
    .await?;
    let Some((a, t, preview)) = done else { return Ok(fail(StatusCode::CONFLICT, "Another admin answered this one already. Reload to see it.")) };
    crate::messages::after_post(&app, t, dm::VAULT, by, preview, false, true).await;
    app.forget();
    Ok(ok(json!({ "ok": true, "status": a.status })))
}

// ---------- bookmarks ----------

#[derive(Deserialize)]
pub struct BookmarkReq {
    /// "store" (a seller's id, 0 for the vault's), "product" (one of the vault's cards) or "card" (a seller's card).
    kind: String,
    id: i64,
}

/// POST /me/bookmarks: bookmark a store or a card, or take the bookmark off (static/bookmarks.js). A seller's card is a
/// saved listing, so the seller still sees how many saved it.
pub async fn bookmark(State(app): State<App>, h: HeaderMap, Json(req): Json<BookmarkReq>) -> Res {
    let m = match acting_any(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let (me, kind, id) = (m.id, req.kind, req.id);
    let on = with_db(&app, move |c| match kind.as_str() {
        "card" => {
            let there: i64 = c.query_row(
                "SELECT COUNT(*) FROM member_cards k JOIN members m ON m.id=k.member_id WHERE k.id=?1 AND k.kind='sale' AND k.status NOT IN ('Hidden','Expired') AND m.profile_ok=1 AND m.blocked=0",
                [id],
                |r| r.get(0),
            )?;
            if there == 0 && !sellers::saved_ids(c, me)?.contains(&id) {
                return Ok(None);
            }
            sellers::toggle_save(c, me, id).map(Some)
        }
        "store" if id == me => Ok(None),
        "store" => {
            if !members::vouchable_store(c, id)? && !sellers::bookmarks_of(c, me)?.iter().any(|(k, r, _)| k == "store" && *r == id) {
                return Ok(None);
            }
            sellers::toggle_bookmark(c, me, "store", id).map(Some)
        }
        "product" => {
            if db::product_by_id(c, id)?.is_none() && !sellers::bookmarks_of(c, me)?.iter().any(|(k, r, _)| k == "product" && *r == id) {
                return Ok(None);
            }
            sellers::toggle_bookmark(c, me, "product", id).map(Some)
        }
        _ => Ok(None),
    })
    .await?;
    let Some(on) = on else { return Ok(fail(StatusCode::NOT_FOUND, "That can't be bookmarked.")) };
    let n = with_db(&app, move |c| sellers::bookmark_count(c, me)).await?;
    Ok(ok(json!({ "ok": true, "on": on, "count": n })))
}

/// GET /me/bookmarks/ids: what the visitor bookmarked, so the bookmark buttons on pages kept for everyone light up
/// (static/bookmarks.js). Empty for a visitor who isn't signed in.
pub async fn bookmark_ids(State(app): State<App>, h: HeaderMap) -> Res {
    let Some(m) = crate::community::current(&app, &h).await? else { return Ok(ok(json!({ "stores": [], "products": [], "cards": [], "count": 0 }))) };
    let id = m.id;
    let (all, n) = with_db(&app, move |c| Ok((sellers::bookmarks_of(c, id)?, sellers::bookmark_count(c, id)?))).await?;
    let pick = |k: &str| all.iter().filter(|(kind, _, _)| kind == k).map(|(_, r, _)| *r).collect::<Vec<i64>>();
    Ok(ok(json!({ "stores": pick("store"), "products": pick("product"), "cards": pick("card"), "count": n })))
}

/// A bookmarked store on the Bookmarks page.
pub(crate) struct BmStore {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub avatar: String,
    pub initial: String,
    pub logo: bool,
    /// "2,213 cards for sale", or why it is closed.
    pub sub: String,
}

/// A bookmarked card, the vault's or a seller's, with what the cart needs to take it.
pub(crate) struct BmCard {
    /// "product" (the vault's) or "card" (a seller's): what its bookmark button and its cart button say.
    pub kind: &'static str,
    pub id: i64,
    pub title: String,
    pub image: String,
    pub set: String,
    pub cond: String,
    pub cond_class: &'static str,
    pub price: String,
    pub worth: f64,
    pub url: String,
    /// It can go in the cart now; otherwise `note` says why not.
    pub buyable: bool,
    pub note: &'static str,
    pub seller_name: String,
    pub seller_slug: String,
}

/// The bookmarked cards of one store, under that store's name: where each card was bookmarked.
pub(crate) struct BmGroup {
    pub store: BmStore,
    pub cards: Vec<BmCard>,
}

impl BmGroup {
    /// How many of its cards can go in the cart now (the "Add all to cart" button shows when there are two or more).
    pub fn buyable_n(&self) -> usize {
        self.cards.iter().filter(|c| c.buyable).count()
    }
}

#[derive(Template)]
#[template(path = "bookmarks.html")]
struct BookmarksPage {
    nav: Chrome,
    stores: Vec<BmStore>,
    groups: Vec<BmGroup>,
}

/// A store as the Bookmarks page shows it: the vault's (0) or a seller's. None when it is gone.
fn bm_store(c: &rusqlite::Connection, id: i64, brand: &str) -> rusqlite::Result<Option<BmStore>> {
    if id == 0 {
        let (n, _, _) = admin::in_vault(c)?;
        return Ok(Some(BmStore { id: 0, name: brand.to_string(), url: VAULT_STORE.into(), avatar: "/static/img/logo-sm-144.png?v=1".into(), initial: String::new(), logo: true, sub: format!("{} card{} for sale", db::thousands(n), if n == 1 { "" } else { "s" }) }));
    }
    let Some(m) = members::get(c, id)? else { return Ok(None) };
    let open = m.seller && m.profile_ok && !m.blocked;
    let sub = if !open {
        "This store is closed for now".to_string()
    } else if m.on_vacation() {
        "On vacation".to_string()
    } else {
        let n = sellers::store_stats(c, id)?.on_sale;
        format!("{n} card{} for sale", if n == 1 { "" } else { "s" })
    };
    Ok(Some(BmStore { id, name: m.store_title(), url: m.store_url(), avatar: if m.has_avatar() { m.avatar.clone() } else { String::new() }, initial: m.initial(), logo: false, sub }))
}

/// Puts a card under its store's heading, making the heading the first time that store comes up.
fn bm_into(c: &rusqlite::Connection, groups: &mut Vec<BmGroup>, store: i64, brand: &str, card: BmCard) -> rusqlite::Result<()> {
    if let Some(g) = groups.iter_mut().find(|g| g.store.id == store) {
        g.cards.push(card);
        return Ok(());
    }
    if let Some(s) = bm_store(c, store, brand)? {
        groups.push(BmGroup { store: s, cards: vec![card] });
    }
    Ok(())
}

/// GET /me/bookmarks: everything the visitor bookmarked: the stores, then the cards under the store each came from,
/// each ready for the cart ("Add all to cart" per store). The account menu and the menu lead here, with the number.
pub async fn bookmarks_page(State(app): State<App>, h: HeaderMap) -> Res {
    let Some(me) = crate::community::current(&app, &h).await? else { return Ok(Redirect::to("/login").into_response()) };
    let id = me.id;
    let (mut nav, stores, groups) = with_db(&app, move |c| {
        let nav = chrome(c)?;
        let brand = nav.brand.clone();
        let (mut stores, mut groups) = (vec![], Vec::<BmGroup>::new());
        for (kind, r, _) in sellers::bookmarks_of(c, id)? {
            match kind.as_str() {
                "store" => {
                    if let Some(s) = bm_store(c, r, &brand)? {
                        stores.push(s);
                    }
                }
                "product" => {
                    let Some(p) = db::product_by_id(c, r)? else { continue };
                    let card = BmCard {
                        kind: "product",
                        id: p.id,
                        title: p.short_title(),
                        image: if p.has_image() { p.img(360) } else { String::new() },
                        set: p.set_text(),
                        cond: p.tile_cond(),
                        cond_class: p.cond_class(),
                        price: p.price_text(),
                        worth: p.price,
                        url: format!("/products/{}", p.handle),
                        buyable: p.available && p.price > 0.0,
                        note: if p.available { "Ask for a price" } else { "Sold" },
                        seller_name: String::new(),
                        seller_slug: String::new(),
                    };
                    bm_into(c, &mut groups, 0, &brand, card)?;
                }
                "card" => {
                    let owner: Option<i64> = c.query_row("SELECT member_id FROM member_cards WHERE id=?1", [r], |x| x.get(0)).optional()?;
                    let Some(owner) = owner else { continue };
                    let (Some(k), Some(m)) = (members::card(c, owner, r)?, members::get(c, owner)?) else { continue };
                    // What the card's own page would refuse to show isn't shown here either.
                    if m.blocked || !m.profile_ok || !k.is_sale() || matches!(k.status.as_str(), "Hidden" | "Expired") {
                        continue;
                    }
                    let open = m.seller && m.profile_ok && !m.on_vacation();
                    let buyable = open && k.buyable() && k.has_price();
                    let note = match k.status.as_str() {
                        _ if buyable => "",
                        "Sold" => "Sold",
                        "Reserved" => "Reserved for someone",
                        "Out of stock" => "Out of stock",
                        _ if m.on_vacation() => "The store is on vacation",
                        _ if !open => "The store is closed for now",
                        _ if !k.has_price() => "Ask the store for a price",
                        _ => "Not for sale right now",
                    };
                    let card = BmCard {
                        kind: "card",
                        id: k.id,
                        title: k.title.clone(),
                        image: k.image.clone(),
                        set: k.card_set.clone(),
                        cond: k.cond.clone(),
                        cond_class: k.cond_class(),
                        price: if k.has_price() { k.price_text() } else { String::new() },
                        worth: k.worth(),
                        url: format!("/trainers/{}/cards/{}", m.id, k.id),
                        buyable,
                        note,
                        seller_name: m.store_title(),
                        seller_slug: m.slug.clone(),
                    };
                    bm_into(c, &mut groups, owner, &brand, card)?;
                }
                _ => {}
            }
        }
        Ok((nav, stores, groups))
    })
    .await?;
    nav.seo.title = format!("Bookmarks · {}", nav.brand);
    Ok(html(render(BookmarksPage { nav, stores, groups })?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use marcuru_rs::members::Added;

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

    /// The store page, or one binder's page, as a visitor sees it in the chosen look (the cards as `store_for` picks them).
    fn page(c: &rusqlite::Connection, slug: &str, binder: Option<&str>) -> String {
        page_as(c, slug, binder, false)
    }

    /// The same, as a visitor or as the store's `owner` (who sees every binder, as staff do).
    fn page_as(c: &rusqlite::Connection, slug: &str, binder: Option<&str>, owner: bool) -> String {
        let m = members::by_slug(c, slug).unwrap().unwrap();
        let binder = binder.map(|b| members::binder_by_slug(c, m.id, b).unwrap().unwrap());
        let cards: Vec<MemberCard> = match &binder {
            Some(b) => members::cards_of_binder(c, m.id, Some(b.id)).unwrap().into_iter().filter(|k| k.is_sale() && matches!(k.status.as_str(), "Active" | "Reserved" | "Out of stock")).collect(),
            None => sellers::storefront_listings(c, m.id).unwrap(),
        };
        let binders: Vec<MemberBinder> = members::binders_of(c, m.id).unwrap().into_iter().filter(|b| b.visibility == "Public" || owner).collect();
        let tiles = if binder.is_none() { seller_tiles(&m, &binders, &cards, owner) } else { vec![] };
        let binders_n = tiles.iter().filter(|t| !t.off).count();
        let html = render(StorePage {
            nav: chrome(c).unwrap(),
            stats: sellers::store_stats(c, m.id).unwrap(),
            cards: cards.into_iter().map(|k| (k, false)).collect(),
            tiles,
            binders_n,
            binder,
            vouches: members::store_vouch_count(c, m.id).unwrap(),
            me: None,
            following: false,
            is_me: owner,
            links: vec![],
            conditions: members::CONDITIONS,
            m,
        });
        html.unwrap()
    }

    #[test]
    fn binders_are_a_grid_and_a_binder_is_its_own_page_in_the_new_look() {
        let c = db();
        let s = members::create(&c, "Shop", "h").unwrap().unwrap();
        members::set_seller(&c, s, true).unwrap();
        members::set_profile_ok(&c, s, true).unwrap();
        let slug = members::get(&c, s).unwrap().unwrap().slug;
        let mut binders = vec![];
        for (i, title) in ["Fire", "Water", "Grass", "Empty"].iter().enumerate() {
            let b = members::create_member_binder(&c, s, title).unwrap();
            for j in 0..(3 - i.min(3)) {
                let Added::Yes(id) = sellers::add_listing(&c, s, &format!("tcgdex:x-{i}-{j}"), &format!("{title} {j}"), "1", "Set", &format!("/p/{i}-{j}.webp"), "").unwrap() else { panic!() };
                members::move_card_to_binder(&c, s, id, Some(b.id)).unwrap();
            }
            binders.push(b);
        }
        let secret = members::create_member_binder(&c, s, "Secret").unwrap();
        c.execute("UPDATE member_binders SET visibility='Private' WHERE id=?1", [secret.id]).unwrap();
        // Fire also holds a card its seller hid: not for sale, so its page doesn't list it and its tile doesn't count it.
        let Added::Yes(hidden) = sellers::add_listing(&c, s, "tcgdex:x-hidden", "Fire hidden", "1", "Set", "/p/hidden.webp", "").unwrap() else { panic!() };
        members::move_card_to_binder(&c, s, hidden, Some(binders[0].id)).unwrap();
        c.execute("UPDATE member_cards SET status='Hidden' WHERE id=?1", [hidden]).unwrap();

        // The store page: the public binders with cards for sale in one grid, each with its pictures; no sideways strip.
        let store = page(&c, &slug, None);
        assert_eq!(store.matches("class=\"sf-bgrid\"").count(), 1);
        assert_eq!(store.matches("class=\"sf-btile\"").count(), 3, "the three public binders with cards for sale: not the empty one, not the private one");
        assert!(!store.contains("sf-binders") && !store.contains(">Secret<") && !store.contains(">Empty<"));
        assert!(store.contains("data-n=\"3\"") && store.contains("data-n=\"2\"") && store.contains("data-n=\"1\"") && !store.contains("data-n=\"0\""));
        assert!(store.contains("<b class=\"sf-bname\">Fire</b><small>3 cards</small>"), "the cards its page shows, not the hidden one");
        assert!(store.contains("<b class=\"sf-bname\">Grass</b><small>1 card</small>"));
        assert!(!store.contains(" data-off>"), "a visitor sees nothing marked for the owner");
        assert!(store.contains("class=\"sf-head") && store.contains(&format!("data-vouches=\"{slug}\"")), "the store page keeps its header, with the Vouches button");
        // The same four numbers as every store, the vault's included.
        assert!(store.contains("<li><b>3</b><span>binders</span></li>") && store.contains("<span>followers</span>") && store.contains(">Vouches <svg"), "{store}");
        // The owner sees every binder; the ones visitors don't see are marked and faded.
        let own = page_as(&c, &slug, None, true);
        assert_eq!(own.matches("class=\"sf-btile\"").count(), 5);
        assert!(own.contains("<b class=\"sf-bname\">Empty</b><small>No cards for sale · hidden from visitors</small>"), "{own}");
        assert!(own.contains("<b class=\"sf-bname\">Secret</b><small>No cards for sale · private</small>"));
        assert_eq!(own.matches(" data-off>").count(), 2);

        // A binder: a bar with an X back to the store, its name and count; no store header, no way back by crumbs, no reviews.
        let fire = page(&c, &slug, Some(&binders[0].slug));
        assert!(fire.contains(&format!("<a class=\"sf-bx\" href=\"/store/{slug}\" aria-label=\"Close\"")), "{fire}");
        assert!(fire.contains("<h1 class=\"sf-bh\">Fire</h1>") && fire.contains("3 cards · by"));
        assert!(!fire.contains("class=\"sf-head") && !fire.contains("data-vouches") && !fire.contains("← All of"));
        assert_eq!(fire.matches("class=\"sf-card\"").count(), 3, "the binder's cards, with their buttons");
        assert!(!fire.contains("data-offer") && !fire.contains("id=\"buySheet\"") && fire.contains("storefront.js"), "Offer and Message live in each card's window");
    }

    /// The vault's own binders and cards: Pikachu Set with two copies for sale and one sold; Graded Cards, never in the
    /// menu and hidden from today's home page; Trainers sold out; Legendary Collections empty.
    fn vault(c: &rusqlite::Connection) {
        for (i, (h, t, shown)) in [("pikachu-set", "Pikachu Set", true), ("graded-cards", "Graded Cards", false), ("trainers", "Trainers", true), ("legendary-collections", "Legendary Collections", true)].iter().enumerate() {
            c.execute("INSERT INTO collections(handle,title,position,on_home,in_menu) VALUES(?1,?2,?3,?4,?4)", rusqlite::params![h, t, i as i64, shown]).unwrap();
        }
        for (id, title, price, image, sale, binder) in [
            (1, "Pikachu V", 500.0, "/uploads/c0-1_l.jpg", true, "pikachu-set"),
            (2, "Pikachu ex", 900.0, "/uploads/c0-2_l.jpg", true, "pikachu-set"),
            (3, "Raichu", 50.0, "/uploads/c0-3_l.jpg", false, "pikachu-set"),
            (4, "Iono", 300.0, "/uploads/c0-4_l.jpg", false, "trainers"),
            (5, "Charizard PSA 10", 9000.0, "/uploads/c0-5_l.jpg", true, "graded-cards"),
        ] {
            c.execute(
                "INSERT INTO products(id,handle,title,price,image,available,created_at) VALUES(?1,?2,?3,?4,?5,?6,'2026-01-01T00:00:00')",
                rusqlite::params![id, format!("c{id}"), title, price, image, sale],
            )
            .unwrap();
            c.execute("INSERT INTO collection_products(collection,product_id) VALUES(?1,?2)", rusqlite::params![binder, id]).unwrap();
        }
        db::set_binder_cover(c, "graded-cards", "/uploads/b-slabs_l.jpg").unwrap();
    }

    fn vault_html(c: &rusqlite::Connection, q: &str) -> String {
        let html = render(vault_page(c, db::Query { q: q.into(), in_stock: true, ..Default::default() }).unwrap());
        html.unwrap()
    }

    #[test]
    fn the_vault_has_its_own_store_with_its_binders() {
        let c = db();
        vault(&c);
        assert_eq!(VAULT_STORE, format!("/store/{}", sellers::VAULT_SLUG));
        let html = vault_html(&c, "");
        assert!(html.contains("<title>Big Boys Vault · Store</title>"), "{html}");
        assert!(html.contains("<link rel=\"canonical\" href=\"https://www.bigboysvault.com/store/vault\">"));
        // A store like every other: no "official" badge anywhere, and the same buttons as a trainer's store.
        assert!(html.contains("<h1 class=\"h1\">Big Boys Vault</h1>") && !html.to_lowercase().contains("official"));
        assert!(html.contains("id=\"followBtn\"") && html.contains("data-dm-to=\"0\"") && html.contains("data-bm=\"store\" data-id=\"0\""));
        assert!(!html.contains("href=\"/ask\"><svg") && !html.contains("<a class=\"btn ghost\" href=\"/chat\">"), "Ask the Vault and Vault chat live in the menu, not on a store");
        assert!(html.contains("data-vouches=\"vault\"") && html.contains("<b id=\"followerN\">0</b>") && html.contains("since Jan 2026"));
        // The staff's link is in every visitor's page, hidden unless a full admin is signed in: the page can be kept.
        assert!(html.contains("<a class=\"btn ghost\" href=\"/admin/binders\" data-staff-only data-full-only>"));
        // Every binder with cards for sale, in the owner's order, menu or not, home page or not; no other.
        let grid = &html[html.find("<ul class=\"sf-bgrid\">").unwrap()..];
        let grid = &grid[..grid.find("</ul>").unwrap()];
        assert_eq!(grid.matches("class=\"sf-btile\"").count(), 2);
        let pika = grid.find("href=\"/collections/pikachu-set\"").expect("Pikachu Set's tile");
        let graded = grid.find("href=\"/collections/graded-cards\"").expect("Graded Cards' tile, though it was never in the menu");
        assert!(pika < graded, "the owner's order");
        assert!(!grid.contains("/collections/trainers") && !grid.contains("/collections/legendary-collections"), "sold out or empty: no tile");
        // A tile: the cards for sale, dearest first, never the sold one; the binder's own count, as its page says it.
        assert!(html.contains("<span class=\"sf-bcov\" data-n=\"2\"><img src=\"/uploads/c0-2_s.jpg\""));
        assert!(html.contains("<img src=\"/uploads/c0-1_s.jpg\"") && !html.contains("c0-3_s.jpg") && !html.contains("c0-3_l.jpg"));
        assert!(html.contains("<b class=\"sf-bname\">Pikachu Set</b><small>3 cards</small>"));
        assert_eq!(db::cards_in(&c, Some("pikachu-set")).unwrap(), 3, "the number the binder's page shows");
        // An uploaded cover fills its tile alone.
        assert!(html.contains("<span class=\"sf-bcov sf-bphoto\" data-n=\"1\"><img src=\"/uploads/b-slabs_s.jpg\""));
        // Its numbers: copies for sale and binders, never what they are worth.
        assert!(html.contains("<li><b>3</b><span>cards for sale</span></li>") && html.contains("<li><b>2</b><span>binders</span></li>"));
        assert!(!html.contains("10,400"), "the vault's total value stays off the public page");
        // Everything in the vault: its cards for sale, as everywhere else.
        assert!(html.contains("Everything on sale"));
        for id in [1, 2, 5] {
            assert!(html.contains(&format!("<a class=\"card\" href=\"/products/c{id}\"")), "card {id}");
        }
        assert!(!html.contains("href=\"/products/c3\"") && !html.contains("href=\"/products/c4\""), "sold cards aren't for sale");
        // What search engines read: the store, and its binders as a list.
        assert!(html.contains("\"@type\":\"OnlineStore\"") && html.contains("https://www.bigboysvault.com/collections/pikachu-set"));
        // A search of the store isn't a page of its own.
        let found = vault_html(&c, "charizard");
        assert!(found.contains("<meta name=\"robots\" content=\"noindex, follow\">"));
        assert_eq!(found.matches("<a class=\"card\" href=").count(), 1);
    }

    /// A seller's binder editor. Hide makes a listing Hidden: visitors don't see it, the editor still shows it (marked
    /// hidden). Show puts it back on sale, or out of stock when none are left, without changing how many there are. Sort
    /// puts the cards in order and saves it the way dragging does.
    #[test]
    fn a_seller_hides_shows_and_sorts_a_binders_cards() {
        let c = db();
        let s = members::create(&c, "Shop", "h").unwrap().unwrap();
        members::set_seller(&c, s, true).unwrap();
        members::set_profile_ok(&c, s, true).unwrap();
        let slug = members::get(&c, s).unwrap().unwrap().slug;
        let b = members::create_member_binder(&c, s, "Mix").unwrap();
        let mut ids = vec![];
        for (k, name, no) in [("tcgdex:x-1", "Mew", "8/102"), ("tcgdex:x-2", "Abra", "43/102"), ("tcgdex:x-3", "Charizard", "4/102")] {
            let Added::Yes(id) = sellers::add_listing(&c, s, k, name, no, "Base Set", &format!("/p/{name}.webp"), "").unwrap() else { panic!() };
            members::move_card_to_binder(&c, s, id, Some(b.id)).unwrap();
            ids.push(id);
        }
        let set = |id: i64, st: &str| sellers::update_listing(&c, s, id, &Patch { status: Some(st.into()), ..Default::default() }).unwrap().unwrap();

        assert_eq!(set(ids[1], "Hidden").status, "Hidden");
        let mix = page(&c, &slug, Some(&b.slug));
        assert!(!mix.contains("Abra") && mix.contains("Mew"), "visitors don't see a hidden card");
        let editor = editor_cards(&c, s, b.id).unwrap();
        assert_eq!(editor.len(), 3, "its owner still does, in the binder's editor");
        let abra = binder_card_json(editor.iter().find(|k| k.id == ids[1]).unwrap(), false, "");
        assert_eq!((abra["hidden"].as_bool(), abra["note"].as_str()), (Some(true), Some("")), "marked hidden, not with a note");
        assert_eq!(binder_card_json(editor.iter().find(|k| k.id == ids[0]).unwrap(), false, "")["hidden"], false);

        // Show: none left, so it comes back out of stock and still says none; with one left, on sale.
        c.execute("UPDATE member_cards SET qty=0 WHERE id=?1", [ids[1]]).unwrap();
        let back = set(ids[1], "Out of stock");
        assert_eq!((back.status.as_str(), back.qty), ("Out of stock", 0));
        assert_eq!(set(ids[0], "Hidden").status, "Hidden");
        assert_eq!(set(ids[0], "Active").status, "Active");

        // Sort: worked out by binder_sort, saved like a drag.
        let cards: Vec<binder_sort::SortCard> =
            editor_cards(&c, s, b.id).unwrap().into_iter().map(|k| binder_sort::SortCard { id: k.id, name: k.title, set: k.card_set, released: String::new(), number: k.number, api_id: k.card_id }).collect();
        let names = || editor_cards(&c, s, b.id).unwrap().into_iter().map(|k| k.title).collect::<Vec<_>>();
        members::order_binder_cards(&c, s, b.id, &binder_sort::order(&cards, binder_sort::By::Az, &Default::default())).unwrap();
        assert_eq!(names(), vec!["Abra", "Charizard", "Mew"]);
        members::order_binder_cards(&c, s, b.id, &binder_sort::order(&cards, binder_sort::By::Number, &Default::default())).unwrap();
        assert_eq!(names(), vec!["Charizard", "Mew", "Abra"]);
        let other = members::create_member_binder(&c, s, "Other").unwrap();
        members::order_binder_cards(&c, s, other.id, &[ids[1], ids[0]]).unwrap();
        assert_eq!(names(), vec!["Charizard", "Mew", "Abra"], "cards of another binder aren't moved by its order");
    }

    /// The binder tools show only for whoever owns the binders: a seller on their own store and binder pages; on the
    /// vault's (kept, the same page for everyone) a button hidden unless a full admin is signed in.
    #[test]
    fn binder_tools_are_offered_to_the_owner_only() {
        let c = db();
        let s = members::create(&c, "Shop", "h").unwrap().unwrap();
        members::set_seller(&c, s, true).unwrap();
        members::set_profile_ok(&c, s, true).unwrap();
        let slug = members::get(&c, s).unwrap().unwrap().slug;
        // No binders yet: the owner still gets the heading, the button and a word on what binders are for.
        let own = page_as(&c, &slug, None, true);
        assert!(own.contains("data-bt-edit=\"seller\"") && own.contains("class=\"bt-none\"") && own.contains("/static/binder-tools.js?v=4"), "{own}");
        let visitor = page(&c, &slug, None);
        assert!(!visitor.contains("data-bt-edit") && !visitor.contains("binder-tools.js") && !visitor.contains(">Binders<"));
        let b = members::create_member_binder(&c, s, "Fire").unwrap();
        let Added::Yes(id) = sellers::add_listing(&c, s, "tcgdex:x-1", "Charmander", "1", "Set", "/p/1.webp", "").unwrap() else { panic!() };
        members::move_card_to_binder(&c, s, id, Some(b.id)).unwrap();
        let own = page_as(&c, &slug, None, true);
        assert!(own.contains(&format!("<a class=\"sf-btile\" href=\"/store/{slug}/binders/fire\" data-b=\"{}\">", b.id)), "{own}");
        assert!(!own.contains("bt-none"));
        let visitor = page(&c, &slug, None);
        assert!(visitor.contains("<h2 class=\"h2\">Binders</h2>") && !visitor.contains("data-bt-edit") && !visitor.contains("bt-head"), "visitors' page as before");
        // A binder's own page: Edit binder for its owner only.
        let fire_own = page_as(&c, &slug, Some("fire"), true);
        assert!(fire_own.contains(&format!("data-bt-binder=\"seller\" data-key=\"{}\"", b.id)) && fire_own.contains("binder-tools.js"));
        assert!(!page(&c, &slug, Some("fire")).contains("data-bt-binder"));

        // The vault's store: the button is in every visitor's page, hidden unless a full admin is signed in.
        vault(&c);
        let html = vault_html(&c, "");
        assert!(html.contains("data-bt-edit=\"vault\" aria-pressed=\"false\" data-staff-only data-full-only>"), "{html}");
        assert!(html.contains("<a class=\"sf-btile\" href=\"/collections/pikachu-set\" data-b=\"pikachu-set\">"));
        // Arranging them, the owner sees every binder, those with nothing for sale faded and saying why.
        let all = vault_tiles(&db::collections(&c).unwrap(), &db::binder_peeks(&c, PEEK_CHOICES).unwrap(), true);
        assert_eq!(all.iter().map(|t| t.key.as_str()).collect::<Vec<_>>(), vec!["pikachu-set", "graded-cards", "trainers", "legendary-collections"]);
        assert_eq!((all[2].off, all[2].words.as_str()), (true, "1 card · none for sale · hidden from visitors"));
        assert_eq!((all[3].off, all[3].words.as_str()), (true, "No cards yet · hidden from visitors"));
        assert!(!all[0].off && all[0].count == 3);
    }

    /// The same few dearest cards sit in several binders: each tile shows cards the tiles before it don't, when it can.
    #[test]
    fn binder_tiles_on_a_page_show_different_cards() {
        let c = db();
        for (i, h) in ["pikachu-set", "favorites", "playables"].iter().enumerate() {
            c.execute("INSERT INTO collections(handle,title,position) VALUES(?1,?1,?2)", rusqlite::params![h, i as i64]).unwrap();
        }
        for (id, price, binders) in [(1, 900.0, "pikachu-set favorites playables"), (2, 800.0, "pikachu-set favorites playables"), (3, 700.0, "pikachu-set favorites"), (4, 100.0, "pikachu-set"), (5, 90.0, "favorites"), (6, 80.0, "favorites")] {
            c.execute("INSERT INTO products(id,handle,title,price,image,available,created_at) VALUES(?1,?2,?2,?3,?4,1,'2026-01-01T00:00:00')", rusqlite::params![id, format!("c{id}"), price, format!("/uploads/c0-{id}_l.jpg")]).unwrap();
            for b in binders.split(' ') {
                c.execute("INSERT INTO collection_products(collection,product_id) VALUES(?1,?2)", rusqlite::params![b, id]).unwrap();
            }
        }
        let pics = |t: &BinderTile| t.pics.iter().map(|p| p.trim_start_matches("/uploads/c0-").trim_end_matches("_s.jpg").to_string()).collect::<Vec<_>>().join(",");
        let tiles = vault_tiles(&db::collections(&c).unwrap(), &db::binder_peeks(&c, PEEK_CHOICES).unwrap(), false);
        assert_eq!(pics(&tiles[0]), "1,2,3", "the first tile: its dearest");
        assert_eq!(pics(&tiles[1]), "5,6,1", "the next: what isn't shown yet first, then the dearest again");
        assert_eq!(pics(&tiles[2]), "1,2", "nothing new: it still shows its cards");
        // A cover the owner picked always leads, even one an earlier tile shows.
        db::set_binder_cover(&c, "favorites", "/uploads/c0-1_l.jpg").unwrap();
        let tiles = vault_tiles(&db::collections(&c).unwrap(), &db::binder_peeks(&c, PEEK_CHOICES).unwrap(), false);
        assert_eq!(pics(&tiles[1]), "1,5,6");

        // A seller's tiles too: two listings of the same card share a picture.
        let s = members::create(&c, "Shop", "h").unwrap().unwrap();
        members::set_seller(&c, s, true).unwrap();
        let m = members::get(&c, s).unwrap().unwrap();
        let (a, b) = (members::create_member_binder(&c, s, "A").unwrap(), members::create_member_binder(&c, s, "B").unwrap());
        for (n, img, bid) in [(1, "/p/x.webp", a.id), (2, "/p/y.webp", a.id), (3, "/p/x.webp", b.id), (4, "/p/z.webp", b.id)] {
            let Added::Yes(id) = sellers::add_listing(&c, s, &format!("tcgdex:s-{n}"), &format!("Card {n}"), "1", "Set", img, "").unwrap() else { panic!() };
            members::move_card_to_binder(&c, s, id, Some(bid)).unwrap();
        }
        let tiles = seller_tiles(&m, &members::binders_of(&c, s).unwrap(), &sellers::storefront_listings(&c, s).unwrap(), false);
        assert_eq!(tiles[0].pics, vec!["/p/x.webp", "/p/y.webp"]);
        assert_eq!(tiles[1].pics, vec!["/p/z.webp", "/p/x.webp"], "B's first card is A's picture: B leads with its own");
        assert_eq!(tiles[1].key, b.id.to_string());
    }

    /// A seller's binder tile (and its cover everywhere else) never leads with a card that isn't for sale: not a sold or
    /// hidden listing, not a highlight, not a wishlist card, even when it is the binder's first card or the picked cover.
    #[test]
    fn a_binder_tile_never_leads_with_a_card_not_for_sale() {
        let c = db();
        let s = members::create(&c, "Shop", "h").unwrap().unwrap();
        members::set_seller(&c, s, true).unwrap();
        members::set_profile_ok(&c, s, true).unwrap();
        let b = members::create_member_binder(&c, s, "Mixed").unwrap();
        let mut ids = vec![];
        for (pos, (name, kind, status)) in [("own", "own", "Active"), ("wish", "wish", "Active"), ("sold", "sale", "Sold"), ("hidden", "sale", "Hidden"), ("ok1", "sale", "Active"), ("ok2", "sale", "Reserved")].iter().enumerate() {
            let Added::Yes(id) = sellers::add_listing(&c, s, &format!("tcgdex:m-{name}"), name, "1", "Set", &format!("/p/{name}.webp"), "").unwrap() else { panic!() };
            members::move_card_to_binder(&c, s, id, Some(b.id)).unwrap();
            c.execute("UPDATE member_cards SET kind=?2, status=?3, position=?4 WHERE id=?1", rusqlite::params![id, kind, status, pos as i64]).unwrap();
            ids.push(id);
        }
        let m = members::get(&c, s).unwrap().unwrap();
        let look = |c: &rusqlite::Connection| {
            let binders = members::binders_of(c, s).unwrap();
            let listings = sellers::storefront_listings(c, s).unwrap();
            let tile = seller_tiles(&m, &binders, &listings, false).remove(0).pics;
            (binders[0].cover_image.clone(), tile, sellers::binder_peeks(&binders[0], &listings, 3))
        };
        let (cover, tile, peeks) = look(&c);
        assert_eq!(cover, "/p/ok1.webp", "no cover picked: the first card on sale, not the highlight, wish, sold or hidden ones before it");
        assert_eq!(tile, vec!["/p/ok1.webp", "/p/ok2.webp"]);
        assert_eq!(peeks, tile, "binder_peeks agrees");
        // A picked cover that has sold (or is hidden, a highlight...) doesn't lead either.
        for gone in &ids[..4] {
            members::set_member_binder_cover(&c, s, b.id, Some(*gone)).unwrap();
            let (cover, tile, peeks) = look(&c);
            assert_eq!((cover.as_str(), tile.clone(), peeks), ("/p/ok1.webp", vec!["/p/ok1.webp".to_string(), "/p/ok2.webp".into()], tile));
        }
        // A picked cover on sale leads.
        members::set_member_binder_cover(&c, s, b.id, Some(ids[5])).unwrap();
        let (cover, tile, peeks) = look(&c);
        assert_eq!(cover, "/p/ok2.webp");
        assert_eq!(tile, vec!["/p/ok2.webp", "/p/ok1.webp"]);
        assert_eq!(peeks, tile);
        // The page visitors get shows none of the others.
        let slug = m.slug.clone();
        let html = page(&c, &slug, None);
        let grid = &html[html.find("<ul class=\"sf-bgrid\">").unwrap()..];
        let grid = &grid[..grid.find("</ul>").unwrap()];
        assert!(grid.contains("<img src=\"/p/ok2.webp\"") && ["own", "wish", "sold", "hidden"].iter().all(|n| !grid.contains(&format!("/p/{n}.webp"))), "{grid}");
    }

    fn stores_html(c: &rusqlite::Connection) -> String {
        let html = render(StoresPage { nav: chrome(c).unwrap(), stores: store_cards(c, "Big Boys Vault").unwrap(), me: None });
        html.unwrap()
    }

    #[test]
    fn every_store_is_listed_alike_the_most_recently_active_first() {
        let c = db();
        vault(&c);
        // No trainer has a store yet: the vault's is there, drawn like any store (no badge, no special frame).
        let html = stores_html(&c);
        assert!(!html.to_lowercase().contains("official") && !html.contains("vault-card") && !html.contains("The vault and its trainers"));
        assert_eq!(html.matches("<article class=\"seller-card").count(), 1);
        assert!(html.contains("href=\"/store/vault\"") && html.contains("3 on sale · 0 sold · 0 followers"), "{html}");
        assert!(html.contains("<a class=\"btn\" href=\"/store/vault\">Visit store →</a>"));
        assert_eq!(html.matches("class=\"sf-btile\" href=\"/collections/").count(), 2, "its binders with cards for sale, not a row of cards");
        assert_eq!(html.matches("<a class=\"card\" href=\"/products/").count(), 0, "no highlighted cards until an admin picks some");
        // A full admin chooses: one binder, and cards (one has sold: it doesn't show).
        sellers::set_featured(&c, 0, &sellers::Featured { binders: vec!["graded-cards".into()], cards: vec!["2".into(), "3".into()] }).unwrap();
        let html = stores_html(&c);
        assert_eq!(html.matches("class=\"sf-btile\" href=\"/collections/").count(), 1);
        assert!(html.contains("class=\"sf-btile\" href=\"/collections/graded-cards\""));
        assert_eq!(html.matches("<a class=\"card\" href=\"/products/").count(), 1);
        assert!(html.contains("<a class=\"card\" href=\"/products/c2\"") && html.contains(">Highlights<"));
        // A trainer opens one and puts up a card today: theirs is the most recently active store, so it comes first,
        // with the same gold Visit button. A seller who picked nothing gets no cards picked for them.
        let s = members::create(&c, "Shop", "h").unwrap().unwrap();
        members::set_seller(&c, s, true).unwrap();
        members::set_profile_ok(&c, s, true).unwrap();
        let Added::Yes(_) = sellers::add_listing(&c, s, "tcgdex:x-1", "Mew", "1", "Set", "/p/mew.webp", "").unwrap() else { panic!() };
        let html = stores_html(&c);
        let shop = members::get(&c, s).unwrap().unwrap();
        let (t, v) = (html.find(&format!("href=\"{}\"", shop.store_url())).unwrap(), html.find("href=\"/store/vault\"").unwrap());
        assert!(t < v, "the store that last put up a card first");
        assert!(html.contains(&format!("<a class=\"btn\" href=\"{}\">Visit store →</a>", shop.store_url())));
        assert!(html.contains("Visit the store to see its 1 card for sale."), "{html}");
        // The vault puts up a card later still: it is first again. Same rule for everyone.
        c.execute("UPDATE products SET created_at='2099-01-01T00:00:00' WHERE id=1", []).unwrap();
        let html = stores_html(&c);
        assert!(html.find("href=\"/store/vault\"").unwrap() < html.find(&format!("href=\"{}\"", shop.store_url())).unwrap());
        // Each store's border colour (gold until its owner picks one).
        sellers::set_border(&c, s, "#4c8dff").unwrap();
        assert!(stores_html(&c).contains("--sborder:#4c8dff;"));
    }

    #[test]
    fn a_border_colour_must_show_on_the_page() {
        assert_eq!(sellers::border_ok("").as_deref(), Some(""));
        assert_eq!(sellers::border_ok("Gold").as_deref(), Some(""));
        assert_eq!(sellers::border_ok(" #4C8DFF ").as_deref(), Some("#4c8dff"));
        assert_eq!(sellers::border_ok("#111111"), None, "too dark to see on the page");
        assert_eq!(sellers::border_ok("red"), None);
        assert_eq!(sellers::border_ok("#12345"), None);
        assert_eq!(sellers::border_ok("url(x)"), None);
        for (_, hex) in sellers::BORDERS {
            assert_eq!(sellers::border_ok(hex).as_deref(), Some(hex), "every colour on offer shows");
        }
    }

    #[test]
    fn vouches_belong_to_a_store_and_its_window_lists_its_reviews_too() {
        let c = db();
        let s = members::create(&c, "Shop", "h").unwrap().unwrap();
        members::set_seller(&c, s, true).unwrap();
        members::set_profile_ok(&c, s, true).unwrap();
        let b = members::create(&c, "Buyer", "h").unwrap().unwrap();
        members::set_profile_ok(&c, b, true).unwrap();
        // One for the vault (the floating Vouch button), one for the shop; a reply joins its vouch's store.
        let members::Posted::Yes(v0) = members::post_vouch(&c, b, None, 0, "Great vault", &[]).unwrap() else { panic!() };
        let members::Posted::Yes(v1) = members::post_vouch(&c, b, None, s, "Fast meetup", &[]).unwrap() else { panic!() };
        let members::Posted::Yes(_) = members::post_vouch(&c, s, Some(v1), 0, "Thanks!", &[]).unwrap() else { panic!() };
        assert_eq!((members::store_vouch_count(&c, 0).unwrap(), members::store_vouch_count(&c, s).unwrap()), (1, 1), "replies don't count");
        let shop = members::store_vouches(&c, s, 10).unwrap();
        assert_eq!(shop.len(), 1);
        assert_eq!((shop[0].id, shop[0].replies.len(), shop[0].replies[0].store_id), (v1, 1, s));
        assert_eq!(members::store_vouches(&c, 0, 10).unwrap()[0].id, v0);
        assert!(members::vouchable_store(&c, 0).unwrap() && members::vouchable_store(&c, s).unwrap() && !members::vouchable_store(&c, b).unwrap());
        // The window: its vouches, with the store's name on each in the Vouches page (hidden in the window itself).
        let items = members::store_vouches(&c, s, 10).unwrap().into_iter().map(VouchItem::Vouch).collect();
        let html = render(StoreVouchesPart { store: s, name: "Shop".into(), items, is_me: false }).unwrap();
        assert!(html.contains("Fast meetup") && html.contains("Thanks!") && html.contains("data-store=\"") && !html.contains("No vouches yet."));
        let empty = render(StoreVouchesPart { store: 0, name: "Big Boys Vault".into(), items: vec![], is_me: false }).unwrap();
        assert!(empty.contains("No vouches yet."));
    }

    #[test]
    fn a_trainer_applies_to_sell_and_an_admin_answers() {
        let c = db();
        let t = members::create(&c, "Newbie", "h").unwrap().unwrap();
        let a = members::create(&c, "Boss", "h").unwrap().unwrap();
        members::set_role(&c, a, "admin").unwrap();
        c.execute_batch(dm::SCHEMA).unwrap();
        let th = dm::thread_between(&c, t, dm::VAULT, 100).unwrap();
        let id = sellers::add_application(&c, t, "Selling since 2019", &[("/uploads/dm1-a_l.jpg".into(), "/uploads/dm1-a_s.jpg".into())], th.id, 100).unwrap();
        assert_eq!(sellers::applications_waiting(&c).unwrap(), 1);
        let got = sellers::application_of(&c, t).unwrap().unwrap();
        assert!(got.waiting() && got.thread_id == th.id);
        assert_eq!(got.pics(), vec![("/uploads/dm1-a_l.jpg".to_string(), "/uploads/dm1-a_s.jpg".to_string())]);
        let (waiting, done) = sellers::applications(&c, 20).unwrap();
        assert_eq!((waiting.len(), done.len()), (1, 0));
        // Approving opens the store; it can't be answered twice.
        let ok = sellers::decide_application(&c, id, true, "", a, 200).unwrap().unwrap();
        assert_eq!(ok.status, "Approved");
        let m = members::get(&c, t).unwrap().unwrap();
        assert!(m.seller && m.profile_ok);
        assert!(sellers::decide_application(&c, id, false, "late", a, 300).unwrap().is_none());
        assert_eq!(sellers::applications_waiting(&c).unwrap(), 0);
    }

    #[test]
    fn bookmarks_keep_stores_and_cards_under_the_store_they_came_from() {
        let c = db();
        vault(&c);
        let s = members::create(&c, "Shop", "h").unwrap().unwrap();
        members::set_seller(&c, s, true).unwrap();
        members::set_profile_ok(&c, s, true).unwrap();
        let Added::Yes(k) = sellers::add_listing(&c, s, "tcgdex:x-1", "Mew", "1", "Set", "/p/mew.webp", "").unwrap() else { panic!() };
        let b = members::create(&c, "Buyer", "h").unwrap().unwrap();
        assert!(sellers::toggle_bookmark(&c, b, "store", 0).unwrap());
        assert!(sellers::toggle_bookmark(&c, b, "product", 2).unwrap());
        assert!(sellers::toggle_save(&c, b, k).unwrap(), "a seller's card: the saved listing");
        assert_eq!(sellers::bookmark_count(&c, b).unwrap(), 3);
        let kinds: Vec<String> = sellers::bookmarks_of(&c, b).unwrap().into_iter().map(|(k, _, _)| k).collect();
        assert!(kinds.contains(&"store".to_string()) && kinds.contains(&"product".to_string()) && kinds.contains(&"card".to_string()));
        assert!(!sellers::toggle_bookmark(&c, b, "store", 0).unwrap(), "the same button takes it off");
        assert_eq!(sellers::bookmark_count(&c, b).unwrap(), 2);
    }
}
