//! The feed: one table of posts, so the home page is one indexed query.
//!
//! A post is written by a trainer (`kind` "post", "share") or made by the site from what already happens
//! (`sync`): the day's new cards in the vault ("arrivals"), a seller's new listings ("listing"), a vouch ("vouch"),
//! a trainer joining ("joined"). (The week's price moves, "prices", left the feed on 18 Nov 2026: the owner wants
//! prices only where cards are shopped.) Automatic posts carry a `ref` (what they stand
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
/// post: it is the sets of cards from the stores' binders (the vault's and the trainers', taking turns) that keep the
/// feed going after the posts run out. (Its name is from when they were the vault's binders only.)
pub const AUTO: [(&str, &str); 5] = [
    ("arrivals", "New cards in the vault (one post a day)"),
    ("listing", "Sellers' new listings"),
    ("vouch", "Vouches"),
    ("joined", "New trainers joining"),
    ("vault", "Sets of cards from the stores' binders after the posts run out (this keeps the feed endless)"),
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
        // (Switched back on: the posts come back as they were; nobody is told about old cards.)
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
CREATE TABLE IF NOT EXISTS comment_reactions(
  comment_id INTEGER NOT NULL, member_id INTEGER NOT NULL, kind TEXT NOT NULL, created_at INTEGER NOT NULL,
  PRIMARY KEY(comment_id, member_id)
);
CREATE TABLE IF NOT EXISTS photo_reactions(
  image_id INTEGER NOT NULL, member_id INTEGER NOT NULL, kind TEXT NOT NULL, created_at INTEGER NOT NULL,
  PRIMARY KEY(image_id, member_id)
);
CREATE TABLE IF NOT EXISTS reports(
  id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, ref_id INTEGER NOT NULL, member_id INTEGER NOT NULL,
  reason TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL, UNIQUE(kind, ref_id, member_id)
);
";

pub fn migrate(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(SCHEMA)?;
    // Added after the first release of the feed: the words someone adds to an automatic post, and when a post or
    // comment was last edited.
    // And (19 Nov 2026) a comment's likes and its one photo.
    for (table, col, kind) in [
        ("posts", "note", "TEXT NOT NULL DEFAULT ''"),
        ("posts", "edited_at", "INTEGER NOT NULL DEFAULT 0"),
        // The post's words with their bold, italic, underline and sizes (`rich.rs`), when any are formatted.
        ("posts", "rich", "TEXT NOT NULL DEFAULT ''"),
        ("comments", "edited_at", "INTEGER NOT NULL DEFAULT 0"),
        ("comments", "like_n", "INTEGER NOT NULL DEFAULT 0"),
        ("comments", "image", "TEXT NOT NULL DEFAULT ''"),
        ("comments", "thumb", "TEXT NOT NULL DEFAULT ''"),
        ("comments", "bytes", "INTEGER NOT NULL DEFAULT 0"),
        // Each photo of a written post has its own likes and comments (the owner, 19 Nov 2026: "unique comment,react,
        // etc. per photo, not just show the entire post"): a photo's counts, and the photo a comment is on (0: the post).
        ("post_images", "like_n", "INTEGER NOT NULL DEFAULT 0"),
        ("post_images", "comment_n", "INTEGER NOT NULL DEFAULT 0"),
        ("comments", "image_id", "INTEGER NOT NULL DEFAULT 0"),
        // Each photo's own caption (the owner, 20 Nov 2026: "add ability to edit or add a caption in every photos").
        ("post_images", "caption", "TEXT NOT NULL DEFAULT ''"),
    ] {
        let there: i64 = c.query_row(&format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name=?1"), [col], |r| r.get(0))?;
        if there == 0 {
            c.execute(&format!("ALTER TABLE {table} ADD COLUMN {col} {kind}"), [])?;
        }
    }
    c.execute_batch("CREATE INDEX IF NOT EXISTS comments_image ON comments(image_id, id)")?;
    // Price moves left the feed (18 Nov 2026): the posts already written stay, out of sight, with their likes.
    c.execute("UPDATE posts SET hidden=3 WHERE kind='prices' AND hidden IN (0,1)", [])?;
    Ok(())
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
    /// Words added by the seller (their listing post) or an admin (the vault's posts) to an automatic post.
    pub note: String,
    pub edited_at: i64,
    /// The words (`text`, or `note` on an automatic post) cut into styled pieces, as JSON (`rich.rs`). "" when plain.
    pub rich: String,
    /// The viewer's own reaction ("" when none).
    pub mine: String,
}

impl Post {
    /// Written by a person (not made by the site).
    pub fn written(&self) -> bool {
        self.kind == "post" || self.kind == "share"
    }
}

const POST_COLS: &str = "p.id, p.member_id, p.kind, p.text, p.payload, p.like_n, p.comment_n, p.share_n, p.pinned, p.hidden, p.created_at, p.note, p.edited_at, p.rich";

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
        note: r.get(11)?,
        edited_at: r.get(12)?,
        rich: r.get(13)?,
        mine: r.get(14)?,
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

/// Who reacted to a post, newest first: (member, name, picture, reaction).
pub fn reactions_of(c: &Connection, post: i64, limit: i64) -> rusqlite::Result<Vec<(i64, String, String, String)>> {
    c.prepare("SELECT m.id, m.name, m.avatar, r.kind FROM reactions r JOIN members m ON m.id=r.member_id WHERE r.post_id=?1 AND m.blocked=0 ORDER BY r.created_at DESC LIMIT ?2")?
        .query_map(params![post, limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect()
}

/// Someone reports a post or a comment ("post" / "comment") to the vault's staff, once each. True when it is new.
pub fn report(c: &Connection, kind: &str, id: i64, member: i64, reason: &str, now: i64) -> rusqlite::Result<bool> {
    Ok(c.execute("INSERT OR IGNORE INTO reports(kind, ref_id, member_id, reason, created_at) VALUES(?1,?2,?3,?4,?5)", params![kind, id, member, reason, now])? == 1)
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
    pub edited_at: i64,
    /// How many reacted to it, and the viewer's own reaction ("" when none).
    pub like_n: i64,
    pub mine: String,
    /// Its one photo, if any: full size and small.
    pub image: String,
    pub thumb: String,
    /// The photo of the post it is on (`post_images.id`); 0 when it is on the post itself.
    pub image_id: i64,
}

fn map_comment(r: &Row) -> rusqlite::Result<Comment> {
    Ok(Comment {
        id: r.get(0)?,
        post_id: r.get(1)?,
        member_id: r.get(2)?,
        parent_id: r.get(3)?,
        name: r.get(4)?,
        avatar: r.get(5)?,
        role: r.get(6)?,
        text: r.get(7)?,
        created_at: r.get(8)?,
        edited_at: r.get(9)?,
        like_n: r.get(10)?,
        image: r.get(11)?,
        thumb: r.get(12)?,
        image_id: r.get(13)?,
        mine: r.get(14)?,
    })
}

const COMMENT_COLS: &str = "k.id, k.post_id, k.member_id, k.parent_id, m.name, m.avatar, m.role, k.text, k.created_at, k.edited_at, k.like_n, k.image, k.thumb, k.image_id";

/// A post's comments for one viewer (0: nobody signed in), oldest first (replies come after the comment they answer).
/// The comments on its photos are not among them: each photo has its own (`comments_on`).
pub fn comments(c: &Connection, post: i64, viewer: i64, limit: i64) -> rusqlite::Result<Vec<Comment>> {
    comments_on(c, post, 0, viewer, limit)
}

/// The comments on one photo of a post (`image`, a `post_images.id`), or on the post itself (0).
pub fn comments_on(c: &Connection, post: i64, image: i64, viewer: i64, limit: i64) -> rusqlite::Result<Vec<Comment>> {
    c.prepare_cached(&format!(
        "SELECT {COMMENT_COLS}, COALESCE((SELECT kind FROM comment_reactions x WHERE x.comment_id=k.id AND x.member_id=?3), '')
         FROM comments k JOIN members m ON m.id=k.member_id WHERE k.post_id=?1 AND k.image_id=?4 AND k.hidden=0 ORDER BY k.id LIMIT ?2"
    ))?
    .query_map(params![post, limit, viewer, image], map_comment)?
    .collect()
}

pub fn comment(c: &Connection, id: i64) -> rusqlite::Result<Option<Comment>> {
    comment_for(c, id, 0)
}

/// One comment, with the viewer's own reaction.
pub fn comment_for(c: &Connection, id: i64, viewer: i64) -> rusqlite::Result<Option<Comment>> {
    c.query_row(
        &format!("SELECT {COMMENT_COLS}, COALESCE((SELECT kind FROM comment_reactions x WHERE x.comment_id=k.id AND x.member_id=?2), '') FROM comments k JOIN members m ON m.id=k.member_id WHERE k.id=?1"),
        [id, viewer],
        map_comment,
    )
    .optional()
}

/// Sets, changes or (kind "") takes back the viewer's reaction to a comment, as on a post. Returns how many reactions
/// the comment has now.
pub fn comment_react(c: &Connection, comment: i64, member: i64, kind: &str, now: i64) -> rusqlite::Result<Option<i64>> {
    if c.query_row("SELECT 1 FROM comments WHERE id=?1 AND hidden=0", [comment], |_| Ok(())).optional()?.is_none() {
        return Ok(None);
    }
    if kind.is_empty() {
        c.execute("DELETE FROM comment_reactions WHERE comment_id=?1 AND member_id=?2", [comment, member])?;
    } else {
        c.execute(
            "INSERT INTO comment_reactions(comment_id, member_id, kind, created_at) VALUES(?1,?2,?3,?4) ON CONFLICT(comment_id, member_id) DO UPDATE SET kind=?3",
            params![comment, member, kind, now],
        )?;
    }
    c.execute("UPDATE comments SET like_n=(SELECT COUNT(*) FROM comment_reactions WHERE comment_id=?1) WHERE id=?1", [comment])?;
    c.query_row("SELECT like_n FROM comments WHERE id=?1", [comment], |r| r.get(0)).map(Some)
}

/// The reactions used most on a comment, most used first (at most three).
pub fn comment_tops(c: &Connection, comment: i64) -> rusqlite::Result<Vec<String>> {
    c.prepare_cached("SELECT kind FROM comment_reactions WHERE comment_id=?1 GROUP BY kind ORDER BY COUNT(*) DESC, MIN(created_at) LIMIT 3")?.query_map([comment], |r| r.get(0))?.collect()
}

/// A post counts the comments on itself; each of its photos counts its own.
fn recount_comments(c: &Connection, post: i64) -> rusqlite::Result<()> {
    c.execute("UPDATE posts SET comment_n=(SELECT COUNT(*) FROM comments WHERE post_id=?1 AND image_id=0 AND hidden=0) WHERE id=?1", [post])?;
    c.execute("UPDATE post_images SET comment_n=(SELECT COUNT(*) FROM comments k WHERE k.image_id=post_images.id AND k.hidden=0) WHERE post_id=?1", [post]).map(|_| ())
}

/// A comment on a post, or a reply to a comment or to another reply (the owner, 19 Nov 2026: "comment within a
/// comment"): each keeps the comment it answers, so threads can go as deep as people take them. With maybe one photo
/// (full size, small, bytes on disk).
pub fn comment_add(c: &Connection, post: i64, member: i64, parent: Option<i64>, text: &str, photo: Option<(&str, &str, i64)>, now: i64) -> rusqlite::Result<Option<i64>> {
    comment_add_on(c, post, 0, member, parent, text, photo, now)
}

/// The same, on one photo of the post (`on`, a `post_images.id` that has its own likes and comments) or on the post
/// itself (0). A reply stays where the comment it answers is.
#[allow(clippy::too_many_arguments)]
pub fn comment_add_on(c: &Connection, post: i64, on: i64, member: i64, parent: Option<i64>, text: &str, photo: Option<(&str, &str, i64)>, now: i64) -> rusqlite::Result<Option<i64>> {
    if c.query_row("SELECT 1 FROM posts WHERE id=?1 AND hidden=0", [post], |_| Ok(())).optional()?.is_none() {
        return Ok(None);
    }
    if on != 0 && !photo_has_its_own(c, post, on)? {
        return Ok(None);
    }
    let parent = match parent {
        Some(p) => c.query_row("SELECT id FROM comments WHERE id=?1 AND post_id=?2 AND image_id=?3", [p, post, on], |r| r.get::<_, i64>(0)).optional()?,
        None => None,
    };
    let (image, thumb, bytes) = photo.unwrap_or(("", "", 0));
    c.execute(
        "INSERT INTO comments(post_id, member_id, parent_id, text, created_at, image, thumb, bytes, image_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![post, member, parent, text, now, image, thumb, bytes, on],
    )?;
    let id = c.last_insert_rowid();
    recount_comments(c, post)?;
    Ok(Some(id))
}

/// A comment and everything under it: the replies to it, the replies to those, and so on (with one number, ?1).
const THREAD: &str = "id IN (WITH RECURSIVE t(id) AS (SELECT ?1 UNION ALL SELECT k.id FROM comments k JOIN t ON k.parent_id=t.id) SELECT id FROM t)";

/// Takes a comment (and every reply under it, however deep) away, with their likes. Returns the post it was on, and
/// the photo files to remove from disk.
pub fn comment_remove(c: &Connection, id: i64) -> rusqlite::Result<Option<(i64, Vec<String>)>> {
    let Some(post) = c.query_row("SELECT post_id FROM comments WHERE id=?1", [id], |r| r.get::<_, i64>(0)).optional()? else { return Ok(None) };
    let files = comment_files(c, THREAD, id)?;
    c.execute(&format!("DELETE FROM comment_reactions WHERE comment_id IN (SELECT id FROM comments WHERE {THREAD})"), [id])?;
    c.execute(&format!("DELETE FROM comments WHERE {THREAD}"), [id])?;
    recount_comments(c, post)?;
    Ok(Some((post, files)))
}

/// The photo files of the comments `filter` picks (with one number, ?1).
fn comment_files(c: &Connection, filter: &str, n: i64) -> rusqlite::Result<Vec<String>> {
    let rows: Vec<(String, String)> = c.prepare(&format!("SELECT image, thumb FROM comments WHERE ({filter}) AND image<>''"))?.query_map([n], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    Ok(rows.into_iter().flat_map(|(a, b)| [a, b]).filter(|f| !f.is_empty()).collect())
}

pub fn comments_since(c: &Connection, member: i64, since: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM comments WHERE member_id=?1 AND created_at>=?2", [member, since], |r| r.get(0))
}

pub fn posts_since(c: &Connection, member: i64, since: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM posts WHERE member_id=?1 AND kind IN ('post','share') AND created_at>=?2", [member, since], |r| r.get(0))
}

/// Does this photo have likes and comments of its own? The photos of a post a person wrote do when the post has two
/// or more (the owner, 19 Nov 2026: "unique comment,react, etc. per photo, not just show the entire post, this doesn't
/// apply to card listings etc. just organic posts"). A post's only photo is the post, as on Facebook: its likes and
/// comments are the post's. A photo that already has some of its own keeps them if the others are taken off.
pub fn photo_has_its_own(c: &Connection, post: i64, image: i64) -> rusqlite::Result<bool> {
    let found: Option<(String, i64, i64, i64)> = c
        .query_row(
            "SELECT p.kind, (SELECT COUNT(*) FROM post_images x WHERE x.post_id=p.id), i.like_n, i.comment_n
             FROM post_images i JOIN posts p ON p.id=i.post_id WHERE i.id=?2 AND i.post_id=?1 AND p.hidden=0",
            [post, image],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    Ok(found.is_some_and(|(kind, photos, likes, said)| (kind == "post" || kind == "share") && (photos >= 2 || likes > 0 || said > 0)))
}

/// Do this post's photos have their own likes and comments? (What the page needs to know to open a photo.)
pub fn photos_have_their_own(c: &Connection, p: &Post, photos: usize) -> rusqlite::Result<bool> {
    if !p.written() || photos == 0 {
        return Ok(false);
    }
    if photos >= 2 {
        return Ok(true);
    }
    Ok(c.query_row("SELECT COUNT(*) FROM post_images WHERE post_id=?1 AND (like_n>0 OR comment_n>0)", [p.id], |r| r.get::<_, i64>(0))? > 0)
}

/// One photo of a post, with its own likes and comments, for one viewer.
#[derive(Clone, Debug, PartialEq)]
pub struct PhotoOwn {
    pub id: i64,
    pub post_id: i64,
    pub like_n: i64,
    pub comment_n: i64,
    /// The viewer's own reaction ("" when none).
    pub mine: String,
    /// Which photo of the post it is (1 is the first), and how many the post has.
    pub at: i64,
    pub of: i64,
}

pub fn photo_own(c: &Connection, post: i64, image: i64, viewer: i64) -> rusqlite::Result<Option<PhotoOwn>> {
    if !photo_has_its_own(c, post, image)? {
        return Ok(None);
    }
    c.query_row(
        "SELECT i.id, i.post_id, i.like_n, i.comment_n, COALESCE((SELECT kind FROM photo_reactions r WHERE r.image_id=i.id AND r.member_id=?2), ''),
                (SELECT COUNT(*) FROM post_images x WHERE x.post_id=i.post_id AND x.id<=i.id), (SELECT COUNT(*) FROM post_images x WHERE x.post_id=i.post_id)
         FROM post_images i WHERE i.id=?1",
        [image, viewer],
        |r| Ok(PhotoOwn { id: r.get(0)?, post_id: r.get(1)?, like_n: r.get(2)?, comment_n: r.get(3)?, mine: r.get(4)?, at: r.get(5)?, of: r.get(6)? }),
    )
    .optional()
}

/// Sets, changes or (kind "") takes back the viewer's reaction to one photo, as on a post. Returns how many reactions
/// the photo has now. None: the photo isn't there, or its likes are its post's.
pub fn photo_react(c: &Connection, post: i64, image: i64, member: i64, kind: &str, now: i64) -> rusqlite::Result<Option<i64>> {
    if !photo_has_its_own(c, post, image)? {
        return Ok(None);
    }
    if kind.is_empty() {
        c.execute("DELETE FROM photo_reactions WHERE image_id=?1 AND member_id=?2", [image, member])?;
    } else {
        c.execute(
            "INSERT INTO photo_reactions(image_id, member_id, kind, created_at) VALUES(?1,?2,?3,?4) ON CONFLICT(image_id, member_id) DO UPDATE SET kind=?3",
            params![image, member, kind, now],
        )?;
    }
    c.execute("UPDATE post_images SET like_n=(SELECT COUNT(*) FROM photo_reactions WHERE image_id=?1) WHERE id=?1", [image])?;
    c.query_row("SELECT like_n FROM post_images WHERE id=?1", [image], |r| r.get(0)).map(Some)
}

/// The reactions used most on a photo, most used first (at most three).
pub fn photo_tops(c: &Connection, image: i64) -> rusqlite::Result<Vec<String>> {
    c.prepare_cached("SELECT kind FROM photo_reactions WHERE image_id=?1 GROUP BY kind ORDER BY COUNT(*) DESC, MIN(created_at) LIMIT 3")?.query_map([image], |r| r.get(0))?.collect()
}

/// Who reacted to a photo, newest first: (member, name, picture, reaction).
pub fn photo_reactions_of(c: &Connection, image: i64, limit: i64) -> rusqlite::Result<Vec<(i64, String, String, String)>> {
    c.prepare("SELECT m.id, m.name, m.avatar, r.kind FROM photo_reactions r JOIN members m ON m.id=r.member_id WHERE r.image_id=?1 AND m.blocked=0 ORDER BY r.created_at DESC LIMIT ?2")?
        .query_map(params![image, limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect()
}

/// Everything said about one photo goes when the photo does: its likes, its comments and theirs. Returns the files
/// of the photos in those comments, for the caller to remove from disk.
fn photo_forget(c: &Connection, image: i64) -> rusqlite::Result<Vec<String>> {
    let files = comment_files(c, "image_id=?1", image)?;
    c.execute("DELETE FROM photo_reactions WHERE image_id=?1", [image])?;
    c.execute("DELETE FROM comment_reactions WHERE comment_id IN (SELECT id FROM comments WHERE image_id=?1)", [image])?;
    c.execute("DELETE FROM comments WHERE image_id=?1", [image])?;
    Ok(files)
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
    Ok(photos_ids(c, post)?.into_iter().map(|(_, full, small)| (full, small)).collect())
}

/// (id, full size, small) of each photo on a post.
pub fn photos_ids(c: &Connection, post: i64) -> rusqlite::Result<Vec<(i64, String, String)>> {
    c.prepare_cached("SELECT id, path, thumb FROM post_images WHERE post_id=?1 ORDER BY id")?.query_map([post], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect()
}

/// Editing a written post: its words, which of its photos stay, and new ones. Returns the files of the photos
/// taken off, for the caller to remove from disk.
pub fn post_edit(c: &Connection, id: i64, text: &str, keep: &[i64], add: &[Photo], now: i64) -> rusqlite::Result<Vec<String>> {
    let mut files = vec![];
    for (pid, path, thumb) in photos_ids(c, id)? {
        if !keep.contains(&pid) {
            files.extend(photo_forget(c, pid)?);
            c.execute("DELETE FROM post_images WHERE id=?1", [pid])?;
            files.push(path);
            files.push(thumb);
        }
    }
    for (path, thumb, w, h, bytes) in add {
        c.execute("INSERT INTO post_images(post_id, path, thumb, w, h, bytes) VALUES(?1,?2,?3,?4,?5,?6)", params![id, path, thumb, w, h, bytes])?;
    }
    // New words are plain until `post_rich` says how they are formatted: old styles never sit on new words.
    c.execute("UPDATE posts SET text=?2, rich='', edited_at=?3 WHERE id=?1", params![id, text, now])?;
    Ok(files)
}

/// The longest caption a photo takes.
pub const CAPTION_MAX: usize = 300;

/// Each photo's caption, in the post's order ("" for none).
pub fn photo_captions(c: &Connection, post: i64) -> rusqlite::Result<Vec<String>> {
    c.prepare_cached("SELECT caption FROM post_images WHERE post_id=?1 ORDER BY id")?.query_map([post], |r| r.get(0))?.collect()
}

/// Sets one photo's caption (tidied; "" takes it off). False when the photo isn't on that post.
pub fn set_photo_caption(c: &Connection, post: i64, image: i64, caption: &str) -> rusqlite::Result<bool> {
    let caption = crate::members::clean_line(caption, CAPTION_MAX);
    Ok(c.execute("UPDATE post_images SET caption=?3 WHERE id=?2 AND post_id=?1", params![post, image, caption])? == 1)
}

/// Captions for the photos just added to a post: the ones not in `before`, in the order they went up.
pub fn caption_new_photos(c: &Connection, post: i64, before: &[i64], captions: &[String]) -> rusqlite::Result<()> {
    let fresh: Vec<i64> = photos_ids(c, post)?.into_iter().map(|x| x.0).filter(|id| !before.contains(id)).collect();
    for (id, cap) in fresh.iter().zip(captions) {
        set_photo_caption(c, post, *id, cap)?;
    }
    Ok(())
}

/// One author's written posts (0: the vault's), newest first, older than `before` (time, id): what their own page shows.
pub fn posts_by(c: &Connection, viewer: i64, author: i64, before: Option<(i64, i64)>, limit: i64) -> rusqlite::Result<Vec<Post>> {
    let (t, id) = before.unwrap_or((i64::MAX, i64::MAX));
    c.prepare_cached(&format!(
        "SELECT {POST_COLS}, COALESCE((SELECT kind FROM reactions r WHERE r.post_id=p.id AND r.member_id=?1), '')
         FROM posts p WHERE p.member_id=?2 AND p.kind IN ('post','share') AND p.hidden=0 AND (p.created_at<?3 OR (p.created_at=?3 AND p.id<?4))
         ORDER BY p.created_at DESC, p.id DESC LIMIT ?5"
    ))?
    .query_map(params![viewer, author, t, id, limit], map_post)?
    .collect()
}

/// How a post's words are formatted: the runs `rich::clean` made for those same words, or "" for plain.
pub fn post_rich(c: &Connection, id: i64, rich: &str) -> rusqlite::Result<()> {
    c.execute("UPDATE posts SET rich=?2 WHERE id=?1", params![id, rich]).map(|_| ())
}

/// The words a seller or an admin puts on an automatic post. The site's refresh never touches them.
pub fn post_note(c: &Connection, id: i64, note: &str, now: i64) -> rusqlite::Result<()> {
    c.execute("UPDATE posts SET note=?2, rich='', edited_at=?3 WHERE id=?1", params![id, note, now]).map(|_| ())
}

pub fn comment_edit(c: &Connection, id: i64, text: &str, now: i64) -> rusqlite::Result<bool> {
    Ok(c.execute("UPDATE comments SET text=?2, edited_at=?3 WHERE id=?1", params![id, text, now])? == 1)
}

/// The space the feed's photos take: posts' and comments'.
pub fn photo_bytes(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT (SELECT COALESCE(SUM(bytes),0) FROM post_images) + (SELECT COALESCE(SUM(bytes),0) FROM comments)", [], |r| r.get(0))
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
    files.extend(comment_files(c, "post_id=?1", id)?);
    c.execute("DELETE FROM photo_reactions WHERE image_id IN (SELECT id FROM post_images WHERE post_id=?1)", [id])?;
    c.execute("DELETE FROM post_images WHERE post_id=?1", [id])?;
    c.execute("DELETE FROM reactions WHERE post_id=?1", [id])?;
    c.execute("DELETE FROM comment_reactions WHERE comment_id IN (SELECT id FROM comments WHERE post_id=?1)", [id])?;
    c.execute("DELETE FROM comments WHERE post_id=?1", [id])?;
    c.execute("DELETE FROM posts WHERE id=?1", [id])?;
    Ok(files)
}

/// Makes or refreshes an automatic post. A post an admin hid stays hidden only while `hide` says so. True when the post
/// is new.
fn upsert(c: &Connection, member: i64, kind: &str, refkey: &str, text: &str, payload: &Value, at: i64, bump: bool) -> rusqlite::Result<bool> {
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
            return Ok(true);
        }
    }
    Ok(false)
}

/// Brings the automatic posts up to date with the site. Cheap and safe to run often. Answers the stores that just put
/// up cards (a new post about the day's cards, from the last day): their ids, 0 the vault's, so followers can be told.
pub fn sync(c: &Connection, now: i64) -> rusqlite::Result<Vec<i64>> {
    let mut fresh: Vec<i64> = vec![];
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
        if upsert(c, 0, "arrivals", &format!("arrivals:{day}"), "", &json!({ "day": day, "count": count, "ids": ids }), at.min(now), true)? && now - at < 86_400 && !fresh.contains(&0) {
            fresh.push(0);
        }
    }

    // 2. Sellers' listings: one post per seller per day (Philippine days).
    if on("listing") {
    let rows: Vec<(i64, String, i64, i64, String)> = c
        .prepare(
            "SELECT c.member_id, date(CASE WHEN c.updated_at>0 THEN c.updated_at ELSE m.created_at END, 'unixepoch', '+8 hours') d, COUNT(*),
                    MAX(CASE WHEN c.updated_at>0 THEN c.updated_at ELSE m.created_at END), group_concat(c.id)
             FROM member_cards c JOIN members m ON m.id=c.member_id
             WHERE c.kind='sale' AND c.status='Active' AND c.qty>0 AND m.seller=1 AND m.blocked=0 AND m.profile_ok=1 AND m.status<>'Vacation' AND m.store_hidden=0
             GROUP BY c.member_id, d",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<_, _>>()?;
    let mut live: Vec<String> = vec![];
    for (member, day, count, at, ids) in rows {
        let cards: Vec<i64> = ids.split(',').filter_map(|x| x.parse().ok()).take(8).collect();
        let key = format!("listing:{member}:{day}");
        if upsert(c, member, "listing", &key, "", &json!({ "count": count, "cards": cards }), at.min(now), true)? && now - at < 86_400 && !fresh.contains(&member) {
            fresh.push(member);
        }
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

    Ok(fresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE products(id INTEGER PRIMARY KEY, title TEXT, variant TEXT NOT NULL DEFAULT '', price REAL, available INTEGER NOT NULL DEFAULT 1, image TEXT NOT NULL DEFAULT 'x.jpg', created_at TEXT NOT NULL DEFAULT '', api_id TEXT NOT NULL DEFAULT '', finish TEXT NOT NULL DEFAULT '');
             CREATE TABLE members(id INTEGER PRIMARY KEY, name TEXT, avatar TEXT NOT NULL DEFAULT '', role TEXT NOT NULL DEFAULT '', seller INTEGER NOT NULL DEFAULT 0, blocked INTEGER NOT NULL DEFAULT 0, profile_ok INTEGER NOT NULL DEFAULT 1, status TEXT NOT NULL DEFAULT 'Active', store_hidden INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL DEFAULT 100);
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
        let a = comment_add(&c, p, 1, None, "Nice card", None, 10).unwrap().unwrap();
        let b = comment_add(&c, p, 2, Some(a), "Thanks", None, 11).unwrap().unwrap();
        let d = comment_add(&c, p, 1, Some(b), "Welcome", None, 12).unwrap().unwrap();
        assert_eq!(comment(&c, d).unwrap().unwrap().parent_id, Some(b), "a reply to a reply keeps the reply it answers");
        let other = post_add(&c, 2, "post", "another", &[], &json!({}), 1001).unwrap();
        let stray = comment_add(&c, other, 1, Some(a), "wrong post", None, 13).unwrap().unwrap();
        assert_eq!(comment(&c, stray).unwrap().unwrap().parent_id, None, "never under a comment of another post");
        let e = comment_add(&c, p, 2, None, "A second thread", None, 14).unwrap().unwrap();
        comment_react(&c, d, 2, "like", 15).unwrap();
        assert_eq!(post(&c, 0, p).unwrap().unwrap().comment_n, 4);
        assert_eq!(comment_remove(&c, b).unwrap(), Some((p, vec![])));
        assert_eq!(comments(&c, p, 0, 50).unwrap().iter().map(|k| k.id).collect::<Vec<_>>(), vec![a, e], "a reply goes with every reply under it; the rest stay");
        assert_eq!(c.query_row("SELECT COUNT(*) FROM comment_reactions", [], |r| r.get::<_, i64>(0)).unwrap(), 0, "their likes go too");
        let f = comment_add(&c, p, 1, Some(a), "Under the first", None, 16).unwrap().unwrap();
        comment_add(&c, p, 2, Some(f), "And under that", None, 17).unwrap().unwrap();
        assert_eq!(comment_remove(&c, a).unwrap(), Some((p, vec![])));
        assert_eq!(comments(&c, p, 0, 50).unwrap().iter().map(|k| k.id).collect::<Vec<_>>(), vec![e], "a comment goes with its whole thread, however deep");
        comment_remove(&c, e).unwrap();
        comment_remove(&c, stray).unwrap();
        assert_eq!(post(&c, 0, p).unwrap().unwrap().comment_n, 0);
        assert_eq!(photo_bytes(&c).unwrap(), 500);
        // Editing: new words, the old photo comes off, a new one goes on; the site's refresh leaves a note alone.
        assert_eq!(post_edit(&c, p, "hello again", &[], &[("b_l.jpg".into(), "b_s.jpg".into(), 5, 5, 200)], 50).unwrap(), vec!["a_l.jpg".to_string(), "a_s.jpg".to_string()]);
        assert_eq!(photos(&c, p).unwrap(), vec![("b_l.jpg".to_string(), "b_s.jpg".to_string())]);
        let kept = photos_ids(&c, p).unwrap()[0].0;
        assert!(post_edit(&c, p, "hello again", &[kept], &[], 50).unwrap().is_empty(), "a photo that is kept stays");
        let now = post(&c, 0, p).unwrap().unwrap();
        assert_eq!((now.text.as_str(), now.edited_at, photo_bytes(&c).unwrap()), ("hello again", 50, 200));
        let k = comment_add(&c, p, 1, None, "frst", None, 60).unwrap().unwrap();
        assert!(comment_edit(&c, k, "first", 61).unwrap());
        assert_eq!(comment(&c, k).unwrap().unwrap().text, "first");
        comment_remove(&c, k).unwrap();
        post_note(&c, p, "a note", 70).unwrap();
        assert_eq!(post(&c, 0, p).unwrap().unwrap().note, "a note");
        assert_eq!(post_remove(&c, p).unwrap(), vec!["b_l.jpg".to_string(), "b_s.jpg".to_string()]);
        assert_eq!(photo_bytes(&c).unwrap(), 0);
    }

    #[test]
    fn comments_take_likes_and_a_photo_and_both_go_with_them() {
        let c = db();
        let p = post_add(&c, 2, "post", "hello", &[], &json!({}), 1000).unwrap();
        let a = comment_add(&c, p, 1, None, "", Some(("k_l.jpg", "k_s.jpg", 300)), 10).unwrap().unwrap();
        let r = comment_add(&c, p, 2, Some(a), "Nice pull", Some(("r_l.jpg", "r_s.jpg", 200)), 11).unwrap().unwrap();
        assert_eq!(photo_bytes(&c).unwrap(), 500, "comment photos count toward the space");
        assert_eq!(comment_react(&c, a, 2, "love", 12).unwrap(), Some(1));
        assert_eq!(comment_react(&c, a, 1, "fire", 13).unwrap(), Some(2));
        assert_eq!(comment_react(&c, a, 2, "fire", 14).unwrap(), Some(2), "changing a reaction doesn't count twice");
        assert_eq!(comment_tops(&c, a).unwrap(), vec!["fire".to_string()]);
        let seen = comments(&c, p, 2, 50).unwrap();
        assert_eq!((seen[0].like_n, seen[0].mine.as_str(), seen[0].image.as_str(), seen[0].thumb.as_str()), (2, "fire", "k_l.jpg", "k_s.jpg"));
        assert_eq!(comments(&c, p, 0, 50).unwrap()[0].mine, "", "a visitor has no reaction of their own");
        assert_eq!(comment_for(&c, r, 1).unwrap().unwrap().mine, "");
        assert_eq!(comment_react(&c, a, 2, "", 15).unwrap(), Some(1), "taken back");
        assert_eq!(comment_react(&c, 999, 2, "like", 15).unwrap(), None);
        // Removing the comment takes its reply, their likes and both photos.
        assert_eq!(comment_remove(&c, a).unwrap(), Some((p, vec!["k_l.jpg".to_string(), "k_s.jpg".to_string(), "r_l.jpg".to_string(), "r_s.jpg".to_string()])));
        assert_eq!(c.query_row("SELECT COUNT(*) FROM comment_reactions", [], |x| x.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(photo_bytes(&c).unwrap(), 0);
        // Removing a post takes its comments' photos and likes too.
        let k = comment_add(&c, p, 1, None, "again", Some(("z_l.jpg", "z_s.jpg", 50)), 20).unwrap().unwrap();
        comment_react(&c, k, 2, "like", 21).unwrap();
        assert_eq!(post_remove(&c, p).unwrap(), vec!["z_l.jpg".to_string(), "z_s.jpg".to_string()]);
        assert_eq!(c.query_row("SELECT COUNT(*) FROM comment_reactions", [], |x| x.get::<_, i64>(0)).unwrap(), 0);
    }

    #[test]
    fn who_reacted_is_listed_and_reports_count_once() {
        let c = db();
        let p = post_add(&c, 2, "post", "hello", &[], &json!({}), 1000).unwrap();
        react(&c, p, 1, "fire", 10).unwrap();
        react(&c, p, 2, "love", 11).unwrap();
        let who: Vec<(String, String)> = reactions_of(&c, p, 10).unwrap().into_iter().map(|(_, n, _, k)| (n, k)).collect();
        assert_eq!(who, vec![("Buyer".to_string(), "love".to_string()), ("Seller".into(), "fire".into())], "newest first");
        assert!(report(&c, "post", p, 1, "spam", 20).unwrap());
        assert!(!report(&c, "post", p, 1, "again", 21).unwrap(), "once per person");
        assert!(report(&c, "post", p, 2, "", 22).unwrap());
        assert!(report(&c, "comment", p, 1, "", 23).unwrap(), "a comment is its own thing");
    }

    #[test]
    fn stores_that_put_up_cards_are_named_once_a_day_and_never_for_old_days() {
        let c = db();
        let now = 1_794_400_000;
        assert!(sync(&c, now).unwrap().is_empty(), "the cards already up are old news");
        c.execute("INSERT INTO member_cards(id, member_id, kind, updated_at) VALUES (12, 1, 'sale', ?1)", [now - 60]).unwrap();
        c.execute("INSERT INTO products(id, title, price, created_at) VALUES (4, 'Eevee', 80, strftime('%Y-%m-%dT%H:%M:%S', ?1, 'unixepoch'))", [now - 120]).unwrap();
        let mut fresh = sync(&c, now).unwrap();
        fresh.sort_unstable();
        assert_eq!(fresh, vec![0, 1], "the vault (0) and the seller, each once");
        c.execute("INSERT INTO member_cards(id, member_id, kind, updated_at) VALUES (13, 1, 'sale', ?1)", [now - 30]).unwrap();
        assert!(sync(&c, now).unwrap().is_empty(), "more cards the same day: the same post, nothing new to say");
    }
}
