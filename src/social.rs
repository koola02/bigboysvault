//! The feed: one table of posts, so the home page is one indexed query.
//!
//! A post is written by a trainer (`kind` "post", "share") or made by the site from what already happens
//! (`sync`): the day's new cards in the vault ("arrivals"), a seller's new listings ("listing"), a vouch ("vouch"),
//! the week's price moves ("prices"), a trainer joining ("joined"). Automatic posts carry a `ref` (what they stand
//! for), so each is made once and brought up to date on later syncs. Reactions and comments hang off the post's id,
//! whatever its kind. `member_id` 0 is the vault itself.
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Value};

pub const TEXT_MAX: usize = 2000;
pub const COMMENT_MAX: usize = 600;
pub const PHOTOS_MAX: usize = 4;
pub const POSTS_PER_HOUR: i64 = 6;
pub const COMMENTS_PER_HOUR: i64 = 60;
pub const REACTIONS: [(&str, &str); 6] = [("like", "👍"), ("love", "❤️"), ("fire", "🔥"), ("wow", "😮"), ("haha", "😆"), ("sad", "😢")];

/// The posts the site writes by itself, each with a switch for the admins (`set_auto`). "vault" is not a stored
/// post: it is the sets of cards that keep the feed going after the posts run out.
pub const AUTO: [(&str, &str); 6] = [
    ("arrivals", "New cards in the vault (one post a day)"),
    ("listing", "Sellers' new listings"),
    ("vouch", "Vouches"),
    ("prices", "The week's price moves"),
    ("joined", "New trainers joining"),
    ("vault", "Sets of cards from the vault after the posts run out (this keeps the feed endless)"),
];

/// The kinds an admin switched off.
pub fn auto_off(c: &Connection) -> rusqlite::Result<Vec<String>> {
    Ok(crate::blocks::setting(c, "feed_auto_off")?.unwrap_or_default().split(',').map(str::trim).filter(|k| !k.is_empty()).map(str::to_string).collect())
}

