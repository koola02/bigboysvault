//! "More like this": the cards most like the one you're looking at.
//!
//! Each card becomes a TF-IDF vector over the words of its name (numbers dropped), and
//!   sim(a, b) = 0.50 · cos(a, b)                                   names alike ("Pikachu V" ~ "Pikachu VMAX")
//!             + 0.15 · [same set]
//!             + 0.20 · exp(−(ln(1 + price_a) − ln(1 + price_b))² / (2 · 0.6²))   prices alike, on a log scale
//!             + 0.10 · [same condition]
//!             + 0.05 · [has a photo]
//!   idf(t)    = ln((N + 1) / (df(t) + 1)) + 1                      a word few cards share says more
//! The picks then go through Maximal Marginal Relevance, so they aren't six copies of the same thing:
//!   next = argmax_c  λ · sim(q, c) − (1 − λ) · max_{s already picked} cos(c, s),   λ = 0.72
use std::collections::HashMap;

pub struct Item {
    pub name: String,
    pub set: String,
    pub price: f64,
    pub cond: &'static str,
    pub has_image: bool,
}

const LAMBDA: f64 = 0.72;
const SIGMA: f64 = 0.6;

/// The words of a name that say what the card is: no numbers, nothing shorter than two letters.
pub fn words(name: &str) -> Vec<String> {
    vault_search::norm(name)
        .split(' ')
        .filter(|w| w.chars().count() >= 2 && !w.chars().any(|c| c.is_ascii_digit()))
        .map(String::from)
        .collect()
}

/// Unit-length TF-IDF vectors, one per item.
fn vectors(items: &[Item]) -> Vec<HashMap<String, f64>> {
    let docs: Vec<Vec<String>> = items.iter().map(|i| words(&i.name)).collect();
    let mut df: HashMap<&str, f64> = HashMap::new();
    for d in &docs {
        let mut seen: Vec<&str> = d.iter().map(String::as_str).collect();
        seen.sort_unstable();
        seen.dedup();
        for w in seen {
            *df.entry(w).or_default() += 1.0;
        }
    }
    let n = docs.len() as f64;
    docs.iter()
        .map(|d| {
            let mut v: HashMap<String, f64> = HashMap::new();
            for w in d {
                *v.entry(w.clone()).or_default() += 1.0;
            }
            for (w, tf) in v.iter_mut() {
                *tf *= ((n + 1.0) / (df.get(w.as_str()).copied().unwrap_or(0.0) + 1.0)).ln() + 1.0;
            }
            let len = v.values().map(|x| x * x).sum::<f64>().sqrt();
            if len > 0.0 {
                v.values_mut().for_each(|x| *x /= len);
            }
            v
        })
        .collect()
}

fn cos(a: &HashMap<String, f64>, b: &HashMap<String, f64>) -> f64 {
    let (small, big) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    small.iter().map(|(w, x)| x * big.get(w).copied().unwrap_or(0.0)).sum()
}

