//! Every trainer can run a store. This is the store's database side: listings with stock and prices, orders
//! (reservations and offers), buyers' questions, reviews, followers, views and notifications, and the numbers
//! on the seller's dashboard. The pages are in the binary's `store.rs`.
//!
//! Buying stays informal: a buyer reserves a card or makes an offer, the seller accepts and shows how to pay
//! (GCash, Maya, bank), the buyer pays in their own app, and the seller marks it paid. Then the stock goes down.
use crate::members::{self, now, MemberBinder, MemberCard};
use rusqlite::{Connection, OptionalExtension, Row};
use serde::Serialize;

/// Listing statuses, in the order a listing moves through them. "Out of stock" is the seller's own switch.
pub const STATUSES: [&str; 6] = ["Active", "Out of stock", "Reserved", "Sold", "Hidden", "Expired"];
/// What a card in a binder is: shown off, for sale, or wanted.
pub const KINDS: [&str; 3] = ["own", "sale", "wish"];
/// Where a vault order stands.
pub const ORDER_STATUSES: [&str; 5] = ["Pending", "Confirmed", "Paid", "Declined", "Cancelled"];
/// Ways to pay a seller can tick.
pub const PAY_METHODS: [&str; 6] = ["GCash", "Maya", "Bank transfer", "GoTyme", "Cash on meetup", "Other"];
/// Reservations the seller hasn't answered expire after this long. Confirmed ones wait this long for payment.
pub const RESERVATION_DAYS: i64 = 3;
/// The vault's own store: /store/vault, listed first among the stores. No trainer can take this address.
pub const VAULT_SLUG: &str = "vault";

/// Names a store address can't take, because the site uses them (or they would pass for the vault's own store).
const RESERVED_SLUGS: &[&str] = &[
    "admin", "api", "me", "store", "stores", "seller", "sellers", "trainer", "trainers", "join", "signin", "signout", "static", "uploads",
    "all", "chat", "ask", "vouch", "vouches", "collections", "products", "cards", "search", "robots.txt", "sitemap.xml", "about", "help",
    "support", "login", "logout", "new", "edit", "delete", "settings", "bigboysvault", VAULT_SLUG, "the-vault", "big-boys-vault",
];

/// A store address the site keeps for itself, whatever the case.
pub fn reserved_slug(s: &str) -> bool {
    let t = s.trim().to_lowercase();
    RESERVED_SLUGS.contains(&t.as_str())
}

/// Adds what the store needs to an existing database. Only ever adds (see docs/HANDOFF.md §5).
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
        ("pay_public", "INTEGER NOT NULL DEFAULT 1"),
        ("seller", "INTEGER NOT NULL DEFAULT 0"),
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
        ("kind", "TEXT NOT NULL DEFAULT 'own'"),
    ] {
        add_column(c, "member_cards", col, def)?;
    }
    // Once: cards sellers already had on their shelves stay for sale.
    c.execute_batch("CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)")?;
    if crate::blocks::setting(c, "kinds_v")?.as_deref() != Some("1") {
        c.execute("UPDATE member_cards SET kind='sale' WHERE member_id IN (SELECT id FROM members WHERE seller=1)", [])?;
        crate::blocks::set_setting(c, "kinds_v", "1")?;
    }
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS vault_orders(
           id INTEGER PRIMARY KEY AUTOINCREMENT, buyer_id INTEGER NOT NULL, status TEXT NOT NULL DEFAULT 'Pending', message TEXT NOT NULL DEFAULT '',
           paid_via TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE IF NOT EXISTS vault_order_items(
           id INTEGER PRIMARY KEY AUTOINCREMENT, order_id INTEGER NOT NULL, product_id INTEGER NOT NULL, title TEXT NOT NULL DEFAULT '',
           price REAL NOT NULL DEFAULT 0, qty INTEGER NOT NULL DEFAULT 1, image TEXT NOT NULL DEFAULT '', handle TEXT NOT NULL DEFAULT '');
         CREATE INDEX IF NOT EXISTS idx_vorders_buyer ON vault_orders(buyer_id, created_at);
         CREATE INDEX IF NOT EXISTS idx_vorders_status ON vault_orders(status, created_at);
         CREATE INDEX IF NOT EXISTS idx_vitems_order ON vault_order_items(order_id);",
    )?;
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
         CREATE TABLE IF NOT EXISTS push_outbox(
           id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, kind TEXT NOT NULL, message TEXT NOT NULL, link TEXT NOT NULL, created_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS card_events(
           id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, card_id INTEGER NOT NULL, kind TEXT NOT NULL, detail TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS store_features(
           store_id INTEGER NOT NULL, kind TEXT NOT NULL, ref TEXT NOT NULL, position INTEGER NOT NULL, PRIMARY KEY(store_id, kind, ref));
         CREATE TABLE IF NOT EXISTS seller_applications(
           id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, note TEXT NOT NULL DEFAULT '', photos TEXT NOT NULL DEFAULT '',
           status TEXT NOT NULL DEFAULT 'Waiting', thread_id INTEGER NOT NULL DEFAULT 0, reason TEXT NOT NULL DEFAULT '',
           decided_by INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL, decided_at INTEGER NOT NULL DEFAULT 0);
         CREATE INDEX IF NOT EXISTS idx_apply_member ON seller_applications(member_id, id);
         CREATE TABLE IF NOT EXISTS bookmarks(
           member_id INTEGER NOT NULL, kind TEXT NOT NULL, ref INTEGER NOT NULL, created_at INTEGER NOT NULL, PRIMARY KEY(member_id, kind, ref));
         CREATE INDEX IF NOT EXISTS idx_apply_status ON seller_applications(status, id);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_apply_one_waiting ON seller_applications(member_id) WHERE status='Waiting';
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
    add_column(c, "push_subscriptions", "fails", "INTEGER NOT NULL DEFAULT 0")?;
    message_push_on_once(c)?;
    for (col, def) in [("qty", "INTEGER NOT NULL DEFAULT 1"), ("message", "TEXT NOT NULL DEFAULT ''"), ("price", "REAL NOT NULL DEFAULT 0"), ("paid_via", "TEXT NOT NULL DEFAULT ''"), ("updated_at", "INTEGER NOT NULL DEFAULT 0"), ("offer_id", "INTEGER NOT NULL DEFAULT 0"), ("batch", "INTEGER NOT NULL DEFAULT 0"), ("buyer_paid_at", "INTEGER NOT NULL DEFAULT 0")] {
        add_column(c, "reservations", col, def)?;
    }
    add_column(c, "vault_orders", "buyer_paid_at", "INTEGER NOT NULL DEFAULT 0")?;
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
    // A line in the bell: who did it (their picture shows beside it; 0 is the site) and what it is about (lines about
    // the same thing stay one unread line, brought up to date).
    add_column(c, "notifications", "actor_id", "INTEGER NOT NULL DEFAULT 0")?;
    add_column(c, "notifications", "ref", "TEXT NOT NULL DEFAULT ''")?;
    c.execute_batch("CREATE INDEX IF NOT EXISTS idx_notif_ref ON notifications(member_id, ref, is_read)")?;
    views_switch_once(c)?;
    Ok(())
}

/// Once: "Tell me when people look at or save my cards" lived in Store settings and the notifications page both, and
/// needed both. It is the "view" switch alone now, keeping each seller's choice: off for whoever had the box off.
fn views_switch_once(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)")?;
    let done: i64 = c.query_row("SELECT COUNT(*) FROM settings WHERE key='views_switch_v'", [], |r| r.get(0))?;
    if done == 0 {
        c.execute(
            "INSERT INTO notify_prefs(member_id, event, site, email, push) SELECT id, 'view', 0, 0, 0 FROM members WHERE notify_views=0
             ON CONFLICT(member_id, event) DO UPDATE SET site=0, email=0, push=0",
            [],
        )?;
        c.execute("INSERT INTO settings(key, value) VALUES('views_switch_v', '1')", [])?;
    }
    Ok(())
}

/// Once: message alerts are on by default. Before alerts could be sent at all, no saved "push" choice was a real one,
/// so every saved message preference starts with push on too.
fn message_push_on_once(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)")?;
    let done: i64 = c.query_row("SELECT COUNT(*) FROM settings WHERE key='message_push_v'", [], |r| r.get(0))?;
    if done == 0 {
        c.execute("UPDATE notify_prefs SET push=1 WHERE event='message'", [])?;
        c.execute("INSERT INTO settings(key, value) VALUES('message_push_v', '1')", [])?;
    }
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
        return Err("Use only letters, numbers and single dashes, like pallet-town-cards.");
    }
    if reserved_slug(&t) {
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

/// How a store looks beyond its pictures and colour (Store settings › Store design; the owner, 19 Nov 2026: "an
/// option for everyone to improve further the design of their stores to make their stores standout according to their
/// own tastes and effort"). The same choices for a seller's store and the vault's. Kept as JSON.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoreLook {
    /// The page behind the store: "" plain, "glow", "dots" or "lines" (drawn in the store's colour), or "photo".
    #[serde(default)]
    pub bg: String,
    /// The store's own background photo. It is kept while another background is chosen, so it can come back.
    #[serde(default)]
    pub photo: String,
    /// The store's name: "" plain, "glow", or "line" (underlined), both in the store's colour.
    #[serde(default)]
    pub name: String,
    /// The cards on the store: "" standard, "l" large, "s" small.
    #[serde(default)]
    pub tile: String,
    /// One line at the top of the store ("Free shipping this week").
    #[serde(default)]
    pub note: String,
}

pub const LOOK_BGS: [(&str, &str); 5] = [("", "Plain"), ("glow", "Glow"), ("dots", "Dots"), ("lines", "Lines"), ("photo", "Your Photo")];
pub const LOOK_NAMES: [(&str, &str); 3] = [("", "Plain"), ("glow", "Glow"), ("line", "Underline")];
pub const LOOK_TILES: [(&str, &str); 3] = [("", "Standard"), ("l", "Large"), ("s", "Small")];

impl StoreLook {
    /// From what was saved. Nothing, or something unreadable, is the plain look.
    pub fn parse(saved: &str) -> Self {
        serde_json::from_str::<Self>(saved).unwrap_or_default().clean()
    }
    /// Only the choices there are, the note as one clean line, and a photo only from the site's own uploads.
    pub fn clean(mut self) -> Self {
        let pick = |v: &str, all: &[(&str, &str)]| if all.iter().any(|(k, _)| *k == v) { v.to_string() } else { String::new() };
        self.bg = pick(&self.bg, &LOOK_BGS);
        self.name = pick(&self.name, &LOOK_NAMES);
        self.tile = pick(&self.tile, &LOOK_TILES);
        self.note = members::clean_line(&self.note, 140);
        let safe = self.photo.starts_with("/uploads/") && self.photo.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'));
        if !safe {
            self.photo.clear();
        }
        if self.bg == "photo" && self.photo.is_empty() {
            self.bg.clear();
        }
        self
    }
    pub fn json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    /// The classes the store's page carries (" lk-bg-glow lk-name-line lk-tile-l"); nothing for the plain look.
    pub fn classes(&self) -> String {
        let mut s = String::new();
        for (pre, v) in [("lk-bg-", &self.bg), ("lk-name-", &self.name), ("lk-tile-", &self.tile)] {
            if !v.is_empty() {
                s.push(' ');
                s.push_str(pre);
                s.push_str(v);
            }
        }
        s
    }
    pub fn has_note(&self) -> bool {
        !self.note.is_empty()
    }
    /// The background is the store's own photo.
    pub fn has_photo(&self) -> bool {
        self.bg == "photo" && !self.photo.is_empty()
    }
}

/// The Store design fields of a settings form (`look_sent`, `look_bg`, `look_name`, `look_tile`, `look_note`), for a
/// seller's Store settings and the vault's alike.
#[derive(Default)]
pub struct LookForm {
    pub sent: bool,
    bg: Option<String>,
    name: Option<String>,
    tile: Option<String>,
    note: Option<String>,
}

impl LookForm {
    /// Takes one form field. False: it isn't one of the look's.
    pub fn take(&mut self, key: &str, v: &str) -> bool {
        match key {
            "look_sent" => self.sent = true,
            "look_bg" => self.bg = Some(v.to_string()),
            "look_name" => self.name = Some(v.to_string()),
            "look_tile" => self.tile = Some(v.to_string()),
            "look_note" => self.note = Some(v.to_string()),
            _ => return false,
        }
        true
    }
    /// The form asks for the store's own photo behind it.
    pub fn wants_photo(&self) -> bool {
        self.bg.as_deref() == Some("photo")
    }
    /// The look to save: what was saved before, with what the form changed and a new background photo if one came.
    pub fn over(&self, was: &StoreLook, new_photo: Option<&str>) -> StoreLook {
        StoreLook {
            bg: self.bg.clone().unwrap_or_else(|| was.bg.clone()),
            photo: new_photo.map(str::to_string).unwrap_or_else(|| was.photo.clone()),
            name: self.name.clone().unwrap_or_else(|| was.name.clone()),
            tile: self.tile.clone().unwrap_or_else(|| was.tile.clone()),
            note: self.note.clone().unwrap_or_else(|| was.note.clone()),
        }
        .clean()
    }
}

/// Saves a seller's store look.
pub fn set_look(c: &Connection, member_id: i64, look: &StoreLook) -> rusqlite::Result<()> {
    c.execute("UPDATE members SET store_look=?2 WHERE id=?1", rusqlite::params![member_id, look.clone().clean().json()])?;
    Ok(())
}

/// The vault's own store look (the `vault_look` setting), and saving it.
pub fn vault_look(c: &Connection) -> rusqlite::Result<StoreLook> {
    Ok(StoreLook::parse(&crate::blocks::setting(c, "vault_look")?.unwrap_or_default()))
}

pub fn set_vault_look(c: &Connection, look: &StoreLook) -> rusqlite::Result<()> {
    crate::blocks::set_setting(c, "vault_look", &look.clone().clean().json())
}

/// The seller's store details, saved from the Store settings page.
#[derive(Default)]
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
    pub pay_public: bool,
    /// The store's own "about" (its owner's page has its own).
    pub store_bio: String,
}

