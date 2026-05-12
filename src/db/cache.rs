use crate::message::Message;
use rusqlite::{params, Connection, Result};

/// Persist a message to the cache.
pub fn insert(conn: &Connection, msg: &Message) -> Result<()> {
    let tags = serde_json::to_string(&msg.tags).unwrap_or_else(|_| "[]".into());
    let actions = serde_json::to_string(&msg.actions).unwrap_or_else(|_| "[]".into());
    let expires = msg.expires.unwrap_or(0);
    let sequence_id = msg.sequence_id.as_deref().unwrap_or(&msg.id);

    conn.execute(
        "INSERT INTO messages
            (id, sequence_id, time, expires, topic, message, title, priority,
             tags, click, icon, actions, content_type, encoding, published)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,1)",
        params![
            msg.id,
            sequence_id,
            msg.time,
            expires,
            msg.topic,
            msg.message,
            msg.title,
            msg.priority,
            tags,
            msg.click,
            msg.icon,
            actions,
            msg.content_type,
            msg.encoding,
        ],
    )?;
    Ok(())
}

/// Retrieve messages for a topic published after `since_time` (Unix seconds).
/// Returns messages in ascending time order.
pub fn since_time(conn: &Connection, topic: &str, since: i64) -> Result<Vec<Message>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, sequence_id, time, expires, topic, message, title, priority,
                tags, click, icon, actions, content_type, encoding
         FROM messages
         WHERE topic = ?1 AND time >= ?2 AND published = 1
         ORDER BY time ASC",
    )?;
    let rows = stmt.query_map(params![topic, since], row_to_message)?;
    rows.collect()
}

#[allow(dead_code)]
/// Retrieve messages for a topic published after the message with `since_id`.
pub fn since_id(conn: &Connection, topic: &str, since_id: &str) -> Result<Vec<Message>> {
    // Find the time of the anchor message, then return everything after it.
    let anchor_time: Option<i64> = conn
        .query_row(
            "SELECT time FROM messages WHERE id = ?1 AND topic = ?2",
            params![since_id, topic],
            |row| row.get(0),
        )
        .ok();

    let since = anchor_time.unwrap_or(0);
    let mut stmt = conn.prepare_cached(
        "SELECT id, sequence_id, time, expires, topic, message, title, priority,
                tags, click, icon, actions, content_type, encoding
         FROM messages
         WHERE topic = ?1 AND time > ?2 AND published = 1
         ORDER BY time ASC",
    )?;
    let rows = stmt.query_map(params![topic, since], row_to_message)?;
    rows.collect()
}

#[allow(dead_code)]
/// Return the single most-recent published message for a topic, if any.
pub fn latest(conn: &Connection, topic: &str) -> Result<Option<Message>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, sequence_id, time, expires, topic, message, title, priority,
                tags, click, icon, actions, content_type, encoding
         FROM messages
         WHERE topic = ?1 AND published = 1
         ORDER BY time DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![topic], row_to_message)?;
    rows.next().transpose()
}

/// Delete messages whose `expires` timestamp is in the past.
/// Returns the number of rows deleted.
pub fn delete_expired(conn: &Connection, now: i64) -> Result<usize> {
    let n = conn.execute(
        "DELETE FROM messages WHERE expires > 0 AND expires < ?1",
        params![now],
    )?;
    Ok(n)
}

/// Total number of cached (published) messages across all topics.
pub fn count(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE published = 1",
        [],
        |row| row.get(0),
    )
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    let tags_json: String = row.get(8)?;
    let actions_json: String = row.get(11)?;
    let expires: i64 = row.get(3)?;
    let sequence_id: String = row.get(1)?;
    let id: String = row.get(0)?;

    let tags: Vec<String> =
        serde_json::from_str(&tags_json).unwrap_or_default();
    let actions: Vec<crate::message::Action> =
        serde_json::from_str(&actions_json).unwrap_or_default();

    Ok(Message {
        id: id.clone(),
        sequence_id: if sequence_id == id {
            None
        } else {
            Some(sequence_id)
        },
        time: row.get(2)?,
        expires: if expires == 0 { None } else { Some(expires) },
        event: crate::message::EVENT_MESSAGE.to_string(),
        topic: row.get(4)?,
        message: row.get(5)?,
        title: row.get(6)?,
        priority: row.get(7)?,
        tags,
        click: row.get(9)?,
        icon: row.get(10)?,
        actions,
        content_type: row.get(12)?,
        encoding: row.get(13)?,
    })
}
