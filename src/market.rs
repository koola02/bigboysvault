//! Today's market value for a card, converted to pesos.
//! Primary source: TCGdex (card data + TCGplayer USD / Cardmarket EUR prices, refreshed daily).
//! Backup source: pokemontcg.io. Exchange rate: frankfurter.dev (cached). Every call retries, and failures never block saving.
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::task::JoinSet;

const DEX: &str = "https://api.tcgdex.net/v2/en";
const PTCG: &str = "https://api.pokemontcg.io/v2/cards";
const PTCG_SELECT: &str = "id,name,number,rarity,set,images,tcgplayer,cardmarket";

#[derive(Clone, Copy)]
pub struct Fx {
    pub usd_php: f64,
    pub eur_php: f64,
}

pub type FxCache = Mutex<Option<(Instant, Fx)>>;

/// USD→PHP and EUR→PHP, cached for 6 hours so a burst of lookups costs one request.
pub async fn fx(client: &reqwest::Client, cache: &FxCache) -> Result<Fx, String> {
    let mut g = cache.lock().await;
    if let Some((t, f)) = *g {
        if t.elapsed() < Duration::from_secs(6 * 3600) {
            return Ok(f);
        }
    }
    let fresh = async {
        let v: Value = client.get("https://api.frankfurter.dev/v1/latest?base=USD&symbols=PHP,EUR").send().await.ok()?.json().await.ok()?;
        let php = v["rates"]["PHP"].as_f64()?;
        let eur = v["rates"]["EUR"].as_f64()?;
        Some(Fx { usd_php: php, eur_php: php / eur })
    }
    .await;
    match fresh {
        Some(f) => {
            *g = Some((Instant::now(), f));
            Ok(f)
        }
        None => g.map(|(_, f)| f).ok_or_else(|| "Couldn't get today's exchange rate.".to_string()), // an old rate beats none
    }
}

fn http_err<E: std::fmt::Display>(e: E) -> String {
    format!("The price service didn't answer ({e}).")
}

/// GET json with retries on 5xx / network errors. Ok(Null) on 404.
async fn get_json(client: &reqwest::Client, url: &str, query: &[(&str, &str)], key: bool) -> Result<Value, String> {
    let mut last = String::from("no answer");
    for attempt in 0..3u64 {
        let mut rb = client.get(url).query(query);
        if key {
            if let Ok(k) = std::env::var("POKEMONTCG_API_KEY") {
                if !k.is_empty() {
                    rb = rb.header("X-Api-Key", k);
                }
            }
        }
        match rb.send().await {
            Ok(r) if r.status().is_success() => return r.json().await.map_err(http_err),
            Ok(r) if r.status().as_u16() == 404 => return Ok(Value::Null),
            Ok(r) => last = r.status().to_string(),
            Err(e) => last = e.to_string(),
        }
        tokio::time::sleep(Duration::from_millis(350 * (attempt + 1))).await;
    }
    Err(http_err(last))
}

pub fn label(key: &str) -> String {
    match key {
        "normal" => "Normal".into(),
        "holofoil" | "holo" => "Holofoil".into(),
        "reverse" | "reverseHolofoil" | "reverse-holofoil" => "Reverse Holo".into(),
        "1stEditionHolofoil" | "1st-edition-holofoil" => "1st Ed. Holo".into(),
        "1stEditionNormal" | "1st-edition" | "1st-edition-normal" => "1st Ed.".into(),
        "unlimitedHolofoil" | "unlimited-holofoil" => "Unlimited Holo".into(),
        "market" => "Market".into(),
        other => {
            let s = other.replace('-', " ");
            let mut c = s.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or(s)
        }
    }
}

/// Every name the price lists use for one finish. Saved cards use the first kind ("holofoil"); TCGdex prices
/// Wizards-era cards under its own ("unlimited-holofoil", "1st-edition"). Anything else is only itself.
pub fn finish_aliases(f: &str) -> &'static [&'static str] {
    match f {
        "1stEditionHolofoil" | "1st-edition-holofoil" => &["1stEditionHolofoil", "1st-edition-holofoil"],
        "1stEditionNormal" | "1st-edition" | "1st-edition-normal" => &["1stEditionNormal", "1st-edition", "1st-edition-normal"],
        "holofoil" | "holo" | "unlimited-holofoil" | "unlimitedHolofoil" => &["holofoil", "unlimited-holofoil", "unlimitedHolofoil", "holo"],
        "normal" | "unlimited" => &["normal", "unlimited"],
        "reverseHolofoil" | "reverse-holofoil" | "reverse" => &["reverseHolofoil", "reverse-holofoil", "reverse"],
        _ => &[],
    }
}

/// True when two finish names mean the same finish ("normal" and TCGdex's "unlimited").
pub fn same_finish(a: &str, b: &str) -> bool {
    a == b || finish_aliases(a).contains(&b)
}

/// The order finishes are listed in, and the one a copy takes when it names none: the common printing first
/// (Holofoil, Normal or Unlimited, Reverse Holo), 1st Edition last, so nothing borrows a 1st Edition price by accident.
fn rank(key: &str) -> u8 {
    match key {
        "holofoil" | "holo" | "unlimited-holofoil" | "unlimitedHolofoil" => 0,
        "normal" | "unlimited" => 1,
        "reverse" | "reverseHolofoil" | "reverse-holofoil" => 2,
        k if k.starts_with("1st") => 4,
        _ => 3,
    }
}

/// The finish a copy with no finish (or one nobody knows) takes: the common printing, never 1st Edition while
/// another finish is listed. Works whatever order the list is in.
pub fn default_finish(fin: &[Value]) -> Option<&Value> {
    fin.iter().min_by_key(|f| rank(f["key"].as_str().unwrap_or("")))
}

/// The market's entry for one finish of a card (`card["finishes"]`), matched by any name the price lists use for it
/// ("normal" is TCGdex's "unlimited"). A copy with no finish, "market", or a finish this site doesn't know takes the
/// default finish. A known finish the service doesn't list gets nothing, so an Unlimited copy never borrows the
/// 1st Edition price. The vault's nightly prices and the owner's bulk "Market price today" both use this.
pub fn finish_pick<'a>(card: &'a Value, finish: &str) -> Option<&'a Value> {
    let fin = card["finishes"].as_array()?;
    match fin.iter().find(|f| same_finish(finish, f["key"].as_str().unwrap_or(""))) {
        Some(f) => Some(f),
        None if finish.is_empty() || finish == "market" || finish_aliases(finish).is_empty() => default_finish(fin),
        None => None,
    }
}

/// The Near Mint price of that finish, in pesos (see `finish_pick`).
pub fn finish_price(card: &Value, finish: &str) -> Option<f64> {
    finish_pick(card, finish).and_then(|f| f["php"].as_f64()).filter(|v| *v > 0.0)
}

/// A seller's finish is typed as words or a key ("Reverse Holo", "Normal", "reverseHolofoil"), any letter case:
/// the matching market finish's Near Mint price, else the default finish's.
pub fn member_finish_price(card: &Value, finish: &str) -> Option<f64> {
    let fin = card["finishes"].as_array()?;
    let f = finish.trim();
    let lower = f.to_ascii_lowercase();
    let dashed = lower.split_whitespace().collect::<Vec<_>>().join("-");
    let pick = fin
        .iter()
        .find(|x| {
            let key = x["key"].as_str().unwrap_or("");
            !f.is_empty()
                && (same_finish(f, key)
                    || same_finish(&lower, key)
                    || same_finish(&dashed, key)
                    || x["label"].as_str().is_some_and(|l| l.eq_ignore_ascii_case(f))
                    || label(key).eq_ignore_ascii_case(f))
        })
        .or_else(|| default_finish(fin));
    pick.and_then(|x| x["php"].as_f64()).filter(|v| *v > 0.0)
}