/// Switches one kind of automatic post on or off. Off: its posts leave the feed (kept, with their likes and
/// comments) and no new ones are written. On: they come back and are brought up to date.
pub fn set_auto(c: &Connection, kind: &str, on: bool, now: i64) -> rusqlite::Result<bool> {
    if !AUTO.iter().any(|(k, _)| *k == kind) {
        return Ok(false);
    }
    let mut off = auto_off(c)?;
    off.retain(|k| k != kind);
    if !on {
        off.push(kind.to_string());
    }
    crate::blocks::set_setting(c, "feed_auto_off", &off.join(","))?;
    if on {
        c.execute("UPDATE posts SET hidden=0 WHERE kind=?1 AND hidden=3", [kind])?;
        sync(c, now)?;
    } else {
        // 3: off for the whole kind (0 shown, 1 nothing left to show, 2 hidden by a person).
        c.execute("UPDATE posts SET hidden=3 WHERE kind=?1 AND hidden IN (0,1)", [kind])?;
    }
    Ok(true)
}

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS posts(
  id INTEGER PRIMARY KEY,
  member_id INTEGER NOT NULL DEFAULT 0,
  kind TEXT NOT NULL,
  ref TEXT NOT NULL DEFAULT '',
  text TEXT NOT NULL DEFAULT '',
  payload TEXT NOT NULL DEFAULT '{}',
  like_n INTEGER NOT NULL DEFAULT 0,
  comment_n INTEGER NOT NULL DEFAULT 0,
  share_n INTEGER NOT NULL DEFAULT 0,
  pinned INTEGER NOT NULL DEFAULT 0,
  hidden INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS posts_ref ON posts(ref) WHERE ref<>'';
CREATE INDEX IF NOT EXISTS posts_time ON posts(hidden, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS posts_member ON posts(member_id, created_at);
CREATE TABLE IF NOT EXISTS post_images(
  id INTEGER PRIMARY KEY, post_id INTEGER NOT NULL, path TEXT NOT NULL, thumb TEXT NOT NULL,
  w INTEGER NOT NULL DEFAULT 0, h INTEGER NOT NULL DEFAULT 0, bytes INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS post_images_post ON post_images(post_id);
CREATE TABLE IF NOT EXISTS reactions(
  post_id INTEGER NOT NULL, member_id INTEGER NOT NULL, kind TEXT NOT NULL, created_at INTEGER NOT NULL,
  PRIMARY KEY(post_id, member_id)
);
CREATE TABLE IF NOT EXISTS comments(
  id INTEGER PRIMARY KEY, post_id INTEGER NOT NULL, member_id INTEGER NOT NULL, parent_id INTEGER,
  text TEXT NOT NULL, hidden INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS comments_post ON comments(post_id, id);
CREATE INDEX IF NOT EXISTS comments_member ON comments(member_id, created_at);
";

pub fn migrate(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(SCHEMA)
}

pub fn reaction_ok(kind: &str) -> bool {
    REACTIONS.iter().any(|(k, _)| *k == kind)
}

pub fn emoji(kind: &str) -> &'static str {
    REACTIONS.iter().find(|(k, _)| *k == kind).map(|(_, e)| *e).unwrap_or("👍")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Post {
    pub id: i64,
    pub member_id: i64,
    pub kind: String,
    pub text: String,
    pub payload: Value,
    pub like_n: i64,
    pub comment_n: i64,
    pub share_n: i64,
    pub pinned: bool,
    pub hidden: bool,
    pub created_at: i64,
    /// The viewer's own reaction ("" when none).
    pub mine: String,
}

const POST_COLS: &str = "p.id, p.member_id, p.kind, p.text, p.payload, p.like_n, p.comment_n, p.share_n, p.pinned, p.hidden, p.created_at";

fn map_post(r: &Row) -> rusqlite::Result<Post> {
    let payload: String = r.get(4)?;
    Ok(Post {
        id: r.get(0)?,
        member_id: r.get(1)?,
        kind: r.get(2)?,
        text: r.get(3)?,
        payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
        like_n: r.get(5)?,
        comment_n: r.get(6)?,
        share_n: r.get(7)?,
        pinned: r.get::<_, i64>(8)? != 0,
        hidden: r.get::<_, i64>(9)? != 0,
        created_at: r.get(10)?,
        mine: r.get(11)?,
    })
}

/// A page of the feed for one viewer (0: nobody signed in): newest first, older than `before` (time, id).
/// Pinned posts are not here: `pinned` gives them, for the top of the first page.
pub fn feed(c: &Connection, viewer: i64, before: Option<(i64, i64)>, limit: i64) -> rusqlite::Result<Vec<Post>> {
    let (t, id) = before.unwrap_or((i64::MAX, i64::MAX));
    c.prepare_cached(&format!(
        "SELECT {POST_COLS}, COALESCE((SELECT kind FROM reactions r WHERE r.post_id=p.id AND r.member_id=?1), '')
         FROM posts p WHERE p.hidden=0 AND p.pinned=0 AND (p.created_at<?2 OR (p.created_at=?2 AND p.id<?3))
         ORDER BY p.created_at DESC, p.id DESC LIMIT ?4"
    ))?
    .query_map(params![viewer, t, id, limit], map_post)?
    .collect()
}

pub fn pinned(c: &Connection, viewer: i64) -> rusqlite::Result<Vec<Post>> {
    c.prepare_cached(&format!(
        "SELECT {POST_COLS}, COALESCE((SELECT kind FROM reactions r WHERE r.post_id=p.id AND r.member_id=?1), '')
         FROM posts p WHERE p.hidden=0 AND p.pinned=1 ORDER BY p.created_at DESC, p.id DESC LIMIT 3"
    ))?
    .query_map([viewer], map_post)?
    .collect()
}

pub fn post(c: &Connection, viewer: i64, id: i64) -> rusqlite::Result<Option<Post>> {
    c.query_row(
        &format!("SELECT {POST_COLS}, COALESCE((SELECT kind FROM reactions r WHERE r.post_id=p.id AND r.member_id=?1), '') FROM posts p WHERE p.id=?2"),
        [viewer, id],
        map_post,
    )
    .optional()
}

/// The reactions used most on a post, most used first (at most three), for the little row of faces.
pub fn top_reactions(c: &Connection, post: i64) -> rusqlite::Result<Vec<String>> {
    c.prepare_cached("SELECT kind FROM reactions WHERE post_id=?1 GROUP BY kind ORDER BY COUNT(*) DESC, MIN(created_at) LIMIT 3")?.query_map([post], |r| r.get(0))?.collect()
}

/// Sets, changes or (kind "") takes back the viewer's reaction. Returns how many reactions the post has now.
pub fn react(c: &Connection, post: i64, member: i64, kind: &str, now: i64) -> rusqlite::Result<Option<i64>> {
    if c.query_row("SELECT 1 FROM posts WHERE id=?1 AND hidden=0", [post], |_| Ok(())).optional()?.is_none() {
        return Ok(None);
    }
    if kind.is_empty() {
        c.execute("DELETE FROM reactions WHERE post_id=?1 AND member_id=?2", [post, member])?;
    } else {
        c.execute(
            "INSERT INTO reactions(post_id, member_id, kind, created_at) VALUES(?1,?2,?3,?4) ON CONFLICT(post_id, member_id) DO UPDATE SET kind=?3",
            params![post, member, kind, now],
        )?;
    }
    c.execute("UPDATE posts SET like_n=(SELECT COUNT(*) FROM reactions WHERE post_id=?1) WHERE id=?1", [post])?;
    c.query_row("SELECT like_n FROM posts WHERE id=?1", [post], |r| r.get(0)).map(Some)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub id: i64,
    pub post_id: i64,
    pub member_id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub avatar: String,
    pub role: String,
    pub text: String,
    pub created_at: i64,
}

fn map_comment(r: &Row) -> rusqlite::Result<Comment> {
    Ok(Comment { id: r.get(0)?, post_id: r.get(1)?, member_id: r.get(2)?, parent_id: r.get(3)?, name: r.get(4)?, avatar: r.get(5)?, role: r.get(6)?, text: r.get(7)?, created_at: r.get(8)? })
}

const COMMENT_COLS: &str = "k.id, k.post_id, k.member_id, k.parent_id, m.name, m.avatar, m.role, k.text, k.created_at";

/// A post's comments, oldest first (replies come after the comment they answer, by time).
pub fn comments(c: &Connection, post: i64, limit: i64) -> rusqlite::Result<Vec<Comment>> {
    c.prepare_cached(&format!("SELECT {COMMENT_COLS} FROM comments k JOIN members m ON m.id=k.member_id WHERE k.post_id=?1 AND k.hidden=0 ORDER BY k.id LIMIT ?2"))?
        .query_map([post, limit], map_comment)?
        .collect()
}

pub fn comment(c: &Connection, id: i64) -> rusqlite::Result<Option<Comment>> {
    c.query_row(&format!("SELECT {COMMENT_COLS} FROM comments k JOIN members m ON m.id=k.member_id WHERE k.id=?1"), [id], map_comment).optional()
}

fn recount_comments(c: &Connection, post: i64) -> rusqlite::Result<()> {
    c.execute("UPDATE posts SET comment_n=(SELECT COUNT(*) FROM comments WHERE post_id=?1 AND hidden=0) WHERE id=?1", [post]).map(|_| ())
}

/// A comment on a post, or a reply to a comment (one level: a reply to a reply answers the same comment).
pub fn comment_add(c: &Connection, post: i64, member: i64, parent: Option<i64>, text: &str, now: i64) -> rusqlite::Result<Option<i64>> {
    if c.query_row("SELECT 1 FROM posts WHERE id=?1 AND hidden=0", [post], |_| Ok(())).optional()?.is_none() {
        return Ok(None);
    }
    let parent = match parent {
        Some(p) => c.query_row("SELECT COALESCE(parent_id, id) FROM comments WHERE id=?1 AND post_id=?2", [p, post], |r| r.get::<_, i64>(0)).optional()?,
        None => None,
    };
    c.execute("INSERT INTO comments(post_id, member_id, parent_id, text, created_at) VALUES(?1,?2,?3,?4,?5)", params![post, member, parent, text, now])?;
    let id = c.last_insert_rowid();
    recount_comments(c, post)?;
    Ok(Some(id))
}

/// Takes a comment (and the replies under it) away. Returns the post it was on.
pub fn comment_remove(c: &Connection, id: i64) -> rusqlite::Result<Option<i64>> {
    let Some(post) = c.query_row("SELECT post_id FROM comments WHERE id=?1", [id], |r| r.get::<_, i64>(0)).optional()? else { return Ok(None) };
    c.execute("DELETE FROM comments WHERE id=?1 OR parent_id=?1", [id])?;
    recount_comments(c, post)?;
    Ok(Some(post))
}

pub fn comments_since(c: &Connection, member: i64, since: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM comments WHERE member_id=?1 AND created_at>=?2", [member, since], |r| r.get(0))
}

pub fn posts_since(c: &Connection, member: i64, since: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM posts WHERE member_id=?1 AND kind IN ('post','share') AND created_at>=?2", [member, since], |r| r.get(0))
}

/// One photo of a post: (full size, small, width, height, bytes on disk).
pub type Photo = (String, String, i64, i64, i64);

/// A trainer's own post: words, photos, and maybe a card (a copy of it, as in messages).
pub fn post_add(c: &Connection, member: i64, kind: &str, text: &str, photos: &[Photo], payload: &Value, now: i64) -> rusqlite::Result<i64> {
    c.execute("INSERT INTO posts(member_id, kind, text, payload, created_at) VALUES(?1,?2,?3,?4,?5)", params![member, kind, text, payload.to_string(), now])?;
    let id = c.last_insert_rowid();
    for (path, thumb, w, h, bytes) in photos {
        c.execute("INSERT INTO post_images(post_id, path, thumb, w, h, bytes) VALUES(?1,?2,?3,?4,?5,?6)", params![id, path, thumb, w, h, bytes])?;
    }
    Ok(id)
}

/// (full size, small) of each photo on a post.
pub fn photos(c: &Connection, post: i64) -> rusqlite::Result<Vec<(String, String)>> {
    c.prepare_cached("SELECT path, thumb FROM post_images WHERE post_id=?1 ORDER BY id")?.query_map([post], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
}

pub fn photo_bytes(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT COALESCE(SUM(bytes),0) FROM post_images", [], |r| r.get(0))
}

pub fn set_hidden(c: &Connection, id: i64, hidden: bool) -> rusqlite::Result<bool> {
    Ok(c.execute("UPDATE posts SET hidden=?2 WHERE id=?1", params![id, hidden])? == 1)
}

pub fn set_pinned(c: &Connection, id: i64, pinned: bool) -> rusqlite::Result<bool> {
    Ok(c.execute("UPDATE posts SET pinned=?2 WHERE id=?1", params![id, pinned])? == 1)
}

/// Deletes a post with everything on it. Returns its photo files, for the caller to remove from disk.
pub fn post_remove(c: &Connection, id: i64) -> rusqlite::Result<Vec<String>> {
    let mut files = vec![];
    for (path, thumb) in photos(c, id)? {
        files.push(path);
        files.push(thumb);
    }
    c.execute("DELETE FROM post_images WHERE post_id=?1", [id])?;
    c.execute("DELETE FROM reactions WHERE post_id=?1", [id])?;
    c.execute("DELETE FROM comments WHERE post_id=?1", [id])?;
    c.execute("DELETE FROM posts WHERE id=?1", [id])?;
    Ok(files)
}

/// Makes or refreshes an automatic post. A post an admin hid stays hidden only while `hide` says so.
fn upsert(c: &Connection, member: i64, kind: &str, refkey: &str, text: &str, payload: &Value, at: i64, bump: bool) -> rusqlite::Result<()> {
    let old: Option<(i64, String, String)> = c.query_row("SELECT id, payload, text FROM posts WHERE ref=?1", [refkey], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
    let body = payload.to_string();
    match old {
        Some((id, was, words)) => {
            if was != body || words != text {
                if bump {
                    c.execute("UPDATE posts SET payload=?2, text=?3, created_at=?4 WHERE id=?1", params![id, body, text, at])?;
                } else {
                    c.execute("UPDATE posts SET payload=?2, text=?3 WHERE id=?1", params![id, body, text])?;
                }
            }
        }
        None => {
            c.execute("INSERT INTO posts(member_id, kind, ref, text, payload, created_at) VALUES(?1,?2,?3,?4,?5,?6)", params![member, kind, refkey, text, body, at])?;
        }
    }
    Ok(())
}

/// Brings the automatic posts up to date with the site. Cheap and safe to run often.
pub fn sync(c: &Connection, now: i64) -> rusqlite::Result<()> {
    let off = auto_off(c)?;
    let on = |kind: &str| !off.iter().any(|k| k == kind);
    // 1. New in the vault: one post a day, with that day's best cards on it.
    let days: Vec<(String, i64, i64)> = if !on("arrivals") { vec![] } else { c
        .prepare("SELECT substr(created_at,1,10) d, COUNT(*), CAST(strftime('%s', MAX(created_at)) AS INTEGER) FROM products WHERE created_at<>'' AND available=1 GROUP BY d ORDER BY d DESC LIMIT 60")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<i64>>(2)?.unwrap_or(0))))?
        .collect::<Result<_, _>>()? };
    for (day, count, at) in days {
        let ids: Vec<i64> = c
            .prepare("SELECT MIN(id) FROM products WHERE substr(created_at,1,10)=?1 AND available=1 AND image<>'' GROUP BY title, variant ORDER BY MAX(price) DESC, MIN(id) LIMIT 8")?
            .query_map([&day], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        if ids.is_empty() || at <= 0 {
            continue;
        }
        upsert(c, 0, "arrivals", &format!("arrivals:{day}"), "", &json!({ "day": day, "count": count, "ids": ids }), at.min(now), true)?;
    }

    // 2. Sellers' listings: one post per seller per day (Philippine days).
    if on("listing") {
    let rows: Vec<(i64, String, i64, i64, String)> = c
        .prepare(
            "SELECT c.member_id, date(CASE WHEN c.updated_at>0 THEN c.updated_at ELSE m.created_at END, 'unixepoch', '+8 hours') d, COUNT(*),
                    MAX(CASE WHEN c.updated_at>0 THEN c.updated_at ELSE m.created_at END), group_concat(c.id)
             FROM member_cards c JOIN members m ON m.id=c.member_id
             WHERE c.kind='sale' AND c.status='Active' AND c.qty>0 AND m.seller=1 AND m.blocked=0 AND m.profile_ok=1 AND m.status<>'Vacation'
             GROUP BY c.member_id, d",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<_, _>>()?;
    let mut live: Vec<String> = vec![];
    for (member, day, count, at, ids) in rows {
        let cards: Vec<i64> = ids.split(',').filter_map(|x| x.parse().ok()).take(8).collect();
        let key = format!("listing:{member}:{day}");
        upsert(c, member, "listing", &key, "", &json!({ "count": count, "cards": cards }), at.min(now), true)?;
        live.push(key);
    }
    // A listing post whose cards all sold, moved or came down has nothing left to show.
    let old: Vec<(i64, String)> = c.prepare("SELECT id, ref FROM posts WHERE kind='listing' AND hidden=0")?.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    for (id, key) in old {
        if !live.contains(&key) {
            c.execute("UPDATE posts SET hidden=1 WHERE id=?1", [id])?;
        }
    }
    for key in &live {
        c.execute("UPDATE posts SET hidden=0 WHERE ref=?1 AND hidden=1 AND kind='listing'", [key])?;
    }
    }

    // 3. Vouches: each is a post; hidden or deleted vouches leave the feed.
    if on("vouch") {
    let vouches: Vec<(i64, i64, String, i64, i64)> = c
        .prepare("SELECT v.id, v.member_id, v.text, v.created_at, v.hidden FROM vouches v JOIN members m ON m.id=v.member_id WHERE v.parent_id IS NULL AND m.blocked=0")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<_, _>>()?;
    let mut seen: Vec<String> = vec![];
    for (id, member, text, at, hidden) in vouches {
        let key = format!("vouch:{id}");
        upsert(c, member, "vouch", &key, &text, &json!({ "vouch": id }), at, false)?;
        // 2 means a person hid the post from the feed: that stays, whatever the vouch says.
        c.execute("UPDATE posts SET hidden=?2 WHERE ref=?1 AND hidden<>?2 AND hidden IN (0,1)", params![key, hidden])?;
        seen.push(key);
    }
    let old: Vec<(i64, String)> = c.prepare("SELECT id, ref FROM posts WHERE kind='vouch'")?.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    for (id, key) in old {
        if !seen.contains(&key) {
            post_remove(c, id)?;
        }
    }
    }

    // 4. Trainers who joined (once their profile is approved).
    // One welcome post for each day's new trainers (Philippine days).
    if on("joined") {
        let days: Vec<(String, String, i64)> = c
            .prepare("SELECT date(created_at, 'unixepoch', '+8 hours') d, group_concat(id), MAX(created_at) FROM members WHERE profile_ok=1 AND blocked=0 GROUP BY d")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        let mut keys: Vec<String> = vec![];
        for (day, ids, at) in days {
            let members: Vec<i64> = ids.split(',').filter_map(|x| x.parse().ok()).collect();
            let key = format!("joined:{day}");
            upsert(c, 0, "joined", &key, "", &json!({ "members": members }), at.min(now), true)?;
            keys.push(key);
        }
        let old: Vec<(i64, String)> = c.prepare("SELECT id, ref FROM posts WHERE kind='joined'")?.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        for (id, key) in old {
            if !keys.contains(&key) {
                post_remove(c, id)?;
            }
        }
    }

    // 5. The week's biggest price moves (needs about a month of price history).
    let latest: Option<String> = if !on("prices") { None } else { c.query_row("SELECT MAX(day) FROM price_history", [], |r| r.get(0)).optional()?.flatten() };
    if let Some(latest) = latest {
        let base: Option<String> = c.query_row("SELECT MAX(day) FROM price_history WHERE day<=date(?1, '-27 days')", [&latest], |r| r.get(0)).optional()?.flatten();
        if let Some(base) = base {
            let moves: Vec<(i64, i64)> = c
                .prepare(
                    "SELECT (SELECT MIN(p.id) FROM products p WHERE p.api_id=n.api_id AND p.finish=n.finish AND p.available=1) pid,
                            CAST(ROUND((n.php - o.php) * 100.0 / o.php) AS INTEGER) pct
                     FROM price_history n JOIN price_history o ON o.api_id=n.api_id AND o.finish=n.finish AND o.day=?2
                     WHERE n.day=?1 AND o.php>=20 AND ABS(n.php - o.php) * 100.0 / o.php >= 15
                     ORDER BY ABS(n.php - o.php) * 1.0 / o.php DESC LIMIT 40",
                )?
                .query_map([&latest, &base], |r| Ok((r.get::<_, Option<i64>>(0)?.unwrap_or(0), r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|(pid, _)| *pid > 0)
                .take(5)
                .collect();
            if !moves.is_empty() {
                let week: String = c.query_row("SELECT strftime('%Y-%W', ?1)", [&latest], |r| r.get(0))?;
                let list: Vec<Value> = moves.iter().map(|(id, pct)| json!({ "id": id, "pct": pct })).collect();
                upsert(c, 0, "prices", &format!("prices:{week}"), "", &json!({ "moves": list, "since": base }), now, false)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE products(id INTEGER PRIMARY KEY, title TEXT, variant TEXT NOT NULL DEFAULT '', price REAL, available INTEGER NOT NULL DEFAULT 1, image TEXT NOT NULL DEFAULT 'x.jpg', created_at TEXT NOT NULL DEFAULT '', api_id TEXT NOT NULL DEFAULT '', finish TEXT NOT NULL DEFAULT '');
             CREATE TABLE members(id INTEGER PRIMARY KEY, name TEXT, avatar TEXT NOT NULL DEFAULT '', role TEXT NOT NULL DEFAULT '', seller INTEGER NOT NULL DEFAULT 0, blocked INTEGER NOT NULL DEFAULT 0, profile_ok INTEGER NOT NULL DEFAULT 1, status TEXT NOT NULL DEFAULT 'Active', created_at INTEGER NOT NULL DEFAULT 100);
             CREATE TABLE member_cards(id INTEGER PRIMARY KEY, member_id INTEGER, kind TEXT, status TEXT NOT NULL DEFAULT 'Active', qty INTEGER NOT NULL DEFAULT 1, updated_at INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE vouches(id INTEGER PRIMARY KEY, member_id INTEGER, parent_id INTEGER, text TEXT, hidden INTEGER NOT NULL DEFAULT 0, created_at INTEGER);
             CREATE TABLE price_history(api_id TEXT, finish TEXT, day TEXT, php REAL, PRIMARY KEY(api_id, finish, day));
             CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO members(id, name, seller) VALUES (1, 'Seller', 1), (2, 'Buyer', 0);
             INSERT INTO products(id, title, price, created_at) VALUES (1, 'Pikachu', 100, '2026-11-08T10:00:00'), (2, 'Raichu', 300, '2026-11-08T12:00:00'), (3, 'Mew', 50, '2026-11-09T09:00:00');
             INSERT INTO member_cards(id, member_id, kind, updated_at) VALUES (10, 1, 'sale', 1794300000), (11, 1, 'own', 1794300000);
             INSERT INTO vouches(id, member_id, parent_id, text, created_at) VALUES (5, 2, NULL, 'Smooth deal!', 1794200000), (6, 1, 5, 'Thanks', 1794200100);",
        )
        .unwrap();
        migrate(&c).unwrap();
        c
    }

    fn kinds(c: &Connection) -> Vec<String> {
        feed(c, 0, None, 50).unwrap().into_iter().map(|p| format!("{}:{}", p.kind, p.member_id)).collect()
    }

    #[test]
    fn the_site_writes_its_own_posts_once() {
        let c = db();
        let now = 1_794_400_000;
        sync(&c, now).unwrap();
        sync(&c, now).unwrap();
        // Two days of arrivals, one listing day for the seller, one vouch (the reply is not a post), one welcome post
        // for the two trainers who joined that day.
        assert_eq!(c.query_row("SELECT COUNT(*) FROM posts", [], |r| r.get::<_, i64>(0)).unwrap(), 5);
        let arrivals: Vec<Post> = feed(&c, 0, None, 50).unwrap().into_iter().filter(|p| p.kind == "arrivals").collect();
        assert_eq!(arrivals[0].payload["day"], "2026-11-09");
        assert_eq!(arrivals[1].payload["ids"], json!([2, 1]), "the day's best card first");
        assert_eq!(arrivals[1].payload["count"], 2);
        // A hidden vouch leaves the feed; a sold listing too; they come back when shown again.
        c.execute("UPDATE vouches SET hidden=1 WHERE id=5", []).unwrap();
        c.execute("UPDATE member_cards SET status='Sold' WHERE id=10", []).unwrap();
        sync(&c, now).unwrap();
        assert!(!kinds(&c).iter().any(|k| k.starts_with("vouch") || k.starts_with("listing")));
        c.execute("UPDATE vouches SET hidden=0 WHERE id=5", []).unwrap();
        sync(&c, now).unwrap();
        assert!(kinds(&c).contains(&"vouch:2".to_string()));
        // A deleted vouch takes its post (and what was on it) away for good.
        c.execute("DELETE FROM vouches WHERE id=5", []).unwrap();
        sync(&c, now).unwrap();
        assert_eq!(c.query_row("SELECT COUNT(*) FROM posts WHERE kind='vouch'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }

    #[test]
    fn admins_switch_automatic_posts_off_and_on() {
        let c = db();
        let now = 1_794_400_000;
        sync(&c, now).unwrap();
        let p = feed(&c, 0, None, 50).unwrap().into_iter().find(|p| p.kind == "joined").unwrap().id;
        react(&c, p, 1, "like", 1).unwrap();
        assert!(set_auto(&c, "joined", false, now).unwrap());
        sync(&c, now).unwrap();
        assert!(!kinds(&c).iter().any(|k| k.starts_with("joined")), "off: gone from the feed, and not written again");
        assert_eq!(auto_off(&c).unwrap(), vec!["joined".to_string()]);
        assert!(kinds(&c).iter().any(|k| k.starts_with("arrivals")), "the other kinds stay");
        assert!(set_auto(&c, "joined", true, now).unwrap());
        assert_eq!(post(&c, 1, p).unwrap().unwrap().mine, "like", "on again: the same posts, likes and all");
        assert!(kinds(&c).iter().any(|k| k.starts_with("joined")));
        assert!(!set_auto(&c, "nonsense", false, now).unwrap());
    }

    #[test]
    fn pages_go_back_in_time_without_gaps() {
        let c = db();
        for i in 0..7 {
            post_add(&c, 2, "post", &format!("post {i}"), &[], &json!({}), 1000 + i / 2).unwrap();
        }
        let first = feed(&c, 0, None, 3).unwrap();
        assert_eq!(first.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), ["post 6", "post 5", "post 4"]);
        let last = first.last().unwrap();
        let second = feed(&c, 0, Some((last.created_at, last.id)), 3).unwrap();
        assert_eq!(second.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), ["post 3", "post 2", "post 1"]);
        // Pinned posts sit apart; hidden ones are nowhere.
        set_pinned(&c, first[0].id, true).unwrap();
        set_hidden(&c, first[1].id, true).unwrap();
        assert_eq!(feed(&c, 0, None, 3).unwrap()[0].text, "post 4");
        assert_eq!(pinned(&c, 0).unwrap()[0].text, "post 6");
    }

    #[test]
    fn reactions_and_comments_count() {
        let c = db();
        let p = post_add(&c, 2, "post", "hello", &[("a_l.jpg".into(), "a_s.jpg".into(), 10, 10, 500)], &json!({}), 1000).unwrap();
        assert_eq!(react(&c, p, 1, "fire", 1).unwrap(), Some(1));
        assert_eq!(react(&c, p, 2, "like", 2).unwrap(), Some(2));
        assert_eq!(react(&c, p, 1, "like", 3).unwrap(), Some(2), "changing a reaction doesn't count twice");
        assert_eq!(post(&c, 1, p).unwrap().unwrap().mine, "like");
        assert_eq!(top_reactions(&c, p).unwrap(), vec!["like".to_string()]);
        assert_eq!(react(&c, p, 1, "", 4).unwrap(), Some(1));
        assert_eq!(react(&c, 999, 1, "like", 4).unwrap(), None);
        let a = comment_add(&c, p, 1, None, "Nice card", 10).unwrap().unwrap();
        let b = comment_add(&c, p, 2, Some(a), "Thanks", 11).unwrap().unwrap();
        let d = comment_add(&c, p, 1, Some(b), "Welcome", 12).unwrap().unwrap();
        assert_eq!(comment(&c, d).unwrap().unwrap().parent_id, Some(a), "a reply to a reply answers the same comment");
        assert_eq!(post(&c, 0, p).unwrap().unwrap().comment_n, 3);
        assert_eq!(comment_remove(&c, a).unwrap(), Some(p));
        assert_eq!(post(&c, 0, p).unwrap().unwrap().comment_n, 0, "its replies go with it");
        assert_eq!(photo_bytes(&c).unwrap(), 500);
        assert_eq!(post_remove(&c, p).unwrap(), vec!["a_l.jpg".to_string(), "a_s.jpg".to_string()]);
        assert_eq!(photo_bytes(&c).unwrap(), 0);
    }
}
