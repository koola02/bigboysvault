//! Market price history: one Near Mint market price per card and finish per day, saved by the nightly price
//! job (`ops::save_market`). Card pages show how the price moved ("Market price up 12% this month") with a
//! small line, and the admin dashboard lists the vault's own prices whose market moved a lot.
use rusqlite::{params, Connection};

pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS price_history(
    api_id TEXT NOT NULL, finish TEXT NOT NULL, day TEXT NOT NULL, php REAL NOT NULL,
    PRIMARY KEY(api_id, finish, day));";

/// Safe on every open: the table, and once, today's known prices as the first day of history.
pub fn migrate(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(SCHEMA)?;
    if crate::blocks::setting(c, "price_history_v")?.is_none() {
        c.execute(
            "INSERT OR IGNORE INTO price_history(api_id, finish, day, php)
             SELECT api_id, finish, date(MAX(market_at), 'unixepoch', '+8 hours'), MAX(market_php) FROM products
             WHERE api_id<>'' AND market_php>0 AND market_at>0 GROUP BY api_id, finish",
            [],
        )?;
        crate::blocks::set_setting(c, "price_history_v", "1")?;
    }
    Ok(())
}

/// Today's market price for a card and finish (`day` is "YYYY-MM-DD"). A second price the same day replaces the first.
pub fn record(c: &Connection, api_id: &str, finish: &str, php: f64, day: &str) -> rusqlite::Result<()> {
    if api_id.is_empty() || php <= 0.0 {
        return Ok(());
    }
    c.execute(
        "INSERT INTO price_history(api_id, finish, day, php) VALUES(?1,?2,?3,?4)
         ON CONFLICT(api_id, finish, day) DO UPDATE SET php=excluded.php",
        params![api_id, finish, day, php],
    )?;
    Ok(())
}

/// The prices of the last `days` days, oldest first.
pub fn series(c: &Connection, api_id: &str, finish: &str, days: i64) -> rusqlite::Result<Vec<(String, f64)>> {
    let mut st = c.prepare_cached(
        "SELECT day, php FROM price_history WHERE api_id=?1 AND finish=?2 AND day >= date('now', '+8 hours', ?3) ORDER BY day",
    )?;
    let v = st.query_map(params![api_id, finish, format!("-{days} days")], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// How a card's market price moved, for its page.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Trend {
    /// Change in percent, rounded: 12 means up 12%, -8 down 8%.
    pub pct: i64,
    /// The first day compared, like "11 Nov".
    pub since: String,
    /// The comparison covers about a month.
    pub month: bool,
    /// SVG polyline points in a 120 x 32 box.
    pub points: String,
    /// The latest market price.
    pub now: f64,
}

impl Trend {
    /// "Market price up 12% this month", "Market price down 8% since 11 Nov", "Market price steady since 11 Nov".
    pub fn text(&self) -> String {
        let when = if self.month { "this month".to_string() } else { format!("since {}", self.since) };
        match self.pct {
            0 => format!("Market price steady {when}"),
            p if p > 0 => format!("Market price up {p}% {when}"),
            p => format!("Market price down {}% {when}", -p),
        }
    }
    pub fn dir(&self) -> &'static str {
        match self.pct {
            0 => "flat",
            p if p > 0 => "up",
            _ => "down",
        }
    }
}

/// The trend over the last month (the line shows up to 90 days). None until there are two days to compare.
pub fn trend(c: &Connection, api_id: &str, finish: &str) -> rusqlite::Result<Option<Trend>> {
    if api_id.is_empty() {
        return Ok(None);
    }
    let all = series(c, api_id, finish, 90)?;
    Ok(trend_of(&all, &month_ago(c)?))
}

fn month_ago(c: &Connection) -> rusqlite::Result<String> {
    c.query_row("SELECT date('now', '+8 hours', '-30 days')", [], |r| r.get(0))
}

/// The arithmetic, apart from the database, so it can be tested.
pub fn trend_of(all: &[(String, f64)], month_ago: &str) -> Option<Trend> {
    if all.len() < 2 {
        return None;
    }
    let (last_day, last) = all.last()?;
    // Compare with the first price of the last 30 days (or the first one there is, when history is younger).
    let from = all.iter().position(|(d, _)| d.as_str() >= month_ago).unwrap_or(0);
    let (first_day, first) = &all[from];
    if first_day == last_day || *first <= 0.0 {
        return None;
    }
    let pct = ((last / first - 1.0) * 100.0).round() as i64;
    let month = first_day.as_str() <= month_ago || {
        // History that starts 28+ days back reads as "this month".
        let (fy, fm, fd) = ymd(first_day);
        let (ly, lm, ld) = ymd(last_day);
        days(ly, lm, ld) - days(fy, fm, fd) >= 28
    };
    Some(Trend { pct, since: short_day(first_day), month, points: points(all), now: *last })
}