fn finishes_json(mut finishes: Vec<(String, f64)>, fx: Fx) -> Vec<Value> {
    finishes.sort_by_key(|(k, _)| rank(k));
    finishes
        .iter()
        .map(|(k, usd)| json!({ "key": k, "label": label(k), "usd": (usd * 100.0).round() / 100.0, "php": (usd * fx.usd_php).round() }))
        .collect()
}

fn price_of(v: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|k| v[*k].as_f64()).filter(|p| *p > 0.0)
}

// ---------------- TCGdex (primary) ----------------
fn dex_shape(card: &Value, fx: Fx) -> Value {
    let mut fin: Vec<(String, f64)> = vec![];
    let mut source = "TCGplayer";
    let mut updated = card["pricing"]["tcgplayer"]["updated"].as_str().unwrap_or("").chars().take(10).collect::<String>();
    if let Some(o) = card["pricing"]["tcgplayer"].as_object() {
        for (k, v) in o {
            if v.is_object() {
                if let Some(usd) = price_of(v, &["marketPrice", "midPrice", "lowPrice"]) {
                    fin.push((k.clone(), usd));
                }
            }
        }
    }
    if fin.is_empty() {
        let cm = &card["pricing"]["cardmarket"];
        if let Some(eur) = price_of(cm, &["trend", "avg"]) {
            fin.push(("market".into(), eur * fx.eur_php / fx.usd_php));
            source = "Cardmarket";
            updated = cm["updated"].as_str().unwrap_or("").chars().take(10).collect();
        }
    }
    let img = card["image"].as_str().unwrap_or("");
    json!({
        "id": format!("tcgdex:{}", card["id"].as_str().unwrap_or("")),
        "name": card["name"], "number": card["localId"],
        "set": card["set"]["name"], "total": card["set"]["cardCount"]["official"], "rarity": card["rarity"],
        "image": if img.is_empty() { Value::Null } else { json!(format!("{img}/low.webp")) },
        "image_large": if img.is_empty() { Value::Null } else { json!(format!("{img}/high.png")) },
        "finishes": finishes_json(fin, fx), "source": source, "updated": updated,
    })
}

async fn dex_detail(client: &reqwest::Client, id: &str) -> Result<Value, String> {
    get_json(client, &format!("{DEX}/cards/{id}"), &[], false).await
}

async fn dex_search(client: &reqwest::Client, fx: Fx, name: &str, stripped: &str, total: Option<i64>) -> Result<Vec<Value>, String> {
    let list = get_json(client, &format!("{DEX}/cards"), &[("name", name), ("localId", stripped)], false).await?;
    let want = stripped.to_lowercase();
    let ids: Vec<String> = list
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|c| c["localId"].as_str().map_or(false, |l| l.trim_start_matches('0').to_lowercase() == want))
        .filter_map(|c| c["id"].as_str().map(str::to_string))
        .take(10)
        .collect();
    let mut set = JoinSet::new();
    for id in ids {
        let client = client.clone();
        set.spawn(async move { dex_detail(&client, &id).await });
    }
    let mut cards: Vec<Value> = vec![];
    while let Some(r) = set.join_next().await {
        if let Ok(Ok(v)) = r {
            if v.is_object() {
                cards.push(v);
            }
        }
    }
    if let Some(t) = total {
        cards.sort_by_key(|c| if c["set"]["cardCount"]["official"].as_i64() == Some(t) { 0 } else { 1 });
    }
    Ok(cards.iter().take(8).map(|c| dex_shape(c, fx)).collect())
}

// ---------------- pokemontcg.io (backup) ----------------
fn ptcg_shape(card: &Value, fx: Fx) -> Value {
    let mut fin: Vec<(String, f64)> = vec![];
    if let Some(p) = card["tcgplayer"]["prices"].as_object() {
        for (k, v) in p {
            if let Some(usd) = price_of(v, &["market", "mid", "low"]) {
                fin.push((k.clone(), usd));
            }
        }
    }
    let mut source = "TCGplayer";
    let mut updated = card["tcgplayer"]["updatedAt"].as_str().unwrap_or("").replace('/', "-");
    if fin.is_empty() {
        let cm = &card["cardmarket"]["prices"];
        if let Some(eur) = price_of(cm, &["trendPrice", "averageSellPrice"]) {
            fin.push(("market".into(), eur * fx.eur_php / fx.usd_php));
            source = "Cardmarket";
            updated = card["cardmarket"]["updatedAt"].as_str().unwrap_or("").replace('/', "-");
        }
    }
    json!({
        "id": card["id"], "name": card["name"], "number": card["number"],
        "set": card["set"]["name"], "total": card["set"]["printedTotal"], "rarity": card["rarity"],
        "image": card["images"]["small"], "image_large": card["images"]["large"],
        "finishes": finishes_json(fin, fx), "source": source, "updated": updated,
    })
}

async fn ptcg_search(client: &reqwest::Client, fx: Fx, name: &str, num: &str, stripped: &str, total: Option<i64>) -> Result<Vec<Value>, String> {
    let q = format!("name:\"{name}\" (number:\"{stripped}\" OR number:\"{num}\")");
    let v = get_json(client, PTCG, &[("q", q.as_str()), ("pageSize", "20"), ("select", PTCG_SELECT)], true).await?;
    let mut cards: Vec<Value> = v["data"].as_array().cloned().unwrap_or_default();
    if let Some(t) = total {
        cards.sort_by_key(|c| if c["set"]["printedTotal"].as_i64() == Some(t) { 0 } else { 1 });
    }
    Ok(cards.iter().take(8).map(|c| ptcg_shape(c, fx)).collect())
}

// ---------------- public API ----------------
/// Find cards by name + number ("057/191", "57", "TG01"). Matches whose printed set total equals the typed total come first.
pub async fn search(client: &reqwest::Client, fxc: &FxCache, name: &str, number: &str) -> Result<Value, String> {
    let clean: String = name.chars().filter(|c| !matches!(c, '"' | '\\' | ':' | '(' | ')' | '*')).collect::<String>().trim().to_string();
    let (num, total) = match number.split_once('/') {
        Some((a, b)) => (a.trim().to_string(), b.trim().parse::<i64>().ok()),
        None => (number.trim().to_string(), None),
    };
    let num: String = num.chars().filter(|c| c.is_alphanumeric()).collect();
    if clean.len() < 2 || num.is_empty() {
        return Ok(json!({ "matches": [] }));
    }
    let stripped = {
        let s = num.trim_start_matches('0');
        if s.is_empty() { "0".to_string() } else { s.to_string() }
    };
    let fx = fx(client, fxc).await?;
    let first = dex_search(client, fx, &clean, &stripped, total).await;
    let matches = match first {
        Ok(m) if !m.is_empty() => m,
        other => match ptcg_search(client, fx, &clean, &num, &stripped, total).await {
            Ok(m) => m,
            Err(e) => match other {
                Ok(_) => vec![], // primary answered "no such card"; backup is down — report no match
                Err(_) => return Err(e),
            },
        },
    };
    let same_total = |m: &Value| total.is_some() && m["total"].as_i64() == total;
    let exact = matches.first().map_or(false, |m| same_total(m)) && matches.iter().filter(|m| same_total(m)).count() == 1;
    Ok(json!({ "matches": matches, "rate": fx.usd_php, "exact": exact }))
}

