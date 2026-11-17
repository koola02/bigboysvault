//! A binder's cards put in order with one tap: A to Z, Z to A, by set, by card number, or by evolution.
//! The order is worked out here, in plain functions tested without the internet; the binder's own arrange code saves it
//! (`members::order_binder_cards` for a seller, `db::arrange_binder` for the vault), the same as dragging does.
//! "By evolution" needs each card's Pokédex number, stage and what it evolves from. Those come from TCGdex once and are
//! kept in `card_dex`; a card TCGdex didn't know is asked about again after a week.
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Duration;

/// What TCGdex told us about a card. A card it doesn't know has a row too (all empty), so it isn't asked every time.
pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS card_dex(api_id TEXT PRIMARY KEY, dex INTEGER NOT NULL DEFAULT 0, stage TEXT NOT NULL DEFAULT '',
  evolve_from TEXT NOT NULL DEFAULT '', category TEXT NOT NULL DEFAULT '', at INTEGER NOT NULL);";

/// How the cards are put in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    Az,
    Za,
    Set,
    Number,
    Evolution,
}

impl By {
    /// "az", "za", "set", "number" or "evolution".
    pub fn parse(s: &str) -> Option<By> {
        Some(match s.trim() {
            "az" => By::Az,
            "za" => By::Za,
            "set" => By::Set,
            "number" => By::Number,
            "evolution" => By::Evolution,
            _ => return None,
        })
    }
}

/// One card of a binder, as the sorting sees it.
#[derive(Clone, Debug, Default)]
pub struct SortCard {
    pub id: i64,
    pub name: String,
    pub set: String,
    /// The set's release date ("1999-01-09"); "" when unknown.
    pub released: String,
    /// The collector number as printed ("4/102", "SV001", "TG05").
    pub number: String,
    /// The catalogue id ("tcgdex:base1-4", "tcgdex-ja:SV1a-001#2", "base1-4"); "" or "own:…" for none.
    pub api_id: String,
}

/// A card's place among Pokémon: its Pokédex number (0 when it has none), stage, what it evolves from, and whether it is a
/// Pokémon, a Trainer or an Energy (TCGdex's words).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dex {
    pub dex: i64,
    pub stage: String,
    pub evolve_from: String,
    pub category: String,
}

/// The cards' ids in the chosen order, first to last. `dex` is only used by evolution (keyed by `dex_key`).
pub fn order(cards: &[SortCard], by: By, dex: &HashMap<String, Dex>) -> Vec<i64> {
    let mut idx: Vec<usize> = (0..cards.len()).collect();
    match by {
        By::Az => idx.sort_by(|&a, &b| by_name(&cards[a], &cards[b]).then_with(|| by_set(&cards[a], &cards[b])).then_with(|| by_number(&cards[a], &cards[b])).then(cards[a].id.cmp(&cards[b].id))),
        By::Za => idx.sort_by(|&a, &b| by_name(&cards[b], &cards[a]).then_with(|| by_set(&cards[a], &cards[b])).then_with(|| by_number(&cards[a], &cards[b])).then(cards[a].id.cmp(&cards[b].id))),
        By::Set => idx.sort_by(|&a, &b| by_set(&cards[a], &cards[b]).then_with(|| by_number(&cards[a], &cards[b])).then_with(|| by_name(&cards[a], &cards[b])).then(cards[a].id.cmp(&cards[b].id))),
        By::Number => idx.sort_by(|&a, &b| {
            num_cmp(number_part(&cards[a].number), number_part(&cards[b].number))
                .then_with(|| by_set(&cards[a], &cards[b]))
                .then_with(|| by_number(&cards[a], &cards[b]))
                .then_with(|| by_name(&cards[a], &cards[b]))
                .then(cards[a].id.cmp(&cards[b].id))
        }),
        By::Evolution => {
            let keys = evolution_keys(cards, dex);
            idx.sort_by(|&a, &b| keys[a].cmp(&keys[b]).then_with(|| by_name(&cards[a], &cards[b])).then_with(|| by_set(&cards[a], &cards[b])).then_with(|| by_number(&cards[a], &cards[b])).then(cards[a].id.cmp(&cards[b].id)));
        }
    }
    idx.into_iter().map(|i| cards[i].id).collect()
}

