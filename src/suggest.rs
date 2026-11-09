//! Search suggestions ranked by probability.
//!
//! Model (a smoothed unigram prior with a noisy-channel typo model):
//!   prior weight   c_i = cards_with_name_i  +  1.5·ln(1 + views_i)  +  2·Σ recent_searches_i · e^(-age/30d)
//!   match quality  q_i = 1.0 if the text starts with what was typed, 0.35 if each typed word starts some word of it
//!   typo penalty   t_i = 0.03^d   where d = edit distance (only used when nothing matches exactly)
//!   score          s_i = c_i · q_i · t_i
//!   probability    P_i = (s_i + α) / (Σ_j s_j + α·m),   α = 0.5 (Laplace/additive smoothing), m = number of candidates
//! So P_i is the estimated chance that candidate i is what the person is typing, learned from *this* collection and
//! *these* searches. It is an estimate, not a universal truth, and it sharpens as more cards and searches accumulate.
use rusqlite::Connection;
use std::collections::HashMap;
pub use vault_search::norm;
use vault_search::osa;

pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS search_log(term TEXT PRIMARY KEY, n INTEGER NOT NULL DEFAULT 0, last INTEGER NOT NULL DEFAULT 0);";

const ALPHA: f64 = 0.5;
const TAU_SECS: f64 = 30.0 * 86400.0;

pub struct Cand {
    text: String,
    lower: String,
    kind: &'static str,
    href: Option<String>,
    w: f64,
}

pub struct Index {
    cands: Vec<Cand>,
    logs: Vec<(String, f64, i64)>,
}

pub struct Hit {
    pub text: String,
    pub p: f64,
    pub kind: &'static str,
    pub href: Option<String>,
    pub typo: bool,
    pub full_prefix: bool,
}

pub fn log_search(c: &Connection, term: &str, now: i64) -> rusqlite::Result<()> {
    let t = norm(term);
    if t.len() < 2 || t.len() > 60 {
        return Ok(());
    }
    c.execute(
        "INSERT INTO search_log(term,n,last) VALUES(?1,1,?2) ON CONFLICT(term) DO UPDATE SET n=n+1,last=?2",
        rusqlite::params![t, now],
    )?;
    Ok(())
}

pub fn build(c: &Connection) -> rusqlite::Result<Index> {
    type Group = (HashMap<String, u32>, f64, f64); // display spellings, card count, views
    let mut names: HashMap<String, Group> = HashMap::new();
    let mut sets: HashMap<String, Group> = HashMap::new();
    let mut st = c.prepare("SELECT title, hits FROM products")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    for r in rows {
        let (title, hits) = r?;
        let parts: Vec<&str> = title.split(" - ").map(str::trim).collect();
        let mut add = |map: &mut HashMap<String, Group>, display: &str| {
            let key = norm(display);
            if key.len() < 2 {
                return;
            }
            let e = map.entry(key).or_default();
            *e.0.entry(display.to_string()).or_default() += 1;
            e.1 += 1.0;
            e.2 += hits as f64;
        };
        add(&mut names, parts[0]);
        if parts.len() >= 3 {
            add(&mut sets, parts[2]);
        }
    }
    let mut cands = vec![];
    let mut push = |map: HashMap<String, Group>, kind: &'static str, scale: f64| {
        for (lower, (disp, n, views)) in map {
            let text = disp.into_iter().max_by_key(|(_, k)| *k).map(|(d, _)| d).unwrap_or_else(|| lower.clone());
            cands.push(Cand { text, lower, kind, href: None, w: scale * (n + 1.5 * (1.0 + views).ln()) });
        }
    };
    push(names, "card", 1.0);
    push(sets, "set", 0.6);
    let mut st = c.prepare("SELECT c.handle,c.title,(SELECT COUNT(*) FROM collection_products WHERE collection=c.handle) FROM collections c")?;
    let bs = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?;
    for b in bs {
        let (handle, title, n) = b?;
        cands.push(Cand { lower: norm(&title), text: title, kind: "binder", href: Some(format!("/collections/{handle}")), w: 2.0 + (1.0 + n as f64).ln() });
    }
    let mut st = c.prepare("SELECT term,n,last FROM search_log")?;
    let logs = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as f64, r.get::<_, i64>(2)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(Index { cands, logs })
}