/// Today's price for an already-matched card id ("tcgdex:sv08-057" or a pokemontcg.io id like "sv8-57").
pub async fn by_id(client: &reqwest::Client, fxc: &FxCache, id: &str) -> Result<Option<Value>, String> {
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':')) {
        return Ok(None);
    }
    let fx = fx(client, fxc).await?;
    if let Some(dex) = id.strip_prefix("tcgdex:") {
        let v = dex_detail(client, dex).await?;
        return Ok(v.is_object().then(|| dex_shape(&v, fx)));
    }
    let v = get_json(client, &format!("{PTCG}/{id}"), &[("select", PTCG_SELECT)], true).await?;
    Ok(v["data"].is_object().then(|| ptcg_shape(&v["data"], fx)))
}

/// Only official card-image hosts may be downloaded when "use this photo" is chosen.
pub fn image_host_ok(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split('/').next().unwrap_or("");
    ["images.pokemontcg.io", "images.scrydex.com", "assets.tcgdex.net", "limitlesstcg.nyc3.cdn.digitaloceanspaces.com"].contains(&host)
}

// ---------------- picture search (quick add, add by set) ----------------
/// Card catalogues. English keeps the original id format ("tcgdex:sv08-057"); the others carry their code in the id
/// ("tcgdex-ja:SV8-057") so prices and pictures come from the right catalogue.
pub const LANGS: [&str; 4] = ["en", "ja", "zh-tw", "zh-cn"];

pub fn lang_of(code: &str) -> &'static str {
    LANGS.iter().copied().find(|l| *l == code).unwrap_or("en")
}

/// What the language switch means: "zh" (Chinese) searches both Chinese catalogues, Traditional and Simplified.
fn catalogues(ui: &str) -> &'static [&'static str] {
    match ui {
        "ja" => &["ja"],
        "zh" | "zh-tw" | "zh-cn" => &["zh-tw", "zh-cn"],
        _ => &["en"],
    }
}

fn dex_base(lang: &str) -> String {
    format!("https://api.tcgdex.net/v2/{}", lang_of(lang))
}

fn card_id(lang: &str, raw: &str) -> String {
    if lang == "en" { format!("tcgdex:{raw}") } else { format!("tcgdex-{lang}:{raw}") }
}

/// "tcgdex-ja:SV8-057" -> ("ja", "SV8-057"); "tcgdex:sv08-057" -> ("en", "sv08-057").
pub fn split_card_id(id: &str) -> Option<(&'static str, &str)> {
    if let Some(raw) = id.strip_prefix("tcgdex:") {
        return Some(("en", raw));
    }
    let rest = id.strip_prefix("tcgdex-")?;
    let (lang, raw) = rest.split_once(':')?;
    let l = lang_of(lang);
    (l == lang).then_some((l, raw))
}

/// Today's price for a card in any language (Japanese and Chinese cards are usually priced on Cardmarket).
pub async fn by_id_lang(client: &reqwest::Client, fxc: &FxCache, id: &str) -> Result<Option<Value>, String> {
    match split_card_id(id) {
        Some((lang, raw)) if lang != "en" => {
            if !raw.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.')) {
                return Ok(None);
            }
            let fx = fx(client, fxc).await?;
            let v = get_json(client, &format!("{}/cards/{raw}", dex_base(lang)), &[], false).await?;
            Ok(v.is_object().then(|| {
                let mut s = dex_shape(&v, fx);
                s["id"] = json!(id);
                s
            }))
        }
        _ => by_id(client, fxc, id).await,
    }
}

// ---- English names for Japanese and Chinese cards (Pokémon and items), from PokeAPI's name tables ----
#[derive(Default)]
pub struct NameRow {
    pub en: String,
    pub en_lower: String,
    pub ja: Vec<String>,
    pub zh: Vec<String>,
}
pub type Names = Vec<NameRow>;
static NAMES: Mutex<Option<(Instant, std::sync::Arc<Names>)>> = Mutex::const_new(None);
const NAME_CSVS: [&str; 2] = [
    "https://raw.githubusercontent.com/PokeAPI/pokeapi/master/data/v2/csv/pokemon_species_names.csv",
    "https://raw.githubusercontent.com/PokeAPI/pokeapi/master/data/v2/csv/item_names.csv",
];

/// Minimal CSV line split that honours "quoted, fields".
fn csv_fields(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut q) = (vec![], String::new(), false);
    let mut it = line.chars().peekable();
    while let Some(ch) = it.next() {
        match ch {
            '"' if q && it.peek() == Some(&'"') => {
                cur.push('"');
                it.next();
            }
            '"' => q = !q,
            ',' if !q => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out
}

/// Pokémon species + item names: English with Japanese (kana and kanji forms) and Chinese (Traditional and Simplified).
/// Refreshed weekly; if the source is down, the last good table is kept.
pub async fn name_table(client: &reqwest::Client) -> std::sync::Arc<Names> {
    names(client).await
}

async fn names(client: &reqwest::Client) -> std::sync::Arc<Names> {
    let mut g = NAMES.lock().await;
    if let Some((t, n)) = g.as_ref() {
        if t.elapsed() < Duration::from_secs(7 * 86_400) && !n.is_empty() {
            return n.clone();
        }
    }
    let mut rows: Names = vec![];
    for (k, url) in NAME_CSVS.iter().enumerate() {
        let Ok(r) = client.get(*url).timeout(Duration::from_secs(20)).send().await else { continue };
        let Ok(text) = r.text().await else { continue };
        let mut by_id: std::collections::BTreeMap<String, NameRow> = Default::default();
        for line in text.lines().skip(1) {
            let f = csv_fields(line);
            if f.len() < 3 || f[2].trim().is_empty() {
                continue;
            }
            let row = by_id.entry(format!("{k}-{}", f[0])).or_default();
            let name = f[2].trim().to_string();
            match f[1].as_str() {
                "9" => {
                    row.en_lower = name.to_lowercase();
                    row.en = name;
                }
                "1" | "11" => {
                    if !row.ja.contains(&name) {
                        row.ja.push(name)
                    }
                }
                "4" | "12" => {
                    if !row.zh.contains(&name) {
                        row.zh.push(name)
                    }
                }
                _ => {}
            }
        }
        rows.extend(by_id.into_values().filter(|r| !r.en.is_empty() && (!r.ja.is_empty() || !r.zh.is_empty())));
    }
    if rows.is_empty() {
        return g.as_ref().map(|(_, n)| n.clone()).unwrap_or_default();
    }
    let arc = std::sync::Arc::new(rows);
    *g = Some((Instant::now(), arc.clone()));
    arc
}

fn native<'a>(r: &'a NameRow, cat: &str) -> &'a [String] {
    if cat == "ja" { &r.ja } else { &r.zh }
}

/// English name for a Japanese/Chinese card name: the longest known Pokémon/item name inside it
/// ("ナンジャモのハラバリー" -> Bellibolt, "カビゴンex" -> Snorlax).
fn english_label<'a>(card_name: &str, cat: &str, names: &'a Names) -> Option<&'a NameRow> {
    let mut best: Option<(&NameRow, usize)> = None;
    for r in names {
        for n in native(r, cat) {
            let l = n.chars().count();
            if l >= 2 && card_name.contains(n.as_str()) && best.map_or(true, |(_, bl)| l > bl) {
                best = Some((r, l));
            }
        }
    }
    best.map(|b| b.0)
}