/// The number part of a collector number: "4/102" → 4, "SV001" → 1, "TG05" → 5. None when it has no digits.
pub fn number_part(n: &str) -> Option<u64> {
    let head = n.split('/').next().unwrap_or("");
    let digits: String = head.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        // "000" is 0; no digits at all is no number.
        return head.chars().any(|c| c.is_ascii_digit()).then_some(0);
    }
    digits.get(..digits.len().min(18)).and_then(|d| d.parse().ok())
}

fn num_cmp(a: Option<u64>, b: Option<u64>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn by_name(a: &SortCard, b: &SortCard) -> Ordering {
    a.name.trim().to_lowercase().cmp(&b.name.trim().to_lowercase())
}

/// Oldest set first by its release date; sets with no known date last, by name.
fn by_set(a: &SortCard, b: &SortCard) -> Ordering {
    let (ra, rb) = (a.released.trim(), b.released.trim());
    ra.is_empty().cmp(&rb.is_empty()).then_with(|| ra.cmp(rb)).then_with(|| a.set.trim().to_lowercase().cmp(&b.set.trim().to_lowercase()))
}

/// By the number part, then as written ("5" before "TG05").
fn by_number(a: &SortCard, b: &SortCard) -> Ordering {
    num_cmp(number_part(&a.number), number_part(&b.number)).then_with(|| a.number.trim().to_lowercase().cmp(&b.number.trim().to_lowercase()))
}

/// Basic, Stage 1, Stage 2, then any other stage (TCGdex's words in English, Japanese and Chinese).
pub fn stage_rank(stage: &str) -> u8 {
    let s: String = stage.trim().to_lowercase().chars().filter(|c| !matches!(c, ' ' | '-' | '_')).collect();
    match s.as_str() {
        "basic" | "たね" | "基礎" | "基础" => 0,
        "stage1" | "1進化" | "1階進化" | "1阶进化" => 1,
        "stage2" | "2進化" | "2階進化" | "2阶进化" => 2,
        _ => 3,
    }
}

fn is_trainer(category: &str) -> bool {
    let c = category.to_lowercase();
    ["trainer", "トレーナー", "訓練家", "训练家"].iter().any(|w| c.contains(w))
}

fn is_energy(category: &str) -> bool {
    let c = category.to_lowercase();
    ["energy", "エネルギー", "能量"].iter().any(|w| c.contains(w))
}

/// A name as it is compared: small letters, no marks ("Farfetch'd" → "farfetchd", "Mr. Mime" → "mr mime").
fn plain_name(s: &str) -> String {
    let kept: String = s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else if c.is_whitespace() || c == '-' || c == '/' { ' ' } else { '\u{0}' }).filter(|c| *c != '\u{0}').collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// (group, family, stage, Pokédex number) for each card. Group: Pokémon 0, Trainers 1, Energy 2, anything unknown 3.
/// A Pokémon's family is the Pokédex number of the earliest card of its line in this binder: what it evolves from is
/// followed through the binder's own cards by name, as far back as they go; with none of them here, its own number.
fn evolution_keys(cards: &[SortCard], dex: &HashMap<String, Dex>) -> Vec<(u8, i64, u8, i64)> {
    let info: Vec<Option<&Dex>> = cards.iter().map(|c| dex.get(&dex_key(&c.api_id))).collect();
    let names: Vec<String> = cards.iter().map(|c| plain_name(&c.name)).collect();
    // The binder's card for a name: the very name first ("Charmeleon"), else a name starting with it ("Charmeleon 24/102").
    // Among several, one we know the Pokédex number of.
    let find = |want: &str, skip: &[usize]| -> Option<usize> {
        let known = |i: &usize| info[*i].is_some_and(|d| d.dex > 0);
        let exact: Vec<usize> = (0..cards.len()).filter(|i| !skip.contains(i) && names[*i] == want).collect();
        let pick = |v: Vec<usize>| v.iter().copied().find(known).or_else(|| v.first().copied());
        pick(exact).or_else(|| {
            let start = format!("{want} ");
            pick((0..cards.len()).filter(|i| !skip.contains(i) && names[*i].starts_with(&start)).collect())
        })
    };
    (0..cards.len())
        .map(|i| {
            let Some(d) = info[i] else { return (3, 0, 3, 0) };
            let mut family = d.dex.max(0);
            let (mut at, mut seen) = (i, vec![i]);
            for _ in 0..6 {
                let from = info[at].map(|x| plain_name(&x.evolve_from)).unwrap_or_default();
                if from.is_empty() {
                    break;
                }
                let Some(j) = find(&from, &seen) else { break };
                seen.push(j);
                if let Some(x) = info[j].filter(|x| x.dex > 0) {
                    family = x.dex;
                }
                at = j;
            }
            if is_trainer(&d.category) {
                (1, 0, 3, 0)
            } else if is_energy(&d.category) {
                (2, 0, 3, 0)
            } else if family > 0 {
                // TCGdex leaves the stage out on some cards (Mewtwo GX): with nothing it evolves from, it's a Basic.
                let stage = if d.stage.trim().is_empty() && d.evolve_from.trim().is_empty() { 0 } else { stage_rank(&d.stage) };
                (0, family, stage, d.dex.max(0))
            } else {
                (3, 0, 3, 0)
            }
        })
        .collect()
}

/// The id a card's details are kept under: its catalogue id without a copy number ("tcgdex:base1-4#2" →
/// "tcgdex:base1-4"). "" when there's nothing to look up (no id, or a trainer's own photo).
pub fn dex_key(api_id: &str) -> String {
    let k = api_id.split('#').next().unwrap_or("").trim();
    if dex_url(k).is_some() { k.to_string() } else { String::new() }
}

/// Where TCGdex keeps a card: "tcgdex:base1-4" (English), "tcgdex-ja:SV1a-001" (Japanese), "tcgdex-zh-tw:…" (Chinese),
/// or a bare "base1-4" (English).
pub fn dex_url(key: &str) -> Option<String> {
    let (lang, raw) = match crate::market::split_card_id(key) {
        Some(x) => x,
        None if !key.contains(':') => ("en", key),
        None => return None,
    };
    let fine = !raw.is_empty() && raw.len() <= 64 && raw.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')) && raw.contains('-');
    fine.then(|| format!("https://api.tcgdex.net/v2/{lang}/cards/{raw}"))
}

/// A TCGdex card's answer, read: `dexId` (the first, for cards with two Pokémon), `stage`, `evolveFrom`, `category`.
pub fn parse_dex(v: &Value) -> Dex {
    let text = |k: &str| v[k].as_str().unwrap_or("").trim().chars().take(60).collect::<String>();
    Dex { dex: v["dexId"].as_array().and_then(|a| a.first()).and_then(Value::as_i64).unwrap_or(0).max(0), stage: text("stage"), evolve_from: text("evolveFrom"), category: text("category") }
}

/// What we know already, for these keys.
pub fn known(c: &Connection, keys: &[String]) -> rusqlite::Result<HashMap<String, Dex>> {
    let mut st = c.prepare_cached("SELECT dex, stage, evolve_from, category FROM card_dex WHERE api_id=?1")?;
    let mut out = HashMap::new();
    for k in keys.iter().filter(|k| !k.is_empty()) {
        if out.contains_key(k) {
            continue;
        }
        if let Some(d) = st.query_row([k], |r| Ok(Dex { dex: r.get(0)?, stage: r.get(1)?, evolve_from: r.get(2)?, category: r.get(3)? })).optional()? {
            out.insert(k.clone(), d);
        }
    }
    Ok(out)
}

/// Keys to ask TCGdex about: never asked, or asked over a week ago without learning anything (no Pokédex number and not a
/// Trainer or Energy). Each once.
pub fn to_learn(c: &Connection, keys: &[String], now: i64) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare_cached("SELECT dex, category, at FROM card_dex WHERE api_id=?1")?;
    let mut out: Vec<String> = vec![];
    for k in keys.iter().filter(|k| !k.is_empty()) {
        if out.contains(k) {
            continue;
        }
        let row: Option<(i64, String, i64)> = st.query_row([k], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
        let stale = match row {
            None => true,
            Some((dex, cat, at)) => dex <= 0 && !is_trainer(&cat) && !is_energy(&cat) && at < now - 7 * 86400,
        };
        if stale {
            out.push(k.clone());
        }
    }
    Ok(out)
}

/// Keep TCGdex's answer for a card. None: TCGdex doesn't know the card (asked again in a week).
pub fn remember(c: &Connection, key: &str, d: Option<&Dex>, now: i64) -> rusqlite::Result<()> {
    let d = d.cloned().unwrap_or_default();
    c.execute(
        "INSERT INTO card_dex(api_id, dex, stage, evolve_from, category, at) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(api_id) DO UPDATE SET dex=?2, stage=?3, evolve_from=?4, category=?5, at=?6",
        rusqlite::params![key, d.dex, d.stage, d.evolve_from, d.category, now],
    )?;
    Ok(())
}

/// At most this many cards are looked up for one sort; the rest are looked up the next time.
pub const LEARN_MAX: usize = 300;

/// Ask TCGdex about these cards: six at a time, 8 seconds each, and nothing new started after 25 seconds. Returns the
/// answers: Some(details), or None for a card TCGdex doesn't know. A card that got no answer (slow, or the service down)
/// is left out, so it isn't remembered as unknown.
pub async fn learn(client: &reqwest::Client, keys: Vec<String>) -> Vec<(String, Option<Dex>)> {
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(6));
    let until = tokio::time::Instant::now() + Duration::from_secs(25);
    let mut jobs = tokio::task::JoinSet::new();
    for key in keys.into_iter().take(LEARN_MAX) {
        let Some(url) = dex_url(&key) else { continue };
        let (client, gate) = (client.clone(), gate.clone());
        jobs.spawn(async move {
            let _turn = gate.acquire_owned().await.ok()?;
            if tokio::time::Instant::now() >= until {
                return None;
            }
            let r = client.get(&url).timeout(Duration::from_secs(8)).send().await.ok()?;
            match r.status().as_u16() {
                404 => Some((key, None)),
                s if (200..300).contains(&s) => {
                    let v: Value = r.json().await.ok()?;
                    Some((key, v.is_object().then(|| parse_dex(&v))))
                }
                _ => None,
            }
        });
    }
    let mut out = vec![];
    while let Some(done) = jobs.join_next().await {
        if let Ok(Some(x)) = done {
            out.push(x);
        }
    }
    out
}

