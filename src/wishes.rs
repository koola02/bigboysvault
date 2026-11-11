//! Wishlist alerts: every hour, each trainer's Wishlist cards are matched against the vault and every open
//! store. When a wanted card is for sale somewhere new, the trainer's bell rings once for that place.
use crate::sellers::{catalogue_key, notify};
use rusqlite::{params, Connection};

pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS wish_alerts(
    member_id INTEGER NOT NULL, wish_id INTEGER NOT NULL, source TEXT NOT NULL, created_at INTEGER NOT NULL,
    PRIMARY KEY(member_id, wish_id, source));";

/// No trainer gets more than this many wishlist notes in one hourly pass; the rest wait for the next one.
const PER_RUN: usize = 5;

pub fn migrate(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(SCHEMA)
}

struct Hit {
    source: String,
    message: String,
    link: String,
}

/// One pass. Returns how many notes went out.
pub fn run(c: &Connection, now: i64) -> rusqlite::Result<usize> {
    let wishes: Vec<(i64, i64, String, String)> = c
        .prepare(
            "SELECT w.id, w.member_id, w.card_id, w.title FROM member_cards w JOIN members m ON m.id=w.member_id
             WHERE w.kind='wish' AND w.card_id NOT LIKE 'own:%' AND w.card_id<>'' AND m.blocked=0 ORDER BY w.member_id, w.id",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let mut sent = 0;
    let mut per_member: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
    for (wish, member, card_id, title) in wishes {
        let key = catalogue_key(&card_id).to_string();
        for hit in hits(c, member, &key, &title)? {
            if per_member.get(&member).copied().unwrap_or(0) >= PER_RUN {
                break;
            }
            let fresh = c.execute(
                "INSERT OR IGNORE INTO wish_alerts(member_id, wish_id, source, created_at) VALUES(?1,?2,?3,?4)",
                params![member, wish, hit.source, now],
            )?;
            if fresh == 1 {
                notify(c, member, "wish", &hit.message, &hit.link)?;
                *per_member.entry(member).or_default() += 1;
                sent += 1;
            }
        }
    }
    Ok(sent)
}

/// Where this wanted card is for sale right now: the vault (cheapest copy) and each open store (not the trainer's own).
fn hits(c: &Connection, member: i64, key: &str, title: &str) -> rusqlite::Result<Vec<Hit>> {
    let short = short(title);
    let mut out = vec![];
    let vault: Option<(String, f64)> = c
        .query_row(
            "SELECT handle, price FROM products WHERE api_id=?1 AND available=1 AND price>0 ORDER BY price, id LIMIT 1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    if let Some((handle, price)) = vault {
        out.push(Hit {
            source: "vault".into(),
            message: format!("{short} from your wishlist is in the vault now, {}.", vault_search::peso(price)),
            link: format!("/products/{handle}"),
        });
    }
    let mut st = c.prepare_cached(
        "SELECT c.id, c.member_id, CASE WHEN m.store_name<>'' THEN m.store_name ELSE m.name END
         FROM member_cards c JOIN members m ON m.id=c.member_id
         WHERE c.kind='sale' AND c.status='Active' AND c.qty>0 AND c.member_id<>?2
           AND (c.card_id=?1 OR c.card_id LIKE ?1 || '#%')
           AND m.seller=1 AND m.blocked=0 AND m.profile_ok=1 AND m.status<>'Vacation'
         ORDER BY c.id",
    )?;
    let stores: Vec<(i64, i64, String)> = st.query_map(params![key, member], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    let mut seen = std::collections::HashSet::new();
    for (card, seller, store) in stores {
        if !seen.insert(seller) {
            continue; // one note per store, even when it lists the card twice
        }
        out.push(Hit {
            source: format!("store:{seller}"),
            message: format!("{store} has {short} from your wishlist for sale."),
            link: format!("/trainers/{seller}/cards/{card}"),
        });
    }
    Ok(out)
}

fn short(title: &str) -> String {
    let t = title.split(" - ").next().unwrap_or(title).trim();
    if t.chars().count() > 40 {
        format!("{}…", t.chars().take(39).collect::<String>())
    } else {
        t.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::members::{self, create};

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::register_set_code(&c).unwrap();
        c.execute_batch(crate::db::SCHEMA).unwrap();
        c.execute_batch(crate::blocks::SCHEMA).unwrap();
        c.execute_batch(members::SCHEMA).unwrap();
        members::migrate(&c).unwrap();
        crate::db::migrate(&c).unwrap();
        migrate(&c).unwrap();
        c
    }

    #[test]
    fn a_wanted_card_rings_once_per_place() {
        let c = db();
        let fan = create(&c, "Fan", "h").unwrap().unwrap();
        let shop = create(&c, "Shop", "h").unwrap().unwrap();
        members::set_seller(&c, shop, true).unwrap();
        members::set_profile_ok(&c, shop, true).unwrap();
        c.execute(
            "INSERT INTO member_cards(member_id,position,card_id,title,number,card_set,image,image_large,cond,value,value_own,value_at,binder_id,status,expires_at,is_custom,qty,finish,notes,sold,view_n,save_n,updated_at,review_flag,kind)
             VALUES(?1,0,'tcgdex:base1-58','Pikachu - 58/102','58/102','Base Set','','','Near Mint',100,0,0,NULL,'Active',0,0,1,'','',0,0,0,0,0,'wish')",
            [fan],
        )
        .unwrap();
        assert_eq!(run(&c, 1).unwrap(), 0, "nothing for sale yet");
        c.execute("INSERT INTO products(id,handle,title,price,available,created_at,api_id) VALUES(1,'pika-1','Pikachu',150,1,'2026-01-01T00:00:00','tcgdex:base1-58')", []).unwrap();
        c.execute(
            "INSERT INTO member_cards(member_id,position,card_id,title,number,card_set,image,image_large,cond,value,value_own,value_at,binder_id,status,expires_at,is_custom,qty,finish,notes,sold,view_n,save_n,updated_at,review_flag,kind)
             VALUES(?1,0,'tcgdex:base1-58#2','Pikachu - 58/102','58/102','Base Set','','','Near Mint',100,120,0,NULL,'Active',0,0,1,'','',0,0,0,0,0,'sale')",
            [shop],
        )
        .unwrap();
        assert_eq!(run(&c, 2).unwrap(), 2, "the vault and the store");
        assert_eq!(run(&c, 3).unwrap(), 0, "told once");
        let msgs: Vec<String> = c.prepare("SELECT message FROM notifications WHERE member_id=?1 ORDER BY id").unwrap().query_map([fan], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(msgs, vec!["Pikachu from your wishlist is in the vault now, ₱150.".to_string(), "Shop has Pikachu from your wishlist for sale.".to_string()]);
    }
}
