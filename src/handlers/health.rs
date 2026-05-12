use crate::{db::cache, error::AppError, state::AppState};
use axum::{extract::State, Json};
use serde_json::{json, Value};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// GET /v1/health
pub async fn health() -> Json<Value> {
    Json(json!({ "healthy": true }))
}

/// GET /v1/version
pub async fn version() -> Json<Value> {
    Json(json!({
        "version": VERSION,
        "sha256":  "unknown",
    }))
}

/// GET /v1/stats
pub async fn stats(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let messages = {
        let conn = state.db.get()?;
        cache::count(&conn)?
    };
    let topics = state.topics.topic_count();
    let subscribers = state.topics.subscriber_count();

    Ok(Json(json!({
        "messages":    messages,
        "topics":      topics,
        "subscribers": subscribers,
    })))
}
