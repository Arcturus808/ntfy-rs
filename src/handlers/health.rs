use crate::{auth::AuthUser, db::cache, error::AppError, state::AppState};
use axum::{extract::State, response::IntoResponse, Extension, Json};
use serde_json::{json, Value};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// GET /v1/health
pub async fn health() -> Json<Value> {
    Json(json!({ "healthy": true }))
}

/// GET /{topic}/auth — ntfy client auth check.
///
/// The ntfy iOS and Android apps hit this before subscribing to verify
/// credentials. Return 200 when auth is disabled or the caller is authenticated,
/// 401 when auth is enabled and no/invalid credentials were supplied.
pub async fn topic_auth(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> impl IntoResponse {
    use crate::auth::{authorize, Permission};
    use axum::http::StatusCode;

    // Re-use the topic path parameter isn't needed here — the app only checks
    // whether the server accepts the credentials at all, not per-topic access.
    // A 200 means "credentials OK (or auth disabled)", 401 means "try again with creds".
    if !state.config.auth_enabled {
        return StatusCode::OK;
    }
    // Auth is enabled — check if the caller authenticated successfully.
    // We do a dummy authorize against a placeholder topic; if they're logged in
    // at all (or default_access allows it) we return 200.
    match authorize(
        state.effective_auth_db(),
        &state.config,
        &auth_user,
        "auth",
        Permission::Read,
    ) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::UNAUTHORIZED,
    }
}

/// GET /v1/version
pub async fn version() -> Json<Value> {
    Json(json!({
        "version": VERSION,
        "sha256":  "unknown",
    }))
}

/// GET /v1/config — server capability response expected by ntfy iOS/Android apps.
///
/// The app uses this to confirm it is talking to a ntfy server and to discover
/// which features are enabled. We return a minimal response with the fields the
/// app actually checks; everything optional is false/empty.
pub async fn config(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "base_url":             state.config.base_url,
        "app_root":             "/",
        "enable_login":         state.config.auth_enabled,
        "require_login":        false,
        "enable_signup":        state.config.auth_enabled,
        "enable_payments":      false,
        "enable_calls":         false,
        "enable_emails":        false,
        "enable_reservations":  false,
        "enable_web_push":      false,
        "disallowed_topics":    [],
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
