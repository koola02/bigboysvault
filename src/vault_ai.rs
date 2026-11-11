//! Ask the Vault's AI answers. Google Gemini reads the question, looks things up through a few read-only tools (the
//! vault's cards, one card's details and price trend, look-alikes, the trainers' stores) and writes a short reply.
//! It may only quote prices and stock that a tool gave it. Cards it recommends come back as tiles, built here from the
//! real products, so a tile's price and "+" are always the site's own.
//!
//! Anything that goes wrong (no key, a timeout, Google's free quota used up, an odd reply) gives `None`, and Ask
//! answers the old way (marcuru_rs::ask), so it never breaks. Settings: GEMINI_API_KEY (a Fly secret), GEMINI_MODEL
//! (default below), GEMINI_DAILY_MAX (answers per day across the site, default 900).
use crate::{api, with_db, App};
use marcuru_rs::{blocks, db, history, sellers};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;
use vault_search::{parse, peso, read_words, search, Sort};

const DEFAULT_MODEL: &str = "gemini-3.6-flash";
const ROUNDS: usize = 4;
/// Answers one visitor may get from the AI in 15 minutes; after that, the rule-based answers.
const PER_VISITOR: u32 = 30;
static PAUSED_UNTIL: AtomicI64 = AtomicI64::new(0);

fn key() -> Option<String> {
    std::env::var("GEMINI_API_KEY").ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}

pub fn enabled() -> bool {
    key().is_some()
}

fn model() -> String {
    std::env::var("GEMINI_MODEL").ok().map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

fn daily_max() -> i64 {
    std::env::var("GEMINI_DAILY_MAX").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(900)
}

fn today(now: i64) -> String {
    // The shop's day (Philippine time), so the count starts again at midnight there.
    chrono_day(now + 8 * 3600)
}

fn chrono_day(t: i64) -> String {
    let days = t.div_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

/// The cards the model has seen in this conversation turn, numbered from 1, so it can name them as [[3]].
#[derive(Default)]
struct Refs(Vec<String>);

impl Refs {
    fn add(&mut self, handle: &str) -> usize {
        match self.0.iter().position(|h| h == handle) {
            Some(i) => i + 1,
            None => {
                self.0.push(handle.to_string());
                self.0.len()
            }
        }
    }
    fn get(&self, r: usize) -> Option<&String> {
        r.checked_sub(1).and_then(|i| self.0.get(i))
    }
}

fn tools() -> Value {
    json!([{ "functionDeclarations": [
        {
            "name": "search_cards",
            "description": "Search the vault's cards for sale. Use it for any question about which cards the vault has, prices or stock. Returns at most 8 cards, each with a ref number.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Words to search for: a Pokémon or card name, a set, a card number, and words like holo, reverse holo, 1st edition, graded, PSA, near mint, japanese. Leave empty to search everything (with a sort)." },
                    "min_price": { "type": "number", "description": "Lowest price in pesos." },
                    "max_price": { "type": "number", "description": "Highest price in pesos." },
                    "sort": { "type": "string", "enum": ["best_match", "cheapest", "priciest", "newest"] }
                }
            }
        },
        {
            "name": "card_details",
            "description": "Everything about one card from search_cards or similar_cards: price, condition, copies in stock, rarity, the market price and how it moved lately.",
            "parameters": { "type": "object", "properties": { "ref": { "type": "integer" } }, "required": ["ref"] }
        },
        {
            "name": "similar_cards",
            "description": "Cards in the vault most like one card (same type, evolution line, artist, rarity or set). Use for 'cards like X' or when a card is gone.",
            "parameters": { "type": "object", "properties": { "ref": { "type": "integer" } }, "required": ["ref"] }
        },
        {
            "name": "store_listings",
            "description": "Search the trainers' own stores on the site (other sellers, not the vault). Returns the store, the card, its price and a link.",
            "parameters": { "type": "object", "properties": { "query": { "type": "string", "description": "A card or Pokémon name." } }, "required": ["query"] }
        }
    ]}])
}

