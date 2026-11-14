//! Visitors: how many people opened the site each day.
//!
//! One row per visitor per day (Manila time). A visitor is a hash of their address and browser, never the address
//! itself (store::viewer_hash). Search engines, link previews, scripts, staff, and pages a browser only fetched
//! ahead of time aren't counted. The request is checked here; the counting is done by main.rs after the page is
//! sent, so nobody waits for it. The admin dashboard shows the totals.
use rusqlite::Connection;

pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS visits(
  day TEXT NOT NULL, who TEXT NOT NULL, views INTEGER NOT NULL DEFAULT 1, signed INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(day, who)) WITHOUT ROWID;";

/// How long a day's visitors are kept.
const KEEP_DAYS: i64 = 400;

pub fn migrate(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(SCHEMA)
}

/// Words that show a request came from a program, not a person: search engines, link previews, checks, scripts.
const NOT_PEOPLE: &[&str] = &[
    "bot", "crawl", "spider", "slurp", "preview", "facebookexternalhit", "whatsapp", "telegram", "discord", "curl", "wget", "python", "headless",
    "lighthouse", "monitor", "uptime", "pingdom", "scrapy", "httpclient", "go-http", "java/", "okhttp", "node-fetch", "axios", "puppeteer",
    "playwright", "cloudflare", "feedfetcher", "inspectiontool", "ahrefs", "semrush",
];

/// True when the browser's name says it isn't a person. A request with no name at all isn't one either.
pub fn is_bot(agent: &str) -> bool {
    let a = agent.to_ascii_lowercase();
    a.trim().is_empty() || NOT_PEOPLE.iter().any(|w| a.contains(w))
}

/// Is this a person opening a page? `dest` and `purpose` are what the browser says about the request
/// (Sec-Fetch-Dest and Sec-Purpose); `accept` and `agent` are its Accept and User-Agent.
/// - Only whole pages count: not pictures, not the card window's parts, not the feed loading more.
/// - A page fetched ahead of time, before anyone asked for it, doesn't count.
/// - The admin's own pages don't count.
pub fn is_page_open(path: &str, dest: Option<&str>, purpose: Option<&str>, accept: &str, agent: &str) -> bool {
    if path == "/admin" || path.starts_with("/admin/") || is_bot(agent) {
        return false;
    }
    if purpose.is_some_and(|p| p.contains("prefetch") || p.contains("prerender")) {
        return false;
    }
    match dest {
        Some(d) => d == "document",
        // Older browsers don't say what a request is for: a page is one that asks for HTML first.
        None => accept.trim_start().starts_with("text/html"),
    }
}

/// One more page opened by this visitor today. `signed` is whether they were logged in.
pub fn count(c: &Connection, at: i64, who: &str, signed: bool) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO visits(day, who, views, signed) VALUES(date(?1,'unixepoch','+8 hours'), ?2, 1, ?3)
         ON CONFLICT(day, who) DO UPDATE SET views=views+1, signed=MAX(signed, excluded.signed)",
        rusqlite::params![at, who, signed as i64],
    )?;
    Ok(())
}

/// One day of visitors: the day, how many people, how many pages they opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Day {
    pub day: String,
    pub visitors: i64,
    pub views: i64,
}

/// The last `days` days up to today, oldest first. A day nobody came is there too, with zeros.
pub fn per_day(c: &Connection, at: i64, days: i64) -> rusqlite::Result<Vec<Day>> {
    let days = days.max(1);
    let mut st = c.prepare(
        "WITH RECURSIVE d(day, n) AS (
           SELECT date(?1,'unixepoch','+8 hours', ?2), 1
           UNION ALL SELECT date(day,'+1 day'), n+1 FROM d WHERE n < ?3)
         SELECT d.day, COUNT(v.who), COALESCE(SUM(v.views),0) FROM d LEFT JOIN visits v ON v.day=d.day GROUP BY d.day ORDER BY d.day",
    )?;
    let rows = st.query_map(rusqlite::params![at, format!("-{} days", days - 1), days], |r| Ok(Day { day: r.get(0)?, visitors: r.get(1)?, views: r.get(2)? }))?;
    rows.collect()
}

/// Different people over the last `days` days: someone who came on three of them is one person.
/// With `signed_only`, only those who were logged in.
pub fn people(c: &Connection, at: i64, days: i64, signed_only: bool) -> rusqlite::Result<i64> {
    c.query_row(
        "SELECT COUNT(DISTINCT who) FROM visits WHERE day >= date(?1,'unixepoch','+8 hours', ?2) AND (?3=0 OR signed=1)",
        rusqlite::params![at, format!("-{} days", days.max(1) - 1), signed_only as i64],
        |r| r.get(0),
    )
}

/// The first day anything was counted, or nothing yet.
pub fn first_day(c: &Connection) -> rusqlite::Result<Option<String>> {
    c.query_row("SELECT MIN(day) FROM visits", [], |r| r.get(0))
}

