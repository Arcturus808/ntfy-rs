use crate::auth::{Permission, Role, User};
use rusqlite::{params, Connection, OptionalExtension, Result};

// ── users ─────────────────────────────────────────────────────────────────────

/// Look up a user by username. Returns None if not found or soft-deleted.
pub fn user_by_name(conn: &Connection, username: &str) -> Result<Option<User>> {
    conn.query_row(
        "SELECT id, username, hash, role FROM users
         WHERE username = ?1 AND deleted = 0",
        params![username],
        row_to_user,
    )
    .optional()
}

/// Look up a user by the token string. Returns None if token not found,
/// expired, or the owning user is deleted.
pub fn user_by_token(conn: &Connection, token: &str) -> Result<Option<User>> {
    let now = chrono::Utc::now().timestamp();
    conn.query_row(
        "SELECT u.id, u.username, u.hash, u.role
         FROM users u
         JOIN tokens t ON t.user_id = u.id
         WHERE t.token = ?1
           AND u.deleted = 0
           AND (t.expires IS NULL OR t.expires > ?2)",
        params![token, now],
        row_to_user,
    )
    .optional()
}

/// Insert a new user. `hash` must already be a bcrypt hash.
pub fn insert_user(conn: &Connection, id: &str, username: &str, hash: &str, role: Role) -> Result<()> {
    conn.execute(
        "INSERT INTO users (id, username, hash, role) VALUES (?1, ?2, ?3, ?4)",
        params![id, username, hash, role.as_str()],
    )?;
    Ok(())
}

/// Update the bcrypt hash for a user (password change).
pub fn update_password(conn: &Connection, user_id: &str, hash: &str) -> Result<()> {
    conn.execute(
        "UPDATE users SET hash = ?1 WHERE id = ?2",
        params![hash, user_id],
    )?;
    Ok(())
}

/// Soft-delete a user (sets deleted = 1). Tokens are cascade-deleted by FK.
pub fn delete_user(conn: &Connection, user_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE users SET deleted = 1 WHERE id = ?1",
        params![user_id],
    )?;
    Ok(())
}

// ── tokens ────────────────────────────────────────────────────────────────────

/// Insert a new token for a user.
pub fn insert_token(
    conn: &Connection,
    token: &str,
    user_id: &str,
    label: &str,
    expires: Option<i64>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO tokens (token, user_id, label, expires) VALUES (?1, ?2, ?3, ?4)",
        params![token, user_id, label, expires],
    )?;
    Ok(())
}

/// Delete a specific token.
pub fn delete_token(conn: &Connection, token: &str, user_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM tokens WHERE token = ?1 AND user_id = ?2",
        params![token, user_id],
    )?;
    Ok(())
}

/// Update last_access and last_origin for a token (fire-and-forget on auth).
pub fn touch_token(conn: &Connection, token: &str, origin: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE tokens SET last_access = ?1, last_origin = ?2 WHERE token = ?3",
        params![now, origin, token],
    )?;
    Ok(())
}

// ── ACL ───────────────────────────────────────────────────────────────────────

/// Check whether `user_id` has the requested permission on `topic`.
/// Returns true if an explicit allow row exists.
pub fn acl_allowed(
    conn: &Connection,
    user_id: &str,
    topic: &str,
    perm: Permission,
) -> Result<bool> {
    let col = match perm {
        Permission::Read => "read",
        Permission::Write => "write",
    };
    let sql = format!(
        "SELECT 1 FROM topic_acl WHERE user_id = ?1 AND topic = ?2 AND {col} = 1"
    );
    let found: Option<i32> = conn
        .query_row(&sql, params![user_id, topic], |row| row.get(0))
        .optional()?;
    Ok(found.is_some())
}

/// Upsert an ACL row for (user_id, topic).
pub fn acl_set(
    conn: &Connection,
    user_id: &str,
    topic: &str,
    read: bool,
    write: bool,
) -> Result<()> {
    conn.execute(
        "INSERT INTO topic_acl (user_id, topic, read, write)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(user_id, topic) DO UPDATE SET read = ?3, write = ?4",
        params![user_id, topic, read as i32, write as i32],
    )?;
    Ok(())
}

/// Remove an ACL row.
pub fn acl_delete(conn: &Connection, user_id: &str, topic: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM topic_acl WHERE user_id = ?1 AND topic = ?2",
        params![user_id, topic],
    )?;
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn row_to_user(row: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    let role_str: String = row.get(3)?;
    Ok(User {
        id: row.get(0)?,
        username: row.get(1)?,
        hash: row.get(2)?,
        role: Role::from_str(&role_str),
    })
}