/// True when every place `n` appears in the card name is inside a longer known name that is also there
/// (ミュウ inside ミュウツー), i.e. the card isn't really about `n`.
fn shadowed(n: &str, cname: &str, cat: &str, names: &Names) -> bool {
    let longer: Vec<&str> = names
        .iter()
        .flat_map(|r| native(r, cat).iter())
        .filter(|m| m.len() > n.len() && m.contains(n) && cname.contains(m.as_str()))
        .map(String::as_str)
        .collect();
    if longer.is_empty() {
        return false;
    }
    let mut rest = cname.to_string();
    for m in &longer {
        rest = rest.replace(m, "|");
    }
    !rest.contains(n)
}

// ---- every card in a catalogue (brief), for searching Japanese / Chinese by name locally ----
type AllCards = std::collections::HashMap<&'static str, (Instant, std::sync::Arc<Vec<Value>>)>;
static ALL_CARDS: Mutex<Option<AllCards>> = Mutex::const_new(None);

async fn all_cards(client: &reqwest::Client, cat: &str) -> Result<std::sync::Arc<Vec<Value>>, String> {
    let cat = lang_of(cat);
    let mut g = ALL_CARDS.lock().await;
    let cache = g.get_or_insert_with(Default::default);
    if let Some((t, v)) = cache.get(cat) {
        if t.elapsed() < Duration::from_secs(86_400) {
            return Ok(v.clone());
        }
    }
    match get_json(client, &format!("{}/cards", dex_base(cat)), &[], false).await {
        Ok(v) => {
            let arc = std::sync::Arc::new(v.as_array().cloned().unwrap_or_default());
            cache.insert(cat, (Instant::now(), arc.clone()));
            Ok(arc)
        }
        Err(e) => cache.get(cat).map(|(_, v)| v.clone()).ok_or(e),
    }
}

#[derive(Clone, Default)]
struct SetRow {
    /// Position in TCGdex's list (oldest first).
    order: usize,
    name: String,
    /// Printed total ("/191"); secret rares go above it.
    total: i64,
    /// Every card in the set, secret rares included.
    count: i64,
    logo: String,
    symbol: String,
}
type SetInfo = std::collections::HashMap<String, SetRow>;

/// Pokémon TCG Pocket (the phone game) sets: A1, A2b, B1, P-A … They have no physical cards.
fn pocket_set(id: &str) -> bool {
    let b = id.as_bytes();
    id.starts_with("P-") || (b.len() >= 2 && (b[0] == b'A' || b[0] == b'B') && b[1].is_ascii_digit())
}

/// Japanese / Chinese sets and the English set they became (checked by hand; the codes alone aren't reliable).
/// The "a" mini-sets were folded into the next English main set, so they point there too.
const EN_TWIN: [(&str, &str); 32] = [
    ("SV1S", "sv01"), ("SV1V", "sv01"), ("SV1a", "sv02"), ("SV2P", "sv02"), ("SV2D", "sv02"), ("SV3", "sv03"), ("SV2a", "sv03.5"),
    ("SV3a", "sv04"), ("SV4K", "sv04"), ("SV4M", "sv04"), ("SV4a", "sv04.5"), ("SV5K", "sv05"), ("SV5M", "sv05"), ("SV5a", "sv06"),
    ("SV6", "sv06"), ("SV6a", "sv06.5"), ("SV7", "sv07"), ("SV7a", "sv08"), ("SV8", "sv08"), ("SV8a", "sv08.5"), ("SV9", "sv09"),
    ("SV9a", "sv10"), ("SV10", "sv10"), ("SV11B", "sv10.5b"), ("SV11W", "sv10.5w"), ("SV-P", "svp"), ("M1L", "me01"), ("M1S", "me01"),
    ("M2", "me02"), ("M2a", "me02.5"), ("M3", "me03"), ("M-P", "mep"),
];

fn en_twin(id: &str) -> Option<&'static str> {
    EN_TWIN.iter().find(|(a, _)| *a == id).map(|t| t.1)
}

