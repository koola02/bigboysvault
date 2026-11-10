//! SQLite storage. WAL + mmap keeps reads cheap; writers (the importer) never block readers.
use rusqlite::{params_from_iter, types::Value, Connection, OptionalExtension, Row};

pub const PAGE_SIZE: usize = 48;

/// A binder's leather colour, one of six, worked out from its handle so it never changes.
pub fn tone(handle: &str) -> u8 {
    (handle.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b))) % 6) as u8
}

/// The conditions offered wherever the owner picks one.
pub const CONDITIONS: [&str; 15] = ["Near Mint", "LP-NM", "Lightly Played", "MP-LP", "Moderately Played", "HP-MP", "Heavily Played", "Damaged-HP", "Damaged", "Sealed", "PSA 10", "PSA 9", "CGC 10", "CGC 9.5", "BGS 9.5"];

#[derive(Clone, Debug)]
pub struct Product {
    pub id: i64,
    pub handle: String,
    pub title: String,
    pub body: String,
    pub tags: String,
    pub variant: String,
    pub price: f64,
    pub compare_at: Option<f64>,
    pub available: bool,
    pub image: String,
    pub image_w: i64,
    pub image_h: i64,
    pub card_no: String,
    pub card_set: String,
    pub api_id: String,
    pub finish: String,
    pub price_updated: String,
    pub price_src: String,
    /// "Illustration rare", "Full Art"... from the card catalogue.
    pub rarity: String,
    /// Today's Near Mint market price for this card's finish, in pesos (0 = not looked up yet). The SRP is worked out from it.
    pub market_php: f64,
    /// Copies of this same card (same card, same condition/grade) — lists show one tile with "×2".
    pub qty: i64,
    /// Every copy's id, comma-separated (lists only), so admin actions can act on all copies.
    pub copy_ids: String,
}

#[derive(Clone, Debug)]
pub struct Collection {
    pub handle: String,
    pub title: String,
    /// Cover: the owner's choice, or else the most valuable card's photo.
    pub image: String,
    pub count: i64,
    /// Shown in the Binders section of the home page.
    pub on_home: bool,
    /// The owner picked this cover (an upload or a card); false = automatic.
    pub custom_cover: bool,
    /// Shown in the site's top menu, at `menu_pos` (its own order, separate from the home page).
    pub in_menu: bool,
    pub menu_pos: i64,
}

impl Product {
    pub fn price_text(&self) -> String {
        peso(self.price)
    }
    pub fn compare_text(&self) -> String {
        self.compare_at.map(peso).unwrap_or_default()
    }
    pub fn on_sale(&self) -> bool {
        self.compare_at.map_or(false, |c| c > self.price)
    }
    pub fn has_image(&self) -> bool {
        !self.image.is_empty()
    }
    /// Responsive Shopify CDN image URL at a given pixel width.
    pub fn img(&self, w: u32) -> String {
        cdn(&self.image, w)
    }
    /// The condition the SRP is for: what's written, or Near Mint when nothing is.
    fn srp_condition(&self) -> &str {
        let v = self.variant.trim();
        if v.is_empty() { "Near Mint" } else { v }
    }
    /// The suggested retail price in pesos for this card's condition, from today's Near Mint market price.
    /// None for graded and sealed cards, unknown conditions, and cards with no market price yet.
    pub fn srp(&self) -> Option<f64> {
        crate::pricing::suggest(self.market_php, self.srp_condition())
    }
    /// One line for the owner: "SRP ₱8 · Lightly Played · 80% of Near Mint ₱10". Only a guide: the owner sets the price.
    pub fn srp_line(&self) -> String {
        if self.api_id.is_empty() {
            return String::new();
        }
        if self.market_php <= 0.0 {
            return "SRP: no market price yet".into();
        }
        let nm = peso(self.market_php);
        let cond = self.srp_condition();
        if crate::pricing::is_graded(cond) {
            return format!("Graded: set your own price. Ungraded Near Mint is {nm}");
        }
        if crate::pricing::is_sealed(cond) {
            return format!("Sealed: set your own price. Near Mint single is {nm}");
        }
        match (self.srp(), crate::pricing::factor(cond), crate::pricing::read_condition(cond)) {
            (Some(srp), Some(share), Some(std)) => format!("SRP {} · {std} · {}% of Near Mint {nm}", peso(srp), (share * 100.0).round() as i64),
            _ => format!("SRP: condition not known. Near Mint is {nm}"),
        }
    }
    pub fn srcset(&self) -> String {
        [240u32, 360, 540, 800]
            .iter()
            .map(|w| format!("{} {}w", self.img(*w), w))
            .collect::<Vec<_>>()
            .join(", ")
    }
    /// "PSA 10" / "CGC 9.5" parsed from the grade field or the title; None for raw cards.
    pub fn grade(&self) -> Option<String> {
        for src in [&self.variant, &self.title] {
            let up = src.trim().to_uppercase();
            for co in ["PSA", "CGC", "BGS", "SGC", "ACE"] {
                if let Some(rest) = up.strip_prefix(co) {
                    let num: String = rest.trim_start().chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                    let num = num.trim_end_matches(".0").trim_end_matches('.');
                    if !num.is_empty() {
                        return Some(format!("{co} {num}"));
                    }
                }
            }
        }
        None
    }
    /// Plain-text notes: strips any HTML from imported descriptions so templates can always escape.
    pub fn notes(&self) -> String {
        let mut out = String::new();
        let mut in_tag = false;
        let mut tag = String::new();
        for ch in self.body.chars() {
            match ch {
                '<' => { in_tag = true; tag.clear(); }
                '>' if in_tag => {
                    in_tag = false;
                    let t = tag.to_lowercase();
                    if t.starts_with("br") || t.starts_with("/p") || t.starts_with("/li") || t.starts_with("/div") { out.push('\n'); }
                }
                _ if in_tag => tag.push(ch),
                _ => out.push(ch),
            }
        }
        out.replace("&amp;", "&").replace("&nbsp;", " ").replace("&#39;", "'").replace("&quot;", "\"")
            .replace("&lt;", "<").replace("&gt;", ">").trim().to_string()
    }
    /// The language this card's text is written in: "en", "ja" or "zh".
    pub fn lang(&self) -> &'static str {
        crate::translate::language_of(&self.title, &self.api_id, &self.tags)
    }
    /// The name without the set on the end. The set has its own line on every tile.
    pub fn short_title(&self) -> String {
        vault_search::title_and_set(&self.title, &self.card_set).0
    }
    pub fn set_text(&self) -> String {
        vault_search::title_and_set(&self.title, &self.card_set).1
    }
    /// The finish, unless it's a plain one that isn't worth repeating on every card.
    pub fn finish_shown(&self) -> String {
        let f = self.finish.trim();
        if f.is_empty() || f.eq_ignore_ascii_case("normal") || f == "market" { String::new() } else { crate::market::label(f) }
    }
    /// Choices for a condition dropdown as (value, words, selected). A condition outside the usual list stays available, so nothing is lost by opening it.
    pub fn condition_choices(&self) -> Vec<(String, String, bool)> {
        let now = self.variant.trim();
        let mut out = vec![];
        if now.is_empty() {
            out.push((String::new(), "Pick a condition".to_string(), true));
        } else if !CONDITIONS.contains(&now) {
            out.push((now.to_string(), crate::translate::variant(now, "en"), true));
        }
        out.extend(CONDITIONS.iter().map(|c| (c.to_string(), c.to_string(), *c == now)));
        out
    }
    /// "Near Mint · Holofoil". A plain finish isn't worth repeating on every card.
    pub fn condition_text(&self) -> String {
        let cond = crate::translate::variant(&self.variant, "en");
        let finish = self.finish_shown();
        match (cond.is_empty(), finish.is_empty()) {
            (false, false) => format!("{cond} · {finish}"),
            (false, true) => cond,
            _ => finish,
        }
    }
    /// Older imports keep condition, finish and rarity on one line. This is the part at `i`, when there is more than one.
    fn variant_part(&self, i: usize) -> String {
        let parts: Vec<&str> = self.variant.split('/').map(str::trim).collect();
        if parts.len() < 2 && i > 0 { return String::new(); }
        parts.get(i).copied().unwrap_or_default().to_string()
    }
    /// The condition alone, for the Condition plate.
    pub fn plate_condition(&self) -> String {
        words_from_slug(&crate::translate::variant(&self.variant_part(0), "en"))
    }
    /// The finish alone, for the Finish plate.
    pub fn plate_finish(&self) -> String {
        let set = self.finish_text();
        if !set.is_empty() { return set; }
        match self.variant_part(1).to_lowercase().as_str() {
            "" => String::new(),
            "reverse-holofoil" | "reverse-holo" => crate::market::label("reverseHolofoil"),
            f => words_from_slug(&crate::market::label(f)),
        }
    }
    /// The rarity alone, for the Rarity plate.
    pub fn plate_rarity(&self) -> String {
        let set = self.rarity_text();
        if !set.is_empty() { set } else { words_from_slug(&self.variant_part(2)) }
    }
    /// The notes as shown on a card. Imported cards carry a stock block repeating the facts on the plates, so that block isn't shown again. Words the owner wrote always are.
    pub fn notes_shown(&self) -> String {
        let all = self.notes();
        let stock = all.lines().any(|l| l.trim().starts_with("Pick your condition from the options") || (l.contains(" from the Pokémon TCG ") && l.contains("(card ")));
        let repeats = |l: &str| {
            let t = l.trim();
            t.starts_with("Pick your condition from the options") || (t.contains(" from the Pokémon TCG ") && t.contains("(card "))
                || ["Set:", "Card number:", "Rarity:", "Finish:", "Language:"].iter().any(|k| t.starts_with(k))
        };
        all.lines().map(str::trim).filter(|l| !l.is_empty() && !(stock && repeats(l))).collect::<Vec<_>>().join("\n")
    }
    /// The owner's own tags, the ones worth showing on the card.
    pub fn shown_tags(&self) -> Vec<String> {
        self.tag_list().into_iter().filter(|t| tag_for_people(t)).collect()
    }
    /// The notes on one line for a tile. Empty when there are none.
    pub fn note_line(&self) -> String {
        let all = self.notes_shown();
        let one = all.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" · ");
        let mut out: String = one.chars().take(160).collect();
        if one.chars().count() > 160 {
            out.push('…');
        }
        out
    }
    /// The pill on a tile.
    pub fn tile_cond(&self) -> String {
        vault_search::tile_condition(&self.condition_text())
    }
    pub fn cond_class(&self) -> &'static str {
        vault_search::cond_class(&self.condition_text())
    }
    pub fn foil_class(&self) -> &'static str {
        vault_search::foil_class(&format!("{} {} {}", self.title, self.condition_text(), self.rarity))
    }
    /// The set's symbol.
    pub fn symbol(&self) -> String {
        vault_search::set_symbol(&self.api_id)
    }
    /// The finish in words, or nothing for cards that don't have one recorded.
    pub fn finish_text(&self) -> String {
        match self.finish.trim() {
            "" | "market" => String::new(),
            f => crate::market::label(f),
        }
    }
    /// The rarity in words. "none" marks cards the catalogue had nothing for.
    pub fn rarity_text(&self) -> String {
        let r = self.rarity.trim();
        if r.is_empty() || r.eq_ignore_ascii_case("none") { String::new() } else { r.to_string() }
    }
    /// The price as a person would type it: "450", not "450.0".
    pub fn price_input(&self) -> String {
        if self.price.fract() == 0.0 { format!("{}", self.price as i64) } else { format!("{:.2}", self.price) }
    }
    pub fn tag_list(&self) -> Vec<String> {
        self.tags
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    }
}

