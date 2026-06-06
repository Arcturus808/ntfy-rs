use crate::{config::Config, db::DbPool, topic::TopicMap, visitor::VisitorMap};
#[cfg(feature = "webpush")]
use crate::webpush::VapidState;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: DbPool,
    #[cfg(feature = "auth")]
    pub auth_db: Option<DbPool>,
    pub topics: Arc<TopicMap>,
    pub visitors: Arc<VisitorMap>,
    /// Shared HTTP client for upstream poll-forward and outbound push requests.
    pub http: reqwest::Client,
    /// VAPID state for web push notifications. None when web push is not initialised.
    #[cfg(feature = "webpush")]
    pub vapid: Option<Arc<VapidState>>,
}

impl AppState {
    #[cfg(all(feature = "webpush", feature = "auth"))]
    pub fn new(config: Config, db: DbPool, auth_db: Option<DbPool>, vapid: Option<Arc<VapidState>>) -> Self {
        let config = Arc::new(config);
        let visitors = Arc::new(VisitorMap::new(Arc::clone(&config)));
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        AppState {
            config: Arc::clone(&config),
            db,
            auth_db,
            topics: Arc::new(TopicMap::new()),
            visitors,
            http,
            vapid,
        }
    }

    #[cfg(all(feature = "webpush", not(feature = "auth")))]
    pub fn new(config: Config, db: DbPool, _auth_db: Option<DbPool>, vapid: Option<Arc<VapidState>>) -> Self {
        let config = Arc::new(config);
        let visitors = Arc::new(VisitorMap::new(Arc::clone(&config)));
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        AppState {
            config: Arc::clone(&config),
            db,
            topics: Arc::new(TopicMap::new()),
            visitors,
            http,
            vapid,
        }
    }

    #[cfg(all(not(feature = "webpush"), feature = "auth"))]
    pub fn new(config: Config, db: DbPool, auth_db: Option<DbPool>, _vapid: Option<Arc<()>>) -> Self {
        let config = Arc::new(config);
        let visitors = Arc::new(VisitorMap::new(Arc::clone(&config)));
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        AppState {
            config: Arc::clone(&config),
            db,
            auth_db,
            topics: Arc::new(TopicMap::new()),
            visitors,
            http,
        }
    }

    #[cfg(all(not(feature = "webpush"), not(feature = "auth")))]
    pub fn new(config: Config, db: DbPool, _auth_db: Option<DbPool>, _vapid: Option<Arc<()>>) -> Self {
        let config = Arc::new(config);
        let visitors = Arc::new(VisitorMap::new(Arc::clone(&config)));
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        AppState {
            config: Arc::clone(&config),
            db,
            topics: Arc::new(TopicMap::new()),
            visitors,
            http,
        }
    }

    #[cfg(feature = "auth")]
    pub fn effective_auth_db(&self) -> &DbPool {
        self.auth_db.as_ref().unwrap_or(&self.db)
    }

    #[cfg(not(feature = "auth"))]
    pub fn effective_auth_db(&self) -> &DbPool {
        &self.db
    }
}
