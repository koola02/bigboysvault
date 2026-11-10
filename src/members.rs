//! Trainer accounts: profile, one binder of up to 50 cards, vouches, replies and star points.
//! A trainer's page (name, picture, description, binder) waits for the owner's approval. Vouches, replies and their photos
//! post straight away, and the owner can hide or delete any of them, or lock, block or remove the trainer (Admin › Trainers).
use argon2::password_hash::{rand_core::{OsRng, RngCore}, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rusqlite::{Connection, OptionalExtension, Row};
use sha2::{Digest, Sha256};

/// How many cards a store may hold unless the owner sets `seller_max_cards` (see `sellers::max_cards`).
pub const MAX_CARDS: i64 = 500;
pub const SESSION_SECS: i64 = 30 * 86_400;
/// Sign-ups stop while this many are waiting for approval, so a bot can't fill the disk.
pub const MAX_PENDING: i64 = 200;
/// Enough for anyone being honest, and a stop for anyone who isn't.
pub const VOUCHES_PER_HOUR: i64 = 20;
pub const REPLIES_PER_HOUR: i64 = 60;
pub const PHOTOS_PER_HOUR: i64 = 200;
pub const TEXT_MAX: usize = 1000;

/// A card's value in pesos falls with its condition: the usual rule of thumb among sellers.
pub fn condition_factor(cond: &str) -> f64 {
    crate::pricing::factor(cond).unwrap_or(1.0)
}

/// Everything a trainer's cards are worth. Must agree with `MemberCard::worth`.
macro_rules! worth_sql {
    () => {
        "(SELECT COALESCE(SUM(CASE WHEN x.value_own>0 THEN x.value_own ELSE x.value*COALESCE((SELECT s.share FROM condition_shares s WHERE s.cond=x.cond),1.0) END),0) FROM member_cards x WHERE x.member_id=m.id)"
    };
}

pub fn worth_sql_clause() -> &'static str {
    worth_sql!()
}

