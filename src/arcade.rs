//! Vault Arcade (the owner, 20 Nov 2026): light games anyone can play on the site, a phone first. A trainer who is
//! logged in saves their score; each game has a leaderboard for the day (Philippine time, midnight to midnight).
//! The games themselves run in the browser (static/arcade.js); this keeps the scores.
//!
//! - who: Who's That Pokémon? Name the silhouette. Score: points for each right answer, more at higher levels.
//! - memory: Memory Match. Find the pairs on 6×6, 6×8 then 8×8. Score: points for each board, less for extra moves and time.
//! - types: Type Challenge. The same 10 questions for everyone each day; the first try of the day is the one that counts.
//! - price: Price Check. Two real cards for sale here: tap the one that costs more. Score: how many in a row.

use rusqlite::{Connection, OptionalExtension};

pub const GAMES: [&str; 4] = ["who", "memory", "types", "price"];

/// The highest score each game can give (anything above is turned away).
pub fn max_score(game: &str) -> i64 {
    match game {
        "who" => 20_000,
        "memory" => 20_000,
        "types" => 10,
        "price" => 2_000,
        _ => 0,
    }
}

pub fn migrate(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS arcade_scores(
           member_id INTEGER NOT NULL, game TEXT NOT NULL, day TEXT NOT NULL, score INTEGER NOT NULL,
           level INTEGER NOT NULL DEFAULT 0, moves INTEGER NOT NULL DEFAULT 0, ms INTEGER NOT NULL DEFAULT 0, at INTEGER NOT NULL,
           PRIMARY KEY(member_id, game, day)) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS idx_arcade_board ON arcade_scores(game, day, score DESC);",
    )
}

/// The day a moment falls on in the Philippines ("2026-11-20"): each leaderboard is one of these.
pub fn day_of(c: &Connection, at: i64) -> rusqlite::Result<String> {
    c.query_row("SELECT date(?1, 'unixepoch', '+8 hours')", [at], |r| r.get(0))
}

/// One line of a leaderboard.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub member_id: i64,
    pub name: String,
    pub avatar: String,
    pub score: i64,
    pub level: i64,
    pub moves: i64,
    pub ms: i64,
}

/// What became of a score sent in.
#[derive(Debug, Clone, PartialEq)]
pub enum Saved {
    /// Their best today so far (and where it stands).
    Best(i64),
    /// They did better earlier today; that one stays (and where it stands).
    Kept(i64),
    /// Type Challenge: today's try was already played.
    Already(i64),
}

/// A score: kept when it is the trainer's best today (a higher score, or the same in less time). Type Challenge keeps
/// only the first try of the day.
pub fn save(c: &Connection, member: i64, game: &str, score: i64, level: i64, moves: i64, ms: i64, at: i64) -> rusqlite::Result<Saved> {
    let day = day_of(c, at)?;
    let had: Option<(i64, i64)> = c
        .query_row("SELECT score, ms FROM arcade_scores WHERE member_id=?1 AND game=?2 AND day=?3", rusqlite::params![member, game, day], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    if game == "types" && had.is_some() {
        return Ok(Saved::Already(rank(c, member, game, &day)?));
    }
    let better = had.map_or(true, |(s, t)| score > s || (score == s && ms < t));
    if better {
        c.execute(
            "INSERT INTO arcade_scores(member_id, game, day, score, level, moves, ms, at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(member_id, game, day) DO UPDATE SET score=excluded.score, level=excluded.level, moves=excluded.moves, ms=excluded.ms, at=excluded.at",
            rusqlite::params![member, game, day, score, level, moves, ms, at],
        )?;
    }
    let r = rank(c, member, game, &day)?;
    Ok(if better { Saved::Best(r) } else { Saved::Kept(r) })
}

/// Where a trainer's best today stands on the game's board (1 is the top).
fn rank(c: &Connection, member: i64, game: &str, day: &str) -> rusqlite::Result<i64> {
    c.query_row(
        "SELECT 1 + COUNT(*) FROM arcade_scores o, arcade_scores me
         WHERE me.member_id=?1 AND me.game=?2 AND me.day=?3 AND o.game=?2 AND o.day=?3 AND o.member_id<>?1
           AND (o.score > me.score OR (o.score = me.score AND o.ms < me.ms))",
        rusqlite::params![member, game, day],
        |r| r.get(0),
    )
}

/// The day's leaderboard for a game: the best score of each trainer, highest first (the same score: the faster one).
pub fn board(c: &Connection, game: &str, day: &str, n: i64) -> rusqlite::Result<Vec<Row>> {
    c.prepare(
        "SELECT s.member_id, m.name, m.avatar, s.score, s.level, s.moves, s.ms FROM arcade_scores s JOIN members m ON m.id=s.member_id
         WHERE s.game=?1 AND s.day=?2 AND m.blocked=0 ORDER BY s.score DESC, s.ms ASC, s.at ASC LIMIT ?3",
    )?
    .query_map(rusqlite::params![game, day, n], |r| {
        Ok(Row { member_id: r.get(0)?, name: r.get(1)?, avatar: r.get(2)?, score: r.get(3)?, level: r.get(4)?, moves: r.get(5)?, ms: r.get(6)? })
    })?
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(crate::members::SCHEMA).unwrap();
        crate::members::migrate(&c).unwrap();
        migrate(&c).unwrap();
        for n in ["Ash", "Misty", "Brock"] {
            crate::members::create(&c, n, "x").unwrap();
        }
        c
    }

    /// Noon in Manila on 20 November 2026.
    const NOON: i64 = 1_795_147_200;

    #[test]
    fn the_best_of_the_day_counts_and_the_board_is_in_order() {
        let c = db();
        assert_eq!(save(&c, 1, "who", 50, 2, 0, 9000, NOON).unwrap(), Saved::Best(1));
        assert_eq!(save(&c, 2, "who", 80, 3, 0, 9000, NOON).unwrap(), Saved::Best(1));
        assert_eq!(save(&c, 1, "who", 30, 1, 0, 9000, NOON).unwrap(), Saved::Kept(2), "a lower score keeps the earlier best");
        assert_eq!(save(&c, 1, "who", 90, 3, 0, 9000, NOON).unwrap(), Saved::Best(1));
        let day = day_of(&c, NOON).unwrap();
        let b = board(&c, "who", &day, 10).unwrap();
        assert_eq!(b.iter().map(|r| (r.name.as_str(), r.score)).collect::<Vec<_>>(), vec![("Ash", 90), ("Misty", 80)]);
        assert!(board(&c, "who", &day_of(&c, NOON + 86_400).unwrap(), 10).unwrap().is_empty(), "a new day, a new board");
    }

    #[test]
    fn type_challenge_counts_the_first_try_of_the_day() {
        let c = db();
        assert_eq!(save(&c, 3, "types", 6, 0, 0, 40_000, NOON).unwrap(), Saved::Best(1));
        assert_eq!(save(&c, 3, "types", 10, 0, 0, 20_000, NOON).unwrap(), Saved::Already(1));
        let day = day_of(&c, NOON).unwrap();
        assert_eq!(board(&c, "types", &day, 10).unwrap()[0].score, 6);
        assert_eq!(save(&c, 3, "types", 9, 0, 0, 20_000, NOON + 86_400).unwrap(), Saved::Best(1), "tomorrow is a new try");
    }
}
