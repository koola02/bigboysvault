//! Seeds the local database from a public Shopify catalog (`/products.json`).
//!
//! The request pacing is a small feedback controller (AIMD): every success shrinks the delay a
//! little (additive/multiplicative decrease); every 429 or error doubles it and honours
//! `Retry-After`. The measured error signal drives the next request rate, so it never hammers the host.
//!
//!   cargo run --release --bin import -- https://marcurushop.com data.db
use marcuru_rs::db;
use serde_json::Value;
use std::{thread::sleep, time::Duration};

struct Pacer {
    delay: Duration,
}
impl Pacer {
    const MIN: Duration = Duration::from_millis(400);
    const MAX: Duration = Duration::from_secs(60);
    fn ok(&mut self) {
        self.delay = self.delay.mul_f32(0.85).max(Self::MIN);
    }
    fn back_off(&mut self, retry_after: Option<u64>) {
        self.delay = (self.delay * 2).min(Self::MAX);
        if let Some(s) = retry_after {
            self.delay = self.delay.max(Duration::from_secs(s));
        }
    }
}

fn get(client: &reqwest::blocking::Client, pacer: &mut Pacer, url: &str) -> Option<Value> {
    for attempt in 0..8 {
        sleep(pacer.delay);
        match client.get(url).send() {
            Ok(r) if r.status().is_success() => {
                pacer.ok();
                return r.json().ok();
            }
            Ok(r) if r.status().as_u16() == 404 => return None,
            Ok(r) => {
                let ra = r.headers().get("retry-after").and_then(|h| h.to_str().ok()).and_then(|s| s.parse().ok());
                eprintln!("  {} on {url} (attempt {attempt}); backing off", r.status());
                pacer.back_off(ra);
            }
            Err(e) => {
                eprintln!("  error {e} on {url}; backing off");
                pacer.back_off(None);
            }
        }
    }
    None
}

fn num(v: &Value) -> Option<f64> {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let base = args.get(1).map(String::as_str).unwrap_or("https://marcurushop.com").trim_end_matches('/').to_string();
    let path = args.get(2).map(String::as_str).unwrap_or("data.db");
    let conn = db::open(path).expect("open db");
    let client = reqwest::blocking::Client::builder()
        .user_agent("marcuru-rs personal importer")
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let mut pacer = Pacer { delay: Duration::from_millis(800) };

    // Collections first, so products can be linked as they stream in.
    let cols = get(&client, &mut pacer, &format!("{base}/collections.json?limit=250"));
    let mut handles = vec![];
    if let Some(cols) = cols.as_ref().and_then(|v| v["collections"].as_array()) {
        for (i, c) in cols.iter().enumerate() {
            let h = c["handle"].as_str().unwrap_or_default();
            if h.is_empty() || c["products_count"].as_i64() == Some(0) {
                continue;
            }
            conn.execute(
                "INSERT INTO collections(handle,title,image,position) VALUES(?1,?2,?3,?4)
                 ON CONFLICT(handle) DO UPDATE SET title=?2,image=?3,position=?4",
                rusqlite::params![h, c["title"].as_str().unwrap_or(h), c["image"]["src"].as_str().unwrap_or(""), i as i64],
            )
            .unwrap();
            handles.push(h.to_string());
        }
    }
    println!("{} collections", handles.len());

    let mut n = 0;
    for page in 1.. {
        let Some(v) = get(&client, &mut pacer, &format!("{base}/products.json?limit=250&page={page}")) else { break };
        let items = v["products"].as_array().cloned().unwrap_or_default();
        if items.is_empty() {
            break;
        }
        let tx = conn.unchecked_transaction().unwrap();
        for p in &items {
            let vars = p["variants"].as_array().cloned().unwrap_or_default();
            let price = vars.iter().filter_map(|x| num(&x["price"])).fold(f64::MAX, f64::min);
            let price = if price == f64::MAX { 0.0 } else { price };
            let cmp = vars.iter().filter_map(|x| num(&x["compare_at_price"])).fold(0.0, f64::max);
            let avail = vars.iter().any(|x| x["available"].as_bool().unwrap_or(false));
            let vt = vars.first().and_then(|x| x["title"].as_str()).filter(|t| *t != "Default Title").unwrap_or("");
            let img = &p["images"][0];
            let tags = match &p["tags"] {
                Value::Array(a) => a.iter().filter_map(|t| t.as_str()).collect::<Vec<_>>().join(", "),
                Value::String(s) => s.clone(),
                _ => String::new(),
            };
            tx.execute(
                "INSERT INTO products(id,handle,title,body,tags,variant,price,compare_at,available,image,image_w,image_h,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
                 ON CONFLICT(id) DO UPDATE SET handle=?2,title=?3,body=?4,tags=?5,variant=?6,price=?7,compare_at=?8,
                   available=?9,image=?10,image_w=?11,image_h=?12,created_at=?13",
                rusqlite::params![
                    p["id"].as_i64().unwrap_or(0), p["handle"].as_str().unwrap_or(""), p["title"].as_str().unwrap_or(""),
                    p["body_html"].as_str().unwrap_or(""), tags, vt, price,
                    if cmp > price { Some(cmp) } else { None }, avail as i64,
                    img["src"].as_str().unwrap_or(""), img["width"].as_i64().unwrap_or(0), img["height"].as_i64().unwrap_or(0),
                    p["created_at"].as_str().unwrap_or("")
                ],
            )
            .unwrap();
            n += 1;
        }
        tx.commit().unwrap();
        println!("page {page}: {n} products total (delay {:?})", pacer.delay);
    }

    // Collection membership.
    for h in &handles {
        conn.execute("DELETE FROM collection_products WHERE collection=?1", [h]).unwrap();
        for page in 1.. {
            let Some(v) = get(&client, &mut pacer, &format!("{base}/collections/{h}/products.json?limit=250&page={page}")) else { break };
            let items = v["products"].as_array().cloned().unwrap_or_default();
            if items.is_empty() {
                break;
            }
            let tx = conn.unchecked_transaction().unwrap();
            for (i, p) in items.iter().enumerate() {
                tx.execute(
                    "INSERT OR IGNORE INTO collection_products(collection,product_id,position) VALUES(?1,?2,?3)",
                    rusqlite::params![h, p["id"].as_i64().unwrap_or(0), (page * 250 + i) as i64],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }
        println!("linked {h}");
    }
    println!("done: {} products in {path}", db::total(&conn).unwrap());
}