pub fn suggest(ix: &Index, q: &str, now: i64) -> Vec<Hit> {
    let ql = norm(q);
    if ql.is_empty() {
        return vec![];
    }
    let words: Vec<&str> = ql.split(' ').collect();
    let boost = |c: &Cand| -> f64 {
        ix.logs.iter().filter(|(t, _, _)| c.lower.starts_with(t.as_str())).map(|(_, n, last)| 2.0 * n * (-((now - last).max(0) as f64) / TAU_SECS).exp()).sum()
    };
    // exact-prefix and word-prefix matches
    let mut scored: Vec<(usize, f64, bool, bool)> = vec![]; // (index, score, full_prefix, typo)
    for (i, c) in ix.cands.iter().enumerate() {
        let full = c.lower.starts_with(&ql);
        let wordwise = full || words.iter().all(|t| c.lower.split(' ').any(|w| w.starts_with(t)));
        if wordwise {
            scored.push((i, (c.w + boost(c)) * if full { 1.0 } else { 0.35 }, full, false));
        }
    }
    // nothing matched: noisy-channel typo correction
    if scored.is_empty() {
        let qc: Vec<char> = ql.chars().collect();
        let max_d = if qc.len() >= 6 { 2 } else if qc.len() >= 4 { 1 } else { 0 };
        if max_d > 0 {
            for (i, c) in ix.cands.iter().enumerate() {
                let cc: Vec<char> = c.lower.chars().collect();
                let head: Vec<char> = cc.iter().take(qc.len()).copied().collect();
                let d = osa(&qc, &head);
                if d <= max_d {
                    scored.push((i, (c.w + boost(c)) * 0.03f64.powi(d as i32), false, true));
                }
            }
        }
    }
    if scored.is_empty() {
        return vec![];
    }
    let sum: f64 = scored.iter().map(|s| s.1).sum();
    let m = scored.len() as f64;
    let mut hits: Vec<Hit> = scored
        .into_iter()
        .map(|(i, s, full, typo)| {
            let c = &ix.cands[i];
            Hit { text: c.text.clone(), p: (s + ALPHA) / (sum + ALPHA * m), kind: c.kind, href: c.href.clone(), typo, full_prefix: full }
        })
        .collect();
    hits.sort_by(|a, b| b.p.partial_cmp(&a.p).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(6);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ix() -> Index {
        let c = |t: &str, w: f64| Cand { text: t.into(), lower: norm(t), kind: "card", href: None, w };
        Index { cands: vec![c("Pikachu", 40.0), c("Pikachu ex", 10.0), c("Piplup", 3.0), c("Charizard", 25.0)], logs: vec![] }
    }
    #[test]
    fn probabilities_sum_to_one_over_matches() {
        let h = suggest(&ix(), "pi", 0);
        assert_eq!(h.len(), 3);
        let total: f64 = h.iter().map(|x| x.p).sum();
        assert!((total - 1.0).abs() < 1e-9, "{total}");
        assert_eq!(h[0].text, "Pikachu");
    }
    #[test]
    fn typo_is_corrected() {
        let h = suggest(&ix(), "pikahcu", 0);
        assert_eq!(h[0].text, "Pikachu");
        assert!(h[0].typo);
    }
    #[test]
    fn history_raises_probability() {
        let mut x = ix();
        let before = suggest(&x, "pi", 0).iter().find(|h| h.text == "Piplup").unwrap().p;
        x.logs.push(("piplup".into(), 30.0, 0));
        let after = suggest(&x, "pi", 0).iter().find(|h| h.text == "Piplup").unwrap().p;
        assert!(after > before);
    }
}
