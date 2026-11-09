//! Editable page sections ("blocks") and site settings. The home page is a list of blocks the owner can
//! add, edit, reorder, hide and delete from /admin/page. Counts and card lists inside blocks stay automatic.
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS blocks(
  id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, position INTEGER NOT NULL,
  visible INTEGER NOT NULL DEFAULT 1, data TEXT NOT NULL DEFAULT '{}');
CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

pub const KINDS: [&str; 10] = ["hero", "ticker", "stats", "binders", "row", "feature", "text", "showcase", "highlights", "vouches"];

#[derive(Clone, Debug)]
pub struct Block {
    pub id: i64,
    pub kind: String,
    pub visible: bool,
    pub data: Value,
}

/// Starting content for each kind of block. `{count}` in text is replaced by the live card total.
pub fn default_data(kind: &str) -> Value {
    match kind {
        "hero" => json!({
            "eyebrow": "Collectibles & Cards", "title": "Big Boys Vault",
            "subtitle": "{count} cards and collectibles. Search by name, card number or binder.",
            "button_label": "Browse all cards", "button_link": "/all", "sticker": "",
            "img_a": "/static/img/slaking.png?v=5", "img_a_w": 431, "img_a_h": 303,
            "img_b": "/static/img/snorlax.png?v=5", "img_b_w": 375, "img_b_h": 299
        }),
        "ticker" => json!({ "phrases": "New this week\nGraded cards added" }),
        "binders" => json!({ "title": "Binders" }),
        "row" => json!({ "handle": "playables", "title": "", "eyebrow": "", "sort": "new", "spotlight": false }),
        "feature" => json!({
            "eyebrow": "", "title": "Find a card",
            "subtitle": "Search {count} cards by name, set or card number.", "button_label": "Browse all {count}", "button_link": "/all",
            "img": "/static/img/logo.webp?v=4", "img_w": 900, "img_h": 900
        }),
        "text" => json!({ "title": "A note", "body": "Write something here." }),
        // Hand-picked cards from anywhere in the collection; the fewer, the bigger the spotlight.
        "showcase" => json!({ "title": "Showcase", "eyebrow": "Hand-picked", "ids": [] }),
        // From trainers' accounts, once the owner has approved them.
        "highlights" => json!({ "title": "Trainers' picks", "eyebrow": "From the community" }),
        "vouches" => json!({ "title": "What trainers say", "eyebrow": "Vouches" }),
        _ => json!({}),
    }
}

/// One-time switch to the Big Boys Vault identity: new site name and fresh default copy for the page sections.
pub fn rebrand_once(c: &Connection) -> rusqlite::Result<()> {
    if setting(c, "brand_v")?.as_deref() == Some("bigboys-1") {
        return Ok(());
    }
    c.execute("DELETE FROM blocks", [])?;
    set_setting(c, "brand", "Big Boys Vault")?;
    set_setting(c, "brand_v", "bigboys-1")
}

/// Hero blocks still pointing at the bundled Slaking/Snorlax art get the current files (new sizes, cache-busted).
/// Blocks where the owner picked their own pictures are left alone.
pub fn refresh_hero_art(c: &Connection) -> rusqlite::Result<()> {
    if setting(c, "hero_art_v")?.as_deref() == Some("5") {
        return Ok(());
    }
    let fresh = default_data("hero");
    for b in blocks(c)?.into_iter().filter(|b| b.kind == "hero") {
        let mut d = b.data.clone();
        for (img, w, h, file) in [("img_a", "img_a_w", "img_a_h", "/static/img/slaking.png"), ("img_b", "img_b_w", "img_b_h", "/static/img/snorlax.png")] {
            if d[img].as_str().is_some_and(|u| u.starts_with(file)) {
                for k in [img, w, h] {
                    d[k] = fresh[k].clone();
                }
            }
        }
        if d != b.data {
            c.execute("UPDATE blocks SET data=?1 WHERE id=?2", rusqlite::params![d.to_string(), b.id])?;
        }
    }
    set_setting(c, "hero_art_v", "5")
}

pub fn ensure_blocks(c: &Connection) -> rusqlite::Result<()> {
    let n: i64 = c.query_row("SELECT COUNT(*) FROM blocks", [], |r| r.get(0))?;
    if n > 0 {
        return Ok(());
    }
    let mut graded = default_data("row");
    graded["handle"] = json!("graded-cards");
    graded["eyebrow"] = json!("");
    graded["sort"] = json!("price-desc");
    graded["spotlight"] = json!(true);
    let mut figures = default_data("row");
    figures["handle"] = json!("pokemon-figures");
    figures["eyebrow"] = json!("");
    let seed = [
        ("hero", default_data("hero")),
        ("binders", default_data("binders")),
        ("row", graded),
        ("row", default_data("row")),
        ("row", figures),
        ("feature", default_data("feature")),
    ];
    for (i, (k, d)) in seed.iter().enumerate() {
        c.execute(
            "INSERT INTO blocks(kind,position,visible,data) VALUES(?1,?2,1,?3)",
            rusqlite::params![k, i as i64, d.to_string()],
        )?;
    }
    Ok(())
}