impl Collection {
    pub fn count_text(&self) -> String {
        thousands(self.count)
    }
    /// The colour of this binder's leather (0 to 5): the same on the home page and when it's open.
    pub fn tone(&self) -> u8 {
        tone(&self.handle)
    }
    pub fn img(&self, w: u32) -> String {
        cdn(&self.image, w)
    }
    pub fn has_image(&self) -> bool {
        !self.image.is_empty()
    }
}

pub fn cdn(src: &str, w: u32) -> String {
    if src.is_empty() {
        return String::new();
    }
    // Locally uploaded photos are stored pre-resized as `_l.jpg` (large) and `_s.jpg` (thumb).
    if src.starts_with("/uploads/") {
        return if w <= 540 { src.replace("_l.jpg", "_s.jpg") } else { src.to_string() };
    }
    let sep = if src.contains('?') { '&' } else { '?' };
    format!("{src}{sep}width={w}")
}

pub fn thousands(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

pub use vault_search::peso;

/// `set_code(api_id)` in SQL, the same as `set_code` here, so sorting by set can use it.
pub fn register_set_code(c: &Connection) -> rusqlite::Result<()> {
    c.create_scalar_function("set_code", 1, rusqlite::functions::FunctionFlags::SQLITE_UTF8 | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC, |ctx| {
        let s: String = ctx.get::<String>(0).unwrap_or_default();
        Ok(set_code(&s))
    })
}

pub fn open(path: &str) -> rusqlite::Result<Connection> {
    let c = Connection::open(path)?;
    c.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA mmap_size=134217728;
         PRAGMA cache_size=-16384; PRAGMA temp_store=MEMORY; PRAGMA busy_timeout=5000;",
    )?;
    register_set_code(&c)?;
    c.execute_batch(SCHEMA)?;
    c.execute_batch(crate::blocks::SCHEMA)?;
    c.execute_batch(crate::suggest::SCHEMA)?;
    c.execute_batch(crate::members::SCHEMA)?;
    crate::members::migrate(&c)?;
    migrate(&c)?;
    Ok(c)
}

/// Add columns introduced after the first release (safe to run on every open).
fn migrate(c: &Connection) -> rusqlite::Result<()> {
    let mut st = c.prepare("PRAGMA table_info(products)")?;
    let have: Vec<String> = st.query_map([], |r| r.get::<_, String>(1))?.collect::<Result<_, _>>()?;
    for col in ["card_no", "card_set", "api_id", "finish", "price_updated", "rarity", "solo", "price_src"] {
        if !have.iter().any(|h| h == col) {
            c.execute(&format!("ALTER TABLE products ADD COLUMN {col} TEXT NOT NULL DEFAULT ''"), [])?;
        }
    }
    for (col, def) in [("market_php", "REAL NOT NULL DEFAULT 0"), ("market_at", "INTEGER NOT NULL DEFAULT 0")] {
        if !have.iter().any(|h| h == col) {
            c.execute(&format!("ALTER TABLE products ADD COLUMN {col} {def}"), [])?;
        }
    }
    if !have.iter().any(|h| h == "hits") {
        c.execute("ALTER TABLE products ADD COLUMN hits INTEGER NOT NULL DEFAULT 0", [])?;
    }
    // The owner's own order of the cards. It starts as newest first, so nothing moves until they drag something.
    if !have.iter().any(|h| h == "position") {
        c.execute("ALTER TABLE products ADD COLUMN position REAL NOT NULL DEFAULT 0", [])?;
        c.execute(
            "UPDATE products SET position=(SELECT COUNT(*) FROM products q WHERE q.created_at>products.created_at OR (q.created_at=products.created_at AND q.id>products.id))",
            [],
        )?;
    }
    // A new card goes to the top.
    c.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_products_pos ON products(position);
         CREATE TRIGGER IF NOT EXISTS products_new_first AFTER INSERT ON products BEGIN
           UPDATE products SET position=(SELECT COALESCE(MIN(position),0) FROM products WHERE id<>NEW.id)-1 WHERE id=NEW.id;
         END;",
    )?;
    let mut st = c.prepare("PRAGMA table_info(collections)")?;
    let cols: Vec<String> = st.query_map([], |r| r.get::<_, String>(1))?.collect::<Result<_, _>>()?;
    if !cols.is_empty() && !cols.iter().any(|h| h == "on_home") {
        c.execute("ALTER TABLE collections ADD COLUMN on_home INTEGER NOT NULL DEFAULT 1", [])?;
        // Matches what the home page showed before this was editable: everything except graded slabs and figures.
        c.execute("UPDATE collections SET on_home=0 WHERE handle IN ('graded-cards','pokemon-figures')", [])?;
    }
    if !cols.is_empty() && !cols.iter().any(|h| h == "in_menu") {
        c.execute("ALTER TABLE collections ADD COLUMN in_menu INTEGER NOT NULL DEFAULT 1", [])?;
        c.execute("ALTER TABLE collections ADD COLUMN menu_pos INTEGER NOT NULL DEFAULT 0", [])?;
        c.execute("UPDATE collections SET menu_pos=position", [])?;
    }
    Ok(())
}

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS products(
  id INTEGER PRIMARY KEY, handle TEXT UNIQUE NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL DEFAULT '',
  tags TEXT NOT NULL DEFAULT '', variant TEXT NOT NULL DEFAULT '', price REAL NOT NULL DEFAULT 0,
  compare_at REAL, available INTEGER NOT NULL DEFAULT 1, image TEXT NOT NULL DEFAULT '',
  image_w INTEGER NOT NULL DEFAULT 0, image_h INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS collections(
  handle TEXT PRIMARY KEY, title TEXT NOT NULL, image TEXT NOT NULL DEFAULT '', position INTEGER NOT NULL DEFAULT 0,
  on_home INTEGER NOT NULL DEFAULT 1, in_menu INTEGER NOT NULL DEFAULT 1, menu_pos INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS collection_products(
  collection TEXT NOT NULL, product_id INTEGER NOT NULL, position INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(collection, product_id));
CREATE INDEX IF NOT EXISTS idx_cp_product ON collection_products(product_id);
CREATE INDEX IF NOT EXISTS idx_products_created ON products(created_at DESC);
CREATE TABLE IF NOT EXISTS card_sets(code TEXT PRIMARY KEY, name TEXT NOT NULL DEFAULT '', series TEXT NOT NULL DEFAULT '', released TEXT NOT NULL DEFAULT '', fetched INTEGER NOT NULL DEFAULT 0);
";

/// How many columns `COLS` has. Anything selected after `COLS` starts at this index.
/// Keep it in step with `COLS` and `map` (a test checks it).
const NCOLS: usize = 20;

const COLS: &str =
    "p.id,p.handle,p.title,p.body,p.tags,p.variant,p.price,p.compare_at,p.available,p.image,p.image_w,p.image_h,p.card_no,p.card_set,p.api_id,p.finish,p.price_updated,p.price_src,p.rarity,p.market_php";

fn map(r: &Row) -> rusqlite::Result<Product> {
    Ok(Product {
        id: r.get(0)?,
        handle: r.get(1)?,
        title: r.get(2)?,
        body: r.get(3)?,
        tags: r.get(4)?,
        variant: r.get(5)?,
        price: r.get(6)?,
        compare_at: r.get(7)?,
        available: r.get::<_, i64>(8)? != 0,
        image: r.get(9)?,
        image_w: r.get(10)?,
        image_h: r.get(11)?,
        card_no: r.get(12)?,
        card_set: r.get(13)?,
        api_id: r.get(14)?,
        finish: r.get(15)?,
        price_updated: r.get(16)?,
        price_src: r.get(17)?,
        rarity: r.get(18)?,
        market_php: r.get(19)?,
        qty: 1,
        copy_ids: String::new(),
    })
}

/// A row from a grouped list: the product plus how many copies it stands for.
fn map_grouped(r: &Row) -> rusqlite::Result<Product> {
    let mut p = map(r)?;
    p.qty = r.get(NCOLS)?;
    p.copy_ids = r.get(NCOLS + 1)?;
    Ok(p)
}

/// What makes two entries "the same card": the same catalogue card and name, condition or grade, finish and price.
/// Change any of those on a copy and it becomes its own card. `solo` marks a copy given its own notes, photo or binders, which also makes it a different card.
/// A card's set release date ("2025-11-14"), by the set code in its catalogue id ("tcgdex:sv08-053" → "sv08"), else by the set's name. "" when unknown.
const SET_DATE: &str = "COALESCE((SELECT s.released FROM card_sets s WHERE s.code=set_code(p.api_id)), (SELECT s.released FROM card_sets s WHERE s.name=p.card_set COLLATE NOCASE LIMIT 1), '')";

/// "tcgdex:sv08-053" → "sv08". Anything else → "".
pub fn set_code(api_id: &str) -> String {
    let Some(rest) = api_id.strip_prefix("tcgdex:") else { return String::new() };
    rest.rsplit_once('-').map(|(code, _)| code.to_string()).unwrap_or_default()
}

/// A set we've looked up: its code, name, series and release date.
pub fn save_set(c: &Connection, code: &str, name: &str, series: &str, released: &str) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO card_sets(code, name, series, released, fetched) VALUES(?1, ?2, ?3, ?4, strftime('%s','now'))
         ON CONFLICT(code) DO UPDATE SET name=?2, series=?3, released=CASE WHEN ?4<>'' THEN ?4 ELSE released END, fetched=strftime('%s','now')",
        rusqlite::params![code, name, series, released],
    )?;
    Ok(())
}

