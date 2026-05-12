use crate::{
    auth::{authorize, AuthUser, Permission},
    db::cache,
    error::AppError,
    message::{parse_topics, valid_topic, Message},
    state::AppState,
};
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use std::sync::Arc;

/// PUT/POST /{topic}
pub async fn publish(
    State(state): State<AppState>,
    Path(topic): Path<String>,
    Extension(auth_user): Extension<AuthUser>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    if !valid_topic(&topic) {
        return Err(AppError::TopicInvalid);
    }
    if body.len() > state.config.message_size_limit {
        return Err(AppError::MessageTooLarge);
    }

    // Rate limiting
    let visitor = state.visitors.get_or_create(auth_user.ip);
    if !visitor.request_allowed() {
        return Err(AppError::TooManyRequests);
    }

    // Authorization
    authorize(
        state.effective_auth_db(),
        &state.config,
        &auth_user,
        &topic,
        Permission::Write,
    )?;

    let body_str = String::from_utf8_lossy(&body).into_owned();
    let mut msg = Message::new_message(&topic, body_str);

    // ── parse metadata headers ────────────────────────────────────────────
    if let Some(v) = header_val(&headers, &["x-title", "title", "t"]) {
        msg.title = v;
    }
    if let Some(v) = header_val(&headers, &["x-priority", "priority", "prio", "p"]) {
        msg.priority = parse_priority(&v);
    }
    if let Some(v) = header_val(&headers, &["x-tags", "tags", "tag", "ta"]) {
        msg.tags = v.split(',').map(|s| s.trim().to_string()).collect();
    }
    if let Some(v) = header_val(&headers, &["x-click", "click"]) {
        msg.click = v;
    }
    if let Some(v) = header_val(&headers, &["x-icon", "icon"]) {
        msg.icon = v;
    }
    if let Some(v) = header_val(&headers, &["x-markdown", "markdown", "md"]) {
        if is_truthy(&v) {
            msg.content_type = "text/markdown".to_string();
        }
    }
    if let Some(v) = header_val(&headers, &["content-type"]) {
        if v.to_lowercase().contains("text/markdown") {
            msg.content_type = "text/markdown".to_string();
        }
    }

    let expires = chrono::Utc::now().timestamp() + state.config.cache_duration_secs as i64;
    msg.expires = Some(expires);

    // Persist
    {
        let conn = state.db.get()?;
        cache::insert(&conn, &msg)?;
    }

    // Fan out
    state.topics.publish(&topic, Arc::new(msg.clone()));

    tracing::debug!(topic = %topic, id = %msg.id, "published");

    Ok((StatusCode::OK, Json(msg)))
}

/// POST /{topic1},{topic2},... — publish to multiple topics at once.
#[allow(dead_code)]
pub async fn publish_multi(
    State(state): State<AppState>,
    Path(topics_raw): Path<String>,
    Extension(auth_user): Extension<AuthUser>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let topics = parse_topics(&topics_raw).ok_or(AppError::TopicInvalid)?;
    for topic in &topics {
        publish(
            State(state.clone()),
            Path(topic.clone()),
            Extension(auth_user.clone()),
            headers.clone(),
            body.clone(),
        )
        .await?;
    }
    Ok((StatusCode::OK, Json(serde_json::json!({ "topics": topics }))))
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn header_val(headers: &HeaderMap, names: &[&str]) -> Option<String> {
    for name in names {
        if let Some(v) = headers.get(*name) {
            if let Ok(s) = v.to_str() {
                let s = s.trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn parse_priority(s: &str) -> i32 {
    match s.to_lowercase().as_str() {
        "1" | "min" => 1,
        "2" | "low" => 2,
        "3" | "default" => 3,
        "4" | "high" => 4,
        "5" | "urgent" | "max" => 5,
        _ => 3,
    }
}

fn is_truthy(s: &str) -> bool {
    matches!(s.to_lowercase().as_str(), "1" | "true" | "yes")
}