/// Japanese sets by their official English names, with the matching English-language set in brackets where there is one,
/// so "base set", "terastal festival" or "vstar universe" finds the Japanese set.
const JA_EN: &[(&str, &str)] = &[
    ("PMCG1", "Expansion Pack (Base Set)"), ("PMCG2", "Pokémon Jungle (Jungle)"), ("PMCG3", "Mystery of the Fossils (Fossil)"),
    ("PMCG4", "Rocket Gang (Team Rocket)"), ("PMCG5", "Leaders' Stadium (Gym Heroes)"), ("PMCG6", "Challenge from the Darkness (Gym Challenge)"),
    ("neo1", "Gold, Silver, to a New World (Neo Genesis)"), ("neo2", "Crossing the Ruins (Neo Discovery)"),
    ("neo3", "Awakening Legends (Neo Revelation)"), ("neo4", "Darkness, and to Light (Neo Destiny)"),
    ("VS1", "Pokémon VS"), ("web1", "Pokémon web"),
    ("E1", "Base Expansion Pack (Expedition)"), ("E2", "The Town on No Map"), ("E3", "Wind from the Sea"), ("E4", "Split Earth"), ("E5", "Mysterious Mountains"),
    ("ADV1", "ADV Expansion Pack (EX Ruby & Sapphire)"), ("ADV2", "Miracle of the Desert (EX Sandstorm)"), ("ADV3", "Rulers of the Heavens (EX Dragon)"),
    ("ADV4", "Undone Seal (EX Team Magma vs Team Aqua)"), ("ADV5", "Flight of Legends (EX Hidden Legends)"),
    ("PCG1", "Clash of the Blue Sky"), ("PCG2", "Rocket Gang Strikes Back"), ("PCG3", "Golden Sky, Silvery Ocean"), ("PCG4", "Mirage Forest"),
    ("PCG5", "Holon Research Tower"), ("PCG6", "Holon Phantom"), ("PCG7", "Miracle Crystal"), ("PCG8", "Offense and Defense of the Furthest Ends"),
    ("PCG9", "Clash at the Summit"), ("PCG10", "World Champions Pack"),
    ("L1a", "HeartGold Collection"), ("L1b", "SoulSilver Collection"), ("L2", "Reviving Legends"), ("L3", "Clash at the Summit"), ("LL", "Lost Link"),
    ("BW1b", "Black Collection"), ("BW1a", "White Collection"), ("BW2", "Red Collection"), ("BW4", "Dark Rush"), ("DS", "Dragon Selection"),
    ("BW5a", "Dragon Blade"), ("BW5b", "Dragon Blast"), ("BW6b", "Freeze Bolt"), ("BW6a", "Cold Flare"), ("BW7", "Plasma Gale"), ("SC", "Shiny Collection"),
    ("BW9", "Megalo Cannon"), ("EBB", "EX Battle Boost"),
    ("XY1b", "Collection X"), ("XY1a", "Collection Y"), ("XY2", "Wild Blaze"), ("XY3", "Rising Fist"), ("XY4", "Phantom Gate"),
    ("XY5a", "Gaia Volcano"), ("XY5b", "Tidal Storm"), ("CP1", "Double Crisis"), ("XY6", "Emerald Break"), ("XY7", "Bandit Ring"),
    ("CP2", "Legendary Holo Collection"), ("XY8b", "Blue Shock"), ("XY8a", "Red Flash"), ("XY9", "Rage of the Broken Heavens"), ("CP3", "PokéKyun Collection"),
    ("XY10", "Awakening Psychic King"), ("CP4", "Premium Champion Pack"), ("XY11a", "Fever-Burst Fighter"), ("XY11b", "Cruel Traitor"),
    ("CP5", "Mythical & Legendary Dream Holo Collection"), ("CP6", "20th Anniversary"),
    ("SM0", "Pikachu's New Friends"), ("SM1M", "Collection Moon"), ("SM1S", "Collection Sun"), ("SM1+", "Sun & Moon Strengthening Expansion"),
    ("SM2L", "Alolan Moonlight"), ("SM2K", "Islands Await You"), ("sm2+", "Facing a New Trial"), ("SM3N", "Darkness that Consumes Light"),
    ("SM3H", "To Have Seen the Battle Rainbow"), ("SM3+", "Shining Legends"), ("SM4A", "Ultradimensional Beasts"), ("SM4S", "Awakened Heroes"),
    ("SM4+", "GX Battle Boost"), ("SM5M", "Ultra Moon"), ("SM5S", "Ultra Sun"), ("SM5+", "Ultra Force"), ("SM6", "Forbidden Light"),
    ("SM6a", "Dragon Storm"), ("SM6b", "Champion Road"), ("SM7", "Sky-Splitting Charisma"), ("SM7a", "Thunderclap Spark"), ("SM7b", "Fairy Rise"),
    ("SM8", "Super-Burst Impact"), ("SM8b", "GX Ultra Shiny"), ("SM8a", "Dark Order"), ("SM9", "Tag Bolt"), ("SM9a", "Night Unison"),
    ("SM9b", "Full Metal Wall"), ("SM10", "Double Blaze"), ("SM10a", "GG End"), ("SM10b", "Sky Legends"), ("SMP2", "Detective Pikachu"),
    ("SM11", "Miracle Twin"), ("SM11a", "Remix Bout"), ("SM11b", "Dream League"), ("SM12", "Alter Genesis"), ("SM12a", "Tag Team GX All Stars"),
    ("S1H", "Shield"), ("S1W", "Sword"), ("S1a", "VMAX Rising"), ("S2", "Rebellion Crash"), ("S2a", "Explosive Walker"), ("S3", "Infinity Zone"),
    ("S3a", "Legendary Heartbeat"), ("S4", "Amazing Volt Tackle"), ("S4a", "Shiny Star V"), ("S5I", "Single Strike Master"), ("S5R", "Rapid Strike Master"),
    ("S5a", "Matchless Fighters"), ("S6H", "Silver Lance"), ("S6K", "Jet-Black Spirit"), ("S6a", "Eevee Heroes"), ("S7D", "Skyscraping Perfection"),
    ("S7R", "Blue Sky Stream"), ("S8", "Fusion Arts"), ("S8a", "25th Anniversary Collection"), ("S8b", "VMAX Climax"), ("S9", "Star Birth"),
    ("S9a", "Battle Region"), ("S10D", "Time Gazer"), ("S10P", "Space Juggler"), ("S10a", "Dark Phantasma"), ("S10b", "Pokémon GO"),
    ("S11", "Lost Abyss"), ("S11a", "Incandescent Arcana"), ("S12", "Paradigm Trigger"), ("S12a", "VSTAR Universe"),
    ("SV1S", "Scarlet ex"), ("SV1V", "Violet ex"), ("SV1a", "Triplet Beat"), ("SV2P", "Snow Hazard"), ("SV2D", "Clay Burst"), ("SV2a", "Pokémon Card 151"),
    ("SV3", "Ruler of the Black Flame"), ("SV3a", "Raging Surf"), ("SV4K", "Ancient Roar"), ("SV4M", "Future Flash"), ("SV4a", "Shiny Treasure ex"),
    ("SV5K", "Wild Force"), ("SV5M", "Cyber Judge"), ("SV5a", "Crimson Haze"), ("SV6", "Mask of Change"), ("SV6a", "Night Wanderer"),
    ("SV7", "Stellar Miracle"), ("SV7a", "Paradise Dragona"), ("SV8", "Super Electric Breaker"), ("SV8a", "Terastal Festival ex"),
    ("SV9", "Battle Partners"), ("SV9a", "Hot Wind Arena"), ("SV10", "The Glory of Team Rocket"), ("SV11B", "Black Bolt"), ("SV11W", "White Flare"),
    ("SV-P", "Scarlet & Violet Promos"), ("M1L", "Mega Brave"), ("M1S", "Mega Symphonia"), ("M2", "Inferno X"), ("M2a", "Mega Dream ex"), ("M-P", "Mega Promos"),
];

/// A Japanese set's English name, and the English-language set it matches if that's not already said: "Terastal Festival ex · Prismatic Evolutions".
fn ja_english(id: &str, en_sets: &SetInfo) -> Option<String> {
    let own = JA_EN.iter().find(|(a, _)| *a == id).map(|t| t.1);
    let twin = en_twin(id).and_then(|t| en_sets.get(t)).map(|t| t.name.as_str());
    match (own, twin) {
        (Some(o), Some(t)) if !o.contains(t) => Some(format!("{o} · {t}")),
        (Some(o), _) => Some(o.to_string()),
        (None, Some(t)) => Some(t.to_string()),
        (None, None) => None,
    }
}

type SetCache = std::collections::HashMap<&'static str, (Instant, std::sync::Arc<SetInfo>)>;
static SETS: Mutex<Option<SetCache>> = Mutex::const_new(None);

/// Every set in one catalogue, refreshed once a day. On failure the last good list is used.
async fn sets(client: &reqwest::Client, lang: &str) -> std::sync::Arc<SetInfo> {
    let lang = lang_of(lang);
    let mut g = SETS.lock().await;
    let cache = g.get_or_insert_with(Default::default);
    if let Some((t, s)) = cache.get(lang) {
        if t.elapsed() < Duration::from_secs(86_400) {
            return s.clone();
        }
    }
    let stale = cache.get(lang).map(|(_, s)| s.clone());
    match get_json(client, &format!("{}/sets", dex_base(lang)), &[], false).await {
        Ok(v) => {
            let map: SetInfo = v
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    let id = s["id"].as_str()?.to_string();
                    let count = s["cardCount"]["total"].as_i64().unwrap_or(0);
                    let total = s["cardCount"]["official"].as_i64().filter(|n| *n > 0).unwrap_or(count);
                    let txt = |k: &str| s[k].as_str().unwrap_or("").to_string();
                    Some((id, SetRow { order: i, name: txt("name"), total, count, logo: txt("logo"), symbol: txt("symbol") }))
                })
                .collect();
            let arc = std::sync::Arc::new(map);
            cache.insert(lang, (Instant::now(), arc.clone()));
            arc
        }
        Err(_) => stale.unwrap_or_default(),
    }
}

const LIMITLESS_JP: &str = "https://limitlesstcg.nyc3.cdn.digitaloceanspaces.com/tpc";

/// Pictures always come from the card's own language. TCGdex has no picture for most Japanese cards; for those,
/// Limitless TCG's scan of the same set code and number is used (same printed card, e.g. SV8 #57 = SV8-057).
/// Promo sets ("SV-P") aren't on Limitless under the same code, so those stay without a picture.
fn pictures(lang: &str, raw: &str, local: &str, img: &str) -> (Value, Value) {
    if !img.is_empty() {
        return (json!(format!("{img}/low.webp")), json!(format!("{img}/high.png")));
    }
    if lang == "ja" {
        let set = raw.strip_suffix(&format!("-{local}")).unwrap_or("");
        let num = local.trim_start_matches('0');
        if !set.is_empty() && !set.contains("-P") && !num.is_empty() && num.chars().all(|c| c.is_ascii_alphanumeric()) {
            return (json!(format!("{LIMITLESS_JP}/{set}/{set}_{num}_R_JP_SM.png")), json!(format!("{LIMITLESS_JP}/{set}/{set}_{num}_R_JP_LG.png")));
        }
    }
    (Value::Null, Value::Null)
}

