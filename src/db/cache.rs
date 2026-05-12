use crate::message::Message;
use rusqlite::{params, Connection, OptionalExtension, Result};

/// Persist a message to the cache with immediate delivery (published=1).
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

/// Persist a delayed message (published=0). `msg.time` holds the delivery timestamp.
pub fn insert_delayed(conn: &Connection, msg: &Message) -> Result<()> {
    let tags = serde_json::to_string(&msg.tags).unwrap_or_else(|_| "[]".into());
    let actions = serde_json::to_string(&msg.actions).unwrap_or_else(|_| "[]".into());
    let expires = msg.expires.unwrap_or(0);
    let sequence_id = msg.sequence_id.as_deref().unwrap_or(&msg.id);

    conn.execute(
        "INSERT INTO messages
            (id, sequence_id, time, expires, topic, message, title, priority,
             tags, click, icon, actions, content_type, encoding, published)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,0)",
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

/// Return all delayed messages whose delivery time has arrived (`time <= now`).
pub fn due_messages(conn: &Connection, now: i64) -> Result<Vec<Message>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, sequence_id, time, expires, topic, message, title, priority,
                tags, click, icon, actions, content_type, encoding
         FROM messages
         WHERE published = 0 AND time <= ?1
         ORDER BY time ASC",
    )?;
    let rows = stmt.query_map(params![now], row_to_message)?;
    rows.collect()
}

/// Mark a delayed message as published so it appears in subscriber queries.
pub fn mark_published(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE messages SET published = 1 WHERE id = ?1",
        params![id],
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

/// Retrieve messages for a topic published after the message with `anchor_id` (exclusive).
///
/// If the anchor message is not found (expired or never existed), returns an
/// empty list — the client is up to date. This prevents re-delivering messages
/// the client has already seen when it reconnects with a known message ID.
pub fn since_id(conn: &Connection, topic: &str, anchor_id: &str) -> Result<Vec<Message>> {
    let anchor_time: Option<i64> = conn
        .query_row(
            "SELECT time FROM messages WHERE id = ?1 AND topic = ?2",
            params![anchor_id, topic],
            |row| row.get(0),
        )
        .optional()?;

    let since = match anchor_time {
        Some(t) => t,
        None => return Ok(vec![]), // anchor expired or unknown — nothing new to deliver
    };

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