pub const CHAT_MAX: usize = 500;
/// A chat stays a chat: the oldest messages go once there are this many.
pub const CHAT_KEEP: i64 = 5000;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS members(
  id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, name_key TEXT NOT NULL UNIQUE, pass TEXT NOT NULL,
  bio TEXT NOT NULL DEFAULT '', avatar TEXT NOT NULL DEFAULT '', highlight INTEGER,
  profile_ok INTEGER NOT NULL DEFAULT 0, vouch TEXT NOT NULL DEFAULT '', vouch_ok INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS member_sessions(token TEXT PRIMARY KEY, member_id INTEGER NOT NULL, expires INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS idx_sessions_member ON member_sessions(member_id);
CREATE TABLE IF NOT EXISTS member_cards(
  id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, position INTEGER NOT NULL DEFAULT 0, card_id TEXT NOT NULL,
  title TEXT NOT NULL, number TEXT NOT NULL DEFAULT '', card_set TEXT NOT NULL DEFAULT '', image TEXT NOT NULL DEFAULT '',
  image_large TEXT NOT NULL DEFAULT '', cond TEXT NOT NULL DEFAULT 'Near Mint', UNIQUE(member_id, card_id));
CREATE INDEX IF NOT EXISTS idx_mcards_member ON member_cards(member_id, position);
CREATE TABLE IF NOT EXISTS vouches(
  id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, parent_id INTEGER, text TEXT NOT NULL DEFAULT '',
  hidden INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS idx_vouches_parent ON vouches(parent_id, id);
CREATE INDEX IF NOT EXISTS idx_vouches_member ON vouches(member_id, created_at);
CREATE TABLE IF NOT EXISTS vouch_images(
  id INTEGER PRIMARY KEY AUTOINCREMENT, vouch_id INTEGER NOT NULL DEFAULT 0, member_id INTEGER NOT NULL, path TEXT NOT NULL, thumb TEXT NOT NULL,
  w INTEGER NOT NULL DEFAULT 0, h INTEGER NOT NULL DEFAULT 0, bytes INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS idx_vimages_vouch ON vouch_images(vouch_id, id);
CREATE INDEX IF NOT EXISTS idx_vimages_member ON vouch_images(member_id, created_at);
CREATE TABLE IF NOT EXISTS star_awards(
  id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, points INTEGER NOT NULL, reason TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS idx_awards_member ON star_awards(member_id, id);
CREATE TABLE IF NOT EXISTS chat_messages(
  id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, text TEXT NOT NULL, hidden INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS idx_chat_member ON chat_messages(member_id, created_at);
";

pub const CONDITIONS: [&str; 10] = ["Near Mint", "LP-NM", "Lightly Played", "MP-LP", "Moderately Played", "HP-MP", "Heavily Played", "Damaged-HP", "Damaged", "Sealed"];

#[derive(Clone, Debug)]
pub struct Member {
    pub id: i64,
    pub name: String,
    pub bio: String,
    pub avatar: String,
    pub highlight: Option<i64>,
    pub profile_ok: bool,
    pub cards: i64,
    pub locked_until: i64,
    pub blocked: bool,
    pub stars: i64,
    /// "" for a trainer, "mod" for a moderator, "admin" for an admin.
    pub role: String,
    /// What this trainer's cards are worth, in pesos.
    pub worth: f64,
    /// Tags an admin gave this trainer, shown to everyone ("Top seller", "Since 2019"), comma separated.
    pub tags: String,
    pub slug: String,
    pub fb_url: String,
    pub tiktok_url: String,
    pub links: String,
    pub cover_photo: String,
    pub location: String,
    pub contact_info: String,
    pub contact_public: bool,
    pub status: String,
}

/// An admin's tags for a trainer, as a list.
pub fn tag_list(tags: &str) -> Vec<String> {
    tags.split(',').map(str::trim).filter(|t| !t.is_empty()).map(String::from).collect()
}

/// What a role is called on the site.
pub fn role_label(role: &str) -> &'static str {
    match role {
        "admin" => "Admin",
        "mod" => "Moderator",
        _ => "",
    }
}

/// A link a trainer typed, kept only if it's a plain https:// address (no `javascript:`, no spaces, no quotes).
pub fn clean_https_url(u: &str) -> String {
    let t = u.trim();
    let ok = t.len() <= 300
        && t.len() > "https://".len()
        && t.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("https://"))
        && !t.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '`' | '\\'));
    if ok { t.to_string() } else { String::new() }
}

/// Extra links, one per line: only good https:// ones, at most ten.
pub fn clean_link_lines(text: &str) -> String {
    text.lines().map(clean_https_url).filter(|l| !l.is_empty()).take(10).collect::<Vec<_>>().join("\n")
}

impl Member {
    pub fn is_staff(&self) -> bool {
        !self.role.is_empty()
    }
    pub fn worth_text(&self) -> String {
        vault_search::peso(self.worth)
    }
    pub fn role_text(&self) -> &'static str {
        role_label(&self.role)
    }
    pub fn tag_list(&self) -> Vec<String> {
        tag_list(&self.tags)
    }
    /// Extra links to show: checked again here, so an old bad link never reaches a page.
    pub fn link_list(&self) -> Vec<String> {
        self.links.lines().map(clean_https_url).filter(|s| !s.is_empty()).collect()
    }
    pub fn seller_url(&self) -> String {
        if self.slug.is_empty() {
            format!("/trainers/{}", self.id)
        } else {
            format!("/trainer/{}", self.slug)
        }
    }
    /// The letter shown when there's no profile picture yet.
    pub fn initial(&self) -> String {
        initial(&self.name)
    }
    pub fn has_avatar(&self) -> bool {
        !self.avatar.is_empty()
    }
    pub fn locked(&self) -> bool {
        self.locked_until > now()
    }
    /// How long the lock has left, in words: "5 hours".
    pub fn lock_left(&self) -> String {
        span((self.locked_until - now()).max(0))
    }
}

fn span(secs: i64) -> String {
    let (n, unit) = if secs < 3600 { ((secs + 59) / 60, "minute") } else if secs < 2 * 86_400 { ((secs + 3599) / 3600, "hour") } else { ((secs + 86_399) / 86_400, "day") };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// "3 days ago". The page refreshes these in the browser, so a saved page never goes stale.
pub fn ago(t: i64) -> String {
    let s = (now() - t).max(0);
    let say = |n: i64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match s {
        0..=59 => "just now".to_string(),
        60..=3_599 => say(s / 60, "minute"),
        3_600..=86_399 => say(s / 3_600, "hour"),
        86_400..=1_209_599 => say(s / 86_400, "day"),
        1_209_600..=5_183_999 => say(s / 604_800, "week"),
        5_184_000..=31_535_999 => say(s / 2_592_000, "month"),
        _ => say(s / 31_536_000, "year"),
    }
}

#[derive(Clone, Debug)]
pub struct VouchImage {
    pub id: i64,
    pub path: String,
    pub thumb: String,
    pub w: i64,
    pub h: i64,
}

/// A vouch, or a reply to one (then `parent_id` is set). Replies sit one level under the vouch.
#[derive(Clone, Debug)]
pub struct Vouch {
    pub id: i64,
    pub member_id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub avatar: String,
    pub stars: i64,
    pub role: String,
    pub text: String,
    pub hidden: bool,
    pub created_at: i64,
    pub images: Vec<VouchImage>,
    pub replies: Vec<Vouch>,
    pub worth: f64,
    pub tags: String,
}

impl Vouch {
    pub fn tag_list(&self) -> Vec<String> {
        tag_list(&self.tags)
    }
    pub fn worth_text(&self) -> String {
        vault_search::peso(self.worth)
    }
    pub fn role_text(&self) -> &'static str {
        role_label(&self.role)
    }
    pub fn initial(&self) -> String {
        initial(&self.name)
    }
    pub fn has_avatar(&self) -> bool {
        !self.avatar.is_empty()
    }
    pub fn when(&self) -> String {
        ago(self.created_at)
    }
    /// Long conversations fold away, keeping the newest two in view.
    pub fn older_replies(&self) -> &[Vouch] {
        &self.replies[..self.replies.len().saturating_sub(2)]
    }
    pub fn latest_replies(&self) -> &[Vouch] {
        &self.replies[self.replies.len().saturating_sub(2)..]
    }
    pub fn pics_shown(&self) -> usize {
        self.images.len().min(6)
    }
}

/// Star points the owner gave, and what for.
#[derive(Clone, Debug)]
pub struct Award {
    pub id: i64,
    pub points: i64,
    pub reason: String,
    pub created_at: i64,
}

impl Award {
    pub fn when(&self) -> String {
        ago(self.created_at)
    }
}

/// Adds what newer versions need to an existing database, and moves each old single vouch to the new table.
/// An old vouch that wasn't approved stays hidden, because it may be one the owner hid on purpose.
pub fn migrate(c: &Connection) -> rusqlite::Result<()> {
    // The condition shares the totals below read. Made first, so `worth_sql!` always has its table.
    crate::pricing::install(c)?;
    for (col, def) in [
        ("locked_until", "INTEGER NOT NULL DEFAULT 0"),
        ("blocked", "INTEGER NOT NULL DEFAULT 0"),
        ("stars", "INTEGER NOT NULL DEFAULT 0"),
        ("role", "TEXT NOT NULL DEFAULT ''"),
        ("tags", "TEXT NOT NULL DEFAULT ''"),
        ("slug", "TEXT NOT NULL DEFAULT ''"),
        ("fb_url", "TEXT NOT NULL DEFAULT ''"),
        ("tiktok_url", "TEXT NOT NULL DEFAULT ''"),
        ("links", "TEXT NOT NULL DEFAULT ''"),
        ("cover_photo", "TEXT NOT NULL DEFAULT ''"),
        ("location", "TEXT NOT NULL DEFAULT ''"),
        ("contact_info", "TEXT NOT NULL DEFAULT ''"),
        ("contact_public", "INTEGER NOT NULL DEFAULT 0"),
        ("status", "TEXT NOT NULL DEFAULT 'Active'"),
    ] {
        let there: i64 = c.query_row("SELECT COUNT(*) FROM pragma_table_info('members') WHERE name=?1", [col], |r| r.get(0))?;
        if there == 0 {
            c.execute_batch(&format!("ALTER TABLE members ADD COLUMN {col} {def}"))?;
        }
    }
    for (col, def) in [
        ("value", "REAL NOT NULL DEFAULT 0"),
        ("value_own", "REAL NOT NULL DEFAULT 0"),
        ("value_at", "INTEGER NOT NULL DEFAULT 0"),
        ("binder_id", "INTEGER"),
        ("status", "TEXT NOT NULL DEFAULT 'Active'"),
        ("expires_at", "INTEGER NOT NULL DEFAULT 0"),
        ("is_custom", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        let there: i64 = c.query_row("SELECT COUNT(*) FROM pragma_table_info('member_cards') WHERE name=?1", [col], |r| r.get(0))?;
        if there == 0 {
            c.execute_batch(&format!("ALTER TABLE member_cards ADD COLUMN {col} {def}"))?;
        }
    }
    c.execute_batch("
    CREATE TABLE IF NOT EXISTS member_binders(
      id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, title TEXT NOT NULL, slug TEXT NOT NULL,
      cover_card INTEGER, position INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
    CREATE INDEX IF NOT EXISTS idx_mb_member ON member_binders(member_id, position);
    CREATE UNIQUE INDEX IF NOT EXISTS idx_mb_slug ON member_binders(member_id, slug);
    ")?;

    for (col, def) in [
        ("description", "TEXT NOT NULL DEFAULT ''"),
        ("visibility", "TEXT NOT NULL DEFAULT 'Public'"),
    ] {
        let there: i64 = c.query_row("SELECT COUNT(*) FROM pragma_table_info('member_binders') WHERE name=?1", [col], |r| r.get(0))?;
        if there == 0 {
            c.execute_batch(&format!("ALTER TABLE member_binders ADD COLUMN {col} {def}"))?;
        }
    }

    // The store: listings' stock and prices, orders, reviews, followers, views and notifications.
    crate::sellers::migrate(c)?;

    // Fill slug for existing members without one
    let missing: Vec<(i64, String)> = c.prepare("SELECT id, name FROM members WHERE slug=''")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (id, name) in missing {
        let s = unique_slug(c, &name, id)?;
        c.execute("UPDATE members SET slug=?1 WHERE id=?2", rusqlite::params![s, id])?;
    }

    // This runs on every connection, so it only writes when there's really something to move.
    let waiting: i64 = c.query_row("SELECT COUNT(*) FROM members WHERE vouch<>''", [], |r| r.get(0))?;
    if waiting == 0 {
        return Ok(());
    }
    c.execute_batch(
        "INSERT INTO vouches(member_id,text,hidden,created_at) SELECT id,vouch,CASE WHEN vouch_ok=1 THEN 0 ELSE 1 END,updated_at FROM members WHERE vouch<>'';
         UPDATE members SET vouch='',vouch_ok=0 WHERE vouch<>'';",
    )
}

pub fn slugify(name: &str) -> String {
    let mut s: String = name.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let res = s.trim_matches('-').to_string();
    if res.is_empty() {
        "seller".to_string()
    } else {
        res
    }
}

pub fn unique_slug(c: &Connection, name: &str, member_id: i64) -> rusqlite::Result<String> {
    let base = slugify(name);
    let mut candidate = base.clone();
    let mut n = 2;
    loop {
        let count: i64 = c.query_row(
            "SELECT COUNT(*) FROM members WHERE slug=?1 AND id<>?2",
            rusqlite::params![candidate, member_id],
            |r| r.get(0),
        )?;
        if count == 0 {
            return Ok(candidate);
        }
        candidate = format!("{base}-{n}");
        n += 1;
    }
}

pub fn unique_binder_slug(c: &Connection, member_id: i64, title: &str, binder_id: Option<i64>) -> rusqlite::Result<String> {
    let base = slugify(title);
    let mut candidate = base.clone();
    let mut n = 2;
    loop {
        let count: i64 = match binder_id {
            Some(bid) => c.query_row(
                "SELECT COUNT(*) FROM member_binders WHERE member_id=?1 AND slug=?2 AND id<>?3",
                rusqlite::params![member_id, candidate, bid],
                |r| r.get(0),
            )?,
            None => c.query_row(
                "SELECT COUNT(*) FROM member_binders WHERE member_id=?1 AND slug=?2",
                rusqlite::params![member_id, candidate],
                |r| r.get(0),
            )?,
        };
        if count == 0 {
            return Ok(candidate);
        }
        candidate = format!("{base}-{n}");
        n += 1;
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct MemberBinder {
    pub id: i64,
    pub member_id: i64,
    pub title: String,
    pub slug: String,
    pub cover_card: Option<i64>,
    pub position: i64,
    pub created_at: i64,
    pub count: i64,
    pub cover_image: String,
    pub description: String,
    pub visibility: String,
}

impl MemberBinder {
    pub fn count_text(&self) -> String {
        self.count.to_string()
    }
    pub fn has_image(&self) -> bool {
        !self.cover_image.is_empty()
    }
}

pub fn initial(name: &str) -> String {
    name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default()
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct MemberCard {
    pub id: i64,
    pub member_id: i64,
    pub card_id: String,
    pub title: String,
    pub number: String,
    pub card_set: String,
    pub image: String,
    pub image_large: String,
    pub cond: String,
    /// Today's market price for a Near Mint copy, in pesos. 0 until it's known.
    pub value: f64,
    /// The trainer's own figure, if they've set one. It replaces the market's.
    pub value_own: f64,
    pub binder_id: Option<i64>,
    pub status: String,
    pub expires_at: i64,
    pub is_custom: bool,
    /// Copies in stock. 0 = sold out.
    pub qty: i64,
    pub finish: String,
    pub notes: String,
    /// Copies sold through the store.
    pub sold: i64,
    pub view_n: i64,
    pub save_n: i64,
    pub updated_at: i64,
    /// The seller asked the vault to look at this custom card.
    pub flagged: bool,
}

impl MemberCard {
    /// A photo the trainer took themselves, rather than a card from the catalogue. It isn't part of the vault.
    pub fn own_photo(&self) -> bool {
        self.card_id.starts_with("own:")
    }
    /// What this copy is worth: the trainer's own figure, or the market's adjusted for its condition.
    pub fn worth(&self) -> f64 {
        if self.value_own > 0.0 { self.value_own } else { self.value * condition_factor(&self.cond) }
    }
    pub fn worth_text(&self) -> String {
        vault_search::peso(self.worth())
    }
    /// The market's figure for this copy, for the box where a trainer can replace it.
    pub fn market_text(&self) -> String {
        vault_search::peso(self.value * condition_factor(&self.cond))
    }
    pub fn own_text(&self) -> String {
        if self.value_own > 0.0 { format!("{:.0}", self.value_own) } else { String::new() }
    }
    /// The set's symbol.
    pub fn symbol(&self) -> String {
        vault_search::set_symbol(crate::sellers::catalogue_key(&self.card_id))
    }
    /// The asking price: the seller's own, or the market's for this condition.
    pub fn price_text(&self) -> String {
        vault_search::peso(self.worth())
    }
    pub fn has_price(&self) -> bool {
        self.worth() > 0.0
    }
    pub fn on_sale(&self) -> bool {
        self.status == "Active" || self.status == "Reserved"
    }
    /// Past its end date, or taken off sale by it.
    pub fn needs_update(&self) -> bool {
        self.status == "Expired" || (self.expires_at > 0 && self.expires_at < now())
    }
    pub fn expires_text(&self) -> String {
        if self.expires_at <= 0 {
            return String::new();
        }
        let left = self.expires_at - now();
        if left < 0 {
            "ended".into()
        } else if left < 86400 {
            "ends today".into()
        } else {
            format!("{} days left", left / 86400)
        }
    }
    pub fn qty_text(&self) -> String {
        match self.qty {
            0 => "Sold out".into(),
            1 => "1 left".into(),
            n => format!("{n} in stock"),
        }
    }
    pub fn when(&self) -> String {
        crate::sellers::ago(self.updated_at)
    }
    /// Which colour this card's condition gets.
    pub fn cond_class(&self) -> &'static str {
        vault_search::cond_class(&self.cond)
    }
    /// The sharpest picture there is.
    pub fn big(&self) -> &str {
        if self.image_large.is_empty() { &self.image } else { &self.image_large }
    }
    pub fn is_graded(&self) -> bool {
        crate::pricing::is_graded(&self.cond)
    }
    pub fn is_sealed(&self) -> bool {
        crate::pricing::is_sealed(&self.cond)
    }
    pub fn suggested_price(&self) -> Option<f64> {
        crate::pricing::suggest(self.value, &self.cond)
    }
    pub fn condition_share(&self) -> Option<f64> {
        crate::pricing::factor(&self.cond)
    }
}

/// A card a trainer has highlighted, with who it belongs to.
#[derive(Clone, Debug)]
pub struct Highlight {
    pub member: Member,
    pub card: MemberCard,
}

const MEMBER_COLS: &str = concat!("m.id,m.name,m.bio,m.avatar,m.highlight,m.profile_ok,(SELECT COUNT(*) FROM member_cards x WHERE x.member_id=m.id),m.locked_until,m.blocked,m.stars,m.role,", worth_sql!(), ",m.tags,m.slug,m.fb_url,m.tiktok_url,m.links,m.cover_photo,m.location,m.contact_info,m.contact_public,m.status");
const MEMBER_COUNT: usize = 22;

fn map_member(r: &Row) -> rusqlite::Result<Member> {
    Ok(Member {
        id: r.get(0)?,
        name: r.get(1)?,
        bio: r.get(2)?,
        avatar: r.get(3)?,
        highlight: r.get(4)?,
        profile_ok: r.get::<_, i64>(5)? != 0,
        cards: r.get(6)?,
        locked_until: r.get(7)?,
        blocked: r.get::<_, i64>(8)? != 0,
        stars: r.get(9)?,
        role: r.get(10)?,
        worth: r.get(11)?,
        tags: r.get(12)?,
        slug: r.get(13)?,
        fb_url: r.get(14)?,
        tiktok_url: r.get(15)?,
        links: r.get(16)?,
        cover_photo: r.get(17)?,
        location: r.get(18)?,
        contact_info: r.get(19)?,
        contact_public: r.get::<_, i64>(20)? != 0,
        status: r.get(21)?,
    })
}

pub const CARD_COLS: &str = "c.id,c.member_id,c.title,c.number,c.card_set,c.image,c.image_large,c.cond,c.card_id,c.value,c.value_own,c.binder_id,c.status,c.expires_at,c.is_custom,c.qty,c.finish,c.notes,c.sold,c.view_n,c.save_n,c.updated_at,c.review_flag";
/// How many columns `CARD_COLS` has: anything selected after it starts here.
pub const CARD_N: usize = 23;

pub fn map_card(r: &Row, at: usize) -> rusqlite::Result<MemberCard> {
    Ok(MemberCard {
        id: r.get(at)?,
        member_id: r.get(at + 1)?,
        title: r.get(at + 2)?,
        number: r.get(at + 3)?,
        card_set: r.get(at + 4)?,
        image: r.get(at + 5)?,
        image_large: r.get(at + 6)?,
        cond: r.get(at + 7)?,
        card_id: r.get(at + 8)?,
        value: r.get(at + 9)?,
        value_own: r.get(at + 10)?,
        binder_id: r.get(at + 11)?,
        status: r.get(at + 12)?,
        expires_at: r.get(at + 13)?,
        is_custom: r.get::<_, i64>(at + 14)? != 0,
        qty: r.get(at + 15)?,
        finish: r.get(at + 16)?,
        notes: r.get(at + 17)?,
        sold: r.get(at + 18)?,
        view_n: r.get(at + 19)?,
        save_n: r.get(at + 20)?,
        updated_at: r.get(at + 21)?,
        flagged: r.get::<_, i64>(at + 22)? != 0,
    })
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

// ---------- what people may type ----------
/// A single line of plain text: no control characters, spaces tidied.
pub fn clean_line(s: &str, max: usize) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().filter(|c| !c.is_control()).take(max).collect()
}

/// A few lines of plain text. Keeps line breaks, at most one blank line between paragraphs.
pub fn clean_text(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in s.replace("\r\n", "\n").split('\n') {
        let l = line.split_whitespace().collect::<Vec<_>>().join(" ");
        let l: String = l.chars().filter(|c| !c.is_control()).collect();
        if l.is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        out.push_str(&l);
        blank = 0;
    }
    out.chars().take(max).collect::<String>().trim_end().to_string()
}

/// What makes two names "the same": case, spaces and punctuation don't count.
pub fn name_key(name: &str) -> String {
    name.chars().flat_map(char::to_lowercase).filter(|c| c.is_alphanumeric()).collect()
}

/// Checks a trainer name. Letters of any alphabet, numbers, spaces and - _ . only, and nothing pretending to be the site.
pub fn valid_name(raw: &str) -> Result<String, &'static str> {
    let name = clean_line(raw, 40);
    let n = name.chars().count();
    if !(2..=24).contains(&n) {
        return Err("A trainer name needs 2 to 24 letters or numbers.");
    }
    if !name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.')) {
        return Err("A trainer name can only use letters, numbers, spaces and - _ .");
    }
    let key = name_key(&name);
    if key.is_empty() {
        return Err("A trainer name needs at least one letter or number.");
    }
    if ["admin", "administrator", "moderator", "bigboysvault", "bigboys", "vaultowner", "owner", "official", "staff", "support"].iter().any(|bad| key.contains(bad)) {
        return Err("That name is reserved. Please pick another.");
    }
    Ok(name)
}

pub fn valid_password(p: &str) -> Result<(), &'static str> {
    let n = p.chars().count();
    if n < 8 {
        Err("Your password needs at least 8 characters.")
    } else if n > 200 {
        Err("That password is too long.")
    } else {
        Ok(())
    }
}

// ---------- passwords and sessions ----------
pub fn hash_password(p: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default().hash_password(p.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| e.to_string())
}

pub fn verify_password(p: &str, stored: &str) -> bool {
    PasswordHash::new(stored).is_ok_and(|h| Argon2::default().verify_password(p.as_bytes(), &h).is_ok())
}

/// The work a real password check does, for names that don't exist, so timing doesn't reveal which names are taken.
pub fn waste_time(p: &str) {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let stored = DUMMY.get_or_init(|| hash_password("not a real password").unwrap_or_default());
    let _ = verify_password(p, stored);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn token_hash(raw: &str) -> String {
    hex(&Sha256::digest(raw.as_bytes()))
}

/// A random password for when a trainer is locked out and asks the owner for help.
pub fn temp_password() -> String {
    let mut b = [0u8; 9];
    OsRng.fill_bytes(&mut b);
    hex(&b)
}

/// Only a hash of the session token is stored, so a copy of the database can't be used to sign in.
pub fn new_session(c: &Connection, member_id: i64) -> rusqlite::Result<String> {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    let raw = hex(&b);
    let t = now();
    c.execute("DELETE FROM member_sessions WHERE expires<?1", [t])?;
    c.execute("INSERT INTO member_sessions(token,member_id,expires) VALUES(?1,?2,?3)", rusqlite::params![token_hash(&raw), member_id, t + SESSION_SECS])?;
    Ok(raw)
}

pub fn end_session(c: &Connection, raw: &str) -> rusqlite::Result<()> {
    c.execute("DELETE FROM member_sessions WHERE token=?1", [token_hash(raw)])?;
    Ok(())
}

pub fn from_session(c: &Connection, raw: &str) -> rusqlite::Result<Option<Member>> {
    let sql = format!("SELECT {MEMBER_COLS} FROM member_sessions s JOIN members m ON m.id=s.member_id WHERE s.token=?1 AND s.expires>?2 AND m.blocked=0");
    c.query_row(&sql, rusqlite::params![token_hash(raw), now()], map_member).optional()
}

// ---------- accounts ----------
pub fn create(c: &Connection, name: &str, pass_hash: &str) -> rusqlite::Result<Option<i64>> {
    let t = now();
    let r = c.execute(
        "INSERT INTO members(name,name_key,pass,created_at,updated_at) VALUES(?1,?2,?3,?4,?4) ON CONFLICT(name_key) DO NOTHING",
        rusqlite::params![name, name_key(name), pass_hash, t],
    )?;
    if r == 1 {
        let id = c.last_insert_rowid();
        let slug = unique_slug(c, name, id)?;
        c.execute("UPDATE members SET slug=?1 WHERE id=?2", rusqlite::params![slug, id])?;
        Ok(Some(id))
    } else {
        Ok(None)
    }
}

/// (id, stored password) for a sign-in attempt.
pub fn for_signin(c: &Connection, name: &str) -> rusqlite::Result<Option<(i64, String)>> {
    c.query_row("SELECT id,pass FROM members WHERE name_key=?1", [name_key(name)], |r| Ok((r.get(0)?, r.get(1)?))).optional()
}

pub fn password_of(c: &Connection, id: i64) -> rusqlite::Result<Option<String>> {
    c.query_row("SELECT pass FROM members WHERE id=?1", [id], |r| r.get(0)).optional()
}

/// A new password signs out everywhere, so a stolen session stops working.
pub fn set_password(c: &Connection, id: i64, pass_hash: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE members SET pass=?2 WHERE id=?1", rusqlite::params![id, pass_hash])?;
    c.execute("DELETE FROM member_sessions WHERE member_id=?1", [id])?;
    Ok(())
}

pub fn get(c: &Connection, id: i64) -> rusqlite::Result<Option<Member>> {
    c.query_row(&format!("SELECT {MEMBER_COLS} FROM members m WHERE m.id=?1"), [id], map_member).optional()
}

pub fn by_slug(c: &Connection, slug: &str) -> rusqlite::Result<Option<Member>> {
    let sql = format!("SELECT {MEMBER_COLS} FROM members m WHERE m.slug=?1");
    c.query_row(&sql, [slug], map_member).optional()
}

pub fn approved_sellers(c: &Connection) -> rusqlite::Result<Vec<Member>> {
    let sql = format!(
        "SELECT {MEMBER_COLS} FROM members m
         WHERE m.profile_ok=1 AND m.blocked=0
           AND ((SELECT COUNT(*) FROM member_binders b WHERE b.member_id=m.id)>0
                OR (SELECT COUNT(*) FROM member_cards x WHERE x.member_id=m.id)>0)
         ORDER BY (m.stars > 0) DESC, m.stars DESC, m.created_at DESC, m.id DESC"
    );
    let mut st = c.prepare(&sql)?;
    let mut rows: Vec<Member> = st.query_map([], map_member)?.collect::<Result<Vec<_>, _>>()?;
    rows.sort_by(|a, b| b.worth.total_cmp(&a.worth).then_with(|| b.stars.cmp(&a.stars)));
    Ok(rows)
}

pub fn vouches_count(c: &Connection, member_id: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM vouches WHERE member_id=?1 AND hidden=0", [member_id], |r| r.get(0))
}

pub fn pending_count(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM members WHERE profile_ok=0 AND blocked=0", [], |r| r.get(0))
}

pub fn unapproved_count(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM members WHERE profile_ok=0", [], |r| r.get(0))
}

/// Saves name, description and picture. Changing what people see sends the profile back for approval.
/// Returns false when the name is already taken by someone else.
pub fn save_profile(c: &Connection, m: &Member, name: &str, bio: &str, avatar: Option<&str>) -> rusqlite::Result<bool> {
    save_profile_seller(c, m, name, bio, avatar, &m.fb_url, &m.tiktok_url, &m.links)
}

pub fn save_profile_seller(
    c: &Connection,
    m: &Member,
    name: &str,
    bio: &str,
    avatar: Option<&str>,
    fb_url: &str,
    tiktok_url: &str,
    links: &str,
) -> rusqlite::Result<bool> {
    let taken: Option<i64> = c.query_row("SELECT id FROM members WHERE name_key=?1 AND id<>?2", rusqlite::params![name_key(name), m.id], |r| r.get(0)).optional()?;
    if taken.is_some() {
        return Ok(false);
    }
    let same = m.name == name && m.bio == bio && avatar.is_none_or(|a| a == m.avatar) && m.fb_url == fb_url && m.tiktok_url == tiktok_url && m.links == links;
    let slug = if m.name != name || m.slug.is_empty() {
        unique_slug(c, name, m.id)?
    } else {
        m.slug.clone()
    };
    c.execute(
        "UPDATE members SET name=?2,name_key=?3,bio=?4,avatar=COALESCE(?5,avatar),updated_at=?6,profile_ok=CASE WHEN ?7 THEN profile_ok ELSE 0 END,slug=?8,fb_url=?9,tiktok_url=?10,links=?11 WHERE id=?1",
        rusqlite::params![m.id, name, name_key(name), bio, avatar, now(), same, slug, fb_url, tiktok_url, links],
    )?;
    Ok(true)
}

pub fn binders_of(c: &Connection, member_id: i64) -> rusqlite::Result<Vec<MemberBinder>> {
    let sql = "SELECT b.id, b.member_id, b.title, b.slug, b.cover_card, b.position, b.created_at,
                      (SELECT COUNT(*) FROM member_cards x WHERE x.binder_id=b.id AND x.member_id=b.member_id),
                      COALESCE((SELECT c.image FROM member_cards c WHERE c.id=b.cover_card AND c.member_id=b.member_id),
                               (SELECT c.image FROM member_cards c WHERE c.binder_id=b.id AND c.member_id=b.member_id LIMIT 1), ''),
                      b.description, b.visibility
               FROM member_binders b
               WHERE b.member_id=?1
               ORDER BY b.position ASC, b.id ASC";
    let mut st = c.prepare(sql)?;
    let rows = st.query_map([member_id], |r| {
        Ok(MemberBinder {
            id: r.get(0)?,
            member_id: r.get(1)?,
            title: r.get(2)?,
            slug: r.get(3)?,
            cover_card: r.get(4)?,
            position: r.get(5)?,
            created_at: r.get(6)?,
            count: r.get(7)?,
            cover_image: r.get(8)?,
            description: r.get(9)?,
            visibility: r.get(10)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn binder_by_slug(c: &Connection, member_id: i64, slug: &str) -> rusqlite::Result<Option<MemberBinder>> {
    let sql = "SELECT b.id, b.member_id, b.title, b.slug, b.cover_card, b.position, b.created_at,
                      (SELECT COUNT(*) FROM member_cards x WHERE x.binder_id=b.id AND x.member_id=b.member_id),
                      COALESCE((SELECT c.image FROM member_cards c WHERE c.id=b.cover_card AND c.member_id=b.member_id),
                               (SELECT c.image FROM member_cards c WHERE c.binder_id=b.id AND c.member_id=b.member_id LIMIT 1), ''),
                      b.description, b.visibility
               FROM member_binders b
               WHERE b.member_id=?1 AND b.slug=?2";
    c.query_row(sql, rusqlite::params![member_id, slug], |r| {
        Ok(MemberBinder {
            id: r.get(0)?,
            member_id: r.get(1)?,
            title: r.get(2)?,
            slug: r.get(3)?,
            cover_card: r.get(4)?,
            position: r.get(5)?,
            created_at: r.get(6)?,
            count: r.get(7)?,
            cover_image: r.get(8)?,
            description: r.get(9)?,
            visibility: r.get(10)?,
        })
    }).optional()
}

pub fn create_member_binder(c: &Connection, member_id: i64, title: &str) -> rusqlite::Result<MemberBinder> {
    let t = title.trim();
    let title = if t.is_empty() { "New Binder" } else { t };
    let slug = unique_binder_slug(c, member_id, title, None)?;
    let t_now = now();
    c.execute(
        "INSERT INTO member_binders(member_id, title, slug, position, created_at)
         VALUES(?1, ?2, ?3, (SELECT COALESCE(MAX(position), -1) + 1 FROM member_binders WHERE member_id=?1), ?4)",
        rusqlite::params![member_id, title, slug, t_now],
    )?;
    let id = c.last_insert_rowid();
    Ok(MemberBinder {
        id,
        member_id,
        title: title.to_string(),
        slug,
        cover_card: None,
        position: 0,
        created_at: t_now,
        count: 0,
        cover_image: String::new(),
        description: String::new(),
        visibility: "Public".to_string(),
    })
}

pub fn rename_member_binder(c: &Connection, member_id: i64, binder_id: i64, title: &str) -> rusqlite::Result<bool> {
    let t = title.trim();
    if t.is_empty() {
        return Ok(false);
    }
    let slug = unique_binder_slug(c, member_id, t, Some(binder_id))?;
    let changed = c.execute(
        "UPDATE member_binders SET title=?3, slug=?4 WHERE id=?1 AND member_id=?2",
        rusqlite::params![binder_id, member_id, t, slug],
    )?;
    Ok(changed > 0)
}

pub fn delete_member_binder(c: &Connection, member_id: i64, binder_id: i64) -> rusqlite::Result<bool> {
    c.execute("UPDATE member_cards SET binder_id=NULL WHERE member_id=?1 AND binder_id=?2", rusqlite::params![member_id, binder_id])?;
    let changed = c.execute("DELETE FROM member_binders WHERE id=?1 AND member_id=?2", rusqlite::params![binder_id, member_id])?;
    Ok(changed > 0)
}

pub fn set_member_binder_cover(c: &Connection, member_id: i64, binder_id: i64, card_id: Option<i64>) -> rusqlite::Result<bool> {
    if let Some(cid) = card_id {
        let owns: i64 = c.query_row("SELECT COUNT(*) FROM member_cards WHERE id=?1 AND member_id=?2", rusqlite::params![cid, member_id], |r| r.get(0))?;
        if owns == 0 {
            return Ok(false);
        }
    }
    let changed = c.execute(
        "UPDATE member_binders SET cover_card=?3 WHERE id=?1 AND member_id=?2",
        rusqlite::params![binder_id, member_id, card_id],
    )?;
    Ok(changed > 0)
}

pub fn move_card_to_binder(c: &Connection, member_id: i64, card_id: i64, binder_id: Option<i64>) -> rusqlite::Result<bool> {
    if let Some(bid) = binder_id {
        let owns_b: i64 = c.query_row("SELECT COUNT(*) FROM member_binders WHERE id=?1 AND member_id=?2", rusqlite::params![bid, member_id], |r| r.get(0))?;
        if owns_b == 0 {
            return Ok(false);
        }
    }
    let changed = c.execute(
        "UPDATE member_cards SET binder_id=?3 WHERE id=?1 AND member_id=?2",
        rusqlite::params![card_id, member_id, binder_id],
    )?;
    Ok(changed > 0)
}

pub fn cards_of_binder(c: &Connection, member_id: i64, binder_id: Option<i64>) -> rusqlite::Result<Vec<MemberCard>> {
    let sql = format!("SELECT {CARD_COLS} FROM member_cards c WHERE c.member_id=?1 AND (?2 IS NULL OR c.binder_id=?2) ORDER BY c.position ASC, c.id ASC");
    let mut st = c.prepare(&sql)?;
    let v = st.query_map(rusqlite::params![member_id, binder_id], |r| map_card(r, 0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

pub fn set_profile_ok(c: &Connection, id: i64, ok: bool) -> rusqlite::Result<()> {
    c.execute("UPDATE members SET profile_ok=?2 WHERE id=?1", rusqlite::params![id, ok])?;
    Ok(())
}

/// Removes the trainer and everything of theirs, replies under their vouches included. Returns the files to delete.
pub fn delete(c: &Connection, id: i64) -> rusqlite::Result<Vec<String>> {
    c.execute_batch("BEGIN IMMEDIATE")?;
    let r = (|| {
        let mut files: Vec<String> = c.query_row("SELECT avatar FROM members WHERE id=?1", [id], |r| r.get(0)).optional()?.into_iter().filter(|a: &String| !a.is_empty()).collect();
        let mut st = c.prepare("SELECT path FROM vouch_images WHERE member_id=?1 OR vouch_id IN (SELECT id FROM vouches WHERE member_id=?1 OR parent_id IN (SELECT id FROM vouches WHERE member_id=?1))")?;
        files.extend(st.query_map([id], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?);
        c.execute("DELETE FROM vouch_images WHERE member_id=?1 OR vouch_id IN (SELECT id FROM vouches WHERE member_id=?1 OR parent_id IN (SELECT id FROM vouches WHERE member_id=?1))", [id])?;
        c.execute("DELETE FROM vouches WHERE member_id=?1 OR parent_id IN (SELECT id FROM vouches WHERE member_id=?1)", [id])?;
        c.execute("DELETE FROM star_awards WHERE member_id=?1", [id])?;
        let mut st = c.prepare("SELECT image, image_large FROM member_cards WHERE member_id=?1 AND card_id LIKE 'own:%'")?;
        for pair in st.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (a, b) = pair?;
            files.extend([a, b].into_iter().filter(|f| f.starts_with("/uploads/")));
        }
        c.execute("DELETE FROM member_cards WHERE member_id=?1", [id])?;
        c.execute("DELETE FROM chat_messages WHERE member_id=?1", [id])?;
        c.execute("DELETE FROM member_sessions WHERE member_id=?1", [id])?;
        c.execute("DELETE FROM members WHERE id=?1", [id])?;
        Ok(files)
    })();
    c.execute_batch(if r.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
    r
}

// ---------- the owner's controls ----------
/// No posting or changes for this many hours. Their page and vouches stay up.
pub fn lock(c: &Connection, id: i64, hours: i64) -> rusqlite::Result<()> {
    c.execute("UPDATE members SET locked_until=?2 WHERE id=?1", [id, now() + hours * 3600])?;
    Ok(())
}

pub fn unlock(c: &Connection, id: i64) -> rusqlite::Result<()> {
    c.execute("UPDATE members SET locked_until=0 WHERE id=?1", [id])?;
    Ok(())
}

/// Blocked: signed out, can't sign in, and everything of theirs comes off the site until they're unblocked.
pub fn set_blocked(c: &Connection, id: i64, blocked: bool) -> rusqlite::Result<()> {
    c.execute("UPDATE members SET blocked=?2 WHERE id=?1", rusqlite::params![id, blocked])?;
    if blocked {
        c.execute("DELETE FROM member_sessions WHERE member_id=?1", [id])?;
    }
    Ok(())
}

/// Gives a trainer a role, or takes it away.
pub fn set_tags(c: &Connection, id: i64, tags: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE members SET tags=?2 WHERE id=?1", rusqlite::params![id, tags])?;
    Ok(())
}

pub fn set_role(c: &Connection, id: i64, role: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE members SET role=?2 WHERE id=?1", rusqlite::params![id, role])?;
    Ok(())
}

pub fn role_of(c: &Connection, id: i64) -> rusqlite::Result<String> {
    c.query_row("SELECT role FROM members WHERE id=?1", [id], |r| r.get(0)).optional().map(Option::unwrap_or_default)
}

/// The role of whoever is signed in, if they hold one and can use it. Locked or blocked staff have no powers.
pub fn staff_from_session(c: &Connection, raw: &str) -> rusqlite::Result<Option<(i64, String)>> {
    let t = now();
    c.query_row(
        "SELECT m.id,m.role FROM member_sessions s JOIN members m ON m.id=s.member_id WHERE s.token=?1 AND s.expires>?2 AND m.blocked=0 AND m.locked_until<=?2 AND m.role<>''",
        rusqlite::params![token_hash(raw), t],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

/// Who wrote this vouch or reply, and their role.
pub fn vouch_author(c: &Connection, id: i64) -> rusqlite::Result<Option<(i64, String)>> {
    c.query_row("SELECT m.id,m.role FROM vouches v JOIN members m ON m.id=v.member_id WHERE v.id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()
}

/// Who uploaded this photo, and their role.
pub fn photo_author(c: &Connection, id: i64) -> rusqlite::Result<Option<(i64, String)>> {
    c.query_row("SELECT m.id,m.role FROM vouch_images i JOIN members m ON m.id=i.member_id WHERE i.id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()
}

pub fn is_blocked(c: &Connection, id: i64) -> rusqlite::Result<bool> {
    c.query_row("SELECT blocked FROM members WHERE id=?1", [id], |r| r.get::<_, i64>(0)).map(|b| b != 0)
}

pub fn give_stars(c: &Connection, id: i64, points: i64, reason: &str) -> rusqlite::Result<()> {
    c.execute("INSERT INTO star_awards(member_id,points,reason,created_at) VALUES(?1,?2,?3,?4)", rusqlite::params![id, points, reason, now()])?;
    c.execute("UPDATE members SET stars=stars+?2 WHERE id=?1", [id, points])?;
    Ok(())
}

/// Takes an award back, for when the owner tapped the wrong trainer.
pub fn undo_award(c: &Connection, member_id: i64, award_id: i64) -> rusqlite::Result<()> {
    let points: Option<i64> = c.query_row("SELECT points FROM star_awards WHERE id=?1 AND member_id=?2", [award_id, member_id], |r| r.get(0)).optional()?;
    if let Some(points) = points {
        c.execute("DELETE FROM star_awards WHERE id=?1", [award_id])?;
        c.execute("UPDATE members SET stars=MAX(0,stars-?2) WHERE id=?1", [member_id, points])?;
    }
    Ok(())
}

pub fn awards_of(c: &Connection, member_id: i64, limit: i64) -> rusqlite::Result<Vec<Award>> {
    let mut st = c.prepare("SELECT id,points,reason,created_at FROM star_awards WHERE member_id=?1 ORDER BY id DESC LIMIT ?2")?;
    let v = st.query_map([member_id, limit], |r| Ok(Award { id: r.get(0)?, points: r.get(1)?, reason: r.get(2)?, created_at: r.get(3)? }))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

// ---------- vouches and replies ----------
const VOUCH_COLS: &str = concat!("v.id,v.member_id,v.parent_id,m.name,CASE WHEN m.profile_ok=1 THEN m.avatar ELSE '' END,m.stars,v.text,v.hidden,v.created_at,m.role,", worth_sql!(), ",m.tags");

fn map_vouch(r: &Row) -> rusqlite::Result<Vouch> {
    Ok(Vouch {
        id: r.get(0)?,
        member_id: r.get(1)?,
        parent_id: r.get(2)?,
        name: r.get(3)?,
        avatar: r.get(4)?,
        stars: r.get(5)?,
        text: r.get(6)?,
        hidden: r.get::<_, i64>(7)? != 0,
        created_at: r.get(8)?,
        role: r.get(9)?,
        images: vec![],
        replies: vec![],
        worth: r.get(10)?,
        tags: r.get(11)?,
    })
}

fn images_of(c: &Connection, vouch_id: i64) -> rusqlite::Result<Vec<VouchImage>> {
    let mut st = c.prepare_cached("SELECT id,path,thumb,w,h FROM vouch_images WHERE vouch_id=?1 ORDER BY id")?;
    let v = st.query_map([vouch_id], |r| Ok(VouchImage { id: r.get(0)?, path: r.get(1)?, thumb: r.get(2)?, w: r.get(3)?, h: r.get(4)? }))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

fn fill_vouch(c: &Connection, v: &mut Vouch) -> rusqlite::Result<()> {
    v.images = images_of(c, v.id)?;
    if v.parent_id.is_none() {
        let mut st = c.prepare_cached(&format!("SELECT {VOUCH_COLS} FROM vouches v JOIN members m ON m.id=v.member_id WHERE v.parent_id=?1 AND v.hidden=0 AND m.blocked=0 ORDER BY v.id"))?;
        let mut replies = st.query_map([v.id], map_vouch)?.collect::<Result<Vec<_>, _>>()?;
        for r in &mut replies {
            r.images = images_of(c, r.id)?;
        }
        v.replies = replies;
    }
    Ok(())
}

/// Newest first. `member` narrows it to one trainer, `before` is for "Show more".
pub fn public_vouches(c: &Connection, member: Option<i64>, before: Option<i64>, limit: i64) -> rusqlite::Result<Vec<Vouch>> {
    let sql = format!("SELECT {VOUCH_COLS} FROM vouches v JOIN members m ON m.id=v.member_id WHERE v.parent_id IS NULL AND v.hidden=0 AND m.blocked=0 AND (?1 IS NULL OR v.member_id=?1) AND (?2 IS NULL OR v.id<?2) ORDER BY v.id DESC LIMIT ?3");
    let mut st = c.prepare(&sql)?;
    let mut list = st.query_map(rusqlite::params![member, before, limit], map_vouch)?.collect::<Result<Vec<_>, _>>()?;
    for v in &mut list {
        fill_vouch(c, v)?;
    }
    Ok(list)
}

/// One vouch or reply, if the public may see it.
pub fn public_vouch(c: &Connection, id: i64) -> rusqlite::Result<Option<Vouch>> {
    let sql = format!("SELECT {VOUCH_COLS} FROM vouches v JOIN members m ON m.id=v.member_id WHERE v.id=?1 AND v.hidden=0 AND m.blocked=0");
    let mut v = c.query_row(&sql, [id], map_vouch).optional()?;
    if let Some(x) = &mut v {
        fill_vouch(c, x)?;
    }
    Ok(v)
}

/// Everything, hidden or not, for the owner's screen.
pub fn admin_feed(c: &Connection, limit: i64) -> rusqlite::Result<Vec<Vouch>> {
    let mut st = c.prepare(&format!("SELECT {VOUCH_COLS} FROM vouches v JOIN members m ON m.id=v.member_id ORDER BY v.id DESC LIMIT ?1"))?;
    let mut list = st.query_map([limit], map_vouch)?.collect::<Result<Vec<_>, _>>()?;
    for v in &mut list {
        v.images = images_of(c, v.id)?;
    }
    Ok(list)
}

pub enum Posted {
    Yes(i64),
    Empty,
    NoParent,
}

/// Posts a vouch or a reply, with the photos this trainer just uploaded. Goes up straight away.
pub fn post_vouch(c: &Connection, member_id: i64, parent: Option<i64>, text: &str, photos: &[i64]) -> rusqlite::Result<Posted> {
    c.execute_batch("BEGIN IMMEDIATE")?;
    let r = (|| {
        // A reply to a reply joins the same conversation.
        let parent = match parent {
            Some(p) => {
                let found: Option<(Option<i64>, i64)> = c.query_row("SELECT v.parent_id,v.hidden FROM vouches v JOIN members m ON m.id=v.member_id WHERE v.id=?1 AND m.blocked=0", [p], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
                match found {
                    Some((up, 0)) => Some(up.unwrap_or(p)),
                    _ => return Ok(Posted::NoParent),
                }
            }
            None => None,
        };
        c.execute("INSERT INTO vouches(member_id,parent_id,text,created_at) VALUES(?1,?2,?3,?4)", rusqlite::params![member_id, parent, text, now()])?;
        let id = c.last_insert_rowid();
        let mut attached = 0;
        for p in photos {
            attached += c.execute("UPDATE vouch_images SET vouch_id=?1 WHERE id=?2 AND member_id=?3 AND vouch_id=0", [id, *p, member_id])?;
        }
        if text.is_empty() && attached == 0 {
            c.execute("DELETE FROM vouches WHERE id=?1", [id])?;
            return Ok(Posted::Empty);
        }
        Ok(Posted::Yes(id))
    })();
    c.execute_batch(if r.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
    r
}

pub fn posts_last_hour(c: &Connection, member_id: i64, replies: bool) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM vouches WHERE member_id=?1 AND created_at>?2 AND (parent_id IS NOT NULL)=?3", rusqlite::params![member_id, now() - 3600, replies], |r| r.get(0))
}

pub fn photos_last_hour(c: &Connection, member_id: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM vouch_images WHERE member_id=?1 AND created_at>?2", [member_id, now() - 3600], |r| r.get(0))
}

/// How much room all the vouch photos take.
pub fn photo_bytes(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT COALESCE(SUM(bytes),0) FROM vouch_images", [], |r| r.get(0))
}

/// A photo uploaded and waiting for its vouch to be posted.
pub fn add_photo(c: &Connection, member_id: i64, path: &str, thumb: &str, w: i64, h: i64, bytes: i64) -> rusqlite::Result<i64> {
    c.execute("INSERT INTO vouch_images(member_id,path,thumb,w,h,bytes,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)", rusqlite::params![member_id, path, thumb, w, h, bytes, now()])?;
    Ok(c.last_insert_rowid())
}

/// A photo taken out before posting. Returns its file.
pub fn drop_waiting_photo(c: &Connection, member_id: i64, id: i64) -> rusqlite::Result<Option<String>> {
    let path: Option<String> = c.query_row("SELECT path FROM vouch_images WHERE id=?1 AND member_id=?2 AND vouch_id=0", [id, member_id], |r| r.get(0)).optional()?;
    if path.is_some() {
        c.execute("DELETE FROM vouch_images WHERE id=?1", [id])?;
    }
    Ok(path)
}

/// Photos uploaded a day ago and never posted. Returns their files.
pub fn sweep_waiting_photos(c: &Connection) -> rusqlite::Result<Vec<String>> {
    let cutoff = now() - 86_400;
    let mut st = c.prepare("SELECT path FROM vouch_images WHERE vouch_id=0 AND created_at<?1")?;
    let files = st.query_map([cutoff], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    c.execute("DELETE FROM vouch_images WHERE vouch_id=0 AND created_at<?1", [cutoff])?;
    Ok(files)
}

/// Who wrote it, for checking who may delete it.
pub fn vouch_owner(c: &Connection, id: i64) -> rusqlite::Result<Option<i64>> {
    c.query_row("SELECT member_id FROM vouches WHERE id=?1", [id], |r| r.get(0)).optional()
}

pub fn set_vouch_hidden(c: &Connection, id: i64, hidden: bool) -> rusqlite::Result<()> {
    c.execute("UPDATE vouches SET hidden=?2 WHERE id=?1", rusqlite::params![id, hidden])?;
    Ok(())
}

/// Deletes a vouch with its replies and photos. Returns the files to delete.
pub fn delete_vouch(c: &Connection, id: i64) -> rusqlite::Result<Vec<String>> {
    c.execute_batch("BEGIN IMMEDIATE")?;
    let r = (|| {
        let mut st = c.prepare("SELECT path FROM vouch_images WHERE vouch_id=?1 OR vouch_id IN (SELECT id FROM vouches WHERE parent_id=?1)")?;
        let files = st.query_map([id], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        c.execute("DELETE FROM vouch_images WHERE vouch_id=?1 OR vouch_id IN (SELECT id FROM vouches WHERE parent_id=?1)", [id])?;
        c.execute("DELETE FROM vouches WHERE id=?1 OR parent_id=?1", [id])?;
        Ok(files)
    })();
    c.execute_batch(if r.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
    r
}

/// The owner takes one photo off a vouch. Returns its file. A vouch left with no words and no photos goes too.
pub fn delete_photo(c: &Connection, id: i64) -> rusqlite::Result<Option<String>> {
    let found: Option<(String, i64)> = c.query_row("SELECT path,vouch_id FROM vouch_images WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let Some((path, vouch)) = found else { return Ok(None) };
    c.execute("DELETE FROM vouch_images WHERE id=?1", [id])?;
    c.execute(
        "DELETE FROM vouches WHERE id=?1 AND text='' AND NOT EXISTS (SELECT 1 FROM vouch_images WHERE vouch_id=?1) AND NOT EXISTS (SELECT 1 FROM vouches r WHERE r.parent_id=?1)",
        [vouch],
    )?;
    Ok(Some(path))
}

// ---------- what the public sees ----------
pub fn approved(c: &Connection) -> rusqlite::Result<Vec<Member>> {
    let mut st = c.prepare(&format!("SELECT {MEMBER_COLS} FROM members m WHERE m.profile_ok=1 AND m.blocked=0 ORDER BY m.stars DESC, m.created_at DESC, m.id DESC LIMIT 500"))?;
    let v = st.query_map([], map_member)?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

/// How many trainers the Trainers page shows.
pub fn approved_count(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM members m WHERE m.profile_ok=1 AND m.blocked=0", [], |r| r.get(0))
}

/// A few trainers' faces for the home page: those with a picture first, then the most stars.
pub fn approved_faces(c: &Connection, limit: i64) -> rusqlite::Result<Vec<Member>> {
    let mut st = c.prepare(&format!("SELECT {MEMBER_COLS} FROM members m WHERE m.profile_ok=1 AND m.blocked=0 ORDER BY (m.avatar<>'') DESC, m.stars DESC, m.id DESC LIMIT ?1"))?;
    let v = st.query_map([limit], map_member)?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

pub fn approved_highlights(c: &Connection, limit: i64) -> rusqlite::Result<Vec<Highlight>> {
    let n = MEMBER_COUNT;
    let sql = format!("SELECT {MEMBER_COLS},{CARD_COLS} FROM members m JOIN member_cards c ON c.id=m.highlight AND c.member_id=m.id WHERE m.profile_ok=1 AND m.blocked=0 ORDER BY m.updated_at DESC, m.id DESC LIMIT ?1");
    let mut st = c.prepare(&sql)?;
    let v = st.query_map([limit], |r| Ok(Highlight { member: map_member(r)?, card: map_card(r, n)? }))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

/// Everyone, waiting for approval first, for the owner's screen.
pub fn all_for_admin(c: &Connection) -> rusqlite::Result<Vec<Member>> {
    let mut st = c.prepare(&format!("SELECT {MEMBER_COLS} FROM members m ORDER BY (m.profile_ok=0 AND m.blocked=0) DESC, m.updated_at DESC, m.id DESC LIMIT 1000"))?;
    let v = st.query_map([], map_member)?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

// ---------- the binder ----------
#[derive(Clone, Debug, serde::Serialize)]
pub struct SellerStats {
    pub total_listings: i64,
    pub active_listings: i64,
    pub items_sold: i64,
    pub binder_count: i64,
    pub total_value: i64,
    pub recent_views: i64,
    pub pending_inquiries: i64,
    pub pending_offers: i64,
}

pub fn seller_stats(c: &Connection, member_id: i64) -> rusqlite::Result<SellerStats> {
    let cards = cards_of(c, member_id)?;
    let total_listings = cards.len() as i64;
    let active_listings = cards.iter().filter(|c| c.status == "Active").count() as i64;
    let items_sold = cards.iter().filter(|c| c.status == "Sold").count() as i64;
    let total_value: i64 = cards.iter().filter(|c| c.status == "Active").map(|c| c.worth().round() as i64).sum();
    let binder_count: i64 = c.query_row("SELECT COUNT(*) FROM member_binders WHERE member_id=?1", [member_id], |r| r.get(0))?;
    let recent_views: i64 = c.query_row(
        "SELECT COUNT(*) FROM listing_views v JOIN member_cards c ON c.id=v.card_id WHERE c.member_id=?1 AND v.created_at >= ?2",
        rusqlite::params![member_id, now() - 30 * 86400],
        |r| r.get(0)
    ).unwrap_or(0);
    let pending_inquiries: i64 = c.query_row(
        "SELECT COUNT(*) FROM inquiries WHERE seller_id=?1",
        [member_id],
        |r| r.get(0)
    ).unwrap_or(0);
    let pending_offers: i64 = c.query_row(
        "SELECT COUNT(*) FROM offers WHERE seller_id=?1 AND status='Pending'",
        [member_id],
        |r| r.get(0)
    ).unwrap_or(0);

    Ok(SellerStats { total_listings, active_listings, items_sold, binder_count, total_value, recent_views, pending_inquiries, pending_offers })
}

pub fn cards_of(c: &Connection, member_id: i64) -> rusqlite::Result<Vec<MemberCard>> {
    let mut st = c.prepare(&format!("SELECT {CARD_COLS} FROM member_cards c WHERE c.member_id=?1 ORDER BY c.position,c.id"))?;
    let v = st.query_map([member_id], |r| map_card(r, 0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

pub fn card(c: &Connection, member_id: i64, card_id: i64) -> rusqlite::Result<Option<MemberCard>> {
    c.query_row(&format!("SELECT {CARD_COLS} FROM member_cards c WHERE c.member_id=?1 AND c.id=?2"), [member_id, card_id], |r| map_card(r, 0)).optional()
}

pub enum Added {
    Yes(i64),
    Duplicate,
    Full,
}

/// Adds a card, inside one write so two quick taps can't pass the limit of 50. The first card becomes the highlight.
pub fn add_card(c: &Connection, member_id: i64, card_id: &str, title: &str, number: &str, set: &str, image: &str, image_large: &str) -> rusqlite::Result<Added> {
    c.execute_batch("BEGIN IMMEDIATE")?;
    let r = (|| {
        let n: i64 = c.query_row("SELECT COUNT(*) FROM member_cards WHERE member_id=?1", [member_id], |r| r.get(0))?;
        if n >= crate::sellers::max_cards(c) {
            return Ok(Added::Full);
        }
        let done = c.execute(
            "INSERT INTO member_cards(member_id,position,card_id,title,number,card_set,image,image_large) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(member_id,card_id) DO NOTHING",
            rusqlite::params![member_id, n, card_id, title, number, set, image, image_large],
        )?;
        if done == 0 {
            return Ok(Added::Duplicate);
        }
        let id = c.last_insert_rowid();
        c.execute("UPDATE members SET highlight=?2 WHERE id=?1 AND highlight IS NULL", [member_id, id])?;
        Ok(Added::Yes(id))
    })();
    match r {
        Ok(v) => {
            c.execute_batch("COMMIT")?;
            Ok(v)
        }
        Err(e) => {
            let _ = c.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

pub fn remove_card(c: &Connection, member_id: i64, card_id: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM member_cards WHERE member_id=?1 AND id=?2", [member_id, card_id])?;
    // The next card takes over as the highlight.
    c.execute(
        "UPDATE members SET highlight=(SELECT id FROM member_cards WHERE member_id=?1 ORDER BY position,id LIMIT 1) WHERE id=?1 AND highlight=?2",
        [member_id, card_id],
    )?;
    Ok(())
}

pub fn set_condition(c: &Connection, member_id: i64, card_id: i64, cond: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE member_cards SET cond=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, card_id, cond])?;
    Ok(())
}

pub fn set_highlight(c: &Connection, member_id: i64, card_id: i64) -> rusqlite::Result<bool> {
    let n = c.execute(
        "UPDATE members SET highlight=?2, updated_at=?3 WHERE id=?1 AND EXISTS(SELECT 1 FROM member_cards WHERE member_id=?1 AND id=?2)",
        [member_id, card_id, now()],
    )?;
    Ok(n == 1)
}

pub fn reorder_cards(c: &Connection, member_id: i64, ids: &[i64]) -> rusqlite::Result<()> {
    for (pos, id) in ids.iter().enumerate() {
        c.execute("UPDATE member_cards SET position=?3 WHERE member_id=?1 AND id=?2", [member_id, *id, pos as i64])?;
    }
    Ok(())
}

/// The market's price for every copy of a card.
pub fn set_card_value(c: &Connection, card_id: &str, value: f64, now: i64) -> rusqlite::Result<usize> {
    // A second listing of the same card is "id#2": it follows the market too.
    c.execute("UPDATE member_cards SET value=?2, value_at=?3 WHERE card_id=?1 OR card_id LIKE ?1 || '#%'", rusqlite::params![card_id, value, now])
}

/// A trainer's own figure for one card. 0 goes back to the market's.
pub fn set_own_value(c: &Connection, member_id: i64, card_id: i64, value: f64) -> rusqlite::Result<usize> {
    c.execute("UPDATE member_cards SET value_own=?3 WHERE member_id=?1 AND id=?2", rusqlite::params![member_id, card_id, value])
}

/// Cards whose market price hasn't been looked at since `before`.
pub fn cards_to_price(c: &Connection, before: i64, limit: i64) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare("SELECT card_id FROM member_cards WHERE value_at<?1 AND card_id NOT LIKE 'own:%' GROUP BY card_id ORDER BY MIN(value_at) LIMIT ?2")?;
    let v = st.query_map(rusqlite::params![before, limit], |r| r.get(0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

/// What the vault itself asks for this card, for when the market has no price.
pub fn vault_price(c: &Connection, card_id: &str) -> rusqlite::Result<f64> {
    c.query_row("SELECT COALESCE(MIN(price),0) FROM products WHERE api_id=?1 AND price>0", [card_id], |r| r.get(0))
}

/* ---------- the chat ---------- */
#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub id: i64,
    pub member_id: i64,
    pub name: String,
    pub avatar: String,
    pub role: String,
    pub stars: i64,
    pub text: String,
    pub created_at: i64,
    pub worth: f64,
    pub tags: String,
}

const CHAT_COLS: &str = concat!("h.id,h.member_id,m.name,CASE WHEN m.profile_ok=1 THEN m.avatar ELSE '' END,m.role,m.stars,h.text,h.created_at,", worth_sql!(), ",m.tags");

fn map_chat(r: &Row) -> rusqlite::Result<ChatMessage> {
    Ok(ChatMessage { id: r.get(0)?, member_id: r.get(1)?, name: r.get(2)?, avatar: r.get(3)?, role: r.get(4)?, stars: r.get(5)?, text: r.get(6)?, created_at: r.get(7)?, worth: r.get(8)?, tags: r.get(9)? })
}

pub fn chat_post(c: &Connection, member_id: i64, text: &str, now: i64) -> rusqlite::Result<i64> {
    c.execute("INSERT INTO chat_messages(member_id,text,created_at) VALUES(?1,?2,?3)", rusqlite::params![member_id, text, now])?;
    let id = c.last_insert_rowid();
    if id % 200 == 0 {
        c.execute("DELETE FROM chat_messages WHERE id<=?1", [id - CHAT_KEEP])?;
    }
    Ok(id)
}

/// The newest messages, or those after or before one, oldest first.
pub fn chat_read(c: &Connection, after: Option<i64>, before: Option<i64>, limit: i64) -> rusqlite::Result<Vec<ChatMessage>> {
    let visible = "h.hidden=0 AND m.blocked=0";
    let mut list = if let Some(a) = after {
        let mut st = c.prepare_cached(&format!("SELECT {CHAT_COLS} FROM chat_messages h JOIN members m ON m.id=h.member_id WHERE {visible} AND h.id>?1 ORDER BY h.id LIMIT ?2"))?;
        let v = st.query_map(rusqlite::params![a, limit], map_chat)?.collect::<Result<Vec<_>, _>>()?;
        v
    } else {
        let mut st = c.prepare_cached(&format!("SELECT {CHAT_COLS} FROM chat_messages h JOIN members m ON m.id=h.member_id WHERE {visible} AND (?1 IS NULL OR h.id<?1) ORDER BY h.id DESC LIMIT ?2"))?;
        let mut v = st.query_map(rusqlite::params![before, limit], map_chat)?.collect::<Result<Vec<_>, _>>()?;
        v.reverse();
        v
    };
    list.truncate(limit as usize);
    Ok(list)
}

/// Recent messages that have come down, so open windows can drop them.
pub fn chat_gone(c: &Connection, above: i64) -> rusqlite::Result<Vec<i64>> {
    let mut st = c.prepare_cached("SELECT h.id FROM chat_messages h JOIN members m ON m.id=h.member_id WHERE h.id>?1 AND (h.hidden=1 OR m.blocked=1) ORDER BY h.id")?;
    let v = st.query_map([above], |r| r.get(0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

/// Who wrote a message, and their role.
pub fn chat_author(c: &Connection, id: i64) -> rusqlite::Result<Option<(i64, String)>> {
    c.query_row("SELECT h.member_id,m.role FROM chat_messages h JOIN members m ON m.id=h.member_id WHERE h.id=?1 AND h.hidden=0", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()
}

pub fn chat_hide(c: &Connection, id: i64) -> rusqlite::Result<usize> {
    c.execute("UPDATE chat_messages SET hidden=1 WHERE id=?1", [id])
}

pub fn chat_recent_count(c: &Connection, member_id: i64, since: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM chat_messages WHERE member_id=?1 AND created_at>=?2", rusqlite::params![member_id, since], |r| r.get(0))
}

pub fn chat_last_text(c: &Connection, member_id: i64, since: i64) -> rusqlite::Result<Option<String>> {
    c.query_row("SELECT text FROM chat_messages WHERE member_id=?1 AND created_at>=?2 ORDER BY id DESC LIMIT 1", rusqlite::params![member_id, since], |r| r.get(0)).optional()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_and_chat() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA).unwrap();
        c.execute_batch("CREATE TABLE products(id INTEGER, api_id TEXT, price REAL)").unwrap();
        migrate(&c).unwrap();
        migrate(&c).unwrap();
        for n in ["a", "b"] {
            c.execute("INSERT INTO members(name,name_key,pass,created_at,updated_at,profile_ok) VALUES(?1,?1,'x',0,0,1)", [n]).unwrap();
        }
        let Added::Yes(one) = add_card(&c, 1, "tcgdex:x-1", "One", "", "", "", "").unwrap() else { panic!() };
        let Added::Yes(two) = add_card(&c, 1, "tcgdex:x-2", "Two", "", "", "", "").unwrap() else { panic!() };
        assert_eq!(cards_to_price(&c, 100, 10).unwrap().len(), 2);
        set_card_value(&c, "tcgdex:x-1", 1000.0, 200).unwrap();
        set_card_value(&c, "tcgdex:x-2", 500.0, 200).unwrap();
        assert_eq!(cards_to_price(&c, 100, 10).unwrap().len(), 0);
        set_condition(&c, 1, two, "Moderately Played").unwrap();
        assert_eq!(get(&c, 1).unwrap().unwrap().worth, 1300.0, "1000 + 500 at 60%");
        let saved = card(&c, 1, two).unwrap().unwrap();
        assert_eq!(saved.worth(), 300.0);
        set_own_value(&c, 1, one, 5000.0).unwrap();
        assert_eq!(get(&c, 1).unwrap().unwrap().worth, 5300.0, "the trainer's figure replaces the market's");
        assert_eq!(card(&c, 1, one).unwrap().unwrap().worth(), 5000.0);
        assert_eq!(set_own_value(&c, 2, one, 9.0).unwrap(), 0, "someone else's card can't be touched");

        let first = chat_post(&c, 1, "hello", 10).unwrap();
        let second = chat_post(&c, 2, "hi there", 11).unwrap();
        let all = chat_read(&c, None, None, 10).unwrap();
        assert_eq!(all.iter().map(|m| m.id).collect::<Vec<_>>(), vec![first, second]);
        assert_eq!(all[0].worth, 5300.0);
        assert_eq!(chat_read(&c, Some(first), None, 10).unwrap().len(), 1);
        assert_eq!(chat_read(&c, None, Some(second), 10).unwrap()[0].id, first);
        assert_eq!(chat_author(&c, first).unwrap().unwrap().0, 1);
        chat_hide(&c, first).unwrap();
        assert_eq!(chat_read(&c, None, None, 10).unwrap().len(), 1);
        assert_eq!(chat_gone(&c, 0).unwrap(), vec![first]);
        assert!(chat_author(&c, first).unwrap().is_none());
        set_blocked(&c, 2, true).unwrap();
        assert!(chat_read(&c, None, None, 10).unwrap().is_empty(), "a blocked trainer's messages come down too");
    }

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA).unwrap();
        migrate(&c).unwrap();
        c
    }

    #[test]
    fn names_are_checked_and_unique_whatever_the_case() {
        assert_eq!(valid_name("  Ash   Ketchum "), Ok("Ash Ketchum".to_string()));
        assert_eq!(valid_name("サトシ"), Ok("サトシ".to_string()));
        assert!(valid_name("a").is_err());
        assert!(valid_name("<script>").is_err());
        assert!(valid_name("Big Boys Vault").is_err());
        assert!(valid_name("the.admin").is_err());
        assert!(valid_name("x".repeat(25).as_str()).is_err());
        let c = db();
        assert!(create(&c, "Ash", "h").unwrap().is_some());
        assert!(create(&c, "ASH", "h").unwrap().is_none());
        assert!(create(&c, "a-s-h", "h").unwrap().is_none());
    }

    #[test]
    fn text_is_tidied() {
        assert_eq!(clean_text("  one\r\n\r\n\r\n two  \u{7}", 100), "one\n\ntwo");
        assert_eq!(clean_line("a \n  b", 100), "a b");
        assert_eq!(clean_text("abcdef", 3), "abc");
    }

    #[test]
    fn passwords_and_sessions() {
        let h = hash_password("correct horse").unwrap();
        assert!(h.starts_with("$argon2"));
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("x", "not a hash"));
        let c = db();
        let id = create(&c, "Misty", &h).unwrap().unwrap();
        let raw = new_session(&c, id).unwrap();
        assert_eq!(from_session(&c, &raw).unwrap().unwrap().name, "Misty");
        // The database holds only a hash of the token.
        let stored: String = c.query_row("SELECT token FROM member_sessions", [], |r| r.get(0)).unwrap();
        assert_ne!(stored, raw);
        assert!(from_session(&c, &stored).unwrap().is_none());
        set_password(&c, id, &h).unwrap();
        assert!(from_session(&c, &raw).unwrap().is_none(), "a new password signs out everywhere");
    }

    #[test]
    fn a_page_waits_for_approval_and_changes_need_approval_again() {
        let c = db();
        let id = create(&c, "Brock", "h").unwrap().unwrap();
        assert!(approved(&c).unwrap().is_empty());
        set_profile_ok(&c, id, true).unwrap();
        assert_eq!(approved(&c).unwrap().len(), 1);
        let m = get(&c, id).unwrap().unwrap();
        assert!(save_profile(&c, &m, "Brock", "", None).unwrap());
        assert!(get(&c, id).unwrap().unwrap().profile_ok, "saving nothing new changes nothing");
        assert!(save_profile(&c, &m, "Brock", "New words", None).unwrap());
        assert!(!get(&c, id).unwrap().unwrap().profile_ok);
        assert!(approved(&c).unwrap().is_empty());
    }

    #[test]
    fn vouches_post_straight_away_and_the_owner_can_take_them_down() {
        let c = db();
        let a = create(&c, "Ash", "h").unwrap().unwrap();
        let b = create(&c, "Misty", "h").unwrap().unwrap();
        // No approval needed, and a trainer can post as many as they like.
        let Posted::Yes(v1) = post_vouch(&c, a, None, "Great vault", &[]).unwrap() else { panic!() };
        let Posted::Yes(v2) = post_vouch(&c, a, None, "Still great", &[]).unwrap() else { panic!() };
        assert_eq!(public_vouches(&c, None, None, 10).unwrap().len(), 2);
        assert_eq!(public_vouches(&c, None, Some(v2), 10).unwrap().len(), 1, "Show more starts after the last one");
        // Nothing to say and no photo: nothing is posted.
        assert!(matches!(post_vouch(&c, a, None, "", &[]).unwrap(), Posted::Empty));
        // Photos join the post, but only the trainer's own, and only once.
        let p1 = add_photo(&c, a, "/uploads/p1_l.jpg", "/uploads/p1_s.jpg", 10, 10, 100).unwrap();
        let p2 = add_photo(&c, b, "/uploads/p2_l.jpg", "/uploads/p2_s.jpg", 10, 10, 100).unwrap();
        let Posted::Yes(v3) = post_vouch(&c, a, None, "", &[p1, p2]).unwrap() else { panic!() };
        assert_eq!(public_vouch(&c, v3).unwrap().unwrap().images.len(), 1);
        assert!(drop_waiting_photo(&c, b, p1).unwrap().is_none(), "a posted photo isn't waiting any more");
        assert_eq!(drop_waiting_photo(&c, b, p2).unwrap().as_deref(), Some("/uploads/p2_l.jpg"));
        // Replies sit under the vouch, and a reply to a reply joins the same conversation.
        let Posted::Yes(r1) = post_vouch(&c, b, Some(v1), "Agreed", &[]).unwrap() else { panic!() };
        let Posted::Yes(r2) = post_vouch(&c, a, Some(r1), "Thanks", &[]).unwrap() else { panic!() };
        let thread = public_vouch(&c, v1).unwrap().unwrap();
        assert_eq!(thread.replies.len(), 2);
        assert_eq!(public_vouch(&c, r2).unwrap().unwrap().parent_id, Some(v1));
        assert_eq!(public_vouches(&c, None, None, 10).unwrap().len(), 3, "replies aren't top-level vouches");
        assert_eq!(posts_last_hour(&c, a, true).unwrap(), 1);
        assert_eq!(posts_last_hour(&c, a, false).unwrap(), 3);
        // Hidden comes off the site, and can't be replied to.
        set_vouch_hidden(&c, v1, true).unwrap();
        assert!(public_vouch(&c, v1).unwrap().is_none());
        assert!(matches!(post_vouch(&c, b, Some(v1), "Hello?", &[]).unwrap(), Posted::NoParent));
        assert_eq!(admin_feed(&c, 50).unwrap().len(), 5, "the owner still sees it");
        set_vouch_hidden(&c, v1, false).unwrap();
        // Deleting a vouch takes its replies with it.
        delete_vouch(&c, v1).unwrap();
        assert!(public_vouch(&c, r1).unwrap().is_none());
        let files = delete_vouch(&c, v3).unwrap();
        assert_eq!(files, vec!["/uploads/p1_l.jpg".to_string()]);
    }

    #[test]
    fn locking_blocking_and_stars() {
        let c = db();
        let id = create(&c, "Gary", "h").unwrap().unwrap();
        set_profile_ok(&c, id, true).unwrap();
        let raw = new_session(&c, id).unwrap();
        let Posted::Yes(v) = post_vouch(&c, id, None, "Smell ya later", &[]).unwrap() else { panic!() };
        // Locked: still here, but marked.
        assert!(!get(&c, id).unwrap().unwrap().locked());
        lock(&c, id, 48).unwrap();
        let m = get(&c, id).unwrap().unwrap();
        assert!(m.locked());
        assert_eq!(m.lock_left(), "2 days");
        assert!(from_session(&c, &raw).unwrap().is_some());
        assert!(public_vouch(&c, v).unwrap().is_some());
        unlock(&c, id).unwrap();
        assert!(!get(&c, id).unwrap().unwrap().locked());
        // Blocked: signed out and off the site, until unblocked.
        set_blocked(&c, id, true).unwrap();
        assert!(from_session(&c, &raw).unwrap().is_none());
        assert!(is_blocked(&c, id).unwrap());
        assert!(approved(&c).unwrap().is_empty());
        assert!(public_vouch(&c, v).unwrap().is_none());
        assert!(public_vouches(&c, None, None, 10).unwrap().is_empty());
        set_blocked(&c, id, false).unwrap();
        assert_eq!(approved(&c).unwrap().len(), 1);
        assert!(public_vouch(&c, v).unwrap().is_some());
        // Stars add up, and a mistake can be taken back.
        give_stars(&c, id, 5, "Best binder").unwrap();
        give_stars(&c, id, 10, "").unwrap();
        assert_eq!(get(&c, id).unwrap().unwrap().stars, 15);
        let awards = awards_of(&c, id, 10).unwrap();
        assert_eq!(awards[1].reason, "Best binder");
        undo_award(&c, id, awards[0].id).unwrap();
        assert_eq!(get(&c, id).unwrap().unwrap().stars, 5);
        // Removing a trainer removes everything of theirs.
        let other = create(&c, "Oak", "h").unwrap().unwrap();
        let Posted::Yes(r) = post_vouch(&c, other, Some(v), "Good", &[]).unwrap() else { panic!() };
        delete(&c, id).unwrap();
        assert!(public_vouch(&c, r).unwrap().is_none());
        assert!(awards_of(&c, id, 10).unwrap().is_empty());
    }

    #[test]
    fn staff_powers_go_with_the_role_and_stop_when_locked_or_blocked() {
        let c = db();
        let id = create(&c, "Lance", "h").unwrap().unwrap();
        let raw = new_session(&c, id).unwrap();
        assert!(staff_from_session(&c, &raw).unwrap().is_none(), "a plain trainer has no powers");
        set_role(&c, id, "mod").unwrap();
        assert_eq!(staff_from_session(&c, &raw).unwrap(), Some((id, "mod".to_string())));
        assert_eq!(get(&c, id).unwrap().unwrap().role_text(), "Moderator");
        set_role(&c, id, "admin").unwrap();
        assert_eq!(staff_from_session(&c, &raw).unwrap(), Some((id, "admin".to_string())));
        assert!(staff_from_session(&c, "somebody else's token").unwrap().is_none());
        lock(&c, id, 2).unwrap();
        assert!(staff_from_session(&c, &raw).unwrap().is_none(), "locked: no powers");
        unlock(&c, id).unwrap();
        assert!(staff_from_session(&c, &raw).unwrap().is_some());
        set_blocked(&c, id, true).unwrap();
        assert!(staff_from_session(&c, &raw).unwrap().is_none(), "blocked: no powers");
        set_blocked(&c, id, false).unwrap();
        let raw = new_session(&c, id).unwrap();
        set_role(&c, id, "").unwrap();
        assert!(staff_from_session(&c, &raw).unwrap().is_none(), "role taken away: no powers, at once");
        assert_eq!(role_of(&c, 9999).unwrap(), "");
    }

    #[test]
    fn old_vouches_move_over_and_hidden_ones_stay_hidden() {
        let c = db();
        let a = create(&c, "Ash", "h").unwrap().unwrap();
        let b = create(&c, "Misty", "h").unwrap().unwrap();
        c.execute("UPDATE members SET vouch='Approved one',vouch_ok=1 WHERE id=?1", [a]).unwrap();
        c.execute("UPDATE members SET vouch='Hidden one',vouch_ok=0 WHERE id=?1", [b]).unwrap();
        migrate(&c).unwrap();
        migrate(&c).unwrap();
        let live = public_vouches(&c, None, None, 10).unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].text, "Approved one");
        assert_eq!(admin_feed(&c, 10).unwrap().len(), 2);
    }

    #[test]
    fn the_binder_stops_at_fifty_and_keeps_a_highlight() {
        let c = db();
        let a = create(&c, "Gary", "h").unwrap().unwrap();
        let b = create(&c, "Oak", "h").unwrap().unwrap();
        set_profile_ok(&c, a, true).unwrap();
        let mut first = 0;
        for i in 0..MAX_CARDS {
            match add_card(&c, a, &format!("c{i}"), "T", "1", "S", "i", "l").unwrap() {
                Added::Yes(id) => if i == 0 { first = id },
                _ => panic!("should fit"),
            }
        }
        assert!(matches!(add_card(&c, a, "more", "T", "1", "S", "i", "l").unwrap(), Added::Full));
        assert!(matches!(add_card(&c, b, "c0", "T", "1", "S", "i", "l").unwrap(), Added::Yes(_)));
        assert!(matches!(add_card(&c, b, "c0", "T", "1", "S", "i", "l").unwrap(), Added::Duplicate));
        assert_eq!(get(&c, a).unwrap().unwrap().highlight, Some(first));
        assert_eq!(approved_highlights(&c, 8).unwrap().len(), 1);
        assert_eq!(approved_count(&c).unwrap(), approved(&c).unwrap().len() as i64);
        assert_eq!(approved_faces(&c, 8).unwrap().len() as i64, approved_count(&c).unwrap());
        // Another trainer's card can't be touched.
        assert!(!set_highlight(&c, b, first).unwrap());
        remove_card(&c, b, first).unwrap();
        assert!(card(&c, a, first).unwrap().is_some());
        remove_card(&c, a, first).unwrap();
        assert_ne!(get(&c, a).unwrap().unwrap().highlight, Some(first));
        delete(&c, a).unwrap();
        assert!(cards_of(&c, a).unwrap().is_empty());
    }

    #[test]
    fn slug_making_and_uniqueness() {
        assert_eq!(slugify("Big Boys Vault"), "big-boys-vault");
        assert_eq!(slugify("Ash Ketchum! #1"), "ash-ketchum-1");
        assert_eq!(slugify("---test---"), "test");
        assert_eq!(slugify(""), "seller");

        let c = db();
        let a = create(&c, "Brock", "h").unwrap().unwrap();
        let m_a = get(&c, a).unwrap().unwrap();
        assert_eq!(m_a.slug, "brock");

        let b = create(&c, "Brock 2", "h").unwrap().unwrap();
        let m_b = get(&c, b).unwrap().unwrap();
        assert_eq!(m_b.slug, "brock-2");
    }

    #[test]
    fn binder_ownership_checks() {
        let c = db();
        let a = create(&c, "Red", "h").unwrap().unwrap();
        let b = create(&c, "Blue", "h").unwrap().unwrap();

        // Trainer A creates binder
        let binder_a = create_member_binder(&c, a, "Grails").unwrap();
        assert_eq!(binder_a.slug, "grails");
        assert_eq!(binders_of(&c, a).unwrap().len(), 1);
        assert_eq!(binders_of(&c, b).unwrap().len(), 0);

        // Trainer B cannot rename A's binder
        assert!(!rename_member_binder(&c, b, binder_a.id, "Stolen").unwrap());
        let a_check = binder_by_slug(&c, a, "grails").unwrap().unwrap();
        assert_eq!(a_check.title, "Grails");

        // Trainer A adds a card
        let Added::Yes(card_a) = add_card(&c, a, "tcgdex:pika", "Pikachu", "1", "Set", "", "").unwrap() else { panic!() };
        // Trainer B adds a card
        let Added::Yes(card_b) = add_card(&c, b, "tcgdex:char", "Charizard", "2", "Set", "", "").unwrap() else { panic!() };

        // Trainer B cannot move their card into Trainer A's binder
        assert!(!move_card_to_binder(&c, b, card_b, Some(binder_a.id)).unwrap());
        // Trainer B cannot move Trainer A's card
        assert!(!move_card_to_binder(&c, b, card_a, None).unwrap());

        // Trainer A can move their card into their binder
        assert!(move_card_to_binder(&c, a, card_a, Some(binder_a.id)).unwrap());
        assert_eq!(cards_of_binder(&c, a, Some(binder_a.id)).unwrap().len(), 1);

        // Trainer B cannot set cover or delete A's binder
        assert!(!set_member_binder_cover(&c, b, binder_a.id, Some(card_b)).unwrap());
        assert!(!delete_member_binder(&c, b, binder_a.id).unwrap());
        assert_eq!(binders_of(&c, a).unwrap().len(), 1);

        // Trainer A can delete their binder; card stays in collection with binder_id=None
        assert!(delete_member_binder(&c, a, binder_a.id).unwrap());
        assert_eq!(binders_of(&c, a).unwrap().len(), 0);
        let cards_after = cards_of(&c, a).unwrap();
        assert_eq!(cards_after.len(), 1);
        assert_eq!(cards_after[0].binder_id, None);
    }
}

/// Trainers and what they've said lately, for the owner's dashboard: (live pages, waiting, vouches this week, chat this week).
pub fn community_numbers(c: &Connection, since: i64) -> rusqlite::Result<(i64, i64, i64, i64)> {
    Ok((
        c.query_row("SELECT COUNT(*) FROM members WHERE profile_ok=1 AND blocked=0", [], |r| r.get(0))?,
        c.query_row("SELECT COUNT(*) FROM members WHERE profile_ok=0 AND blocked=0", [], |r| r.get(0))?,
        c.query_row("SELECT COUNT(*) FROM vouches WHERE hidden=0 AND created_at>=?1", [since], |r| r.get(0))?,
        c.query_row("SELECT COUNT(*) FROM chat_messages WHERE hidden=0 AND created_at>=?1", [since], |r| r.get(0))?,
    ))
}

/// Whose card this is, their role, and the card itself, for staff taking it down.
pub fn card_owner(c: &Connection, card_id: i64) -> rusqlite::Result<Option<(i64, String, MemberCard)>> {
    let row: Option<(i64, String)> = c.query_row("SELECT c.member_id, m.role FROM member_cards c JOIN members m ON m.id=c.member_id WHERE c.id=?1", [card_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let Some((owner, role)) = row else { return Ok(None) };
    Ok(card(c, owner, card_id)?.map(|k| (owner, role, k)))
}

// ---------- notifications ----------
#[derive(Clone, Debug, serde::Serialize)]
pub struct Notification {
    pub id: i64,
    pub member_id: i64,
    pub kind: String,
    pub message: String,
    pub link: String,
    pub is_read: bool,
    pub created_at: i64,
}

pub fn notifications(c: &Connection, member_id: i64, unread_only: bool) -> rusqlite::Result<Vec<Notification>> {
    let mut sql = "SELECT id, member_id, kind, message, link, is_read, created_at FROM notifications WHERE member_id=?1".to_string();
    if unread_only {
        sql.push_str(" AND is_read=0");
    }
    sql.push_str(" ORDER BY created_at DESC, id DESC LIMIT 100");
    let mut st = c.prepare(&sql)?;
    let rows = st.query_map([member_id], |r| {
        Ok(Notification {
            id: r.get(0)?,
            member_id: r.get(1)?,
            kind: r.get(2)?,
            message: r.get(3)?,
            link: r.get(4)?,
            is_read: r.get::<_, i64>(5)? != 0,
            created_at: r.get(6)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn unread_notification_count(c: &Connection, member_id: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM notifications WHERE member_id=?1 AND is_read=0", [member_id], |r| r.get(0))
}

pub fn mark_notifications_read(c: &Connection, member_id: i64, before_id: Option<i64>) -> rusqlite::Result<usize> {
    if let Some(bid) = before_id {
        c.execute("UPDATE notifications SET is_read=1 WHERE member_id=?1 AND is_read=0 AND id<=?2", rusqlite::params![member_id, bid])
    } else {
        c.execute("UPDATE notifications SET is_read=1 WHERE member_id=?1 AND is_read=0", [member_id])
    }
}

pub fn insert_notification(c: &Connection, member_id: i64, kind: &str, message: &str, link: &str) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO notifications(member_id, kind, message, link, created_at) VALUES(?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![member_id, kind, message, link, now()],
    )?;
    Ok(())
}