/// A card's set release date, the way "sort by set" finds it: by the set code in its catalogue id ("tcgdex:sv08-053" →
/// "sv08"), else by the set's name, in `card_sets`. "" when unknown.
pub fn released(c: &Connection, api_id: &str, set_name: &str) -> rusqlite::Result<String> {
    let code = crate::db::set_code(api_id.split('#').next().unwrap_or(""));
    c.query_row(
        "SELECT COALESCE((SELECT s.released FROM card_sets s WHERE s.code=?1 AND ?1<>''), (SELECT s.released FROM card_sets s WHERE s.name=?2 COLLATE NOCASE AND ?2<>'' LIMIT 1), '')",
        rusqlite::params![code, set_name.trim()],
        |r| r.get(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: i64, name: &str, set: &str, released: &str, number: &str, api_id: &str) -> SortCard {
        SortCard { id, name: name.into(), set: set.into(), released: released.into(), number: number.into(), api_id: api_id.into() }
    }
    fn dex(n: i64, stage: &str, from: &str, cat: &str) -> Dex {
        Dex { dex: n, stage: stage.into(), evolve_from: from.into(), category: cat.into() }
    }

    #[test]
    fn number_parts() {
        assert_eq!(number_part("4/102"), Some(4));
        assert_eq!(number_part("SV001"), Some(1));
        assert_eq!(number_part("TG05"), Some(5));
        assert_eq!(number_part("058/198"), Some(58));
        assert_eq!(number_part("000"), Some(0));
        assert_eq!(number_part(""), None);
        assert_eq!(number_part("Promo"), None);
        assert_eq!(number_part("99999999999999999999999"), Some(999_999_999_999_999_999), "a silly long number never breaks the sort");
    }

    #[test]
    fn a_to_z_and_back_then_set_then_number() {
        let cards = vec![
            card(1, "pikachu", "Jungle", "1999-06-16", "60/64", ""),
            card(2, "Bulbasaur", "Base Set", "1999-01-09", "44/102", ""),
            card(3, "Pikachu", "Base Set", "1999-01-09", "58/102", ""),
            card(4, "Pikachu", "Base Set", "1999-01-09", "27/102", ""),
            card(5, "Zubat", "", "", "", ""),
        ];
        let none = HashMap::new();
        assert_eq!(order(&cards, By::Az, &none), vec![2, 4, 3, 1, 5], "names ignore capitals; the same name goes by set, then number");
        assert_eq!(order(&cards, By::Za, &none), vec![5, 4, 3, 1, 2], "Z to A turns the names round, not the sets");
    }

    #[test]
    fn by_set_oldest_first_unknown_last() {
        let cards = vec![
            card(1, "A", "Scarlet & Violet", "2023-03-31", "10/198", ""),
            card(2, "B", "Mystery Box", "", "1", ""),
            card(3, "C", "Base Set", "1999-01-09", "58/102", ""),
            card(4, "D", "Base Set", "1999-01-09", "4/102", ""),
            card(5, "E", "Another Box", "", "2", ""),
        ];
        assert_eq!(order(&cards, By::Set, &HashMap::new()), vec![4, 3, 1, 5, 2], "by release date, a set's cards by number, unknown sets last by name");
    }

    #[test]
    fn by_number_then_set() {
        let cards = vec![
            card(1, "A", "Jungle", "1999-06-16", "4/64", ""),
            card(2, "B", "Base Set", "1999-01-09", "4/102", ""),
            card(3, "C", "Crown Zenith", "2023-01-20", "GG01", ""),
            card(4, "D", "Lost Origin", "2022-09-09", "TG05", ""),
            card(5, "E", "Promo", "", "", ""),
            card(6, "F", "Black Star", "", "SV001", ""),
        ];
        assert_eq!(order(&cards, By::Number, &HashMap::new()), vec![3, 6, 2, 1, 4, 5], "1, 1, 4, 4, 5, then no number; the same number goes oldest set first");
    }

    #[test]
    fn by_evolution_keeps_each_line_together() {
        let cards = vec![
            card(1, "Charizard", "Base Set", "1999-01-09", "4/102", "tcgdex:base1-4"),
            card(2, "Professor Oak", "Base Set", "1999-01-09", "88/102", "tcgdex:base1-88"),
            card(3, "Bulbasaur", "Base Set", "1999-01-09", "44/102", "tcgdex:base1-44"),
            card(4, "Charmander", "Base Set", "1999-01-09", "46/102", "tcgdex:base1-46#2"),
            card(5, "Fire Energy", "Base Set", "1999-01-09", "98/102", "tcgdex:base1-98"),
            card(6, "Charmeleon", "Base Set", "1999-01-09", "24/102", "tcgdex:base1-24"),
            card(7, "My own photo", "", "", "", "own:abc"),
            card(8, "Pichu", "Neo Genesis", "2000-12-16", "12/111", "tcgdex:neo1-12"),
            card(9, "Raichu ex", "Some Set", "", "1", "tcgdex-ja:SV1a-001"),
            card(10, "Pikachu", "Sword & Shield", "2020-02-07", "65/202", "tcgdex:swsh1-65"),
            card(11, "Ivysaur", "Base Set", "1999-01-09", "30/102", "tcgdex:base1-30"),
        ];
        let mut d = HashMap::new();
        d.insert("tcgdex:base1-4".to_string(), dex(6, "Stage2", "Charmeleon", "Pokemon"));
        d.insert("tcgdex:base1-88".to_string(), dex(0, "", "", "Trainer"));
        d.insert("tcgdex:base1-44".to_string(), dex(1, "Basic", "", "Pokemon"));
        d.insert("tcgdex:base1-46".to_string(), dex(4, "Basic", "", "Pokemon"));
        d.insert("tcgdex:base1-98".to_string(), dex(0, "", "", "Energy"));
        d.insert("tcgdex:base1-24".to_string(), dex(5, "Stage1", "Charmander", "Pokemon"));
        d.insert("tcgdex:neo1-12".to_string(), dex(172, "Basic", "", "Pokemon"));
        d.insert("tcgdex:swsh1-65".to_string(), dex(25, "Basic", "", "Pokemon"));
        d.insert("tcgdex-ja:SV1a-001".to_string(), dex(26, "Stage1", "Pikachu", "Pokemon"));
        d.insert("tcgdex:base1-30".to_string(), dex(2, "Stage1", "Bulbasaur", "Pokemon"));
        let got = order(&cards, By::Evolution, &d);
        assert_eq!(got, vec![3, 11, 4, 6, 1, 10, 9, 8, 2, 5, 7], "lines in Pokédex order, each from Basic up (a copy's \"#2\" is the same card); Trainers, Energy, then unknown");
    }

    #[test]
    fn a_line_without_its_middle_uses_its_own_number() {
        let cards = vec![card(1, "Charizard", "", "", "", "a-1"), card(2, "Squirtle", "", "", "", "a-2"), card(3, "Charmander", "", "", "", "a-3")];
        let mut d = HashMap::new();
        d.insert("a-1".to_string(), dex(6, "Stage2", "Charmeleon", "Pokemon"));
        d.insert("a-2".to_string(), dex(7, "Basic", "", "Pokemon"));
        d.insert("a-3".to_string(), dex(4, "Basic", "", "Pokemon"));
        assert_eq!(order(&cards, By::Evolution, &d), vec![3, 1, 2]);
    }

    #[test]
    fn a_card_with_no_stage_and_nothing_before_it_is_a_basic() {
        let cards = vec![card(1, "Mewtwo V", "", "", "", "m-1"), card(2, "Mewtwo GX", "", "", "", "m-2"), card(3, "Mewtwo VMAX", "", "", "", "m-3")];
        let mut d = HashMap::new();
        d.insert("m-1".to_string(), dex(150, "Basic", "", "Pokemon"));
        d.insert("m-2".to_string(), dex(150, "", "", "Pokemon"));
        d.insert("m-3".to_string(), dex(150, "VMAX", "Mewtwo V", "Pokemon"));
        assert_eq!(order(&cards, By::Evolution, &d), vec![2, 1, 3], "the Basics by name, then what evolves from them");
    }

    #[test]
    fn a_loop_of_names_never_hangs() {
        let cards = vec![card(1, "Aaa", "", "", "", "x-1"), card(2, "Bbb", "", "", "", "x-2")];
        let mut d = HashMap::new();
        d.insert("x-1".to_string(), dex(10, "Stage1", "Bbb", "Pokemon"));
        d.insert("x-2".to_string(), dex(20, "Stage1", "Aaa", "Pokemon"));
        assert_eq!(order(&cards, By::Evolution, &d).len(), 2);
    }

    #[test]
    fn keys_and_addresses() {
        assert_eq!(dex_key("tcgdex:base1-4#2"), "tcgdex:base1-4");
        assert_eq!(dex_key("own:123"), "");
        assert_eq!(dex_key(""), "");
        assert_eq!(dex_url("tcgdex:base1-4").as_deref(), Some("https://api.tcgdex.net/v2/en/cards/base1-4"));
        assert_eq!(dex_url("tcgdex-ja:SV1a-001").as_deref(), Some("https://api.tcgdex.net/v2/ja/cards/SV1a-001"));
        assert_eq!(dex_url("tcgdex-zh-tw:SV1-001").as_deref(), Some("https://api.tcgdex.net/v2/zh-tw/cards/SV1-001"));
        assert_eq!(dex_url("swsh12.5-001").as_deref(), Some("https://api.tcgdex.net/v2/en/cards/swsh12.5-001"));
        assert_eq!(dex_url("tcgdex:../admin"), None, "only plain card ids go in the address");
        assert_eq!(dex_url("tcgdex-xx:a-1"), None);
    }

    #[test]
    fn reading_an_answer() {
        let v = serde_json::json!({ "id": "base1-4", "name": "Charizard", "dexId": [6], "stage": "Stage2", "evolveFrom": "Charmeleon", "category": "Pokemon" });
        assert_eq!(parse_dex(&v), dex(6, "Stage2", "Charmeleon", "Pokemon"));
        let t = serde_json::json!({ "id": "base1-88", "category": "Trainer" });
        assert_eq!(parse_dex(&t), dex(0, "", "", "Trainer"));
        assert_eq!(stage_rank("Stage1"), 1);
        assert_eq!(stage_rank("2進化"), 2);
        assert_eq!(stage_rank("VMAX"), 3);
    }

    #[test]
    fn remembered_and_asked_again_after_a_week() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA).unwrap();
        c.execute_batch("CREATE TABLE card_sets(code TEXT PRIMARY KEY, name TEXT NOT NULL DEFAULT '', series TEXT NOT NULL DEFAULT '', released TEXT NOT NULL DEFAULT '', fetched INTEGER NOT NULL DEFAULT 0);").unwrap();
        let now = 1_800_000_000;
        let keys: Vec<String> = ["tcgdex:base1-4", "tcgdex:base1-88", "tcgdex:zz-1", "tcgdex:yy-1", "tcgdex:base1-4", ""].iter().map(|s| s.to_string()).collect();
        assert_eq!(to_learn(&c, &keys, now).unwrap(), vec!["tcgdex:base1-4", "tcgdex:base1-88", "tcgdex:zz-1", "tcgdex:yy-1"], "each once, nothing for no id");
        remember(&c, "tcgdex:base1-4", Some(&dex(6, "Stage2", "Charmeleon", "Pokemon")), now).unwrap();
        remember(&c, "tcgdex:base1-88", Some(&dex(0, "", "", "Trainer")), now - 30 * 86400).unwrap();
        remember(&c, "tcgdex:zz-1", None, now - 86400).unwrap();
        remember(&c, "tcgdex:yy-1", None, now - 8 * 86400).unwrap();
        assert_eq!(to_learn(&c, &keys, now).unwrap(), vec!["tcgdex:yy-1"], "a card TCGdex didn't know is asked again after a week; a Trainer never");
        let k = known(&c, &keys).unwrap();
        assert_eq!(k["tcgdex:base1-4"].dex, 6);
        assert_eq!(k.len(), 4);

        c.execute("INSERT INTO card_sets(code, name, released) VALUES('base1', 'Base Set', '1999-01-09'), ('', 'Jungle', '1999-06-16')", []).unwrap();
        assert_eq!(released(&c, "tcgdex:base1-4#2", "").unwrap(), "1999-01-09", "by the set code in the id");
        assert_eq!(released(&c, "", " jungle ").unwrap(), "1999-06-16", "else by the set's name");
        assert_eq!(released(&c, "", "").unwrap(), "");
    }
}