fn card_json(lang: &str, c: &Value, local: &str, row: &SetRow, en: Option<&str>) -> Option<Value> {
    let raw = c["id"].as_str()?;
    let img = c["image"].as_str().unwrap_or("");
    if img.contains("/tcgp/") {
        return None; // Pokémon TCG Pocket (the phone game) has no physical cards
    }
    let (small, large) = pictures(lang, raw, local, img);
    Some(json!({
        "id": card_id(lang, raw), "name": c["name"], "number": local, "set": row.name, "total": row.total, "lang": lang,
        "en": en, "image": small, "image_large": large,
    }))
}

/// Splits "pikachu 57/191" into ("pikachu", Some("57")). A lone number stays part of the name ("Porygon2" style names are safe).
fn name_and_number(q: &str) -> (String, Option<String>) {
    let q: String = q.chars().filter(|c| !matches!(c, '"' | '\\' | '*' | '%' | '&' | '?' | '#')).collect();
    let mut words: Vec<&str> = q.split_whitespace().collect();
    let mut number = None;
    if let Some(last) = words.last() {
        let head = last.split('/').next().unwrap_or("");
        if words.len() > 1 && head.chars().any(|c| c.is_ascii_digit()) && head.chars().all(|c| c.is_ascii_alphanumeric()) {
            let n = head.trim_start_matches('0').to_lowercase();
            number = Some(if n.is_empty() { "0".into() } else { n });
            words.pop();
        }
    }
    (words.join(" "), number)
}

/// Every printing whose name matches, newest sets first, cards with pictures first, each with its official picture. No cap.
/// A trailing number narrows it down: "slaking 170" or "pikachu 57/191". Prices are looked up when a card is tapped.
/// Japanese / Chinese: type the English name ("Snorlax", "Ultra Ball") — it is translated — or the name in that language.
pub async fn catalog(client: &reqwest::Client, q: &str, lang: &str) -> Result<Value, String> {
    let (name, number) = name_and_number(q);
    if name.chars().count() < 2 {
        return Ok(json!({ "cards": [] }));
    }
    let num_ok = |local: &str| number.as_ref().map_or(true, |n| local.trim_start_matches('0').to_lowercase() == *n);
    let mut cards: Vec<(bool, usize, String, Value)> = vec![];
    let cats = catalogues(lang);
    if cats == ["en"] {
        let list = get_json(client, &format!("{}/cards", dex_base("en")), &[("name", name.as_str())], false).await?;
        let sets = sets(client, "en").await;
        for c in list.as_array().into_iter().flatten() {
            let (Some(raw), local) = (c["id"].as_str(), c["localId"].as_str().unwrap_or("")) else { continue };
            let set_id = raw.strip_suffix(&format!("-{local}")).unwrap_or(raw);
            if !num_ok(local) || pocket_set(set_id) {
                continue;
            }
            let row = sets.get(set_id).cloned().unwrap_or_default();
            if let Some(j) = card_json("en", c, local, &row, None) {
                cards.push((j["image"].is_string(), row.order, local.to_string(), j));
            }
        }
    } else {
        let names = names(client).await;
        let typed_native = !name.is_ascii();
        let wanted = name.to_lowercase();
        // English typed: every Pokémon/item whose English name contains it (exact names first).
        let mut rows: Vec<&NameRow> = if typed_native { vec![] } else { names.iter().filter(|r| r.en_lower.contains(&wanted)).collect() };
        if rows.iter().any(|r| r.en_lower == wanted) {
            rows.retain(|r| r.en_lower == wanted);
        }
        if !typed_native && rows.is_empty() {
            return Ok(json!({ "cards": [], "hint": "no-translation" }));
        }
        for cat in cats {
            let (all, sets) = tokio::join!(all_cards(client, cat), sets(client, cat));
            let Ok(all) = all else { continue };
            for c in all.iter() {
                let (Some(raw), Some(cname)) = (c["id"].as_str(), c["name"].as_str()) else { continue };
                let local = c["localId"].as_str().unwrap_or("");
                if !num_ok(local) {
                    continue;
                }
                let hit = if typed_native {
                    cname.contains(name.as_str())
                } else {
                    rows.iter().any(|r| native(r, cat).iter().any(|n| n.chars().count() >= 2 && cname.contains(n.as_str())))
                };
                if !hit {
                    continue;
                }
                // "Mew" must not bring in Mewtwo (ミュウ is inside ミュウツー), but tag teams like "Pikachu & Zekrom" stay.
                if !typed_native && !rows.iter().any(|r| native(r, cat).iter().any(|n| n.chars().count() >= 2 && cname.contains(n.as_str()) && !shadowed(n, cname, cat, &names))) {
                    continue;
                }
                let label = rows
                    .iter()
                    .find(|r| native(r, cat).iter().any(|n| cname.contains(n.as_str())))
                    .copied()
                    .or_else(|| english_label(cname, cat, &names));
                let set_id = raw.strip_suffix(&format!("-{local}")).unwrap_or(raw);
                if pocket_set(set_id) {
                    continue;
                }
                let row = sets.get(set_id).cloned().unwrap_or_default();
                if let Some(j) = card_json(cat, c, local, &row, label.map(|l| l.en.as_str())) {
                    cards.push((j["image"].is_string(), row.order, local.to_string(), j));
                }
            }
        }
    }
    // Cards with a picture first (Japanese and Chinese catalogues are missing some), then newest set first.
    cards.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then_with(|| a.2.cmp(&b.2)));
    let mut cards: Vec<Value> = cards.into_iter().map(|c| c.3).collect();
    // A Wizards-era card is listed once per printing (1st Edition, Unlimited...), the same as in Add by set.
    let vintage = |c: &Value| c["id"].as_str().and_then(|i| i.strip_prefix("tcgdex:")).and_then(|i| i.rsplit_once('-')).is_some_and(|(set, _)| EDITION_SETS.contains(&set));
    if lang == "en" && cards.iter().any(vintage) {
        let mut out = vec![];
        let mut run: Vec<Value> = vec![];
        for c in cards.drain(..) {
            if vintage(&c) { run.push(c); } else { out.extend(split_printings(client, std::mem::take(&mut run)).await); out.push(c); }
        }
        out.extend(split_printings(client, run).await);
        cards = out;
    }
    Ok(json!({ "cards": cards, "lang": lang }))
}

// ---------------- add by set ----------------
fn set_json(cat: &str, id: &str, s: &SetRow, en_name: Option<&str>) -> Value {
    json!({
        "id": id, "cat": cat, "name": s.name, "total": s.total, "count": s.count, "en_name": en_name,
        "logo": if s.logo.is_empty() { Value::Null } else { json!(format!("{}.webp", s.logo)) },
        "symbol": if s.symbol.is_empty() { Value::Null } else { json!(format!("{}.webp", s.symbol)) },
    })
}

