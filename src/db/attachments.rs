use rusqlite::{params, Connection, OptionalExtension, Result};

/// A persisted attachment record mirroring the `attachments` table.
#[derive(Debug, Clone)]
pub struct AttachmentRecord {
    pub id: String,
    pub name: String,
    pub content_type: String,
    pub size: u64,
    pub expires: i64,
    /// Absolute path to the stored file on disk.
    pub path: String,
}

/// Insert a new attachment record linked to `message_id`.
pub fn insert(conn: &Connection, rec: &AttachmentRecord, message_id: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO attachments (id, message_id, name, content_type, size, expires, path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            rec.id,
            message_id,
            rec.name,
            rec.content_type,
            rec.size as i64,
            rec.expires,
            rec.path,
        ],
    )?;
    Ok(())
}

/// Look up an attachment by its public ID.
pub fn get(conn: &Connection, id: &str) -> Result<Option<AttachmentRecord>> {
    conn.query_row(
        "SELECT id, name, content_type, size, expires, path FROM attachments WHERE id = ?1",
        params![id],
        |row| {
            Ok(AttachmentRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                content_type: row.get(2)?,
                size: row.get::<_, i64>(3)? as u64,
                expires: row.get(4)?,
                path: row.get(5)?,
            })
        },
    )
    .optional()
}

/// Delete all attachments that have expired (expires < now).
/// Returns the file paths of deleted records so the caller can remove them from disk.
pub fn delete_expired(conn: &Connection, now: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT path FROM attachments WHERE expires < ?1",
    )?;
    let paths: Vec<String> = stmt
        .query_map(params![now], |row| row.get(0))?
        .collect::<Result<_>>()?;

    conn.execute("DELETE FROM attachments WHERE expires < ?1", params![now])?;
    Ok(paths)
}

/// Return the sum of sizes of all stored attachments (bytes).
pub fn total_size(conn: &Connection) -> Result<u64> {
    let sum: Option<i64> = conn.query_row(
        "SELECT SUM(size) FROM attachments",
        [],
        |row| row.get(0),
    )?;
    Ok(sum.unwrap_or(0) as u64)
}
