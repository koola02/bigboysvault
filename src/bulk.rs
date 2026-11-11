//! Bulk edits on many cards at once. Every edit can return a snapshot so the UI can offer Undo.
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Snap {
    pub id: i64,
    pub price: f64,
    pub variant: String,
    pub binders: Vec<String>,
    #[serde(default)]
    pub finish: String,
    #[serde(default)]
    pub price_updated: String,
    /// Where the price came from. Missing in an Undo made before this was kept: then it stays as it is.
    #[serde(default)]
    pub price_src: Option<String>,
}

pub fn snapshot(c: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<Snap>> {
    let mut out = vec![];
    for id in ids {
        let row = c.query_row("SELECT price,variant,finish,price_updated,price_src FROM products WHERE id=?1", [id], |r| {
            Ok((r.get::<_, f64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
        });
        if let Ok((price, variant, finish, price_updated, price_src)) = row {
            let mut st = c.prepare("SELECT collection FROM collection_products WHERE product_id=?1")?;
            let binders = st.query_map([id], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
            out.push(Snap { id: *id, price, variant, binders, finish, price_updated, price_src: Some(price_src) });
        }
    }
    Ok(out)
}

pub fn restore(c: &Connection, snaps: &[Snap]) -> rusqlite::Result<usize> {
    let tx = c.unchecked_transaction()?;
    for s in snaps {
        tx.execute(
            "UPDATE products SET price=?2,variant=?3,finish=?4,price_updated=?5,price_src=COALESCE(?6,price_src) WHERE id=?1",
            rusqlite::params![s.id, s.price, s.variant, s.finish, s.price_updated, s.price_src],
        )?;
        tx.execute("DELETE FROM collection_products WHERE product_id=?1", [s.id])?;
        for b in &s.binders {
            tx.execute("INSERT OR IGNORE INTO collection_products(collection,product_id,position) VALUES(?1,?2,0)", rusqlite::params![b, s.id])?;
        }
    }
    tx.commit()?;
    Ok(snaps.len())
}

/// Only binders that exist (the owner can add and delete binders).
fn valid<'a>(c: &Connection, binders: &'a [String]) -> Vec<&'a str> {
    binders.iter().map(String::as_str).filter(|b| crate::db::binder_exists(c, b)).collect()
}

/// mode: "add" | "remove" | "move" (replace all binders with the given ones)
pub fn binder_op(c: &Connection, ids: &[i64], binders: &[String], mode: &str) -> rusqlite::Result<()> {
    let bs = valid(c, binders);
    let tx = c.unchecked_transaction()?;
    for id in ids {
        match mode {
            "remove" => {
                for b in &bs {
                    tx.execute("DELETE FROM collection_products WHERE product_id=?1 AND collection=?2", rusqlite::params![id, b])?;
                }
            }
            "move" => {
                tx.execute("DELETE FROM collection_products WHERE product_id=?1", [id])?;
                for b in &bs {
                    tx.execute("INSERT OR IGNORE INTO collection_products(collection,product_id,position) VALUES(?1,?2,0)", rusqlite::params![b, id])?;
                }
            }
            _ => {
                for b in &bs {
                    tx.execute("INSERT OR IGNORE INTO collection_products(collection,product_id,position) VALUES(?1,?2,0)", rusqlite::params![b, id])?;
                }
            }
        }
    }
    tx.commit()
}

pub fn set_variant(c: &Connection, ids: &[i64], v: &str) -> rusqlite::Result<()> {
    let tx = c.unchecked_transaction()?;
    for id in ids {
        tx.execute("UPDATE products SET variant=?2 WHERE id=?1", rusqlite::params![id, v])?;
    }
    tx.commit()
}

/// Every selected card gets this price. It's the owner's own now ('own'), so the nightly market refresh leaves it alone.
pub fn set_price(c: &Connection, ids: &[i64], price: f64) -> rusqlite::Result<()> {
    let tx = c.unchecked_transaction()?;
    for id in ids {
        tx.execute("UPDATE products SET price=?2, price_src='own', price_updated='' WHERE id=?1", rusqlite::params![id, price.max(0.0).round()])?;
    }
    tx.commit()
}

/// Raise or lower every value by a percentage (e.g. +10 or -15). Like set_price, the new prices are the owner's own.
pub fn adjust_price(c: &Connection, ids: &[i64], pct: f64) -> rusqlite::Result<()> {
    let tx = c.unchecked_transaction()?;
    for id in ids {
        tx.execute("UPDATE products SET price=MAX(0,ROUND(price*(1+?2/100.0))), price_src='own', price_updated='' WHERE id=?1", rusqlite::params![id, pct])?;
    }
    tx.commit()
}

/// Cards that were matched to a catalogue entry, with what's needed to price them: (id, catalogue id, finish, condition).
/// A card with no condition counts as Near Mint, as it does everywhere else.
pub fn priced_cards(c: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<(i64, String, String, String)>> {
    let mut out = vec![];
    for id in ids {
        let row = c.query_row("SELECT api_id,finish,variant FROM products WHERE id=?1 AND api_id<>''", [id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        });
        if let Ok((api_id, finish, variant)) = row {
            let cond = if variant.trim().is_empty() { "Near Mint".to_string() } else { variant };
            out.push((*id, api_id, finish, cond));
        }
    }
    Ok(out)
}

/// Today's suggested prices: (id, price). These follow the market from now on ('suggested'), each copy in its own
/// finish: the finish is never changed here.
pub fn apply_market(c: &Connection, updates: &[(i64, f64)], today: &str) -> rusqlite::Result<()> {
    let tx = c.unchecked_transaction()?;
    for (id, php) in updates {
        tx.execute("UPDATE products SET price=?2,price_updated=?3,price_src='suggested' WHERE id=?1", rusqlite::params![id, php, today])?;
    }
    tx.commit()
}

/// Delete many cards; returns their image paths so the files can be removed.
pub fn delete_many(c: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<String>> {
    let tx = c.unchecked_transaction()?;
    let mut images = vec![];
    for id in ids {
        if let Ok(img) = tx.query_row("SELECT image FROM products WHERE id=?1", [id], |r| r.get::<_, String>(0)) {
            images.push(img);
        }
        crate::db::repoint_open_orders(&tx, *id)?;
        tx.execute("DELETE FROM collection_products WHERE product_id=?1", [id])?;
        tx.execute("DELETE FROM products WHERE id=?1", [id])?;
    }
    tx.commit()?;
    Ok(images)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real database file in the temp folder, with the site's tables and one card that follows the market.
    fn db(name: &str) -> (Connection, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("bbv-bulk-{name}-{}.db", std::process::id()));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
        }
        let c = crate::db::open(path.to_str().unwrap()).unwrap();
        c.execute(
            "INSERT INTO products(id,handle,title,variant,price,api_id,finish,price_updated,price_src,created_at) VALUES(1,'a-1','A','Lightly Played',100,'tcgdex:x-1','normal','2026-11-01','suggested','2026-11-01T00:00:00')",
            [],
        )
        .unwrap();
        (c, path)
    }
    fn src(c: &Connection) -> (f64, String, String) {
        c.query_row("SELECT price,price_src,price_updated FROM products WHERE id=1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap()
    }

    #[test]
    fn a_bulk_price_is_the_owners_own_and_undo_brings_the_market_back() {
        let (c, path) = db("own");
        let undo = snapshot(&c, &[1]).unwrap();
        set_price(&c, &[1], 250.0).unwrap();
        assert_eq!(src(&c), (250.0, "own".into(), String::new()), "the nightly refresh must leave it alone");
        adjust_price(&c, &[1], 10.0).unwrap();
        assert_eq!(src(&c), (275.0, "own".into(), String::new()));
        restore(&c, &undo).unwrap();
        assert_eq!(src(&c), (100.0, "suggested".into(), "2026-11-01".into()));
        // An Undo made before price_src was kept leaves it as it is.
        let mut old = undo.clone();
        old[0].price_src = None;
        set_price(&c, &[1], 5.0).unwrap();
        restore(&c, &old).unwrap();
        assert_eq!(src(&c), (100.0, "own".into(), "2026-11-01".into()));
        drop(c);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn market_today_reads_each_copys_condition() {
        let (c, path) = db("market");
        c.execute("INSERT INTO products(id,handle,title,variant,price,api_id,created_at) VALUES(2,'b-2','B','',50,'tcgdex:x-2','2026-11-01T00:00:00')", []).unwrap();
        let cards = priced_cards(&c, &[1, 2, 3]).unwrap();
        assert_eq!(cards.len(), 2, "a card that doesn't exist is skipped");
        assert_eq!(cards[0].3, "Lightly Played");
        assert_eq!(cards[1].3, "Near Mint", "no condition counts as Near Mint");
        apply_market(&c, &[(1, 80.0)], "2026-11-11").unwrap();
        assert_eq!(src(&c), (80.0, "suggested".into(), "2026-11-11".into()));
        let finish: String = c.query_row("SELECT finish FROM products WHERE id=1", [], |r| r.get(0)).unwrap();
        assert_eq!(finish, "normal", "the copy keeps its own finish");
        drop(c);
        let _ = std::fs::remove_file(path);
    }
}
