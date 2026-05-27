pub mod attachments;
pub mod cache;
pub mod schema;
pub mod users;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::PathBuf;

pub type DbPool = Pool<SqliteConnectionManager>;

/// Open (or create) the SQLite database and run schema migrations.
pub fn open(path: Option<&PathBuf>) -> anyhow::Result<DbPool> {
    let manager = match path {
        Some(p) => SqliteConnectionManager::file(p),
        None => SqliteConnectionManager::memory(),
    };

    let pool = Pool::builder()
        // SQLite allows only one writer at a time; keep the pool small.
        // Reads can share connections, but rusqlite connections are not Send
        // across threads without the pool, so we use a modest pool size.
        .max_size(4)
        .build(manager)?;

    // Run schema migrations on a single connection before handing the pool out.
    {
        let conn = pool.get()?;
        schema::migrate(&conn)?;
    }

    Ok(pool)
}