/// Set codes the vault's cards use that we haven't looked up yet (or looked up without finding a date, over a week ago).
pub fn sets_to_fetch(c: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare(
        "SELECT DISTINCT set_code(p.api_id) AS code FROM products p WHERE p.api_id LIKE 'tcgdex:%'
         AND NOT EXISTS (SELECT 1 FROM card_sets s WHERE s.code=set_code(p.api_id) AND (s.released<>'' OR s.fetched>strftime('%s','now')-7*86400))
         ORDER BY code LIMIT 200",
    )?;
    let v = st.query_map([], |r| r.get(0))?.filter_map(Result::ok).filter(|s: &String| !s.is_empty()).collect();
    Ok(v)
}

/// Set names (cards saved without a catalogue id) we have no date for.
pub fn set_names_to_fetch(c: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare(
        "SELECT DISTINCT trim(p.card_set) FROM products p WHERE p.card_set<>'' AND (p.api_id NOT LIKE 'tcgdex:%')
         AND NOT EXISTS (SELECT 1 FROM card_sets s WHERE s.name=trim(p.card_set) COLLATE NOCASE) LIMIT 100",
    )?;
    let v = st.query_map([], |r| r.get(0))?.filter_map(Result::ok).collect();
    Ok(v)
}

const DUP_KEY: &str = "(p.api_id || '|' || lower(trim(p.title)) || '|' || lower(trim(p.variant)) || '|' || lower(trim(p.finish)) || '|' || printf('%.2f', p.price) || '|' || p.solo)";

/// "holo-rare" reads as "Holo Rare" and "common" as "Common". Anything already capitalised stays as it is.
fn words_from_slug(t: &str) -> String {
    if t.chars().all(|c| !c.is_uppercase()) {
        t.split(['-', ' ']).filter(|w| !w.is_empty()).map(|w| { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default() }).collect::<Vec<_>>().join(" ")
    } else {
        t.to_string()
    }
}

/// How many cards a binder holds, copies included. `None` is the whole vault. Every number on the site about a binder's size comes from here.
pub fn cards_in(c: &Connection, collection: Option<&str>) -> rusqlite::Result<i64> {
    match collection {
        Some(h) => c.query_row("SELECT COUNT(*) FROM collection_products cp JOIN products p ON p.id=cp.product_id WHERE cp.collection=?1", [h], |r| r.get(0)),
        None => c.query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0)),
    }
}

/// Catalogue cards that never got a picture.
pub fn cards_without_picture(c: &Connection) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut st = c.prepare("SELECT id, api_id FROM products WHERE image='' AND api_id<>''")?;
    let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

pub fn set_picture(c: &Connection, id: i64, image: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE products SET image=?2 WHERE id=?1 AND image=''", rusqlite::params![id, image])?;
    Ok(())
}

/// Tags the site adds for its own search (binder-favorites, finish-normal and so on) aren't worth showing to people.
pub fn tag_for_people(t: &str) -> bool {
    let t = t.trim().to_lowercase();
    !["binder-", "finish-", "rarity-", "set-"].iter().any(|p| t.starts_with(p))
}

/// Copies saved together with one new photo all point at the same stored file.
pub fn share_image(c: &Connection, id: i64, from: i64) -> rusqlite::Result<usize> {
    c.execute("UPDATE products SET image=(SELECT image FROM products WHERE id=?2) WHERE id=?1", [id, from])
}

/// How many copies of this card (same card, same condition) the collection holds.
pub fn copies(c: &Connection, p: &Product) -> rusqlite::Result<i64> {
    c.query_row(
        &format!("SELECT COUNT(*) FROM products p WHERE {DUP_KEY} = (SELECT {DUP_KEY} FROM products p WHERE p.id=?1)"),
        [p.id],
        |r| r.get(0),
    )
}

/// One version of a card: its condition and finish, and how many of them the vault holds.
#[derive(Clone, Debug)]
pub struct Version {
    pub handle: String,
    pub condition: String,
    pub finish: String,
    pub rarity: String,
    pub price: String,
    pub qty: i64,
    pub current: bool,
}

/// Every version of this card in the vault, best condition first. A page only lists them when there's more than one.
pub fn versions(c: &Connection, p: &Product) -> rusqlite::Result<Vec<Version>> {
    let (sql, key) = if p.api_id.is_empty() {
        (format!("SELECT {COLS}, {DUP_KEY} FROM products p WHERE p.api_id='' AND lower(trim(p.title))=lower(trim(?1)) ORDER BY p.id"), p.title.clone())
    } else {
        (format!("SELECT {COLS}, {DUP_KEY} FROM products p WHERE p.api_id=?1 ORDER BY p.id"), p.api_id.clone())
    };
    let mut st = c.prepare(&sql)?;
    let rows = st.query_map([key], |r| Ok((map(r)?, r.get::<_, String>(NCOLS)?)))?.collect::<Result<Vec<_>, _>>()?;
    let mut out: Vec<(String, Version)> = vec![];
    for (r, key) in rows {
        let current = r.id == p.id;
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => {
                v.qty += 1;
                v.current |= current;
            }
            None => out.push((key, Version {
                condition: crate::translate::variant(&r.variant, "en"),
                finish: r.finish_text(),
                rarity: r.rarity_text(),
                price: r.price_text(),
                handle: r.handle.clone(),
                qty: 1,
                current,
            })),
        }
    }
    const ORDER: [&str; 5] = ["near mint", "lightly played", "moderately played", "heavily played", "damaged"];
    let rank = |v: &Version| ORDER.iter().position(|o| v.condition.to_lowercase().starts_with(o)).unwrap_or(ORDER.len());
    let mut list: Vec<Version> = out.into_iter().map(|x| x.1).collect();
    list.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.finish.cmp(&b.finish)));
    Ok(list)
}

/// Catalogue cards that don't know their rarity yet.
pub fn missing_rarity(c: &Connection, limit: i64) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare("SELECT DISTINCT api_id FROM products WHERE api_id<>'' AND rarity='' LIMIT ?1")?;
    let v = st.query_map([limit], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
    Ok(v)
}

pub fn set_rarity(c: &Connection, api_id: &str, rarity: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE products SET rarity=?2 WHERE api_id=?1", [api_id, rarity])?;
    Ok(())
}

