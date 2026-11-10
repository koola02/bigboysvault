//! Each seller's store: the seller panel under /me (dashboard, listings, binders, orders, settings,
//! notifications) and the public storefront at /store/<name>. The database side is `sellers.rs`.
use crate::community::{acting, acting_any, client_ip, fail, html, not_found, ok, over_limit, render, store_photo, PHOTO_GATE};
use crate::{admin, chrome, ise, with_db, App, Chrome, Res};
use askama::Template;
use axum::{
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use marcuru_rs::members::{self, Member, MemberBinder, MemberCard, Notification};
use marcuru_rs::sellers::{self, Dashboard, ListFilter, Order, Orders, Patch, Pref, Review, Sale, StoreStats};
use serde::Deserialize;
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

fn panel(c: &rusqlite::Connection, m: Member, tab: &'static str) -> rusqlite::Result<Panel> {
    let (r, o, q) = sellers::pending(c, m.id)?;
    let unread = members::unread_notification_count(c, m.id)?;
    Ok(Panel { m, tab, todo: r + o + q, unread })
}

/// Signed in, or sent to sign in. Pages, not JSON.
async fn signed_in(app: &App, h: &HeaderMap) -> Result<Result<Member, Response>, (StatusCode, String)> {
    Ok(match crate::community::current(app, h).await? {
        Some(m) => Ok(m),
        None => Err(Redirect::to("/join?signin=1").into_response()),
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

#[derive(Deserialize)]
pub struct ListQs {
    #[serde(default)]
    q: String,
    #[serde(default)]
    status: String,
    binder: Option<i64>,
    #[serde(default)]
    sort: String,
    /// Open this listing's editor straight away (links from notifications).
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
    statuses: [&'static str; 5],
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
    let f = ListFilter { q: q.q.chars().take(60).collect(), status: q.status, binder: q.binder, sort: q.sort };
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
        "flagged": c.flagged, "when": c.when()
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
    let (patch, flag) = (req.patch(), req.flag);
    let card = with_db(&app, move |c| {
        if let Some(f) = flag {
            sellers::flag_card(c, mid, id, f)?;
        }
        sellers::update_listing(c, mid, id, &patch)
    })
    .await?;
    let Some(card) = card else { return Ok(fail(StatusCode::NOT_FOUND, "That card isn't in your store.")) };
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
    let (mid, ids, action, percent, patch) = (m.id, req.ids, req.action, req.percent.unwrap_or(0.0), req.fields.patch());
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
            for k in gone.iter().filter(|k| k.own_photo()) {
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

pub async fn binders(State(app): State<App>, h: HeaderMap) -> Res {
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
    with_db(&app, move |c| {
        for (pos, cid) in ids.iter().enumerate() {
            c.execute("UPDATE member_cards SET position=?4 WHERE member_id=?1 AND id=?2 AND binder_id=?3", rusqlite::params![id, cid, bid, pos as i64])?;
        }
        Ok(())
    })
    .await?;
    app.forget();
    Ok(ok(json!({ "ok": true })))
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
}

pub async fn orders(State(app): State<App>, h: HeaderMap, Query(q): Query<TabQs>) -> Res {
    let m = match signed_in(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    let tab = match q.tab.as_str() {
        "buying" | "questions" | "sales" => q.tab.clone(),
        _ => "selling".to_string(),
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
    Ok(html(render(OrdersPage { nav, p, tab, selling, buying, sales, purchases, pay, methods: sellers::PAY_METHODS })?))
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
    let (mut cover, mut methods, mut drop_cover) = (None::<Vec<u8>>, Vec::<String>::new(), false);
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
            "contact_public" => s.contact_public = v == "1" || v == "on",
            "status" => s.status = v,
            "fb_url" => s.fb_url = v,
            "fb_group_url" => s.fb_group_url = v,
            "ig_url" => s.ig_url = v,
            "tiktok_url" => s.tiktok_url = v,
            "shopee_url" => s.shopee_url = v,
            "links" => s.links = v,
            "pay_method" => methods.push(v),
            "pay_methods_sent" => { s.pay_methods = String::new(); s.pay_public = false; s.notify_views = false; s.contact_public = false; }
            "pay_info" => s.pay_info = v,
            "email" => s.email = v,
            "notify_views" => s.notify_views = v == "1" || v == "on",
            "pay_public" => s.pay_public = v == "1" || v == "on",
            "drop_cover" => drop_cover = v == "1" || v == "on",
            _ => {}
        }
    }
    if !methods.is_empty() {
        s.pay_methods = methods.join(",");
    }
    let mut new_cover = None;
    if let Some(bytes) = cover {
        let gate = PHOTO_GATE.acquire().await.map_err(ise)?;
        let dir = PathBuf::from(&*app.uploads);
        let stem = format!("cover{}-{}", m.id, members::temp_password());
        let made = tokio::task::spawn_blocking(move || store_photo(bytes, dir, stem)).await.map_err(ise)?;
        drop(gate);
        match made {
            Ok((large, small, _, _, _)) => {
                admin::remove_upload(&app.uploads, &small);
                new_cover = Some(large);
            }
            Err(_) => return Ok(fail(StatusCode::BAD_REQUEST, "That cover photo couldn't be read. Try a JPEG or PNG.")),
        }
    }
    let (id, old_cover) = (m.id, m.cover_photo.clone());
    let cover_now = new_cover.clone();
    with_db(&app, move |c| {
        sellers::save_store(c, id, &s)?;
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
    Ok(html(render(NotificationsPage { nav, p, notes, page, pages, prefs, push_key: std::env::var("VAPID_PUBLIC_KEY").unwrap_or_default(), has_email })?))
}

#[derive(Deserialize)]
pub struct PrefReq {
    event: String,
    site: bool,
    email: bool,
    push: bool,
}

pub async fn save_pref(State(app): State<App>, h: HeaderMap, Json(req): Json<PrefReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
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

/// Saves a browser's push subscription. Sending needs VAPID keys and a push library: see docs/SELLER_API.md.
pub async fn push_subscribe(State(app): State<App>, h: HeaderMap, Json(req): Json<PushReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let id = m.id;
    with_db(&app, move |c| if req.remove { sellers::push_unsubscribe(c, id, &req.endpoint) } else { sellers::push_subscribe(c, id, &req.endpoint, &req.keys.p256dh, &req.keys.auth) }).await?;
    Ok(ok(json!({ "ok": true, "enabled": std::env::var("PUSH_ENABLED").is_ok_and(|v| v == "1") })))
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
    binders: Vec<MemberBinder>,
    binder: Option<MemberBinder>,
    reviews: Vec<Review>,
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
    let (nav, stats, binders, binder, reviews, following, saved) = with_db(app, move |c| {
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
        Ok((chrome(c)?, sellers::store_stats(c, id)?, binders, binder, sellers::reviews_of(c, id, false)?, following, saved))
    })
    .await?;
    let binder = match binder {
        Some(Some(b)) if b.visibility != "Private" || is_me || staff => Some(b),
        Some(_) => return not_found(app).await,
        None => None,
    };
    let bid = binder.as_ref().map(|b| b.id);
    let cards: Vec<MemberCard> = with_db(app, move |c| match bid {
        Some(b) => Ok(members::cards_of_binder(c, id, Some(b))?.into_iter().filter(|k| k.on_sale() || k.status == "Sold").collect()),
        None => sellers::storefront_listings(c, id),
    })
    .await?;
    let cards: Vec<(MemberCard, bool)> = cards.into_iter().map(|k| { let s = saved.contains(&k.id); (k, s) }).collect();
    let mut nav = nav;
    let title = match &binder {
        Some(b) => format!("{} · {} · {}", b.title, m.store_title(), nav.brand),
        None => format!("{} · {}", m.store_title(), nav.brand),
    };
    let desc = if m.bio.trim().is_empty() {
        format!("{} sells Pokémon cards on {}: {} on sale.", m.store_title(), nav.brand, stats.on_sale)
    } else {
        crate::clip(&m.bio, 150)
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
    Ok(html(render(StorePage { nav, m, stats, cards, binders, binder, reviews, me, following, is_me, links, conditions: members::CONDITIONS })?))
}

pub async fn storefront(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>) -> Res {
    store_for(&app, &h, slug, None).await
}

pub async fn store_binder(State(app): State<App>, h: HeaderMap, Path((slug, bslug)): Path<(String, String)>) -> Res {
    store_for(&app, &h, slug, Some(bslug)).await
}

#[derive(Template)]
#[template(path = "stores.html")]
struct StoresPage {
    nav: Chrome,
    stores: Vec<(Member, StoreStats, Vec<MemberCard>)>,
    me: Option<Member>,
}

pub async fn directory(State(app): State<App>, h: HeaderMap) -> Res {
    let me = crate::community::current(&app, &h).await?;
    let (mut nav, stores) = with_db(&app, |c| Ok((chrome(c)?, sellers::directory(c)?))).await?;
    nav.seo = crate::Seo {
        title: format!("Stores · {}", nav.brand),
        desc: format!("Pokémon card stores run by the trainers of {}: browse what's on sale, reserve a card and pay the seller directly.", nav.brand),
        path: "/stores".into(),
        kind: "website",
        ..Default::default()
    };
    Ok(html(render(StoresPage { nav, stores, me })?))
}

async fn seller_by_slug(app: &App, slug: String) -> Result<Option<Member>, (StatusCode, String)> {
    with_db(app, move |c| members::by_slug(c, &slug)).await
}

pub async fn follow(State(app): State<App>, h: HeaderMap, Path(slug): Path<String>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let Some(seller) = seller_by_slug(&app, slug).await? else { return Ok(fail(StatusCode::NOT_FOUND, "No such store.")) };
    let (me, sid) = (m.id, seller.id);
    let following = with_db(&app, move |c| sellers::toggle_follow(c, me, sid)).await?;
    let n = with_db(&app, move |c| sellers::followers(c, sid)).await?;
    Ok(ok(json!({ "ok": true, "following": following, "followers": n })))
}

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
}

pub async fn admin_sellers(State(app): State<App>, h: HeaderMap) -> Res {
    if !admin::can_moderate(&app, &h) {
        return Ok(Redirect::to("/admin").into_response());
    }
    let (nav, stores, flagged, reviews, sellers_n) = with_db(&app, |c| {
        let n: i64 = c.query_row("SELECT COUNT(*) FROM members WHERE blocked=0 AND seller=1", [], |r| r.get(0))?;
        Ok((chrome(c)?, sellers::directory(c)?, sellers::flagged_cards(c)?, sellers::all_reviews(c, 50)?, n))
    })
    .await?;
    Ok(html(render(AdminSellersPage { nav, tab: "sellers", stores, flagged, reviews, sellers_n })?))
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
                    "reservation" => "Someone reserved a card",
                    "offer" => "You have an offer",
                    "sale" => "A sale is done",
                    "inquiry" => "A question from a buyer",
                    "review" => "A new review",
                    "follow" => "A new follower",
                    "expiry" => "A listing expired",
                    "announcement" => "News from the vault",
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
