//! Every trainer can run a store. This is the store's database side: listings with stock and prices, orders
//! (reservations and offers), buyers' questions, reviews, followers, views and notifications, and the numbers
//! on the seller's dashboard. The pages are in the binary's `store.rs`.
//!
//! Buying stays informal: a buyer reserves a card or makes an offer, the seller accepts and shows how to pay
//! (GCash, Maya, bank), the buyer pays in their own app, and the seller marks it paid. Then the stock goes down.
use crate::members::{self, now, MemberBinder, MemberCard};
use rusqlite::{Connection, OptionalExtension, Row};
use serde::Serialize;

/// Listing statuses, in the order a listing moves through them.
pub const STATUSES: [&str; 5] = ["Active", "Reserved", "Sold", "Hidden", "Expired"];
/// Ways to pay a seller can tick.
pub const PAY_METHODS: [&str; 6] = ["GCash", "Maya", "Bank transfer", "GoTyme", "Cash on meetup", "Other"];
/// Reservations the seller hasn't answered expire after this long. Confirmed ones wait this long for payment.
pub const RESERVATION_DAYS: i64 = 3;
/// Names a store address can't take, because the site uses them.
const RESERVED_SLUGS: [&str; 35] = [
    "admin", "api", "me", "store", "stores", "seller", "sellers", "trainer", "trainers", "join", "signin", "signout", "static", "uploads",
    "all", "chat", "ask", "vouch", "vouches", "collections", "products", "cards", "search", "robots.txt", "sitemap.xml", "about", "help",
    "support", "login", "logout", "new", "edit", "delete", "settings", "bigboysvault",
];