#[derive(Default, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Query {
    pub collection: Option<String>,
    pub q: String,
    pub sort: String,
    pub in_stock: bool,
    pub page: usize,
}

pub fn list(c: &Connection, q: &Query) -> rusqlite::Result<(Vec<Product>, i64)> {
    let mut wh: Vec<String> = vec!["1=1".into()];
    let mut args: Vec<Value> = vec![];
    let mut join = String::new();
    if let Some(col) = &q.collection {
        join = " JOIN collection_products cp ON cp.product_id=p.id".into();
        wh.push("cp.collection=?".into());
        args.push(Value::Text(col.clone()));
    }
    for term in q.q.split_whitespace().take(6) {
        wh.push("(p.title LIKE ? ESCAPE '\\' OR p.tags LIKE ? ESCAPE '\\')".into());
        let like = format!(
            "%{}%",
            term.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
        );
        args.push(Value::Text(like.clone()));
        args.push(Value::Text(like));
    }
    if q.in_stock {
        wh.push("p.available=1".into());
    }
    let order = match q.sort.as_str() {
        "price-asc" => "p.price ASC, p.id",
        "price-desc" => "p.price DESC, p.id",
        "title" => "p.title COLLATE NOCASE ASC",
        "newest" => "p.created_at DESC, p.id DESC",
        // By the set's release date (card_sets, filled from the card service). Sets with no known date go last.
        "set-old" => "(rel=''), rel ASC, card_set COLLATE NOCASE, card_no, id",
        "set-new" => "(rel=''), rel DESC, card_set COLLATE NOCASE, card_no, id",
        // The owner's order. Copies of a card sit where the first of them does.
        _ => "gpos ASC, p.created_at DESC, p.id DESC",
    };
    let whs = wh.join(" AND ");
    // Copies of the same card are one entry with a count ("×2"), everywhere a list is shown.
    let matched = format!("SELECT DISTINCT {COLS}, {DUP_KEY} AS gk, p.created_at AS created, p.position AS pos, {SET_DATE} AS rel FROM products p{join} WHERE {whs}");
    let total: i64 = c.query_row(&format!("SELECT COUNT(DISTINCT gk) FROM ({matched})"), params_from_iter(args.iter()), |r| r.get(0))?;
    let order = order.replace("p.created_at", "created").replace("p.", "");
    let mut a2 = args.clone();
    a2.push(Value::Integer(PAGE_SIZE as i64));
    a2.push(Value::Integer((q.page * PAGE_SIZE) as i64));
    let cols = COLS.replace("p.", "");
    let sql = format!(
        "WITH m AS ({matched}),
              g AS (SELECT *, COUNT(*) OVER (PARTITION BY gk) AS qty, group_concat(id) OVER (PARTITION BY gk) AS ids, MIN(pos) OVER (PARTITION BY gk) AS gpos,
                           ROW_NUMBER() OVER (PARTITION BY gk ORDER BY created DESC, id DESC) AS rn FROM m)
         SELECT {cols}, qty, ids FROM g WHERE rn=1 ORDER BY {order} LIMIT ? OFFSET ?"
    );
    let mut st = c.prepare(&sql)?;
    let rows = st
        .query_map(params_from_iter(a2.iter()), map_grouped)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok((rows, total))
}

/// Every card as one catalogue file for the card finder and the AI tools. Copies of a card are one entry with a count.
/// The vault's most valuable cards with pictures, one of each, for the home page's opening scene. Real catalogue cards come first, so it's cards in orbit and not boxes.
pub fn orbit_cards(c: &Connection, limit: i64) -> rusqlite::Result<Vec<Product>> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM products p WHERE p.image<>'' AND p.image NOT LIKE '%placeholder%' GROUP BY lower(trim(p.title)) ORDER BY (p.api_id<>'') DESC, p.price DESC, p.id DESC LIMIT ?1"))?;
    let v = st.query_map([limit], map)?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

pub fn catalogue(c: &Connection) -> rusqlite::Result<String> {
    use std::collections::HashMap;
    let binders = collections(c)?;
    let index: HashMap<&str, u16> = binders.iter().enumerate().map(|(i, b)| (b.handle.as_str(), i as u16)).collect();
    let mut member: HashMap<i64, Vec<u16>> = HashMap::new();
    let mut st = c.prepare("SELECT product_id, collection FROM collection_products")?;
    for row in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
        let (id, col) = row?;
        if let Some(i) = index.get(col.as_str()) {
            member.entry(id).or_default().push(*i);
        }
    }
    let mut out = String::new();
    for b in &binders {
        out += &vault_search::binder_line(&vault_search::Binder { handle: b.handle.clone(), title: b.title.clone(), img: b.img(360), count: b.count.max(0) as u32 });
    }
    let mut st = c.prepare(&format!("SELECT {COLS}, {DUP_KEY} AS gk FROM products p ORDER BY p.position ASC, p.created_at DESC, p.id DESC"))?;
    let rows = st.query_map([], |r| Ok((map(r)?, r.get::<_, String>(NCOLS)?)))?;
    let (mut order, mut seen): (Vec<(Product, Vec<u16>, u32)>, HashMap<String, usize>) = (vec![], HashMap::new());
    for row in rows {
        let (p, key) = row?;
        let mine = member.remove(&p.id).unwrap_or_default();
        match seen.get(&key) {
            Some(&at) => {
                order[at].2 += 1;
                order[at].1.extend(mine); // once per copy, so each binder can count its own
            }
            None => {
                seen.insert(key, order.len());
                order.push((p, mine, 1));
            }
        }
    }
    for (p, bs, qty) in order {
        let (variant, set, notes) = (p.condition_text(), p.set_text(), p.note_line());
        out += &vault_search::card_line(&vault_search::Card {
            sym: p.symbol(),
            finish: vault_search::finish_group(&p.plate_finish(), &p.plate_rarity()).to_string(),
            lang: p.lang().to_string(),
            grade: p.grade().unwrap_or_default(),
            img: p.img(360),
            handle: p.handle,
            title: p.title,
            variant,
            set,
            no: p.card_no,
            price: p.price,
            qty,
            binders: bs,
            notes,
            tags: p.tags,
            text: String::new(),
            order: 0,
        });
    }
    Ok(out)
}

pub fn product(c: &Connection, handle: &str) -> rusqlite::Result<Option<Product>> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM products p WHERE p.handle=?"))?;
    let mut it = st.query_map([handle], map)?;
    it.next().transpose()
}

