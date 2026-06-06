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
/// credentials. Must return `{"success":true}` with 200 when auth is disabled
/// or the caller is authenticated, or 401 when credentials are required but
/// missing/invalid. The app checks the JSON body, not just the status code.
#[cfg(feature = "auth")]
pub async fn topic_auth(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> impl IntoResponse {
    use crate::auth::{authorize, Permission};
    use axum::http::StatusCode;

    if !state.config.auth_enabled {
        return (StatusCode::OK, Json(json!({ "success": true }))).into_response();
    }
    match authorize(
        state.effective_auth_db(),
        &state.config,
        &auth_user,
        "auth",
        Permission::Read,
    ) {
        Ok(_) => (StatusCode::OK, Json(json!({ "success": true }))).into_response(),
        Err(_) => (StatusCode::UNAUTHORIZED, Json(json!({ "success": false }))).into_response(),
    }
}

/// No-op topic_auth when auth feature is disabled — always succeeds.
#[cfg(not(feature = "auth"))]
pub async fn topic_auth() -> impl IntoResponse {
    use axum::http::StatusCode;
    (StatusCode::OK, Json(json!({ "success": true })))
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
    #[cfg(feature = "auth")]
    let enable_login = state.config.auth_enabled;
    #[cfg(not(feature = "auth"))]
    let enable_login = false;

    #[cfg(feature = "email")]
    let enable_emails = state.config.smtp.is_some();
    #[cfg(not(feature = "email"))]
    let enable_emails = false;

    #[cfg(feature = "webpush")]
    let enable_web_push = state.vapid.is_some();
    #[cfg(not(feature = "webpush"))]
    let enable_web_push = false;

    Json(json!({
        "base_url":             state.config.base_url,
        "upstream_base_url":    state.config.upstream_base_url,
        "app_root":             "/",
        "enable_login":         enable_login,
        "require_login":        false,
        "enable_signup":        enable_login,
        "enable_payments":      false,
        "enable_calls":         false,
        "enable_emails":        enable_emails,
        "enable_reservations":  false,
        "enable_web_push":      enable_web_push,
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
