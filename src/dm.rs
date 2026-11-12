//! Private messages, Messenger style: one conversation per pair of people, so a buyer who writes to the same seller
//! again lands in the same chat. "The vault" is one side of a conversation too (VAULT, id 0): every full admin
//! answers for it. A message is text, a card (a copy of the card as it was, plus the buyer's words), a photo, a voice
//! note or a call's log line.
//!
//! Admins can read every conversation (crate admin pages); reading as an admin never marks anything seen, so the people
//! in it can't tell. The live part (who's online, new-message wake-ups, call signals) lives in the web app; this file is
//! only the database.
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

/// The vault's side of a conversation: answered by every full admin.
pub const VAULT: i64 = 0;
pub const TEXT_MAX: usize = 1000;
/// Messages one person may send in a minute and in an hour.
pub const PER_MINUTE: i64 = 20;
pub const PER_HOUR: i64 = 400;
/// Anyone may write to anyone. So that this is never a way to pester the whole site, a person who isn't staff can
/// start this many new conversations in a day; answering, and conversations already going, never count.
pub const NEW_CHATS_PER_DAY: i64 = 30;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS dm_threads(
  id INTEGER PRIMARY KEY,
  a INTEGER NOT NULL,
  b INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  last_at INTEGER NOT NULL DEFAULT 0,
  last_text TEXT NOT NULL DEFAULT '',
  last_from INTEGER NOT NULL DEFAULT -1,
  a_read INTEGER NOT NULL DEFAULT 0,
  b_read INTEGER NOT NULL DEFAULT 0,
  UNIQUE(a, b)
);
CREATE TABLE IF NOT EXISTS dm_messages(
  id INTEGER PRIMARY KEY,
  thread_id INTEGER NOT NULL,
  side INTEGER NOT NULL,
  author INTEGER NOT NULL DEFAULT 0,
  kind TEXT NOT NULL DEFAULT 'text',
  text TEXT NOT NULL DEFAULT '',
  card TEXT NOT NULL DEFAULT '',
  media TEXT NOT NULL DEFAULT '',
  media_thumb TEXT NOT NULL DEFAULT '',
  secs INTEGER NOT NULL DEFAULT 0,
  bytes INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS dm_messages_thread ON dm_messages(thread_id, id);
CREATE INDEX IF NOT EXISTS dm_messages_author ON dm_messages(author, created_at);
CREATE INDEX IF NOT EXISTS dm_threads_b ON dm_threads(b, last_at);
";

pub fn migrate(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(SCHEMA)
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Thread {
    pub id: i64,
    pub a: i64,
    pub b: i64,
    pub last_at: i64,
    pub last_text: String,
    pub last_from: i64,
    pub a_read: i64,
    pub b_read: i64,
}

impl Thread {
    pub fn has(&self, side: i64) -> bool {
        self.a == side || self.b == side
    }
    pub fn other(&self, side: i64) -> i64 {
        if self.a == side {
            self.b
        } else {
            self.a
        }
    }
    /// The last message this side has seen.
    pub fn read_by(&self, side: i64) -> i64 {
        if self.a == side {
            self.a_read
        } else {
            self.b_read
        }
    }
    pub fn is_vault(&self) -> bool {
        self.a == VAULT
    }
}

const THREAD_COLS: &str = "id, a, b, last_at, last_text, last_from, a_read, b_read";

fn map_thread(r: &Row) -> rusqlite::Result<Thread> {
    Ok(Thread { id: r.get(0)?, a: r.get(1)?, b: r.get(2)?, last_at: r.get(3)?, last_text: r.get(4)?, last_from: r.get(5)?, a_read: r.get(6)?, b_read: r.get(7)? })
}

pub fn thread(c: &Connection, id: i64) -> rusqlite::Result<Option<Thread>> {
    c.query_row(&format!("SELECT {THREAD_COLS} FROM dm_threads WHERE id=?1"), [id], map_thread).optional()
}

/// The conversation between two sides, made the first time.
pub fn thread_between(c: &Connection, x: i64, y: i64, now: i64) -> rusqlite::Result<Thread> {
    let (a, b) = if x <= y { (x, y) } else { (y, x) };
    c.execute("INSERT OR IGNORE INTO dm_threads(a, b, created_at) VALUES(?1, ?2, ?3)", params![a, b, now])?;
    c.query_row(&format!("SELECT {THREAD_COLS} FROM dm_threads WHERE a=?1 AND b=?2"), [a, b], map_thread)
}

pub fn find_between(c: &Connection, x: i64, y: i64) -> rusqlite::Result<Option<Thread>> {
    let (a, b) = if x <= y { (x, y) } else { (y, x) };
    c.query_row(&format!("SELECT {THREAD_COLS} FROM dm_threads WHERE a=?1 AND b=?2"), [a, b], map_thread).optional()
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Message {
    pub id: i64,
    pub thread_id: i64,
    /// Whose side wrote it: a member's id, or VAULT.
    pub side: i64,
    /// The member who actually typed it (an admin, for the vault's side); 0 for the owner's password.
    pub author: i64,
    pub kind: String,
    pub text: String,
    /// A copy of the card, as JSON, when kind is "card".
    pub card: String,
    pub media: String,
    pub media_thumb: String,
    pub secs: i64,
    pub created_at: i64,
}

const MSG_COLS: &str = "id, thread_id, side, author, kind, text, card, media, media_thumb, secs, created_at";

fn map_msg(r: &Row) -> rusqlite::Result<Message> {
    Ok(Message {
        id: r.get(0)?,
        thread_id: r.get(1)?,
        side: r.get(2)?,
        author: r.get(3)?,
        kind: r.get(4)?,
        text: r.get(5)?,
        card: r.get(6)?,
        media: r.get(7)?,
        media_thumb: r.get(8)?,
        secs: r.get(9)?,
        created_at: r.get(10)?,
    })
}

/// What one new message holds.
#[derive(Clone, Debug, Default)]
pub struct New {
    pub kind: String,
    pub text: String,
    pub card: String,
    pub media: String,
    pub media_thumb: String,
    pub secs: i64,
    pub bytes: i64,
}

/// The line shown in the inbox for a message.
pub fn preview(m: &New) -> String {
    let words = m.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let short = |s: &str| if s.chars().count() > 80 { format!("{}…", s.chars().take(79).collect::<String>()) } else { s.to_string() };
    match m.kind.as_str() {
        "photo" => "📷 Photo".into(),
        "voice" => "🎤 Voice message".into(),
        "call" => short(&words),
        "card" => {
            let title = serde_json::from_str::<serde_json::Value>(&m.card).ok().and_then(|v| v["title"].as_str().map(str::to_string)).unwrap_or_default();
            if words.is_empty() { format!("🃏 {}", short(&title)) } else { short(&words) }
        }
        _ => short(&words),
    }
}

pub fn post(c: &Connection, t: &Thread, side: i64, author: i64, m: &New, now: i64) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO dm_messages(thread_id, side, author, kind, text, card, media, media_thumb, secs, bytes, created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![t.id, side, author, m.kind, m.text, m.card, m.media, m.media_thumb, m.secs, m.bytes, now],
    )?;
    let id = c.last_insert_rowid();
    // The writer has seen their own message.
    let col = if t.a == side { "a_read" } else { "b_read" };
    c.execute(&format!("UPDATE dm_threads SET last_at=?2, last_text=?3, last_from=?4, {col}=?5 WHERE id=?1"), params![t.id, now, preview(m), side, id])?;
    Ok(id)
}

pub fn messages(c: &Connection, thread: i64, after: Option<i64>, before: Option<i64>, limit: i64) -> rusqlite::Result<Vec<Message>> {
    let mut out: Vec<Message> = if let Some(a) = after {
        c.prepare(&format!("SELECT {MSG_COLS} FROM dm_messages WHERE thread_id=?1 AND id>?2 ORDER BY id LIMIT ?3"))?
            .query_map(params![thread, a, limit], map_msg)?
            .collect::<Result<_, _>>()?
    } else {
        let mut v: Vec<Message> = c
            .prepare(&format!("SELECT {MSG_COLS} FROM dm_messages WHERE thread_id=?1 AND id<?2 ORDER BY id DESC LIMIT ?3"))?
            .query_map(params![thread, before.unwrap_or(i64::MAX), limit], map_msg)?
            .collect::<Result<_, _>>()?;
        v.reverse();
        v
    };
    out.retain(|m| m.thread_id == thread);
    Ok(out)
}

/// Marks the conversation read up to a message, for one side. Never goes backwards.
pub fn mark_read(c: &Connection, t: &Thread, side: i64, upto: i64) -> rusqlite::Result<bool> {
    let col = if t.a == side { "a_read" } else if t.b == side { "b_read" } else { return Ok(false) };
    let n = c.execute(&format!("UPDATE dm_threads SET {col}=?2 WHERE id=?1 AND {col}<?2 AND ?2<=(SELECT COALESCE(MAX(id),0) FROM dm_messages WHERE thread_id=?1)"), params![t.id, upto])?;
    Ok(n > 0)
}

/// Messages sent by one person since a time, for the rate limit.
/// True while nobody has written in this conversation yet.
pub fn is_fresh(c: &Connection, thread: i64) -> rusqlite::Result<bool> {
    c.query_row("SELECT NOT EXISTS(SELECT 1 FROM dm_messages WHERE thread_id=?1)", [thread], |r| r.get(0))
}

/// How many conversations this member started (theirs are the first words) since `since`.
pub fn started_since(c: &Connection, member: i64, since: i64) -> rusqlite::Result<i64> {
    c.query_row(
        "SELECT COUNT(*) FROM dm_threads t
         WHERE (SELECT m.author FROM dm_messages m WHERE m.thread_id=t.id ORDER BY m.id LIMIT 1)=?1
           AND (SELECT m.created_at FROM dm_messages m WHERE m.thread_id=t.id ORDER BY m.id LIMIT 1)>=?2",
        params![member, since],
        |r| r.get(0),
    )
}

pub fn sent_since(c: &Connection, author: i64, since: i64) -> rusqlite::Result<i64> {
    c.query_row("SELECT COUNT(*) FROM dm_messages WHERE author=?1 AND created_at>=?2 AND kind<>'call'", params![author, since], |r| r.get(0))
}

/// Room photos and voice notes take on the disk.
pub fn media_bytes(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT COALESCE(SUM(bytes),0) FROM dm_messages", [], |r| r.get(0))
}

/// One line of an inbox.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Row1 {
    pub id: i64,
    /// The side the reader is on in this conversation (their own id, or VAULT).
    pub me: i64,
    pub other: i64,
    pub last_at: i64,
    pub last_text: String,
    pub last_from: i64,
    pub unread: i64,
}

/// A member's conversations, newest first; a full admin also gets the vault's. Empty conversations are left out.
pub fn inbox(c: &Connection, member: i64, vault_too: bool, limit: i64) -> rusqlite::Result<Vec<Row1>> {
    let mut st = c.prepare(
        "SELECT t.id, t.a, t.b, t.last_at, t.last_text, t.last_from, t.a_read, t.b_read,
                (SELECT COUNT(*) FROM dm_messages m WHERE m.thread_id=t.id AND m.side<>?1 AND m.side<>CASE WHEN t.a=0 AND ?2 THEN 0 ELSE -9 END
                    AND m.id > CASE WHEN t.a=?1 THEN t.a_read WHEN t.b=?1 THEN t.b_read ELSE t.a_read END)
         FROM dm_threads t
         WHERE t.last_at>0 AND (t.a=?1 OR t.b=?1 OR (?2 AND t.a=0))
         ORDER BY t.last_at DESC LIMIT ?3",
    )?;
    let rows = st
        .query_map(params![member, vault_too, limit], |r| {
            let (id, a, b): (i64, i64, i64) = (r.get(0)?, r.get(1)?, r.get(2)?);
            // A full admin who is also in the conversation as themselves reads it as themselves; otherwise as the vault.
            let me = if a == member || b == member { member } else { VAULT };
            Ok(Row1 { id, me, other: if a == me { b } else { a }, last_at: r.get(3)?, last_text: r.get(4)?, last_from: r.get(5)?, unread: r.get(8)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Every conversation with any message, for the admins' overview.
pub fn all(c: &Connection, limit: i64) -> rusqlite::Result<Vec<Thread>> {
    c.prepare(&format!("SELECT {THREAD_COLS} FROM dm_threads WHERE last_at>0 ORDER BY last_at DESC LIMIT ?1"))?.query_map([limit], map_thread)?.collect()
}

/// Full admins, who answer for the vault and watch every conversation.
pub fn admins(c: &Connection) -> rusqlite::Result<Vec<i64>> {
    c.prepare("SELECT id FROM members WHERE role='admin' AND blocked=0 ORDER BY id")?.query_map([], |r| r.get(0))?.collect()
}

/// Tells a member about a message, keeping one unread line per conversation: the line is brought up to date
/// (latest words, how many) instead of a new line for each message.
pub fn notify(c: &Connection, member: i64, kind: &str, who: &str, words: &str, link: &str, now: i64) -> rusqlite::Result<()> {
    let open: Option<(i64, String)> = c
        .query_row("SELECT id, message FROM notifications WHERE member_id=?1 AND kind=?2 AND link=?3 AND is_read=0 ORDER BY id DESC LIMIT 1", params![member, kind, link], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    match open {
        Some((id, old)) => {
            // "Juan: “hi”" becomes "Juan · 2 new messages: “hello?”", and so on.
            let n = old.split(" · ").nth(1).and_then(|s| s.split(' ').next()).and_then(|n| n.parse::<i64>().ok()).unwrap_or(1) + 1;
            let msg = format!("{who} · {n} new messages: {words}");
            c.execute("UPDATE notifications SET message=?2, created_at=?3 WHERE id=?1", params![id, msg, now])?;
            Ok(())
        }
        None => crate::sellers::notify(c, member, kind, &format!("{who}: {words}"), link),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE members(id INTEGER PRIMARY KEY, name TEXT, role TEXT NOT NULL DEFAULT '', blocked INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE notifications(id INTEGER PRIMARY KEY AUTOINCREMENT, member_id INTEGER NOT NULL, kind TEXT NOT NULL, message TEXT NOT NULL, link TEXT NOT NULL, is_read INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
             CREATE TABLE notify_prefs(member_id INTEGER NOT NULL, event TEXT NOT NULL, site INTEGER NOT NULL DEFAULT 1, email INTEGER NOT NULL DEFAULT 0, push INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(member_id, event));
             INSERT INTO members(id, name, role) VALUES (1, 'Admin', 'admin'), (2, 'Seller', ''), (3, 'Buyer', '');",
        )
        .unwrap();
        migrate(&c).unwrap();
        c
    }

    fn text(s: &str) -> New {
        New { kind: "text".into(), text: s.into(), ..Default::default() }
    }

    #[test]
    fn new_conversations_are_counted_by_who_spoke_first() {
        let c = db();
        let ab = thread_between(&c, 3, 2, 100).unwrap();
        assert!(is_fresh(&c, ab.id).unwrap(), "opened, nothing said yet");
        assert_eq!(started_since(&c, 3, 0).unwrap(), 0, "opening a chat without writing starts nothing");
        post(&c, &ab, 3, 3, &text("hi, is this still for sale?"), 110).unwrap();
        assert!(!is_fresh(&c, ab.id).unwrap());
        post(&c, &ab, 2, 2, &text("yes!"), 120).unwrap();
        assert_eq!(started_since(&c, 3, 0).unwrap(), 1, "the buyer spoke first");
        assert_eq!(started_since(&c, 2, 0).unwrap(), 0, "answering is not starting");
        let ac = thread_between(&c, 3, 1, 200).unwrap();
        post(&c, &ac, 3, 3, &text("hello admin"), 210).unwrap();
        assert_eq!(started_since(&c, 3, 0).unwrap(), 2);
        assert_eq!(started_since(&c, 3, 150).unwrap(), 1, "only the ones started since then");
    }

    #[test]
    fn one_conversation_per_pair_and_unread_counts() {
        let c = db();
        let t = thread_between(&c, 3, 2, 10).unwrap();
        assert_eq!((t.a, t.b), (2, 3));
        assert_eq!(thread_between(&c, 2, 3, 11).unwrap().id, t.id, "the same pair gets the same chat");
        assert!(inbox(&c, 2, false, 50).unwrap().is_empty(), "an empty chat isn't listed");
        post(&c, &t, 3, 3, &text("Is the Pikachu still there?"), 20).unwrap();
        post(&c, &t, 3, 3, &text("hello?"), 21).unwrap();
        let seller = inbox(&c, 2, false, 50).unwrap();
        assert_eq!((seller[0].other, seller[0].unread, seller[0].last_text.as_str()), (3, 2, "hello?"));
        assert_eq!(inbox(&c, 3, false, 50).unwrap()[0].unread, 0, "your own messages aren't unread");
        let t = thread(&c, t.id).unwrap().unwrap();
        let last = messages(&c, t.id, None, None, 50).unwrap().last().unwrap().id;
        assert!(mark_read(&c, &t, 2, last).unwrap());
        assert_eq!(inbox(&c, 2, false, 50).unwrap()[0].unread, 0);
        assert!(!mark_read(&c, &t, 2, last + 50).unwrap(), "can't mark past the last message");
    }

    #[test]
    fn the_vault_is_answered_by_admins() {
        let c = db();
        let t = thread_between(&c, 3, VAULT, 10).unwrap();
        assert!(t.is_vault());
        post(&c, &t, 3, 3, &New { kind: "card".into(), card: r#"{"title":"Charizard ex"}"#.into(), ..Default::default() }, 20).unwrap();
        let admin = inbox(&c, 1, true, 50).unwrap();
        assert_eq!((admin[0].me, admin[0].other, admin[0].unread, admin[0].last_text.as_str()), (VAULT, 3, 1, "🃏 Charizard ex"));
        assert!(inbox(&c, 2, false, 50).unwrap().is_empty(), "other members don't see it");
        let t = thread(&c, t.id).unwrap().unwrap();
        post(&c, &t, VAULT, 1, &text("Yes, still here"), 30).unwrap();
        assert_eq!(inbox(&c, 1, true, 50).unwrap()[0].unread, 0, "the vault's reply means it was seen");
        assert_eq!(inbox(&c, 3, false, 50).unwrap()[0].unread, 1);
        assert_eq!(admins(&c).unwrap(), vec![1]);
    }

    #[test]
    fn one_notification_line_per_conversation() {
        let c = db();
        notify(&c, 2, "message", "Buyer", "“hi”", "/me/messages?t=1", 1).unwrap();
        notify(&c, 2, "message", "Buyer", "“still there?”", "/me/messages?t=1", 2).unwrap();
        notify(&c, 2, "message", "Buyer", "“hello”", "/me/messages?t=1", 3).unwrap();
        let rows: Vec<String> = c.prepare("SELECT message FROM notifications WHERE member_id=2").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(rows, vec!["Buyer · 3 new messages: “hello”".to_string()]);
        c.execute("UPDATE notifications SET is_read=1", []).unwrap();
        notify(&c, 2, "message", "Buyer", "“new”", "/me/messages?t=1", 4).unwrap();
        assert_eq!(c.query_row("SELECT COUNT(*) FROM notifications", [], |r| r.get::<_, i64>(0)).unwrap(), 2, "a read line starts a new one");
    }
}
