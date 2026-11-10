//! Shared pricing brain for admins and sellers.
//! One single source of truth for condition-adjusted prices.

use std::collections::HashMap;
use serde_json::Value;

/// The standard condition labels and their default share of the Near Mint price.
pub const DEFAULT_SHARES: [(&str, f64); 9] = [
    ("Near Mint", 1.0),
    ("LP-NM", 0.9),
    ("Lightly Played", 0.8),
    ("MP-LP", 0.7),
    ("Moderately Played", 0.6),
    ("HP-MP", 0.5),
    ("Heavily Played", 0.4),
    ("Damaged-HP", 0.32),
    ("Damaged", 0.25),
];

pub fn default_shares() -> [(&'static str, f64); 9] {
    DEFAULT_SHARES
}

pub fn default_shares_map() -> HashMap<String, f64> {
    DEFAULT_SHARES.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

/// Parses saved shares JSON into a map. Missing or broken settings fall back to defaults.
pub fn parse_shares(json_str: &str) -> HashMap<String, f64> {
    let mut map = default_shares_map();
    if let Ok(Value::Object(obj)) = serde_json::from_str(json_str) {
        for (k, v) in obj {
            if let Some(f) = v.as_f64() {
                if (0.0..=5.0).contains(&f) {
                    map.insert(k, f);
                }
            }
        }
    }
    map
}

/// Loads the saved shares from the database, or returns default shares if unset or broken.
pub fn load_shares(c: &rusqlite::Connection) -> HashMap<String, f64> {
    match crate::blocks::setting(c, "condition_shares") {
        Ok(Some(s)) => parse_shares(&s),
        _ => default_shares_map(),
    }
}

/// The shares in use right now, so every price on the site (screens, totals, SQL) agrees.
static CURRENT: std::sync::RwLock<Option<HashMap<String, f64>>> = std::sync::RwLock::new(None);

/// The shares in use: the owner's saved ones once the database has been read, otherwise the usual ones.
pub fn current() -> HashMap<String, f64> {
    CURRENT.read().ok().and_then(|g| g.clone()).unwrap_or_else(default_shares_map)
}

fn current_share(std: &str) -> Option<f64> {
    if let Ok(g) = CURRENT.read() {
        if let Some(m) = g.as_ref() {
            if let Some(v) = m.get(std) {
                return Some(*v);
            }
        }
    }
    DEFAULT_SHARES.iter().find(|(k, _)| *k == std).map(|(_, v)| *v)
}

/// The table the SQL totals read (`members::worth_sql!`). One row per condition name.
const TABLE: &str = "CREATE TABLE IF NOT EXISTS condition_shares(cond TEXT PRIMARY KEY, share REAL NOT NULL)";

fn write_table(c: &rusqlite::Connection, shares: &HashMap<String, f64>) -> rusqlite::Result<()> {
    c.execute_batch(TABLE)?;
    c.execute("DELETE FROM condition_shares", [])?;
    for (cond, _) in DEFAULT_SHARES {
        let v = shares.get(cond).copied().unwrap_or(1.0);
        c.execute("INSERT INTO condition_shares(cond, share) VALUES(?1, ?2)", rusqlite::params![cond, v])?;
    }
    Ok(())
}

/// Runs with the other migrations: makes the table and reads the owner's shares once per process.
pub fn install(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    c.execute_batch(TABLE)?;
    let loaded = CURRENT.read().map(|g| g.is_some()).unwrap_or(false);
    let rows: i64 = c.query_row("SELECT COUNT(*) FROM condition_shares", [], |r| r.get(0))?;
    if !loaded || rows == 0 {
        let shares = load_shares(c);
        if rows != DEFAULT_SHARES.len() as i64 {
            write_table(c, &shares)?;
        }
        if let Ok(mut g) = CURRENT.write() {
            *g = Some(shares);
        }
    }
    Ok(())
}

/// Saves the owner's shares (None = back to the usual ones) everywhere they're read.
pub fn save(c: &rusqlite::Connection, shares: Option<&HashMap<String, f64>>) -> rusqlite::Result<HashMap<String, f64>> {
    let map = match shares {
        Some(m) => parse_shares(&serialize_shares(m)),
        None => default_shares_map(),
    };
    let text = if shares.is_some() { serialize_shares(&map) } else { String::new() };
    crate::blocks::set_setting(c, "condition_shares", &text)?;
    write_table(c, &map)?;
    if let Ok(mut g) = CURRENT.write() {
        *g = Some(map.clone());
    }
    Ok(map)
}

/// Serializes shares map to JSON string.
pub fn serialize_shares(shares: &HashMap<String, f64>) -> String {
    let mut out = serde_json::Map::new();
    for (cond, _) in DEFAULT_SHARES {
        let val = shares.get(cond).copied().unwrap_or_else(|| {
            DEFAULT_SHARES.iter().find(|(k, _)| *k == cond).map(|(_, v)| *v).unwrap_or(1.0)
        });
        out.insert(cond.to_string(), Value::from(val));
    }
    Value::Object(out).to_string()
}

/// Reads any condition string used on the site and identifies whether it matches a standard condition.
/// Understands:
/// - "Near Mint", "LP-NM", "Lightly Played", "MP-LP", "Moderately Played", "HP-MP", "Heavily Played", "Damaged-HP", "Damaged"
/// - Abbreviations: "NM", "LP", "MP", "HP", "DMG" in any casing
/// - "Mint", "Pristine" (which count as Near Mint)
/// - Multi-part older imports like "near-mint / reverse-holofoil / holo-rare"
pub fn read_condition(text: &str) -> Option<&'static str> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }

    // If multi-part (e.g. split by '/' or '·'), take the first part
    let first_part = t.split('/').next().unwrap_or(t);
    let first_part = first_part.split('·').next().unwrap_or(first_part).trim();

    let norm = first_part.to_lowercase().replace(['_', '/', '.'], " ");
    let cleaned = norm.replace('-', " ");
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let compact = norm.replace([' ', '-'], "");

    // 1. Direct checks for standard or abbreviation forms
    match compact.as_str() {
        "nearmint" | "nm" | "mint" | "pristine" => return Some("Near Mint"),
        "lpnm" | "nmlp" => return Some("LP-NM"),
        "lightlyplayed" | "lp" => return Some("Lightly Played"),
        "mplp" | "lpmp" => return Some("MP-LP"),
        "moderatelyplayed" | "mp" | "modplayed" => return Some("Moderately Played"),
        "hpmp" | "mphp" => return Some("HP-MP"),
        "heavilyplayed" | "hp" | "heavyplayed" => return Some("Heavily Played"),
        "damagedhp" | "hpdamaged" | "dmghp" | "hpdmg" => return Some("Damaged-HP"),
        "damaged" | "dmg" => return Some("Damaged"),
        _ => {}
    }

    // 2. Multi-word checks
    if words.is_empty() {
        return None;
    }

    if words.contains(&"near") && words.contains(&"mint") {
        return Some("Near Mint");
    }
    if words.contains(&"mint") || words.contains(&"pristine") {
        return Some("Near Mint");
    }
    if (words.contains(&"lp") && words.contains(&"nm")) || (words.contains(&"nm") && words.contains(&"lp")) {
        return Some("LP-NM");
    }
    if words.contains(&"mp") && words.contains(&"lp") {
        return Some("MP-LP");
    }
    if words.contains(&"hp") && words.contains(&"mp") {
        return Some("HP-MP");
    }
    if (words.contains(&"damaged") || words.contains(&"dmg")) && words.contains(&"hp") {
        return Some("Damaged-HP");
    }
    if words.contains(&"lightly") || words.first() == Some(&"lp") {
        return Some("Lightly Played");
    }
    if words.contains(&"moderately") || words.contains(&"mod") || words.first() == Some(&"mp") {
        return Some("Moderately Played");
    }
    if words.contains(&"heavily") || words.contains(&"heavy") || words.first() == Some(&"hp") {
        return Some("Heavily Played");
    }
    if words.contains(&"damaged") || words.first() == Some(&"dmg") {
        return Some("Damaged");
    }

    None
}

/// True if the text represents a professionally graded card (PSA, CGC, BGS, SGC, ACE, TAG, etc.).
pub fn is_graded(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    if t.is_empty() {
        return false;
    }
    let parts: Vec<&str> = t.split_whitespace().collect();
    let first = parts.first().copied().unwrap_or("");
    if ["psa", "cgc", "bgs", "sgc", "ace", "tag"].iter().any(|&g| first == g || first.starts_with(g)) {
        return true;
    }
    t.contains("gem mt") || t.contains("gem-mt") || t.contains("graded") || t.contains("slab")
}

/// True if the item is sealed product.
pub fn is_sealed(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    t.contains("sealed")
}

/// Returns the share (0.0 .. 1.0+) of the Near Mint price for a condition, using the shares in use.
/// None means no automatic suggestion (graded, sealed, or unrecognized).
pub fn factor(condition: &str) -> Option<f64> {
    if is_graded(condition) || is_sealed(condition) {
        return None;
    }
    current_share(read_condition(condition)?)
}

/// The usual share, ignoring anything the owner saved. For tests and for showing "the usual numbers".
pub fn default_factor(condition: &str) -> Option<f64> {
    if is_graded(condition) || is_sealed(condition) {
        return None;
    }
    let std = read_condition(condition)?;
    DEFAULT_SHARES.iter().find(|(k, _)| *k == std).map(|(_, v)| *v)
}

/// Returns the share of the Near Mint price for a condition using custom shares.
pub fn factor_with_shares(shares: &HashMap<String, f64>, condition: &str) -> Option<f64> {
    if is_graded(condition) || is_sealed(condition) {
        return None;
    }
    let std = read_condition(condition)?;
    shares.get(std).copied().or_else(|| {
        DEFAULT_SHARES.iter().find(|(k, _)| *k == std).map(|(_, v)| *v)
    })
}

/// Rounding rule:
/// Whole pesos.
/// - Under ₱100: nearest ₱1
/// - ₱100–₱1,000: nearest ₱5
/// - Over ₱1,000: nearest ₱10
/// - Never below ₱1.
pub fn round_php(raw: f64) -> f64 {
    if raw <= 0.0 {
        return 1.0;
    }
    let r = if raw < 100.0 {
        raw.round()
    } else if raw <= 1000.0 {
        (raw / 5.0).round() * 5.0
    } else {
        (raw / 10.0).round() * 10.0
    };
    r.max(1.0)
}

/// Calculates the suggested price for a card in pesos from its Near Mint market price and condition,
/// using default shares.
/// Returns None for graded cards, sealed cards, unrecognized conditions, or near_mint_php <= 0.
pub fn suggest(near_mint_php: f64, condition: &str) -> Option<f64> {
    if near_mint_php <= 0.0 {
        return None;
    }
    let share = factor(condition)?;
    Some(round_php(near_mint_php * share))
}

/// Calculates the suggested price using custom shares.
pub fn suggest_with_shares(shares: &HashMap<String, f64>, near_mint_php: f64, condition: &str) -> Option<f64> {
    if near_mint_php <= 0.0 {
        return None;
    }
    let share = factor_with_shares(shares, condition)?;
    Some(round_php(near_mint_php * share))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_condition_form_gives_the_right_share() {
        assert_eq!(factor("Near Mint"), Some(1.0));
        assert_eq!(factor("near-mint / reverse-holofoil / holo-rare"), Some(1.0));
        assert_eq!(factor("NM"), Some(1.0));
        assert_eq!(factor("nm"), Some(1.0));
        assert_eq!(factor("Mint"), Some(1.0));
        assert_eq!(factor("mint"), Some(1.0));
        assert_eq!(factor("Pristine"), Some(1.0));
        assert_eq!(factor("pristine"), Some(1.0));

        assert_eq!(factor("LP-NM"), Some(0.9));
        assert_eq!(factor("lp-nm"), Some(0.9));

        assert_eq!(factor("Lightly Played"), Some(0.8));
        assert_eq!(factor("lightly played"), Some(0.8));
        assert_eq!(factor("lp"), Some(0.8));
        assert_eq!(factor("LP"), Some(0.8));

        assert_eq!(factor("MP-LP"), Some(0.7));
        assert_eq!(factor("mp-lp"), Some(0.7));

        assert_eq!(factor("Moderately Played"), Some(0.6));
        assert_eq!(factor("moderately played"), Some(0.6));
        assert_eq!(factor("mp"), Some(0.6));
        assert_eq!(factor("MP"), Some(0.6));
        assert_eq!(factor("Mod. Played"), Some(0.6));

        assert_eq!(factor("HP-MP"), Some(0.5));
        assert_eq!(factor("hp-mp"), Some(0.5));

        assert_eq!(factor("Heavily Played"), Some(0.4));
        assert_eq!(factor("heavily played"), Some(0.4));
        assert_eq!(factor("hp"), Some(0.4));
        assert_eq!(factor("HP"), Some(0.4));

        assert_eq!(factor("Damaged-HP"), Some(0.32));
        assert_eq!(factor("damaged-hp"), Some(0.32));
        assert_eq!(factor("dmg-hp"), Some(0.32));

        assert_eq!(factor("Damaged"), Some(0.25));
        assert_eq!(factor("damaged"), Some(0.25));
        assert_eq!(factor("DMG"), Some(0.25));
        assert_eq!(factor("dmg"), Some(0.25));
    }

    #[test]
    fn graded_and_sealed_give_no_suggestion() {
        assert_eq!(factor("PSA 10"), None);
        assert_eq!(factor("PSA 9"), None);
        assert_eq!(factor("PSA 9 GEM MT"), None);
        assert_eq!(factor("CGC 9.5"), None);
        assert_eq!(factor("CGC 10"), None);
        assert_eq!(factor("BGS 9.5"), None);
        assert_eq!(factor("ACE 10"), None);
        assert_eq!(factor("TAG 10"), None);
        assert_eq!(factor("Sealed"), None);
        assert_eq!(factor("sealed booster pack"), None);

        assert_eq!(suggest(100.0, "PSA 10"), None);
        assert_eq!(suggest(100.0, "CGC 9.5"), None);
        assert_eq!(suggest(100.0, "BGS 9.5"), None);
        assert_eq!(suggest(100.0, "PSA 9 GEM MT"), None);
        assert_eq!(suggest(100.0, "Sealed"), None);
    }

    #[test]
    fn rounding_at_key_numbers() {
        // Under ₱100, nearest ₱1
        assert_eq!(round_php(9.6), 10.0);
        assert_eq!(round_php(99.0), 99.0);

        // ₱100–₱1,000, nearest ₱5
        assert_eq!(round_php(101.0), 100.0);
        assert_eq!(round_php(998.0), 1000.0);

        // Over ₱1,000, nearest ₱10
        assert_eq!(round_php(1234.0), 1230.0);

        // Never below ₱1
        assert_eq!(round_php(0.25), 1.0);
        assert_eq!(round_php(0.0), 1.0);
        assert_eq!(round_php(-5.0), 1.0);
    }

    #[test]
    fn saved_shares_replace_defaults_and_broken_setting_falls_back() {
        let custom_json = r#"{"Lightly Played": 0.85, "Damaged": 0.20}"#;
        let shares = parse_shares(custom_json);
        assert_eq!(shares.get("Lightly Played"), Some(&0.85));
        assert_eq!(shares.get("Damaged"), Some(&0.20));
        assert_eq!(shares.get("Near Mint"), Some(&1.0)); // keeps default

        let custom_factor = factor_with_shares(&shares, "Lightly Played");
        assert_eq!(custom_factor, Some(0.85));

        // Broken setting falls back to defaults
        let broken_json = "{ invalid json ";
        let fallback = parse_shares(broken_json);
        assert_eq!(fallback.get("Lightly Played"), Some(&0.8));
        assert_eq!(fallback.get("Damaged"), Some(&0.25));
    }

    /// The SQL totals read the same table the screens use, so a saved share changes both.
    #[test]
    fn worth_sql_reads_the_shared_table() {
        let sql = crate::members::worth_sql_clause();
        assert!(sql.contains("condition_shares"), "worth_sql must read the condition_shares table");
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)").ok();
        write_table(&c, &default_shares_map()).unwrap();
        for (cond, share) in DEFAULT_SHARES {
            let got: f64 = c.query_row("SELECT share FROM condition_shares WHERE cond=?1", [cond], |r| r.get(0)).unwrap();
            assert_eq!(got, share, "{cond}");
        }
    }
}