/// Saves a store's settings. Nothing here waits for approval: a store that is open stays open whatever changes.
pub fn save_store(c: &Connection, member_id: i64, s: &StoreSettings) -> rusqlite::Result<()> {
    let status = if ["Active", "Away", "Vacation"].contains(&s.status.as_str()) { s.status.as_str() } else { "Active" };
    let methods: Vec<&str> = s.pay_methods.split(',').map(str::trim).filter(|m| PAY_METHODS.contains(m)).collect();
    c.execute(
        "UPDATE members SET store_name=?2, location=?3, contact_info=?4, contact_public=?5, status=?6, fb_url=?7, fb_group_url=?8, ig_url=?9, tiktok_url=?10,
           shopee_url=?11, links=?12, pay_methods=?13, pay_info=?14, email=?15, notify_views=?16, updated_at=?17, pay_public=?18, store_bio=?19
         WHERE id=?1",
        rusqlite::params![
            member_id, members::clean_line(&s.store_name, 40), members::clean_line(&s.location, 80), members::clean_text(&s.contact_info, 300), s.contact_public, status,
            members::clean_https_url(&s.fb_url), members::clean_https_url(&s.fb_group_url), members::clean_https_url(&s.ig_url), members::clean_https_url(&s.tiktok_url),
            members::clean_https_url(&s.shopee_url), clean_link_lines(&s.links), methods.join(","), members::clean_text(&s.pay_info, 600),
            clean_email(&s.email), s.notify_views, now(), s.pay_public, members::clean_text(&s.store_bio, 300)
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
    // The next copy gets the next free number: after a copy was deleted, counting alone could hit one still there
    // (seen 18 Nov 2026: "That card is already in your store" for a third copy).
    let taken = |id: &str| -> rusqlite::Result<bool> { c.query_row("SELECT COUNT(*) FROM member_cards WHERE member_id=?1 AND card_id=?2", rusqlite::params![member_id, id], |r| r.get::<_, i64>(0)).map(|n| n > 0) };
    let mut id = if n == 0 { key.clone() } else { format!("{key}#{}", n + 1) };
    let mut next = n + 1;
    while taken(&id)? {
        next += 1;
        id = format!("{key}#{next}");
    }
    let added = members::add_card(c, member_id, &id, title, number, set, image, image_large)?;
    if let members::Added::Yes(cid) = added {
        let selling: i64 = c.query_row("SELECT seller FROM members WHERE id=?1", [member_id], |r| r.get(0)).unwrap_or(0);
        c.execute("UPDATE member_cards SET updated_at=?3, kind=?4 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, cid, now(), if selling == 1 { "sale" } else { "own" }])?;
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
        // Stock gone: sold out. Stock back: on sale, or Reserved while open reservations hold every copy.
        // "Out of stock", Hidden and Expired are the seller's own choice (or the clock's): the count never moves them.
        refresh_reserved(c, id)?;
        if q != before.qty {
            log_event(c, member_id, id, "stock", &format!("{} → {q}", before.qty))?;
        }
    }
    if let Some(s) = &p.status {
        if STATUSES.contains(&s.as_str()) {
            let set_status = |st: &str| c.execute("UPDATE member_cards SET status=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, st]);
            let set_qty = |from: i64, to: i64| -> rusqlite::Result<()> {
                c.execute("UPDATE member_cards SET qty=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, to])?;
                log_event(c, member_id, id, "stock", &format!("{from} → {to}"))
            };
            let q: i64 = c.query_row("SELECT qty FROM member_cards WHERE member_id=?1 AND id=?2", [member_id, id], |r| r.get(0))?;
            match s.as_str() {
                // Back in stock means the seller has at least one copy. It stays Reserved while every copy is held.
                "Active" => {
                    set_status("Active")?;
                    if before.expires_at > 0 && before.expires_at < now() {
                        c.execute("UPDATE member_cards SET expires_at=0 WHERE member_id=?1 AND id=?2", [member_id, id])?;
                    }
                    if q < 1 {
                        set_qty(q, 1)?;
                    }
                    refresh_reserved(c, id)?;
                }
                // Sold means none left, so the count says so too (and a later count change can bring it back).
                "Sold" => {
                    if q != 0 {
                        set_qty(q, 0)?;
                    }
                    set_status("Sold")?;
                }
                // Only real reservations make a card Reserved: work it out from them, never just take the word.
                "Reserved" => {
                    if matches!(before.status.as_str(), "Out of stock" | "Hidden" | "Expired") {
                        set_status("Active")?;
                    }
                    refresh_reserved(c, id)?;
                }
                other => {
                    set_status(other)?;
                }
            }
            let after: String = c.query_row("SELECT status FROM member_cards WHERE member_id=?1 AND id=?2", [member_id, id], |r| r.get(0))?;
            if after != before.status {
                log_event(c, member_id, id, "status", &format!("{} → {after}", before.status))?;
                if after != "Out of stock" && after != "Active" {
                    notify(c, member_id, "status", &format!("{} is now {}.", short(&before.title), after), &format!("/me/listings?id={id}"))?;
                }
            }
        }
    }
    if let Some(f) = &p.finish {
        // A new finish has its own price: the listing is looked up again first thing (value_at 0) and keeps its value meanwhile.
        c.execute(
            "UPDATE member_cards SET finish=?3, value_at=CASE WHEN finish<>?3 THEN 0 ELSE value_at END WHERE member_id=?1 AND id=?2",
            rusqlite::params![member_id, id, members::clean_line(f, 40)],
        )?;
    }
    if let Some(n) = &p.notes {
        c.execute("UPDATE member_cards SET notes=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, members::clean_text(n, 500)])?;
    }
    if let Some(b) = p.binder_id {
        members::move_card_to_binder(c, member_id, id, b)?;
    }
    if let Some(e) = p.expires_at {
        let e = e.max(0);
        c.execute("UPDATE member_cards SET expires_at=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, id, e])?;
        // A new end date (or none) puts an expired listing back on sale. Then stock decides: Active, Reserved or Sold.
        let status: String = c.query_row("SELECT status FROM member_cards WHERE member_id=?1 AND id=?2", [member_id, id], |r| r.get(0))?;
        if (e == 0 || e > now()) && status == "Expired" {
            c.execute("UPDATE member_cards SET status='Active' WHERE member_id=?1 AND id=?2", [member_id, id])?;
            refresh_reserved(c, id)?;
            let next: String = c.query_row("SELECT status FROM member_cards WHERE member_id=?1 AND id=?2", [member_id, id], |r| r.get(0))?;
            log_event(c, member_id, id, "status", &format!("Expired → {next}"))?;
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

/// What a card is: "own" (a highlight), "sale" (in the store) or "wish" (wanted). Only a seller can put a card up for sale.
pub fn set_kind(c: &Connection, member_id: i64, card_id: i64, kind: &str) -> rusqlite::Result<Result<MemberCard, &'static str>> {
    if !KINDS.contains(&kind) {
        return Ok(Err("Pick Highlight, For sale or Wishlist."));
    }
    if kind == "sale" {
        let selling: i64 = c.query_row("SELECT seller FROM members WHERE id=?1", [member_id], |r| r.get(0))?;
        if selling == 0 {
            return Ok(Err("Selling isn't switched on for your trainer. Ask the vault owner."));
        }
    }
    let Some(before) = members::card(c, member_id, card_id)? else { return Ok(Err("That card isn't in your binder.")) };
    if before.kind == kind {
        // Already that kind: nothing changes (a bulk "For sale" mustn't touch cards already on sale).
        return Ok(Ok(before));
    }
    let t = now();
    c.execute("UPDATE member_cards SET kind=?3, updated_at=?4 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, card_id, kind, t])?;
    if kind == "sale" {
        // A card put up for sale is live, with at least one copy. The seller's own "Out of stock" and Hidden stay as they are.
        c.execute(
            "UPDATE member_cards SET status='Active', qty=MAX(qty, 1), expires_at=CASE WHEN expires_at>0 AND expires_at<?3 THEN 0 ELSE expires_at END
             WHERE member_id=?1 AND id=?2 AND status IN ('Active','Reserved','Sold','Expired')",
            rusqlite::params![member_id, card_id, t],
        )?;
        refresh_reserved(c, card_id)?;
    } else if before.is_sale() {
        // Off sale: open reservations and offers on it close, and the buyers are told.
        close_open_orders(c, member_id, card_id)?;
    }
    Ok(members::card(c, member_id, card_id)?.ok_or("That card isn't in your binder."))
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
            // Close its orders while the card's name can still be read for the buyers' notes. (Running it twice tells nobody twice.)
            close_open_orders(c, member_id, *id)?;
            members::remove_card(c, member_id, *id)?;
            gone.push(k);
        }
    }
    Ok(gone)
}

/// Keeps a listing for another `days` days (0 = no end date). Only the end date changes, except that an expired
/// listing goes back on sale (then stock decides: Active, Reserved or Sold).
pub fn renew(c: &Connection, member_id: i64, id: i64, days: i64) -> rusqlite::Result<Option<MemberCard>> {
    let until = if days <= 0 { 0 } else { now() + days.min(365) * 86400 };
    update_listing(c, member_id, id, &Patch { expires_at: Some(until), ..Default::default() })
}

/// Listings past their end date come off sale, and reservations nobody answered or paid for run out.
/// Returns how many listings and reservations just changed (each person is told once).
/// Many pages run this at the same moment, so every change checks again that it's still due, and only
/// the run that really made the change tells anyone. That way nobody hears the same news twice.
pub fn expire_due(c: &Connection) -> rusqlite::Result<usize> {
    let t = now();
    let mut changed = 0;
    let due: Vec<(i64, i64, String)> = c
        .prepare("SELECT id, member_id, title FROM member_cards WHERE status='Active' AND expires_at>0 AND expires_at<?1")?
        .query_map([t], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (id, owner, title) in &due {
        let n = c.execute("UPDATE member_cards SET status='Expired', updated_at=?2 WHERE id=?1 AND status='Active' AND expires_at>0 AND expires_at<?2", rusqlite::params![id, t])?;
        if n == 1 {
            changed += 1;
            log_event(c, *owner, *id, "status", "Active → Expired")?;
            notify(c, *owner, "expiry", &format!("{} has expired. Renew it to put it back on sale.", short(title)), &format!("/me/listings?id={id}"))?;
        }
    }
    // Reservations nobody answered, or nobody paid for, free the cards again. Both sides are told.
    let stale: Vec<(i64, i64, i64, i64)> = c
        .prepare("SELECT id, buyer_id, seller_id, card_id FROM reservations WHERE status IN ('Pending','Confirmed') AND expires_at>0 AND expires_at<?1")?
        .query_map([t], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    for (id, buyer, seller, card) in stale {
        let n = c.execute("UPDATE reservations SET status='Expired', updated_at=?2 WHERE id=?1 AND status IN ('Pending','Confirmed') AND expires_at>0 AND expires_at<?2", rusqlite::params![id, t])?;
        if n == 1 {
            changed += 1;
            refresh_reserved(c, card)?;
            let (_, title) = names(c, buyer, card)?;
            notify(c, buyer, "my_order", &format!("Your order for {} ran out after {RESERVATION_DAYS} days. If you already paid, message the seller. If not, you can order it again.", short(&title)), "/me/orders?tab=buying")?;
            notify(c, seller, "reservation", &format!("An order for {} ran out, so the copy is on sale again. If the buyer already paid you, message them.", short(&title)), "/me/orders")?;
        }
    }
    // Vault orders the vault never answered let their copies go, and the buyer is told.
    let lapsed: Vec<(i64, i64)> = c
        .prepare("SELECT id, buyer_id FROM vault_orders WHERE status='Pending' AND created_at<?1")?
        .query_map([t - VAULT_ORDER_DAYS * 86400], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (id, buyer) in lapsed {
        let n = c.execute("UPDATE vault_orders SET status='Cancelled', updated_at=?2 WHERE id=?1 AND status='Pending'", rusqlite::params![id, t])?;
        if n == 1 {
            notify(c, buyer, "my_order", &format!("Your vault order waited {VAULT_ORDER_DAYS} days without an answer, so it was closed. You can order again."), "/me/orders?tab=buying")?;
        }
    }
    Ok(changed)
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
    /// "newest", "price-desc", "price-asc", "title", "views", "stock", the table's own columns ("price:desc,card:asc"),
    /// or the seller's own order.
    pub sort: String,
    /// "own", "sale", "wish", or "" for all.
    pub kind: String,
}

/// The names of the columns a seller's listings table can be sorted by.
pub const LISTING_SORT_COLUMNS: [&str; 8] = ["card", "status", "price", "stock", "kind", "cond", "binder", "note"];

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
    if KINDS.contains(&f.kind.as_str()) {
        sql += &format!(" AND c.kind=?{}", args.len() + 1);
        args.push(f.kind.clone().into());
    }
    // The table's own columns ("price:desc,card:asc") come before the named orders below.
    let cond = crate::db::COND_RANK.replace("{col}", "c.cond");
    let columns: [(&str, &str); 8] = [
        ("card", "c.title COLLATE NOCASE {dir}"),
        ("status", "CASE c.status WHEN 'Active' THEN 1 WHEN 'Reserved' THEN 2 WHEN 'Out of stock' THEN 3 WHEN 'Hidden' THEN 4 WHEN 'Sold' THEN 5 ELSE 6 END {dir}"),
        ("price", "CASE WHEN c.value_own>0 THEN c.value_own ELSE c.value END {dir}"),
        ("stock", "c.qty {dir}"),
        ("kind", "CASE c.kind WHEN 'sale' THEN 1 WHEN 'own' THEN 2 ELSE 3 END {dir}"),
        ("cond", &cond),
        ("binder", "(c.binder_id IS NULL) ASC, (SELECT b.title FROM member_binders b WHERE b.id=c.binder_id) COLLATE NOCASE {dir}"),
        ("note", "(c.notes='') ASC, c.notes COLLATE NOCASE {dir}"),
    ];
    if let Some(order) = crate::db::column_sort(&f.sort, &columns) {
        sql += &format!(" ORDER BY {order}, c.id");
        let mut st = c.prepare(&sql)?;
        let v = st.query_map(rusqlite::params_from_iter(args.iter()), |r| members::map_card(r, 0))?.collect::<Result<Vec<_>, _>>()?;
        return Ok(v);
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
    let mut st = c.prepare(&format!("SELECT {} FROM member_cards c WHERE c.member_id=?1 AND c.kind='sale' AND c.status IN ('Active','Out of stock','Reserved') ORDER BY c.position, c.id", members::CARD_COLS))?;
    let v = st.query_map([member_id], |r| members::map_card(r, 0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

/// What a store shows on its tile on the Stores page, as its owner chose: binders and highlighted cards, each in order.
/// Store 0 is the vault (binders by handle, cards by product id); a seller's binders and cards go by their numbers.
/// Nothing chosen yet (`featured` gives `None`): the tile shows every binder and no highlighted cards.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Featured {
    pub binders: Vec<String>,
    pub cards: Vec<String>,
}

/// At most this many of each on a tile (the tile scrolls sideways; more would only bury the rest).
pub const FEATURE_MAX: usize = 12;

pub fn featured(c: &Connection, store: i64) -> rusqlite::Result<Option<Featured>> {
    let mut st = c.prepare_cached("SELECT kind, ref FROM store_features WHERE store_id=?1 ORDER BY position, ref")?;
    let mut f = Featured::default();
    let mut saved = false;
    for row in st.query_map([store], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (kind, key) = row?;
        match kind.as_str() {
            "binder" => f.binders.push(key),
            "card" => f.cards.push(key),
            _ => saved = true,
        }
    }
    Ok((saved || !f.binders.is_empty() || !f.cards.is_empty()).then_some(f))
}

/// Saves a store's choice, replacing the last one: the order given, each once, at most FEATURE_MAX of each. Even an empty
/// choice is kept (a store can show no binders); the caller checks that every one belongs to the store.
pub fn set_featured(c: &Connection, store: i64, f: &Featured) -> rusqlite::Result<()> {
    c.execute_batch("BEGIN IMMEDIATE")?;
    let r = (|| {
        c.execute("DELETE FROM store_features WHERE store_id=?1", [store])?;
        c.execute("INSERT INTO store_features(store_id, kind, ref, position) VALUES(?1, 'saved', '', 0)", [store])?;
        let mut st = c.prepare_cached("INSERT OR IGNORE INTO store_features(store_id, kind, ref, position) VALUES(?1, ?2, ?3, ?4)")?;
        for (kind, keys) in [("binder", &f.binders), ("card", &f.cards)] {
            let mut seen = std::collections::HashSet::new();
            let keep: Vec<&str> = keys.iter().map(|k| k.trim()).filter(|k| !k.is_empty() && seen.insert(k.to_string())).take(FEATURE_MAX).collect();
            for (i, k) in keep.iter().enumerate() {
                st.execute(rusqlite::params![store, kind, k, i as i64 + 1])?;
            }
        }
        Ok(())
    })();
    c.execute_batch(if r.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
    r
}

/// What makes a seller's card buyable right now, the same for The Vault page and Ask the Vault: for sale and Active, in
/// stock, not past its end date (`?1` is the time now), in a store that is approved, not blocked, selling, not on
/// vacation and not hidden by its seller (the same checks as `can_sell`). Copies an open reservation holds are not for
/// sale: `LEFT` counts the rest.
const OPEN_FOR_SALE: &str = "c.kind='sale' AND c.status='Active' AND c.qty>0 AND (c.expires_at<=0 OR c.expires_at>=?1)
           AND m.seller=1 AND m.profile_ok=1 AND m.blocked=0 AND m.status<>'Vacation' AND m.store_hidden=0";
/// A listing's copies that no open reservation holds.
const LEFT: &str = "c.qty - COALESCE((SELECT SUM(r.qty) FROM reservations r WHERE r.card_id=c.id AND r.status IN ('Pending','Confirmed')),0)";

/// The open listings, with their store and copies left (`open_row` reads a row), and `more` after the checks.
fn open_sql(more: &str) -> String {
    format!("SELECT {}, m.slug, m.store_name, m.name, COALESCE(NULLIF(m.store_avatar,''), m.avatar), {LEFT} FROM member_cards c JOIN members m ON m.id=c.member_id WHERE {OPEN_FOR_SALE}{more}", members::CARD_COLS)
}

/// A seller's card that can be bought right now (`OPEN_FOR_SALE`), with its store.
pub struct OpenListing {
    pub card: MemberCard,
    /// The store: its seller's number, address, name (the store's own, or the seller's) and the seller's picture.
    pub store: vault_search::Store,
    /// Copies no open reservation holds.
    pub left: i64,
}

fn open_row(r: &Row) -> rusqlite::Result<OpenListing> {
    let n = members::CARD_N;
    let card = members::map_card(r, 0)?;
    let (slug, store_name, name, avatar): (String, String, String, String) = (r.get(n)?, r.get(n + 1)?, r.get(n + 2)?, r.get(n + 3)?);
    let title = if store_name.trim().is_empty() { name } else { store_name };
    Ok(OpenListing { store: vault_search::Store { id: card.member_id, slug, title, img: avatar, count: 0 }, left: r.get(n + 4)?, card })
}

/// A listing's condition with its finish, the way tiles show it ("Near Mint · Reverse Holo"), and the finish's group.
fn variant_of(k: &MemberCard) -> (String, &'static str) {
    let label = if k.finish.trim().is_empty() { String::new() } else { crate::market::label(k.finish.trim()) };
    let finish = vault_search::finish_group(&label, "");
    let variant = if label.is_empty() || finish == "Normal" { k.cond.clone() } else { format!("{} · {label}", k.cond) };
    (variant, finish)
}

/// The seller's notes on one line, at most 160 letters, for a tile.
fn tile_notes(notes: &str) -> String {
    let one_line = notes.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" · ");
    let mut out: String = one_line.chars().take(160).collect();
    if one_line.chars().count() > 160 {
        out.push('…');
    }
    out
}

impl OpenListing {
    /// Its condition with its finish ("Near Mint · Reverse Holo").
    pub fn variant(&self) -> String {
        variant_of(&self.card).0
    }
    /// Its own page, which opens as the card window over the page.
    pub fn url(&self) -> String {
        format!("/trainers/{}/cards/{}", self.card.member_id, self.card.id)
    }
    /// The card as an Ask the Vault tile (static/vault-talk.js), built from the listing itself: picture, title,
    /// condition, the seller's price and the store's name. `lid` and `amount` give the tile its "+", which puts it in the
    /// cart as this seller's card (static/cart.js), like the "+" on The Vault page.
    pub fn tile(&self) -> serde_json::Value {
        let k = &self.card;
        let (variant, _) = variant_of(k);
        let price = k.worth();
        let grade = if vault_search::cond_class(&k.cond) == "gr" { k.cond.as_str() } else { "" };
        serde_json::json!({
            "lid": k.id, "url": self.url(),
            "title": vault_search::title_and_set(&k.title, &k.card_set).0, "full": k.title, "img": k.image,
            "price": vault_search::peso(price), "amount": price,
            "c": vault_search::cond_class(&variant), "cond": vault_search::tile_condition(&variant),
            "set": k.card_set, "sym": k.symbol(), "foil": vault_search::foil_class(&format!("{} {variant}", k.title)),
            "grade": grade,
            "notes": tile_notes(&k.notes), "tags": [],
            "seller": self.store.title, "seller_slug": self.store.slug, "seller_id": self.store.id, "seller_img": self.store.img,
        })
    }
}

/// One seller's card, while it can be bought right now: the same checks as The Vault page (`market_lines`).
pub fn open_listing(c: &Connection, id: i64) -> rusqlite::Result<Option<OpenListing>> {
    let l = c.query_row(&open_sql(" AND c.id=?2"), rusqlite::params![now(), id], open_row).optional()?;
    Ok(l.filter(|l| l.left > 0))
}

/// A seller's card as an Ask the Vault tile, only while it can be bought right now and has a price.
pub fn ask_tile(c: &Connection, id: i64) -> rusqlite::Result<Option<serde_json::Value>> {
    Ok(open_listing(c, id)?.filter(|l| l.card.has_price()).map(|l| l.tile()))
}

/// Every seller's card that can be bought right now (`OPEN_FOR_SALE`, a copy left), newest first.
pub fn open_listings(c: &Connection) -> rusqlite::Result<Vec<OpenListing>> {
    let mut st = c.prepare_cached(&open_sql(" ORDER BY c.id DESC"))?;
    let rows = st.query_map([now()], open_row)?;
    let mut v = vec![];
    for row in rows {
        let l = row?;
        if l.left > 0 {
            v.push(l);
        }
    }
    Ok(v)
}

/// How many sellers' cards can be bought right now: the cards The Vault page lists (`OPEN_FOR_SALE`, a copy left).
pub fn open_count(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row(&format!("SELECT COUNT(*) FROM member_cards c JOIN members m ON m.id=c.member_id WHERE {OPEN_FOR_SALE} AND {LEFT}>0"), [now()], |r| r.get(0))
}

impl OpenListing {
    /// The store's first letter, for a store with no picture.
    pub fn initial(&self) -> String {
        members::initial(&self.store.title)
    }
}

/// A card in "More like this" under a trainer's card: one of the vault's, or a listing in an open store.
pub enum Like {
    Vault(crate::db::Product),
    Store(OpenListing),
}

/// "More like this" under a trainer's card: up to `n` cards for sale anywhere on the site, the vault's and every open
/// store's listings with a price, never the card itself. Cards with the same name come first (the same card in another
/// store, condition or printing is what a buyer wants to compare), then cards like it. Each group is scored as the
/// vault's own cards are (`crate::similar`: names alike, the same set, a price close by, the same condition).
pub fn related(c: &Connection, me: &MemberCard, n: usize) -> rusqlite::Result<Vec<Like>> {
    let item = |title: &str, set: &str, price: f64, cond: &'static str, has_image: bool| {
        let (name, set) = vault_search::title_and_set(title, set);
        crate::similar::Item { name, set, price, cond, has_image }
    };
    let mut pool: Vec<Like> = crate::db::available_cards(c)?.into_iter().map(Like::Vault).collect();
    pool.extend(open_listings(c)?.into_iter().filter(|l| l.card.id != me.id && l.card.has_price()).map(Like::Store));
    let mut items = vec![item(&me.title, &me.card_set, me.worth(), me.cond_class(), !me.image.is_empty())];
    items.extend(pool.iter().map(|x| match x {
        Like::Vault(p) => item(&p.title, &p.card_set, p.price, p.cond_class(), p.has_image()),
        Like::Store(l) => item(&l.card.title, &l.card.card_set, l.card.worth(), l.card.cond_class(), !l.card.image.is_empty()),
    }));
    // Item 0 is this card; the pool follows it, one place on.
    let name = crate::similar::words(&items[0].name);
    let same: Vec<bool> = items.iter().map(|x| !name.is_empty() && crate::similar::words(&x.name) == name).collect();
    let mut picks = crate::similar::pick(&items, 0, n, |i| !same[i]);
    picks.extend(crate::similar::pick(&items, 0, n - picks.len(), |i| same[i]));
    let mut pool: Vec<Option<Like>> = pool.into_iter().map(Some).collect();
    Ok(picks.into_iter().filter_map(|i| pool.get_mut(i.checked_sub(1)?)?.take()).collect())
}

/// When a store's card was put up for sale, in seconds, for The Vault page's newest-first order (`market_lines`; `e` is
/// each card's first entry in its history, `card_events`). A card added through Add a card has "listed" as its first
/// entry. An older card goes by the earliest thing known about it (its first entry or its last change), and failing
/// both, the day its seller joined.
const LISTED_AT: &str = "COALESCE(MIN(NULLIF(c.updated_at,0), e.first), NULLIF(c.updated_at,0), e.first, m.created_at, 0)";

/// Every seller's card that can be bought right now, for The Vault page's card finder (`/vault.tsv`, after the vault's
/// own catalogue) and Ask the Vault: one `S` line per store, the stores with the most cards first, then one `M` line per
/// listing, newest first, each with the day it was listed (`LISTED_AT`). A card is in when it is open for sale
/// (`OPEN_FOR_SALE`) and has a copy left that no open reservation holds.
pub fn market_lines(c: &Connection) -> rusqlite::Result<String> {
    use std::collections::HashMap;
    let mut st = c.prepare(&format!(
        "SELECT {}, m.slug, m.store_name, m.name, COALESCE(NULLIF(m.store_avatar,''), m.avatar), {LEFT}, {LISTED_AT} AS listed
         FROM member_cards c JOIN members m ON m.id=c.member_id
         LEFT JOIN (SELECT card_id, MIN(created_at) AS first FROM card_events GROUP BY card_id) e ON e.card_id=c.id
         WHERE {OPEN_FOR_SALE}
         ORDER BY listed DESC, c.id DESC",
        members::CARD_COLS
    ))?;
    let n = members::CARD_N;
    let rows = st.query_map([now()], |r| Ok((open_row(r)?, r.get::<_, i64>(n + 5)?)))?;
    let (mut cards, mut stores): (Vec<(MemberCard, i64, i64)>, HashMap<i64, vault_search::Store>) = (vec![], HashMap::new());
    for row in rows {
        let (OpenListing { card: k, store: s, left }, listed) = row?;
        if left <= 0 {
            continue;
        }
        stores.entry(k.member_id).or_insert(s).count += 1;
        cards.push((k, left, listed));
    }
    let mut order: Vec<vault_search::Store> = stores.into_values().collect();
    order.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())).then(a.id.cmp(&b.id)));
    let at: HashMap<i64, u16> = order.iter().enumerate().map(|(i, s)| (s.id, i as u16 + 1)).collect();
    let mut out: String = order.iter().enumerate().map(|(i, s)| vault_search::store_line(i as u16 + 1, s)).collect();
    for (k, left, listed) in cards {
        let key = catalogue_key(&k.card_id);
        let (variant, finish) = variant_of(&k);
        out += &vault_search::market_line(&vault_search::Card {
            handle: String::new(),
            title: k.title.clone(),
            variant,
            set: k.card_set.clone(),
            no: k.number.clone(),
            price: k.worth().round(),
            img: k.image.clone(),
            grade: if vault_search::cond_class(&k.cond) == "gr" { k.cond.clone() } else { String::new() },
            qty: u32::try_from(left).unwrap_or(u32::MAX),
            binders: vec![],
            tags: String::new(),
            sym: k.symbol(),
            finish: finish.to_string(),
            lang: crate::translate::language_of(&k.title, key, "").to_string(),
            notes: tile_notes(&k.notes),
            text: String::new(),
            order: 0,
            store: at.get(&k.member_id).copied().unwrap_or(0),
            lid: k.id,
            time: listed,
        });
    }
    Ok(out)
}

/// The pictures on a binder's tile on the store page, at most `n`: the cover the seller picked first, while that card
/// is on sale in this binder, then the binder's cards on sale, in the seller's order. `listings` is what the store page
/// already shows (`storefront_listings`), so this costs no trip to the database, and a card that is hidden, sold, ended,
/// a highlight or on a wishlist never shows, not even in front. The same picture is never shown twice.
pub fn binder_peeks(b: &MemberBinder, listings: &[MemberCard], n: usize) -> Vec<String> {
    let mut pics: Vec<String> = Vec::new();
    if let Some(cover) = picked_cover(b, listings) {
        pics.push(cover.image.clone());
    }
    for k in listings.iter().filter(|k| k.binder_id == Some(b.id) && Some(k.id) != b.cover_card && !k.image.is_empty()) {
        if pics.len() >= n {
            break;
        }
        if !pics.contains(&k.image) {
            pics.push(k.image.clone());
        }
    }
    pics.truncate(n);
    pics
}

/// The card a seller picked as a binder's cover, while it is on sale in that binder (one of `listings`) and has a picture.
pub fn picked_cover<'a>(b: &MemberBinder, listings: &'a [MemberCard]) -> Option<&'a MemberCard> {
    let id = b.cover_card?;
    listings.iter().find(|k| k.id == id && k.binder_id == Some(b.id) && !k.image.is_empty())
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
    /// The cards of one store sent together from one cart share it (the first card's id); 0 for one alone, and offers.
    pub batch: i64,
    /// When the buyer said "I've paid" (0: not yet).
    pub buyer_paid_at: i64,
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
    /// When the buyer said they paid, in words ("" when they haven't).
    pub fn buyer_paid_text(&self) -> String {
        if self.buyer_paid_at > 0 { ago(self.buyer_paid_at) } else { String::new() }
    }
    /// How it was paid, as shown (proper case).
    pub fn paid_via_text(&self) -> String {
        members::proper(&self.paid_via)
    }
    /// The step it is at, in the words the buttons use ("Accept" makes it "Accepted").
    pub fn status_text(&self) -> &str {
        match self.status.as_str() {
            "Pending" => "New",
            "Confirmed" => "Accepted",
            "Expired" => "Ran Out",
            "Withdrawn" => "Taken Back",
            s => s,
        }
    }
    /// What the buyer should do or wait for, in plain words.
    pub fn buyer_line(&self) -> &'static str {
        match (self.kind, self.status.as_str()) {
            ("reservation", "Pending") => "Waiting for the seller to accept.",
            ("reservation", "Confirmed") => "Accepted. Pay the seller, then they'll mark it paid.",
            ("reservation", "Paid") => "Paid. Thank you!",
            ("reservation", "Declined") => "The seller declined.",
            ("reservation", "Cancelled") => "Cancelled.",
            ("reservation", "Expired") => "This order ran out.",
            ("offer", "Pending") => "Waiting for the seller.",
            ("offer", "Accepted") => "Accepted! It's now an order.",
            ("offer", "Countered") => "The seller made a counter offer.",
            ("offer", "Declined") => "The seller declined.",
            ("offer", "Withdrawn") => "You took it back.",
            _ => "",
        }
    }
}

const ORDER_RES: &str = "r.id, r.buyer_id, COALESCE(b.name, 'A member who left'), r.seller_id, COALESCE(NULLIF(TRIM(s.store_name), ''), s.name, 'A store that closed'), COALESCE(s.slug, ''), r.card_id, c.title, c.image, c.cond, r.qty, r.price, 0.0, r.status, r.message, r.paid_via, r.expires_at, r.created_at, r.updated_at, r.batch, r.buyer_paid_at
    FROM reservations r LEFT JOIN members b ON b.id=r.buyer_id LEFT JOIN members s ON s.id=r.seller_id LEFT JOIN member_cards c ON c.id=r.card_id";
const ORDER_OFF: &str = "o.id, o.sender_id, COALESCE(b.name, 'A member who left'), o.seller_id, COALESCE(NULLIF(TRIM(s.store_name), ''), s.name, 'A store that closed'), COALESCE(s.slug, ''), o.card_id, c.title, c.image, c.cond, o.qty, o.amount, o.counter, o.status, o.message, '', 0, o.created_at, o.updated_at, 0, 0
    FROM offers o LEFT JOIN members b ON b.id=o.sender_id LEFT JOIN members s ON s.id=o.seller_id LEFT JOIN member_cards c ON c.id=o.card_id";

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
            batch: r.get(19)?,
            buyer_paid_at: r.get(20)?,
        })
    }
}

/// The cards one buyer sent one store from one cart belong together: `batch` is the first of them.
pub fn set_batch(c: &Connection, id: i64, batch: i64) -> rusqlite::Result<()> {
    c.execute("UPDATE reservations SET batch=?2 WHERE id=?1", rusqlite::params![id, batch]).map(|_| ())
}

/// The buyer of an accepted order says "I've paid" (the cards they sent this store together go with it). The seller hears,
/// to check their app and mark it paid. Err(text) says why not.
pub fn buyer_paid(c: &Connection, buyer: i64, id: i64) -> rusqlite::Result<Result<(), &'static str>> {
    let Some(r) = reservation(c, id)?.filter(|r| r.buyer_id == buyer) else { return Ok(Err("That order isn't yours.")) };
    if r.status != "Confirmed" {
        return Ok(Err("Pay once the seller accepts your order."));
    }
    let t = now();
    let ids: Vec<i64> = if r.batch > 0 {
        c.prepare("SELECT id FROM reservations WHERE batch=?1 AND buyer_id=?2 AND seller_id=?3 AND status='Confirmed' AND buyer_paid_at=0")?.query_map(rusqlite::params![r.batch, buyer, r.seller_id], |x| x.get(0))?.collect::<Result<_, _>>()?
    } else {
        vec![id]
    };
    for i in &ids {
        c.execute("UPDATE reservations SET buyer_paid_at=?2 WHERE id=?1 AND buyer_paid_at=0", rusqlite::params![i, t])?;
    }
    let what = if ids.len() > 1 { format!("their order of {} cards", ids.len()) } else { short(&r.title) };
    notify_from(c, r.seller_id, buyer, "reservation", &format!("paid:{}", if r.batch > 0 { r.batch } else { id }), &format!("{} says they paid for {what}. Check, then mark it paid.", r.buyer_name), "/me/orders")?;
    Ok(Ok(()))
}

/// The same for an order of the vault's cards: its admins hear.
pub fn vault_buyer_paid(c: &Connection, buyer: i64, id: i64) -> rusqlite::Result<Result<(), &'static str>> {
    let Some(o) = vault_order(c, id)?.filter(|o| o.buyer_id == buyer) else { return Ok(Err("That order isn't yours.")) };
    if o.status != "Confirmed" {
        return Ok(Err("Pay once the vault accepts your order."));
    }
    c.execute("UPDATE vault_orders SET buyer_paid_at=?2 WHERE id=?1 AND buyer_paid_at=0", rusqlite::params![id, now()])?;
    let admins: Vec<i64> = c.prepare("SELECT id FROM members WHERE role='admin' AND blocked=0")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for a in admins.into_iter().filter(|a| *a != buyer) {
        notify_from(c, a, buyer, "order", &format!("vpaid:{id}"), &format!("{} says they paid for their vault order ({}). Check, then mark it paid.", o.buyer_name, o.total_text()), "/admin/orders")?;
    }
    Ok(Ok(()))
}