fn ymd(d: &str) -> (i64, i64, i64) {
    let mut it = d.split('-').map(|x| x.parse::<i64>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(1), it.next().unwrap_or(1))
}

/// Days since a fixed point, enough to subtract two dates.
fn days(y: i64, m: i64, d: i64) -> i64 {
    let (y, m) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    365 * y + y / 4 - y / 100 + y / 400 + (153 * (m - 3) + 2) / 5 + d
}

fn short_day(d: &str) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (_, m, day) = ymd(d);
    format!("{day} {}", M[((m - 1).clamp(0, 11)) as usize])
}

/// The prices as a line in a 120 x 32 box, with a little room top and bottom.
fn points(all: &[(String, f64)]) -> String {
    let lo = all.iter().map(|x| x.1).fold(f64::INFINITY, f64::min);
    let hi = all.iter().map(|x| x.1).fold(f64::NEG_INFINITY, f64::max);
    let span = if hi - lo < 1e-9 { 1.0 } else { hi - lo };
    let n = all.len().max(2) - 1;
    all.iter()
        .enumerate()
        .map(|(i, (_, p))| {
            let x = 120.0 * i as f64 / n as f64;
            let y = if hi - lo < 1e-9 { 16.0 } else { 28.0 - 24.0 * (p - lo) / span };
            format!("{x:.1},{y:.1}")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A vault card whose own price may need a look, because its market moved a lot.
#[derive(Clone, Debug)]
pub struct Mover {
    pub id: i64,
    pub title: String,
    pub price: f64,
    pub trend: Trend,
}

impl Mover {
    pub fn price_text(&self) -> String {
        vault_search::peso(self.price)
    }
    pub fn market_text(&self) -> String {
        vault_search::peso(self.trend.now)
    }
}

/// Vault cards priced by hand ('own') whose market moved at least `min_pct` percent in the last month, biggest first.
pub fn movers(c: &Connection, min_pct: i64, limit: usize) -> rusqlite::Result<Vec<Mover>> {
    let ago = month_ago(c)?;
    let mut st = c.prepare(
        "SELECT MIN(p.id), p.title, MIN(p.price), p.api_id, p.finish FROM products p
         WHERE p.available=1 AND p.price_src='own' AND p.api_id<>'' AND p.price>0
         GROUP BY p.api_id, p.finish, lower(trim(p.title))",
    )?;
    let rows: Vec<(i64, String, f64, String, String)> =
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<Result<_, _>>()?;
    let mut out = vec![];
    for (id, title, price, api, fin) in rows {
        let all = series(c, &api, &fin, 45)?;
        if let Some(t) = trend_of(&all, &ago) {
            if t.pct.abs() >= min_pct {
                out.push(Mover { id, title, price, trend: t });
            }
        }
    }
    out.sort_by_key(|m| -m.trend.pct.abs());
    out.truncate(limit);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[(&str, f64)]) -> Vec<(String, f64)> {
        v.iter().map(|(d, p)| (d.to_string(), *p)).collect()
    }

    #[test]
    fn a_trend_needs_two_days_and_compares_with_a_month_ago() {
        assert!(trend_of(&s(&[("2026-11-11", 100.0)]), "2026-10-12").is_none());
        let t = trend_of(&s(&[("2026-11-11", 100.0), ("2026-11-12", 112.0)]), "2026-10-13").unwrap();
        assert_eq!((t.pct, t.month, t.since.as_str()), (12, false, "11 Nov"));
        assert_eq!(t.text(), "Market price up 12% since 11 Nov");
        // Older history: compared with the first price inside the last 30 days.
        let t = trend_of(&s(&[("2026-09-01", 50.0), ("2026-10-12", 200.0), ("2026-11-11", 150.0)]), "2026-10-12").unwrap();
        assert_eq!((t.pct, t.month), (-25, true));
        assert_eq!(t.text(), "Market price down 25% this month");
        let t = trend_of(&s(&[("2026-11-01", 80.0), ("2026-11-11", 80.0)]), "2026-10-12").unwrap();
        assert_eq!(t.text(), "Market price steady since 1 Nov");
        assert_eq!(t.points.split(' ').count(), 2);
    }

    #[test]
    fn prices_are_kept_one_per_day() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA).unwrap();
        record(&c, "tcgdex:a-1", "", 100.0, "2026-11-11").unwrap();
        record(&c, "tcgdex:a-1", "", 120.0, "2026-11-11").unwrap();
        record(&c, "tcgdex:a-1", "", 0.0, "2026-11-12").unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0)).unwrap();
        let p: f64 = c.query_row("SELECT php FROM price_history", [], |r| r.get(0)).unwrap();
        assert_eq!((n, p), (1, 120.0));
    }
}