pub fn collections(c: &Connection) -> rusqlite::Result<Vec<Collection>> {
    let mut st = c.prepare(
        "SELECT c.handle,c.title,
           COALESCE(NULLIF(c.image,''),(SELECT p.image FROM collection_products cp JOIN products p ON p.id=cp.product_id
             WHERE cp.collection=c.handle AND p.image<>'' ORDER BY p.price DESC LIMIT 1),''),
           (SELECT COUNT(*) FROM collection_products cp JOIN products p ON p.id=cp.product_id WHERE cp.collection=c.handle), c.on_home, c.image<>'', c.in_menu, c.menu_pos
         FROM collections c ORDER BY c.position",
    )?;
    let v = st
        .query_map([], |r| {
            Ok(Collection { handle: r.get(0)?, title: r.get(1)?, image: r.get(2)?, count: r.get(3)?, on_home: r.get(4)?, custom_cover: r.get(5)?, in_menu: r.get(6)?, menu_pos: r.get(7)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

pub fn total(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0))
}

pub fn related(c: &Connection, p: &Product, n: usize) -> rusqlite::Result<Vec<Product>> {
    // Every card once (copies grouped), scored against this one by crate::similar: names alike, same set, prices alike, same condition.
    let mut st = c.prepare_cached(&format!("SELECT {COLS}, COUNT(*) AS qty, group_concat(p.id) AS ids FROM products p GROUP BY {DUP_KEY} ORDER BY MIN(p.position), MIN(p.id)"))?;
    let all = st.query_map([], map_grouped)?.collect::<Result<Vec<_>, _>>()?;
    let me = p.id.to_string();
    let Some(q) = all.iter().position(|x| x.copy_ids.split(',').any(|i| i == me)) else { return Ok(vec![]) };
    let items: Vec<crate::similar::Item> = all
        .iter()
        .map(|x| crate::similar::Item { name: x.short_title(), set: x.set_text(), price: x.price, cond: x.cond_class(), has_image: x.has_image() })
        .collect();
    // The same card in another condition is already listed under "Other copies", so it isn't repeated here.
    let key = |x: &Product| (x.short_title().to_lowercase(), x.set_text().to_lowercase());
    let mine = key(p);
    let picked = crate::similar::pick(&items, q, n, |i| key(&all[i]) == mine);
    Ok(picked.into_iter().map(|i| all[i].clone()).collect())
}

/// The address search engines should use for this card: copies of the same card share the first copy's page.
pub fn canonical_handle(c: &Connection, p: &Product) -> rusqlite::Result<String> {
    let sql = format!("SELECT p.handle FROM products p WHERE {DUP_KEY} = (SELECT {DUP_KEY} FROM products p WHERE p.id=?1) ORDER BY p.id LIMIT 1");
    Ok(c.query_row(&sql, [p.id], |r| r.get(0)).optional()?.unwrap_or_else(|| p.handle.clone()))
}

/// One entry per card for the sitemap (copies share the first copy's page): handle, title, when added, photo.
pub fn sitemap_cards(c: &Connection) -> rusqlite::Result<Vec<(String, String, String, String)>> {
    let sql = format!("SELECT p.handle, p.title, p.created_at, p.image FROM products p WHERE p.id IN (SELECT MIN(p.id) FROM products p GROUP BY {DUP_KEY}) ORDER BY p.position, p.id LIMIT 45000");
    let mut st = c.prepare(&sql)?;
    let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

/// Where this price sits in the vault: the share of cards that cost less, 0 to 100.
pub fn standing(c: &Connection, price: f64) -> rusqlite::Result<u8> {
    let mut st = c.prepare_cached("SELECT price FROM products")?;
    let prices = st.query_map([], |r| r.get::<_, f64>(0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(crate::similar::standing(&prices, price))
}

/// The starting binders, created once on a new database (owner can rename, delete and add after that). (handle, title, legacy source collection, title heuristic for demo data)
pub const BINDERS: [(&str, &str, &str, &str); 8] = [
    ("trainers", "Trainers", "", "Boss|Professor|Cynthia|Iono|Arven|Marnie|Lillie|Nemona|Penny|Giovanni|Lysandre|Hop |Gloria"),
    ("favorites", "Favorites", "favorites-binder", ""),
    ("pikachu-set", "Pikachu Set", "", "Pikachu|Raichu|Pichu"),
    ("playables", "Playables", "battle-cards", ""),
    ("legendary-collections", "Legendary Collections", "legendary-collections", ""),
    ("japanese-collections", "Japanese Collections", "", "(JP)|JP)"),
    ("graded-cards", "Graded Cards", "graded-cards", ""),
    ("pokemon-figures", "Pokémon Figures", "pokemon-figures", ""),
];

/// Create the eight binders if missing. On a database seeded from the public catalog, new binders are
/// pre-filled from matching legacy collections so the site isn't empty; the old collections are then dropped.
/// Once, when the menu became editable: binders with no cards stay out of it, as before. After that it's the owner's call.
pub fn menu_hide_empty_once(c: &Connection) -> rusqlite::Result<()> {
    let done: Option<String> = c.query_row("SELECT value FROM settings WHERE key='menu_v1'", [], |r| r.get(0)).optional()?;
    if done.is_some() {
        return Ok(());
    }
    c.execute("UPDATE collections SET in_menu=0 WHERE NOT EXISTS (SELECT 1 FROM collection_products cp WHERE cp.collection=collections.handle)", [])?;
    c.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('menu_v1','1')", [])?;
    Ok(())
}

pub fn ensure_binders(c: &Connection) -> rusqlite::Result<()> {
    // Seeded once. After that the owner's list (Admin › Binders) is the only source: added binders stay,
    // deleted ones don't come back.
    let seeded: Option<String> = c.query_row("SELECT value FROM settings WHERE key='binders_seeded'", [], |r| r.get(0)).optional()?;
    if seeded.is_some() {
        return Ok(());
    }
    seed_binders(c)?;
    c.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('binders_seeded','1')", [])?;
    Ok(())
}

fn seed_binders(c: &Connection) -> rusqlite::Result<()> {
    for (i, (h, title, legacy, like)) in BINDERS.iter().enumerate() {
        let created = c.execute(
            "INSERT OR IGNORE INTO collections(handle,title,image,position,on_home) VALUES(?1,?2,'',?3,?4)",
            rusqlite::params![h, title, i as i64, !matches!(*h, "graded-cards" | "pokemon-figures")],
        )? > 0;
        if !created {
            continue;
        }
        if !legacy.is_empty() {
            c.execute(
                "INSERT OR IGNORE INTO collection_products(collection,product_id,position)
                 SELECT ?1,product_id,position FROM collection_products WHERE collection=?2",
                rusqlite::params![h, legacy],
            )?;
        }
        for pat in like.split('|').filter(|p| !p.is_empty()) {
            c.execute(
                "INSERT OR IGNORE INTO collection_products(collection,product_id,position)
                 SELECT ?1,id,0 FROM products WHERE title LIKE ?2",
                rusqlite::params![h, format!("%{pat}%")],
            )?;
        }
    }
    let keep = BINDERS.map(|b| format!("'{}'", b.0)).join(",");
    c.execute(&format!("DELETE FROM collections WHERE handle NOT IN ({keep})"), [])?;
    c.execute(&format!("DELETE FROM collection_products WHERE collection NOT IN ({keep})"), [])?;
    Ok(())
}

/// Rename a binder (the link in the menu, the home page tile and the admin pickers all follow).
/// A safe binder handle: lowercase letters, digits and dashes.
pub fn handle_ok(h: &str) -> bool {
    !h.is_empty() && h.len() <= 60 && h.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn binder_exists(c: &Connection, handle: &str) -> bool {
    handle_ok(handle) && c.query_row("SELECT 1 FROM collections WHERE handle=?1", [handle], |_| Ok(())).is_ok()
}

/// New binder at the end of the list, shown on the home page. Returns its handle (made from the name, kept unique).
pub fn create_binder(c: &Connection, title: &str) -> rusqlite::Result<String> {
    let mut base = slug(title);
    if base.is_empty() {
        base = "binder".into();
    }
    let base: String = base.chars().take(48).collect::<String>().trim_matches('-').to_string();
    let mut handle = base.clone();
    let mut n = 2;
    while binder_exists(c, &handle) || handle == "all" {
        handle = format!("{base}-{n}");
        n += 1;
    }
    c.execute(
        "INSERT INTO collections(handle,title,image,position,on_home,in_menu,menu_pos) VALUES(?1,?2,'',(SELECT COALESCE(MAX(position),-1)+1 FROM collections),1,1,(SELECT COALESCE(MAX(menu_pos),-1)+1 FROM collections))",
        rusqlite::params![handle, title],
    )?;
    Ok(handle)
}

/// Delete a binder. Its cards stay in the collection; they just leave this binder. Returns the old cover path.
pub fn delete_binder(c: &Connection, handle: &str) -> rusqlite::Result<String> {
    let old: String = c.query_row("SELECT image FROM collections WHERE handle=?1", [handle], |r| r.get(0)).unwrap_or_default();
    c.execute("DELETE FROM collection_products WHERE collection=?1", [handle])?;
    c.execute("DELETE FROM collections WHERE handle=?1", [handle])?;
    Ok(old)
}

pub fn rename_binder(c: &Connection, handle: &str, title: &str) -> rusqlite::Result<usize> {
    c.execute("UPDATE collections SET title=?2 WHERE handle=?1", rusqlite::params![handle, title])
}

/// The top menu: exactly these binders, in this order. Every other binder leaves the menu (it still exists).
pub fn set_menu(c: &Connection, handles: &[String]) -> rusqlite::Result<()> {
    c.execute("UPDATE collections SET in_menu=0", [])?;
    for (i, h) in handles.iter().enumerate() {
        c.execute("UPDATE collections SET in_menu=1, menu_pos=?2 WHERE handle=?1", rusqlite::params![h, i as i64])?;
    }
    Ok(())
}

pub fn set_binder_home(c: &Connection, handle: &str, on: bool) -> rusqlite::Result<usize> {
    c.execute("UPDATE collections SET on_home=?2 WHERE handle=?1", rusqlite::params![handle, on])
}

/// Cover image path; empty = automatic (the binder's most valuable card). Returns the previous cover.
pub fn set_binder_cover(c: &Connection, handle: &str, image: &str) -> rusqlite::Result<String> {
    let old: String = c.query_row("SELECT image FROM collections WHERE handle=?1", [handle], |r| r.get(0)).unwrap_or_default();
    c.execute("UPDATE collections SET image=?2 WHERE handle=?1", rusqlite::params![handle, image])?;
    Ok(old)
}

/// Move a binder one place up (-1) or down (+1). Positions are renumbered 0..n so they stay tidy.
pub fn move_binder(c: &Connection, handle: &str, dir: i64) -> rusqlite::Result<()> {
    let mut st = c.prepare("SELECT handle FROM collections ORDER BY position, rowid")?;
    let mut order: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    if let Some(i) = order.iter().position(|h| h == handle) {
        let j = i as i64 + dir;
        if j >= 0 && (j as usize) < order.len() {
            order.swap(i, j as usize);
        }
    }
    for (i, h) in order.iter().enumerate() {
        c.execute("UPDATE collections SET position=?2 WHERE handle=?1", rusqlite::params![h, i as i64])?;
    }
    Ok(())
}

/// Put binders in the given order (drag and drop). Binders not listed keep their relative order at the end.
pub fn reorder_binders(c: &Connection, order: &[String]) -> rusqlite::Result<()> {
    let mut st = c.prepare("SELECT handle FROM collections ORDER BY position, rowid")?;
    let current: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let mut hs: Vec<String> = vec![];
    for h in order.iter().chain(current.iter()) {
        if current.contains(h) && !hs.contains(h) {
            hs.push(h.clone());
        }
    }
    for (i, h) in hs.iter().enumerate() {
        c.execute("UPDATE collections SET position=?2 WHERE handle=?1", rusqlite::params![h, i as i64])?;
    }
    Ok(())
}

/// Cards in a binder that have a photo, most valuable first: the choices for its cover.
pub fn binder_cover_choices(c: &Connection, handle: &str, limit: i64) -> rusqlite::Result<Vec<(i64, String, String)>> {
    let mut st = c.prepare(
        "SELECT p.id, p.title, p.image FROM collection_products cp JOIN products p ON p.id=cp.product_id
         WHERE cp.collection=?1 AND p.image<>'' ORDER BY p.price DESC, p.id DESC LIMIT ?2",
    )?;
    let v = st.query_map(rusqlite::params![handle, limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// One more copy of a card: same details, photo and binders; a new id and link. Returns the new id.
pub fn duplicate(c: &Connection, id: i64) -> rusqlite::Result<Option<i64>> {
    c.execute_batch("BEGIN IMMEDIATE")?;
    let r = (|| {
        let Some(p) = product_by_id(c, id)? else { return Ok(None) };
        let nid: i64 = c.query_row("SELECT MAX(COALESCE(MAX(id),0),1000000)+1 FROM products", [], |r| r.get(0))?;
        c.execute(
            "INSERT INTO products(id,handle,title,body,tags,variant,price,compare_at,available,image,image_w,image_h,created_at,card_no,card_set,api_id,finish,price_updated,price_src,rarity,solo,market_php,market_at)
             SELECT ?1, ?2, title,body,tags,variant,price,compare_at,available,image,image_w,image_h,strftime('%Y-%m-%dT%H:%M:%S','now'),card_no,card_set,api_id,finish,price_updated,price_src,rarity,solo,market_php,market_at FROM products WHERE id=?3",
            rusqlite::params![nid, format!("{}-{nid}", slug(&p.title)), id],
        )?;
        // A copy stays beside its card in the owner's order.
        c.execute("UPDATE products SET position=(SELECT position FROM products WHERE id=?2) WHERE id=?1", [nid, id])?;
        c.execute("INSERT OR IGNORE INTO collection_products(collection,product_id,position) SELECT collection,?1,position FROM collection_products WHERE product_id=?2", rusqlite::params![nid, id])?;
        Ok(Some(nid))
    })();
    match r {
        Ok(v) => { c.execute_batch("COMMIT")?; Ok(v) }
        Err(e) => { let _ = c.execute_batch("ROLLBACK"); Err(e) }
    }
}

/// Number of different cards (copies of the same card count once).
pub fn distinct_cards(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row(&format!("SELECT COUNT(DISTINCT {DUP_KEY}) FROM products p"), [], |r| r.get(0))
}

pub fn binders_of(c: &Connection, product_id: i64) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare("SELECT collection FROM collection_products WHERE product_id=?1")?;
    let v = st.query_map([product_id], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
    Ok(v)
}

pub struct Entry {
    pub id: Option<i64>,
    pub title: String,
    pub variant: String,
    pub price: f64,
    pub body: String,
    pub tags: String,
    pub image: Option<String>, // None = keep existing
    pub binders: Vec<String>,
    pub card_no: String,
    pub card_set: String,
    pub api_id: String,
    pub finish: String,
    pub price_updated: Option<String>, // None = keep existing
    pub price_src: String,
    pub rarity: String,                // empty = keep existing
    pub solo: bool,                    // edited on its own: stays a separate card from now on
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for ch in s.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(60).collect()
}

/// Insert or update a hand-entered card and its binder membership. Returns (id, handle).
pub fn save_entry(c: &Connection, e: &Entry) -> rusqlite::Result<(i64, String)> {
    // One write transaction: picking the next id and inserting must not interleave with another save,
    // or two new cards saved at the same moment get the same id and the second overwrites the first.
    c.execute_batch("BEGIN IMMEDIATE")?;
    match save_entry_tx(c, e) {
        Ok(r) => {
            c.execute_batch("COMMIT")?;
            Ok(r)
        }
        Err(err) => {
            let _ = c.execute_batch("ROLLBACK");
            Err(err)
        }
    }
}

fn save_entry_tx(c: &Connection, e: &Entry) -> rusqlite::Result<(i64, String)> {
    let id = match e.id {
        Some(id) => id,
        None => c.query_row("SELECT MAX(COALESCE(MAX(id),0),1000000)+1 FROM products", [], |r| r.get(0))?,
    };
    let handle = format!("{}-{id}", slug(&e.title));
    let now = c.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%S','now')", [], |r| r.get::<_, String>(0))?;
    c.execute(
        "INSERT INTO products(id,handle,title,body,tags,variant,price,available,image,created_at,card_no,card_set,api_id,finish,price_updated,price_src,rarity)
         VALUES(?1,?2,?3,?4,?5,?6,?7,1,COALESCE(?8,''),?9,?10,?11,?12,?13,COALESCE(?14,''),?15,?16)
         ON CONFLICT(id) DO UPDATE SET handle=?2,title=?3,body=?4,tags=?5,variant=?6,price=?7,
           image=COALESCE(?8,image),card_no=?10,card_set=?11,api_id=?12,finish=?13,
           price_updated=COALESCE(?14,price_updated),price_src=?15,rarity=CASE WHEN ?16<>'' THEN ?16 ELSE rarity END,
           solo=CASE WHEN ?17 THEN CAST(?1 AS TEXT) ELSE solo END",
        rusqlite::params![id, handle, e.title, e.body, e.tags, e.variant, e.price, e.image, now, e.card_no, e.card_set, e.api_id, e.finish, e.price_updated, e.price_src, e.rarity, e.solo],
    )?;
    c.execute("DELETE FROM collection_products WHERE product_id=?1", [id])?;
    for b in &e.binders {
        if binder_exists(c, b) {
            c.execute("INSERT OR IGNORE INTO collection_products(collection,product_id,position) VALUES(?1,?2,0)", rusqlite::params![b, id])?;
        }
    }
    Ok((id, handle))
}

/// Today's Near Mint market price for every copy of a catalogue card (the SRP is worked out from it).
pub fn set_market(c: &Connection, api_id: &str, php: f64, now: i64) -> rusqlite::Result<usize> {
    c.execute("UPDATE products SET market_php=?2, market_at=?3 WHERE api_id=?1", rusqlite::params![api_id, php, now])
}

/// Catalogue cards whose market price is the oldest (or never looked up): (api_id, finish), one per card.
pub fn stale_market(c: &Connection, limit: i64, older_than: i64) -> rusqlite::Result<Vec<(String, String)>> {
    let mut st = c.prepare("SELECT api_id, MIN(finish) FROM products WHERE api_id<>'' AND market_at<?2 GROUP BY api_id ORDER BY MIN(market_at), api_id LIMIT ?1")?;
    let v = st.query_map(rusqlite::params![limit, older_than], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// Quick edits from My cards: price, description, condition. They apply to every copy of the card, like the − and ＋ buttons. A price typed by hand isn't a market price any more.
pub fn quick_edit(c: &Connection, ids: &[i64], price: Option<f64>, price_src: Option<&str>, notes: Option<&str>, condition: Option<&str>, tags: Option<&[String]>) -> rusqlite::Result<()> {
    c.execute_batch("BEGIN IMMEDIATE")?;
    let r = (|| {
        for id in ids {
            if let Some(v) = price {
                let src = price_src.unwrap_or("");
                let now: String = if src == "suggested" || src == "market" { c.query_row("SELECT strftime('%Y-%m-%d','now')", [], |r| r.get(0)).unwrap_or_default() } else { String::new() };
                c.execute("UPDATE products SET price=?2, price_src=?3, price_updated=?4 WHERE id=?1", rusqlite::params![id, v, src, now])?;
            }
            if let Some(n) = notes {
                c.execute("UPDATE products SET body=?2 WHERE id=?1", rusqlite::params![id, n])?;
            }
            if let Some(v) = condition {
                c.execute("UPDATE products SET variant=?2 WHERE id=?1", rusqlite::params![id, v])?;
            }
            if let Some(list) = tags {
                // The site's own tags (binder-…, set-… and so on) stay; the owner's typed ones are replaced.
                let now: String = c.query_row("SELECT tags FROM products WHERE id=?1", [id], |r| r.get(0)).optional()?.unwrap_or_default();
                let mut all: Vec<String> = now.split(',').map(str::trim).filter(|t| !t.is_empty() && !tag_for_people(t)).map(str::to_string).collect();
                all.extend(list.iter().cloned());
                c.execute("UPDATE products SET tags=?2 WHERE id=?1", rusqlite::params![id, all.join(", ")])?;
            }
        }
        Ok(())
    })();
    match r {
        Ok(()) => c.execute_batch("COMMIT"),
        Err(e) => {
            let _ = c.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// True while a card or binder cover still uses this picture. Copies of a card share one file.
pub fn image_in_use(c: &Connection, path: &str) -> rusqlite::Result<bool> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM products WHERE image=?1) OR EXISTS(SELECT 1 FROM collections WHERE image=?1)", [path], |r| r.get(0))
}

/// Uploaded card pictures with the catalogue card they came from.
pub fn catalogue_uploads(c: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut st = c.prepare("SELECT DISTINCT image, api_id FROM products WHERE image LIKE '/uploads/%' AND api_id<>''")?;
    let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

pub fn replace_image(c: &Connection, old: &str, new: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE products SET image=?2 WHERE image=?1", [old, new])?;
    Ok(())
}

pub fn delete_entry(c: &Connection, id: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM collection_products WHERE product_id=?1", [id])?;
    c.execute("DELETE FROM products WHERE id=?1", [id])?;
    Ok(())
}

pub fn product_by_id(c: &Connection, id: i64) -> rusqlite::Result<Option<Product>> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM products p WHERE p.id=?"))?;
    let mut it = st.query_map([id], map)?;
    it.next().transpose()
}

/// Existing cards with the same (case-insensitive) name, or containing it, excluding `exclude_id`.
pub fn similar(c: &Connection, title: &str, exclude_id: i64) -> rusqlite::Result<Vec<(i64, String)>> {
    let like = format!("%{}%", title.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    let mut st = c.prepare(
        "SELECT id,title FROM products WHERE id<>?1 AND (lower(title)=lower(?2) OR title LIKE ?3 ESCAPE '\\')
         ORDER BY (lower(title)=lower(?2)) DESC, id DESC LIMIT 3",
    )?;
    let v = st.query_map(rusqlite::params![exclude_id, title, like], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

pub fn total_value(c: &Connection) -> rusqlite::Result<f64> {
    c.query_row("SELECT COALESCE(SUM(price),0) FROM products", [], |r| r.get(0))
}

/// Every product id matching the filters (for "select all matching"). Capped for safety.
pub fn ids(c: &Connection, q: &Query) -> rusqlite::Result<Vec<i64>> {
    let mut wh: Vec<String> = vec!["1=1".into()];
    let mut args: Vec<Value> = vec![];
    let mut join = String::new();
    if let Some(col) = &q.collection {
        join = " JOIN collection_products cp ON cp.product_id=p.id".into();
        wh.push("cp.collection=?".into());
        args.push(Value::Text(col.clone()));
    }
    for term in q.q.split_whitespace().take(6) {
        wh.push("(p.title LIKE ? ESCAPE '\\' OR p.tags LIKE ? ESCAPE '\\')".into());
        let like = format!("%{}%", term.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        args.push(Value::Text(like.clone()));
        args.push(Value::Text(like));
    }
    let sql = format!("SELECT DISTINCT p.id FROM products p{join} WHERE {} LIMIT 5000", wh.join(" AND "));
    let mut st = c.prepare(&sql)?;
    let v = st.query_map(params_from_iter(args.iter()), |r| r.get(0))?.collect::<Result<Vec<i64>, _>>()?;
    Ok(v)
}

/* ---------- the owner's dashboard ---------- */

/// What a description holding only a condition should become: "LP-NM" is a condition, "Poke ball holo, LP-NM" is a note.
pub fn condition_in_note(note: &str) -> Option<&'static str> {
    let t: String = note.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.replace(" - ", "-").replace(" / ", "-").replace('/', "-").replace("dmg", "damaged");
    Some(match t.as_str() {
        "nm" | "near mint" | "mint" => "Near Mint",
        "lp-nm" | "nm-lp" => "LP-NM",
        "lp" | "lightly played" => "Lightly Played",
        "mp-lp" | "lp-mp" => "MP-LP",
        "mp" | "moderately played" => "Moderately Played",
        "hp-mp" | "mp-hp" => "HP-MP",
        "hp" | "heavily played" => "Heavily Played",
        "damaged-hp" | "hp-damaged" => "Damaged-HP",
        "damaged" => "Damaged",
        "sealed" => "Sealed",
        _ => return None,
    })
}

/// Cards whose whole description is a condition: (id, title, the note, the condition it means).
pub fn condition_notes(c: &Connection) -> rusqlite::Result<Vec<(i64, String, String, String)>> {
    let mut st = c.prepare("SELECT id,title,body FROM products WHERE body<>'' AND length(body)<=40 ORDER BY id DESC")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
    let mut out = vec![];
    for row in rows {
        let (id, title, body) = row?;
        if let Some(cond) = condition_in_note(&body) {
            out.push((id, title, body.trim().to_string(), cond.to_string()));
        }
    }
    Ok(out)
}

/// Moves each of those into the card's condition and clears the description. Older cards keep their finish and rarity.
pub fn move_condition_notes(c: &Connection) -> rusqlite::Result<usize> {
    let list = condition_notes(c)?;
    let tx = c.unchecked_transaction()?;
    for (id, _, _, cond) in &list {
        let variant: String = tx.query_row("SELECT variant FROM products WHERE id=?1", [id], |r| r.get(0))?;
        let mut parts: Vec<String> = variant.split(" / ").map(str::to_string).collect();
        if parts.len() > 1 { parts[0] = cond.clone(); } else { parts = vec![cond.clone()]; }
        tx.execute("UPDATE products SET variant=?2, body='' WHERE id=?1", rusqlite::params![id, parts.join(" / ")])?;
    }
    tx.commit()?;
    Ok(list.len())
}

/// How many cards came in since some time ago (an SQLite modifier like "-7 days"), and what they're worth.
pub fn added_since(c: &Connection, ago: &str) -> rusqlite::Result<(i64, f64)> {
    c.query_row("SELECT COUNT(*), COALESCE(SUM(price),0) FROM products WHERE created_at >= strftime('%Y-%m-%dT%H:%M:%S','now',?1)", [ago], |r| Ok((r.get(0)?, r.get(1)?)))
}

/// Cards added each day for the last `days` days, oldest first, every day listed.
pub fn added_per_day(c: &Connection, days: i64) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut st = c.prepare(
        "WITH RECURSIVE d(day) AS (SELECT date('now', ?1) UNION ALL SELECT date(day,'+1 day') FROM d WHERE day < date('now'))
         SELECT d.day, (SELECT COUNT(*) FROM products p WHERE substr(p.created_at,1,10)=d.day) FROM d",
    )?;
    let v = st.query_map([format!("-{} days", days - 1)], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

/// Each binder with its number of cards and their value, most valuable first.
pub fn binder_values(c: &Connection) -> rusqlite::Result<Vec<(String, String, i64, f64)>> {
    let mut st = c.prepare(
        "SELECT c.title, c.handle, COUNT(p.id), COALESCE(SUM(p.price),0) FROM collections c
         JOIN collection_products cp ON cp.collection=c.handle JOIN products p ON p.id=cp.product_id
         GROUP BY c.handle ORDER BY 4 DESC",
    )?;
    let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

/// The cards opened most often, with how many times.
pub fn most_viewed(c: &Connection, limit: i64) -> rusqlite::Result<Vec<(Product, i64)>> {
    let mut st = c.prepare(&format!("SELECT {COLS}, p.hits FROM products p WHERE p.hits>0 ORDER BY p.hits DESC, p.id DESC LIMIT ?1"))?;
    let v = st.query_map([limit], |r| Ok((map(r)?, r.get(NCOLS)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

/// Cards missing a photo or a price: how many, and the first few.
pub fn missing(c: &Connection, what: &str, limit: i64) -> rusqlite::Result<(i64, Vec<Product>)> {
    let wh = if what == "photo" { "p.image=''" } else { "p.price<=0" };
    let n = c.query_row(&format!("SELECT COUNT(*) FROM products p WHERE {wh}"), [], |r| r.get(0))?;
    let mut st = c.prepare(&format!("SELECT {COLS} FROM products p WHERE {wh} ORDER BY p.id DESC LIMIT ?1"))?;
    let v = st.query_map([limit], map)?.collect::<Result<Vec<_>, _>>()?;
    Ok((n, v))
}

/// What visitors type into search most.
pub fn top_searches(c: &Connection, limit: i64) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut st = c.prepare("SELECT term, n FROM search_log ORDER BY n DESC, last DESC LIMIT ?1")?;
    let v = st.query_map([limit], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

#[cfg(test)]
mod dashboard_tests {
    use super::*;

    #[test]
    fn the_owners_order() {
        let c = Connection::open_in_memory().unwrap();
        register_set_code(&c).unwrap();
        c.execute_batch(SCHEMA).unwrap();
        migrate(&c).unwrap();
        for (i, t) in ["A", "B", "C", "D"].iter().enumerate() {
            c.execute("INSERT INTO products(id,handle,title,created_at) VALUES(?1,?2,?2,?3)", rusqlite::params![i as i64 + 1, t, format!("2026-01-0{}T00:00:00", i + 1)]).unwrap();
        }
        let order = |c: &Connection| list(c, &Query::default()).unwrap().0.iter().map(|p| p.title.clone()).collect::<Vec<_>>().join("");
        assert_eq!(order(&c), "DCBA", "newest first to begin with");
        reorder_groups(&c, &[vec![2], vec![4], vec![3]]).unwrap();
        assert_eq!(order(&c), "BDCA", "the dragged three swap among their own places; A stays put");
        let copy = duplicate(&c, 4).unwrap().unwrap();
        assert!(copy > 4);
        assert_eq!(list(&c, &Query::default()).unwrap().0.len(), 4, "the copy joins its card");
        assert_eq!(order(&c), "BDCA", "a copy doesn't move its card");
        c.execute("INSERT INTO products(id,handle,title,created_at) VALUES(9,'E','E','2026-02-01T00:00:00')", []).unwrap();
        assert_eq!(order(&c), "EBDCA", "a new card goes to the top");
        groups_to_top(&c, &[vec![3], vec![1]]).unwrap();
        assert_eq!(order(&c), "CAEBD", "a home row arranged from another sort puts its cards first, in the dragged order");
        assert_eq!(clean_tags(&["Grail".into(), "grail".into(), "  for, trade ".into(), "binder-x".into(), "".into()]), vec!["Grail".to_string(), "for trade".to_string()]);
    }

    /// Different cards that share a rarity must never count as copies of each other.
    /// (A shifted column once grouped every "Common" Pikachu from every set into one card.)
    #[test]
    fn copies_group_by_card_not_by_rarity() {
        let c = Connection::open_in_memory().unwrap();
        register_set_code(&c).unwrap();
        c.execute_batch(SCHEMA).unwrap();
        migrate(&c).unwrap();
        assert_eq!(COLS.split(',').count(), NCOLS, "NCOLS must match COLS");
        let add = |id: i64, title: &str, api: &str, price: f64| {
            c.execute(
                "INSERT INTO products(id,handle,title,api_id,price,rarity,variant,created_at,hits) VALUES(?1,?2,?3,?4,?5,'Common','Near Mint','2026-01-01T00:00:00',?1)",
                rusqlite::params![id, format!("h{id}"), title, api, price],
            )
            .unwrap();
        };
        add(1, "Pikachu - 058/102 - Base Set", "base1-58", 50.0);
        add(2, "Pikachu - 060/064 - Jungle", "base2-60", 40.0);
        add(3, "Pikachu - 058/102 - Base Set", "base1-58", 50.0); // a real second copy of card 1
        let tsv = catalogue(&c).unwrap();
        let card_lines = tsv.lines().filter(|l| l.starts_with("C\t")).count();
        assert_eq!(card_lines, 2, "two different Pikachus, one with two copies:\n{tsv}");
        let p1 = product_by_id(&c, 1).unwrap().unwrap();
        let v = versions(&c, &p1).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].qty, 2, "Base Set Pikachu has two copies");
        let p2 = product_by_id(&c, 2).unwrap().unwrap();
        assert_eq!(versions(&c, &p2).unwrap()[0].qty, 1, "Jungle Pikachu is its own card");
        let mv = most_viewed(&c, 5).unwrap();
        assert_eq!(mv[0].1, 3, "view counts are read from the hits column");
        let (rows, total) = list(&c, &Query::default()).unwrap();
        assert_eq!(total, 2);
        assert_eq!(rows.iter().map(|p| p.qty).sum::<i64>(), 3);
    }

    /// Sorting by set: release date from the set code, else the set's name; unknown sets last.
    #[test]
    fn sorting_by_set_release_date() {
        let c = Connection::open_in_memory().unwrap();
        register_set_code(&c).unwrap();
        c.execute_batch(SCHEMA).unwrap();
        migrate(&c).unwrap();
        assert_eq!(set_code("tcgdex:sv08-053"), "sv08");
        assert_eq!(set_code("tcgdex:me02.5-012"), "me02.5");
        assert_eq!(set_code("own:abc"), "");
        let add = |id: i64, title: &str, api: &str, set: &str| {
            c.execute("INSERT INTO products(id,handle,title,api_id,card_set,created_at) VALUES(?1,?2,?3,?4,?5,'2026-01-01T00:00:00')", rusqlite::params![id, format!("h{id}"), title, api, set]).unwrap();
        };
        add(1, "New", "tcgdex:me02-001", "Phantasmal Flames");
        add(2, "Old", "tcgdex:base1-004", "Base Set");
        add(3, "Mid", "", "Surging Sparks");
        add(4, "Unknown", "tcgdex:zzz-001", "Mystery");
        save_set(&c, "me02", "Phantasmal Flames", "Mega Evolution", "2025-11-14").unwrap();
        save_set(&c, "base1", "Base Set", "Base", "1999-01-09").unwrap();
        save_set(&c, "sv08", "Surging Sparks", "Scarlet & Violet", "2024-11-08").unwrap();
        let titles = |sort: &str| list(&c, &Query { sort: sort.into(), ..Default::default() }).unwrap().0.iter().map(|p| p.title.clone()).collect::<Vec<_>>().join(",");
        assert_eq!(titles("set-old"), "Old,Mid,New,Unknown");
        assert_eq!(titles("set-new"), "New,Mid,Old,Unknown");
        assert_eq!(sets_to_fetch(&c).unwrap(), vec!["zzz".to_string()]);
        assert!(set_names_to_fetch(&c).unwrap().is_empty(), "Surging Sparks is known by name");
    }

    #[test]
    fn the_srp_follows_the_condition_and_never_the_owners_price() {
        let mut p = Product { id: 1, handle: "x".into(), title: "Pikachu".into(), body: String::new(), tags: String::new(), variant: "Lightly Played".into(), price: 999.0, compare_at: None, available: true, image: String::new(), image_w: 0, image_h: 0, card_no: String::new(), card_set: String::new(), api_id: "tcgdex:base1-58".into(), finish: String::new(), price_updated: String::new(), price_src: String::new(), rarity: String::new(), market_php: 0.0, qty: 1, copy_ids: String::new() };
        assert_eq!(p.srp_line(), "SRP: no market price yet");
        p.market_php = 100.0;
        assert_eq!(p.srp(), Some(80.0));
        assert_eq!(p.srp_line(), "SRP ₱80 · Lightly Played · 80% of Near Mint ₱100");
        assert_eq!(p.price, 999.0, "the owner's price is untouched");
        p.variant = String::new();
        assert_eq!(p.srp(), Some(100.0), "no condition written means Near Mint");
        p.variant = "PSA 10".into();
        assert_eq!(p.srp(), None);
        assert!(p.srp_line().starts_with("Graded"));
        p.api_id.clear();
        assert_eq!(p.srp_line(), "", "own photos have no SRP");
    }

    #[test]
    fn conditions_typed_as_notes() {
        assert_eq!(condition_in_note("LP-NM"), Some("LP-NM"));
        assert_eq!(condition_in_note(" lp - nm "), Some("LP-NM"));
        assert_eq!(condition_in_note("MP"), Some("Moderately Played"));
        assert_eq!(condition_in_note("HP-MP"), Some("HP-MP"));
        assert_eq!(condition_in_note("dmg-hp"), Some("Damaged-HP"));
        assert_eq!(condition_in_note("Sealed"), Some("Sealed"));
        assert_eq!(condition_in_note("Poke ball holo, LP-NM"), None);
        assert_eq!(condition_in_note("W/ crease"), None);
    }
}

/// Typed tags, tidied: no commas inside, short, no repeats, at most twelve.
pub fn clean_tags(list: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for t in list {
        let t: String = t.replace(',', " ").split_whitespace().collect::<Vec<_>>().join(" ").chars().filter(|c| !c.is_control()).take(30).collect();
        if !t.is_empty() && tag_for_people(&t) && !out.iter().any(|o| o.eq_ignore_ascii_case(&t)) {
            out.push(t);
        }
        if out.len() >= 12 {
            break;
        }
    }
    out
}

/// The owner dragged cards into a new order. Each group is one card's copies, top to bottom.
/// The groups take over the places they held between them, so cards that weren't on screen keep theirs.
/// These groups go to the very top of the owner's order, in the order given; everything else keeps its place below them.
pub fn groups_to_top(c: &Connection, groups: &[Vec<i64>]) -> rusqlite::Result<()> {
    let tx = c.unchecked_transaction()?;
    let first: f64 = tx.query_row("SELECT COALESCE(MIN(position), 0) FROM products", [], |r| r.get(0))?;
    let n = groups.len() as f64;
    for (i, g) in groups.iter().enumerate() {
        for id in g {
            tx.execute("UPDATE products SET position=?2 WHERE id=?1", rusqlite::params![id, first - n + i as f64])?;
        }
    }
    tx.commit()
}

pub fn reorder_groups(c: &Connection, groups: &[Vec<i64>]) -> rusqlite::Result<()> {
    let tx = c.unchecked_transaction()?;
    let mut slots: Vec<f64> = vec![];
    for g in groups {
        let mut best: Option<f64> = None;
        for id in g {
            let p: Option<f64> = tx.query_row("SELECT position FROM products WHERE id=?1", [id], |r| r.get(0)).optional()?;
            if let Some(p) = p {
                best = Some(best.map_or(p, |b: f64| b.min(p)));
            }
        }
        slots.push(best.unwrap_or(f64::MAX));
    }
    slots.sort_by(f64::total_cmp);
    // Two groups that somehow share a place get pulled apart, keeping the new order.
    for i in 1..slots.len() {
        if slots[i] <= slots[i - 1] {
            slots[i] = slots[i - 1] + 0.001;
        }
    }
    for (g, slot) in groups.iter().zip(slots) {
        for id in g {
            tx.execute("UPDATE products SET position=?2 WHERE id=?1", rusqlite::params![id, slot])?;
        }
    }
    tx.commit()
}