/// The cards one buyer sent one store together: one order, answered all at once or card by card.
#[derive(Clone, Debug)]
pub struct OrderGroup {
    pub rows: Vec<Order>,
}

impl OrderGroup {
    pub fn first(&self) -> &Order {
        &self.rows[0]
    }
    /// More than one card: shown under one heading with Accept all.
    pub fn many(&self) -> bool {
        self.rows.len() > 1
    }
    /// When the buyer said they paid for it ("" when they haven't).
    pub fn buyer_paid_text(&self) -> String {
        self.rows.iter().map(|o| o.buyer_paid_at).max().filter(|t| *t > 0).map(ago).unwrap_or_default()
    }
    /// Some card in it is at this step ("Pending", "Confirmed").
    pub fn has(&self, status: &str) -> bool {
        self.rows.iter().any(|o| o.status == status)
    }
    /// What the cards still on come to (declined, cancelled and run-out ones left out).
    pub fn total_text(&self) -> String {
        vault_search::peso(self.rows.iter().filter(|o| !matches!(o.status.as_str(), "Declined" | "Cancelled" | "Expired")).map(Order::total).sum())
    }
    pub fn units(&self) -> i64 {
        self.rows.iter().map(|o| o.qty).sum()
    }
}

/// A seller's orders as the buyers sent them: the cards of one cart for this store together (where the first of them
/// sits in the list), a card ordered alone by itself.
pub fn order_groups(rows: Vec<Order>) -> Vec<OrderGroup> {
    let mut out: Vec<OrderGroup> = vec![];
    for o in rows {
        match out.iter_mut().find(|g| o.batch > 0 && g.rows[0].batch == o.batch) {
            Some(g) => g.rows.push(o),
            None => out.push(OrderGroup { rows: vec![o] }),
        }
    }
    out
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
    can_sell(c, seller, card_id, qty)
}

/// Why `qty` copies of this listing can't be sold right now: the store, the card and the copies left.
fn can_sell(c: &Connection, seller: i64, card_id: i64, qty: i64) -> rusqlite::Result<Result<MemberCard, &'static str>> {
    let Some(card) = members::card(c, seller, card_id)? else { return Ok(Err("That card isn't in this store.")) };
    let (status, selling, blocked, profile_ok, hidden): (String, i64, i64, i64, i64) = c.query_row(
        "SELECT status, seller, blocked, profile_ok, store_hidden FROM members WHERE id=?1",
        [seller],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    if blocked != 0 || profile_ok == 0 || hidden != 0 {
        return Ok(Err("This store is closed."));
    }
    if selling == 0 {
        return Ok(Err("This trainer isn't selling."));
    }
    if status == "Vacation" {
        return Ok(Err("This seller is on vacation. Please come back later."));
    }
    if !card.is_sale() {
        return Ok(Err("That card isn't for sale."));
    }
    if card.status == "Out of stock" {
        return Ok(Err("That card is out of stock right now."));
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

/// Runs `f` while holding the database's write lock (BEGIN IMMEDIATE), so nobody else can slip in between a
/// check ("is a copy left?") and the write after it. The changes are kept only when `f` says Ok(Ok(..)).
/// Already inside a transaction (a caller holds the lock), it just runs `f`.
fn locked<T, E>(c: &Connection, f: impl FnOnce() -> rusqlite::Result<Result<T, E>>) -> rusqlite::Result<Result<T, E>> {
    if !c.is_autocommit() {
        return f();
    }
    c.execute_batch("BEGIN IMMEDIATE")?;
    match f() {
        Ok(Ok(v)) => match c.execute_batch("COMMIT") {
            Ok(()) => Ok(Ok(v)),
            Err(e) => {
                c.execute_batch("ROLLBACK").ok();
                Err(e)
            }
        },
        Ok(Err(e)) => {
            c.execute_batch("ROLLBACK")?;
            Ok(Err(e))
        }
        Err(e) => {
            c.execute_batch("ROLLBACK").ok();
            Err(e)
        }
    }
}

/// A buyer reserves a card at the asking price. The seller is told. Err(text) says why not.
/// Two buyers can't both get the last copy: the check and the reservation happen under one lock.
pub fn reserve(c: &Connection, buyer: i64, seller: i64, card_id: i64, qty: i64, message: &str) -> rusqlite::Result<Result<i64, &'static str>> {
    locked(c, || {
        let card = match can_buy(c, buyer, seller, card_id, qty)? {
            Ok(k) => k,
            Err(e) => return Ok(Err(e)),
        };
        let open: i64 = c.query_row("SELECT COUNT(*) FROM reservations WHERE buyer_id=?1 AND card_id=?2 AND status IN ('Pending','Confirmed')", [buyer, card_id], |r| r.get(0))?;
        if open > 0 {
            return Ok(Err("You've already ordered this card. Check My Orders."));
        }
        if card.worth() <= 0.0 {
            return Ok(Err("This card has no price yet. Ask the seller, or make an offer."));
        }
        reserve_at(c, buyer, seller, card_id, qty, card.worth(), message, "Pending", 0)
    })
}

/// Every reservation goes through here, so the store, the card and the copies left are always checked first
/// (an offer accepted days later may find the card gone, sold or reserved by someone else).
fn reserve_at(c: &Connection, buyer: i64, seller: i64, card_id: i64, qty: i64, price: f64, message: &str, status: &str, offer_id: i64) -> rusqlite::Result<Result<i64, &'static str>> {
    if let Err(e) = can_sell(c, seller, card_id, qty)? {
        return Ok(Err(e));
    }
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
        // One cart is one line (and one buzz) for each store, brought up to date as its cards arrive: what this buyer
        // is waiting on from this seller, ordered in the last few minutes.
        let (n, units, total): (i64, i64, f64) = c.query_row(
            "SELECT COUNT(*), COALESCE(SUM(qty),0), COALESCE(SUM(price*qty),0) FROM reservations WHERE buyer_id=?1 AND seller_id=?2 AND status='Pending' AND created_at>=?3",
            rusqlite::params![buyer, seller, t - 600],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let words = if n > 1 { format!("{buyer_name} wants to buy {units} cards for {}.", vault_search::peso(total)) } else { format!("{buyer_name} wants to buy {} ×{qty} for {}.", short(&title), vault_search::peso(price * qty as f64)) };
        notify_from(c, seller, buyer, "reservation", &format!("cart:{buyer}"), &words, "/me/orders")?;
    }
    Ok(Ok(id))
}

fn names(c: &Connection, member: i64, card_id: i64) -> rusqlite::Result<(String, String)> {
    let name: String = c.query_row("SELECT name FROM members WHERE id=?1", [member], |r| r.get(0)).optional()?.unwrap_or_else(|| "Someone".into());
    let title: String = c.query_row("SELECT title FROM member_cards WHERE id=?1", [card_id], |r| r.get(0)).optional()?.unwrap_or_else(|| "a card".into());
    Ok((name, title))
}

/// The seller answers a reservation: "Confirmed" (go ahead and pay), "Paid" (money received: the sale is done), "Declined".
/// Accepting checks the copies are still there (the seller may have lowered the count, or marked it Sold).
/// Marking paid doesn't: the money has already changed hands, and the seller may have marked the card Sold first.
pub fn answer_reservation(c: &Connection, seller: i64, id: i64, status: &str, paid_via: &str) -> rusqlite::Result<Result<Order, &'static str>> {
    locked(c, || answer_reservation_now(c, seller, id, status, paid_via))
}

fn answer_reservation_now(c: &Connection, seller: i64, id: i64, status: &str, paid_via: &str) -> rusqlite::Result<Result<Order, &'static str>> {
    let Some(r) = reservation(c, id)?.filter(|r| r.seller_id == seller) else { return Ok(Err("That order isn't yours.")) };
    let ok = matches!((r.status.as_str(), status), ("Pending", "Confirmed") | ("Pending", "Declined") | ("Confirmed", "Paid") | ("Confirmed", "Declined") | ("Pending", "Paid"));
    if !ok {
        return Ok(Err("That order can't change like that any more."));
    }
    if r.status == "Pending" && status == "Confirmed" {
        let have: Option<i64> = c.query_row("SELECT qty FROM member_cards WHERE id=?1", [r.card_id], |x| x.get(0)).optional()?;
        // Copies held by the other open reservations on this card.
        let others = held(c, r.card_id)? - r.qty;
        if have.map_or(true, |q| q - others < r.qty) {
            return Ok(Err("Not enough copies are left for this order. Add copies first, or decline it."));
        }
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
        }
        _ => {}
    }
    // The cards of one order answered together are one line for the buyer (and one buzz), kept up to date.
    let (group, many) = if r.batch > 0 {
        let n: i64 = c.query_row("SELECT COUNT(*) FROM reservations WHERE batch=?1 AND seller_id=?2 AND status=?3", rusqlite::params![r.batch, seller, status], |x| x.get(0))?;
        (format!("answer:{}:{status}", r.batch), n)
    } else {
        (String::new(), 1)
    };
    let what = if many > 1 { format!("your order of {many} cards") } else { format!("your order for {}", short(&r.title)) };
    let words = match status {
        "Paid" => format!("{} marked {what} as paid. Enjoy {}! Tap to say how it went.", r.seller_name, if many > 1 { "your cards" } else { "your card" }),
        "Confirmed" => format!("{} accepted {what}. Time to pay.", r.seller_name),
        _ => format!("{} declined {what}.", r.seller_name),
    };
    notify_from(c, r.buyer_id, seller, "my_order", &group, &words, "/me/orders?tab=buying")?;
    refresh_reserved(c, r.card_id)?;
    Ok(Ok(reservation(c, id)?.unwrap_or(r)))
}

/// A member is leaving the site. Nothing they started may keep holding cards: their vault orders and
/// reservations are cancelled and their offers withdrawn. Buyers of their own cards are told the cards are off sale.
pub fn close_member_orders(c: &Connection, id: i64) -> rusqlite::Result<()> {
    let t = now();
    c.execute("UPDATE vault_orders SET status='Cancelled', updated_at=?2 WHERE buyer_id=?1 AND status IN ('Pending','Confirmed')", rusqlite::params![id, t])?;
    let held: Vec<i64> = {
        let mut st = c.prepare("SELECT DISTINCT card_id FROM reservations WHERE buyer_id=?1 AND status IN ('Pending','Confirmed')")?;
        let v = st.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        v
    };
    c.execute("UPDATE reservations SET status='Cancelled', updated_at=?2 WHERE buyer_id=?1 AND status IN ('Pending','Confirmed')", rusqlite::params![id, t])?;
    for card in held {
        refresh_reserved(c, card)?;
    }
    c.execute("UPDATE offers SET status='Withdrawn', updated_at=?2 WHERE sender_id=?1 AND status IN ('Pending','Countered')", rusqlite::params![id, t])?;
    close_store_orders(c, id)?;
    Ok(())
}

/// A seller stops selling, or is blocked: every open reservation and offer on their cards is closed, and the
/// buyers are told once per card. What they are buying from others is left alone. Returns how many closed.
/// All of it happens at once, or none of it.
pub fn close_store_orders(c: &Connection, seller_id: i64) -> rusqlite::Result<usize> {
    let done: Result<usize, ()> = locked(c, || {
        let cards: Vec<i64> = c
            .prepare(
                "SELECT card_id FROM reservations WHERE seller_id=?1 AND status IN ('Pending','Confirmed')
                 UNION SELECT card_id FROM offers WHERE seller_id=?1 AND status IN ('Pending','Countered')",
            )?
            .query_map([seller_id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        let mut n = 0;
        for card in cards {
            n += close_open_orders(c, seller_id, card)?;
        }
        Ok(Ok(n))
    })?;
    Ok(done.unwrap_or(0))
}

/// A listing leaves the store (removed, or no longer for sale): its open reservations are cancelled and its open
/// offers declined, and each buyer is told once. Returns how many were closed. Running it twice tells nobody twice.
pub fn close_open_orders(c: &Connection, seller_id: i64, card_id: i64) -> rusqlite::Result<usize> {
    let res: Vec<i64> = c
        .prepare("SELECT buyer_id FROM reservations WHERE seller_id=?1 AND card_id=?2 AND status IN ('Pending','Confirmed')")?
        .query_map([seller_id, card_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let offs: Vec<i64> = c
        .prepare("SELECT sender_id FROM offers WHERE seller_id=?1 AND card_id=?2 AND status IN ('Pending','Countered')")?
        .query_map([seller_id, card_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if res.is_empty() && offs.is_empty() {
        return Ok(0);
    }
    let t = now();
    let n = c.execute("UPDATE reservations SET status='Cancelled', updated_at=?3 WHERE seller_id=?1 AND card_id=?2 AND status IN ('Pending','Confirmed')", rusqlite::params![seller_id, card_id, t])?
        + c.execute("UPDATE offers SET status='Declined', updated_at=?3 WHERE seller_id=?1 AND card_id=?2 AND status IN ('Pending','Countered')", rusqlite::params![seller_id, card_id, t])?;
    let (_, title) = names(c, seller_id, card_id)?;
    // Buyers know the seller by the store's name (the trainer's own name when the store has none).
    let seller_name: String = c.query_row("SELECT COALESCE(NULLIF(TRIM(store_name), ''), name) FROM members WHERE id=?1", [seller_id], |r| r.get(0)).optional()?.unwrap_or_else(|| "The seller".into());
    let mut told: Vec<i64> = vec![];
    for buyer in res.iter().chain(offs.iter()).copied() {
        if told.contains(&buyer) {
            continue;
        }
        told.push(buyer);
        notify_from(c, buyer, seller_id, "my_order", "", &format!("{seller_name} took {} off sale.", short(&title)), "/me/orders?tab=buying")?;
    }
    refresh_reserved(c, card_id)?;
    Ok(n)
}

/// The buyer changes their mind, before it's paid.
pub fn cancel_reservation(c: &Connection, buyer: i64, id: i64) -> rusqlite::Result<bool> {
    let Some(r) = reservation(c, id)?.filter(|r| r.buyer_id == buyer && matches!(r.status.as_str(), "Pending" | "Confirmed")) else { return Ok(false) };
    c.execute("UPDATE reservations SET status='Cancelled', updated_at=?2 WHERE id=?1", rusqlite::params![id, now()])?;
    refresh_reserved(c, r.card_id)?;
    notify_from(c, r.seller_id, buyer, "reservation", "", &format!("{} cancelled their order for {}.", r.buyer_name, short(&r.title)), "/me/orders")?;
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
        return Ok(Err("You already have an offer on this card. Check My Orders."));
    }
    let t = now();
    c.execute(
        "INSERT INTO offers(sender_id, seller_id, card_id, amount, status, created_at, qty, message, updated_at) VALUES(?1,?2,?3,?4,'Pending',?5,?6,?7,?5)",
        rusqlite::params![buyer, seller, card_id, amount.round(), t, qty, members::clean_text(message, 300)],
    )?;
    let id = c.last_insert_rowid();
    let (buyer_name, title) = names(c, buyer, card_id)?;
    notify_from(c, seller, buyer, "offer", "", &format!("{buyer_name} offers {} for {} ×{qty}.", vault_search::peso(amount), short(&title)), "/me/orders")?;
    Ok(Ok(id))
}

/// The seller answers an offer: "Accepted" (it becomes a confirmed reservation at that price), "Declined", or "Countered" with a price.
pub fn answer_offer(c: &Connection, seller: i64, id: i64, status: &str, counter: f64) -> rusqlite::Result<Result<Order, &'static str>> {
    locked(c, || answer_offer_now(c, seller, id, status, counter))
}

fn answer_offer_now(c: &Connection, seller: i64, id: i64, status: &str, counter: f64) -> rusqlite::Result<Result<Order, &'static str>> {
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
            notify_from(c, o.buyer_id, seller, "my_order", "", &format!("{} accepted your offer of {} for {}. Time to pay.", o.seller_name, o.price_text(), short(&o.title)), "/me/orders?tab=buying")?;
        }
        "Countered" => {
            if !counter.is_finite() || !(1.0..=10_000_000.0).contains(&counter) {
                return Ok(Err("Type your counter price in pesos."));
            }
            c.execute("UPDATE offers SET status='Countered', counter=?2, updated_at=?3 WHERE id=?1", rusqlite::params![id, counter.round(), t])?;
            notify_from(c, o.buyer_id, seller, "my_order", "", &format!("{} counters with {} for {}.", o.seller_name, vault_search::peso(counter), short(&o.title)), "/me/orders?tab=buying")?;
        }
        "Declined" => {
            c.execute("UPDATE offers SET status='Declined', updated_at=?2 WHERE id=?1", rusqlite::params![id, t])?;
            notify_from(c, o.buyer_id, seller, "my_order", "", &format!("{} declined your offer for {}.", o.seller_name, short(&o.title)), "/me/orders?tab=buying")?;
        }
        _ => return Ok(Err("Accept, decline or counter.")),
    }
    Ok(Ok(offer(c, id)?.unwrap_or(o)))
}

/// The buyer takes a counter offer ("accept": it becomes a confirmed reservation at the seller's price) or drops the offer ("withdraw").
pub fn answer_counter(c: &Connection, buyer: i64, id: i64, accept: bool) -> rusqlite::Result<Result<(), &'static str>> {
    locked(c, || answer_counter_now(c, buyer, id, accept))
}

fn answer_counter_now(c: &Connection, buyer: i64, id: i64, accept: bool) -> rusqlite::Result<Result<(), &'static str>> {
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
        notify_from(c, o.seller_id, buyer, "offer", "", &format!("{} accepted your counter of {} for {}.", o.buyer_name, o.counter_text(), short(&o.title)), "/me/orders")?;
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

const INQ: &str = "i.id, i.sender_id, b.name, i.seller_id, COALESCE(NULLIF(TRIM(s.store_name), ''), s.name), i.card_id, c.title, c.image, i.message, i.reply, i.replied_at, i.created_at
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
    notify_from(c, seller, sender, "inquiry", "", &format!("{name} asks{about}: “{}”", short(&msg)), "/me/orders?tab=questions")?;
    Ok(Ok(id))
}

