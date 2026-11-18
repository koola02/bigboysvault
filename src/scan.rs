//! Scan a card, for sellers (the owner, 18 Nov 2026: "scanning for sellers only and it should be separate first since
//! it's not perfect yet. put it in the menu like scan a card, don't replace the add a card with it"). It is its own page,
//! /me/scan, marked "Try it" in the menu; Add cards in the Store Manager is untouched.
//!
//! The seller photographs a card with the card camera (static/card-cam.js, `data-card-cam="scan"`). Claude reads it:
//! the photo goes to Anthropic's Messages API (raw HTTP: there is no Rust SDK), to Claude Sonnet 5.5 at high effort (the
//! owner's choices), which answers in a fixed JSON shape (structured outputs): the card's name, its Pokémon, its collector
//! number, its set (a best guess) and its language. The site then looks the card up in the card list (TCGdex,
//! `ops::catalog_cached`): by its name, and by its name and number (so a popular Pokémon's old printing isn't lost past
//! the first page). It shows (static/scan.js):
//! - the card itself, one tile per version the market prices (Normal, Holo, Reverse Holo...); when the number couldn't
//!   be read, every printing, with the best guess (the set read) first and marked;
//! - the same card in other sets and printings;
//! - its evolution family (Lombre and Ludicolo for a Lotad), the same set first. The family comes from PokeAPI's
//!   species table (`market::families`), not from the model, which can mix families up.
//!
//! Everything comes from the full online card list Add cards searches, never only the vault's cards. A tile opens the
//! usual "Add to your store" window (static/add-card.js), with that version chosen: the seller still picks the
//! condition, types the price and says how many. Nothing is added or priced by itself, and the photo is not kept.
//!
//! Settings: ANTHROPIC_API_KEY (a Fly secret; without it the page says scanning isn't on yet), SCAN_MODEL (default
//! below), SCAN_EFFORT (how hard the model thinks: low, medium, high, xhigh or max; default high, the owner's choice on
//! 18 Nov 2026), SCAN_PER_SELLER (scans a day for each seller, default 40), SCAN_DAILY_MAX (a fixed total a day for the
//! whole site; unset, the total follows the number of sellers: `site_max`), ANTHROPIC_WORKSPACE_ID (the workspace to bill, needed when the key is an all-workspaces key; not secret),
//! ANTHROPIC_BASE_URL (a stand-in server, only for trying this out). The owner sees how it is going at
//! /admin/scan/status, and can try a photo at POST /admin/scan/try.
use crate::community::{acting, check_size, current, fail, html, ok, over_limit, render};
use crate::{chrome, with_db, App, Chrome, Res};
use askama::Template;
use axum::{
    extract::{Multipart, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
};
use base64::Engine;
use marcuru_rs::{blocks, market};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

const DEFAULT_MODEL: &str = "claude-sonnet-5-5";
/// The owner asked for Sonnet 5.5 at high effort (18 Nov 2026): it thinks before it answers.
const DEFAULT_EFFORT: &str = "high";
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
/// The Messages API version this is written for. It accepts no sampling settings, and none are sent.
const API_VERSION: &str = "2026-10-21";
/// Server-side fallback (`fallbacks: "default"`): if the model declines a photo, Anthropic retries it on another model
/// inside the same call. Only sent with the default model.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// The biggest photo taken. The card camera sends about 1,150 × 1,600 pixels (a few hundred KB).
const MAX_BYTES: usize = 5 * 1024 * 1024;
/// Scans one seller may start in 15 minutes, whatever is left of their day: a stuck button or a script can't use it up.
const BURST: u32 = 20;
/// Cards shown for each Pokémon of the evolution family; the rest are a search away in Add cards.
const FAMILY_SHOWN: usize = 24;
/// Pokémon of the evolution family looked up (Eevee's has nine).
const FAMILY_MAX: usize = 9;
/// Printings of the card itself shown as "this card" (a 1st Edition and an Unlimited, say).
const EXACT_MAX: usize = 4;
/// What a reply can say is wrong with the photo ("none": the card was read).
const PROBLEMS: [&str; 5] = ["none", "not_a_card", "card_back", "blurry", "several_cards"];
const LANGUAGES: [&str; 6] = ["en", "ja", "zh-tw", "zh-cn", "ko", "other"];
static PAUSED_UNTIL: AtomicI64 = AtomicI64::new(0);

fn key() -> Option<String> {
    std::env::var("ANTHROPIC_API_KEY").ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}

pub fn enabled() -> bool {
    key().is_some()
}

/// The workspace a key that isn't tied to one must name on every request ("wrkspc_..."), from ANTHROPIC_WORKSPACE_ID.
fn workspace() -> Option<String> {
    std::env::var("ANTHROPIC_WORKSPACE_ID").ok().map(|w| w.trim().to_string()).filter(|w| !w.is_empty())
}

fn model() -> String {
    std::env::var("SCAN_MODEL").ok().map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

fn effort() -> String {
    std::env::var("SCAN_EFFORT").ok().map(|e| e.trim().to_lowercase()).filter(|e| EFFORTS.contains(&e.as_str())).unwrap_or_else(|| DEFAULT_EFFORT.to_string())
}

fn env_num(name: &str, default: i64) -> i64 {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok()).filter(|n: &i64| *n >= 0).unwrap_or(default)
}

/// The site's scans a day: SCAN_DAILY_MAX when it is set; otherwise each seller's share for every seller (not blocked),
/// plus one share for the vault's own scans (the owner's tries at /admin/scan/try). It grows by itself as sellers join, so
/// every seller can always use their whole day's share (the owner, 18 Nov 2026: "adjust limits accordingly, right now we
/// just 10 sellers"), and the most a day can cost stays in step with the number of sellers.
fn site_max(c: &rusqlite::Connection, per: i64) -> rusqlite::Result<i64> {
    let fixed = std::env::var("SCAN_DAILY_MAX").ok().and_then(|v| v.trim().parse::<i64>().ok()).filter(|n| *n >= 0);
    site_max_with(c, per, fixed)
}

fn site_max_with(c: &rusqlite::Connection, per: i64, fixed: Option<i64>) -> rusqlite::Result<i64> {
    if let Some(n) = fixed {
        return Ok(n);
    }
    let sellers: i64 = c.query_row("SELECT COUNT(*) FROM members WHERE seller=1 AND blocked=0", [], |r| r.get(0))?;
    Ok(per * (sellers + 1))
}

/// Scans a day for each seller: 40, the owner's choice for a month of 10 sellers (18 Nov 2026: "lets do 40").
fn per_seller() -> i64 {
    env_num("SCAN_PER_SELLER", 40)
}

// ---------- how many scans are left today ----------

/// The settings key that counts one seller's scans today (0 is the owner trying it from /admin/scan/try).
fn seller_key(id: i64) -> String {
    format!("scan_day_m{id}")
}

/// How many scans this key counted today. Kept as "{day}|{n}", so another day's count reads as none.
fn used(c: &rusqlite::Connection, key: &str, day: &str) -> rusqlite::Result<i64> {
    Ok(blocks::setting(c, key)?.and_then(|v| v.strip_prefix(&format!("{day}|")).and_then(|n| n.parse().ok())).unwrap_or(0))
}

/// Scans left today: (for this seller, for the whole site).
fn left(c: &rusqlite::Connection, id: i64, day: &str, per: i64, max: i64) -> rusqlite::Result<(i64, i64)> {
    Ok(((per - used(c, &seller_key(id), day)?).max(0), (max - used(c, "scan_day", day)?).max(0)))
}

/// One more scan today, for the seller and for the site.
fn count(c: &rusqlite::Connection, id: i64, day: &str) -> rusqlite::Result<()> {
    for k in ["scan_day".to_string(), seller_key(id)] {
        let n = used(c, &k, day)? + 1;
        blocks::set_setting(c, &k, &format!("{day}|{n}"))?;
    }
    Ok(())
}

// ---------- what Claude is asked, and what it answers ----------

const SYSTEM: &str = "You read photos of Pokémon Trading Card Game cards for sellers on a card marketplace, so each card can \
be found in the card list. The photo should show the front of one card, often in a sleeve or a toploader, sometimes with a \
price sticker, a finger or a busy background: ignore all of that and read only what is printed on the card.

Fill in every field:
- problem: \"none\" when the front of one Pokémon TCG card can be read; \"card_back\" when it is the back of a card; \
\"several_cards\" when more than one card front shows and none is clearly the main one; \"blurry\" when the card is there \
but its name can't be read; \"not_a_card\" for anything else. When it isn't \"none\", fill the other fields as far as you can.
- name: the card's name as printed at the top, with any suffix, in the card's own language and script (for example \
\"Lotad\", \"Charizard ex\", \"Pikachu VMAX\", \"Professor's Research\", \"Basic Fire Energy\").
- name_en: the same name in English (the same as name for an English card).
- species: the English species name of the Pokémon the card shows, without Mega, ex, V, GX, regional or owner words \
(\"Charizard\" for \"Charizard ex\", \"Mega Charizard X ex\" or \"Blaine's Charizard\"; \"Meowth\" for \"Galarian Meowth\"). \
Empty for Trainer and Energy cards.
- number: the collector number printed near the bottom, exactly as printed (for example \"92/132\", \"025/165\", \
\"TG05/TG30\", \"SV049/SV094\", \"SWSH045\"). Empty if you can't read it clearly: never guess a digit.
- set_name: your best guess of the card's set, in English, from the set symbol, the number, the copyright year, the \
card's style and what you know of the card (for example \"Secret Wonders\"). Empty only when you have no idea.
- language: the language the card is printed in.
- sure: \"high\" when you read both the name and the number clearly, \"medium\" when one of them is unclear, \"low\" otherwise.";

/// The answer's fixed shape (structured outputs: every field required, nothing else allowed).
fn schema() -> Value {
    let text = |d: &str| json!({ "type": "string", "description": d });
    json!({
        "type": "object",
        "properties": {
            "problem": { "type": "string", "enum": PROBLEMS },
            "name": text("The card's name as printed, in its own language."),
            "name_en": text("The card's name in English."),
            "species": text("The English species name of the Pokémon on the card; empty for Trainer and Energy cards."),
            "number": text("The collector number as printed, or empty."),
            "set_name": text("Your best guess of the set's English name, or empty."),
            "language": { "type": "string", "enum": LANGUAGES },
            "sure": { "type": "string", "enum": ["high", "medium", "low"] }
        },
        "required": ["problem", "name", "name_en", "species", "number", "set_name", "language", "sure"],
        "additionalProperties": false
    })
}

/// The Messages API request for one photo: adaptive thinking at the effort chosen (`SCAN_EFFORT`), the JSON shape above,
/// and no sampling settings (API version 2026-10-21 rejects them). Thinking counts towards `max_tokens`, so it has room.
fn body(model: &str, effort: &str, media_type: &str, photo_b64: &str) -> Value {
    let mut b = json!({
        "model": model,
        "max_tokens": 16000,
        "system": SYSTEM,
        "output_config": { "effort": effort, "format": { "type": "json_schema", "schema": schema() } },
        "messages": [{
            "role": "user",
            "content": [
                { "type": "image", "source": { "type": "base64", "media_type": media_type, "data": photo_b64 } },
                { "type": "text", "text": "Read this card." }
            ]
        }]
    });
    if model == DEFAULT_MODEL {
        b["fallbacks"] = json!("default");
    }
    b
}

/// The photo's kind, from its first bytes: only what Claude reads (JPEG, PNG, WebP).
fn media_type(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

async fn call(app: &App, key: &str, body: &Value) -> Result<Value, String> {
    // ANTHROPIC_BASE_URL is only for trying this against a stand-in server.
    let base = std::env::var("ANTHROPIC_BASE_URL").ok().map(|u| u.trim().trim_end_matches('/').to_string()).filter(|u| !u.is_empty()).unwrap_or_else(|| "https://api.anthropic.com".into());
    let mut req = app
        .http
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", key)
        .header("anthropic-version", API_VERSION)
        .timeout(Duration::from_secs(90))
        .json(body);
    if body.get("fallbacks").is_some() {
        req = req.header("anthropic-beta", FALLBACK_BETA);
    }
    if let Some(w) = workspace() {
        req = req.header("anthropic-workspace-id", w);
    }
    let r = req.send().await.map_err(|e| format!("network: {e}"))?;
    let status = r.status();
    let text = r.text().await.map_err(|e| format!("network: {e}"))?;
    if !status.is_success() {
        return Err(format!("{} {}", status.as_u16(), text.chars().take(400).collect::<String>()));
    }
    serde_json::from_str(&text).map_err(|e| format!("odd reply: {e}"))
}

/// What Claude read on the card (the shape `schema` asks for).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Reading {
    pub problem: String,
    pub name: String,
    pub name_en: String,
    pub species: String,
    pub number: String,
    pub set_name: String,
    pub language: String,
    pub sure: String,
}

/// One line of text: single spaces, straight apostrophes, at most `n` letters.
fn line(s: &str, n: usize) -> String {
    s.replace('’', "'").split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect()
}

impl Reading {
    /// Trimmed and kept short, with only the words the shape allows (anything else becomes the careful choice).
    fn tidy(mut self) -> Self {
        self.name = line(&self.name, 60);
        self.name_en = line(&self.name_en, 60);
        self.species = line(&self.species, 40);
        self.number = line(&self.number, 20);
        self.set_name = line(&self.set_name, 80);
        if !PROBLEMS.contains(&self.problem.as_str()) {
            self.problem = "none".into();
        }
        if !LANGUAGES.contains(&self.language.as_str()) {
            self.language = "other".into();
        }
        if !["high", "medium", "low"].contains(&self.sure.as_str()) {
            self.sure = "low".into();
        }
        if self.name_en.is_empty() && self.language == "en" {
            self.name_en = self.name.clone();
        }
        if self.problem == "none" && self.query().is_empty() {
            self.problem = "blurry".into();
        }
        self
    }

    /// The card list to look in: the card's language's, else the English one (Korean cards aren't in it).
    fn lang(&self) -> &'static str {
        match self.language.as_str() {
            "ja" => "ja",
            "zh-tw" | "zh-cn" => "zh",
            _ => "en",
        }
    }

    /// What to look up. An English card by its printed name ("Charizard ex"); another language's in the English card
    /// list by its English name; a Japanese or Chinese card by its Pokémon, which that card list translates
    /// ("Charizard" finds リザードン and リザードンex), or by its English name when it is a Trainer or an Energy.
    fn query(&self) -> String {
        let pick = if self.language == "en" && !self.name.is_empty() {
            &self.name
        } else if self.lang() != "en" && !self.species.is_empty() {
            &self.species
        } else {
            &self.name_en
        };
        line(pick, 60)
    }
}

/// The reading in a Messages API reply. A decline, a reply cut short or one that isn't the JSON asked for is an error.
/// Blocks are read by their type: thinking blocks (and a fallback's) come before the answer's text.
fn read_reply(v: &Value) -> Result<Reading, String> {
    match v["stop_reason"].as_str().unwrap_or("") {
        "refusal" => return Err(format!("refusal ({})", v["stop_details"]["category"].as_str().unwrap_or("no category"))),
        "max_tokens" => return Err("cut off".into()),
        _ => {}
    }
    let text = v["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .filter(|t| !t.trim().is_empty())
        .last()
        .ok_or_else(|| "no answer".to_string())?;
    serde_json::from_str::<Reading>(text.trim()).map(Reading::tidy).map_err(|e| format!("odd answer: {e}"))
}

// ---------- finding the card in the card list ----------

/// Names and set names as compared: lower case, "é" as "e", only letters, digits and apostrophes, single spaces.
fn plain(s: &str) -> String {
    let s: String = s.to_lowercase().replace('é', "e").replace('’', "'").chars().map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' }).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A collector number's part before the slash, as (its letters, its number): "092/132" -> ("", 92), "TG05/TG30" ->
/// ("tg", 5), "SWSH045" -> ("swsh", 45), "177a" -> ("|a", 177). None when it has no digits.
fn number_key(s: &str) -> Option<(String, u64)> {
    let head: String = s.split('/').next().unwrap_or("").chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
    let at = head.find(|c: char| c.is_ascii_digit())?;
    let (letters, rest) = head.split_at(at);
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let tail: String = rest.chars().skip_while(char::is_ascii_digit).collect();
    let letters = letters.trim_end_matches('-');
    Some((if tail.is_empty() { letters.to_string() } else { format!("{letters}|{tail}") }, digits.parse().ok()?))
}

/// The set size printed after the slash: "92/132" -> 132. None for "TG05/TG30", or with no slash.
fn printed_total(s: &str) -> Option<u64> {
    s.split_once('/')?.1.trim().parse().ok().filter(|t| *t > 0)
}

fn text<'a>(c: &'a Value, k: &str) -> &'a str {
    c[k].as_str().unwrap_or("")
}

/// The card list's card has the name read (in its own language, or its English name).
fn same_name(c: &Value, r: &Reading) -> bool {
    let (name, en) = (plain(text(c, "name")), plain(text(c, "en")));
    [plain(&r.name), plain(&r.name_en)].iter().any(|w| !w.is_empty() && (*w == name || *w == en))
}

/// How well a card from the card list fits what was read. None when its number isn't the one printed on the card.
fn fit(c: &Value, r: &Reading) -> Option<i32> {
    let want = number_key(&r.number)?;
    if number_key(text(c, "number"))? != want {
        return None;
    }
    let mut s = 0;
    match (printed_total(&r.number), c["total"].as_u64().filter(|t| *t > 0)) {
        (Some(a), Some(b)) if a == b => s += 2,
        (Some(_), Some(_)) => s -= 2,
        _ => {}
    }
    let (want_set, set) = (plain(&r.set_name), plain(text(c, "set")));
    if !want_set.is_empty() && !set.is_empty() {
        if want_set == set {
            s += 3;
        } else if want_set.contains(&set) || set.contains(&want_set) {
            s += 1;
        }
    }
    if same_name(c, r) {
        s += 1;
    }
    Some(s)
}

/// The card itself (the best fits, when they fit well: several when the card list has more than one printing with that
/// number, a 1st Edition and an Unlimited say), and the rest in a useful order: the same number first (a near miss),
/// then the same name, then the set read; the card list's order (newest set first) inside each. `by_number` is the
/// search for the name with the number (it finds an old printing a popular name's first page leaves out); `by_name`,
/// every printing of the name. Each card comes once.
fn split(by_number: Vec<Value>, by_name: Vec<Value>, r: &Reading) -> (Vec<Value>, Vec<Value>) {
    let mut seen = HashSet::new();
    let cards: Vec<Value> = by_number.into_iter().chain(by_name).filter(|c| seen.insert(text(c, "id").to_string())).collect();
    let fits: Vec<Option<i32>> = cards.iter().map(|c| fit(c, r)).collect();
    let best = fits.iter().flatten().max().copied().filter(|b| *b >= 0);
    let (mut exact, mut rest) = (vec![], vec![]);
    for (c, f) in cards.into_iter().zip(fits) {
        if best.is_some() && f == best && exact.len() < EXACT_MAX {
            exact.push(c);
        } else {
            rest.push(c);
        }
    }
    let set = exact.first().map(|c| plain(text(c, "set"))).filter(|s| !s.is_empty()).unwrap_or_else(|| plain(&r.set_name));
    let in_set = |c: &Value| !set.is_empty() && { let s = plain(text(c, "set")); !s.is_empty() && (s == set || set.contains(&s) || s.contains(&set)) };
    rest.sort_by_key(|c| (fit(c, r).is_none(), !same_name(c, r), !in_set(c)));
    // With no card sure, the first is the best guess when it has the number read, or the name and the set read.
    if exact.is_empty() {
        if let Some(first) = rest.first_mut() {
            if fit(first, r).is_some() || (same_name(first, r) && in_set(first)) {
                first["guess"] = json!(true);
            }
        }
    }
    (exact, rest)
}

/// The Pokémon of the evolution family to show, in order: all but the one whose cards are already shown.
fn family_names(family: &[String], query: &str) -> Vec<String> {
    let shown = plain(query);
    family.iter().filter(|f| plain(f) != shown).take(FAMILY_MAX).cloned().collect()
}

/// The number as the card list searches it, with the name ("Pikachu 79"): the part before the slash, when it has a digit
/// and only letters and digits ("79", "TG05", "SWSH045").
fn number_head(number: &str) -> Option<String> {
    let head = number.split('/').next().unwrap_or("").trim();
    (head.chars().any(|c| c.is_ascii_digit()) && head.chars().all(|c| c.is_ascii_alphanumeric())).then(|| head.to_string())
}

/// A card's versions with today's market price in pesos (Near Mint), from the card list's prices. A "market" price (one
/// price for the card, from Cardmarket) names no version, so it gives none.
fn finishes(price: &Value) -> Value {
    let list = price["matches"][0]["finishes"].as_array().cloned().unwrap_or_default();
    Value::Array(
        list.iter()
            .filter_map(|f| {
                let key = f["key"].as_str().filter(|k| *k != "market")?.to_string();
                let label = f["label"].as_str().map(str::to_string).unwrap_or_else(|| market::label(&key));
                Some(json!({ "key": key, "label": label, "php": f["php"].as_f64().unwrap_or(0.0) }))
            })
            .collect(),
    )
}

/// The card list's answer for what was read: the card itself (with its versions), its other printings, and its
/// evolution family. Every search runs at once; each is kept for a day, each card's prices for an hour.
async fn look_up(app: &App, r: &Reading) -> Value {
    let (lang, query) = (r.lang(), r.query());
    // The family by the Pokémon read; if that was left empty, by the card's English name ("Lotad"; a Trainer's finds none).
    let families = market::families(&app.http).await;
    let species = if r.species.is_empty() { &r.name_en } else { &r.species };
    let fam = family_names(&market::family_of(&families, species), &query);
    // Searches: [the name, the name with the number (or none), each Pokémon of the family].
    let by_number = number_head(&r.number).map(|n| format!("{query} {n}"));
    let mut searches = tokio::task::JoinSet::new();
    let qs: Vec<Option<String>> = [Some(query.clone()), by_number].into_iter().chain(fam.iter().cloned().map(Some)).collect();
    for (i, q) in qs.iter().enumerate() {
        let Some(q) = q.clone() else { continue };
        let app = app.clone();
        searches.spawn(async move { (i, crate::ops::catalog_cached(&app, &q, lang).await) });
    }
    let mut found = vec![Value::Null; qs.len()];
    while let Some(done) = searches.join_next().await {
        if let Ok((i, v)) = done {
            found[i] = v;
        }
    }
    let cards = |v: &Value| v["cards"].as_array().cloned().unwrap_or_default();
    let (mut exact, versions) = split(cards(&found[1]), cards(&found[0]), r);
    let mut prices = tokio::task::JoinSet::new();
    for (i, c) in exact.iter().enumerate() {
        let (app, id) = (app.clone(), text(c, "id").to_string());
        prices.spawn(async move { (i, crate::admin::price_for_id(&app, &id).await) });
    }
    while let Some(done) = prices.join_next().await {
        if let Ok((i, p)) = done {
            exact[i]["finishes"] = finishes(&p);
        }
    }
    // The family: each Pokémon's cards not shown already, the card's own set first, then that Pokémon's plain name.
    let set = exact.first().map(|c| plain(text(c, "set"))).filter(|s| !s.is_empty()).unwrap_or_else(|| plain(&r.set_name));
    let mut shown: HashSet<String> = exact.iter().chain(versions.iter()).map(|c| text(c, "id").to_string()).collect();
    let mut family = vec![];
    for (i, name) in fam.iter().enumerate() {
        let want = plain(name);
        let mut cs: Vec<Value> = cards(&found[i + 2]).into_iter().filter(|c| shown.insert(text(c, "id").to_string())).collect();
        cs.sort_by_key(|c| (set.is_empty() || plain(text(c, "set")) != set, plain(text(c, "name")) != want && plain(text(c, "en")) != want));
        let total = cs.len();
        cs.truncate(FAMILY_SHOWN);
        if total > 0 {
            family.push(json!({ "name": name, "cards": cs, "total": total }));
        }
    }
    let total = found[0]["total"].as_u64().map_or(versions.len(), |t| t as usize).saturating_sub(exact.len()).max(versions.len());
    json!({ "lang": lang, "query": query, "exact": exact, "versions": versions, "versions_total": total, "family": family, "error": found[0]["error"] })
}

// ---------- the page and the scan ----------

#[derive(Template)]
#[template(path = "me_scan.html")]
struct ScanPage {
    nav: Chrome,
    /// Only sellers scan; anyone else is shown how to become one.
    seller: bool,
    /// The site has a key (ANTHROPIC_API_KEY).
    on: bool,
    /// Scans this seller has left today, and how many a day each seller gets.
    left: i64,
    per: i64,
}

/// GET /me/scan: the page. Signed out: the log-in page, which comes back here.
pub async fn page(State(app): State<App>, h: HeaderMap) -> Res {
    let Some(m) = current(&app, &h).await? else { return Ok(Redirect::to("/login?next=/me/scan").into_response()) };
    let (id, seller) = (m.id, m.seller);
    let day = crate::vault_ai::today(crate::unix_now());
    let per = per_seller();
    let (mut nav, scans_left) = with_db(&app, move |c| {
        let (mine, site) = left(c, id, &day, per, site_max(c, per)?)?;
        Ok((chrome(c)?, mine.min(site)))
    })
    .await?;
    nav.seo.title = format!("Scan a card · {}", nav.brand);
    Ok(html(render(ScanPage { nav, seller, on: enabled(), left: scans_left, per })?))
}

/// POST /me/scan (a form with the photo): a seller's scan. Answers with what was read and what the card list has.
pub async fn read(State(app): State<App>, h: HeaderMap, mp: Multipart) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if !m.seller {
        return Ok(fail(StatusCode::FORBIDDEN, "Scanning is for sellers. Apply as a seller from the menu."));
    }
    scan(&app, m.id, mp).await
}

/// POST /admin/scan/try (a form with the photo): the owner tries the scanner, counted as the vault's own scans (0).
pub async fn try_it(State(app): State<App>, h: HeaderMap, mp: Multipart) -> Res {
    if !crate::admin::authed(&app, &h) || !crate::community::same_site(&h) {
        return Ok((StatusCode::NOT_FOUND, "Not found").into_response());
    }
    scan(&app, 0, mp).await
}

async fn scan(app: &App, who: i64, mut mp: Multipart) -> Res {
    let Some(key) = key() else {
        return Ok(fail(StatusCode::SERVICE_UNAVAILABLE, "Scanning isn't switched on yet. Use Add cards in My store for now."));
    };
    let now = crate::unix_now();
    if PAUSED_UNTIL.load(Ordering::Relaxed) > now {
        return Ok(fail(StatusCode::SERVICE_UNAVAILABLE, "The card reader is resting for a few minutes. Please try again soon, or use Add cards in My store."));
    }
    let mut photo = None;
    while let Some(f) = mp.next_field().await.map_err(|_| (StatusCode::BAD_REQUEST, "The photo didn't upload. Please try again.".to_string()))? {
        if f.name() == Some("photo") {
            let b = f.bytes().await.map_err(|_| (StatusCode::BAD_REQUEST, "The photo didn't upload. Please try again.".to_string()))?;
            if !b.is_empty() {
                photo = Some(b.to_vec());
            }
        }
    }
    let Some(photo) = photo else { return Ok(fail(StatusCode::BAD_REQUEST, "Take a photo of the card first.")) };
    let Some(kind) = media_type(&photo) else { return Ok(fail(StatusCode::BAD_REQUEST, "That photo couldn't be read. Try a JPEG or PNG.")) };
    if photo.len() > MAX_BYTES || check_size(&photo).is_err() {
        return Ok(fail(StatusCode::BAD_REQUEST, "That photo is too big. Please take it again with the card camera."));
    }
    if over_limit(app, format!("scan|{who}"), BURST) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of scans in a short time. Please take a short break and try again."));
    }
    let day = crate::vault_ai::today(now);
    let per = per_seller();
    let d2 = day.clone();
    let (mine, site) = with_db(app, move |c| left(c, who, &d2, per, site_max(c, per)?)).await?;
    if site <= 0 {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "The site has used today's scans. They start again at midnight. Use Add cards in My store for now."));
    }
    if mine <= 0 {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, &format!("You've used today's {per} scans. They start again at midnight. Use Add cards in My store for now.")));
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(&photo);
    drop(photo); // read, not kept
    let reply = call(app, &key, &body(&model(), &effort(), kind, &b64)).await;
    // What the last reply used (tokens in and out), for the owner's status: the cost of a scan, as it really is.
    let usage = reply.as_ref().ok().map(|v| format!("{}|{}", v["usage"]["input_tokens"].as_i64().unwrap_or(0), v["usage"]["output_tokens"].as_i64().unwrap_or(0)));
    if reply.is_ok() {
        // A reply came back, so it is paid for, whatever it says: it counts.
        with_db(app, move |c| count(c, who, &day)).await?;
    }
    let reading = match reply.and_then(|v| read_reply(&v)) {
        Ok(r) => {
            with_db(app, move |c| {
                blocks::set_setting(c, "scan_last_ok", &now.to_string())?;
                blocks::set_setting(c, "scan_last_usage", &usage.unwrap_or_default())
            })
            .await
            .ok();
            r
        }
        Err(e) => {
            // Too many at once: a minute. The key, its billing or its access is the problem: ten minutes, so sellers
            // don't each wait on a call that can't work until someone fixes it.
            if e.starts_with("429") {
                PAUSED_UNTIL.store(now + 60, Ordering::Relaxed);
            } else if e.starts_with("401") || e.starts_with("403") || (e.starts_with("400") && ["credit", "workspace"].iter().any(|w| e.to_lowercase().contains(w))) {
                PAUSED_UNTIL.store(now + 600, Ordering::Relaxed);
            }
            tracing::warn!("Scan a card: {e}");
            let note = format!("{now}|{}", e.chars().take(300).collect::<String>());
            with_db(app, move |c| blocks::set_setting(c, "scan_last_error", &note)).await.ok();
            let unreadable = ["refusal", "cut off", "no answer", "odd answer"].iter().any(|p| e.starts_with(p));
            return Ok(if unreadable {
                fail(StatusCode::UNPROCESSABLE_ENTITY, "That photo couldn't be read. Try again with the card filling the frame.")
            } else {
                fail(StatusCode::SERVICE_UNAVAILABLE, "The card reader isn't answering right now. Please try again in a moment, or use Add cards in My store.")
            });
        }
    };
    let left_now = (mine - 1).min(site - 1).max(0);
    let mut out = if reading.problem == "none" { look_up(app, &reading).await } else { json!({ "exact": [], "versions": [], "versions_total": 0, "family": [] }) };
    out["read"] = json!(reading);
    out["left"] = json!(left_now);
    Ok(ok(out))
}