/// The `n` items most like item `q`, best first, skipping any `skip` says to (the card itself, its other copies).
pub fn pick(items: &[Item], q: usize, n: usize, skip: impl Fn(usize) -> bool) -> Vec<usize> {
    if q >= items.len() {
        return vec![];
    }
    let v = vectors(items);
    let me = &items[q];
    let lp = (1.0 + me.price.max(0.0)).ln();
    let score: Vec<(usize, f64)> = (0..items.len())
        .filter(|&i| i != q && !skip(i))
        .map(|i| {
            let it = &items[i];
            let d = (1.0 + it.price.max(0.0)).ln() - lp;
            let s = 0.50 * cos(&v[q], &v[i])
                + 0.15 * f64::from(u8::from(!me.set.is_empty() && it.set.eq_ignore_ascii_case(&me.set)))
                + 0.20 * (-(d * d) / (2.0 * SIGMA * SIGMA)).exp()
                + 0.10 * f64::from(u8::from(!me.cond.is_empty() && it.cond == me.cond))
                + 0.05 * f64::from(u8::from(it.has_image));
            (i, s)
        })
        .collect();
    // Only the strongest candidates go through the slower diversity step.
    let mut pool = score;
    pool.sort_by(|a, b| b.1.total_cmp(&a.1));
    pool.truncate(n * 8);
    let mut chosen: Vec<usize> = vec![];
    while chosen.len() < n && !pool.is_empty() {
        let (k, _) = pool
            .iter()
            .enumerate()
            .map(|(k, &(i, s))| {
                let overlap = chosen.iter().map(|&c| cos(&v[i], &v[c])).fold(0.0, f64::max);
                (k, LAMBDA * s - (1.0 - LAMBDA) * overlap)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        chosen.push(pool.remove(k).0);
    }
    chosen
}

/// Where a price sits in the vault: the share of cards that cost less (0 to 100).
pub fn standing(prices: &[f64], price: f64) -> u8 {
    if prices.is_empty() {
        return 0;
    }
    let below = prices.iter().filter(|&&p| p < price).count() as f64;
    (below * 100.0 / prices.len() as f64).round().clamp(0.0, 100.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(name: &str, set: &str, price: f64) -> Item {
        Item { name: name.into(), set: set.into(), price, cond: "nm", has_image: true }
    }

    #[test]
    fn like_finds_like_and_keeps_it_varied() {
        let items = vec![
            it("Pikachu V - 43/185", "Vivid Voltage", 300.0),
            it("Pikachu VMAX - 44/185", "Vivid Voltage", 900.0),
            it("Pikachu - 25/102", "Base Set", 250.0),
            it("Pikachu - 26/102", "Base Set", 260.0),
            it("Pikachu - 27/102", "Base Set", 255.0),
            it("Snorlax - 11/64", "Jungle", 280.0),
            it("Charizard ex - 199/165", "151", 9000.0),
            it("Raichu - 14/102", "Vivid Voltage", 310.0),
        ];
        let got = pick(&items, 0, 3, |_| false);
        let names: Vec<&str> = got.iter().map(|&i| items[i].name.as_str()).collect();
        assert!(names[0].starts_with("Pikachu"), "the nearest is another Pikachu: {names:?}");
        assert!(!names.contains(&"Charizard ex - 199/165"), "a ₱9,000 Charizard isn't like a ₱300 Pikachu V: {names:?}");
        let plain = names.iter().filter(|n| n.starts_with("Pikachu - ")).count();
        assert!(plain <= 2, "not three copies of the same plain Pikachu: {names:?}");
        assert!(pick(&items, 0, 3, |i| i == 1).iter().all(|&i| i != 1), "skipped cards stay out");
    }

    #[test]
    fn standing_is_the_share_below() {
        let p = [10.0, 20.0, 30.0, 40.0];
        assert_eq!(standing(&p, 35.0), 75);
        assert_eq!(standing(&p, 10.0), 0);
        assert_eq!(standing(&[], 5.0), 0);
        assert_eq!(words("Pikachu & Zekrom GX - 33/181"), vec!["pikachu", "zekrom", "gx"]);
    }
}

/// Picks worked out on the VM by a small language model from each card's full details (type, stage, artist,
/// rarity, set). Replaced whole every night. `card_id` is any copy of the card; `picks` are copy ids, best first.
pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS similar_cards(card_id INTEGER PRIMARY KEY, picks TEXT NOT NULL, at INTEGER NOT NULL);";

/// Replaces every stored pick at once. Returns how many cards have picks.
pub fn save(c: &rusqlite::Connection, rows: &[(i64, Vec<i64>)], now: i64) -> rusqlite::Result<usize> {
    let tx = c.unchecked_transaction()?;
    tx.execute("DELETE FROM similar_cards", [])?;
    let mut n = 0;
    for (id, picks) in rows {
        if picks.is_empty() {
            continue;
        }
        let list = picks.iter().take(24).map(i64::to_string).collect::<Vec<_>>().join(",");
        tx.execute("INSERT OR REPLACE INTO similar_cards(card_id, picks, at) VALUES(?1,?2,?3)", rusqlite::params![id, list, now])?;
        n += 1;
    }
    tx.commit()?;
    Ok(n)
}

/// The stored picks for a card, given the ids of its copies. None when the VM hasn't worked this card out.
pub fn picks_for(c: &rusqlite::Connection, copy_ids: &[i64]) -> rusqlite::Result<Option<Vec<i64>>> {
    let mut st = c.prepare_cached("SELECT picks FROM similar_cards WHERE card_id=?1")?;
    for id in copy_ids {
        let found: Option<String> = st.query_row([id], |r| r.get(0)).ok();
        if let Some(list) = found {
            return Ok(Some(list.split(',').filter_map(|x| x.parse().ok()).collect()));
        }
    }
    Ok(None)
}