pub fn reply(c: &Connection, seller: i64, id: i64, text: &str) -> rusqlite::Result<Result<(), &'static str>> {
    let Some(q) = c.query_row(&format!("SELECT {INQ} WHERE i.id=?1"), [id], map_inq).optional()?.filter(|q| q.seller_id == seller) else { return Ok(Err("That question isn't yours.")) };
    let t = members::clean_text(text, 500);
    if t.chars().count() < 1 {
        return Ok(Err("Type a reply."));
    }
    c.execute("UPDATE inquiries SET reply=?2, replied_at=?3 WHERE id=?1", rusqlite::params![id, t, now()])?;
    notify_from(c, q.sender_id, seller, "my_order", "", &format!("{} replied: “{}”", q.seller_name, short(&t)), "/me/orders?tab=buying")?;
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
    /// How it was paid, as shown (proper case).
    pub fn paid_via_text(&self) -> String {
        members::proper(&self.paid_via)
    }
    pub fn amount_text(&self) -> String {
        vault_search::peso(self.amount)
    }
    pub fn when(&self) -> String {
        ago(self.created_at)
    }
}

const SALE: &str = "t.id, t.buyer_id, b.name, t.seller_id, COALESCE(NULLIF(TRIM(s.store_name), ''), s.name), t.card_id, COALESCE(NULLIF(t.title,''), c.title, 'A card'), COALESCE(c.image,''), t.qty, t.amount, t.paid_via, t.created_at,
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
    notify_from(c, seller, reviewer, "review", "", &format!("{name} left you a {rating}-star review."), &format!("/store/{}#vouches", slug_of(c, seller)?))?;
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

/// Follow a store (`seller` 0 is the vault's), or stop. Returns whether they're following now.
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
    if seller == 0 {
        // The vault's store is run by its admins: each of them hears it, as a seller would.
        let admins: Vec<i64> = c.prepare("SELECT id FROM members WHERE role='admin' AND blocked=0")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        for a in admins.into_iter().filter(|a| *a != follower) {
            notify_from(c, a, follower, "follow", "", &format!("{name} now follows the vault's store."), &format!("/trainers/{follower}"))?;
        }
    } else {
        let store = c.query_row("SELECT seller FROM members WHERE id=?1", [seller], |r| r.get::<_, i64>(0)).optional()?.unwrap_or(0) != 0;
        let words = if store { format!("{name} now follows your store.") } else { format!("{name} now follows you.") };
        notify_from(c, seller, follower, "follow", "", &words, &format!("/trainers/{follower}"))?;
    }
    Ok(true)
}

/// The border colours "Edit store" offers (any other "#rrggbb" can be picked too): name, colour. '' is gold, the
/// colour every store starts with, the vault's included.
pub const BORDERS: [(&str, &str); 10] = [
    ("Gold", ""),
    ("Silver", "#c0c6d4"),
    ("Bronze", "#c98a55"),
    ("Red", "#e5534b"),
    ("Orange", "#f0883e"),
    ("Green", "#46c37b"),
    ("Teal", "#2bb6a8"),
    ("Blue", "#4c8dff"),
    ("Purple", "#a371f7"),
    ("Pink", "#f778ba"),
];