fn instructions(pay_methods: &str, pay_info: &str) -> String {
    let pay = if pay_methods.trim().is_empty() { "the ways the vault shows when it accepts the order".to_string() } else { pay_methods.replace(',', ", ") };
    let info = pay_info.trim();
    format!(
        "You are Ask the Vault, the shop assistant of Big Boys Vault, a Pokémon card shop in the Philippines (bigboysvault.com).\n\
Rules:\n\
- For anything about cards, prices, stock or stores, use the tools first. Never state a price, stock or a card the vault has unless a tool just gave it to you. If the tools find nothing, say so plainly and suggest something close that they did find.\n\
- Prices are in pesos; write them exactly as the tools give them (like ₱1,250).\n\
- When you recommend vault cards, write each card's ref in double brackets right after its name, like: Pikachu ex [[2]]. The site shows those cards as tiles under your answer with their price and an add-to-cart button. Recommend at most 6.\n\
- Store cards (from store_listings) have no ref: name the store instead.\n\
- Keep it short: one to four sentences, plain text, no markdown, no lists, no emoji.\n\
- Answer in the customer's language (English, Tagalog or Taglish).\n\
- Only talk about Pokémon cards and this shop. Don't give investment advice: you may say how a card's market price moved, nothing more.\n\
- You can't see orders, accounts or messages, and you can't change anything. For shipping, meetups, returns, orders or anything else you can't look up, tell them to message the vault in Chat.\n\
How buying works: add vault cards to the cart and send the order; nothing is paid on the site. The vault accepts the order and shows how to pay: {pay}.{} Trainers' stores work the same way, paying that seller directly.",
        if info.is_empty() { String::new() } else { format!(" The vault's note on paying: {info}") }
    )
}

/// The answer, in the same shape as the rule-based one, or None to fall back.
pub async fn answer(app: &App, ip: &str, question: &str, prev: Option<&str>) -> Option<Value> {
    let key = key()?;
    let q = question.trim();
    if q.is_empty() {
        return None;
    }
    let now = crate::unix_now();
    if PAUSED_UNTIL.load(Ordering::Relaxed) > now {
        return None;
    }
    let lim = format!("ai|{ip}");
    let n = app.limits.get(&lim).unwrap_or(0) + 1;
    app.limits.insert(lim, n);
    if n > PER_VISITOR {
        return None;
    }
    // One more answer today, if the site hasn't reached its daily limit.
    let day = today(now);
    let max = daily_max();
    let d2 = day.clone();
    let (allowed, pay) = with_db(app, move |c| {
        let used: i64 = blocks::setting(c, "ai_day")?.and_then(|v| v.strip_prefix(&format!("{d2}|")).and_then(|n| n.parse().ok())).unwrap_or(0);
        if used >= max {
            return Ok((false, (String::new(), String::new())));
        }
        blocks::set_setting(c, "ai_day", &format!("{d2}|{}", used + 1))?;
        Ok((true, sellers::vault_pay(c)?))
    })
    .await
    .ok()?;
    if !allowed {
        return None;
    }
    match run(app, &key, q, prev, &pay).await {
        Ok(v) => {
            with_db(app, move |c| blocks::set_setting(c, "ai_last_ok", &now.to_string())).await.ok();
            Some(v)
        }
        Err(e) => {
            // Too many at once: a minute. The key, the project's billing or its access is the problem: ten minutes,
            // so visitors don't wait on Google for every question until someone fixes it.
            if e.starts_with("429") {
                PAUSED_UNTIL.store(now + 60, Ordering::Relaxed);
            } else if ["400 API key", "401", "402", "403"].iter().any(|p| e.starts_with(p)) || (e.starts_with("400") && e.contains("API key")) {
                PAUSED_UNTIL.store(now + 600, Ordering::Relaxed);
            }
            tracing::warn!("Ask the Vault AI: {e}");
            let note = format!("{now}|{}", e.chars().take(300).collect::<String>());
            with_db(app, move |c| blocks::set_setting(c, "ai_last_error", &note)).await.ok();
            None
        }
    }
}

