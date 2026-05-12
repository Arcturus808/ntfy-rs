//! Matrix Push Gateway / UnifiedPush relay.
//!
//! ntfy-rs acts as a Matrix Push Gateway (spec: https://spec.matrix.org/v1.2/push-gateway-api/)
//! combined with UnifiedPush as the Provider Push Protocol.
//!
//! Flow:
//!   1. A Matrix homeserver POSTs to `/_matrix/push/v1/notify` with a JSON body
//!      containing a `pushkey` — a full ntfy topic URL like
//!      `https://ntfy.example.com/upXXXXXXXX?up=1`.
//!   2. This handler extracts the pushkey, validates it starts with our base URL,
//!      then re-publishes the raw Matrix JSON body to that ntfy topic.
//!   3. The ntfy Android/iOS app (acting as the UnifiedPush distributor) receives
//!      the message on that topic and forwards it to the registered app.
//!
//! Discovery:
//!   GET `/_matrix/push/v1/notify` → `{"unifiedpush":{"gateway":"matrix"}}`
//!   This lets clients auto-discover that this server is a Matrix gateway.

use crate::{
    auth::{AuthUser, Permission},
    db::cache,
    error::AppError,
    message::{valid_topic, Message},
    state::AppState,
    upstream,
};
use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

// ── wire types ────────────────────────────────────────────────────────────────

/// Minimal subset of the Matrix push gateway notification body.
/// We only need the first device's pushkey; everything else is forwarded verbatim.
#[derive(Deserialize)]
struct MatrixRequest {
    notification: Option<MatrixNotification>,
}

#[derive(Deserialize)]
struct MatrixNotification {
    devices: Option<Vec<MatrixDevice>>,
}

#[derive(Deserialize)]
struct MatrixDevice {
    pushkey: String,
}

/// Response body as required by the Matrix push gateway spec.
#[derive(Serialize)]
pub struct MatrixResponse {
    rejected: Vec<String>,
}

// ── handlers ──────────────────────────────────────────────────────────────────

/// GET /_matrix/push/v1/notify — UnifiedPush gateway discovery.
pub async fn discovery() -> impl IntoResponse {
    Json(json!({ "unifiedpush": { "gateway": "matrix" } }))
}

/// POST /_matrix/push/v1/notify — receive a Matrix push notification and
/// forward it to the ntfy topic encoded in the pushkey.
pub async fn notify(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    body: Bytes,
) -> Result<(StatusCode, Json<MatrixResponse>), AppError> {
    if body.len() > state.config.message_size_limit {
        return Ok(matrix_ok(vec![]));
    }

    // Parse just enough to extract the pushkey.
    let req: MatrixRequest = serde_json::from_slice(&body)
        .map_err(|_| AppError::BadRequest("invalid Matrix JSON".into()))?;

    let pushkey = req
        .notification
        .as_ref()
        .and_then(|n| n.devices.as_ref())
        .and_then(|d| d.first())
        .map(|d| d.pushkey.clone())
        .unwrap_or_default();

    if pushkey.is_empty() {
        return Err(AppError::BadRequest("missing pushkey".into()));
    }

    // Validate pushkey starts with our base URL.
    let base = state.config.base_url.trim_end_matches('/');
    if base.is_empty() {
        return Err(AppError::Internal(
            "base_url not configured; cannot handle Matrix gateway requests".into(),
        ));
    }
    let prefix = format!("{base}/");
    if !pushkey.starts_with(&prefix) {
        // Pushkey belongs to a different server — reject it so the homeserver
        // stops sending to this endpoint.
        return Ok(matrix_ok(vec![pushkey]));
    }

    // Extract the topic from the pushkey.
    // e.g. "https://ntfy.example.com/upABCDEF?up=1" → topic = "upABCDEF"
    let path_and_query = &pushkey[prefix.len()..];
    let topic = path_and_query
        .split('?')
        .next()
        .unwrap_or(path_and_query)
        .trim_end_matches('/')
        .to_string();

    if !valid_topic(&topic) {
        return Ok(matrix_ok(vec![pushkey]));
    }

    // Rate limiting.
    let visitor = state.visitors.get_or_create(auth_user.ip);
    if !visitor.request_allowed() {
        return Err(AppError::TooManyRequests);
    }

    // Authorization — Matrix gateway publishes as anonymous; respects default_access.
    crate::auth::authorize(
        state.effective_auth_db(),
        &state.config,
        &auth_user,
        &topic,
        Permission::Write,
    )?;

    // Build and persist the message. The raw Matrix JSON is the message body —
    // UnifiedPush clients decode it themselves.
    let body_str = String::from_utf8_lossy(&body).into_owned();
    let mut msg = Message::new_message(&topic, body_str);

    let now = chrono::Utc::now().timestamp();
    msg.expires = Some(now + state.config.cache_duration_secs as i64);

    {
        let conn = state.db.get()?;
        cache::insert(&conn, &msg)?;
    }

    state.topics.publish(&topic, Arc::new(msg));

    // iOS upstream poll-forward.
    if state.config.upstream_base_url.is_some() {
        let state2 = state.clone();
        let topic2 = topic.clone();
        tokio::spawn(async move {
            upstream::forward_poll(&state2.config, &topic2, &state2.http).await;
        });
    }

    tracing::debug!(topic = %topic, pushkey = %pushkey, "matrix gateway forwarded");

    Ok(matrix_ok(vec![]))
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn matrix_ok(rejected: Vec<String>) -> (StatusCode, Json<MatrixResponse>) {
    (StatusCode::OK, Json(MatrixResponse { rejected }))
}