/// Sets whose name (or code, like "sv08") contains every word typed, newest first. Empty search: every set. No cap.
/// Japanese / Chinese sets also match their English counterpart's name ("Surging Sparks" finds 超電ブレイカー).
pub async fn set_search(client: &reqwest::Client, q: &str, lang: &str) -> Value {
    // "pokemon" finds "Pokémon", and "base set" finds "Expansion Pack (Base Set)".
    let plain = |t: &str| t.to_lowercase().replace('é', "e");
    let words: Vec<String> = plain(q).split_whitespace().map(str::to_string).collect();
    let en_sets = sets(client, "en").await;
    let mut hits: Vec<(usize, Value)> = vec![];
    let mut ready = false;
    for (k, cat) in catalogues(lang).iter().enumerate() {
        let all = sets(client, cat).await;
        ready |= !all.is_empty();
        for (id, s) in all.iter() {
            if pocket_set(id) || s.count == 0 {
                continue;
            }
            let twin: Option<String> = match *cat {
                "en" => None,
                "ja" => ja_english(id, &en_sets),
                _ => en_twin(id).and_then(|t| en_sets.get(t)).map(|t| t.name.clone()),
            };
            let twin = twin.as_deref();
            let hay = plain(&format!("{} {} {}", s.name, id, twin.unwrap_or("")));
            if words.iter().all(|w| hay.contains(w.as_str())) {
                // Newest first; with two Chinese catalogues, interleave by their own order.
                hits.push((s.order * 2 + (1 - k.min(1)), set_json(cat, id, s, twin)));
            }
        }
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0));
    json!({ "sets": hits.into_iter().map(|h| h.1).collect::<Vec<_>>(), "ready": ready, "lang": lang })
}

// ---------------- printings: 1st Edition, Shadowless, Unlimited ----------------
/// Sets printed in both 1st Edition and Unlimited (Wizards of the Coast, 1999–2002). TCGdex lists each card once,
/// so here every card is split into each printing there was, so the right one can be added.
pub const EDITION_SETS: [&str; 10] = ["base1", "base2", "base3", "base5", "gym1", "gym2", "neo1", "neo2", "neo3", "neo4"];

#[derive(Clone, Debug, PartialEq)]
pub struct Printing {
    /// Short and stable, for ids: "1st-edition", "unlimited", "shadowless".
    pub key: String,
    /// For people: "1st Edition", "Unlimited", "Shadowless, red cheeks".
    pub label: String,
    /// The finish to start with: 1stEditionHolofoil, holofoil, 1stEditionNormal, normal, reverseHolofoil,
    /// or the name the card is really priced under ("1st-edition", "unlimited-holofoil"; see `finish_aliases`).
    pub finish: &'static str,
    pub first: bool,
}

fn pretty_words(t: &str) -> String {
    t.split(['-', '_']).filter(|w| !w.is_empty()).map(|w| {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
    }).collect::<Vec<_>>().join(" ")
}

/// Every printing of one card, from TCGdex's detailed variants. Empty when it only had one.
pub fn printings_of(card: &Value) -> Vec<Printing> {
    let mut out: Vec<Printing> = vec![];
    // The finish names this card is priced under, so each printing starts on one that has a price.
    let priced: Vec<&str> = card["pricing"]["tcgplayer"].as_object().map(|o| o.iter().filter(|(_, v)| v.is_object()).map(|(k, _)| k.as_str()).collect()).unwrap_or_default();
    for v in card["variants_detailed"].as_array().cloned().unwrap_or_default() {
        let kind = v["type"].as_str().unwrap_or("normal");
        let sub = v["subtype"].as_str().unwrap_or("unlimited");
        let stamps: Vec<&str> = v["stamp"].as_array().map(|a| a.iter().filter_map(|s| s.as_str()).collect()).unwrap_or_default();
        let first = stamps.contains(&"1st-edition");
        let mut label = if first {
            match sub {
                "unlimited" | "shadowless" => "1st Edition".to_string(),
                "shadowless-red-cheek" => "1st Edition, red cheeks".to_string(),
                other => format!("1st Edition, {}", pretty_words(other).to_lowercase()),
            }
        } else {
            match sub {
                "unlimited" => "Unlimited".to_string(),
                "shadowless" => "Shadowless".to_string(),
                "shadowless-red-cheek" => "Shadowless, red cheeks".to_string(),
                "1999-2000-copyright" => "Unlimited, 1999–2000 copyright".to_string(),
                "missing-expansion-symbol" => "Unlimited, no set symbol".to_string(),
                other => pretty_words(other),
            }
        };
        for s in stamps.iter().filter(|s| **s != "1st-edition") {
            // Misprints are named for what they are; other stamps by their name ("Prerelease stamp").
            if let Some(err) = s.strip_suffix("-error") {
                label = format!("{} error", pretty_words(err));
            } else if *s == "pre-release" {
                label.push_str(", Prerelease stamp");
            } else {
                label.push_str(&format!(", {} stamp", pretty_words(s)));
            }
        }
        let finish = match (kind, first) {
            ("holo", true) => "1stEditionHolofoil",
            ("holo", false) => "holofoil",
            ("reverse", _) => "reverseHolofoil",
            (_, true) => "1stEditionNormal",
            _ => "normal",
        };
        let finish = if priced.contains(&finish) { finish } else { finish_aliases(finish).iter().copied().find(|a| priced.contains(a)).unwrap_or(finish) };
        let key: String = label.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>().split('-').filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-");
        if !out.iter().any(|p| p.key == key) {
            out.push(Printing { key, label, finish, first });
        }
    }
    // 1st Edition first, the way collectors list them; then the rest as TCGdex has them.
    out.sort_by_key(|p| !p.first);
    if out.len() < 2 { vec![] } else { out }
}

type PrintCache = std::collections::HashMap<String, (Instant, std::sync::Arc<Vec<Printing>>)>;
static PRINTS: Mutex<Option<PrintCache>> = Mutex::const_new(None);

/// One card's printings, looked up once a day.
async fn printings(client: &reqwest::Client, id: &str) -> std::sync::Arc<Vec<Printing>> {
    if let Some((t, p)) = PRINTS.lock().await.get_or_insert_with(Default::default).get(id) {
        if t.elapsed() < Duration::from_secs(86_400) {
            return p.clone();
        }
    }
    let got = match get_json(client, &format!("{}/cards/{id}", dex_base("en")), &[], false).await {
        Ok(v) => std::sync::Arc::new(printings_of(&v)),
        Err(_) => return std::sync::Arc::new(vec![]),
    };
    PRINTS.lock().await.get_or_insert_with(Default::default).insert(id.to_string(), (Instant::now(), got.clone()));
    got
}

/// The Unlimited picture of a vintage card: the 1st Edition scan with its seal painted out (see stamp.rs).
fn unlimited_picture(large: &str, size: &str) -> Value {
    let src = large.replace(':', "%3A").replace('/', "%2F");
    json!(format!("/admin/cardimg/unlimited?size={size}&src={src}"))
}

/// Splits each card of a 1st Edition set into its printings, each with its own label, finish and picture.
async fn split_printings(client: &reqwest::Client, cards: Vec<Value>) -> Vec<Value> {
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(24));
    let mut jobs = tokio::task::JoinSet::new();
    for (i, c) in cards.iter().enumerate() {
        let raw = c["id"].as_str().unwrap_or("").trim_start_matches("tcgdex:").to_string();
        let (client, gate) = (client.clone(), gate.clone());
        jobs.spawn(async move {
            let _p = gate.acquire_owned().await;
            (i, printings(&client, &raw).await)
        });
    }
    let mut found = vec![std::sync::Arc::new(vec![]); cards.len()];
    while let Some(Ok((i, p))) = jobs.join_next().await {
        found[i] = p;
    }
    let mut out = vec![];
    for (c, prints) in cards.into_iter().zip(found) {
        if prints.is_empty() {
            out.push(c);
            continue;
        }
        let api = c["id"].as_str().unwrap_or("").to_string();
        let large = c["image_large"].as_str().unwrap_or("").to_string();
        for p in prints.iter() {
            let mut v = c.clone();
            v["id"] = json!(format!("{api}#{}", p.key));
            v["api"] = json!(api);
            v["edition"] = json!(p.label);
            v["finish"] = json!(p.finish);
            // The scans are of 1st Edition cards; every other printing shows the same card with the seal taken off.
            if !p.first && large.starts_with("https://assets.tcgdex.net/") {
                v["image"] = unlimited_picture(&large, "low");
                v["image_large"] = unlimited_picture(&large, "high");
            }
            out.push(v);
        }
    }
    out
}