async fn run(app: &App, key: &str, q: &str, prev: Option<&str>, pay: &(String, String)) -> Result<Value, String> {
    let model = model();
    let text = match prev.map(str::trim).filter(|p| !p.is_empty() && *p != q) {
        Some(p) => format!("(My previous question was: {p})\n{q}"),
        None => q.to_string(),
    };
    let mut contents: Vec<Value> = vec![json!({ "role": "user", "parts": [{ "text": text }] })];
    let mut refs = Refs::default();
    let mut thinking = true;
    for _ in 0..ROUNDS {
        let mut body = json!({
            "systemInstruction": { "parts": [{ "text": instructions(&pay.0, &pay.1) }] },
            "contents": contents,
            "tools": tools(),
            "generationConfig": { "maxOutputTokens": 1024 }
        });
        if thinking {
            body["generationConfig"]["thinkingConfig"] = json!({ "thinkingLevel": "LOW" });
        }
        let resp = match call(app, key, &model, &body).await {
            Err(e) if thinking && e.starts_with("400") && e.to_lowercase().contains("think") => {
                // A model without thinking levels: ask again without them.
                thinking = false;
                body["generationConfig"].as_object_mut().map(|g| g.remove("thinkingConfig"));
                call(app, key, &model, &body).await?
            }
            r => r?,
        };
        let content = resp["candidates"][0]["content"].clone();
        let parts = content["parts"].as_array().cloned().unwrap_or_default();
        let calls: Vec<&Value> = parts.iter().filter(|p| p.get("functionCall").is_some()).collect();
        if calls.is_empty() {
            let said: String = parts.iter().filter(|p| p["thought"].as_bool() != Some(true)).filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(" ");
            if said.trim().is_empty() {
                return Err(format!("no answer (finish: {})", resp["candidates"][0]["finishReason"].as_str().unwrap_or("?")));
            }
            return finish(app, &said, &refs, q).await;
        }
        // The model's turn goes back exactly as it came (its thought signatures included), then the tools' results.
        contents.push(content);
        let mut results = vec![];
        for p in calls {
            let f = &p["functionCall"];
            let name = f["name"].as_str().unwrap_or("").to_string();
            let out = tool(app, &name, &f["args"], &mut refs).await.unwrap_or_else(|e| json!({ "error": e }));
            let mut fr = json!({ "name": name, "response": out });
            if let Some(id) = f.get("id") {
                fr["id"] = id.clone();
            }
            results.push(json!({ "functionResponse": fr }));
        }
        contents.push(json!({ "role": "user", "parts": results }));
    }
    Err("too many lookups".into())
}

async fn call(app: &App, key: &str, model: &str, body: &Value) -> Result<Value, String> {
    // GEMINI_URL is only for trying this against a stand-in server.
    let base = std::env::var("GEMINI_URL").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| "https://generativelanguage.googleapis.com".into());
    let url = format!("{base}/v1beta/models/{model}:generateContent");
    let r = app
        .http
        .post(url)
        .header("x-goog-api-key", key)
        .timeout(Duration::from_secs(15))
        .json(body)
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?;
    let status = r.status();
    let text = r.text().await.map_err(|e| format!("network: {e}"))?;
    if !status.is_success() {
        return Err(format!("{} {}", status.as_u16(), text.chars().take(400).collect::<String>()));
    }
    serde_json::from_str(&text).map_err(|e| format!("odd reply: {e}"))
}

fn num(v: &Value) -> Option<usize> {
    v.as_u64().map(|n| n as usize).or_else(|| v.as_f64().map(|n| n as usize)).or_else(|| v.as_str().and_then(|s| s.trim().trim_matches(['[', ']']).parse().ok()))
}