/// Adds what the store needs to an existing database. Only ever adds (see GEMINI.md §5).
pub fn migrate(c: &Connection) -> rusqlite::Result<()> {
    for (col, def) in [
        ("email", "TEXT NOT NULL DEFAULT ''"),
        ("store_name", "TEXT NOT NULL DEFAULT ''"),
        ("fb_group_url", "TEXT NOT NULL DEFAULT ''"),
        ("ig_url", "TEXT NOT NULL DEFAULT ''"),
        ("shopee_url", "TEXT NOT NULL DEFAULT ''"),
        ("pay_info", "TEXT NOT NULL DEFAULT ''"),
        ("pay_methods", "TEXT NOT NULL DEFAULT ''"),
        ("notify_views", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column(c, "members", col, def)?;
    }
    for (col, def) in [
        ("qty", "INTEGER NOT NULL DEFAULT 1"),
        ("finish", "TEXT NOT NULL DEFAULT ''"),
        ("notes", "TEXT NOT NULL DEFAULT ''"),
        ("sold", "INTEGER NOT NULL DEFAULT 0"),
        ("view_n", "INTEGER NOT NULL DEFAULT 0"),
        ("save_n", "INTEGER NOT NULL DEFAULT 0"),
        ("updated_at", "INTEGER NOT NULL DEFAULT 0"),
        ("review_flag", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column(c, "member_cards", col, def)?;
    }
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS reservations(
           id INTEGER PRIMARY KEY AUTOINCREMENT, buyer_id INTEGER NOT NULL, seller_id INTEGER NOT NULL, card_id INTEGER NOT NULL,
           status TEXT NOT NULL DEFAULT 'Pending', expires_at INTEGER NOT NULL, created_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS offers(
           id INTEGER PRIMARY KEY AUTOINCREMENT, sender_id INTEGER NOT NULL, seller_id INTEGER NOT NULL, card_id INTEGER NOT NULL,
           amount REAL NOT NULL, status TEXT NOT NULL DEFAULT 'Pending', created_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS inquiries(
           id INTEGER PRIMARY KEY AUTOINCREMENT, sender_id INTEGER NOT NULL, seller_id INTEGER NOT NULL, card_id INTEGER, message TEXT NOT NULL, created_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS transactions(
           id INTEGER PRIMARY KEY AUTOINCREMENT, buyer_id INTEGER NOT NULL, seller_id INTEGER NOT NULL, card_id INTEGER NOT NULL,
           amount REAL NOT NULL, status TEXT NOT NULL DEFAULT 'Completed', created_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS reviews(
           id INTEGER PRIMARY KEY AUTOINCREMENT, reviewer_id INTEGER NOT NULL, seller_id INTEGER NOT NULL, transaction_id INTEGER NOT NULL,
           rating INTEGER NOT NULL, text TEXT NOT NULL, created_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS store_follows(
           id INTEGER PRIMARY KEY AUTOINCREMENT, follower_id INTEGER NOT NULL, seller_id INTEGER NOT NULL, created_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS listing_views(
           id INTEGER PRIMARY KEY AUTOINCREMENT, card_id INTEGER NOT NULL, viewer_ip_hash TEXT NOT NULL, created_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS listing_saves(
           id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, card_id INTEGER NOT NULL, created_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS notifications(
           id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, kind TEXT NOT NULL, message TEXT NOT NULL, link TEXT NOT NULL,
           is_read INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS notify_prefs(member_id INTEGER NOT NULL, event TEXT NOT NULL, site INTEGER NOT NULL DEFAULT 1,
           email INTEGER NOT NULL DEFAULT 0, push INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(member_id, event));
         CREATE TABLE IF NOT EXISTS push_subscriptions(
           id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, endpoint TEXT NOT NULL, p256dh TEXT NOT NULL, auth TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS card_events(
           id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, card_id INTEGER NOT NULL, kind TEXT NOT NULL, detail TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_sf_pair ON store_follows(follower_id, seller_id);
         CREATE INDEX IF NOT EXISTS idx_reviews_seller ON reviews(seller_id, created_at);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_reviews_tx ON reviews(transaction_id);
         CREATE INDEX IF NOT EXISTS idx_inquiries_seller ON inquiries(seller_id, created_at);
         CREATE INDEX IF NOT EXISTS idx_inquiries_sender ON inquiries(sender_id, created_at);
         CREATE INDEX IF NOT EXISTS idx_offers_seller ON offers(seller_id, status, created_at);
         CREATE INDEX IF NOT EXISTS idx_offers_sender ON offers(sender_id, created_at);
         CREATE INDEX IF NOT EXISTS idx_res_seller ON reservations(seller_id, status, created_at);
         CREATE INDEX IF NOT EXISTS idx_res_buyer ON reservations(buyer_id, created_at);
         CREATE INDEX IF NOT EXISTS idx_res_card ON reservations(card_id, status);
         CREATE INDEX IF NOT EXISTS idx_tx_seller ON transactions(seller_id, created_at);
         CREATE INDEX IF NOT EXISTS idx_tx_buyer ON transactions(buyer_id, created_at);
         CREATE INDEX IF NOT EXISTS idx_views_card ON listing_views(card_id, created_at);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_saves_member_card ON listing_saves(member_id, card_id);
         CREATE INDEX IF NOT EXISTS idx_notif_member ON notifications(member_id, is_read, created_at);
         CREATE INDEX IF NOT EXISTS idx_push_member ON push_subscriptions(member_id);
         CREATE INDEX IF NOT EXISTS idx_events_member ON card_events(member_id, created_at);
         CREATE INDEX IF NOT EXISTS idx_mcards_status ON member_cards(member_id, status);",
    )?;
    for (col, def) in [("qty", "INTEGER NOT NULL DEFAULT 1"), ("message", "TEXT NOT NULL DEFAULT ''"), ("price", "REAL NOT NULL DEFAULT 0"), ("paid_via", "TEXT NOT NULL DEFAULT ''"), ("updated_at", "INTEGER NOT NULL DEFAULT 0"), ("offer_id", "INTEGER NOT NULL DEFAULT 0")] {
        add_column(c, "reservations", col, def)?;
    }
    for (col, def) in [("qty", "INTEGER NOT NULL DEFAULT 1"), ("message", "TEXT NOT NULL DEFAULT ''"), ("counter", "REAL NOT NULL DEFAULT 0"), ("updated_at", "INTEGER NOT NULL DEFAULT 0")] {
        add_column(c, "offers", col, def)?;
    }
    for (col, def) in [("reply", "TEXT NOT NULL DEFAULT ''"), ("replied_at", "INTEGER NOT NULL DEFAULT 0")] {
        add_column(c, "inquiries", col, def)?;
    }
    for (col, def) in [("qty", "INTEGER NOT NULL DEFAULT 1"), ("reservation_id", "INTEGER NOT NULL DEFAULT 0"), ("paid_via", "TEXT NOT NULL DEFAULT ''"), ("title", "TEXT NOT NULL DEFAULT ''")] {
        add_column(c, "transactions", col, def)?;
    }
    for (col, def) in [("hidden", "INTEGER NOT NULL DEFAULT 0"), ("reply", "TEXT NOT NULL DEFAULT ''")] {
        add_column(c, "reviews", col, def)?;
    }
    add_column(c, "listing_views", "day", "INTEGER NOT NULL DEFAULT 0")?;
    c.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS idx_views_once ON listing_views(card_id, viewer_ip_hash, day)")?;
    Ok(())
}

fn add_column(c: &Connection, table: &str, col: &str, def: &str) -> rusqlite::Result<()> {
    let there: i64 = c.query_row(&format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name=?1"), [col], |r| r.get(0))?;
    if there == 0 {
        c.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col} {def}"))?;
    }
    Ok(())
}

// ---------- the store's address ----------

/// Checks a store address a seller typed: 3 to 30 letters, numbers and dashes, not a name the site uses.
pub fn check_slug(s: &str) -> Result<String, &'static str> {
    let t = s.trim().to_lowercase();
    if t.len() < 3 || t.len() > 30 {
        return Err("A store address is 3 to 30 characters.");
    }
    if !t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || t.starts_with('-') || t.ends_with('-') || t.contains("--") {
        return Err("Use only letters, numbers and single dashes, like big-boys-vault.");
    }
    if RESERVED_SLUGS.contains(&t.as_str()) {
        return Err("That address is taken by the site. Pick another.");
    }
    Ok(t)
}

/// Gives a seller a new store address. Err(text) tells them what's wrong. Ok(false): someone has it.
pub fn set_slug(c: &Connection, member_id: i64, slug: &str) -> rusqlite::Result<Result<bool, &'static str>> {
    let s = match check_slug(slug) {
        Ok(s) => s,
        Err(e) => return Ok(Err(e)),
    };
    let taken: i64 = c.query_row("SELECT COUNT(*) FROM members WHERE lower(slug)=?1 AND id<>?2", rusqlite::params![s, member_id], |r| r.get(0))?;
    if taken > 0 {
        return Ok(Ok(false));
    }
    c.execute("UPDATE members SET slug=?2, updated_at=?3 WHERE id=?1", rusqlite::params![member_id, s, now()])?;
    Ok(Ok(true))
}

/// The seller's store details, saved from the Store settings page.
pub struct StoreSettings {
    pub store_name: String,
    pub location: String,
    pub contact_info: String,
    pub contact_public: bool,
    pub status: String,
    pub fb_url: String,
    pub fb_group_url: String,
    pub ig_url: String,
    pub tiktok_url: String,
    pub shopee_url: String,
    pub links: String,
    pub pay_methods: String,
    pub pay_info: String,
    pub email: String,
    pub notify_views: bool,
}

pub fn save_store(c: &Connection, member_id: i64, s: &StoreSettings) -> rusqlite::Result<()> {
    let status = if ["Active", "Away", "Vacation"].contains(&s.status.as_str()) { s.status.as_str() } else { "Active" };
    let methods: Vec<&str> = s.pay_methods.split(',').map(str::trim).filter(|m| PAY_METHODS.contains(m)).collect();
    c.execute(
        "UPDATE members SET store_name=?2, location=?3, contact_info=?4, contact_public=?5, status=?6, fb_url=?7, fb_group_url=?8, ig_url=?9, tiktok_url=?10,
           shopee_url=?11, links=?12, pay_methods=?13, pay_info=?14, email=?15, notify_views=?16, updated_at=?17 WHERE id=?1",
        rusqlite::params![
            member_id, members::clean_line(&s.store_name, 40), members::clean_line(&s.location, 80), members::clean_text(&s.contact_info, 300), s.contact_public, status,
            members::clean_https_url(&s.fb_url), members::clean_https_url(&s.fb_group_url), members::clean_https_url(&s.ig_url), members::clean_https_url(&s.tiktok_url),
            members::clean_https_url(&s.shopee_url), clean_link_lines(&s.links), methods.join(","), members::clean_text(&s.pay_info, 600),
            clean_email(&s.email), s.notify_views, now()
        ],
    )?;
    Ok(())
}

/// Extra links, one per line, as "Label | https://..." or just the address. Only good https:// links, at most ten.
pub fn clean_link_lines(text: &str) -> String {
    text.lines()
        .filter_map(|l| {
            let (label, url) = match l.split_once('|') {
                Some((a, b)) => (members::clean_line(a, 30), b),
                None => (String::new(), l),
            };
            let url = members::clean_https_url(url);
            if url.is_empty() {
                None
            } else if label.is_empty() {
                Some(url)
            } else {
                Some(format!("{label} | {url}"))
            }
        })
        .take(10)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A link line as (label, url). A line without a label is named after its site.
pub fn link_parts(line: &str) -> (String, String) {
    match line.split_once(" | ") {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (site_name(line), line.to_string()),
    }
}

fn site_name(url: &str) -> String {
    let host = url.trim_start_matches("https://").split('/').next().unwrap_or("").trim_start_matches("www.");
    let name = host.split('.').next().unwrap_or("Link");
    let mut cs = name.chars();
    match cs.next() {
        Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
        None => "Link".into(),
    }
}

fn clean_email(s: &str) -> String {
    let t = s.trim();
    if t.len() <= 120 && t.contains('@') && !t.contains(char::is_whitespace) && t.split('@').nth(1).is_some_and(|d| d.contains('.')) {
        t.to_string()
    } else {
        String::new()
    }
}

// ---------- listings ----------

/// The catalogue id behind a listing: a second listing of the same card is "tcgdex:base1-58#2".
pub fn catalogue_key(card_id: &str) -> &str {
    card_id.split('#').next().unwrap_or(card_id)
}

/// How many cards a seller may list. The owner can change it in the admin settings.
pub fn max_cards(c: &Connection) -> i64 {
    crate::blocks::setting(c, "seller_max_cards").ok().flatten().and_then(|v| v.trim().parse().ok()).filter(|n| *n > 0).unwrap_or(members::MAX_CARDS)
}

/// Adds a card to a seller's store. The same card can be listed more than once (say, a Near Mint and a Lightly Played copy).
pub fn add_listing(c: &Connection, member_id: i64, card_id: &str, title: &str, number: &str, set: &str, image: &str, image_large: &str) -> rusqlite::Result<members::Added> {
    let key = catalogue_key(card_id).to_string();
    let n: i64 = c.query_row("SELECT COUNT(*) FROM member_cards WHERE member_id=?1 AND (card_id=?2 OR card_id LIKE ?2 || '#%')", rusqlite::params![member_id, key], |r| r.get(0))?;
    let id = if n == 0 { key } else { format!("{key}#{}", n + 1) };
    let added = members::add_card(c, member_id, &id, title, number, set, image, image_large)?;
    if let members::Added::Yes(cid) = added {
        c.execute("UPDATE member_cards SET updated_at=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, cid, now()])?;
        log_event(c, member_id, cid, "listed", "")?;
    }
    Ok(added)
}

/// What a seller can change on one listing. None = leave it.
#[derive(Default)]
pub struct Patch {
    pub price: Option<f64>,
    pub cond: Option<String>,
    pub qty: Option<i64>,
    pub status: Option<String>,
    pub finish: Option<String>,
    pub notes: Option<String>,
    pub binder_id: Option<Option<i64>>,
    pub expires_at: Option<i64>,
    pub title: Option<String>,
    pub card_set: Option<String>,
    pub number: Option<String>,
}

/// Changes a listing. Returns the listing as it is now, or None if it isn't this seller's.
pub fn update_listing(c: &Connection, member_id: i64, id: i64, p: &Patch) -> rusqlite::Result<Option<MemberCard>> {
    let Some(before) = members::card(c, member_id, id)? else { return Ok(None) };
    if let Some(v) = p.price {
        c.execute("UPDATE member_cards SET value_own=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, v.max(0.0).round()])?;
    }
    if let Some(cond) = &p.cond {
        if members::CONDITIONS.contains(&cond.as_str()) || crate::pricing::is_graded(cond) {
            c.execute("UPDATE member_cards SET cond=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, members::clean_line(cond, 40)])?;
        }
    }
    if let Some(q) = p.qty {
        let q = q.clamp(0, 9999);
        c.execute("UPDATE member_cards SET qty=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, q])?;
        // Stock back in: a sold-out card goes back on sale. Stock gone: it's sold out.
        if q > 0 && before.status == "Sold" {
            c.execute("UPDATE member_cards SET status='Active' WHERE member_id=?1 AND id=?2", [member_id, id])?;
        } else if q == 0 && before.status == "Active" {
            c.execute("UPDATE member_cards SET status='Sold' WHERE member_id=?1 AND id=?2", [member_id, id])?;
        }
        if q != before.qty {
            log_event(c, member_id, id, "stock", &format!("{} → {q}", before.qty))?;
        }
    }
    if let Some(s) = &p.status {
        if STATUSES.contains(&s.as_str()) && *s != before.status {
            c.execute("UPDATE member_cards SET status=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, s])?;
            if s == "Active" && before.expires_at > 0 && before.expires_at < now() {
                c.execute("UPDATE member_cards SET expires_at=0 WHERE member_id=?1 AND id=?2", [member_id, id])?;
            }
            log_event(c, member_id, id, "status", &format!("{} → {s}", before.status))?;
            notify(c, member_id, "status", &format!("{} is now {}.", short(&before.title), s), &format!("/me/listings?id={id}"))?;
        }
    }
    if let Some(f) = &p.finish {
        c.execute("UPDATE member_cards SET finish=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, members::clean_line(f, 40)])?;
    }
    if let Some(n) = &p.notes {
        c.execute("UPDATE member_cards SET notes=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, members::clean_text(n, 500)])?;
    }
    if let Some(b) = p.binder_id {
        members::move_card_to_binder(c, member_id, id, b)?;
    }
    if let Some(e) = p.expires_at {
        c.execute("UPDATE member_cards SET expires_at=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, e.max(0)])?;
        if e > now() && before.status == "Expired" {
            c.execute("UPDATE member_cards SET status='Active' WHERE member_id=?1 AND id=?2", [member_id, id])?;
        }
    }
    if before.own_photo() || before.is_custom {
        if let Some(t) = &p.title {
            let t = members::clean_line(t, 120);
            if !t.is_empty() {
                c.execute("UPDATE member_cards SET title=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, t])?;
            }
        }
        if let Some(s) = &p.card_set {
            c.execute("UPDATE member_cards SET card_set=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, members::clean_line(s, 80)])?;
        }
        if let Some(n) = &p.number {
            c.execute("UPDATE member_cards SET number=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, members::clean_line(n, 20)])?;
        }
    }
    c.execute("UPDATE member_cards SET updated_at=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, now()])?;
    members::card(c, member_id, id)
}

/// The same change on many listings at once. Returns how many changed.
pub fn bulk_update(c: &Connection, member_id: i64, ids: &[i64], p: &Patch) -> rusqlite::Result<usize> {
    let mut n = 0;
    for id in ids.iter().take(500) {
        if update_listing(c, member_id, *id, p)?.is_some() {
            n += 1;
        }
    }
    Ok(n)
}

/// Raises or lowers every chosen price by a percentage (-90 to +500). Prices that follow the market get a typed one first.
pub fn bulk_reprice(c: &Connection, member_id: i64, ids: &[i64], percent: f64) -> rusqlite::Result<usize> {
    let k = 1.0 + percent.clamp(-90.0, 500.0) / 100.0;
    let mut n = 0;
    for id in ids.iter().take(500) {
        let Some(card) = members::card(c, member_id, *id)? else { continue };
        let base = card.worth();
        if base <= 0.0 {
            continue;
        }
        c.execute("UPDATE member_cards SET value_own=?3, updated_at=?4 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, crate::pricing::round_php(base * k), now()])?;
        n += 1;
    }
    Ok(n)
}

/// Takes listings out of the store.
pub fn bulk_delete(c: &Connection, member_id: i64, ids: &[i64]) -> rusqlite::Result<Vec<MemberCard>> {
    let mut gone = vec![];
    for id in ids.iter().take(500) {
        if let Some(k) = members::card(c, member_id, *id)? {
            members::remove_card(c, member_id, *id)?;
            gone.push(k);
        }
    }
    Ok(gone)
}

/// Keeps a listing for another `days` days (0 = no end date), and puts an expired one back on sale.
pub fn renew(c: &Connection, member_id: i64, id: i64, days: i64) -> rusqlite::Result<Option<MemberCard>> {
    let until = if days <= 0 { 0 } else { now() + days.min(365) * 86400 };
    update_listing(c, member_id, id, &Patch { expires_at: Some(until), status: Some("Active".into()), ..Default::default() })
}

/// Listings past their end date come off sale. Returns how many just did (each owner is told).
pub fn expire_due(c: &Connection) -> rusqlite::Result<usize> {
    let t = now();
    let due: Vec<(i64, i64, String)> = c
        .prepare("SELECT id, member_id, title FROM member_cards WHERE status='Active' AND expires_at>0 AND expires_at<?1")?
        .query_map([t], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (id, owner, title) in &due {
        c.execute("UPDATE member_cards SET status='Expired', updated_at=?2 WHERE id=?1", rusqlite::params![id, t])?;
        log_event(c, *owner, *id, "status", "Active → Expired")?;
        notify(c, *owner, "expiry", &format!("{} has expired. Renew it to put it back on sale.", short(title)), &format!("/me/listings?id={id}"))?;
    }
    // Reservations nobody answered, or nobody paid for, free the cards again.
    let stale: Vec<i64> = c
        .prepare("SELECT id FROM reservations WHERE status IN ('Pending','Confirmed') AND expires_at>0 AND expires_at<?1")?
        .query_map([t], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in stale {
        c.execute("UPDATE reservations SET status='Expired', updated_at=?2 WHERE id=?1", rusqlite::params![id, t])?;
        if let Some(card) = c.query_row("SELECT card_id FROM reservations WHERE id=?1", [id], |r| r.get::<_, i64>(0)).optional()? {
            refresh_reserved(c, card)?;
        }
    }
    Ok(due.len())
}

/// Listings expiring within a week, oldest first.
pub fn expiring_soon(c: &Connection, member_id: i64) -> rusqlite::Result<Vec<MemberCard>> {
    let mut st = c.prepare(&format!("SELECT {} FROM member_cards c WHERE c.member_id=?1 AND ((c.status='Active' AND c.expires_at>0 AND c.expires_at<?2) OR c.status='Expired') ORDER BY c.expires_at, c.id LIMIT 20", members::CARD_COLS))?;
    let v = st.query_map(rusqlite::params![member_id, now() + 7 * 86400], |r| members::map_card(r, 0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

/// How a seller wants their listings page shown.
#[derive(Default, Clone)]
pub struct ListFilter {
    pub q: String,
    pub status: String,
    pub binder: Option<i64>,
    /// "newest", "price-desc", "price-asc", "title", "views", "stock", or the seller's own order.
    pub sort: String,
}

/// A seller's listings, filtered and sorted.
pub fn listings(c: &Connection, member_id: i64, f: &ListFilter) -> rusqlite::Result<Vec<MemberCard>> {
    let mut sql = format!("SELECT {} FROM member_cards c WHERE c.member_id=?1", members::CARD_COLS);
    let mut args: Vec<rusqlite::types::Value> = vec![member_id.into()];
    if !f.q.trim().is_empty() {
        sql += " AND (c.title LIKE ?2 OR c.card_set LIKE ?2 OR c.number LIKE ?2 OR c.notes LIKE ?2)";
        args.push(format!("%{}%", f.q.trim()).into());
    }
    if STATUSES.contains(&f.status.as_str()) {
        sql += &format!(" AND c.status=?{}", args.len() + 1);
        args.push(f.status.clone().into());
    }
    if let Some(b) = f.binder {
        sql += &format!(" AND c.binder_id=?{}", args.len() + 1);
        args.push(b.into());
    }
    sql += match f.sort.as_str() {
        "newest" => " ORDER BY c.id DESC",
        "price-desc" => " ORDER BY CASE WHEN c.value_own>0 THEN c.value_own ELSE c.value END DESC, c.id DESC",
        "price-asc" => " ORDER BY CASE WHEN c.value_own>0 THEN c.value_own ELSE c.value END ASC, c.id DESC",
        "title" => " ORDER BY c.title COLLATE NOCASE, c.id",
        "views" => " ORDER BY c.view_n DESC, c.id DESC",
        "stock" => " ORDER BY c.qty ASC, c.id DESC",
        _ => " ORDER BY c.position, c.id",
    };
    let mut st = c.prepare(&sql)?;
    let v = st.query_map(rusqlite::params_from_iter(args.iter()), |r| members::map_card(r, 0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

/// What a store shows the public: cards on sale (and reserved ones, marked), in the seller's order.
pub fn storefront_listings(c: &Connection, member_id: i64) -> rusqlite::Result<Vec<MemberCard>> {
    let mut st = c.prepare(&format!("SELECT {} FROM member_cards c WHERE c.member_id=?1 AND c.status IN ('Active','Reserved') ORDER BY c.position, c.id", members::CARD_COLS))?;
    let v = st.query_map([member_id], |r| members::map_card(r, 0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

fn short(title: &str) -> String {
    let t = title.split(" - ").next().unwrap_or(title).trim();
    if t.chars().count() > 40 { t.chars().take(39).collect::<String>() + "…" } else { t.to_string() }
}

pub fn log_event(c: &Connection, member_id: i64, card_id: i64, kind: &str, detail: &str) -> rusqlite::Result<()> {
    c.execute("INSERT INTO card_events(member_id, card_id, kind, detail, created_at) VALUES(?1,?2,?3,?4,?5)", rusqlite::params![member_id, card_id, kind, detail, now()])?;
    Ok(())
}

// ---------- orders: reservations, offers, questions ----------

/// A reservation or an offer, with the names and the card shown beside it.
#[derive(Clone, Debug, Serialize)]
pub struct Order {
    pub id: i64,
    /// "reservation" or "offer"
    pub kind: &'static str,
    pub buyer_id: i64,
    pub buyer_name: String,
    pub seller_id: i64,
    pub seller_name: String,
    pub seller_slug: String,
    pub card_id: i64,
    pub title: String,
    pub image: String,
    pub cond: String,
    pub qty: i64,
    /// Per card. For an offer: what the buyer offered.
    pub price: f64,
    /// For an offer the seller countered: the seller's price.
    pub counter: f64,
    pub status: String,
    pub message: String,
    pub paid_via: String,
    pub expires_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Order {
    pub fn total(&self) -> f64 {
        self.price * self.qty as f64
    }
    pub fn total_text(&self) -> String {
        vault_search::peso(self.total())
    }
    pub fn price_text(&self) -> String {
        vault_search::peso(self.price)
    }
    pub fn counter_text(&self) -> String {
        vault_search::peso(self.counter)
    }
    pub fn when(&self) -> String {
        ago(self.created_at)
    }
    pub fn is_open(&self) -> bool {
        matches!(self.status.as_str(), "Pending" | "Confirmed" | "Countered")
    }
    /// What the buyer should do or wait for, in plain words.
    pub fn buyer_line(&self) -> &'static str {
        match (self.kind, self.status.as_str()) {
            ("reservation", "Pending") => "Waiting for the seller to accept.",
            ("reservation", "Confirmed") => "Accepted. Pay the seller, then they'll mark it paid.",
            ("reservation", "Paid") => "Paid. Thank you!",
            ("reservation", "Declined") => "The seller declined.",
            ("reservation", "Cancelled") => "Cancelled.",
            ("reservation", "Expired") => "This reservation ran out.",
            ("offer", "Pending") => "Waiting for the seller.",
            ("offer", "Accepted") => "Accepted! It's now a reservation.",
            ("offer", "Countered") => "The seller made a counter offer.",
            ("offer", "Declined") => "The seller declined.",
            ("offer", "Withdrawn") => "You took it back.",
            _ => "",
        }
    }
}

const ORDER_RES: &str = "r.id, r.buyer_id, b.name, r.seller_id, s.name, s.slug, r.card_id, c.title, c.image, c.cond, r.qty, r.price, 0.0, r.status, r.message, r.paid_via, r.expires_at, r.created_at, r.updated_at
    FROM reservations r JOIN members b ON b.id=r.buyer_id JOIN members s ON s.id=r.seller_id LEFT JOIN member_cards c ON c.id=r.card_id";
const ORDER_OFF: &str = "o.id, o.sender_id, b.name, o.seller_id, s.name, s.slug, o.card_id, c.title, c.image, c.cond, o.qty, o.amount, o.counter, o.status, o.message, '', 0, o.created_at, o.updated_at
    FROM offers o JOIN members b ON b.id=o.sender_id JOIN members s ON s.id=o.seller_id LEFT JOIN member_cards c ON c.id=o.card_id";

fn map_order(kind: &'static str) -> impl Fn(&Row) -> rusqlite::Result<Order> {
    move |r| {
        Ok(Order {
            id: r.get(0)?,
            kind,
            buyer_id: r.get(1)?,
            buyer_name: r.get(2)?,
            seller_id: r.get(3)?,
            seller_name: r.get(4)?,
            seller_slug: r.get(5)?,
            card_id: r.get(6)?,
            title: r.get::<_, Option<String>>(7)?.unwrap_or_else(|| "A card that's gone".into()),
            image: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
            cond: r.get::<_, Option<String>>(9)?.unwrap_or_default(),
            qty: r.get(10)?,
            price: r.get(11)?,
            counter: r.get(12)?,
            status: r.get(13)?,
            message: r.get(14)?,
            paid_via: r.get(15)?,
            expires_at: r.get(16)?,
            created_at: r.get(17)?,
            updated_at: r.get(18)?,
        })
    }
}

pub fn reservation(c: &Connection, id: i64) -> rusqlite::Result<Option<Order>> {
    c.query_row(&format!("SELECT {ORDER_RES} WHERE r.id=?1"), [id], map_order("reservation")).optional()
}
pub fn offer(c: &Connection, id: i64) -> rusqlite::Result<Option<Order>> {
    c.query_row(&format!("SELECT {ORDER_OFF} WHERE o.id=?1"), [id], map_order("offer")).optional()
}

/// Copies of a listing spoken for by open reservations.
pub fn held(c: &Connection, card_id: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COALESCE(SUM(qty),0) FROM reservations WHERE card_id=?1 AND status IN ('Pending','Confirmed')", [card_id], |r| r.get(0))
}

/// A listing is Reserved while every copy is spoken for, and Active again when one frees up.
fn refresh_reserved(c: &Connection, card_id: i64) -> rusqlite::Result<()> {
    let Some((qty, status)) = c.query_row("SELECT qty, status FROM member_cards WHERE id=?1", [card_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).optional()? else { return Ok(()) };
    let h = held(c, card_id)?;
    let next = if qty <= 0 { "Sold" } else if h >= qty { "Reserved" } else { "Active" };
    if (status == "Active" || status == "Reserved" || status == "Sold") && status != next {
        c.execute("UPDATE member_cards SET status=?2, updated_at=?3 WHERE id=?1", rusqlite::params![card_id, next, now()])?;
    }
    Ok(())
}

/// Why a buyer can't reserve or make an offer right now.
fn can_buy(c: &Connection, buyer: i64, seller: i64, card_id: i64, qty: i64) -> rusqlite::Result<Result<MemberCard, &'static str>> {
    if buyer == seller {
        return Ok(Err("That's your own card."));
    }
    let Some(card) = members::card(c, seller, card_id)? else { return Ok(Err("That card isn't in this store.")) };
    let status: String = c.query_row("SELECT status FROM members WHERE id=?1", [seller], |r| r.get(0))?;
    if status == "Vacation" {
        return Ok(Err("This seller is on vacation. Please come back later."));
    }
    if card.status != "Active" && card.status != "Reserved" {
        return Ok(Err("That card isn't for sale right now."));
    }
    if qty < 1 || qty > 99 {
        return Ok(Err("How many? 1 to 99."));
    }
    if card.qty - held(c, card_id)? < qty {
        return Ok(Err("Not enough copies are left. Someone may have reserved it first."));
    }
    Ok(Ok(card))
}

/// A buyer reserves a card at the asking price. The seller is told. Err(text) says why not.
pub fn reserve(c: &Connection, buyer: i64, seller: i64, card_id: i64, qty: i64, message: &str) -> rusqlite::Result<Result<i64, &'static str>> {
    let card = match can_buy(c, buyer, seller, card_id, qty)? {
        Ok(k) => k,
        Err(e) => return Ok(Err(e)),
    };
    let open: i64 = c.query_row("SELECT COUNT(*) FROM reservations WHERE buyer_id=?1 AND card_id=?2 AND status IN ('Pending','Confirmed')", [buyer, card_id], |r| r.get(0))?;
    if open > 0 {
        return Ok(Err("You've already reserved this card. Check My orders."));
    }
    if card.worth() <= 0.0 {
        return Ok(Err("This card has no price yet. Ask the seller, or make an offer."));
    }
    reserve_at(c, buyer, seller, card_id, qty, card.worth(), message, "Pending", 0)
}

fn reserve_at(c: &Connection, buyer: i64, seller: i64, card_id: i64, qty: i64, price: f64, message: &str, status: &str, offer_id: i64) -> rusqlite::Result<Result<i64, &'static str>> {
    let t = now();
    c.execute(
        "INSERT INTO reservations(buyer_id, seller_id, card_id, status, expires_at, created_at, qty, message, price, updated_at, offer_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?6,?10)",
        rusqlite::params![buyer, seller, card_id, status, t + RESERVATION_DAYS * 86400, t, qty, members::clean_text(message, 300), price, offer_id],
    )?;
    let id = c.last_insert_rowid();
    refresh_reserved(c, card_id)?;
    let (buyer_name, title) = names(c, buyer, card_id)?;
    log_event(c, seller, card_id, "reserved", &format!("{buyer_name} ×{qty}"))?;
    if status == "Pending" {
        notify(c, seller, "reservation", &format!("{buyer_name} wants to buy {} ×{qty} for {}.", short(&title), vault_search::peso(price * qty as f64)), "/me/orders")?;
    }
    Ok(Ok(id))
}

fn names(c: &Connection, member: i64, card_id: i64) -> rusqlite::Result<(String, String)> {
    let name: String = c.query_row("SELECT name FROM members WHERE id=?1", [member], |r| r.get(0)).optional()?.unwrap_or_else(|| "Someone".into());
    let title: String = c.query_row("SELECT title FROM member_cards WHERE id=?1", [card_id], |r| r.get(0)).optional()?.unwrap_or_else(|| "a card".into());
    Ok((name, title))
}

/// The seller answers a reservation: "Confirmed" (go ahead and pay), "Paid" (money received: the sale is done), "Declined".
pub fn answer_reservation(c: &Connection, seller: i64, id: i64, status: &str, paid_via: &str) -> rusqlite::Result<Result<Order, &'static str>> {
    let Some(r) = reservation(c, id)?.filter(|r| r.seller_id == seller) else { return Ok(Err("That reservation isn't yours.")) };
    let ok = matches!((r.status.as_str(), status), ("Pending", "Confirmed") | ("Pending", "Declined") | ("Confirmed", "Paid") | ("Confirmed", "Declined") | ("Pending", "Paid"));
    if !ok {
        return Ok(Err("That reservation can't change like that any more."));
    }
    let t = now();
    let via = members::clean_line(paid_via, 40);
    c.execute("UPDATE reservations SET status=?2, updated_at=?3, paid_via=?4, expires_at=CASE WHEN ?2='Confirmed' THEN ?5 ELSE expires_at END WHERE id=?1", rusqlite::params![id, status, t, via, t + RESERVATION_DAYS * 86400])?;
    match status {
        "Paid" => {
            c.execute(
                "INSERT INTO transactions(buyer_id, seller_id, card_id, amount, status, created_at, qty, reservation_id, paid_via, title) VALUES(?1,?2,?3,?4,'Completed',?5,?6,?7,?8,?9)",
                rusqlite::params![r.buyer_id, seller, r.card_id, r.total(), t, r.qty, id, via, r.title],
            )?;
            c.execute("UPDATE member_cards SET qty=MAX(0, qty-?2), sold=sold+?2, updated_at=?3 WHERE id=?1", rusqlite::params![r.card_id, r.qty, t])?;
            log_event(c, seller, r.card_id, "sold", &format!("{} ×{} {}", r.buyer_name, r.qty, r.total_text()))?;
            notify(c, r.buyer_id, "sale", &format!("{} marked your {} as paid. Enjoy your card!", r.seller_name, short(&r.title)), "/me/orders?tab=buying")?;
        }
        "Confirmed" => notify(c, r.buyer_id, "reservation", &format!("{} accepted your reservation for {}. Time to pay.", r.seller_name, short(&r.title)), "/me/orders?tab=buying")?,
        _ => notify(c, r.buyer_id, "reservation", &format!("{} declined your reservation for {}.", r.seller_name, short(&r.title)), "/me/orders?tab=buying")?,
    }
    refresh_reserved(c, r.card_id)?;
    Ok(Ok(reservation(c, id)?.unwrap_or(r)))
}

/// The buyer changes their mind, before it's paid.
pub fn cancel_reservation(c: &Connection, buyer: i64, id: i64) -> rusqlite::Result<bool> {
    let Some(r) = reservation(c, id)?.filter(|r| r.buyer_id == buyer && matches!(r.status.as_str(), "Pending" | "Confirmed")) else { return Ok(false) };
    c.execute("UPDATE reservations SET status='Cancelled', updated_at=?2 WHERE id=?1", rusqlite::params![id, now()])?;
    refresh_reserved(c, r.card_id)?;
    notify(c, r.seller_id, "reservation", &format!("{} cancelled their reservation for {}.", r.buyer_name, short(&r.title)), "/me/orders")?;
    Ok(true)
}

/// A buyer names a price. Err(text) says why not.
pub fn make_offer(c: &Connection, buyer: i64, seller: i64, card_id: i64, amount: f64, qty: i64, message: &str) -> rusqlite::Result<Result<i64, &'static str>> {
    if let Err(e) = can_buy(c, buyer, seller, card_id, qty)? {
        return Ok(Err(e));
    }
    if !amount.is_finite() || !(1.0..=10_000_000.0).contains(&amount) {
        return Ok(Err("Type an amount in pesos."));
    }
    let open: i64 = c.query_row("SELECT COUNT(*) FROM offers WHERE sender_id=?1 AND card_id=?2 AND status IN ('Pending','Countered')", [buyer, card_id], |r| r.get(0))?;
    if open > 0 {
        return Ok(Err("You already have an offer on this card. Check My orders."));
    }
    let t = now();
    c.execute(
        "INSERT INTO offers(sender_id, seller_id, card_id, amount, status, created_at, qty, message, updated_at) VALUES(?1,?2,?3,?4,'Pending',?5,?6,?7,?5)",
        rusqlite::params![buyer, seller, card_id, amount.round(), t, qty, members::clean_text(message, 300)],
    )?;
    let id = c.last_insert_rowid();
    let (buyer_name, title) = names(c, buyer, card_id)?;
    notify(c, seller, "offer", &format!("{buyer_name} offers {} for {} ×{qty}.", vault_search::peso(amount), short(&title)), "/me/orders")?;
    Ok(Ok(id))
}

/// The seller answers an offer: "Accepted" (it becomes a confirmed reservation at that price), "Declined", or "Countered" with a price.
pub fn answer_offer(c: &Connection, seller: i64, id: i64, status: &str, counter: f64) -> rusqlite::Result<Result<Order, &'static str>> {
    let Some(o) = offer(c, id)?.filter(|o| o.seller_id == seller) else { return Ok(Err("That offer isn't yours.")) };
    if o.status != "Pending" {
        return Ok(Err("That offer was already answered."));
    }
    let t = now();
    match status {
        "Accepted" => {
            if let Err(e) = reserve_at(c, o.buyer_id, seller, o.card_id, o.qty, o.price, "", "Confirmed", id)? {
                return Ok(Err(e));
            }
            c.execute("UPDATE offers SET status='Accepted', updated_at=?2 WHERE id=?1", rusqlite::params![id, t])?;
            notify(c, o.buyer_id, "offer", &format!("{} accepted your offer of {} for {}. Time to pay.", o.seller_name, o.price_text(), short(&o.title)), "/me/orders?tab=buying")?;
        }
        "Countered" => {
            if !counter.is_finite() || !(1.0..=10_000_000.0).contains(&counter) {
                return Ok(Err("Type your counter price in pesos."));
            }
            c.execute("UPDATE offers SET status='Countered', counter=?2, updated_at=?3 WHERE id=?1", rusqlite::params![id, counter.round(), t])?;
            notify(c, o.buyer_id, "offer", &format!("{} counters with {} for {}.", o.seller_name, vault_search::peso(counter), short(&o.title)), "/me/orders?tab=buying")?;
        }
        "Declined" => {
            c.execute("UPDATE offers SET status='Declined', updated_at=?2 WHERE id=?1", rusqlite::params![id, t])?;
            notify(c, o.buyer_id, "offer", &format!("{} declined your offer for {}.", o.seller_name, short(&o.title)), "/me/orders?tab=buying")?;
        }
        _ => return Ok(Err("Accept, decline or counter.")),
    }
    Ok(Ok(offer(c, id)?.unwrap_or(o)))
}

/// The buyer takes a counter offer ("accept": it becomes a confirmed reservation at the seller's price) or drops the offer ("withdraw").
pub fn answer_counter(c: &Connection, buyer: i64, id: i64, accept: bool) -> rusqlite::Result<Result<(), &'static str>> {
    let Some(o) = offer(c, id)?.filter(|o| o.buyer_id == buyer) else { return Ok(Err("That offer isn't yours.")) };
    let t = now();
    if accept {
        if o.status != "Countered" {
            return Ok(Err("There's no counter offer to accept."));
        }
        if let Err(e) = reserve_at(c, buyer, o.seller_id, o.card_id, o.qty, o.counter, "", "Confirmed", id)? {
            return Ok(Err(e));
        }
        c.execute("UPDATE offers SET status='Accepted', amount=counter, updated_at=?2 WHERE id=?1", rusqlite::params![id, t])?;
        notify(c, o.seller_id, "offer", &format!("{} accepted your counter of {} for {}.", o.buyer_name, o.counter_text(), short(&o.title)), "/me/orders")?;
    } else {
        if !matches!(o.status.as_str(), "Pending" | "Countered") {
            return Ok(Err("That offer is already closed."));
        }
        c.execute("UPDATE offers SET status='Withdrawn', updated_at=?2 WHERE id=?1", rusqlite::params![id, t])?;
    }
    Ok(Ok(()))
}

/// A question from a buyer, with the seller's reply once there is one.
#[derive(Clone, Debug, Serialize)]
pub struct Inquiry {
    pub id: i64,
    pub sender_id: i64,
    pub sender_name: String,
    pub seller_id: i64,
    pub seller_name: String,
    pub card_id: Option<i64>,
    pub title: String,
    pub image: String,
    pub message: String,
    pub reply: String,
    pub replied_at: i64,
    pub created_at: i64,
}

impl Inquiry {
    pub fn when(&self) -> String {
        ago(self.created_at)
    }
    pub fn reply_when(&self) -> String {
        ago(self.replied_at)
    }
}

const INQ: &str = "i.id, i.sender_id, b.name, i.seller_id, s.name, i.card_id, c.title, c.image, i.message, i.reply, i.replied_at, i.created_at
    FROM inquiries i JOIN members b ON b.id=i.sender_id JOIN members s ON s.id=i.seller_id LEFT JOIN member_cards c ON c.id=i.card_id";

fn map_inq(r: &Row) -> rusqlite::Result<Inquiry> {
    Ok(Inquiry {
        id: r.get(0)?,
        sender_id: r.get(1)?,
        sender_name: r.get(2)?,
        seller_id: r.get(3)?,
        seller_name: r.get(4)?,
        card_id: r.get(5)?,
        title: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
        image: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
        message: r.get(8)?,
        reply: r.get(9)?,
        replied_at: r.get(10)?,
        created_at: r.get(11)?,
    })
}

pub fn ask(c: &Connection, sender: i64, seller: i64, card_id: Option<i64>, message: &str) -> rusqlite::Result<Result<i64, &'static str>> {
    if sender == seller {
        return Ok(Err("That's your own store."));
    }
    let msg = members::clean_text(message, 500);
    if msg.chars().count() < 2 {
        return Ok(Err("Type your question."));
    }
    if let Some(cid) = card_id {
        if members::card(c, seller, cid)?.is_none() {
            return Ok(Err("That card isn't in this store."));
        }
    }
    c.execute("INSERT INTO inquiries(sender_id, seller_id, card_id, message, created_at) VALUES(?1,?2,?3,?4,?5)", rusqlite::params![sender, seller, card_id, msg, now()])?;
    let id = c.last_insert_rowid();
    let (name, title) = names(c, sender, card_id.unwrap_or(0))?;
    let about = if card_id.is_some() { format!(" about {}", short(&title)) } else { String::new() };
    notify(c, seller, "inquiry", &format!("{name} asks{about}: “{}”", short(&msg)), "/me/orders?tab=questions")?;
    Ok(Ok(id))
}

pub fn reply(c: &Connection, seller: i64, id: i64, text: &str) -> rusqlite::Result<Result<(), &'static str>> {
    let Some(q) = c.query_row(&format!("SELECT {INQ} WHERE i.id=?1"), [id], map_inq).optional()?.filter(|q| q.seller_id == seller) else { return Ok(Err("That question isn't yours.")) };
    let t = members::clean_text(text, 500);
    if t.chars().count() < 1 {
        return Ok(Err("Type a reply."));
    }
    c.execute("UPDATE inquiries SET reply=?2, replied_at=?3 WHERE id=?1", rusqlite::params![id, t, now()])?;
    notify(c, q.sender_id, "inquiry", &format!("{} replied: “{}”", q.seller_name, short(&t)), "/me/orders?tab=buying")?;
    Ok(Ok(()))
}

/// Everything waiting on a seller, newest first, and everything a buyer has going.
pub struct Orders {
    pub reservations: Vec<Order>,
    pub offers: Vec<Order>,
    pub questions: Vec<Inquiry>,
}

pub fn orders_for_seller(c: &Connection, seller: i64) -> rusqlite::Result<Orders> {
    let reservations = c.prepare(&format!("SELECT {ORDER_RES} WHERE r.seller_id=?1 ORDER BY (r.status IN ('Pending','Confirmed')) DESC, r.created_at DESC LIMIT 200"))?.query_map([seller], map_order("reservation"))?.collect::<Result<_, _>>()?;
    let offers = c.prepare(&format!("SELECT {ORDER_OFF} WHERE o.seller_id=?1 ORDER BY (o.status IN ('Pending','Countered')) DESC, o.created_at DESC LIMIT 200"))?.query_map([seller], map_order("offer"))?.collect::<Result<_, _>>()?;
    let questions = c.prepare(&format!("SELECT {INQ} WHERE i.seller_id=?1 ORDER BY (i.reply='') DESC, i.created_at DESC LIMIT 200"))?.query_map([seller], map_inq)?.collect::<Result<_, _>>()?;
    Ok(Orders { reservations, offers, questions })
}

pub fn orders_for_buyer(c: &Connection, buyer: i64) -> rusqlite::Result<Orders> {
    let reservations = c.prepare(&format!("SELECT {ORDER_RES} WHERE r.buyer_id=?1 ORDER BY (r.status IN ('Pending','Confirmed')) DESC, r.created_at DESC LIMIT 200"))?.query_map([buyer], map_order("reservation"))?.collect::<Result<_, _>>()?;
    let offers = c.prepare(&format!("SELECT {ORDER_OFF} WHERE o.sender_id=?1 ORDER BY (o.status IN ('Pending','Countered')) DESC, o.created_at DESC LIMIT 200"))?.query_map([buyer], map_order("offer"))?.collect::<Result<_, _>>()?;
    let questions = c.prepare(&format!("SELECT {INQ} WHERE i.sender_id=?1 ORDER BY i.created_at DESC LIMIT 200"))?.query_map([buyer], map_inq)?.collect::<Result<_, _>>()?;
    Ok(Orders { reservations, offers, questions })
}

/// Counts for the seller's panel badges: (open reservations, open offers, unanswered questions).
pub fn pending(c: &Connection, seller: i64) -> rusqlite::Result<(i64, i64, i64)> {
    Ok((
        c.query_row("SELECT COUNT(*) FROM reservations WHERE seller_id=?1 AND status IN ('Pending','Confirmed')", [seller], |r| r.get(0))?,
        c.query_row("SELECT COUNT(*) FROM offers WHERE seller_id=?1 AND status='Pending'", [seller], |r| r.get(0))?,
        c.query_row("SELECT COUNT(*) FROM inquiries WHERE seller_id=?1 AND reply=''", [seller], |r| r.get(0))?,
    ))
}

/// The seller's pay details, for a buyer whose reservation the seller accepted. (Kept off the public page.)
pub fn pay_info_for(c: &Connection, buyer: i64, reservation_id: i64) -> rusqlite::Result<Option<(String, String)>> {
    c.query_row(
        "SELECT s.pay_methods, s.pay_info FROM reservations r JOIN members s ON s.id=r.seller_id WHERE r.id=?1 AND r.buyer_id=?2 AND r.status IN ('Confirmed','Paid')",
        [reservation_id, buyer],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

// ---------- sales ----------

#[derive(Clone, Debug, Serialize)]
pub struct Sale {
    pub id: i64,
    pub buyer_id: i64,
    pub buyer_name: String,
    pub seller_id: i64,
    pub seller_name: String,
    pub card_id: i64,
    pub title: String,
    pub image: String,
    pub qty: i64,
    pub amount: f64,
    pub paid_via: String,
    pub created_at: i64,
    pub reviewed: bool,
}

impl Sale {
    pub fn amount_text(&self) -> String {
        vault_search::peso(self.amount)
    }
    pub fn when(&self) -> String {
        ago(self.created_at)
    }
}

const SALE: &str = "t.id, t.buyer_id, b.name, t.seller_id, s.name, t.card_id, COALESCE(NULLIF(t.title,''), c.title, 'A card'), COALESCE(c.image,''), t.qty, t.amount, t.paid_via, t.created_at,
    EXISTS(SELECT 1 FROM reviews v WHERE v.transaction_id=t.id)
    FROM transactions t JOIN members b ON b.id=t.buyer_id JOIN members s ON s.id=t.seller_id LEFT JOIN member_cards c ON c.id=t.card_id";

fn map_sale(r: &Row) -> rusqlite::Result<Sale> {
    Ok(Sale {
        id: r.get(0)?,
        buyer_id: r.get(1)?,
        buyer_name: r.get(2)?,
        seller_id: r.get(3)?,
        seller_name: r.get(4)?,
        card_id: r.get(5)?,
        title: r.get(6)?,
        image: r.get(7)?,
        qty: r.get(8)?,
        amount: r.get(9)?,
        paid_via: r.get(10)?,
        created_at: r.get(11)?,
        reviewed: r.get::<_, i64>(12)? != 0,
    })
}

pub fn sales_of(c: &Connection, seller: i64, limit: i64) -> rusqlite::Result<Vec<Sale>> {
    let v = c.prepare(&format!("SELECT {SALE} WHERE t.seller_id=?1 AND t.status='Completed' ORDER BY t.created_at DESC LIMIT ?2"))?.query_map([seller, limit], map_sale)?.collect::<Result<_, _>>()?;
    Ok(v)
}

pub fn purchases_of(c: &Connection, buyer: i64, limit: i64) -> rusqlite::Result<Vec<Sale>> {
    let v = c.prepare(&format!("SELECT {SALE} WHERE t.buyer_id=?1 AND t.status='Completed' ORDER BY t.created_at DESC LIMIT ?2"))?.query_map([buyer, limit], map_sale)?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// (sales, pesos) this week, this month, and ever.
#[derive(Clone, Debug, Default, Serialize)]
pub struct SalesSummary {
    pub week_n: i64,
    pub week_php: f64,
    pub month_n: i64,
    pub month_php: f64,
    pub all_n: i64,
    pub all_php: f64,
    pub units: i64,
}

impl SalesSummary {
    pub fn week_text(&self) -> String {
        vault_search::peso(self.week_php)
    }
    pub fn month_text(&self) -> String {
        vault_search::peso(self.month_php)
    }
    pub fn all_text(&self) -> String {
        vault_search::peso(self.all_php)
    }
}

pub fn sales_summary(c: &Connection, seller: i64) -> rusqlite::Result<SalesSummary> {
    let t = now();
    let q = |since: i64| -> rusqlite::Result<(i64, f64)> {
        c.query_row("SELECT COUNT(*), COALESCE(SUM(amount),0) FROM transactions WHERE seller_id=?1 AND status='Completed' AND created_at>=?2", rusqlite::params![seller, since], |r| Ok((r.get(0)?, r.get(1)?)))
    };
    let (week_n, week_php) = q(t - 7 * 86400)?;
    let (month_n, month_php) = q(t - 30 * 86400)?;
    let (all_n, all_php) = q(0)?;
    let units: i64 = c.query_row("SELECT COALESCE(SUM(qty),0) FROM transactions WHERE seller_id=?1 AND status='Completed'", [seller], |r| r.get(0))?;
    Ok(SalesSummary { week_n, week_php, month_n, month_php, all_n, all_php, units })
}

// ---------- reviews and followers ----------

#[derive(Clone, Debug, Serialize)]
pub struct Review {
    pub id: i64,
    pub reviewer_id: i64,
    pub reviewer_name: String,
    pub reviewer_avatar: String,
    pub seller_id: i64,
    pub rating: i64,
    pub text: String,
    pub reply: String,
    pub hidden: bool,
    pub created_at: i64,
    pub title: String,
    pub seller_name: String,
}

impl Review {
    pub fn when(&self) -> String {
        ago(self.created_at)
    }
    pub fn initial(&self) -> String {
        members::initial(&self.reviewer_name)
    }
    pub fn stars(&self) -> String {
        "★".repeat(self.rating.clamp(0, 5) as usize) + &"☆".repeat((5 - self.rating.clamp(0, 5)) as usize)
    }
}

const REVIEW: &str = "v.id, v.reviewer_id, b.name, b.avatar, v.seller_id, v.rating, v.text, v.reply, v.hidden, v.created_at, COALESCE(NULLIF(t.title,''), c.title, ''), s.name
    FROM reviews v JOIN members b ON b.id=v.reviewer_id JOIN members s ON s.id=v.seller_id LEFT JOIN transactions t ON t.id=v.transaction_id LEFT JOIN member_cards c ON c.id=t.card_id";

fn map_review(r: &Row) -> rusqlite::Result<Review> {
    Ok(Review {
        id: r.get(0)?,
        reviewer_id: r.get(1)?,
        reviewer_name: r.get(2)?,
        reviewer_avatar: r.get(3)?,
        seller_id: r.get(4)?,
        rating: r.get(5)?,
        text: r.get(6)?,
        reply: r.get(7)?,
        hidden: r.get::<_, i64>(8)? != 0,
        created_at: r.get(9)?,
        title: r.get(10)?,
        seller_name: r.get(11)?,
    })
}

/// Only a buyer with a completed sale can review it, once. Sellers can't review themselves.
pub fn add_review(c: &Connection, reviewer: i64, transaction_id: i64, rating: i64, text: &str) -> rusqlite::Result<Result<i64, &'static str>> {
    let Some((seller, buyer)) = c.query_row("SELECT seller_id, buyer_id FROM transactions WHERE id=?1 AND status='Completed'", [transaction_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).optional()? else {
        return Ok(Err("There's no finished sale to review."));
    };
    if buyer != reviewer {
        return Ok(Err("Only the buyer can review this sale."));
    }
    if seller == reviewer {
        return Ok(Err("You can't review yourself."));
    }
    if !(1..=5).contains(&rating) {
        return Ok(Err("Pick 1 to 5 stars."));
    }
    let done: i64 = c.query_row("SELECT COUNT(*) FROM reviews WHERE transaction_id=?1", [transaction_id], |r| r.get(0))?;
    if done > 0 {
        return Ok(Err("You've already reviewed this sale."));
    }
    c.execute(
        "INSERT INTO reviews(reviewer_id, seller_id, transaction_id, rating, text, created_at) VALUES(?1,?2,?3,?4,?5,?6)",
        rusqlite::params![reviewer, seller, transaction_id, rating, members::clean_text(text, 600), now()],
    )?;
    let id = c.last_insert_rowid();
    let (name, _) = names(c, reviewer, 0)?;
    notify(c, seller, "review", &format!("{name} left you a {rating}-star review."), &format!("/store/{}#reviews", slug_of(c, seller)?))?;
    Ok(Ok(id))
}

pub fn reply_review(c: &Connection, seller: i64, id: i64, text: &str) -> rusqlite::Result<bool> {
    let n = c.execute("UPDATE reviews SET reply=?3 WHERE id=?1 AND seller_id=?2", rusqlite::params![id, seller, members::clean_text(text, 400)])?;
    Ok(n == 1)
}

pub fn hide_review(c: &Connection, id: i64, hidden: bool) -> rusqlite::Result<bool> {
    Ok(c.execute("UPDATE reviews SET hidden=?2 WHERE id=?1", rusqlite::params![id, hidden])? == 1)
}

pub fn reviews_of(c: &Connection, seller: i64, with_hidden: bool) -> rusqlite::Result<Vec<Review>> {
    let wh = if with_hidden { "" } else { " AND v.hidden=0" };
    let v = c.prepare(&format!("SELECT {REVIEW} WHERE v.seller_id=?1{wh} ORDER BY v.created_at DESC LIMIT 100"))?.query_map([seller], map_review)?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// (average stars, how many reviews). The average is rounded to one decimal.
pub fn rating_of(c: &Connection, seller: i64) -> rusqlite::Result<(f64, i64)> {
    let (avg, n): (Option<f64>, i64) = c.query_row("SELECT AVG(rating), COUNT(*) FROM reviews WHERE seller_id=?1 AND hidden=0", [seller], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(((avg.unwrap_or(0.0) * 10.0).round() / 10.0, n))
}

/// Follow a store, or stop. Returns whether they're following now.
pub fn toggle_follow(c: &Connection, follower: i64, seller: i64) -> rusqlite::Result<bool> {
    if follower == seller {
        return Ok(false);
    }
    let gone = c.execute("DELETE FROM store_follows WHERE follower_id=?1 AND seller_id=?2", [follower, seller])?;
    if gone > 0 {
        return Ok(false);
    }
    c.execute("INSERT INTO store_follows(follower_id, seller_id, created_at) VALUES(?1,?2,?3)", rusqlite::params![follower, seller, now()])?;
    let (name, _) = names(c, follower, 0)?;
    notify(c, seller, "follow", &format!("{name} now follows your store."), "/me/store")?;
    Ok(true)
}

pub fn is_following(c: &Connection, follower: i64, seller: i64) -> rusqlite::Result<bool> {
    let n: i64 = c.query_row("SELECT COUNT(*) FROM store_follows WHERE follower_id=?1 AND seller_id=?2", [follower, seller], |r| r.get(0))?;
    Ok(n > 0)
}

pub fn followers(c: &Connection, seller: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM store_follows WHERE seller_id=?1", [seller], |r| r.get(0))
}

/// Who follows this store (for telling them about new cards).
pub fn follower_ids(c: &Connection, seller: i64) -> rusqlite::Result<Vec<i64>> {
    let v = c.prepare("SELECT follower_id FROM store_follows WHERE seller_id=?1")?.query_map([seller], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(v)
}

fn slug_of(c: &Connection, member: i64) -> rusqlite::Result<String> {
    c.query_row("SELECT slug FROM members WHERE id=?1", [member], |r| r.get(0)).optional().map(|s| s.unwrap_or_default())
}

// ---------- views and saves ----------

/// Counts one look at a listing per viewer per day. The owner's own looks don't count. Returns true when it counted.
pub fn count_view(c: &Connection, card_id: i64, viewer_hash: &str, viewer: Option<i64>) -> rusqlite::Result<bool> {
    let Some(owner) = c.query_row("SELECT member_id FROM member_cards WHERE id=?1", [card_id], |r| r.get::<_, i64>(0)).optional()? else { return Ok(false) };
    if viewer == Some(owner) || viewer_hash.is_empty() {
        return Ok(false);
    }
    let t = now();
    let n = c.execute("INSERT OR IGNORE INTO listing_views(card_id, viewer_ip_hash, created_at, day) VALUES(?1,?2,?3,?4)", rusqlite::params![card_id, viewer_hash, t, t / 86400])?;
    if n == 1 {
        c.execute("UPDATE member_cards SET view_n=view_n+1 WHERE id=?1", [card_id])?;
        let wants: bool = c.query_row("SELECT notify_views FROM members WHERE id=?1", [owner], |r| r.get::<_, i64>(0).map(|v| v != 0))?;
        if wants {
            // One note a day per card, not one per look.
            let today: i64 = c.query_row("SELECT COUNT(*) FROM listing_views WHERE card_id=?1 AND day=?2", [card_id, t / 86400], |r| r.get(0))?;
            if today == 1 {
                let (_, title) = names(c, owner, card_id)?;
                notify(c, owner, "view", &format!("People are looking at {} today.", short(&title)), &format!("/me/listings?id={card_id}"))?;
            }
        }
    }
    Ok(n == 1)
}

/// Save a listing for later, or unsave it. Returns whether it's saved now.
pub fn toggle_save(c: &Connection, member: i64, card_id: i64) -> rusqlite::Result<bool> {
    let gone = c.execute("DELETE FROM listing_saves WHERE member_id=?1 AND card_id=?2", [member, card_id])?;
    if gone > 0 {
        c.execute("UPDATE member_cards SET save_n=MAX(0, save_n-1) WHERE id=?1", [card_id])?;
        return Ok(false);
    }
    let Some(owner) = c.query_row("SELECT member_id FROM member_cards WHERE id=?1", [card_id], |r| r.get::<_, i64>(0)).optional()? else { return Ok(false) };
    c.execute("INSERT INTO listing_saves(member_id, card_id, created_at) VALUES(?1,?2,?3)", rusqlite::params![member, card_id, now()])?;
    c.execute("UPDATE member_cards SET save_n=save_n+1 WHERE id=?1", [card_id])?;
    if owner != member {
        let wants: bool = c.query_row("SELECT notify_views FROM members WHERE id=?1", [owner], |r| r.get::<_, i64>(0).map(|v| v != 0))?;
        if wants {
            let (name, title) = names(c, member, card_id)?;
            notify(c, owner, "view", &format!("{name} saved {}.", short(&title)), &format!("/me/listings?id={card_id}"))?;
        }
    }
    Ok(true)
}

pub fn saved_ids(c: &Connection, member: i64) -> rusqlite::Result<Vec<i64>> {
    let v = c.prepare("SELECT card_id FROM listing_saves WHERE member_id=?1")?.query_map([member], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// Views per day for the chart: `days` entries, oldest first, as (day label, views, height in % of the busiest day).
pub fn views_by_day(c: &Connection, seller: i64, days: i64) -> rusqlite::Result<Vec<(String, i64, u32)>> {
    let today = now() / 86400;
    let rows: Vec<(i64, i64)> = c
        .prepare("SELECT v.day, COUNT(*) FROM listing_views v JOIN member_cards c ON c.id=v.card_id WHERE c.member_id=?1 AND v.day>?2 GROUP BY v.day")?
        .query_map([seller, today - days], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let top = rows.iter().map(|r| r.1).max().unwrap_or(0).max(1);
    Ok((0..days)
        .map(|i| {
            let d = today - days + 1 + i;
            let n = rows.iter().find(|r| r.0 == d).map_or(0, |r| r.1);
            (day_label(d), n, (n * 100 / top) as u32)
        })
        .collect())
}

fn day_label(day: i64) -> String {
    // "9 Nov": enough for a bar's label.
    let secs = day * 86400;
    let days = secs.div_euclid(86400);
    let (y, m, d) = civil(days);
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let _ = y;
    format!("{d} {}", M[(m - 1) as usize])
}

/// Days since 1970 to (year, month, day). Howard Hinnant's civil_from_days.
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn views_since(c: &Connection, seller: i64, days: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM listing_views v JOIN member_cards c ON c.id=v.card_id WHERE c.member_id=?1 AND v.day>?2", [seller, now() / 86400 - days], |r| r.get(0))
}

pub fn most_viewed(c: &Connection, seller: i64, limit: i64) -> rusqlite::Result<Vec<MemberCard>> {
    let v = c.prepare(&format!("SELECT {} FROM member_cards c WHERE c.member_id=?1 AND c.view_n>0 ORDER BY c.view_n DESC, c.save_n DESC, c.id LIMIT ?2", members::CARD_COLS))?.query_map([seller, limit], |r| members::map_card(r, 0))?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// Binders by how often their cards are looked at: (binder, views).
pub fn popular_binders(c: &Connection, seller: i64) -> rusqlite::Result<Vec<(MemberBinder, i64)>> {
    let mut out: Vec<(MemberBinder, i64)> = members::binders_of(c, seller)?
        .into_iter()
        .map(|b| {
            let n: i64 = c.query_row("SELECT COALESCE(SUM(view_n),0) FROM member_cards WHERE member_id=?1 AND binder_id=?2", [seller, b.id], |r| r.get(0)).unwrap_or(0);
            (b, n)
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out.truncate(5);
    Ok(out)
}

// ---------- notifications ----------

/// Kinds of notification a seller can switch on or off, with what each means.
pub const EVENTS: [(&str, &str); 9] = [
    ("follow", "Someone follows my store"),
    ("view", "Someone looks at or saves a card (summarised, once a day per card)"),
    ("inquiry", "Someone asks a question"),
    ("offer", "Someone makes an offer"),
    ("reservation", "Someone reserves a card"),
    ("sale", "A sale is done"),
    ("expiry", "A listing expires"),
    ("status", "A card's status changes"),
    ("announcement", "News from the vault"),
];

#[derive(Clone, Debug, Serialize)]
pub struct Pref {
    pub event: String,
    pub label: String,
    pub site: bool,
    pub email: bool,
    pub push: bool,
}

pub fn prefs_of(c: &Connection, member: i64) -> rusqlite::Result<Vec<Pref>> {
    let mut out = vec![];
    for (event, label) in EVENTS {
        let saved: Option<(i64, i64, i64)> = c.query_row("SELECT site, email, push FROM notify_prefs WHERE member_id=?1 AND event=?2", rusqlite::params![member, event], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
        let (site, email, push) = saved.unwrap_or((1, 0, 0));
        out.push(Pref { event: event.into(), label: label.into(), site: site != 0, email: email != 0, push: push != 0 });
    }
    Ok(out)
}

pub fn set_pref(c: &Connection, member: i64, event: &str, site: bool, email: bool, push: bool) -> rusqlite::Result<bool> {
    if !EVENTS.iter().any(|(e, _)| *e == event) {
        return Ok(false);
    }
    c.execute(
        "INSERT INTO notify_prefs(member_id, event, site, email, push) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(member_id, event) DO UPDATE SET site=?3, email=?4, push=?5",
        rusqlite::params![member, event, site, email, push],
    )?;
    Ok(true)
}

/// Tells a member something, if they want to hear it on the site. Email and push are queued for the mailer (see `outbox`).
pub fn notify(c: &Connection, member: i64, kind: &str, message: &str, link: &str) -> rusqlite::Result<()> {
    let (site, email): (i64, i64) = c
        .query_row("SELECT site, email FROM notify_prefs WHERE member_id=?1 AND event=?2", rusqlite::params![member, kind], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .unwrap_or((1, 0));
    if site == 0 && email == 0 {
        return Ok(());
    }
    // A note hidden from the site but wanted by email still goes into the list, marked read, so the mailer can pick it up.
    c.execute(
        "INSERT INTO notifications(member_id, kind, message, link, is_read, created_at) VALUES(?1,?2,?3,?4,?5,?6)",
        rusqlite::params![member, kind, message, link, site == 0, now()],
    )?;
    if email != 0 {
        c.execute("INSERT INTO outbox(notification_id, created_at) VALUES(?1,?2)", rusqlite::params![c.last_insert_rowid(), now()]).ok();
    }
    Ok(())
}

/// Mail waiting to go out: (notification id, email address, kind, message, link). The mailer deletes what it sent.
pub fn outbox(c: &Connection, limit: i64) -> rusqlite::Result<Vec<(i64, String, String, String, String)>> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS outbox(notification_id INTEGER PRIMARY KEY, created_at INTEGER NOT NULL, tries INTEGER NOT NULL DEFAULT 0)")?;
    let v = c
        .prepare("SELECT n.id, m.email, n.kind, n.message, n.link FROM outbox o JOIN notifications n ON n.id=o.notification_id JOIN members m ON m.id=n.member_id WHERE o.tries<5 ORDER BY o.created_at LIMIT ?1")?
        .query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<_, _>>()?;
    Ok(v)
}

pub fn outbox_done(c: &Connection, id: i64, sent: bool) -> rusqlite::Result<()> {
    if sent {
        c.execute("DELETE FROM outbox WHERE notification_id=?1", [id])?;
    } else {
        c.execute("UPDATE outbox SET tries=tries+1 WHERE notification_id=?1", [id])?;
    }
    Ok(())
}

/// A page of someone's notifications, newest first: (notes, how many in all).
pub fn notifications_page(c: &Connection, member: i64, page: i64, per: i64) -> rusqlite::Result<(Vec<members::Notification>, i64)> {
    let total: i64 = c.query_row("SELECT COUNT(*) FROM notifications WHERE member_id=?1", [member], |r| r.get(0))?;
    let v = c
        .prepare("SELECT id, member_id, kind, message, link, is_read, created_at FROM notifications WHERE member_id=?1 ORDER BY created_at DESC, id DESC LIMIT ?2 OFFSET ?3")?
        .query_map(rusqlite::params![member, per, page.max(0) * per], |r| {
            Ok(members::Notification { id: r.get(0)?, member_id: r.get(1)?, kind: r.get(2)?, message: r.get(3)?, link: r.get(4)?, is_read: r.get::<_, i64>(5)? != 0, created_at: r.get(6)? })
        })?
        .collect::<Result<_, _>>()?;
    Ok((v, total))
}

/// The owner tells every seller something. Returns how many were told.
pub fn announce(c: &Connection, message: &str, link: &str) -> rusqlite::Result<usize> {
    let msg = members::clean_text(message, 500);
    if msg.is_empty() {
        return Ok(0);
    }
    let ids: Vec<i64> = c.prepare("SELECT id FROM members WHERE blocked=0")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for id in &ids {
        notify(c, *id, "announcement", &msg, &members::clean_line(link, 200))?;
    }
    Ok(ids.len())
}

pub fn push_subscribe(c: &Connection, member: i64, endpoint: &str, p256dh: &str, auth: &str) -> rusqlite::Result<()> {
    if !endpoint.starts_with("https://") || endpoint.len() > 1000 || p256dh.len() > 200 || auth.len() > 100 {
        return Ok(());
    }
    c.execute("DELETE FROM push_subscriptions WHERE member_id=?1 AND endpoint=?2", rusqlite::params![member, endpoint])?;
    c.execute("INSERT INTO push_subscriptions(member_id, endpoint, p256dh, auth) VALUES(?1,?2,?3,?4)", rusqlite::params![member, endpoint, p256dh, auth])?;
    Ok(())
}

pub fn push_unsubscribe(c: &Connection, member: i64, endpoint: &str) -> rusqlite::Result<()> {
    c.execute("DELETE FROM push_subscriptions WHERE member_id=?1 AND endpoint=?2", rusqlite::params![member, endpoint])?;
    Ok(())
}

// ---------- the dashboard ----------

/// Something that happened in the store lately, for the "Recent activity" list.
#[derive(Clone, Debug, Serialize)]
pub struct Activity {
    pub kind: String,
    pub text: String,
    pub link: String,
    pub created_at: i64,
}

impl Activity {
    pub fn when(&self) -> String {
        ago(self.created_at)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Dashboard {
    pub listings: i64,
    pub active: i64,
    pub reserved: i64,
    pub sold_units: i64,
    pub binders: i64,
    /// Asking prices of everything on sale, times stock, in pesos.
    pub value: f64,
    pub views7: i64,
    pub views30: i64,
    pub followers: i64,
    pub rating: f64,
    pub reviews: i64,
    pub pending_reservations: i64,
    pub pending_offers: i64,
    pub open_questions: i64,
    pub unread: i64,
    pub sales: SalesSummary,
    pub chart: Vec<(String, i64, u32)>,
    pub top_cards: Vec<MemberCard>,
    pub top_binders: Vec<(MemberBinder, i64)>,
    pub expiring: Vec<MemberCard>,
    pub recent: Vec<Activity>,
    pub recent_sales: Vec<Sale>,
}

impl Dashboard {
    pub fn value_text(&self) -> String {
        vault_search::peso(self.value)
    }
    pub fn todo(&self) -> i64 {
        self.pending_reservations + self.pending_offers + self.open_questions + self.expiring.len() as i64
    }
}

pub fn dashboard(c: &Connection, seller: i64) -> rusqlite::Result<Dashboard> {
    let count = |status: &str| -> rusqlite::Result<i64> { c.query_row("SELECT COUNT(*) FROM member_cards WHERE member_id=?1 AND status=?2", rusqlite::params![seller, status], |r| r.get(0)) };
    let listings: i64 = c.query_row("SELECT COUNT(*) FROM member_cards WHERE member_id=?1", [seller], |r| r.get(0))?;
    let active = count("Active")?;
    let reserved = count("Reserved")?;
    let sold_units: i64 = c.query_row("SELECT COALESCE(SUM(sold),0) FROM member_cards WHERE member_id=?1", [seller], |r| r.get(0))?;
    let binders: i64 = c.query_row("SELECT COUNT(*) FROM member_binders WHERE member_id=?1", [seller], |r| r.get(0))?;
    let value: f64 = storefront_listings(c, seller)?.iter().map(|k| k.worth() * k.qty.max(0) as f64).sum();
    let (pending_reservations, pending_offers, open_questions) = pending(c, seller)?;
    let (rating, reviews) = rating_of(c, seller)?;
    let mut recent: Vec<Activity> = c
        .prepare("SELECT e.kind, e.detail, c.title, e.card_id, e.created_at FROM card_events e LEFT JOIN member_cards c ON c.id=e.card_id WHERE e.member_id=?1 ORDER BY e.created_at DESC, e.id DESC LIMIT 12")?
        .query_map([seller], |r| {
            let (kind, detail, title, card_id, at): (String, String, Option<String>, i64, i64) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?);
            let title = title.map(|t| short(&t)).unwrap_or_else(|| "A card".into());
            let text = match kind.as_str() {
                "listed" => format!("Listed {title}"),
                "stock" => format!("Stock of {title}: {detail}"),
                "status" => format!("{title}: {detail}"),
                "reserved" => format!("{title} reserved by {detail}"),
                "sold" => format!("Sold {title} to {detail}"),
                _ => format!("{title} {detail}"),
            };
            Ok(Activity { kind, text, link: format!("/me/listings?id={card_id}"), created_at: at })
        })?
        .collect::<Result<_, _>>()?;
    for n in members::notifications(c, seller, false)?.into_iter().filter(|n| matches!(n.kind.as_str(), "follow" | "inquiry" | "offer" | "review")).take(6) {
        recent.push(Activity { kind: n.kind, text: n.message, link: n.link, created_at: n.created_at });
    }
    recent.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    recent.truncate(10);
    Ok(Dashboard {
        listings,
        active,
        reserved,
        sold_units,
        binders,
        value,
        views7: views_since(c, seller, 7)?,
        views30: views_since(c, seller, 30)?,
        followers: followers(c, seller)?,
        rating,
        reviews,
        pending_reservations,
        pending_offers,
        open_questions,
        unread: members::unread_notification_count(c, seller)?,
        sales: sales_summary(c, seller)?,
        chart: views_by_day(c, seller, 30)?,
        top_cards: most_viewed(c, seller, 5)?,
        top_binders: popular_binders(c, seller)?,
        expiring: expiring_soon(c, seller)?,
        recent,
        recent_sales: sales_of(c, seller, 5)?,
    })
}

/// "just now", "5 min ago", "3 h ago", "2 d ago", or the date.
pub fn ago(t: i64) -> String {
    let s = (now() - t).max(0);
    if s < 60 {
        "just now".into()
    } else if s < 3600 {
        format!("{} min ago", s / 60)
    } else if s < 86400 {
        format!("{} h ago", s / 3600)
    } else if s < 7 * 86400 {
        format!("{} d ago", s / 86400)
    } else {
        day_label(t / 86400)
    }
}

/// What a store's public page shows about its seller, worked out from real data.
#[derive(Clone, Debug, Serialize)]
pub struct StoreStats {
    pub on_sale: i64,
    pub sold: i64,
    pub sales: i64,
    pub followers: i64,
    pub rating: f64,
    pub reviews: i64,
    pub since: String,
}

pub fn store_stats(c: &Connection, seller: i64) -> rusqlite::Result<StoreStats> {
    let on_sale: i64 = c.query_row("SELECT COUNT(*) FROM member_cards WHERE member_id=?1 AND status IN ('Active','Reserved')", [seller], |r| r.get(0))?;
    let sold: i64 = c.query_row("SELECT COALESCE(SUM(qty),0) FROM transactions WHERE seller_id=?1 AND status='Completed'", [seller], |r| r.get(0))?;
    let sales: i64 = c.query_row("SELECT COUNT(*) FROM transactions WHERE seller_id=?1 AND status='Completed'", [seller], |r| r.get(0))?;
    let created: i64 = c.query_row("SELECT created_at FROM members WHERE id=?1", [seller], |r| r.get(0))?;
    let (y, m, _) = civil(created / 86400);
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (rating, reviews) = rating_of(c, seller)?;
    Ok(StoreStats { on_sale, sold, sales, followers: followers(c, seller)?, rating, reviews, since: format!("{} {y}", M[(m - 1) as usize]) })
}

/// Sellers with a live page and cards on sale, best rated and busiest first.
pub fn directory(c: &Connection) -> rusqlite::Result<Vec<(members::Member, StoreStats, Vec<MemberCard>)>> {
    let mut out = vec![];
    for m in members::approved_sellers(c)? {
        let stats = store_stats(c, m.id)?;
        if stats.on_sale == 0 {
            continue;
        }
        let cards = storefront_listings(c, m.id)?.into_iter().take(6).collect();
        out.push((m, stats, cards));
    }
    out.sort_by(|a, b| (b.1.sales, b.1.on_sale).cmp(&(a.1.sales, a.1.on_sale)));
    Ok(out)
}

// ---------- admin ----------

/// Custom cards (own photos) a seller asked the vault to look at.
pub fn flagged_cards(c: &Connection) -> rusqlite::Result<Vec<(MemberCard, String)>> {
    let v = c
        .prepare(&format!("SELECT {}, m.name FROM member_cards c JOIN members m ON m.id=c.member_id WHERE c.review_flag=1 ORDER BY c.updated_at DESC LIMIT 100", members::CARD_COLS))?
        .query_map([], |r| Ok((members::map_card(r, 0)?, r.get(members::CARD_N)?)))?
        .collect::<Result<_, _>>()?;
    Ok(v)
}

pub fn clear_flag(c: &Connection, card_id: i64) -> rusqlite::Result<()> {
    c.execute("UPDATE member_cards SET review_flag=0 WHERE id=?1", [card_id])?;
    Ok(())
}

pub fn flag_card(c: &Connection, member: i64, card_id: i64, on: bool) -> rusqlite::Result<bool> {
    Ok(c.execute("UPDATE member_cards SET review_flag=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member, card_id, on])? == 1)
}

pub fn all_reviews(c: &Connection, limit: i64) -> rusqlite::Result<Vec<Review>> {
    let v = c.prepare(&format!("SELECT {REVIEW} ORDER BY v.created_at DESC LIMIT ?1"))?.query_map([limit], map_review)?.collect::<Result<_, _>>()?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::members::{create, Added};

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(crate::db::SCHEMA).unwrap();
        c.execute_batch(crate::blocks::SCHEMA).unwrap();
        c.execute_batch(members::SCHEMA).unwrap();
        members::migrate(&c).unwrap();
        c
    }

    fn seller_with_card(c: &Connection) -> (i64, i64, i64) {
        let s = create(c, "Shop", "h").unwrap().unwrap();
        let b = create(c, "Buyer", "h").unwrap().unwrap();
        let Added::Yes(card) = add_listing(c, s, "tcgdex:base1-58", "Pikachu - 058/102 - Base Set", "58/102", "Base Set", "", "").unwrap() else { panic!() };
        members::set_card_value(c, "tcgdex:base1-58", 100.0, now()).unwrap();
        update_listing(c, s, card, &Patch { qty: Some(2), ..Default::default() }).unwrap();
        (s, b, card)
    }

    #[test]
    fn store_addresses_are_checked() {
        assert_eq!(check_slug("Big-Boys"), Ok("big-boys".into()));
        assert!(check_slug("ab").is_err());
        assert!(check_slug("admin").is_err());
        assert!(check_slug("bad--name").is_err());
        assert!(check_slug("-lead").is_err());
        assert!(check_slug("has space").is_err());
        let c = db();
        let a = create(&c, "Ash", "h").unwrap().unwrap();
        let b = create(&c, "Brock", "h").unwrap().unwrap();
        assert_eq!(set_slug(&c, a, "pallet-town").unwrap(), Ok(true));
        assert_eq!(set_slug(&c, b, "Pallet-Town").unwrap(), Ok(false), "taken whatever the case");
        assert!(set_slug(&c, b, "me").unwrap().is_err());
    }

    #[test]
    fn the_same_card_can_be_listed_twice_and_prices_still_refresh() {
        let c = db();
        let s = create(&c, "Shop", "h").unwrap().unwrap();
        let Added::Yes(a) = add_listing(&c, s, "tcgdex:x-1", "Mew", "1", "Set", "", "").unwrap() else { panic!() };
        let Added::Yes(b) = add_listing(&c, s, "tcgdex:x-1", "Mew", "1", "Set", "", "").unwrap() else { panic!() };
        assert_ne!(a, b);
        let second = members::card(&c, s, b).unwrap().unwrap();
        assert_eq!(second.card_id, "tcgdex:x-1#2");
        assert_eq!(catalogue_key(&second.card_id), "tcgdex:x-1");
        members::set_card_value(&c, "tcgdex:x-1", 50.0, now()).unwrap();
        assert_eq!(members::card(&c, s, b).unwrap().unwrap().value, 50.0, "the second listing follows the market too");
    }

    #[test]
    fn stock_status_and_expiry() {
        let c = db();
        let (s, _, card) = seller_with_card(&c);
        let k = update_listing(&c, s, card, &Patch { qty: Some(0), ..Default::default() }).unwrap().unwrap();
        assert_eq!(k.status, "Sold", "no stock means sold out");
        let k = update_listing(&c, s, card, &Patch { qty: Some(3), ..Default::default() }).unwrap().unwrap();
        assert_eq!(k.status, "Active", "restocked means back on sale");
        update_listing(&c, s, card, &Patch { expires_at: Some(now() - 10), ..Default::default() }).unwrap();
        assert_eq!(expire_due(&c).unwrap(), 1);
        assert_eq!(members::card(&c, s, card).unwrap().unwrap().status, "Expired");
        let k = renew(&c, s, card, 30).unwrap().unwrap();
        assert_eq!(k.status, "Active");
        assert!(k.expires_at > now() + 29 * 86400);
        assert!(update_listing(&c, 999, card, &Patch { qty: Some(1), ..Default::default() }).unwrap().is_none(), "not their card");
        assert_eq!(bulk_reprice(&c, s, &[card], 10.0).unwrap(), 1);
        assert_eq!(members::card(&c, s, card).unwrap().unwrap().value_own, 110.0);
    }

    #[test]
    fn a_sale_from_reservation_to_review() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        assert!(reserve(&c, s, s, card, 1, "").unwrap().is_err(), "not your own card");
        assert!(reserve(&c, b, s, card, 5, "").unwrap().is_err(), "not that many");
        let r = reserve(&c, b, s, card, 2, "Meet at SM?").unwrap().unwrap();
        assert_eq!(members::card(&c, s, card).unwrap().unwrap().status, "Reserved", "every copy is spoken for");
        assert!(reserve(&c, b, s, card, 1, "").unwrap().is_err(), "already reserved");
        assert_eq!(members::unread_notification_count(&c, s).unwrap(), 1, "the seller is told");
        assert!(pay_info_for(&c, b, r).unwrap().is_none(), "pay details wait for the seller");
        assert!(answer_reservation(&c, b, r, "Confirmed", "").unwrap().is_err(), "only the seller answers");
        let o = answer_reservation(&c, s, r, "Confirmed", "").unwrap().unwrap();
        assert_eq!(o.status, "Confirmed");
        assert!(pay_info_for(&c, b, r).unwrap().is_some());
        let o = answer_reservation(&c, s, r, "Paid", "GCash").unwrap().unwrap();
        assert_eq!(o.status, "Paid");
        let k = members::card(&c, s, card).unwrap().unwrap();
        assert_eq!((k.qty, k.sold, k.status.as_str()), (0, 2, "Sold"));
        let sales = sales_of(&c, s, 10).unwrap();
        assert_eq!(sales.len(), 1);
        assert_eq!(sales[0].amount, 200.0);
        assert!(add_review(&c, s, sales[0].id, 5, "me").unwrap().is_err(), "sellers can't review themselves");
        assert!(add_review(&c, b, sales[0].id, 5, "Fast and legit!").unwrap().is_ok());
        assert!(add_review(&c, b, sales[0].id, 4, "again").unwrap().is_err(), "one review per sale");
        assert_eq!(rating_of(&c, s).unwrap(), (5.0, 1));
        let d = dashboard(&c, s).unwrap();
        assert_eq!((d.sold_units, d.sales.all_n, d.sales.all_php), (2, 1, 200.0));
        assert!(d.recent.iter().any(|a| a.kind == "sold"));
    }

    #[test]
    fn offers_counter_and_vacation() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        let o = make_offer(&c, b, s, card, 80.0, 1, "").unwrap().unwrap();
        assert!(make_offer(&c, b, s, card, 90.0, 1, "").unwrap().is_err(), "one open offer per card");
        answer_offer(&c, s, o, "Countered", 90.0).unwrap().unwrap();
        assert!(answer_counter(&c, b, o, true).unwrap().is_ok());
        let orders = orders_for_buyer(&c, b).unwrap();
        assert_eq!(orders.reservations.len(), 1);
        assert_eq!((orders.reservations[0].status.as_str(), orders.reservations[0].price), ("Confirmed", 90.0));
        assert_eq!(orders.offers[0].status, "Accepted");
        c.execute("UPDATE members SET status='Vacation' WHERE id=?1", [s]).unwrap();
        assert!(make_offer(&c, b, s, card, 80.0, 1, "").unwrap().is_err(), "closed for vacation");
        assert!(cancel_reservation(&c, b, orders.reservations[0].id).unwrap());
        assert_eq!(members::card(&c, s, card).unwrap().unwrap().status, "Active", "the copy is free again");
    }

    #[test]
    fn views_count_once_a_day_and_never_the_owner() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        assert!(count_view(&c, card, "abc", Some(b)).unwrap());
        assert!(!count_view(&c, card, "abc", Some(b)).unwrap(), "same viewer, same day");
        assert!(count_view(&c, card, "xyz", None).unwrap());
        assert!(!count_view(&c, card, "own", Some(s)).unwrap(), "the owner doesn't count");
        assert_eq!(members::card(&c, s, card).unwrap().unwrap().view_n, 2);
        assert_eq!(views_since(&c, s, 7).unwrap(), 2);
        let chart = views_by_day(&c, s, 7).unwrap();
        assert_eq!(chart.len(), 7);
        assert_eq!(chart[6].1, 2);
        assert!(toggle_save(&c, b, card).unwrap());
        assert!(!toggle_save(&c, b, card).unwrap());
        assert_eq!(most_viewed(&c, s, 5).unwrap().len(), 1);
    }

    #[test]
    fn notifications_follow_preferences() {
        let c = db();
        let (s, b, _) = seller_with_card(&c);
        assert!(toggle_follow(&c, b, s).unwrap());
        assert_eq!(members::unread_notification_count(&c, s).unwrap(), 1);
        assert!(set_pref(&c, s, "follow", false, false, false).unwrap());
        assert!(!toggle_follow(&c, b, s).unwrap());
        assert!(toggle_follow(&c, b, s).unwrap());
        assert_eq!(members::unread_notification_count(&c, s).unwrap(), 1, "switched off: no new note");
        assert_eq!(followers(&c, s).unwrap(), 1);
        assert!(!toggle_follow(&c, s, s).unwrap(), "can't follow yourself");
        assert_eq!(announce(&c, "Hello sellers", "/").unwrap(), 2);
        let (page, total) = notifications_page(&c, s, 0, 10).unwrap();
        assert_eq!(total, 2);
        assert_eq!(page[0].kind, "announcement");
        assert_eq!(prefs_of(&c, s).unwrap().len(), EVENTS.len());
    }

    #[test]
    fn link_lines_keep_labels_and_drop_bad_links() {
        let cleaned = clean_link_lines("Shopee | https://shopee.ph/bbv\nhttps://carousell.ph/u/bbv\njavascript:alert(1)\nFB | http://not-https.com");
        assert_eq!(cleaned, "Shopee | https://shopee.ph/bbv\nhttps://carousell.ph/u/bbv");
        assert_eq!(link_parts("https://carousell.ph/u/bbv"), ("Carousell".into(), "https://carousell.ph/u/bbv".into()));
        assert_eq!(link_parts("Shopee | https://shopee.ph/bbv").0, "Shopee");
    }
}