/// Every card in one set, in card-number order, each with its official picture (and English name for Japanese / Chinese).
/// One set's name, series and release date ("2025-11-14"), for sorting cards by set.
pub async fn set_info(client: &reqwest::Client, code: &str, lang: &str) -> Option<(String, String, String)> {
    if code.is_empty() || code.len() > 40 || !code.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.')) {
        return None;
    }
    let v = get_json(client, &format!("{}/sets/{code}", dex_base(lang)), &[], false).await.ok()?;
    let txt = |x: &Value| x.as_str().unwrap_or("").to_string();
    Some((txt(&v["name"]), txt(&v["serie"]["name"]), txt(&v["releaseDate"])))
}

/// The set code for a set name ("Surging Sparks" → "sv08"), from the catalogue's set list.
pub async fn set_code_for_name(client: &reqwest::Client, name: &str, lang: &str) -> Option<String> {
    let all = sets(client, lang).await;
    let want = name.trim().to_lowercase();
    all.iter().find(|(_, row)| row.name.trim().to_lowercase() == want).map(|(id, _)| id.clone())
}

pub async fn set_cards(client: &reqwest::Client, id: &str, cat: &str) -> Result<Value, String> {
    let cat = catalogues(cat).iter().copied().find(|c| *c == cat).unwrap_or(catalogues(cat)[0]);
    if id.is_empty() || id.len() > 40 || !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.')) || pocket_set(id) {
        return Ok(json!({ "cards": [] }));
    }
    let url = format!("{}/sets/{id}", dex_base(cat));
    let (v, all) = tokio::join!(get_json(client, &url, &[], false), sets(client, cat));
    let v = v?;
    let row = all.get(id).cloned().unwrap_or_else(|| SetRow {
        name: v["name"].as_str().unwrap_or("").to_string(),
        total: v["cardCount"]["official"].as_i64().unwrap_or(0),
        count: v["cardCount"]["total"].as_i64().unwrap_or(0),
        ..Default::default()
    });
    let names = if cat == "en" { Default::default() } else { names(client).await };
    let twin = if cat == "en" { None } else { en_twin(id) };
    let en_sets = if cat != "en" { sets(client, "en").await } else { Default::default() };
    let twin_name = if cat == "ja" { ja_english(id, &en_sets) } else { twin.and_then(|t| en_sets.get(t)).map(|s| s.name.clone()) };
    let cards: Vec<Value> = v["cards"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|c| {
            let label = if cat == "en" { None } else { c["name"].as_str().and_then(|n| english_label(n, cat, &names)).map(|l| l.en.as_str()) };
            card_json(cat, c, c["localId"].as_str().unwrap_or(""), &row, label)
        })
        .collect();
    let cards = if cat == "en" && EDITION_SETS.contains(&id) { split_printings(client, cards).await } else { cards };
    Ok(json!({ "set": set_json(cat, id, &row, twin_name.as_deref()), "cards": cards, "lang": cat }))
}

#[cfg(test)]
mod printing_tests {
    use super::*;

    #[test]
    fn a_base_set_card_comes_in_each_printing() {
        // Charizard, Base Set: as TCGdex lists it.
        let charizard = json!({ "variants_detailed": [
            { "type": "holo", "subtype": "unlimited", "stamp": null },
            { "type": "holo", "subtype": "shadowless", "stamp": ["1st-edition"] },
            { "type": "holo", "subtype": "shadowless", "stamp": null },
            { "type": "holo", "subtype": "1999-2000-copyright", "stamp": null } ] });
        let p = printings_of(&charizard);
        let labels: Vec<&str> = p.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(labels, ["1st Edition", "Unlimited", "Shadowless", "Unlimited, 1999–2000 copyright"]);
        assert_eq!(p[0].finish, "1stEditionHolofoil");
        assert_eq!(p[1].finish, "holofoil");
        assert!(p[0].first && !p[1].first);
        assert_eq!(p[0].key, "1st-edition");
    }

    #[test]
    fn printings_start_on_a_finish_with_a_price() {
        // Jungle Pikachu: TCGdex prices Wizards-era cards under its own names.
        let pikachu = json!({
            "variants_detailed": [
                { "type": "normal", "subtype": "unlimited", "stamp": null },
                { "type": "normal", "subtype": "unlimited", "stamp": ["1st-edition"] } ],
            "pricing": { "tcgplayer": { "updated": "2026-11-01", "1st-edition": { "marketPrice": 30.0 }, "unlimited": { "marketPrice": 5.0 } } } });
        let p = printings_of(&pikachu);
        assert_eq!(p.iter().map(|x| x.finish).collect::<Vec<_>>(), ["1st-edition", "unlimited"]);
        // Base Set keeps the plain names, because that's what it's priced under.
        let base = json!({
            "variants_detailed": [
                { "type": "holo", "subtype": "unlimited", "stamp": null },
                { "type": "holo", "subtype": "shadowless", "stamp": null } ],
            "pricing": { "tcgplayer": { "holofoil": { "marketPrice": 300.0 } } } });
        let p = printings_of(&base);
        assert_eq!(p.iter().map(|x| x.finish).collect::<Vec<_>>(), ["holofoil", "holofoil"]);
        assert!(same_finish("normal", "unlimited") && same_finish("1st-edition-holofoil", "1stEditionHolofoil"));
        assert!(!same_finish("normal", "1st-edition") && !same_finish("holofoil", "1st-edition-holofoil"));
        assert_eq!(label("unlimited-holofoil"), "Unlimited Holo");
    }

    #[test]
    fn the_same_printing_listed_twice_is_one() {
        // Pikachu, Base Set: "unlimited" and a plain entry are the same printing; a stamped promo is its own.
        let pikachu = json!({ "variants_detailed": [
            { "type": "normal", "subtype": "unlimited", "stamp": null },
            { "type": "normal", "subtype": "shadowless", "stamp": ["1st-edition"] },
            { "type": "normal", "subtype": "shadowless-red-cheek", "stamp": ["1st-edition"] },
            { "type": "normal", "subtype": null, "stamp": null },
            { "type": "normal", "subtype": "unlimited", "stamp": ["poketour-99"] } ] });
        let labels: Vec<String> = printings_of(&pikachu).into_iter().map(|x| x.label).collect();
        assert_eq!(labels, ["1st Edition", "1st Edition, red cheeks", "Unlimited", "Unlimited, Poketour 99 stamp"]);
        let odd = json!({ "variants_detailed": [
            { "type": "holo", "subtype": null, "stamp": null },
            { "type": "holo", "subtype": null, "stamp": ["pre-release"] },
            { "type": "holo", "subtype": null, "stamp": ["d-edition-error"] } ] });
        let labels: Vec<String> = printings_of(&odd).into_iter().map(|x| x.label).collect();
        assert_eq!(labels, ["Unlimited", "Unlimited, Prerelease stamp", "D Edition error"]);
    }

    #[test]
    fn a_card_with_one_printing_is_left_whole() {
        assert!(printings_of(&json!({ "variants_detailed": [{ "type": "holo", "subtype": null, "stamp": null }] })).is_empty());
        assert!(printings_of(&json!({})).is_empty());
    }
}
