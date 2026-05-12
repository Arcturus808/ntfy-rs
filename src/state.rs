use crate::{config::Config, db::DbPool, topic::TopicMap, visitor::VisitorMap};
use std::sync::Arc;

/// Shared application state injected into every handler via axum's `State`
/// extractor. Cheap to clone — all fields are `Arc` or `Clone`.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    /// Message cache database (always open).
    pub db: DbPool,
    /// Auth database — separate pool when auth_file is set, None otherwise.
    pub auth_db: Option<DbPool>,
    pub topics: Arc<TopicMap>,
    pub visitors: Arc<VisitorMap>,
}

impl AppState {
    pub fn new(config: Config, db: DbPool, auth_db: Option<DbPool>) -> Self {
        let config = Arc::new(config);
        let visitors = Arc::new(VisitorMap::new(Arc::clone(&config)));
        AppState {
            config: Arc::clone(&config),
            db,
            auth_db,
            topics: Arc::new(TopicMap::new()),
            visitors,
        }
    }

    /// Return the auth DB pool, falling back to the message DB pool.
    pub fn effective_auth_db(&self) -> &DbPool {
        self.auth_db.as_ref().unwrap_or(&self.db)
    }
}
