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
}

pub fn snapshot(c: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<Snap>> {
    let mut out = vec![];
    for id in ids {
        let row = c.query_row("SELECT price,variant,finish,price_updated FROM products WHERE id=?1", [id], |r| {
            Ok((r.get::<_, f64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
        });
        if let Ok((price, variant, finish, price_updated)) = row {
            let mut st = c.prepare("SELECT collection FROM collection_products WHERE product_id=?1")?;
            let binders = st.query_map([id], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
            out.push(Snap { id: *id, price, variant, binders, finish, price_updated });
        }
    }
    Ok(out)
}

pub fn restore(c: &Connection, snaps: &[Snap]) -> rusqlite::Result<usize> {
    let tx = c.unchecked_transaction()?;
    for s in snaps {
        tx.execute(
            "UPDATE products SET price=?2,variant=?3,finish=?4,price_updated=?5 WHERE id=?1",
            rusqlite::params![s.id, s.price, s.variant, s.finish, s.price_updated],
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

pub fn set_price(c: &Connection, ids: &[i64], price: f64) -> rusqlite::Result<()> {
    let tx = c.unchecked_transaction()?;
    for id in ids {
        tx.execute("UPDATE products SET price=?2 WHERE id=?1", rusqlite::params![id, price.max(0.0).round()])?;
    }
    tx.commit()
}

/// Raise or lower every value by a percentage (e.g. +10 or -15).
pub fn adjust_price(c: &Connection, ids: &[i64], pct: f64) -> rusqlite::Result<()> {
    let tx = c.unchecked_transaction()?;
    for id in ids {
        tx.execute("UPDATE products SET price=MAX(0,ROUND(price*(1+?2/100.0))) WHERE id=?1", rusqlite::params![id, pct])?;
    }
    tx.commit()
}

/// Cards that were matched to a catalogue entry, with the info needed to refresh their price.
pub fn priced_cards(c: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<(i64, String, String)>> {
    let mut out = vec![];
    for id in ids {
        if let Ok(row) = c.query_row("SELECT api_id,finish FROM products WHERE id=?1 AND api_id<>''", [id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) {
            out.push((*id, row.0, row.1));
        }
    }
    Ok(out)
}

pub fn apply_market(c: &Connection, updates: &[(i64, f64, String)], today: &str) -> rusqlite::Result<()> {
    let tx = c.unchecked_transaction()?;
    for (id, php, finish) in updates {
        tx.execute("UPDATE products SET price=?2,finish=?3,price_updated=?4 WHERE id=?1", rusqlite::params![id, php, finish, today])?;
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