/// Forgets days older than about a year. Run now and then.
pub fn prune(c: &Connection, at: i64) -> rusqlite::Result<usize> {
    c.execute("DELETE FROM visits WHERE day < date(?1,'unixepoch','+8 hours', ?2)", rusqlite::params![at, format!("-{KEEP_DAYS} days")])
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHROME: &str = "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Mobile Safari/537.36";
    /// Noon in Manila on 14 November 2026.
    const NOON: i64 = 1_794_628_800;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        c
    }

    #[test]
    fn only_people_opening_pages_count() {
        assert!(is_page_open("/", Some("document"), None, "text/html", CHROME));
        assert!(is_page_open("/products/pikachu", Some("document"), None, "text/html", CHROME));
        assert!(is_page_open("/me/orders", Some("document"), None, "text/html", CHROME), "a trainer's own pages are visits too");
        // The card window's parts, the feed loading more, pictures: not pages.
        assert!(!is_page_open("/products/pikachu", Some("empty"), None, "*/*", CHROME));
        assert!(!is_page_open("/static/img/logo.png", Some("image"), None, "image/*", CHROME));
        // Fetched ahead of time, before anyone asked.
        assert!(!is_page_open("/all", Some("document"), Some("prefetch;prerender"), "text/html", CHROME));
        assert!(!is_page_open("/all", Some("document"), Some("prefetch"), "text/html", CHROME));
        // The admin's own pages.
        assert!(!is_page_open("/admin", Some("document"), None, "text/html", CHROME));
        assert!(!is_page_open("/admin/dashboard", Some("document"), None, "text/html", CHROME));
        assert!(is_page_open("/administrator", Some("document"), None, "text/html", CHROME), "only /admin itself and what is under it");
        // An older browser that doesn't say what the request is for.
        assert!(is_page_open("/", None, None, "text/html,application/xhtml+xml", CHROME));
        assert!(!is_page_open("/", None, None, "*/*", CHROME));
    }

    #[test]
    fn programs_are_not_people() {
        for agent in ["", "  ", "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)", "facebookexternalhit/1.1", "curl/8.4.0", "python-requests/2.31", "Mozilla/5.0 HeadlessChrome/130.0", "UptimeRobot/2.0"] {
            assert!(is_bot(agent), "{agent}");
            assert!(!is_page_open("/", Some("document"), None, "text/html", agent), "{agent}");
        }
        assert!(!is_bot(CHROME));
        assert!(!is_bot("Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1"));
    }

    #[test]
    fn a_person_counts_once_a_day_however_many_pages() {
        let c = db();
        for _ in 0..3 {
            count(&c, NOON, "ana", false).unwrap();
        }
        count(&c, NOON + 60, "ben", true).unwrap();
        let days = per_day(&c, NOON, 1).unwrap();
        assert_eq!(days, vec![Day { day: "2026-11-14".into(), visitors: 2, views: 4 }]);
        // Logging in later the same day marks the visitor as logged in; it never goes back.
        count(&c, NOON + 120, "ana", true).unwrap();
        count(&c, NOON + 180, "ana", false).unwrap();
        assert_eq!(people(&c, NOON, 1, true).unwrap(), 2);
    }

    #[test]
    fn the_day_is_manilas_day() {
        let c = db();
        // 23:30 in Manila on the 14th is 15:30 UTC; half an hour later it is the 15th in Manila.
        let late = NOON + 11 * 3600 + 30 * 60;
        count(&c, late, "ana", false).unwrap();
        count(&c, late + 3600, "ana", false).unwrap();
        let days = per_day(&c, late + 3600, 2).unwrap();
        assert_eq!(days.iter().map(|d| (d.day.as_str(), d.visitors)).collect::<Vec<_>>(), vec![("2026-11-14", 1), ("2026-11-15", 1)]);
    }

    #[test]
    fn quiet_days_show_as_zero_and_people_are_counted_once_across_days() {
        let c = db();
        count(&c, NOON - 5 * 86_400, "ana", false).unwrap();
        count(&c, NOON - 86_400, "ana", false).unwrap();
        count(&c, NOON, "ben", false).unwrap();
        let days = per_day(&c, NOON, 7).unwrap();
        assert_eq!(days.len(), 7);
        assert_eq!(days.first().unwrap().day, "2026-11-08");
        assert_eq!(days.last().unwrap().day, "2026-11-14");
        assert_eq!(days.iter().map(|d| d.visitors).collect::<Vec<_>>(), vec![0, 1, 0, 0, 0, 1, 1]);
        assert_eq!(people(&c, NOON, 7, false).unwrap(), 2, "ana came twice and is one person");
        assert_eq!(people(&c, NOON, 1, false).unwrap(), 1);
        assert_eq!(first_day(&c).unwrap().as_deref(), Some("2026-11-09"));
    }

    #[test]
    fn old_days_are_forgotten() {
        let c = db();
        assert_eq!(first_day(&c).unwrap(), None);
        count(&c, NOON - 500 * 86_400, "old", false).unwrap();
        count(&c, NOON, "new", false).unwrap();
        assert_eq!(prune(&c, NOON).unwrap(), 1);
        assert_eq!(first_day(&c).unwrap().as_deref(), Some("2026-11-14"));
    }
}
