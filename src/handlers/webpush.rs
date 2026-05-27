//! HTTP handlers for the Web Push API.
//!
//! Endpoints (all under `/v1/webpush/`):
//!
//! | Method | Path                          | Description                              |
//! |--------|-------------------------------|------------------------------------------|
//! | GET    | `/v1/webpush/vapid-key`       | Return the server's VAPID public key     |
//! | POST   | `/v1/webpush/subscriptions`   | Register a browser push subscription    |
//! | DELETE | `/v1/webpush/subscriptions/:id` | Unregister a subscription              |

use crate::{
    db::webpush::{add_subscription, delete_subscription, Subscription},
    error::AppError,
    message::valid_topic,
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

// ── Endpoint validation ─────────────────────────────────────────────────────────

/// Returns `true` if `endpoint` is safe to deliver push notifications to.
///
/// Prevents SSRF (cf. GHSA-w9hq-5jg7-q4j7 in ntfy Go v2.22.0): a malicious
/// subscriber could register an internal URL (e.g. `https://169.254.169.254/`)
/// and cause the server to exfiltrate requests to internal infrastructure on
/// every publish. We defend by:
///
/// 1. Requiring a well-formed `https://` URL.
/// 2. Rejecting raw IP addresses entirely — legitimate push service endpoints
///    (FCM, Mozilla, etc.) are always domain names. This covers all private,
///    loopback, link-local, and reserved ranges for both IPv4 and IPv6 without
///    relying on unstable `IpAddr` methods.
/// 3. Rejecting well-known local/reserved hostnames.
fn valid_push_endpoint(endpoint: &str) -> bool {
    let Ok(url) = Url::parse(endpoint) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.is_empty() {
        return false;
    }
    // Reject raw IP addresses (covers all private/loopback/link-local ranges).
    if host.parse::<IpAddr>().is_ok() {
        return false;
    }
    // Reject well-known local or reserved hostnames.
    let h = host.to_ascii_lowercase();
    if h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
        || h.ends_with(".test")
        || h.ends_with(".example")
    {
        return false;
    }
    true
}

// ── GET /v1/webpush/vapid-key ─────────────────────────────────────────────────

#[derive(Serialize)]
struct VapidKeyResponse {
    #[serde(rename = "publicKey")]
    public_key: String,
}

/// Return the server's VAPID public key (uncompressed P-256, base64url).
///
/// Browsers use this value as the `applicationServerKey` when calling
/// `pushManager.subscribe()`.
pub async fn get_vapid_key(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let vapid = state
        .vapid
        .as_ref()
        .ok_or_else(|| AppError::Internal("web push is not configured".into()))?;

    Ok(Json(VapidKeyResponse {
        public_key: vapid.public_key_b64.clone(),
    }))
}

// ── POST /v1/webpush/subscriptions ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SubscribeRequest {
    pub topic: String,
    pub endpoint: String,
    pub keys: SubscribeKeys,
}

#[derive(Deserialize)]
pub struct SubscribeKeys {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Serialize)]
struct SubscribeResponse {
    id: String,
}

/// Register a new web push subscription for a topic.
///
/// The caller supplies the `PushSubscription` values obtained from the browser's
/// `pushManager.subscribe()` call. Returns the opaque subscription ID, which the
/// client must retain in order to unsubscribe.
pub async fn subscribe(
    State(state): State<AppState>,
    Json(req): Json<SubscribeRequest>,
) -> Result<impl IntoResponse, AppError> {
    if !valid_topic(&req.topic) {
        return Err(AppError::TopicInvalid);
    }

    if !valid_push_endpoint(&req.endpoint) {
        return Err(AppError::BadRequest(
            "endpoint must be a valid https:// URL pointing to a public push service".into(),
        ));
    }

    if req.keys.p256dh.is_empty() || req.keys.auth.is_empty() {
        return Err(AppError::BadRequest("p256dh and auth keys are required".into()));
    }

    let sub = Subscription {
        id: Uuid::new_v4().to_string(),
        topic: req.topic,
        endpoint: req.endpoint,
        p256dh: req.keys.p256dh,
        auth: req.keys.auth,
        created: Utc::now().timestamp(),
    };

    {
        let conn = state.db.get()?;
        add_subscription(&conn, &sub).map_err(|e| AppError::Internal(e.to_string()))?;
    }

    Ok((StatusCode::CREATED, Json(SubscribeResponse { id: sub.id })))
}

// ── DELETE /v1/webpush/subscriptions/:id ─────────────────────────────────────

/// Unregister a web push subscription by its opaque ID.
///
/// Returns 204 whether or not the ID existed (idempotent).
pub async fn unsubscribe(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let conn = state.db.get()?;
    delete_subscription(&conn, &id).map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}