/// GET /admin/scan/status: is scanning on, scans today, the last problem (the owner's check).
pub async fn status(State(app): State<App>, h: HeaderMap) -> Res {
    if !crate::admin::authed(&app, &h) {
        return Ok((StatusCode::NOT_FOUND, "Not found").into_response());
    }
    let day = crate::vault_ai::today(crate::unix_now());
    let per = per_seller();
    let (today, last_ok, last_error, usage, daily_max) = with_db(&app, move |c| {
        Ok((used(c, "scan_day", &day)?, blocks::setting(c, "scan_last_ok")?, blocks::setting(c, "scan_last_error")?, blocks::setting(c, "scan_last_usage")?, site_max(c, per)?))
    })
    .await?;
    // "1234|56": the last scan's tokens in and out.
    let (tokens_in, tokens_out) = usage.as_deref().and_then(|u| u.split_once('|')).map_or((0, 0), |(i, o)| (i.parse().unwrap_or(0), o.parse().unwrap_or(0)));
    Ok(ok(json!({
        "on": enabled(), "model": model(), "effort": effort(), "workspace": workspace(), "today": today, "daily_max": daily_max, "per_seller": per,
        "last_ok": last_ok, "last_error": last_error, "last_tokens_in": tokens_in, "last_tokens_out": tokens_out,
        "paused_until": PAUSED_UNTIL.load(Ordering::Relaxed)
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, name: &str, number: &str, set: &str, total: u64) -> Value {
        json!({ "id": id, "name": name, "number": number, "set": set, "total": total, "lang": "en", "en": null, "image": "", "image_large": "" })
    }

    fn reading(name: &str, number: &str, set: &str) -> Reading {
        Reading { problem: "none".into(), name: name.into(), name_en: name.into(), species: name.into(), number: number.into(), set_name: set.into(), language: "en".into(), sure: "high".into() }
    }

    fn ids(v: &[Value]) -> Vec<&str> {
        v.iter().map(|c| text(c, "id")).collect()
    }

    #[test]
    fn collector_numbers_are_compared_by_their_letters_and_their_number() {
        assert_eq!(number_key("092/132"), Some((String::new(), 92)));
        assert_eq!(number_key("92"), number_key("092/132"));
        assert_eq!(number_key("TG05/TG30"), Some(("tg".into(), 5)));
        assert_eq!(number_key("tg5"), number_key("TG05"));
        assert_eq!(number_key("SV049/SV094"), Some(("sv".into(), 49)));
        assert_eq!(number_key("SWSH045"), Some(("swsh".into(), 45)));
        assert_eq!(number_key("SWSH 045"), number_key("SWSH045"));
        assert_ne!(number_key("177a"), number_key("177"), "a lettered printing is another card");
        assert_ne!(number_key("TG05"), number_key("5"));
        assert_eq!(number_key(""), None);
        assert_eq!(number_key("Lotad"), None);
        assert_eq!(printed_total("92/132"), Some(132));
        assert_eq!(printed_total("TG05/TG30"), None);
        assert_eq!(printed_total("SWSH045"), None);
    }

    #[test]
    fn the_card_read_comes_first_and_its_other_printings_follow() {
        let list = vec![
            card("tcgdex:sv06-140", "Lotad", "140", "Twilight Masquerade", 167),
            card("tcgdex:dp3-92", "Lotad", "92", "Secret Wonders", 132),
            card("tcgdex:ex7-64", "Lotad", "64", "Team Rocket Returns", 109),
            card("tcgdex:xy4-92", "Lotad", "92", "Phantom Forces", 119),
        ];
        let (exact, rest) = split(vec![], list.clone(), &reading("Lotad", "92/132", "Secret Wonders"));
        assert_eq!(ids(&exact), ["tcgdex:dp3-92"], "the number, the set's size and its name all agree");
        assert_eq!(ids(&rest), ["tcgdex:xy4-92", "tcgdex:sv06-140", "tcgdex:ex7-64"], "a near miss (the same number) first, then the card list's order");
        assert!(rest.iter().all(|c| c.get("guess").is_none()), "with the card found, nothing is a guess");
        // Without the set's name, its size still tells the two 92s apart.
        let (exact, _) = split(vec![], list.clone(), &reading("Lotad", "92/132", ""));
        assert_eq!(ids(&exact), ["tcgdex:dp3-92"]);
        // With no number read, nothing is "this card": everything is shown, the set guessed first and marked.
        let (exact, rest) = split(vec![], list.clone(), &reading("Lotad", "", "Secret Wonders"));
        assert!(exact.is_empty());
        assert_eq!(ids(&rest)[0], "tcgdex:dp3-92", "the set read comes first");
        assert_eq!(rest[0]["guess"], true);
        assert!(rest[1].get("guess").is_none());
        // A set guessed loosely ("EX Team Rocket Returns") still counts.
        let (_, rest) = split(vec![], list.clone(), &reading("Lotad", "", "EX Team Rocket Returns"));
        assert_eq!((ids(&rest)[0], &rest[0]["guess"]), ("tcgdex:ex7-64", &json!(true)));
        // Nothing to go on: no guess.
        let (_, rest) = split(vec![], list.clone(), &reading("Lotad", "", ""));
        assert_eq!(ids(&rest)[0], "tcgdex:sv06-140");
        assert!(rest[0].get("guess").is_none());
        // A number whose set size disagrees with every printing isn't trusted as "this card", only as the best guess.
        let (exact, rest) = split(vec![], list, &reading("Lotad", "140/99", ""));
        assert!(exact.is_empty());
        assert_eq!((ids(&rest)[0], &rest[0]["guess"]), ("tcgdex:sv06-140", &json!(true)));
    }

    #[test]
    fn the_numbers_own_search_finds_a_printing_the_names_first_page_leaves_out() {
        // "Pikachu" has hundreds of printings: the name's first page (newest first) doesn't reach Holon Phantoms.
        let by_name = vec![card("tcgdex:30th-025", "Pikachu", "025", "30th Celebration", 160), card("tcgdex:sv08-063", "Pikachu ex", "063", "Surging Sparks", 191)];
        let by_number = vec![card("tcgdex:ex13-79", "Pikachu δ", "79", "Holon Phantoms", 110), card("tcgdex:sv08-063", "Pikachu ex", "063", "Surging Sparks", 191)];
        let mut r = reading("Pikachu", "79/110", "EX Holon Phantoms");
        r.sure = "medium".into();
        let (exact, rest) = split(by_number, by_name, &r);
        assert_eq!(ids(&exact), ["tcgdex:ex13-79"]);
        assert_eq!(ids(&rest), ["tcgdex:30th-025", "tcgdex:sv08-063"], "each card once");
        assert_eq!(number_head("79/110").as_deref(), Some("79"));
        assert_eq!(number_head("TG05/TG30").as_deref(), Some("TG05"));
        assert_eq!(number_head("SWSH045").as_deref(), Some("SWSH045"));
        assert_eq!(number_head(""), None);
        assert_eq!(number_head("SV-P 001"), None, "not a number the card list searches");
    }

    #[test]
    fn printings_with_the_same_number_are_all_this_card() {
        let list = vec![card("tcgdex:base1-4-1st", "Charizard", "4", "Base Set", 102), card("tcgdex:base1-4", "Charizard", "4", "Base Set", 102), card("tcgdex:base4-4", "Charizard", "4", "Base Set 2", 130)];
        let (exact, rest) = split(vec![], list, &reading("Charizard", "4/102", ""));
        assert_eq!(ids(&exact), ["tcgdex:base1-4-1st", "tcgdex:base1-4"]);
        assert_eq!(ids(&rest), ["tcgdex:base4-4"]);
    }

    #[test]
    fn the_family_leaves_out_the_cards_already_shown() {
        let lotad: Vec<String> = ["Lotad", "Lombre", "Ludicolo"].map(String::from).to_vec();
        let r = reading("Lotad", "92/132", "");
        assert_eq!(family_names(&lotad, &r.query()), ["Lombre", "Ludicolo"]);
        // "Charizard ex": plain Charizard cards are family, not the same card.
        let charizard: Vec<String> = ["Charmander", "Charmeleon", "Charizard"].map(String::from).to_vec();
        let mut r = reading("Charizard ex", "", "");
        r.species = "Charizard".into();
        assert_eq!(family_names(&charizard, &r.query()), ["Charmander", "Charmeleon", "Charizard"]);
        // A Japanese card is looked up by its Pokémon, so that one is already shown.
        r.language = "ja".into();
        r.name = "リザードンex".into();
        assert_eq!((r.lang(), r.query().as_str()), ("ja", "Charizard"));
        assert_eq!(family_names(&charizard, &r.query()), ["Charmander", "Charmeleon"]);
        assert!(family_names(&[], "Dusk Ball").is_empty());
    }

    #[test]
    fn a_reply_is_read_by_block_type_and_kept_to_the_shape() {
        let answer = json!({
            "problem": "none", "name": " Lotad ", "name_en": "", "species": "Lotad", "number": "92/132", "set_name": "Secret Wonders",
            "language": "en", "sure": "certain"
        });
        let reply = json!({
            "stop_reason": "end_turn",
            "content": [{ "type": "thinking", "thinking": "" }, { "type": "text", "text": answer.to_string() }]
        });
        let r = read_reply(&reply).unwrap();
        assert_eq!((r.name.as_str(), r.name_en.as_str(), r.sure.as_str()), ("Lotad", "Lotad", "low"), "trimmed; an English name filled in; an odd 'sure' is 'low'");
        assert!(read_reply(&json!({ "stop_reason": "refusal", "stop_details": { "category": "general_harms" }, "content": [] })).unwrap_err().starts_with("refusal"));
        assert_eq!(read_reply(&json!({ "stop_reason": "max_tokens", "content": [] })).unwrap_err(), "cut off");
        assert!(read_reply(&json!({ "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Lotad, I think" }] })).unwrap_err().starts_with("odd answer"));
        // Nothing to look up is a photo that couldn't be read.
        let blank = json!({ "stop_reason": "end_turn", "content": [{ "type": "text", "text": json!({ "problem": "none", "language": "en" }).to_string() }] });
        assert_eq!(read_reply(&blank).unwrap().problem, "blurry");
    }

    #[test]
    fn the_request_asks_for_the_shape_and_sends_no_sampling_settings() {
        let b = body(DEFAULT_MODEL, DEFAULT_EFFORT, "image/jpeg", "AAAA");
        assert_eq!(b["model"], "claude-sonnet-5-5");
        assert_eq!(b["output_config"]["effort"], "high", "the owner's choice");
        assert_eq!(b["max_tokens"], 16000, "room for the thinking as well as the answer");
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        let s = &b["output_config"]["format"]["schema"];
        assert_eq!(s["additionalProperties"], false);
        assert_eq!(s["required"].as_array().unwrap().len(), s["properties"].as_object().unwrap().len(), "every field is required");
        assert_eq!(b["messages"][0]["content"][0]["source"]["media_type"], "image/jpeg");
        assert_eq!(b["fallbacks"], "default");
        for k in ["temperature", "top_p", "top_k", "thinking", "tool_choice"] {
            assert!(b.get(k).is_none(), "{k} isn't sent");
        }
        assert!(body("claude-haiku-5-5", "low", "image/png", "AAAA").get("fallbacks").is_none(), "only the default model asks for the fallback");
        assert!(EFFORTS.contains(&DEFAULT_EFFORT));
        assert_eq!(media_type(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(media_type(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(media_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(media_type(b"GIF89a"), None);
    }

    #[test]
    fn a_cards_versions_come_from_its_prices_and_a_bare_market_price_is_none() {
        let price = json!({ "matches": [{ "finishes": [
            { "key": "normal", "label": "Normal", "usd": 0.45, "php": 26.0 },
            { "key": "reverse-holofoil", "label": "Reverse Holo", "usd": 6.1, "php": 350.0 },
            { "key": "market", "label": "Market", "usd": 1.0, "php": 58.0 }
        ] }] });
        assert_eq!(finishes(&price), json!([{ "key": "normal", "label": "Normal", "php": 26.0 }, { "key": "reverse-holofoil", "label": "Reverse Holo", "php": 350.0 }]));
        assert_eq!(finishes(&json!({ "matches": [] })), json!([]));
    }

    #[test]
    fn the_sites_day_follows_the_number_of_sellers() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch(marcuru_rs::members::SCHEMA).unwrap();
        marcuru_rs::members::migrate(&c).unwrap();
        let add = |name: &str, seller: i64, blocked: i64| {
            c.execute(
                "INSERT INTO members(name, name_key, pass, created_at, updated_at, seller, blocked) VALUES(?1, ?1, 'x', 0, 0, ?2, ?3)",
                rusqlite::params![name, seller, blocked],
            )
            .unwrap();
        };
        assert_eq!(site_max_with(&c, 30, None).unwrap(), 30, "no sellers yet: the vault's own share");
        for i in 0..10 {
            add(&format!("seller{i}"), 1, 0);
        }
        add("trainer", 0, 0);
        add("blocked", 1, 1);
        assert_eq!(site_max_with(&c, 30, None).unwrap(), 330, "ten sellers: 30 each, and 30 for the vault; trainers and blocked sellers don't count");
        assert_eq!(site_max_with(&c, 30, Some(500)).unwrap(), 500, "SCAN_DAILY_MAX fixes it");
    }

    #[test]
    fn scans_are_counted_per_seller_and_for_the_site_each_day() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch(blocks::SCHEMA).unwrap();
        assert_eq!(left(&c, 7, "2026-11-18", 2, 3).unwrap(), (2, 3));
        count(&c, 7, "2026-11-18").unwrap();
        count(&c, 7, "2026-11-18").unwrap();
        assert_eq!(left(&c, 7, "2026-11-18", 2, 3).unwrap(), (0, 1), "the seller's two are used; the site has one left");
        assert_eq!(left(&c, 8, "2026-11-18", 2, 3).unwrap(), (2, 1), "another seller still has theirs, within the site's");
        count(&c, 8, "2026-11-18").unwrap();
        assert_eq!(left(&c, 8, "2026-11-18", 2, 3).unwrap().1, 0);
        assert_eq!(left(&c, 7, "2026-11-19", 2, 3).unwrap(), (2, 3), "a new day starts again");
    }
}
