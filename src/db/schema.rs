use rusqlite::{Connection, Result};

/// Current schema version. Bump when adding migrations.
const SCHEMA_VERSION: u32 = 2;

/// Apply all pending migrations in order.
pub fn migrate(conn: &Connection) -> Result<()> {
    // Enable WAL mode for better concurrent read performance.
    conn.execute_batch("PRAGMA journal_mode = WAL;")?;
    conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;

    let version = user_version(conn)?;

    if version < 1 {
        migrate_v1(conn)?;
    }
    if version < 2 {
        migrate_v2(conn)?;
    }

    Ok(())
}

fn user_version(conn: &Connection) -> Result<u32> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
}

fn set_user_version(conn: &Connection, v: u32) -> Result<()> {
    conn.execute_batch(&format!("PRAGMA user_version = {v}"))
}

fn migrate_v2(conn: &Connection) -> Result<()> {
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS users (
            id       TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            hash     TEXT NOT NULL,
            role     TEXT NOT NULL DEFAULT 'user',
            deleted  INTEGER NOT NULL DEFAULT 0
        );

        -- Tokens for Bearer auth. expires = NULL means no expiry.
        CREATE TABLE IF NOT EXISTS tokens (
            token       TEXT PRIMARY KEY,
            user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            label       TEXT NOT NULL DEFAULT '',
            expires     INTEGER,
            last_access INTEGER,
            last_origin TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_tokens_user
            ON tokens (user_id);

        -- Per-user, per-topic ACL. owner_id NULL = everyone (anonymous).
        CREATE TABLE IF NOT EXISTS topic_acl (
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            topic   TEXT NOT NULL,
            read    INTEGER NOT NULL DEFAULT 0,
            write   INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (user_id, topic)
        );
    ")?;

    set_user_version(conn, SCHEMA_VERSION)?;
    Ok(())
}

fn migrate_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS messages (
            id           TEXT NOT NULL,
            sequence_id  TEXT NOT NULL,
            time         INTEGER NOT NULL,
            expires      INTEGER NOT NULL,
            topic        TEXT NOT NULL,
            message      TEXT NOT NULL DEFAULT '',
            title        TEXT NOT NULL DEFAULT '',
            priority     INTEGER NOT NULL DEFAULT 0,
            tags         TEXT NOT NULL DEFAULT '[]',
            click        TEXT NOT NULL DEFAULT '',
            icon         TEXT NOT NULL DEFAULT '',
            actions      TEXT NOT NULL DEFAULT '[]',
            content_type TEXT NOT NULL DEFAULT '',
            encoding     TEXT NOT NULL DEFAULT '',
            published    INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (id, topic)
        );

        CREATE INDEX IF NOT EXISTS idx_messages_topic_time
            ON messages (topic, time);

        CREATE INDEX IF NOT EXISTS idx_messages_expires
            ON messages (expires);

        CREATE INDEX IF NOT EXISTS idx_messages_due
            ON messages (time)
            WHERE published = 0;
    ")?;

    set_user_version(conn, SCHEMA_VERSION)?;
    Ok(())
}