async fn tool(app: &App, name: &str, args: &Value, refs: &mut Refs) -> Result<Value, String> {
    let fail = |e: (axum::http::StatusCode, String)| e.1;
    match name {
        "search_cards" => {
            let body = api::catalogue(app).await.map_err(fail)?;
            let query: String = args["query"].as_str().unwrap_or("").chars().take(120).collect();
            let sort = match args["sort"].as_str().unwrap_or("") {
                "cheapest" => Sort::PriceAsc,
                "priciest" => Sort::PriceDesc,
                "newest" => Sort::Newest,
                _ => Sort::Newest,
            };
            let (min, max) = (args["min_price"].as_f64(), args["max_price"].as_f64());
            let rows = tokio::task::spawn_blocking(move || {
                let (_, cards) = parse(&body);
                let mut f = read_words(&query);
                f.min_price = f.min_price.or(min);
                f.max_price = f.max_price.or(max);
                let found = search(&cards, &f, sort);
                let hits: Vec<usize> = found.hits.into_iter().filter(|&i| cards[i].qty > 0).collect();
                let total = hits.len();
                let rows: Vec<(String, Value)> = hits
                    .iter()
                    .take(8)
                    .map(|&i| {
                        let c = &cards[i];
                        (c.handle.clone(), json!({ "title": c.title, "set": c.set, "number": c.no, "condition": c.variant, "grade": c.grade, "finish": c.finish, "price": peso(c.price), "copies": c.qty }))
                    })
                    .collect();
                (total, found.typo, rows)
            })
            .await
            .map_err(|e| e.to_string())?;
            let (total, typo, rows) = rows;
            let cards: Vec<Value> = rows
                .into_iter()
                .map(|(h, mut v)| {
                    v["ref"] = json!(refs.add(&h));
                    v
                })
                .collect();
            let mut out = json!({ "found": total, "cards": cards });
            if typo {
                out["note"] = json!("Nothing matched exactly; these are the closest spellings.");
            }
            Ok(out)
        }
        "card_details" | "similar_cards" => {
            let r = num(&args["ref"]).ok_or("ref is missing")?;
            let handle = refs.get(r).cloned().ok_or("unknown ref: use one from search_cards")?;
            if name == "card_details" {
                let p = with_db(app, move |c| {
                    let Some(p) = db::product(c, &handle)? else { return Ok(None) };
                    let t = history::trend(c, &p.api_id, &p.finish)?;
                    Ok(Some((p, t)))
                })
                .await
                .map_err(fail)?;
                let Some((p, t)) = p else { return Err("that card is gone".into()) };
                let mut out = json!({
                    "title": p.title, "set": p.set_text(), "number": p.card_no, "rarity": p.rarity, "condition": p.variant,
                    "finish": p.finish, "price": peso(p.price), "for_sale": p.available && p.price > 0.0, "copies": p.qty,
                });
                if p.market_php > 0.0 {
                    out["market_price_near_mint"] = json!(peso(p.market_php));
                }
                if let Some(t) = t {
                    out["price_trend"] = json!(t.text());
                }
                let about: String = p.body.chars().take(300).collect();
                if !about.trim().is_empty() {
                    out["about"] = json!(about);
                }
                Ok(out)
            } else {
                let rel = with_db(app, move |c| match db::product(c, &handle)? {
                    Some(p) => db::related(c, &p, 6),
                    None => Ok(vec![]),
                })
                .await
                .map_err(fail)?;
                let cards: Vec<Value> = rel
                    .iter()
                    .map(|p| json!({ "ref": refs.add(&p.handle), "title": p.title, "set": p.set_text(), "condition": p.variant, "price": peso(p.price) }))
                    .collect();
                Ok(json!({ "cards": cards }))
            }
        }
        "store_listings" => {
            let query: String = args["query"].as_str().unwrap_or("").chars().take(80).collect();
            let words: Vec<String> = query.split_whitespace().take(5).map(|w| w.to_lowercase().replace(['%', '_'], "")).filter(|w| !w.is_empty()).collect();
            if words.is_empty() {
                return Err("say which card".into());
            }
            let rows = with_db(app, move |c| {
                let mut sql = String::from(
                    "SELECT CASE WHEN m.store_name<>'' THEN m.store_name ELSE m.name END, c.title, c.cond, c.value, c.member_id, c.id
                     FROM member_cards c JOIN members m ON m.id=c.member_id
                     WHERE c.kind='sale' AND c.status='Active' AND c.qty>0
                       AND m.seller=1 AND m.blocked=0 AND m.profile_ok=1 AND m.status<>'Vacation'",
                );
                for i in 0..words.len() {
                    sql.push_str(&format!(" AND lower(c.title) LIKE '%' || ?{} || '%'", i + 1));
                }
                sql.push_str(" ORDER BY c.value LIMIT 8");
                let mut st = c.prepare(&sql)?;
                let rows = st
                    .query_map(rusqlite::params_from_iter(words.iter()), |r| {
                        let price: f64 = r.get(3)?;
                        Ok(json!({ "store": r.get::<_, String>(0)?, "title": r.get::<_, String>(1)?, "condition": r.get::<_, String>(2)?, "price": if price > 0.0 { peso(price) } else { "ask the seller".into() }, "link": format!("/trainers/{}/cards/{}", r.get::<_, i64>(4)?, r.get::<_, i64>(5)?) }))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .map_err(fail)?;
            Ok(json!({ "listings": rows }))
        }
        _ => Err(format!("no tool called {name}")),
    }
}

/// The reply as plain text with the [[ref]] marks taken out, and the cards they named, in order.
pub fn tidy(said: &str) -> (String, Vec<usize>) {
    let mut out = String::new();
    let mut picks: Vec<usize> = vec![];
    let mut rest = said;
    while let Some(a) = rest.find("[[") {
        let Some(b) = rest[a..].find("]]") else { break };
        out.push_str(&rest[..a]);
        for n in rest[a + 2..a + b].split(',') {
            if let Ok(n) = n.trim().parse::<usize>() {
                if !picks.contains(&n) {
                    picks.push(n);
                }
            }
        }
        rest = &rest[a + b + 2..];
    }
    out.push_str(rest);
    // Plain text: the reply is shown as typed, so markdown marks would show as they are.
    let out = out.replace("**", "").replace("__", "").replace('`', "");
    let lines: Vec<String> = out.lines().map(|l| l.trim().trim_start_matches(['*', '-', '•']).trim().to_string()).filter(|l| !l.is_empty()).collect();
    let mut text = lines.join(" ");
    while text.contains("  ") {
        text = text.replace("  ", " ");
    }
    let text = text.replace(" ,", ",").replace(" .", ".").replace(" !", "!").replace(" ?", "?");
    (text.trim().to_string(), picks)
}

async fn finish(app: &App, said: &str, refs: &Refs, q: &str) -> Result<Value, String> {
    let (text, picks) = tidy(said);
    if text.is_empty() {
        return Err("empty answer".into());
    }
    let handles: Vec<String> = picks.iter().filter_map(|r| refs.get(*r).cloned()).take(8).collect();
    let products = with_db(app, move |c| handles.iter().map(|h| db::product(c, h)).collect::<rusqlite::Result<Vec<_>>>())
        .await
        .map_err(|e| e.1)?;
    let cards: Vec<Value> = products.into_iter().flatten().map(|p| api::product_tile(&p)).collect();
    let ctx: String = q.chars().take(190).collect();
    Ok(json!({ "say": text, "cards": cards, "chips": [], "ctx": ctx, "ai": true }))
}

/// For the owner: is the AI on, how much was used today, the last problem.
pub async fn status(app: &App) -> Value {
    let day = today(crate::unix_now());
    let s = with_db(app, move |c| {
        Ok((blocks::setting(c, "ai_day")?, blocks::setting(c, "ai_last_ok")?, blocks::setting(c, "ai_last_error")?))
    })
    .await
    .unwrap_or((None, None, None));
    let used: i64 = s.0.and_then(|v| v.strip_prefix(&format!("{day}|")).and_then(|n| n.parse().ok())).unwrap_or(0);
    json!({ "on": enabled(), "model": model(), "today": used, "daily_max": daily_max(), "last_ok": s.1, "last_error": s.2, "paused_until": PAUSED_UNTIL.load(Ordering::Relaxed) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_become_tiles_and_text_stays_plain() {
        let (t, p) = tidy("**Yes!** We have Pikachu ex [[2]] for ₱1,250 and Raichu [[3]].\n- Also Pichu [[2]].");
        assert_eq!(t, "Yes! We have Pikachu ex for ₱1,250 and Raichu. Also Pichu.");
        assert_eq!(p, vec![2, 3]);
        assert_eq!(tidy("Try [[1, 4]]").1, vec![1, 4]);
        assert_eq!(tidy("No marks here").1, Vec::<usize>::new());
    }

    #[test]
    fn days_are_philippine_dates() {
        assert_eq!(chrono_day(0), "1970-01-01");
        // 2026-11-11 16:00 UTC is already the 12th in Manila.
        assert_eq!(today(1_794_355_200 + 16 * 3600), "2026-11-12");
        assert_eq!(chrono_day(1_794_355_200), "2026-11-11");
    }
}