/// A border colour as it is kept: '' (gold) or "#rrggbb" in small letters. None when it isn't a colour, or is so dark
/// it would vanish against the page (less than 2 to 1 against its background, #0b0d14).
pub fn border_ok(v: &str) -> Option<String> {
    let v = v.trim().to_ascii_lowercase();
    if v.is_empty() || v == "gold" {
        return Some(String::new());
    }
    let hex = v.strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let lum = |h: &str| {
        let lin = |i: usize| {
            let x = u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f64 / 255.0;
            if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(0) + 0.7152 * lin(2) + 0.0722 * lin(4)
    };
    ((lum(hex) + 0.05) / (lum("0b0d14") + 0.05) >= 2.0).then(|| format!("#{hex}"))
}

/// The vault's store border, kept like its other store settings ('' is gold).
pub fn vault_border(c: &Connection) -> rusqlite::Result<String> {
    Ok(crate::blocks::setting(c, "vault_store_border")?.and_then(|b| border_ok(&b)).unwrap_or_default())
}

pub fn set_vault_border(c: &Connection, border: &str) -> rusqlite::Result<()> {
    crate::blocks::set_setting(c, "vault_store_border", border)
}

/// A seller's store border ('' is gold). Checked with `border_ok` before it gets here.
pub fn set_border(c: &Connection, seller: i64, border: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE members SET store_border=?2 WHERE id=?1", rusqlite::params![seller, border]).map(|_| ())
}

/// Copies the vault has sold: the cards in orders it marked as paid.
pub fn vault_sold(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT COALESCE(SUM(i.qty),0) FROM vault_order_items i JOIN vault_orders o ON o.id=i.order_id WHERE o.status='Paid'", [], |r| r.get(0))
}

/// When a store last put a card up or changed one for sale: the vault's newest card, or the seller's latest change to a
/// listing. The Stores page lists the most recently active store first.
pub fn last_active(c: &Connection, seller: i64) -> rusqlite::Result<i64> {
    if seller == 0 {
        // The vault's cards carry their date as text ("2026-11-09T22:06:16").
        return c.query_row("SELECT COALESCE(CAST(strftime('%s', MAX(created_at)) AS INTEGER),0) FROM products WHERE available=1 AND created_at<>''", [], |r| r.get(0));
    }
    c.query_row("SELECT COALESCE(MAX(updated_at),0) FROM member_cards WHERE member_id=?1 AND kind='sale'", [seller], |r| r.get(0))
}

pub fn is_following(c: &Connection, follower: i64, seller: i64) -> rusqlite::Result<bool> {
    let n: i64 = c.query_row("SELECT COUNT(*) FROM store_follows WHERE follower_id=?1 AND seller_id=?2", [follower, seller], |r| r.get(0))?;
    Ok(n > 0)
}

/// Who follows a trainer or a store (0: the vault's), newest first: (id, name, picture). Only trainers whose page is
/// up, as on Trainers.
pub fn follower_list(c: &Connection, seller: i64, limit: i64) -> rusqlite::Result<Vec<(i64, String, String)>> {
    c.prepare("SELECT m.id, m.name, m.avatar FROM store_follows f JOIN members m ON m.id=f.follower_id WHERE f.seller_id=?1 AND m.blocked=0 AND m.profile_ok=1 ORDER BY f.created_at DESC LIMIT ?2")?
        .query_map(rusqlite::params![seller, limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect()
}

/// The vault's staff, who hear about reports: its admins and moderators.
pub fn staff_ids(c: &Connection) -> rusqlite::Result<Vec<i64>> {
    c.prepare("SELECT id FROM members WHERE role IN ('admin','mod') AND blocked=0 ORDER BY id")?.query_map([], |r| r.get(0))?.collect()
}

pub fn followers(c: &Connection, seller: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM store_follows WHERE seller_id=?1", [seller], |r| r.get(0))
}

/// A seller hides their store from everyone else, or shows it again. Hidden, it is left out of Stores, The Vault page,
/// Ask the Vault, the feed, wishlist alerts, bookmarks and the sitemap, its page and cards are not found, and it takes
/// no orders, offers or questions; orders already placed carry on, and its owner keeps working on it.
pub fn set_store_hidden(c: &Connection, seller: i64, hidden: bool) -> rusqlite::Result<()> {
    c.execute("UPDATE members SET store_hidden=?2 WHERE id=?1", rusqlite::params![seller, hidden as i64])?;
    Ok(())
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
        // One note a day per card, not one per look (the "view" switch says whether it is wanted).
        let today: i64 = c.query_row("SELECT COUNT(*) FROM listing_views WHERE card_id=?1 AND day=?2", [card_id, t / 86400], |r| r.get(0))?;
        if today == 1 {
            let (_, title) = names(c, owner, card_id)?;
            notify(c, owner, "view", &format!("People are looking at {} today.", short(&title)), &format!("/me/listings?id={card_id}"))?;
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
        let (name, title) = names(c, member, card_id)?;
        notify_from(c, owner, member, "view", "", &format!("{name} saved {}.", short(&title)), &format!("/me/listings?id={card_id}"))?;
    }
    Ok(true)
}

// ---------- bookmarks ----------
// What a trainer keeps to come back to: stores ("store", the vault's is 0) and the vault's cards ("product") here; a
// seller's card is a saved listing (`listing_saves`, so the seller still sees how many saved it). One Bookmarks page
// shows them all, each card under the store it came from.

/// Bookmark a store ("store": a seller's id, 0 for the vault's) or one of the vault's cards ("product"), or take the
/// bookmark off. Returns whether it is bookmarked now.
pub fn toggle_bookmark(c: &Connection, member: i64, kind: &str, id: i64) -> rusqlite::Result<bool> {
    let gone = c.execute("DELETE FROM bookmarks WHERE member_id=?1 AND kind=?2 AND ref=?3", rusqlite::params![member, kind, id])?;
    if gone > 0 {
        return Ok(false);
    }
    c.execute("INSERT INTO bookmarks(member_id, kind, ref, created_at) VALUES(?1,?2,?3,?4)", rusqlite::params![member, kind, id, now()])?;
    Ok(true)
}

/// Everything one trainer bookmarked, newest first: (kind, id, when). Seller cards come as "card".
pub fn bookmarks_of(c: &Connection, member: i64) -> rusqlite::Result<Vec<(String, i64, i64)>> {
    let v = c
        .prepare("SELECT kind, ref, created_at FROM bookmarks WHERE member_id=?1 UNION ALL SELECT 'card', card_id, created_at FROM listing_saves WHERE member_id=?1 ORDER BY 3 DESC")?
        .query_map([member], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    Ok(v)
}

/// How many things one trainer bookmarked: the number beside Bookmarks in the menu.
pub fn bookmark_count(c: &Connection, member: i64) -> rusqlite::Result<i64> {
    c.query_row(
        "SELECT (SELECT COUNT(*) FROM bookmarks b WHERE b.member_id=?1 AND (
                   (b.kind='store' AND (b.ref=0 OR EXISTS(SELECT 1 FROM members m WHERE m.id=b.ref AND m.blocked=0 AND m.store_hidden=0)))
                OR (b.kind='product' AND EXISTS(SELECT 1 FROM products p WHERE p.id=b.ref))))
              + (SELECT COUNT(*) FROM listing_saves s JOIN member_cards k ON k.id=s.card_id JOIN members m ON m.id=k.member_id
                 WHERE s.member_id=?1 AND m.blocked=0 AND m.profile_ok=1 AND m.store_hidden=0 AND k.kind='sale' AND k.status NOT IN ('Hidden','Expired'))",
        [member],
        |r| r.get(0),
    )
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

/// Kinds of notification a member can switch, with what each means in plain words. "my_order" is the buyer's side:
/// news about cards they are buying; "following" is news from stores they follow; "staff" is for the vault's staff.
/// The others are about their own store.
pub const EVENTS: [(&str, &str); 17] = [
    ("message", "Private messages"),
    ("my_order", "My Orders: accepted, declined, a counter offer, a reply"),
    ("reservation", "Someone orders one of my cards"),
    ("offer", "Someone makes an offer on my card"),
    ("inquiry", "Someone asks me a question"),
    ("order", "Someone orders from the vault (admins)"),
    ("follow", "Someone follows me or my store"),
    ("social", "Likes, comments and replies"),
    ("review", "Someone vouches for my store or reviews a sale"),
    ("following", "Someone I follow posts or puts up cards"),
    ("wish", "A card on my wishlist is for sale"),
    ("announcement", "News from the vault"),
    ("staff", "New trainers, reports (staff)"),
    ("view", "People look at or save my cards (once a day per card)"),
    ("price", "My price is far from today's market"),
    ("expiry", "A card's end date passes"),
    ("status", "A card's status changes"),
];

/// How a member hears about each kind until they choose: (on the site, by email, on their phone). Everything shows
/// in the bell; what someone is waiting on also rings their phone (where alerts are on); the rest stays in the bell.
pub fn default_pref(event: &str) -> (bool, bool, bool) {
    let quiet = matches!(event, "view" | "price" | "expiry" | "status" | "dm_watch");
    (true, false, !quiet)
}

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
        let (s0, e0, p0) = default_pref(event);
        let (site, email, push) = saved.unwrap_or((s0 as i64, e0 as i64, p0 as i64));
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

/// Members told something since the last look: every open page of theirs asks the bell again within seconds (the
/// push loop in main.rs wakes them through the messages' live connection).
static WOKEN: std::sync::Mutex<Vec<i64>> = std::sync::Mutex::new(Vec::new());

/// The members told something since the last call, each once.
pub fn take_woken() -> Vec<i64> {
    let mut w = WOKEN.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = std::mem::take(&mut *w);
    v.sort_unstable();
    v.dedup();
    v
}

/// This member's bell changed (a line written or brought up to date somewhere else): their open pages ask again.
pub fn wake(member: i64) {
    let mut w = WOKEN.lock().unwrap_or_else(|e| e.into_inner());
    if w.len() < 10_000 {
        w.push(member);
    }
}

/// Tells a member something, the ways they chose: on the site, by email (queued for the mailer, see `outbox`), and
/// by push to their phone or computer (queued in `push_outbox`; private messages send their own, see messages.rs).
pub fn notify(c: &Connection, member: i64, kind: &str, message: &str, link: &str) -> rusqlite::Result<()> {
    notify_from(c, member, 0, kind, "", message, link)
}

/// `notify`, saying who did it (`actor`: their picture shows beside the line; 0 is the site) and what it is about
/// (`group`): while a line with the same group is unread, it is brought up to date instead of written again, and the
/// phone isn't buzzed twice. Grouped: reactions on one post, the cards of one cart, one store's new cards a day.
pub fn notify_from(c: &Connection, member: i64, actor: i64, kind: &str, group: &str, message: &str, link: &str) -> rusqlite::Result<()> {
    let (s0, e0, p0) = default_pref(kind);
    let (site, email, push): (i64, i64, i64) = c
        .query_row("SELECT site, email, push FROM notify_prefs WHERE member_id=?1 AND event=?2", rusqlite::params![member, kind], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?
        .unwrap_or((s0 as i64, e0 as i64, p0 as i64));
    if !group.is_empty() && site != 0 {
        let open: Option<i64> = c.query_row("SELECT id FROM notifications WHERE member_id=?1 AND ref=?2 AND is_read=0 ORDER BY id DESC LIMIT 1", rusqlite::params![member, group], |r| r.get(0)).optional()?;
        if let Some(id) = open {
            c.execute("UPDATE notifications SET message=?2, link=?3, actor_id=?4, created_at=?5 WHERE id=?1", rusqlite::params![id, message, link, actor, now()])?;
            wake(member);
            return Ok(());
        }
    }
    if push != 0 && kind != "message" {
        c.execute(
            "INSERT INTO push_outbox(member_id, kind, message, link, created_at) VALUES(?1,?2,?3,?4,?5)",
            rusqlite::params![member, kind, members::clean_line(message, 300), link, now()],
        )?;
    }
    if site == 0 && email == 0 {
        return Ok(());
    }
    // A note hidden from the site but wanted by email still goes into the list, marked read, so the mailer can pick it up.
    c.execute(
        "INSERT INTO notifications(member_id, kind, message, link, is_read, created_at, actor_id, ref) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        rusqlite::params![member, kind, message, link, site == 0, now(), actor, group],
    )?;
    if email != 0 {
        c.execute("INSERT INTO outbox(notification_id, created_at) VALUES(?1,?2)", rusqlite::params![c.last_insert_rowid(), now()]).ok();
    }
    if site != 0 {
        wake(member);
    }
    Ok(())
}

/// Plain words for a kind of notification, as the bell and the notifications page name it.
pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        "message" | "dm_watch" => "Message",
        "my_order" => "My Order",
        "reservation" | "order" => "Order",
        "offer" => "Offer",
        "inquiry" => "Question",
        "follow" => "Follow",
        "following" => "Following",
        "social" => "Like Or Comment",
        "review" => "Vouch",
        "wish" => "Wishlist",
        "announcement" => "News",
        "staff" => "Staff",
        "view" => "Views",
        "price" => "Price",
        "expiry" | "status" => "Listing",
        _ => "Note",
    }
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
        .prepare(&format!("SELECT {} FROM {} WHERE n.member_id=?1 ORDER BY n.created_at DESC, n.id DESC LIMIT ?2 OFFSET ?3", members::NOTE_COLS, members::NOTE_FROM))?
        .query_map(rusqlite::params![member, per, page.max(0) * per], members::map_note)?
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

/// Pushes waiting to go out (oldest first), taken off the queue: (member, kind, message, link). Anything older than a
/// day is dropped instead: news that late is no longer news.
pub fn take_push_outbox(c: &Connection, limit: i64) -> rusqlite::Result<Vec<(i64, String, String, String)>> {
    c.execute("DELETE FROM push_outbox WHERE created_at < ?1", [now() - 86_400])?;
    let rows: Vec<(i64, i64, String, String, String)> = c
        .prepare("SELECT id, member_id, kind, message, link FROM push_outbox ORDER BY id LIMIT ?1")?
        .query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<_, _>>()?;
    for (id, ..) in &rows {
        c.execute("DELETE FROM push_outbox WHERE id=?1", [id])?;
    }
    Ok(rows.into_iter().map(|(_, m, k, msg, l)| (m, k, msg, l)).collect())
}

/// Saves a browser's push subscription for a member. One browser belongs to one person: whoever signed up on it last
/// (anyone else's sign-up on that browser ends). Message alerts are on by default (see prefs_of).
pub fn push_subscribe(c: &Connection, member: i64, endpoint: &str, p256dh: &str, auth: &str) -> rusqlite::Result<()> {
    if !endpoint.starts_with("https://") || endpoint.len() > 1000 || p256dh.len() > 200 || auth.len() > 100 {
        return Ok(());
    }
    c.execute("DELETE FROM push_subscriptions WHERE endpoint=?1", [endpoint])?;
    // Ten devices each at most: a new one pushes out the oldest.
    c.execute(
        "DELETE FROM push_subscriptions WHERE member_id=?1 AND id NOT IN (SELECT id FROM push_subscriptions WHERE member_id=?1 ORDER BY id DESC LIMIT ?2)",
        rusqlite::params![member, MAX_PUSH_DEVICES - 1],
    )?;
    c.execute("INSERT INTO push_subscriptions(member_id, endpoint, p256dh, auth) VALUES(?1,?2,?3,?4)", rusqlite::params![member, endpoint, p256dh, auth])?;
    Ok(())
}

/// How many browsers or phones one member can get push notifications on.
const MAX_PUSH_DEVICES: i64 = 10;

/// A buyer's own email for the notifications they switch on. Anything that isn't an email address clears it.
pub fn set_email(c: &Connection, member: i64, email: &str) -> rusqlite::Result<String> {
    let e = clean_email(email);
    c.execute("UPDATE members SET email=?2, updated_at=?3 WHERE id=?1", rusqlite::params![member, e, now()])?;
    Ok(e)
}

pub fn push_unsubscribe(c: &Connection, member: i64, endpoint: &str) -> rusqlite::Result<()> {
    c.execute("DELETE FROM push_subscriptions WHERE member_id=?1 AND endpoint=?2", rusqlite::params![member, endpoint])?;
    Ok(())
}

/// Forgets a browser's subscription, whoever it belonged to. Only that browser knows its address, so having it is
/// proof enough (used when someone signs out, or a different person signs in on the same browser).
pub fn push_forget(c: &Connection, endpoint: &str) -> rusqlite::Result<usize> {
    c.execute("DELETE FROM push_subscriptions WHERE endpoint=?1", [endpoint])
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
    let count = |status: &str| -> rusqlite::Result<i64> { c.query_row("SELECT COUNT(*) FROM member_cards WHERE member_id=?1 AND kind='sale' AND status=?2", rusqlite::params![seller, status], |r| r.get(0)) };
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
                "reserved" => format!("{title} ordered by {detail}"),
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

// ---------- applying to sell ----------

/// A trainer asking to open a store: their words and photos (proof they sell honestly), the chat with the vault's team
/// the whole application happens in, and the answer.
#[derive(Clone, Debug)]
pub struct Application {
    pub id: i64,
    pub member_id: i64,
    pub name: String,
    pub avatar: String,
    pub note: String,
    /// "large|small" picture pairs, one per line.
    pub photos: String,
    /// "Waiting", "Approved" or "Declined".
    pub status: String,
    /// The conversation between the trainer and the vault's team where it was sent, and answered.
    pub thread_id: i64,
    /// Why it was declined, when the admin said.
    pub reason: String,
    pub created_at: i64,
    pub decided_at: i64,
}

impl Application {
    /// (large, small) for each photo.
    pub fn pics(&self) -> Vec<(String, String)> {
        self.photos.lines().filter_map(|l| l.split_once('|')).map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }
    pub fn waiting(&self) -> bool {
        self.status == "Waiting"
    }
    pub fn when(&self) -> String {
        ago(self.created_at)
    }
    pub fn decided_when(&self) -> String {
        ago(self.decided_at)
    }
    pub fn initial(&self) -> String {
        members::initial(&self.name)
    }
}

const APPLY_COLS: &str = "a.id,a.member_id,m.name,m.avatar,a.note,a.photos,a.status,a.thread_id,a.reason,a.created_at,a.decided_at FROM seller_applications a JOIN members m ON m.id=a.member_id";

fn map_application(r: &rusqlite::Row) -> rusqlite::Result<Application> {
    Ok(Application {
        id: r.get(0)?,
        member_id: r.get(1)?,
        name: r.get(2)?,
        avatar: r.get(3)?,
        note: r.get(4)?,
        photos: r.get(5)?,
        status: r.get(6)?,
        thread_id: r.get(7)?,
        reason: r.get(8)?,
        created_at: r.get(9)?,
        decided_at: r.get(10)?,
    })
}

/// A trainer's newest application, if they ever sent one.
pub fn application_of(c: &Connection, member: i64) -> rusqlite::Result<Option<Application>> {
    c.query_row(&format!("SELECT {APPLY_COLS} WHERE a.member_id=?1 ORDER BY a.id DESC LIMIT 1"), [member], map_application).optional()
}

pub fn application(c: &Connection, id: i64) -> rusqlite::Result<Option<Application>> {
    c.query_row(&format!("SELECT {APPLY_COLS} WHERE a.id=?1"), [id], map_application).optional()
}

/// Saves an application. `photos` are (large, small) pictures already on disk.
pub fn add_application(c: &Connection, member: i64, note: &str, photos: &[(String, String)], thread: i64, at: i64) -> rusqlite::Result<i64> {
    let list = photos.iter().map(|(a, b)| format!("{a}|{b}")).collect::<Vec<_>>().join("\n");
    c.execute(
        "INSERT INTO seller_applications(member_id, note, photos, thread_id, created_at) VALUES(?1,?2,?3,?4,?5)",
        rusqlite::params![member, note, list, thread, at],
    )?;
    Ok(c.last_insert_rowid())
}

/// The applications for the admins: every one still waiting (oldest first, so nobody is left behind), then the latest answered.
pub fn applications(c: &Connection, answered: i64) -> rusqlite::Result<(Vec<Application>, Vec<Application>)> {
    let waiting = c.prepare(&format!("SELECT {APPLY_COLS} WHERE a.status='Waiting' AND m.blocked=0 ORDER BY a.id"))?.query_map([], map_application)?.collect::<Result<_, _>>()?;
    let done = c.prepare(&format!("SELECT {APPLY_COLS} WHERE a.status<>'Waiting' ORDER BY a.decided_at DESC LIMIT ?1"))?.query_map([answered], map_application)?.collect::<Result<_, _>>()?;
    Ok((waiting, done))
}

/// How many applications wait for an answer: the number on the admins' Sellers tab.
pub fn applications_waiting(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM seller_applications a JOIN members m ON m.id=a.member_id WHERE a.status='Waiting' AND m.blocked=0", [], |r| r.get(0))
}

/// An admin answers. Approving switches selling on (and shows their profile, since the admin has looked at them).
/// Only a waiting application can be answered; returns it as it is now, or None if it was answered already.
pub fn decide_application(c: &Connection, id: i64, approve: bool, reason: &str, by: i64, at: i64) -> rusqlite::Result<Option<Application>> {
    let status = if approve { "Approved" } else { "Declined" };
    let n = c.execute(
        "UPDATE seller_applications SET status=?2, reason=?3, decided_by=?4, decided_at=?5 WHERE id=?1 AND status='Waiting'",
        rusqlite::params![id, status, members::clean_line(reason, 300), by, at],
    )?;
    if n == 0 {
        return Ok(None);
    }
    let Some(a) = application(c, id)? else { return Ok(None) };
    if approve {
        members::set_seller(c, a.member_id, true)?;
        members::set_profile_ok(c, a.member_id, true)?;
    }
    Ok(Some(a))
}

/// "just now", "5 min ago", "3 h ago", "2 d ago", or the date.
pub fn ago(t: i64) -> String {
    let s = (now() - t).max(0);
    if s < 60 {
        "Just now".into()
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
    let on_sale: i64 = c.query_row("SELECT COUNT(*) FROM member_cards WHERE member_id=?1 AND kind='sale' AND status IN ('Active','Reserved')", [seller], |r| r.get(0))?;
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
        if !m.seller {
            continue;
        }
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

// ---------- the cart: orders from the vault's own cards ----------

/// One line of a vault order, as it was when ordered.
#[derive(Clone, Debug, Serialize)]
pub struct OrderItem {
    pub product_id: i64,
    pub title: String,
    pub price: f64,
    pub qty: i64,
    pub image: String,
    pub handle: String,
}

impl OrderItem {
    pub fn price_text(&self) -> String {
        vault_search::peso(self.price)
    }
    pub fn line_text(&self) -> String {
        vault_search::peso(self.price * self.qty as f64)
    }
}

/// An order for the vault's own cards: the buyer, the lines, where it stands.
#[derive(Clone, Debug, Serialize)]
pub struct VaultOrder {
    pub id: i64,
    pub buyer_id: i64,
    pub buyer_name: String,
    pub status: String,
    pub message: String,
    pub paid_via: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub items: Vec<OrderItem>,
    /// When the buyer said "I've paid" (0: not yet).
    pub buyer_paid_at: i64,
}

impl VaultOrder {
    /// How it was paid, as shown (proper case).
    pub fn paid_via_text(&self) -> String {
        members::proper(&self.paid_via)
    }
    /// When the buyer said they paid, in words ("" when they haven't).
    pub fn buyer_paid_text(&self) -> String {
        if self.buyer_paid_at > 0 { ago(self.buyer_paid_at) } else { String::new() }
    }
    pub fn total(&self) -> f64 {
        self.items.iter().map(|i| i.price * i.qty as f64).sum()
    }
    pub fn total_text(&self) -> String {
        vault_search::peso(self.total())
    }
    pub fn units(&self) -> i64 {
        self.items.iter().map(|i| i.qty).sum()
    }
    pub fn when(&self) -> String {
        ago(self.created_at)
    }
    pub fn is_open(&self) -> bool {
        matches!(self.status.as_str(), "Pending" | "Confirmed")
    }
    pub fn buyer_line(&self) -> &'static str {
        match self.status.as_str() {
            "Pending" => "Waiting for the vault to accept.",
            "Confirmed" => "Accepted. Pay the vault, then it's marked paid.",
            "Paid" => "Paid. Thank you!",
            "Declined" => "The vault declined this order.",
            "Cancelled" => "Cancelled.",
            _ => "",
        }
    }
}

fn order_items(c: &Connection, order_id: i64) -> rusqlite::Result<Vec<OrderItem>> {
    let v = c
        .prepare("SELECT product_id, title, price, qty, image, handle FROM vault_order_items WHERE order_id=?1 ORDER BY id")?
        .query_map([order_id], |r| Ok(OrderItem { product_id: r.get(0)?, title: r.get(1)?, price: r.get(2)?, qty: r.get(3)?, image: r.get(4)?, handle: r.get(5)? }))?
        .collect::<Result<_, _>>()?;
    Ok(v)
}

fn map_vorder(c: &Connection, r: &Row) -> rusqlite::Result<VaultOrder> {
    let id: i64 = r.get(0)?;
    Ok(VaultOrder { id, buyer_id: r.get(1)?, buyer_name: r.get(2)?, status: r.get(3)?, message: r.get(4)?, paid_via: r.get(5)?, created_at: r.get(6)?, updated_at: r.get(7)?, items: order_items(c, id)?, buyer_paid_at: r.get(8)? })
}

const VORDER: &str = "o.id, o.buyer_id, COALESCE(b.name, 'A member who left'), o.status, o.message, o.paid_via, o.created_at, o.updated_at, o.buyer_paid_at FROM vault_orders o LEFT JOIN members b ON b.id=o.buyer_id";

pub fn vault_order(c: &Connection, id: i64) -> rusqlite::Result<Option<VaultOrder>> {
    let row = c.query_row(&format!("SELECT {VORDER} WHERE o.id=?1"), [id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?, r.get::<_, i64>(6)?, r.get::<_, i64>(7)?, r.get::<_, i64>(8)?))).optional()?;
    let Some((id, buyer_id, buyer_name, status, message, paid_via, created_at, updated_at, buyer_paid_at)) = row else { return Ok(None) };
    Ok(Some(VaultOrder { id, buyer_id, buyer_name, status, message, paid_via, created_at, updated_at, items: order_items(c, id)?, buyer_paid_at }))
}

fn vault_orders_where(c: &Connection, wh: &str, arg: i64) -> rusqlite::Result<Vec<VaultOrder>> {
    let ids: Vec<i64> = c.prepare(&format!("SELECT o.id FROM vault_orders o WHERE {wh} ORDER BY (o.status IN ('Pending','Confirmed')) DESC, o.created_at DESC LIMIT 300"))?.query_map([arg], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let _ = map_vorder;
    ids.into_iter().filter_map(|id| vault_order(c, id).transpose()).collect()
}

pub fn vault_orders_of_buyer(c: &Connection, buyer: i64) -> rusqlite::Result<Vec<VaultOrder>> {
    vault_orders_where(c, "o.buyer_id=?1", buyer)
}

pub fn vault_orders_all(c: &Connection) -> rusqlite::Result<Vec<VaultOrder>> {
    vault_orders_where(c, "?1=1", 1)
}

pub fn vault_orders_open(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM vault_orders WHERE status IN ('Pending','Confirmed')", [], |r| r.get(0))
}

/// A buyer orders some of the vault's cards: (product id, how many). Prices and names are taken from the vault, not the browser.
/// Err(text) says what's wrong; a line the vault can't fill fails the whole order, so nothing is half-ordered.
/// Limits on vault orders, which hold the vault's copies while they wait.
const VAULT_ORDER_LINES: usize = 30;
const VAULT_ORDER_COPIES: i64 = 60;
const VAULT_OPEN_ORDERS: i64 = 3;
/// A vault order nobody answered lets its copies go after this many days.
pub const VAULT_ORDER_DAYS: i64 = 7;

pub fn place_vault_order(c: &Connection, buyer: i64, lines: &[(i64, i64)], message: &str) -> rusqlite::Result<Result<i64, String>> {
    if lines.is_empty() {
        return Ok(Err("Your cart is empty.".into()));
    }
    // An order holds the vault's copies while it waits, so one buyer can't hold too many at once.
    if lines.len() > VAULT_ORDER_LINES || lines.iter().map(|(_, q)| (*q).clamp(1, 99)).sum::<i64>() > VAULT_ORDER_COPIES {
        return Ok(Err(format!("That's a lot for one order. Please send up to {VAULT_ORDER_COPIES} cards at a time.")));
    }
    // From the check to the order itself under the write lock: two buyers of the last copy can't both get it.
    locked(c, || {
        let open: i64 = c.query_row("SELECT COUNT(*) FROM vault_orders WHERE buyer_id=?1 AND status IN ('Pending','Confirmed')", [buyer], |r| r.get(0))?;
        if open >= VAULT_OPEN_ORDERS {
            return Ok(Err("You have three vault orders open already. Finish or cancel one first.".into()));
        }
        let mut items = vec![];
        for (pid, qty) in lines {
            let qty = (*qty).clamp(1, 99);
            // The copy in the cart may have sold: another copy of the same card that's still for sale takes its place.
            let pid = crate::db::sale_copy(c, *pid)?.unwrap_or(*pid);
            let Some(p) = crate::db::product_by_id(c, pid)? else { return Ok(Err("A card in your cart isn't in the vault any more.".into())) };
            if !p.available || p.price <= 0.0 {
                return Ok(Err(format!("{} isn't for sale right now.", p.short_title())));
            }
            items.push(OrderItem { product_id: pid, title: p.short_title(), price: p.price, qty, image: p.img(360), handle: p.handle.clone() });
        }
        // Copies in other people's open orders are spoken for.
        if let Some((left, title)) = vault_short(c, &items, 0)? {
            return Ok(Err(format!("Only {left} of {title} {} left.", if left == 1 { "is" } else { "are" })));
        }
        let t = now();
        c.execute("INSERT INTO vault_orders(buyer_id, status, message, created_at, updated_at) VALUES(?1,'Pending',?2,?3,?3)", rusqlite::params![buyer, members::clean_text(message, 300), t])?;
        let id = c.last_insert_rowid();
        for i in &items {
            c.execute("INSERT INTO vault_order_items(order_id, product_id, title, price, qty, image, handle) VALUES(?1,?2,?3,?4,?5,?6,?7)", rusqlite::params![id, i.product_id, i.title, i.price, i.qty, i.image, i.handle])?;
        }
        let (name, _) = names(c, buyer, 0)?;
        let total: f64 = items.iter().map(|i| i.price * i.qty as f64).sum();
        let admins: Vec<i64> = c.prepare("SELECT id FROM members WHERE role='admin' AND blocked=0")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        for a in admins {
            notify_from(c, a, buyer, "order", "", &format!("{name} ordered {} card{} from the vault for {}.", items.iter().map(|i| i.qty).sum::<i64>(), if items.len() == 1 && items[0].qty == 1 { "" } else { "s" }, vault_search::peso(total)), "/admin/orders")?;
        }
        Ok(Ok(id))
    })
}

/// Which lines of a cart the vault can't fill right now, as (product id, copies left, why), so the cart can
/// mark just those lines and the buyer can fix them. Lines that are copies of the same card are added up.
/// Empty when every line is fine (the order may still fail as a whole, e.g. too many open orders).
pub fn vault_short_lines(c: &Connection, lines: &[(i64, i64)]) -> rusqlite::Result<Vec<(i64, i64, String)>> {
    let mut out = vec![];
    let mut want: Vec<(String, i64)> = vec![];
    for (sent, qty) in lines.iter().take(VAULT_ORDER_LINES * 2) {
        let qty = (*qty).clamp(1, 99);
        // Checked as the order is: a sold copy counts as another copy of the same card still for sale. The line is
        // reported under the id the browser sent, so the cart can find it.
        let pid = &crate::db::sale_copy(c, *sent)?.unwrap_or(*sent);
        let Some(p) = crate::db::product_by_id(c, *pid)? else {
            out.push((*sent, 0, "No longer in the vault".to_string()));
            continue;
        };
        if !p.available || p.price <= 0.0 {
            out.push((*sent, 0, "Not for sale right now".to_string()));
            continue;
        }
        let key = crate::db::copy_key(c, *pid)?.unwrap_or_else(|| format!("gone:{pid}"));
        let n = match want.iter_mut().find(|w| w.0 == key) {
            Some(w) => {
                w.1 += qty;
                w.1
            }
            None => {
                want.push((key, qty));
                qty
            }
        };
        let left = crate::db::vault_left(c, *pid, 0)?;
        if left < n {
            let why = if left <= 0 { "All copies are spoken for".to_string() } else { format!("Only {left} left") };
            out.push((*sent, left, why));
        }
    }
    Ok(out)
}

/// The first line the vault can't fill now, as (copies left, card name). Copies in other open orders are spoken for;
/// `except` is the order being answered, so it doesn't hold copies against itself (0 = a new order).
/// Lines that are copies of the same card are added up.
fn vault_short(c: &Connection, items: &[OrderItem], except: i64) -> rusqlite::Result<Option<(i64, String)>> {
    let mut want: Vec<(String, i64)> = vec![];
    for i in items {
        let key = crate::db::copy_key(c, i.product_id)?.unwrap_or_else(|| format!("gone:{}", i.product_id));
        let n = match want.iter_mut().find(|w| w.0 == key) {
            Some(w) => {
                w.1 += i.qty;
                w.1
            }
            None => {
                want.push((key, i.qty));
                i.qty
            }
        };
        let left = crate::db::vault_left(c, i.product_id, except)?;
        if left < n {
            return Ok(Some((left, i.title.clone())));
        }
    }
    Ok(None)
}

/// The vault answers an order: "Confirmed" (pay now), "Paid" (done: those copies are sold), "Declined".
/// Accepting or marking paid checks the copies are still there; if not, it says which card ran out and changes nothing.
pub fn answer_vault_order(c: &Connection, id: i64, status: &str, paid_via: &str) -> rusqlite::Result<Result<VaultOrder, String>> {
    // Under the write lock, so accepting one order and marking another paid can't both pass the copies check.
    locked(c, || answer_vault_order_now(c, id, status, paid_via))
}

fn answer_vault_order_now(c: &Connection, id: i64, status: &str, paid_via: &str) -> rusqlite::Result<Result<VaultOrder, String>> {
    let Some(o) = vault_order(c, id)? else { return Ok(Err("No such order.".into())) };
    let ok = matches!((o.status.as_str(), status), ("Pending", "Confirmed") | ("Pending", "Declined") | ("Pending", "Paid") | ("Confirmed", "Paid") | ("Confirmed", "Declined"));
    if !ok {
        return Ok(Err("That order can't change like that any more.".into()));
    }
    if status == "Confirmed" || status == "Paid" {
        if let Some((left, title)) = vault_short(c, &o.items, id)? {
            return Ok(Err(format!("Only {left} left of {title}. Decline or change the order.")));
        }
    }
    let t = now();
    if status == "Paid" {
        // Every copy must really come off the shelf, or nothing does.
        c.execute_batch("SAVEPOINT vault_paid")?;
        let took = (|| -> rusqlite::Result<Option<(i64, String)>> {
            for i in &o.items {
                let n = crate::db::mark_sold(c, i.product_id, i.qty)? as i64;
                if n < i.qty {
                    return Ok(Some((n, i.title.clone())));
                }
            }
            Ok(None)
        })();
        match took {
            Ok(None) => c.execute_batch("RELEASE vault_paid")?,
            Ok(Some((left, title))) => {
                c.execute_batch("ROLLBACK TO vault_paid; RELEASE vault_paid")?;
                return Ok(Err(format!("Only {left} left of {title}. Decline or change the order.")));
            }
            Err(e) => {
                c.execute_batch("ROLLBACK TO vault_paid; RELEASE vault_paid").ok();
                return Err(e);
            }
        }
    }
    c.execute("UPDATE vault_orders SET status=?2, updated_at=?3, paid_via=?4 WHERE id=?1", rusqlite::params![id, status, t, members::clean_line(paid_via, 40)])?;
    match status {
        "Paid" => {
            notify(c, o.buyer_id, "my_order", &format!("The vault marked your order of {} as paid. Enjoy your cards! Tap to vouch for the vault.", o.units()), "/store/vault#vouches")?;
        }
        "Confirmed" => notify(c, o.buyer_id, "my_order", &format!("The vault accepted your order ({}). Time to pay.", o.total_text()), "/me/orders?tab=buying")?,
        _ => notify(c, o.buyer_id, "my_order", "The vault declined your order.", "/me/orders?tab=buying")?,
    }
    Ok(Ok(vault_order(c, id)?.unwrap_or(o)))
}

pub fn cancel_vault_order(c: &Connection, buyer: i64, id: i64) -> rusqlite::Result<bool> {
    let n = c.execute("UPDATE vault_orders SET status='Cancelled', updated_at=?3 WHERE id=?1 AND buyer_id=?2 AND status IN ('Pending','Confirmed')", rusqlite::params![id, buyer, now()])?;
    Ok(n == 1)
}

/// The vault's own store as its admins set it in Vault Manager › Store settings (19 Nov 2026): a cover photo, its words
/// and its links, the same as a seller's store has. Kept in `settings`; how buyers pay and the border colour have their
/// own keys (`vault_pay`, `vault_border`).
#[derive(Clone, Debug, Default)]
pub struct VaultStore {
    pub cover: String,
    pub bio: String,
    pub fb_url: String,
    pub fb_group_url: String,
    pub ig_url: String,
    pub tiktok_url: String,
    pub shopee_url: String,
    pub links: String,
}

impl VaultStore {
    /// Its links as (label, address, the address in short), as a seller's store shows them.
    pub fn social_links(&self) -> Vec<(String, String, String)> {
        let mut v: Vec<(String, String, String)> = [("Facebook", &self.fb_url), ("Facebook Group", &self.fb_group_url), ("Instagram", &self.ig_url), ("TikTok", &self.tiktok_url), ("Shopee", &self.shopee_url)]
            .into_iter()
            .filter(|(_, u)| !u.is_empty())
            .map(|(l, u)| (l.to_string(), u.clone(), members::short_link(u)))
            .collect();
        for line in self.links.lines().filter(|l| !l.trim().is_empty()) {
            let (label, url) = link_parts(line);
            if url.starts_with("https://") {
                let s = members::short_link(&url);
                v.push((label, url, s));
            }
        }
        v
    }
}

pub fn vault_store(c: &Connection) -> rusqlite::Result<VaultStore> {
    let get = |k: &str| -> rusqlite::Result<String> { Ok(crate::blocks::setting(c, k)?.unwrap_or_default()) };
    Ok(VaultStore {
        cover: get("vault_cover")?,
        bio: get("vault_bio")?,
        fb_url: get("vault_fb_url")?,
        fb_group_url: get("vault_fb_group_url")?,
        ig_url: get("vault_ig_url")?,
        tiktok_url: get("vault_tiktok_url")?,
        shopee_url: get("vault_shopee_url")?,
        links: get("vault_links")?,
    })
}

/// Saves the vault store's words and links, cleaned the way a seller's are (links must be https://). The cover photo is
/// saved by itself (`set_vault_cover`), once the picture is stored.
pub fn set_vault_store(c: &Connection, s: &VaultStore) -> rusqlite::Result<()> {
    let set = |k: &str, v: String| crate::blocks::set_setting(c, k, &v);
    set("vault_bio", members::clean_text(&s.bio, 300))?;
    set("vault_fb_url", members::clean_https_url(&s.fb_url))?;
    set("vault_fb_group_url", members::clean_https_url(&s.fb_group_url))?;
    set("vault_ig_url", members::clean_https_url(&s.ig_url))?;
    set("vault_tiktok_url", members::clean_https_url(&s.tiktok_url))?;
    set("vault_shopee_url", members::clean_https_url(&s.shopee_url))?;
    set("vault_links", clean_link_lines(&s.links))
}

pub fn set_vault_cover(c: &Connection, path: &str) -> rusqlite::Result<()> {
    crate::blocks::set_setting(c, "vault_cover", path)
}

/// How buyers pay the vault: (ways to pay, how to pay), from the admin's settings.
pub fn vault_pay(c: &Connection) -> rusqlite::Result<(String, String)> {
    Ok((crate::blocks::setting(c, "vault_pay_methods")?.unwrap_or_default(), crate::blocks::setting(c, "vault_pay_info")?.unwrap_or_default()))
}

pub fn set_vault_pay(c: &Connection, methods: &str, info: &str) -> rusqlite::Result<()> {
    let m: Vec<&str> = methods.split(',').map(str::trim).filter(|x| PAY_METHODS.contains(x)).collect();
    crate::blocks::set_setting(c, "vault_pay_methods", &m.join(","))?;
    crate::blocks::set_setting(c, "vault_pay_info", &members::clean_text(info, 600))
}

/// A cart line as the drawer should show it now: title, price, picture, whether it can still be bought.
#[derive(Clone, Debug, Serialize)]
pub struct CartLine {
    pub kind: &'static str,
    pub id: i64,
    pub title: String,
    pub price: f64,
    pub image: String,
    pub url: String,
    pub ok: bool,
    pub why: String,
    pub seller_id: i64,
    pub seller_name: String,
    pub seller_slug: String,
    pub left: i64,
}

pub fn cart_lines(c: &Connection, vault_ids: &[i64], card_ids: &[i64]) -> rusqlite::Result<Vec<CartLine>> {
    let mut out = vec![];
    // The vault's cards come from its store like any other: its name (the site's) and /store/vault.
    let vault_name = crate::blocks::setting(c, "brand")?.map(|b| b.trim().to_string()).filter(|b| !b.is_empty()).unwrap_or_else(|| "Big Boys Vault".into());
    for id in vault_ids.iter().take(60) {
        // The copy in the cart may have sold while identical copies are still for sale: the line stands for those.
        let now_id = crate::db::sale_copy(c, *id)?.unwrap_or(*id);
        match crate::db::product_by_id(c, now_id)? {
            Some(p) => {
                // Copies in open orders are spoken for.
                let left = crate::db::vault_left(c, now_id, 0)?;
                let ok = p.available && p.price > 0.0 && left > 0;
                let why = if ok { "" } else if p.available && p.price > 0.0 { "All copies are spoken for" } else { "Not for sale right now" };
                out.push(CartLine { kind: "vault", id: *id, title: p.short_title(), price: p.price, image: p.img(360), url: format!("/products/{}", p.handle), ok, why: why.into(), seller_id: 0, seller_name: vault_name.clone(), seller_slug: VAULT_SLUG.into(), left });
            }
            None => out.push(CartLine { kind: "vault", id: *id, title: "A card that's gone".into(), price: 0.0, image: String::new(), url: "/".into(), ok: false, why: "No longer in the vault".into(), seller_id: 0, seller_name: String::new(), seller_slug: String::new(), left: 0 }),
        }
    }
    for id in card_ids.iter().take(60) {
        let row = c
            .query_row(&format!("SELECT {}, COALESCE(NULLIF(TRIM(m.store_name), ''), m.name), m.slug, m.status, m.seller, m.blocked, m.profile_ok, m.store_hidden FROM member_cards c JOIN members m ON m.id=c.member_id WHERE c.id=?1", members::CARD_COLS), [id], |r| {
                let closed = r.get::<_, i64>(members::CARD_N + 4)? != 0 || r.get::<_, i64>(members::CARD_N + 5)? == 0 || r.get::<_, i64>(members::CARD_N + 6)? != 0;
                Ok((members::map_card(r, 0)?, r.get::<_, String>(members::CARD_N)?, r.get::<_, String>(members::CARD_N + 1)?, r.get::<_, String>(members::CARD_N + 2)?, r.get::<_, i64>(members::CARD_N + 3)?, closed))
            })
            .optional()?;
        match row {
            // A blocked, not-yet-approved or hidden store is hidden everywhere else, so the cart shows nothing about it either:
            // no card name, picture, price or seller. Only the id the browser sent, and why.
            Some((_, _, _, _, _, true)) => out.push(CartLine { kind: "card", id: *id, title: "A card from a closed store".into(), price: 0.0, image: String::new(), url: "/stores".into(), ok: false, why: "This store is closed.".into(), seller_id: 0, seller_name: String::new(), seller_slug: String::new(), left: 0 }),
            Some((k, name, slug, status, selling, _)) => {
                let left = k.qty - held(c, k.id)?;
                let why = if selling == 0 { "This trainer isn't selling" } else if !k.is_sale() { "Not for sale" } else if status == "Vacation" { "The seller is on vacation" } else if k.status == "Out of stock" { "Out of stock" } else if !k.buyable() && k.status != "Reserved" { "Not for sale right now" } else if left <= 0 { "All copies are spoken for" } else { "" };
                out.push(CartLine { kind: "card", id: k.id, title: k.title.clone(), price: k.worth(), image: k.image.clone(), url: format!("/trainers/{}/cards/{}", k.member_id, k.id), ok: why.is_empty() && k.worth() > 0.0, why: if why.is_empty() && k.worth() <= 0.0 { "No price yet: make an offer instead".into() } else { why.into() }, seller_id: k.member_id, seller_name: name, seller_slug: slug, left: left.max(0) });
            }
            None => out.push(CartLine { kind: "card", id: *id, title: "A card that's gone".into(), price: 0.0, image: String::new(), url: "/stores".into(), ok: false, why: "No longer listed".into(), seller_id: 0, seller_name: String::new(), seller_slug: String::new(), left: 0 }),
        }
    }
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
        crate::db::register_set_code(&c).unwrap();
        c.execute_batch(crate::db::SCHEMA).unwrap();
        c.execute_batch(crate::blocks::SCHEMA).unwrap();
        c.execute_batch(members::SCHEMA).unwrap();
        members::migrate(&c).unwrap();
        crate::db::migrate(&c).unwrap();
        c
    }

    #[test]
    fn a_store_keeps_its_featured_binders_and_cards_in_order() {
        let c = db();
        assert_eq!(featured(&c, 7).unwrap(), None, "nothing chosen yet");
        let f = Featured { binders: vec!["3".into(), "1".into(), "3".into(), " ".into()], cards: (1..=20).map(|n| n.to_string()).collect() };
        set_featured(&c, 7, &f).unwrap();
        let got = featured(&c, 7).unwrap().unwrap();
        assert_eq!(got.binders, vec!["3", "1"], "the order given, each once, no blanks");
        assert_eq!(got.cards.len(), FEATURE_MAX);
        assert_eq!(got.cards[0], "1");
        assert_eq!(got.cards[FEATURE_MAX - 1], FEATURE_MAX.to_string());
        set_featured(&c, 7, &Featured::default()).unwrap();
        assert_eq!(featured(&c, 7).unwrap(), Some(Featured::default()), "an empty choice is a choice: no binders on the tile");
        assert_eq!(featured(&c, 0).unwrap(), None, "the vault's choice is its own");
        set_featured(&c, 0, &Featured { binders: vec!["pikachu-set".into()], cards: vec![] }).unwrap();
        assert_eq!(featured(&c, 0).unwrap().unwrap().binders, vec!["pikachu-set"]);
        assert_eq!(featured(&c, 7).unwrap(), Some(Featured::default()));
    }

    fn seller_with_card(c: &Connection) -> (i64, i64, i64) {
        let s = create(c, "Shop", "h").unwrap().unwrap();
        members::set_seller(c, s, true).unwrap(); // an admin switched selling on
        members::set_profile_ok(c, s, true).unwrap(); // and approved the profile: the store is open
        let b = create(c, "Buyer", "h").unwrap().unwrap();
        let Added::Yes(card) = add_listing(c, s, "tcgdex:base1-58", "Pikachu - 058/102 - Base Set", "58/102", "Base Set", "", "").unwrap() else { panic!() };
        members::set_card_value(c, "tcgdex:base1-58", 100.0, now()).unwrap();
        set_kind(c, s, card, "sale").unwrap().unwrap();
        update_listing(c, s, card, &Patch { qty: Some(2), ..Default::default() }).unwrap();
        (s, b, card)
    }

    #[test]
    fn listings_sort_by_one_or_two_columns() {
        let c = db();
        let s = create(&c, "Shop", "h").unwrap().unwrap();
        members::set_seller(&c, s, true).unwrap();
        members::set_profile_ok(&c, s, true).unwrap();
        // title, condition, the seller's price, stock, what it is
        let cards = [("Eevee", "Near Mint", 50.0, 3, "sale"), ("Abra", "Lightly Played", 50.0, 1, "sale"), ("Zubat", "Damaged", 5.0, 9, "own"), ("Mew", "Near Mint", 900.0, 1, "sale")];
        for (i, (title, cond, price, qty, kind)) in cards.iter().enumerate() {
            let Added::Yes(id) = add_listing(&c, s, &format!("tcgdex:x-{i}"), title, "1/1", "Set", "", "").unwrap() else { panic!() };
            c.execute("UPDATE member_cards SET cond=?2, value_own=?3, qty=?4, kind=?5 WHERE id=?1", rusqlite::params![id, cond, price, qty, kind]).unwrap();
        }
        let by = |sort: &str| listings(&c, s, &ListFilter { sort: sort.into(), ..Default::default() }).unwrap().iter().map(|x| x.title.chars().next().unwrap()).collect::<String>();
        assert_eq!(by("price:desc,card:asc"), "MAEZ", "dearest first; the two at 50 by name");
        assert_eq!(by("price:asc,stock:desc"), "ZEAM", "cheapest first; of the two at 50, the one with more stock");
        assert_eq!(by("cond:asc,price:desc"), "MEAZ", "best condition first, the Near Mints by price");
        assert_eq!(by("stock:desc"), "ZEAM");
        assert_eq!(by("kind:asc,card:asc"), "AEMZ", "for sale first, then what is only shown off");
        assert_eq!(by("card:desc"), "ZMEA");
        assert_eq!(by("nonsense:desc"), by(""), "an unknown column leaves the seller's own order");
        assert!(by("price-desc").starts_with('M') && by("price-desc").ends_with('Z'), "the named orders still work");
    }

    #[test]
    fn the_vault_page_shows_only_store_cards_that_can_be_bought_now() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        let read = || vault_search::parse_market(&market_lines(&c).unwrap());
        let lids = || read().2.iter().map(|k| k.lid).collect::<Vec<_>>();
        // More cards in the same store: each `set` is one way of being, or not being, for sale right now.
        let list = |n: &str, set: &str| -> i64 {
            let Added::Yes(id) = add_listing(&c, s, &format!("tcgdex:x-{n}"), &format!("Card {n}"), "1", "Set", "", "").unwrap() else { panic!() };
            c.execute(&format!("UPDATE member_cards SET kind='sale', status='Active', qty=1, {set} WHERE id=?1"), [id]).unwrap();
            id
        };
        let later = list("later", &format!("expires_at={}", now() + 86_400));
        let _ended = list("ended", &format!("expires_at={}", now() - 60));
        let _own = list("own", "kind='own'");
        let _wish = list("wish", "kind='wish'");
        let _out = list("out", "status='Out of stock'");
        let _hidden = list("hidden", "status='Hidden'");
        let _none = list("none", "qty=0");
        let photo = list("photo", "card_id='own:photo-1', value_own=250");
        let unpriced = list("unpriced", "value=0, value_own=0, finish='reverseHolofoil', cond='PSA 10'");
        assert_eq!(lids(), vec![unpriced, photo, later, card], "newest first; ended, not for sale, out of stock, hidden and sold out are left out");
        let (_, stores, cards) = read();
        assert_eq!(stores.iter().map(|x| (x.id, x.title.as_str(), x.slug.as_str(), x.count)).collect::<Vec<_>>(), vec![(s, "Shop", "shop", 4)], "a store goes by its address");
        assert!(cards.iter().all(|k| k.store == 1));
        let k = |id: i64| cards.iter().find(|x| x.lid == id).unwrap().clone();
        assert_eq!((k(card).qty, k(card).price > 0.0, k(card).title.as_str()), (2, true, "Pikachu - 058/102 - Base Set"));
        assert_eq!(k(photo).price, 250.0, "an own photo for sale is in, at the seller's price");
        let u = k(unpriced);
        assert_eq!((u.price, u.grade.as_str(), u.finish.as_str(), u.variant.as_str()), (0.0, "PSA 10", "Reverse Holo", "PSA 10 · Reverse Holo"), "no price yet: the finder asks for one");
        // Copies held by open reservations aren't for sale; with every copy held the card is Reserved and leaves the page.
        reserve(&c, b, s, card, 1, "").unwrap().unwrap();
        assert_eq!(k(card).qty, 2, "the earlier read");
        assert_eq!(read().2.iter().find(|x| x.lid == card).unwrap().qty, 1, "one copy left after the reservation");
        let b2 = create(&c, "Buyer Two", "h").unwrap().unwrap();
        reserve(&c, b2, s, card, 1, "").unwrap().unwrap();
        assert!(!lids().contains(&card), "every copy is spoken for");
        // A second store, then each way a store isn't open for business.
        let m = create(&c, "Misty", "h").unwrap().unwrap();
        members::set_seller(&c, m, true).unwrap();
        members::set_profile_ok(&c, m, true).unwrap();
        c.execute("UPDATE members SET store_name='Cerulean Gym', slug='cerulean' WHERE id=?1", [m]).unwrap();
        let Added::Yes(gym) = add_listing(&c, m, "tcgdex:x-gym", "Staryu", "1", "Set", "", "").unwrap() else { panic!() };
        let (_, stores, _) = read();
        assert_eq!(stores.iter().map(|x| x.title.as_str()).collect::<Vec<_>>(), vec!["Shop", "Cerulean Gym"], "the store with the most cards first");
        for (shut, open) in [("status='Vacation'", "status='Active'"), ("blocked=1", "blocked=0"), ("profile_ok=0", "profile_ok=1"), ("seller=0", "seller=1"), ("store_hidden=1", "store_hidden=0")] {
            c.execute(&format!("UPDATE members SET {shut} WHERE id=?1"), [s]).unwrap();
            let (_, stores, cards) = read();
            assert_eq!(stores.iter().map(|x| x.id).collect::<Vec<_>>(), vec![m], "{shut}: the store is left out");
            assert_eq!(cards.iter().map(|x| x.lid).collect::<Vec<_>>(), vec![gym], "{shut}: and so are its cards");
            c.execute(&format!("UPDATE members SET {open} WHERE id=?1"), [s]).unwrap();
        }
        assert_eq!(lids(), vec![gym, unpriced, photo, later]);
        // The vault's own catalogue reader never sees any of it.
        assert!(vault_search::parse(&market_lines(&c).unwrap()).1.is_empty());
    }

    #[test]
    fn more_like_a_store_card_is_for_sale_now_and_never_the_card_itself() {
        let c = db();
        let (s, b, card) = seller_with_card(&c); // "Pikachu - 058/102 - Base Set", ₱100 at the market, 2 in stock
        // The vault: two cards for sale and one that isn't.
        for (id, title, price, on) in [(1, "Pikachu - 25/102 - Base Set", 120.0, 1), (2, "Mewtwo - 10/102 - Base Set", 4000.0, 1), (3, "Pikachu - 27/102 - Base Set", 110.0, 0)] {
            c.execute(
                "INSERT INTO products(id,handle,title,variant,price,available,card_set,created_at) VALUES(?1,?2,?3,'Near Mint',?4,?5,'Base Set','2026-11-01T00:00:00')",
                rusqlite::params![id, format!("p-{id}"), title, price, on],
            )
            .unwrap();
        }
        // A second store, open, with a Pikachu for sale and every way of a Pikachu not being for sale right now.
        let m = create(&c, "Misty", "h").unwrap().unwrap();
        members::set_seller(&c, m, true).unwrap();
        members::set_profile_ok(&c, m, true).unwrap();
        c.execute("UPDATE members SET store_name='Cerulean Gym', slug='cerulean' WHERE id=?1", [m]).unwrap();
        let list = |who: i64, n: &str, set: &str| -> i64 {
            let Added::Yes(id) = add_listing(&c, who, &format!("tcgdex:base1-{n}"), &format!("Pikachu - {n}/102 - Base Set"), &format!("{n}/102"), "Base Set", "/p.png", "").unwrap() else { panic!() };
            c.execute(&format!("UPDATE member_cards SET kind='sale', status='Active', qty=1, value_own=150, {set} WHERE id=?1"), [id]).unwrap();
            id
        };
        let gym = list(m, "60", "cond='Near Mint'");
        let mut off = vec![];
        for (n, why) in [("61", "status='Hidden'"), ("62", "status='Expired'"), ("63", "status='Sold'"), ("64", "status='Out of stock'"), ("65", "qty=0"), ("66", "kind='own'"), ("67", "kind='wish'"), ("68", "value_own=0, value=0")] {
            off.push((list(m, n, why), why));
        }
        off.push((list(m, "69", &format!("expires_at={}", now() - 60)), "ended"));
        let held = list(m, "70", "cond='Near Mint'");
        reserve(&c, b, m, held, 1, "").unwrap().unwrap();
        off.push((held, "every copy held for someone"));
        // A copy of the same card in the seller's own store is another listing: it may show. The card itself never does.
        let again = list(s, "58", "cond='Lightly Played'");
        let me = members::card(&c, s, card).unwrap().unwrap();
        let got = || -> Vec<(char, i64)> {
            related(&c, &me, 12).unwrap().iter().map(|x| match x { Like::Vault(p) => ('v', p.id), Like::Store(l) => ('s', l.card.id) }).collect()
        };
        let all = got();
        assert!(all.contains(&('v', 1)) && all.contains(&('v', 2)), "the vault's cards for sale: {all:?}");
        assert!(!all.contains(&('v', 3)), "not one the vault has sold");
        assert!(all.contains(&('s', gym)) && all.contains(&('s', again)), "open stores' cards for sale: {all:?}");
        assert!(!all.contains(&('s', card)), "never the card itself");
        for (id, why) in &off {
            assert!(!all.contains(&('s', *id)), "{why}: left out");
        }
        assert_eq!(all.len(), 4);
        assert_eq!(all[0], ('s', gym), "the same name first, the nearest in set, price and condition first: {all:?}");
        assert_eq!(all[3], ('v', 2), "then cards like it: the Mewtwo");
        assert_eq!(related(&c, &me, 2).unwrap().len(), 2, "no more than asked for");
        // A store that isn't open, in each way, takes its cards with it.
        for (shut, open) in [("status='Vacation'", "status='Active'"), ("blocked=1", "blocked=0"), ("profile_ok=0", "profile_ok=1"), ("seller=0", "seller=1")] {
            c.execute(&format!("UPDATE members SET {shut} WHERE id=?1"), [m]).unwrap();
            let seen = got();
            assert!(!seen.contains(&('s', gym)), "{shut}: the closed store's card is left out");
            assert!(seen.contains(&('s', again)) && seen.contains(&('v', 1)), "{shut}: the others stay");
            c.execute(&format!("UPDATE members SET {open} WHERE id=?1"), [m]).unwrap();
        }
        // Each store's tile knows whose it is, for the cart.
        let tile = related(&c, &me, 12).unwrap().into_iter().find_map(|x| match x { Like::Store(l) if l.card.id == gym => Some(l), _ => None }).unwrap();
        assert_eq!((tile.store.title.as_str(), tile.store.slug.as_str(), tile.card.member_id), ("Cerulean Gym", "cerulean", m));
    }

    #[test]
    fn ask_shows_a_store_card_as_a_tile_only_while_it_can_be_bought_with_a_price() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        c.execute("UPDATE member_cards SET value_own=120, notes='Straight from the pack' WHERE id=?1", [card]).unwrap();
        // Ask the Vault's plain answer finds it in the same list The Vault page shows.
        let (binders, _, cards) = vault_search::parse_market(&market_lines(&c).unwrap());
        let a = crate::ask::answer("pikachu", None, &cards, &binders, &[]);
        assert_eq!(a.picks.iter().map(|&i| cards[i].lid).collect::<Vec<_>>(), vec![card], "{a:?}");
        // The tile is built from the listing itself.
        let t = ask_tile(&c, card).unwrap().expect("open, Active and priced: a tile");
        assert_eq!(t["lid"], card);
        assert_eq!(t["url"], format!("/trainers/{s}/cards/{card}"), "it opens in the card window");
        assert_eq!(t["title"], vault_search::title_and_set("Pikachu - 058/102 - Base Set", "Base Set").0);
        assert_eq!(t["full"], "Pikachu - 058/102 - Base Set");
        assert_eq!((t["price"].as_str(), t["amount"].as_f64()), (Some("₱120"), Some(120.0)), "the seller's own price");
        assert_eq!((t["seller"].as_str(), t["seller_slug"].as_str(), t["seller_id"].as_i64()), (Some("Shop"), Some("shop"), Some(s)), "whose card it is");
        assert_eq!(t["notes"], "Straight from the pack");
        assert!(t.get("id").is_none() && t.get("handle").is_none(), "nothing that reads as a vault card");
        // Every way of not being buyable right now leaves the tile out: the same checks as The Vault page.
        let gone = |set: &str, back: &str, why: &str| {
            c.execute(&format!("UPDATE member_cards SET {set} WHERE id=?1"), [card]).unwrap();
            assert_eq!(ask_tile(&c, card).unwrap(), None, "{why}");
            c.execute(&format!("UPDATE member_cards SET {back} WHERE id=?1"), [card]).unwrap();
            assert!(ask_tile(&c, card).unwrap().is_some(), "{why}: and back again");
        };
        gone("value=0, value_own=0", "value=100, value_own=120", "no price yet");
        gone("status='Hidden'", "status='Active'", "hidden by the seller");
        gone("status='Out of stock'", "status='Active'", "out of stock");
        gone("status='Reserved'", "status='Active'", "reserved");
        gone("status='Sold'", "status='Active'", "sold");
        gone("kind='own'", "kind='sale'", "only shown off");
        gone("kind='wish'", "kind='sale'", "on a wishlist");
        gone("qty=0", "qty=2", "no copies left");
        gone(&format!("expires_at={}", now() - 60), "expires_at=0", "past its end date");
        for (shut, open) in [("status='Vacation'", "status='Active'"), ("blocked=1", "blocked=0"), ("profile_ok=0", "profile_ok=1"), ("seller=0", "seller=1")] {
            c.execute(&format!("UPDATE members SET {shut} WHERE id=?1"), [s]).unwrap();
            assert_eq!(ask_tile(&c, card).unwrap(), None, "{shut}: the store is closed");
            c.execute(&format!("UPDATE members SET {open} WHERE id=?1"), [s]).unwrap();
        }
        assert!(ask_tile(&c, card).unwrap().is_some());
        assert_eq!(ask_tile(&c, 999_999).unwrap(), None, "no such listing");
        // Copies held by open reservations aren't for sale.
        reserve(&c, b, s, card, 1, "").unwrap().unwrap();
        assert_eq!(open_listing(&c, card).unwrap().map(|l| l.left), Some(1), "one copy left");
        let b2 = create(&c, "Buyer Two", "h").unwrap().unwrap();
        reserve(&c, b2, s, card, 1, "").unwrap().unwrap();
        assert_eq!(ask_tile(&c, card).unwrap(), None, "every copy is spoken for");
    }

    #[test]
    fn a_store_card_goes_by_the_day_it_was_listed() {
        let c = db();
        let (s, _, card) = seller_with_card(&c);
        let time = |id: i64| vault_search::parse_market(&market_lines(&c).unwrap()).2.iter().find(|x| x.lid == id).map(|x| x.time);
        let listed: i64 = c.query_row("SELECT created_at FROM card_events WHERE card_id=?1 AND kind='listed'", [card], |r| r.get(0)).unwrap();
        assert!(listed > 0);
        assert_eq!(time(card), Some(listed), "the moment Add a card put it up");
        // Changing it later doesn't make it new again.
        c.execute("UPDATE member_cards SET updated_at=?2 WHERE id=?1", [card, listed + 5000]).unwrap();
        log_event(&c, s, card, "stock", "2 → 3").unwrap();
        assert_eq!(time(card), Some(listed));
        // A card from before "listed" was kept: the earliest thing known about it.
        c.execute("DELETE FROM card_events WHERE card_id=?1", [card]).unwrap();
        c.execute("INSERT INTO card_events(member_id,card_id,kind,detail,created_at) VALUES(?1,?2,'stock','',?3)", [s, card, listed - 300]).unwrap();
        assert_eq!(time(card), Some(listed - 300), "its first entry, from before its last change");
        c.execute("DELETE FROM card_events WHERE card_id=?1", [card]).unwrap();
        assert_eq!(time(card), Some(listed + 5000), "no history: its last change");
        c.execute("UPDATE member_cards SET updated_at=0 WHERE id=?1", [card]).unwrap();
        c.execute("UPDATE members SET created_at=?2 WHERE id=?1", [s, 1_700_000_000]).unwrap();
        assert_eq!(time(card), Some(1_700_000_000), "nothing known about it: the day its seller joined");
    }

    #[test]
    fn store_addresses_are_checked() {
        assert_eq!(check_slug("Big-Boys"), Ok("big-boys".into()));
        assert!(check_slug("ab").is_err());
        assert!(check_slug("admin").is_err());
        assert!(check_slug("bad--name").is_err());
        assert!(check_slug("-lead").is_err());
        assert!(check_slug("has space").is_err());
        // The vault's own store lives at /store/vault: nobody else can be "vault", or pass for it.
        for taken in ["vault", "Vault", "the-vault", "big-boys-vault", "bigboysvault"] {
            assert_eq!(check_slug(taken), Err("That address is taken by the site. Pick another."), "{taken}");
        }
        assert_eq!(check_slug("vault-hunter"), Ok("vault-hunter".into()), "only the exact words are kept");
        let c = db();
        let a = create(&c, "Ash", "h").unwrap().unwrap();
        let b = create(&c, "Brock", "h").unwrap().unwrap();
        assert_eq!(set_slug(&c, a, "pallet-town").unwrap(), Ok(true));
        assert_eq!(set_slug(&c, b, "Pallet-Town").unwrap(), Ok(false), "taken whatever the case");
        assert!(set_slug(&c, b, "me").unwrap().is_err());
        assert!(set_slug(&c, b, "vault").unwrap().is_err());
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
        c.execute("UPDATE members SET status='Active', seller=0 WHERE id=?1", [s]).unwrap();
        assert!(make_offer(&c, b, s, card, 80.0, 1, "").unwrap().is_err(), "selling switched off by an admin");
        members::set_seller(&c, s, true).unwrap();
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
    fn another_copy_after_one_was_deleted_still_adds() {
        let c = db();
        let s = members::create(&c, "Shop", "h").unwrap().unwrap();
        members::set_seller(&c, s, true).unwrap();
        let ids: Vec<i64> = (0..3).map(|_| match add_listing(&c, s, "tcgdex:x-1", "Mew", "1", "Set", "/p/m.webp", "").unwrap() { members::Added::Yes(id) => id, _ => panic!() }).collect();
        c.execute("DELETE FROM member_cards WHERE id=?1", [ids[0]]).unwrap();
        assert!(matches!(add_listing(&c, s, "tcgdex:x-1", "Mew", "1", "Set", "/p/m.webp", "").unwrap(), members::Added::Yes(_)), "a free number is found");
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
    fn kinds_stock_and_the_cart() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        // A seller's existing cards are for sale; a collector's are highlights, and they can't put one up for sale.
        assert_eq!(members::card(&c, s, card).unwrap().unwrap().kind, "sale", "add_listing by a seller sells");
        assert!(set_kind(&c, b, card, "sale").unwrap().is_err(), "not their card");
        let Added::Yes(bc) = add_listing(&c, b, "tcgdex:x-9", "Mew", "9", "Set", "", "").unwrap() else { panic!() };
        assert_eq!(members::card(&c, b, bc).unwrap().unwrap().kind, "own");
        assert!(set_kind(&c, b, bc, "sale").unwrap().is_err(), "collectors can't sell");
        assert_eq!(set_kind(&c, b, bc, "wish").unwrap().unwrap().kind_label(), "Wishlist");
        // Out of stock: nobody can buy, back in stock they can.
        update_listing(&c, s, card, &Patch { status: Some("Out of stock".into()), ..Default::default() }).unwrap();
        assert!(reserve(&c, b, s, card, 1, "").unwrap().is_err());
        assert_eq!(storefront_listings(&c, s).unwrap().len(), 1, "still shown, marked out of stock");
        update_listing(&c, s, card, &Patch { status: Some("Active".into()), ..Default::default() }).unwrap();
        assert!(reserve(&c, b, s, card, 1, "").unwrap().is_ok());
        // A highlight isn't in the store.
        set_kind(&c, s, card, "own").unwrap().unwrap();
        assert!(storefront_listings(&c, s).unwrap().is_empty());
        // The vault's own cards: an order, accepted, paid; the copies are sold.
        c.execute("INSERT INTO products(id,handle,title,price,available,created_at) VALUES(1,'pika','Pikachu - 058/102 - Base Set',100,1,'2026-01-01T00:00:00'),(2,'pika-2','Pikachu - 058/102 - Base Set',100,1,'2026-01-01T00:00:00'),(3,'mew','Mew',0,1,'2026-01-01T00:00:00')", []).unwrap();
        assert!(place_vault_order(&c, b, &[(3, 1)], "").unwrap().is_err(), "no price");
        assert!(place_vault_order(&c, b, &[(1, 3)], "").unwrap().is_err(), "only two copies");
        let lines = cart_lines(&c, &[1, 3], &[card]).unwrap();
        assert!(lines[0].ok && !lines[1].ok && !lines[2].ok, "{lines:?}");
        let o = place_vault_order(&c, b, &[(1, 2)], "Meet at SM?").unwrap().unwrap();
        let lines = cart_lines(&c, &[1], &[]).unwrap();
        assert!(!lines[0].ok && lines[0].left == 0, "both copies are in the open order: {lines:?}");
        assert_eq!(vault_orders_of_buyer(&c, b).unwrap()[0].total(), 200.0);
        assert!(answer_vault_order(&c, o, "Paid", "GCash").unwrap().is_ok());
        assert_eq!(crate::db::copies_for_sale(&c, 1).unwrap(), 0, "both copies sold");
        assert!(place_vault_order(&c, b, &[(1, 1)], "").unwrap().is_err(), "sold out");
        assert!(cancel_vault_order(&c, b, o).unwrap() == false, "a paid order can't be cancelled");
    }

    #[test]
    fn the_count_moves_the_status_but_never_the_sellers_own_choice() {
        let c = db();
        let (s, b, card) = seller_with_card(&c); // two copies, Active
        let set = |p: Patch| update_listing(&c, s, card, &p).unwrap().unwrap();
        // "Out of stock" is the seller's switch: the count never moves it.
        set(Patch { status: Some("Out of stock".into()), ..Default::default() });
        assert_eq!(set(Patch { qty: Some(5), ..Default::default() }).status, "Out of stock");
        let k = set(Patch { qty: Some(0), ..Default::default() });
        assert_eq!((k.status.as_str(), k.qty), ("Out of stock", 0));
        // Switched back on with no copies: the seller has one.
        let k = set(Patch { status: Some("Active".into()), ..Default::default() });
        assert_eq!((k.status.as_str(), k.qty), ("Active", 1), "in stock means at least one copy");
        // Hidden stays hidden whatever the count.
        set(Patch { status: Some("Hidden".into()), ..Default::default() });
        assert_eq!(set(Patch { qty: Some(0), ..Default::default() }).status, "Hidden");
        assert_eq!(set(Patch { qty: Some(4), ..Default::default() }).status, "Hidden");
        set(Patch { status: Some("Active".into()), ..Default::default() });
        // Sold out at 0, back on sale when copies come in.
        assert_eq!(set(Patch { qty: Some(0), ..Default::default() }).status, "Sold");
        assert_eq!(set(Patch { qty: Some(2), ..Default::default() }).status, "Active");
        // Every copy reserved: Reserved. One more copy: Active. Back down: Reserved again.
        reserve(&c, b, s, card, 2, "").unwrap().unwrap();
        assert_eq!(members::card(&c, s, card).unwrap().unwrap().status, "Reserved");
        assert_eq!(set(Patch { qty: Some(3), ..Default::default() }).status, "Active");
        assert_eq!(set(Patch { qty: Some(2), ..Default::default() }).status, "Reserved");
        // Switched off and on while every copy is held: still Reserved.
        set(Patch { status: Some("Out of stock".into()), ..Default::default() });
        assert_eq!(set(Patch { status: Some("Active".into()), ..Default::default() }).status, "Reserved");
    }

    #[test]
    fn renew_changes_only_the_end_date_and_brings_back_expired_cards() {
        let c = db();
        let (s, _, card) = seller_with_card(&c);
        for st in ["Hidden", "Out of stock"] {
            update_listing(&c, s, card, &Patch { status: Some(st.into()), ..Default::default() }).unwrap();
            let k = renew(&c, s, card, 30).unwrap().unwrap();
            assert_eq!(k.status, st, "renew leaves {st} alone");
            assert!(k.expires_at > now() + 29 * 86400);
        }
        // An expired card comes back; with no copies it comes back sold out.
        c.execute("UPDATE member_cards SET status='Expired', expires_at=?2, qty=0 WHERE id=?1", rusqlite::params![card, now() - 10]).unwrap();
        let k = renew(&c, s, card, 0).unwrap().unwrap();
        assert_eq!((k.status.as_str(), k.expires_at), ("Sold", 0));
        c.execute("UPDATE member_cards SET status='Expired', expires_at=?2, qty=2 WHERE id=?1", rusqlite::params![card, now() - 10]).unwrap();
        assert_eq!(renew(&c, s, card, 30).unwrap().unwrap().status, "Active");
        // Many at once: "No end date" brings expired cards back too; a date in the past doesn't.
        c.execute("UPDATE member_cards SET status='Expired', expires_at=?2 WHERE id=?1", rusqlite::params![card, now() - 10]).unwrap();
        assert_eq!(bulk_update(&c, s, &[card], &Patch { expires_at: Some(now() - 5), ..Default::default() }).unwrap(), 1);
        assert_eq!(members::card(&c, s, card).unwrap().unwrap().status, "Expired");
        assert_eq!(bulk_update(&c, s, &[card], &Patch { expires_at: Some(0), ..Default::default() }).unwrap(), 1);
        let k = members::card(&c, s, card).unwrap().unwrap();
        assert_eq!((k.status.as_str(), k.expires_at), ("Active", 0));
    }

    #[test]
    fn for_sale_only_changes_cards_that_were_not_for_sale() {
        let c = db();
        let (s, _, card) = seller_with_card(&c);
        update_listing(&c, s, card, &Patch { status: Some("Out of stock".into()), ..Default::default() }).unwrap();
        let k = set_kind(&c, s, card, "sale").unwrap().unwrap();
        assert_eq!((k.status.as_str(), k.qty), ("Out of stock", 2), "already for sale: left alone");
        // A highlight going on sale: sold out gets one copy, hidden stays hidden, expired comes back.
        let Added::Yes(h) = add_listing(&c, s, "tcgdex:x-2", "Mew", "2", "Set", "", "").unwrap() else { panic!() };
        set_kind(&c, s, h, "own").unwrap().unwrap();
        c.execute("UPDATE member_cards SET status='Sold', qty=0 WHERE id=?1", [h]).unwrap();
        let k = set_kind(&c, s, h, "sale").unwrap().unwrap();
        assert_eq!((k.status.as_str(), k.qty), ("Active", 1));
        set_kind(&c, s, h, "own").unwrap().unwrap();
        c.execute("UPDATE member_cards SET status='Hidden' WHERE id=?1", [h]).unwrap();
        assert_eq!(set_kind(&c, s, h, "sale").unwrap().unwrap().status, "Hidden");
        set_kind(&c, s, h, "own").unwrap().unwrap();
        c.execute("UPDATE member_cards SET status='Expired', expires_at=?2 WHERE id=?1", rusqlite::params![h, now() - 10]).unwrap();
        let k = set_kind(&c, s, h, "sale").unwrap().unwrap();
        assert_eq!((k.status.as_str(), k.expires_at), ("Active", 0), "live again, and the clock won't take it straight back");
    }

    #[test]
    fn an_offer_cant_take_a_copy_someone_else_reserved() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        update_listing(&c, s, card, &Patch { qty: Some(1), ..Default::default() }).unwrap();
        let o = make_offer(&c, b, s, card, 80.0, 1, "").unwrap().unwrap();
        let b2 = create(&c, "Buyer Two", "h").unwrap().unwrap();
        reserve(&c, b2, s, card, 1, "").unwrap().unwrap();
        assert!(answer_offer(&c, s, o, "Accepted", 0.0).unwrap().is_err(), "the last copy is reserved");
        assert_eq!(offer(&c, o).unwrap().unwrap().status, "Pending", "the offer waits");
        answer_offer(&c, s, o, "Countered", 90.0).unwrap().unwrap();
        assert!(answer_counter(&c, b, o, true).unwrap().is_err(), "the buyer can't take it either");
        assert_eq!(offer(&c, o).unwrap().unwrap().status, "Countered");
        assert_eq!(held(&c, card).unwrap(), 1, "still just the one reservation");
        // A blocked store, or one whose profile isn't approved, sells nothing.
        update_listing(&c, s, card, &Patch { qty: Some(5), ..Default::default() }).unwrap();
        let b3 = create(&c, "Buyer Three", "h").unwrap().unwrap();
        c.execute("UPDATE members SET blocked=1 WHERE id=?1", [s]).unwrap();
        assert_eq!(reserve(&c, b3, s, card, 1, "").unwrap(), Err("This store is closed."));
        let lines = cart_lines(&c, &[], &[card]).unwrap();
        assert!(!lines[0].ok && lines[0].why == "This store is closed.", "{lines:?}");
        c.execute("UPDATE members SET blocked=0, profile_ok=0 WHERE id=?1", [s]).unwrap();
        assert!(make_offer(&c, b3, s, card, 80.0, 1, "").unwrap().is_err());
        members::set_profile_ok(&c, s, true).unwrap();
        assert!(reserve(&c, b3, s, card, 1, "").unwrap().is_ok());
    }

    #[test]
    fn two_vault_orders_cant_both_take_the_last_copy() {
        let c = db();
        let b = create(&c, "Buyer", "h").unwrap().unwrap();
        let b2 = create(&c, "Buyer Two", "h").unwrap().unwrap();
        c.execute("INSERT INTO products(id,handle,title,price,available,created_at) VALUES(1,'pika','Pikachu',100,1,'2026-01-01T00:00:00')", []).unwrap();
        let o1 = place_vault_order(&c, b, &[(1, 1)], "").unwrap().unwrap();
        assert!(place_vault_order(&c, b2, &[(1, 1)], "").unwrap().is_err(), "the last copy is in an open order");
        assert_eq!(crate::db::vault_left(&c, 1, 0).unwrap(), 0);
        assert_eq!(crate::db::vault_left(&c, 1, o1).unwrap(), 1, "an order doesn't hold copies against itself");
        // A newer order that got in anyway can't be accepted or paid while the older one holds the copy.
        c.execute("INSERT INTO vault_orders(buyer_id, status, message, created_at, updated_at) VALUES(?1,'Pending','',0,0)", [b2]).unwrap();
        let o2 = c.last_insert_rowid();
        c.execute("INSERT INTO vault_order_items(order_id, product_id, title, price, qty) VALUES(?1,1,'Pikachu',100,1)", [o2]).unwrap();
        let e = answer_vault_order(&c, o2, "Confirmed", "").unwrap().err().unwrap();
        assert_eq!(e, "Only 0 left of Pikachu. Decline or change the order.");
        assert!(answer_vault_order(&c, o2, "Paid", "").unwrap().is_err());
        assert_eq!(vault_order(&c, o2).unwrap().unwrap().status, "Pending", "nothing changed");
        assert!(answer_vault_order(&c, o1, "Confirmed", "").unwrap().is_ok(), "an order's own copies don't block it");
        assert!(answer_vault_order(&c, o1, "Paid", "GCash").unwrap().is_ok());
        assert_eq!(crate::db::copies_for_sale(&c, 1).unwrap(), 0);
        assert!(answer_vault_order(&c, o2, "Paid", "").unwrap().is_err(), "sold to the first buyer");
        assert!(answer_vault_order(&c, o2, "Declined", "").unwrap().is_ok());
    }

    #[test]
    fn a_cart_line_whose_copy_sold_takes_another_copy_of_the_card() {
        let c = db();
        let a = create(&c, "Buyer", "h").unwrap().unwrap();
        let b = create(&c, "Buyer Two", "h").unwrap().unwrap();
        c.execute("INSERT INTO products(id,handle,title,price,available,created_at) VALUES(1,'pika-1','Pikachu',100,1,'2026-01-01T00:00:00'),(2,'pika-2','Pikachu',100,1,'2026-01-02T00:00:00')", []).unwrap();
        // Both buyers added the card from the same tile (copy 1). The first buyer's order is paid: copy 1 is sold.
        let o = place_vault_order(&c, a, &[(1, 1)], "").unwrap().unwrap();
        assert!(answer_vault_order(&c, o, "Paid", "GCash").unwrap().is_ok());
        assert_eq!(crate::db::sale_copy(&c, 1).unwrap(), Some(2));
        let line = &cart_lines(&c, &[1], &[]).unwrap()[0];
        assert_eq!((line.id, line.ok, line.left, line.url.as_str()), (1, true, 1, "/products/pika-2"), "{line:?}");
        assert!(vault_short_lines(&c, &[(1, 1)]).unwrap().is_empty());
        let short = vault_short_lines(&c, &[(1, 2)]).unwrap();
        assert_eq!(short, vec![(1, 1, "Only 1 left".to_string())], "reported under the id the cart sent");
        let o2 = place_vault_order(&c, b, &[(1, 1)], "").unwrap().unwrap();
        assert_eq!(vault_order(&c, o2).unwrap().unwrap().items[0].product_id, 2, "the order takes the copy still for sale");
        assert!(answer_vault_order(&c, o2, "Paid", "").unwrap().is_ok());
        assert_eq!(crate::db::sale_copy(&c, 1).unwrap(), None);
        assert!(!cart_lines(&c, &[1], &[]).unwrap()[0].ok, "every copy sold");
        assert!(place_vault_order(&c, b, &[(1, 1)], "").unwrap().is_err());
    }

    #[test]
    fn stock_words_and_vault_holds_stay_honest() {
        let c = db();
        // A hand-picked "Sold" empties the stock. "Reserved" is only ever worked out from real holds.
        let (s, b, card) = seller_with_card(&c);
        let set = |p: Patch| update_listing(&c, s, card, &p).unwrap().unwrap();
        let k = set(Patch { status: Some("Sold".into()), ..Default::default() });
        assert_eq!((k.status.as_str(), k.qty), ("Sold", 0));
        assert_eq!(set(Patch { qty: Some(2), ..Default::default() }).status, "Active");
        assert_eq!(set(Patch { status: Some("Reserved".into()), ..Default::default() }).status, "Active", "nobody holds it");
        // A member who leaves stops holding anything.
        c.execute("INSERT INTO products(id,handle,title,price,available,created_at) VALUES(1,'pika-1','Pikachu',100,1,'2026-01-01T00:00:00'),(2,'pika-2','Pikachu',100,1,'2026-01-02T00:00:00')", []).unwrap();
        let b2 = create(&c, "Buyer Two", "h").unwrap().unwrap();
        place_vault_order(&c, b2, &[(1, 2)], "").unwrap().unwrap();
        assert_eq!(crate::db::vault_left(&c, 1, 0).unwrap(), 0);
        let r = reserve(&c, b2, s, card, 1, "").unwrap().unwrap();
        members::delete(&c, b2).unwrap();
        assert_eq!(crate::db::vault_left(&c, 1, 0).unwrap(), 2, "the leaving buyer's order let go");
        assert_eq!(reservation(&c, r).unwrap().unwrap().status, "Cancelled");
        assert_eq!(members::card(&c, s, card).unwrap().unwrap().status, "Active");
        // Deleting the copy an order points at moves the order onto another copy of the card.
        let o = place_vault_order(&c, b, &[(2, 1)], "").unwrap().unwrap();
        crate::db::delete_entry(&c, 2).unwrap();
        assert_eq!(crate::db::vault_left(&c, 1, 0).unwrap(), 0, "the order now holds the other copy");
        assert!(answer_vault_order(&c, o, "Confirmed", "").unwrap().is_ok());
        // One buyer can't hold the whole vault: orders have limits, and unanswered ones lapse.
        assert!(place_vault_order(&c, b, &[(1, 61)], "").unwrap().is_err());
        c.execute("UPDATE vault_orders SET status='Pending', created_at=0 WHERE id=?1", [o]).unwrap();
        expire_due(&c).unwrap();
        assert_eq!(vault_order(&c, o).unwrap().unwrap().status, "Cancelled");
        assert_eq!(crate::db::vault_left(&c, 1, 0).unwrap(), 1);
    }

    #[test]
    fn closing_a_listings_orders_tells_each_buyer_once() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        let b2 = create(&c, "Buyer Two", "h").unwrap().unwrap();
        let r = reserve(&c, b, s, card, 1, "").unwrap().unwrap();
        let ob = make_offer(&c, b, s, card, 70.0, 1, "").unwrap().unwrap();
        let o2 = make_offer(&c, b2, s, card, 80.0, 1, "").unwrap().unwrap();
        answer_offer(&c, s, o2, "Countered", 90.0).unwrap().unwrap();
        let (nb, nb2) = (members::unread_notification_count(&c, b).unwrap(), members::unread_notification_count(&c, b2).unwrap());
        assert_eq!(close_open_orders(&c, s, card).unwrap(), 3);
        assert_eq!(reservation(&c, r).unwrap().unwrap().status, "Cancelled");
        assert_eq!(offer(&c, ob).unwrap().unwrap().status, "Declined");
        assert_eq!(offer(&c, o2).unwrap().unwrap().status, "Declined");
        assert_eq!(members::unread_notification_count(&c, b).unwrap(), nb + 1, "told once, not once per order");
        assert_eq!(members::unread_notification_count(&c, b2).unwrap(), nb2 + 1);
        assert_eq!(notifications_page(&c, b2, 0, 1).unwrap().0[0].message, "Shop took Pikachu off sale.");
        assert_eq!(held(&c, card).unwrap(), 0);
        assert_eq!(close_open_orders(&c, s, card).unwrap(), 0, "nothing left to close");
        assert_eq!(members::unread_notification_count(&c, b).unwrap(), nb + 1, "and nobody is told twice");
        // Taking a card off sale, or removing it, closes its orders too.
        let r2 = reserve(&c, b, s, card, 1, "").unwrap().unwrap();
        set_kind(&c, s, card, "own").unwrap().unwrap();
        assert_eq!(reservation(&c, r2).unwrap().unwrap().status, "Cancelled");
        set_kind(&c, s, card, "sale").unwrap().unwrap();
        let r3 = reserve(&c, b2, s, card, 1, "").unwrap().unwrap();
        assert_eq!(bulk_delete(&c, s, &[card]).unwrap().len(), 1);
        assert_eq!(reservation(&c, r3).unwrap().unwrap().status, "Cancelled");
    }

    #[test]
    fn running_out_tells_each_person_once() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        let r = reserve(&c, b, s, card, 1, "").unwrap().unwrap();
        update_listing(&c, s, card, &Patch { expires_at: Some(now() - 10), ..Default::default() }).unwrap();
        c.execute("UPDATE reservations SET expires_at=?2 WHERE id=?1", rusqlite::params![r, now() - 10]).unwrap();
        let (ns, nb) = (members::unread_notification_count(&c, s).unwrap(), members::unread_notification_count(&c, b).unwrap());
        assert_eq!(expire_due(&c).unwrap(), 2, "one listing and one reservation");
        assert_eq!(reservation(&c, r).unwrap().unwrap().status, "Expired");
        assert_eq!(members::unread_notification_count(&c, b).unwrap(), nb + 1, "the buyer hears the reservation ran out");
        assert_eq!(members::unread_notification_count(&c, s).unwrap(), ns + 2, "the seller hears about the listing and the reservation");
        assert_eq!(expire_due(&c).unwrap(), 0, "a second sweep changes nothing");
        assert_eq!(members::unread_notification_count(&c, b).unwrap(), nb + 1, "and tells nobody twice");
        assert_eq!(members::unread_notification_count(&c, s).unwrap(), ns + 2);
    }

    #[test]
    fn accepting_checks_the_copies_are_still_there() {
        let c = db();
        let (s, b, card) = seller_with_card(&c); // two copies
        let r = reserve(&c, b, s, card, 2, "").unwrap().unwrap();
        update_listing(&c, s, card, &Patch { qty: Some(1), ..Default::default() }).unwrap();
        assert!(answer_reservation(&c, s, r, "Confirmed", "").unwrap().is_err(), "only one copy is left for two");
        assert_eq!(reservation(&c, r).unwrap().unwrap().status, "Pending", "nothing changed");
        update_listing(&c, s, card, &Patch { qty: Some(2), ..Default::default() }).unwrap();
        assert!(answer_reservation(&c, s, r, "Confirmed", "").unwrap().is_ok());
        // Marking paid still works after the seller marked the card Sold: the money already changed hands.
        update_listing(&c, s, card, &Patch { status: Some("Sold".into()), ..Default::default() }).unwrap();
        assert!(answer_reservation(&c, s, r, "Paid", "GCash").unwrap().is_ok());
        // A reservation made inside someone else's transaction still works (no nested BEGIN).
        update_listing(&c, s, card, &Patch { qty: Some(3), ..Default::default() }).unwrap();
        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert!(reserve(&c, b, s, card, 1, "").unwrap().is_ok());
        c.execute_batch("COMMIT").unwrap();
        assert!(c.is_autocommit());
    }

    #[test]
    fn a_store_that_closes_lets_its_buyers_go() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        let b2 = create(&c, "Buyer Two", "h").unwrap().unwrap();
        let r = reserve(&c, b, s, card, 1, "").unwrap().unwrap();
        let o = make_offer(&c, b2, s, card, 80.0, 1, "").unwrap().unwrap();
        // What the seller is buying from someone else stays open.
        let other = create(&c, "Other Shop", "h").unwrap().unwrap();
        members::set_seller(&c, other, true).unwrap();
        members::set_profile_ok(&c, other, true).unwrap();
        let Added::Yes(oc) = add_listing(&c, other, "tcgdex:x-5", "Mew", "5", "Set", "", "").unwrap() else { panic!() };
        update_listing(&c, other, oc, &Patch { price: Some(50.0), ..Default::default() }).unwrap();
        let mine = reserve(&c, s, other, oc, 1, "").unwrap().unwrap();
        let nb = members::unread_notification_count(&c, b).unwrap();
        assert_eq!(close_store_orders(&c, s).unwrap(), 2);
        assert_eq!(reservation(&c, r).unwrap().unwrap().status, "Cancelled");
        assert_eq!(offer(&c, o).unwrap().unwrap().status, "Declined");
        assert_eq!(reservation(&c, mine).unwrap().unwrap().status, "Pending", "their own buying is left alone");
        assert_eq!(members::unread_notification_count(&c, b).unwrap(), nb + 1);
        assert_eq!(held(&c, card).unwrap(), 0);
        assert_eq!(close_store_orders(&c, s).unwrap(), 0, "nothing left to close");
    }

    #[test]
    fn the_cart_hides_closed_stores_and_names_the_short_vault_lines() {
        let c = db();
        let (s, _, card) = seller_with_card(&c);
        for sql in ["UPDATE members SET blocked=1 WHERE id=?1", "UPDATE members SET blocked=0, profile_ok=0 WHERE id=?1", "UPDATE members SET profile_ok=1, store_hidden=1 WHERE id=?1"] {
            c.execute(sql, [s]).unwrap();
            let l = &cart_lines(&c, &[], &[card]).unwrap()[0];
            assert!(!l.ok && l.why == "This store is closed.", "{l:?}");
            assert!(l.seller_name.is_empty() && l.seller_slug.is_empty() && l.image.is_empty() && l.price == 0.0 && l.seller_id == 0, "{l:?}");
            assert!(!l.title.contains("Pikachu"), "{l:?}");
        }
        c.execute("INSERT INTO products(id,handle,title,price,available,created_at) VALUES(1,'pika','Pikachu',100,1,'2026-01-01T00:00:00'),(2,'mew','Mew',100,1,'2026-01-01T00:00:00'),(3,'eevee','Eevee',0,1,'2026-01-01T00:00:00')", []).unwrap();
        let short = vault_short_lines(&c, &[(1, 1), (2, 2), (3, 1), (9, 1)]).unwrap();
        let ids: Vec<i64> = short.iter().map(|x| x.0).collect();
        assert_eq!(ids, vec![2, 3, 9], "only the lines that can't be filled: {short:?}");
        assert_eq!((short[0].1, short[0].2.as_str()), (1, "Only 1 left"));
        assert!(vault_short_lines(&c, &[(1, 1), (2, 1)]).unwrap().is_empty());
    }

    #[test]
    fn buyers_news_has_its_own_switch() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        // The buyer switches off news about their own store's reservations; their own order news still comes.
        assert!(set_pref(&c, b, "reservation", false, false, false).unwrap());
        let r = reserve(&c, b, s, card, 1, "").unwrap().unwrap();
        let nb = members::unread_notification_count(&c, b).unwrap();
        answer_reservation(&c, s, r, "Confirmed", "").unwrap().unwrap();
        assert_eq!(members::unread_notification_count(&c, b).unwrap(), nb + 1);
        assert_eq!(notifications_page(&c, b, 0, 1).unwrap().0[0].kind, "my_order");
        // Push: ten devices at most.
        for i in 0..12 {
            push_subscribe(&c, b, &format!("https://push.example/{i}"), "k", "a").unwrap();
        }
        let n: i64 = c.query_row("SELECT COUNT(*) FROM push_subscriptions WHERE member_id=?1", [b], |r| r.get(0)).unwrap();
        assert_eq!(n, MAX_PUSH_DEVICES);
        assert_eq!(set_email(&c, b, " not an email ").unwrap(), "");
        assert_eq!(set_email(&c, b, "ash@example.com").unwrap(), "ash@example.com");
    }

    #[test]
    fn a_browser_belongs_to_whoever_signed_up_on_it_last_and_pushes_wait_in_a_queue() {
        let c = db();
        push_subscribe(&c, 2, "https://fcm.googleapis.com/fcm/send/one", "k", "a").unwrap();
        push_subscribe(&c, 3, "https://fcm.googleapis.com/fcm/send/one", "k", "a").unwrap();
        let owners: Vec<i64> = c.prepare("SELECT member_id FROM push_subscriptions").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
        assert_eq!(owners, vec![3], "the browser moved to the person who signed up on it last");
        // Message alerts are on unless the member turned them off; turning alerts on never overrides that choice.
        let push = |m: i64| prefs_of(&c, m).unwrap().into_iter().find(|p| p.event == "message").unwrap().push;
        assert!(push(3), "on by default");
        set_pref(&c, 2, "message", true, false, false).unwrap();
        push_subscribe(&c, 2, "https://fcm.googleapis.com/fcm/send/two", "k", "a").unwrap();
        assert!(!push(2), "they had said no to message alerts");
        assert_eq!(push_forget(&c, "https://fcm.googleapis.com/fcm/send/two").unwrap(), 1);
        // Notes go to the queue for the events the member wants pushed: on by default for what people wait on, off for
        // views and hints, off where the member chose "Bell only", and never for private messages (they send their own).
        set_pref(&c, 3, "offer", true, false, true).unwrap();
        notify(&c, 3, "offer", "Juan offers ₱500", "/me/offers").unwrap();
        notify(&c, 3, "view", "3 people looked at your card", "/me/listings").unwrap();
        set_pref(&c, 3, "follow", true, false, false).unwrap();
        notify(&c, 3, "follow", "Someone follows your store", "/me/store").unwrap();
        notify(&c, 3, "message", "Juan: “hi”", "/me/messages?t=1").unwrap();
        notify(&c, 3, "wish", "A card on your wishlist is for sale", "/all").unwrap();
        let q = take_push_outbox(&c, 10).unwrap();
        assert_eq!(
            q,
            vec![
                (3, "offer".to_string(), "Juan offers ₱500".to_string(), "/me/offers".to_string()),
                (3, "wish".to_string(), "A card on your wishlist is for sale".to_string(), "/all".to_string()),
            ]
        );
        assert!(take_push_outbox(&c, 10).unwrap().is_empty(), "taken off the queue");
    }

    #[test]
    fn one_cart_is_one_line_for_each_store_and_each_line_says_who() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        let Added::Yes(two) = add_listing(&c, s, "tcgdex:base1-25", "Dewgong - 025/102 - Base Set", "25/102", "Base Set", "", "").unwrap() else { panic!() };
        members::set_card_value(&c, "tcgdex:base1-25", 50.0, now()).unwrap();
        set_kind(&c, s, two, "sale").unwrap().unwrap();
        reserve(&c, b, s, card, 1, "").unwrap().unwrap();
        reserve(&c, b, s, two, 1, "").unwrap().unwrap();
        let lines: Vec<(String, i64, String)> = c.prepare("SELECT message, actor_id, ref FROM notifications WHERE member_id=?1").unwrap().query_map([s], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
        assert_eq!(lines.len(), 1, "one cart, one line: {lines:?}");
        assert!(lines[0].0.starts_with("Buyer wants to buy 2 cards for ₱"), "{lines:?}");
        assert_eq!((lines[0].1, lines[0].2.clone()), (b, format!("cart:{b}")));
        assert_eq!(take_push_outbox(&c, 10).unwrap().iter().filter(|p| p.0 == s).count(), 1, "and one buzz");
        // In the bell: the buyer's first letter (no picture yet) and the kind in plain words.
        let n = members::notifications(&c, s, true).unwrap();
        assert_eq!((n[0].actor_id, n[0].initial.as_str(), n[0].label.as_str()), (b, "B", "Order"));
        // Tapping the line reads that line only.
        toggle_follow(&c, b, s).unwrap();
        assert_eq!(members::unread_notification_count(&c, s).unwrap(), 2);
        assert_eq!(members::mark_one_read(&c, s, n[0].id).unwrap(), 1);
        assert_eq!(members::mark_one_read(&c, b, n[0].id).unwrap(), 0, "only your own lines");
        let n = members::notifications(&c, s, true).unwrap();
        assert_eq!((n.len(), n[0].kind.as_str(), n[0].actor_id), (1, "follow", b), "a follow says who, too");
        // A line from the site itself shows the site's logo.
        notify(&c, s, "announcement", "News from the vault", "/").unwrap();
        let n = members::notifications(&c, s, true).unwrap();
        assert!(n[0].actor_id == 0 && n[0].avatar.contains("logo") && n[0].initial.is_empty(), "{:?}", n[0]);
    }

    #[test]
    fn one_cart_is_one_order_answered_together_and_heard_as_one_line() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        let Added::Yes(two) = add_listing(&c, s, "tcgdex:base1-25", "Dewgong - 025/102 - Base Set", "25/102", "Base Set", "", "").unwrap() else { panic!() };
        members::set_card_value(&c, "tcgdex:base1-25", 50.0, now()).unwrap();
        set_kind(&c, s, two, "sale").unwrap().unwrap();
        let r1 = reserve(&c, b, s, card, 1, "Meet at SM?").unwrap().unwrap();
        let r2 = reserve(&c, b, s, two, 1, "").unwrap().unwrap();
        set_batch(&c, r1, r1).unwrap();
        set_batch(&c, r2, r1).unwrap();
        // Someone else's order of the same card stays apart.
        let b2 = create(&c, "Misty", "h").unwrap().unwrap();
        let r3 = reserve(&c, b2, s, card, 1, "").unwrap().unwrap();
        let groups = order_groups(orders_for_seller(&c, s).unwrap().reservations);
        let sizes: Vec<usize> = groups.iter().map(|g| g.rows.len()).collect();
        assert_eq!(sizes.iter().sum::<usize>(), 3);
        let cart = groups.iter().find(|g| g.many()).expect("the cart is one order");
        assert_eq!((cart.rows.len(), cart.first().buyer_name.as_str(), cart.units(), cart.has("Pending")), (2, "Buyer", 2, true));
        assert!(groups.iter().any(|g| !g.many() && g.first().id == r3), "an order of one card stays by itself");
        assert_eq!(cart.first().status_text(), "New");
        // Accept all: one line for the buyer, brought up to date, and one buzz.
        take_push_outbox(&c, 50).unwrap();
        answer_reservation(&c, s, r1, "Confirmed", "").unwrap().unwrap();
        answer_reservation(&c, s, r2, "Confirmed", "").unwrap().unwrap();
        let lines: Vec<String> = c.prepare("SELECT message FROM notifications WHERE member_id=?1 AND kind='my_order'").unwrap().query_map([b], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(lines, vec!["Shop accepted your order of 2 cards. Time to pay.".to_string()]);
        assert_eq!(take_push_outbox(&c, 50).unwrap().iter().filter(|p| p.0 == b).count(), 1);
        assert!(order_groups(orders_for_seller(&c, s).unwrap().reservations).iter().any(|g| g.many() && g.has("Confirmed") && !g.has("Pending")));
        // A card ordered alone is answered in its own words.
        answer_reservation(&c, s, r3, "Declined", "").unwrap().unwrap();
        let last: String = c.query_row("SELECT message FROM notifications WHERE member_id=?1 ORDER BY id DESC LIMIT 1", [b2], |r| r.get(0)).unwrap();
        assert_eq!(last, "Shop declined your order for Pikachu.");
        // "X now follows your store" opens X's page.
        toggle_follow(&c, b2, s).unwrap();
        let link: String = c.query_row("SELECT link FROM notifications WHERE member_id=?1 AND kind='follow'", [s], |r| r.get(0)).unwrap();
        assert_eq!(link, format!("/trainers/{b2}"));
    }

    #[test]
    fn the_buyer_says_ive_paid_and_the_seller_hears_once() {
        let c = db();
        let (s, b, card) = seller_with_card(&c);
        let Added::Yes(two) = add_listing(&c, s, "tcgdex:base1-25", "Dewgong - 025/102 - Base Set", "25/102", "Base Set", "", "").unwrap() else { panic!() };
        members::set_card_value(&c, "tcgdex:base1-25", 50.0, now()).unwrap();
        set_kind(&c, s, two, "sale").unwrap().unwrap();
        let r1 = reserve(&c, b, s, card, 1, "").unwrap().unwrap();
        let r2 = reserve(&c, b, s, two, 1, "").unwrap().unwrap();
        set_batch(&c, r1, r1).unwrap();
        set_batch(&c, r2, r1).unwrap();
        assert_eq!(buyer_paid(&c, b, r1).unwrap(), Err("Pay once the seller accepts your order."));
        answer_reservation(&c, s, r1, "Confirmed", "").unwrap().unwrap();
        answer_reservation(&c, s, r2, "Confirmed", "").unwrap().unwrap();
        assert_eq!(buyer_paid(&c, s, r1).unwrap(), Err("That order isn't yours."));
        assert_eq!(buyer_paid(&c, b, r1).unwrap(), Ok(()));
        assert!(reservation(&c, r2).unwrap().unwrap().buyer_paid_at > 0, "the whole order");
        let told: Vec<String> = c.prepare("SELECT message FROM notifications WHERE member_id=?1 AND message LIKE '%says they paid%'").unwrap().query_map([s], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(told, vec!["Buyer says they paid for their order of 2 cards. Check, then mark it paid.".to_string()]);
        let g = order_groups(orders_for_seller(&c, s).unwrap().reservations).into_iter().find(|g| g.many()).unwrap();
        assert!(!g.buyer_paid_text().is_empty());
        // The vault's order: its admins hear.
        let admin = create(&c, "Boss", "h").unwrap().unwrap();
        c.execute("UPDATE members SET role='admin' WHERE id=?1", [admin]).unwrap();
        c.execute("INSERT INTO vault_orders(buyer_id, status, message, created_at, updated_at) VALUES(?1,'Confirmed','',1,1)", [b]).unwrap();
        let vo = c.last_insert_rowid();
        assert_eq!(vault_buyer_paid(&c, b, vo).unwrap(), Ok(()));
        assert!(vault_order(&c, vo).unwrap().unwrap().buyer_paid_at > 0);
        let n: i64 = c.query_row("SELECT COUNT(*) FROM notifications WHERE member_id=?1 AND kind='order' AND message LIKE '%says they paid%'", [admin], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn people_are_followed_like_stores() {
        let c = db();
        let ash = create(&c, "Ash", "h").unwrap().unwrap();
        let misty = create(&c, "Misty", "h").unwrap().unwrap();
        members::set_profile_ok(&c, misty, true).unwrap();
        assert!(toggle_follow(&c, misty, ash).unwrap());
        let line: (String, String) = c.query_row("SELECT message, link FROM notifications WHERE member_id=?1 AND kind='follow'", [ash], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(line, ("Misty now follows you.".to_string(), format!("/trainers/{misty}")), "someone without a store is followed as themselves");
        assert_eq!(follower_list(&c, ash, 10).unwrap().iter().map(|f| f.1.clone()).collect::<Vec<_>>(), vec!["Misty".to_string()]);
        members::set_profile_ok(&c, misty, false).unwrap();
        assert!(follower_list(&c, ash, 10).unwrap().is_empty(), "only trainers whose page is up are listed");
        assert_eq!(followers(&c, ash).unwrap(), 1, "they still count");
        let staff = create(&c, "Mod", "h").unwrap().unwrap();
        c.execute("UPDATE members SET role='mod' WHERE id=?1", [staff]).unwrap();
        assert_eq!(staff_ids(&c).unwrap(), vec![staff]);
    }

    #[test]
    fn likes_on_one_post_are_one_line_until_it_is_read() {
        let c = db();
        let a = create(&c, "Ash", "h").unwrap().unwrap();
        let (m, b) = (create(&c, "Misty", "h").unwrap().unwrap(), create(&c, "Brock", "h").unwrap().unwrap());
        notify_from(&c, a, m, "social", "react:7", "Misty liked your post.", "/posts/7").unwrap();
        notify_from(&c, a, b, "social", "react:7", "Brock and 1 other liked your post.", "/posts/7").unwrap();
        notify_from(&c, a, b, "social", "react:8", "Brock liked your other post.", "/posts/8").unwrap();
        let lines = |c: &Connection| -> Vec<(String, i64)> { c.prepare("SELECT message, actor_id FROM notifications WHERE member_id=?1 ORDER BY id").unwrap().query_map([a], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect() };
        assert_eq!(lines(&c), vec![("Brock and 1 other liked your post.".to_string(), b), ("Brock liked your other post.".to_string(), b)], "the first line brought up to date");
        assert_eq!(take_push_outbox(&c, 10).unwrap().len(), 2, "one buzz per post, not one per like");
        members::mark_notifications_read(&c, a, None).unwrap();
        notify_from(&c, a, m, "social", "react:7", "Misty and 2 others liked your post.", "/posts/7").unwrap();
        assert_eq!(lines(&c).len(), 3, "once read, the next like is news again");
        // Someone who turned a kind off hears nothing of it, grouped or not.
        set_pref(&c, a, "social", false, false, false).unwrap();
        notify_from(&c, a, m, "social", "react:9", "Misty liked your post.", "/posts/9").unwrap();
        assert_eq!(lines(&c).len(), 3);
        assert_eq!(kind_label("following"), "Following");
        assert_eq!(default_pref("view"), (true, false, false), "views stay in the bell");
        assert_eq!(default_pref("follow"), (true, false, true), "what people wait on rings the phone");
    }

    #[test]
    fn link_lines_keep_labels_and_drop_bad_links() {
        let cleaned = clean_link_lines("Shopee | https://shopee.ph/bbv\nhttps://carousell.ph/u/bbv\njavascript:alert(1)\nFB | http://not-https.com");
        assert_eq!(cleaned, "Shopee | https://shopee.ph/bbv\nhttps://carousell.ph/u/bbv");
        assert_eq!(link_parts("https://carousell.ph/u/bbv"), ("Carousell".into(), "https://carousell.ph/u/bbv".into()));
        assert_eq!(link_parts("Shopee | https://shopee.ph/bbv").0, "Shopee");
    }

    #[test]
    fn a_binder_tile_shows_its_cover_then_its_next_cards_on_sale() {
        let c = db();
        let s = create(&c, "Shop", "h").unwrap().unwrap();
        members::set_seller(&c, s, true).unwrap();
        members::set_profile_ok(&c, s, true).unwrap();
        let mut ids = vec![];
        for i in 1..=6 {
            let Added::Yes(id) = add_listing(&c, s, &format!("tcgdex:x-{i}"), &format!("Card {i}"), "1", "Set", &format!("/p/{i}.webp"), "").unwrap() else { panic!() };
            ids.push(id);
        }
        let shelf = members::create_member_binder(&c, s, "Shelf").unwrap();
        let empty = members::create_member_binder(&c, s, "Empty").unwrap();
        for (pos, id) in ids.iter().enumerate() {
            members::move_card_to_binder(&c, s, *id, Some(shelf.id)).unwrap();
            c.execute("UPDATE member_cards SET position=?2 WHERE id=?1", rusqlite::params![id, pos as i64]).unwrap();
        }
        // The second card is hidden and the fifth is a copy of the fourth: neither peeks out.
        c.execute("UPDATE member_cards SET status='Hidden' WHERE id=?1", [ids[1]]).unwrap();
        c.execute("UPDATE member_cards SET image='/p/4.webp' WHERE id=?1", [ids[4]]).unwrap();
        let tile = |c: &Connection, id: i64| {
            let b = members::binders_of(c, s).unwrap().into_iter().find(|b| b.id == id).unwrap();
            binder_peeks(&b, &storefront_listings(c, s).unwrap(), 3)
        };
        assert_eq!(tile(&c, shelf.id), vec!["/p/1.webp", "/p/3.webp", "/p/4.webp"], "no cover picked: the first card, then the next ones on sale");
        // The seller picks the sixth card as the cover: it comes first and isn't repeated.
        members::set_member_binder_cover(&c, s, shelf.id, Some(ids[5])).unwrap();
        assert_eq!(tile(&c, shelf.id), vec!["/p/6.webp", "/p/1.webp", "/p/3.webp"]);
        let b = members::binders_of(&c, s).unwrap().into_iter().find(|b| b.id == shelf.id).unwrap();
        assert_eq!(binder_peeks(&b, &storefront_listings(&c, s).unwrap(), 1), vec!["/p/6.webp"], "never more than asked for");
        assert!(tile(&c, empty.id).is_empty(), "an empty binder has nothing to show");
    }
}