/// Adds the two community sections to an existing home page, once, just above the closing banner. The owner can then move or remove them.
pub fn community_once(c: &Connection) -> rusqlite::Result<()> {
    if setting(c, "community_v")?.as_deref() == Some("1") {
        return Ok(());
    }
    let all = blocks(c)?;
    if !all.is_empty() {
        let at = all.iter().position(|b| b.kind == "feature").unwrap_or(all.len());
        let mut after = at.checked_sub(1).map(|i| all[i].id);
        for kind in ["highlights", "vouches"] {
            let id = add_block_at(c, kind, &default_data(kind), after.or(Some(-1)))?;
            after = Some(id);
        }
    }
    set_setting(c, "community_v", "1")
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Block> {
    let d: String = r.get(3)?;
    Ok(Block { id: r.get(0)?, kind: r.get(1)?, visible: r.get::<_, i64>(2)? != 0, data: serde_json::from_str(&d).unwrap_or(json!({})) })
}

pub fn blocks(c: &Connection) -> rusqlite::Result<Vec<Block>> {
    let mut st = c.prepare("SELECT id,kind,visible,data FROM blocks ORDER BY position,id")?;
    let v = st.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(v)
}

pub fn block(c: &Connection, id: i64) -> rusqlite::Result<Option<Block>> {
    c.query_row("SELECT id,kind,visible,data FROM blocks WHERE id=?1", [id], row).optional()
}

pub fn add_block(c: &Connection, kind: &str) -> rusqlite::Result<i64> {
    let pos: i64 = c.query_row("SELECT COALESCE(MAX(position),-1)+1 FROM blocks", [], |r| r.get(0))?;
    c.execute(
        "INSERT INTO blocks(kind,position,visible,data) VALUES(?1,?2,1,?3)",
        rusqlite::params![kind, pos, default_data(kind).to_string()],
    )?;
    Ok(c.last_insert_rowid())
}

/// Add a section right after another one (or at the end), with its starting content.
pub fn add_block_at(c: &Connection, kind: &str, data: &Value, after: Option<i64>) -> rusqlite::Result<i64> {
    let pos: i64 = c.query_row("SELECT COALESCE(MAX(position),-1)+1 FROM blocks", [], |r| r.get(0))?;
    c.execute("INSERT INTO blocks(kind,position,visible,data) VALUES(?1,?2,1,?3)", rusqlite::params![kind, pos, data.to_string()])?;
    let id = c.last_insert_rowid();
    if let Some(a) = after {
        let mut ids: Vec<i64> = blocks(c)?.into_iter().map(|b| b.id).filter(|x| *x != id).collect();
        // -1 = the very top; an unknown id = the bottom.
        let at = if a == -1 { 0 } else { ids.iter().position(|x| *x == a).map(|i| i + 1).unwrap_or(ids.len()) };
        ids.insert(at, id);
        reorder(c, &ids)?;
    }
    Ok(id)
}

pub fn save_block(c: &Connection, id: i64, data: &Value) -> rusqlite::Result<()> {
    c.execute("UPDATE blocks SET data=?2 WHERE id=?1", rusqlite::params![id, data.to_string()])?;
    Ok(())
}

pub fn move_block(c: &Connection, id: i64, up: bool) -> rusqlite::Result<()> {
    let mut ids: Vec<i64> = blocks(c)?.into_iter().map(|b| b.id).collect();
    if let Some(i) = ids.iter().position(|x| *x == id) {
        let j = if up { i.checked_sub(1) } else { (i + 1 < ids.len()).then_some(i + 1) };
        if let Some(j) = j {
            ids.swap(i, j);
        }
    }
    for (pos, bid) in ids.iter().enumerate() {
        c.execute("UPDATE blocks SET position=?2 WHERE id=?1", rusqlite::params![bid, pos as i64])?;
    }
    Ok(())
}

/// Put sections in the given order (drag and drop). Sections not listed keep their relative order at the end.
pub fn reorder(c: &Connection, order: &[i64]) -> rusqlite::Result<()> {
    let current: Vec<i64> = blocks(c)?.into_iter().map(|b| b.id).collect();
    let mut ids: Vec<i64> = order.iter().copied().filter(|id| current.contains(id)).collect();
    ids.dedup();
    ids.extend(current.iter().copied().filter(|id| !order.contains(id)));
    for (pos, bid) in ids.iter().enumerate() {
        c.execute("UPDATE blocks SET position=?2 WHERE id=?1", rusqlite::params![bid, pos as i64])?;
    }
    Ok(())
}

pub fn set_visible(c: &Connection, id: i64, on: bool) -> rusqlite::Result<()> {
    c.execute("UPDATE blocks SET visible=?2 WHERE id=?1", rusqlite::params![id, on as i64])?;
    Ok(())
}

pub fn delete_block(c: &Connection, id: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM blocks WHERE id=?1", [id])?;
    Ok(())
}

pub fn setting(c: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    c.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0)).optional()
}

pub fn set_setting(c: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=?2",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

pub fn set_product_image(c: &Connection, id: i64, image: Option<&str>) -> rusqlite::Result<()> {
    c.execute("UPDATE products SET image=?2 WHERE id=?1", rusqlite::params![id, image.unwrap_or("")])?;
    Ok(())
}
