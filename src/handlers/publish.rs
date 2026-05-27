use crate::{
    auth::{authorize, AuthUser, Permission},
    db::{self, cache},
    email,
    error::AppError,
    message::{generate_id, parse_topics, valid_topic, Action, Attachment, Message},
    state::AppState,
    upstream,
};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use std::collections::HashMap;
use std::sync::Arc;

/// PUT/POST /{topic}
pub async fn publish(
    State(state): State<AppState>,
    Path(topic): Path<String>,
    Extension(auth_user): Extension<AuthUser>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    if !valid_topic(&topic) {
        return Err(AppError::TopicInvalid);
    }

    // ── Detect attachment early (X-Filename or non-text Content-Type) ─────
    let is_attachment = detect_attachment(&headers, &params);

    // ── Size check ────────────────────────────────────────────────────────
    if is_attachment {
        if state.config.attachment_cache_dir.is_none() {
            return Err(AppError::AttachmentsDisabled);
        }
        if body.len() as u64 > state.config.attachment_file_size_limit {
            return Err(AppError::MessageTooLarge);
        }
    } else if body.len() > state.config.message_size_limit {
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

    // For attachments the message body field is empty; the bytes go to disk.
    let body_str = if is_attachment {
        String::new()
    } else {
        String::from_utf8_lossy(&body).into_owned()
    };
    let mut msg = Message::new_message(&topic, body_str);

    // ── parse metadata headers + query params ────────────────────────────
    // Each field is read from headers first, falling back to query params —
    // matching ntfy's readParam() behaviour so clients can use either form.
    if let Some(v) = param(&headers, &params, &["x-title", "title", "t"]) {
        msg.title = v;
    }
    if let Some(v) = param(&headers, &params, &["x-priority", "priority", "prio", "p"]) {
        msg.priority = parse_priority(&v);
    }
    if let Some(v) = param(&headers, &params, &["x-tags", "tags", "tag", "ta"]) {
        msg.tags = v.split(',').map(|s| s.trim().to_string()).collect();
    }
    if let Some(v) = param(&headers, &params, &["x-click", "click"]) {
        msg.click = v;
    }
    if let Some(v) = param(&headers, &params, &["x-icon", "icon"]) {
        msg.icon = v;
    }
    if let Some(v) = param(&headers, &params, &["x-markdown", "markdown", "md"]) {
        if is_truthy(&v) {
            msg.content_type = "text/markdown".to_string();
        }
    }
    if let Some(v) = param(&headers, &params, &["content-type", "content_type"]) {
        if v.to_lowercase().contains("text/markdown") {
            msg.content_type = "text/markdown".to_string();
        }
    }
    if let Some(v) = param(&headers, &params, &["x-actions", "actions", "action"]) {
        msg.actions = parse_actions(&v);
    }
    if let Some(v) = param(&headers, &params, &["x-encoding", "encoding", "enc", "e"]) {
        if v.to_lowercase() == "base64" {
            msg.encoding = "base64".to_string();
        } else {
            return Err(AppError::BadRequest(format!("unsupported encoding: {v}")));
        }
    }

    let now = chrono::Utc::now().timestamp();

    // ── Attachment: write to disk and record metadata ─────────────────────
    if is_attachment {
        let cache_dir = state.config.attachment_cache_dir.as_ref().unwrap(); // safe: checked above

        // Reject if accepting this file would exceed the total storage cap.
        {
            let conn = state.db.get()?;
            let total = db::attachments::total_size(&conn)
                .map_err(|e| AppError::Internal(e.to_string()))?;
            if total + body.len() as u64 > state.config.attachment_total_size_limit {
                return Err(AppError::BadRequest("attachment storage limit reached".into()));
            }
        }

        let att_id = generate_id();
        let file_name = param(&headers, &params, &["x-filename", "filename"])
            .unwrap_or_else(|| format!("attachment-{att_id}"));
        let att_content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let att_expires = now + state.config.attachment_expiry_secs as i64;
        let att_path = cache_dir.join(&att_id);

        tokio::fs::create_dir_all(cache_dir)
            .await
            .map_err(|e| AppError::Internal(format!("cannot create attachment dir: {e}")))?;
        tokio::fs::write(&att_path, &body)
            .await
            .map_err(|e| AppError::Internal(format!("failed to write attachment: {e}")))?;

        let base = state.config.base_url.trim_end_matches('/');
        let att_url = format!("{base}/file/{att_id}");

        msg.attachment = Some(Attachment {
            name: file_name.clone(),
            content_type: att_content_type.clone(),
            size: body.len() as u64,
            expires: att_expires,
            url: att_url,
        });

        {
            let conn = state.db.get()?;
            db::attachments::insert(
                &conn,
                &db::attachments::AttachmentRecord {
                    id: att_id,
                    name: file_name,
                    content_type: att_content_type,
                    size: body.len() as u64,
                    expires: att_expires,
                    path: att_path.to_string_lossy().into_owned(),
                },
                &msg.id,
            )
            .map_err(|e| AppError::Internal(e.to_string()))?;
        }
    }

    // ── parse delay (X-Delay / X-At / X-In, header or query param) ───────
    let deliver_at: Option<i64> =
        if let Some(v) = param(&headers, &params, &["x-delay", "delay", "x-at", "at", "x-in", "in"]) {
            parse_delay(&v, state.config.max_delay_secs)
                .map_err(|_| AppError::BadRequest("invalid delay value".into()))?
        } else {
            None
        };

    let expires = now + state.config.cache_duration_secs as i64;
    msg.expires = Some(expires);

    let is_delayed = deliver_at.is_some();

    // Persist — published=1 for immediate, published=0 for delayed.
    {
        let conn = state.db.get()?;
        if let Some(fire_at) = deliver_at {
            // Store with the scheduled delivery time as `time` and published=0.
            // The manager will flip published=1 and fan-out when the time comes.
            let mut delayed = msg.clone();
            delayed.time = fire_at;
            cache::insert_delayed(&conn, &delayed)?;
        } else {
            cache::insert(&conn, &msg)?;
        }
    }

    // Fan out immediately only for non-delayed messages.
    if !is_delayed {
        state.topics.publish(&topic, Arc::new(msg.clone()));

        // iOS upstream poll-forward — fire-and-forget.
        if state.config.upstream_base_url.is_some() {
            let state2 = state.clone();
            let topic2 = topic.clone();
            let msg_id2 = msg.id.clone();
            tokio::spawn(async move {
                upstream::forward_poll(&state2.config, &topic2, &msg_id2, &state2.http).await;
            });
        }

        // Outbound email notification — fire-and-forget.
        if let Some(smtp) = state.config.smtp.clone() {
            let msg2 = msg.clone();
            tokio::spawn(async move {
                email::send_notification(&smtp, &msg2).await;
            });
        }
    }

    tracing::debug!(topic = %topic, id = %msg.id, delayed = is_delayed, "published");
    metrics::counter!("ntfy_messages_published_total").increment(1);

    Ok((StatusCode::OK, Json(msg)))
}

/// POST /{topic1},{topic2},... — publish to multiple topics at once.
#[allow(dead_code)]
pub async fn publish_multi(
    State(state): State<AppState>,
    Path(topics_raw): Path<String>,
    Extension(auth_user): Extension<AuthUser>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let topics = parse_topics(&topics_raw).ok_or(AppError::TopicInvalid)?;
    for topic in &topics {
        publish(
            State(state.clone()),
            Path(topic.clone()),
            Extension(auth_user.clone()),
            Query(params.clone()),
            headers.clone(),
            body.clone(),
        )
        .await?;
    }
    Ok((StatusCode::OK, Json(serde_json::json!({ "topics": topics }))))
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Returns `true` when the request should be treated as a file attachment rather
/// than a text message. This matches the `X-Filename` header (or `filename` query
/// param) being set, OR the Content-Type being non-text.
fn detect_attachment(headers: &HeaderMap, params: &HashMap<String, String>) -> bool {
    // Explicit filename → definitely an attachment.
    for name in &["x-filename", "filename"] {
        if headers.contains_key(*name) {
            return true;
        }
        if params.contains_key(*name) {
            return true;
        }
    }
    // Non-text Content-Type → treat as binary attachment.
    if let Some(ct) = headers.get("content-type").and_then(|v| v.to_str().ok()) {
        let ct = ct.to_lowercase();
        if !ct.is_empty() && !ct.starts_with("text/plain") && !ct.starts_with("text/markdown") {
            return true;
        }
    }
    false
}

/// Read a parameter from headers first, then query string — matching ntfy's readParam().
fn param(headers: &HeaderMap, query: &HashMap<String, String>, names: &[&str]) -> Option<String> {
    // Headers take priority.
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
    // Fall back to query string (lowercase keys).
    for name in names {
        let key = name.to_lowercase();
        if let Some(v) = query.get(&key) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return Some(v);
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

/// Parse a delay value into a future Unix timestamp.
///
/// Accepted formats (matching ntfy wire protocol):
/// - Unix timestamp:  `"1712345678"`
/// - RFC 3339:        `"2024-04-05T12:00:00Z"`
/// - Duration string: `"30m"`, `"2h"`, `"1d"`, `"90s"`
///
/// Returns `Err` if the value is unparseable or exceeds `max_delay_secs`.
/// Returns `Ok(None)` if the value resolves to "now or past" (treat as immediate).
fn parse_delay(s: &str, max_delay_secs: u64) -> Result<Option<i64>, ()> {
    let now = chrono::Utc::now().timestamp();

    // Try plain Unix timestamp first.
    if let Ok(ts) = s.parse::<i64>() {
        return validate_delay(ts, now, max_delay_secs);
    }

    // Try RFC 3339 / ISO 8601.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return validate_delay(dt.timestamp(), now, max_delay_secs);
    }

    // Try duration string: optional integer + unit suffix.
    if let Some(secs) = parse_duration_str(s) {
        return validate_delay(now + secs, now, max_delay_secs);
    }

    Err(())
}

/// Validate a resolved timestamp: reject past/now (return None = immediate)
/// and reject values beyond max_delay_secs (return Err).
fn validate_delay(ts: i64, now: i64, max_delay_secs: u64) -> Result<Option<i64>, ()> {
    if ts <= now {
        return Ok(None); // already in the past — deliver immediately
    }
    if (ts - now) as u64 > max_delay_secs {
        return Err(()); // too far in the future
    }
    Ok(Some(ts))
}

/// Parse simple duration strings: `"30s"`, `"5m"`, `"2h"`, `"1d"`.
/// Returns the number of seconds, or None if unrecognised.
fn parse_duration_str(s: &str) -> Option<i64> {
    let s = s.trim();
    let (num_part, unit) = if let Some(stripped) = s.strip_suffix('s') {
        (stripped, 1i64)
    } else if let Some(stripped) = s.strip_suffix('m') {
        (stripped, 60)
    } else if let Some(stripped) = s.strip_suffix('h') {
        (stripped, 3600)
    } else if let Some(stripped) = s.strip_suffix('d') {
        (stripped, 86400)
    } else {
        return None;
    };
    num_part.trim().parse::<i64>().ok().map(|n| n * unit)
}

/// Parse the `X-Actions` header value into a list of [`Action`]s.
///
/// Format: semicolon-separated list of actions, each being a comma-separated
/// list of fields:
/// ```text
/// <type>, <label>[, <url>][, key=value, ...]
/// ```
///
/// Supported types:
/// - `view`      — open a URL: `view, Open, https://example.com[, clear=true]`
/// - `http`      — fire HTTP request: `http, Restart, https://example.com/restart[, method=POST][, headers.Authorization=Bearer t][, body={}][, clear=true]`
/// - `broadcast` — Android broadcast: `broadcast, Take photo[, intent=io.example.ACTION][, extras.cmd=snap][, clear=true]`
///
/// Unknown types and malformed entries are silently skipped, matching ntfy (Go) behaviour.
fn parse_actions(s: &str) -> Vec<Action> {
    s.split(';')
        .filter_map(|part| parse_single_action(part.trim()))
        .collect()
}

fn parse_single_action(s: &str) -> Option<Action> {
    let fields: Vec<&str> = s.split(',').map(str::trim).collect();
    if fields.len() < 2 {
        return None;
    }
    let action_type = fields[0].to_lowercase();
    let label = fields[1].to_string();

    // Field 2 is a positional URL if it does not contain '=' (i.e. is not a key=value pair).
    let (positional, kv_start) = if fields.len() > 2 && !fields[2].contains('=') {
        (Some(fields[2].to_string()), 3)
    } else {
        (None, 2)
    };

    // Parse remaining fields as key=value pairs; split on the first '=' only so
    // values such as `headers.Authorization=Bearer token` are handled correctly.
    // Keys are stored with their original case so header names (e.g. `Authorization`)
    // round-trip correctly; lookups for built-in params use case-insensitive comparison.
    let mut kv: HashMap<String, String> = HashMap::new();
    for f in &fields[kv_start..] {
        if let Some((k, v)) = f.split_once('=') {
            kv.insert(k.trim().to_string(), v.trim().to_string());
        }
    }

    let kv_get = |name: &str| -> Option<String> {
        kv.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    };

    let clear = kv_get("clear").map(|v| v == "true").unwrap_or(false);

    let action = match action_type.as_str() {
        "view" => Action {
            id: generate_id(),
            action: "view".into(),
            label,
            url: positional,
            method: None,
            headers: None,
            body: None,
            intent: None,
            extras: None,
            clear,
        },
        "http" => {
            let headers: HashMap<String, String> = kv
                .iter()
                .filter(|(k, _)| k.to_lowercase().starts_with("headers."))
                .map(|(k, v)| (k["headers.".len()..].to_string(), v.clone()))
                .collect();
            Action {
                id: generate_id(),
                action: "http".into(),
                label,
                url: positional,
                method: kv_get("method"),
                headers: if headers.is_empty() { None } else { Some(headers) },
                body: kv_get("body"),
                intent: None,
                extras: None,
                clear,
            }
        }
        "broadcast" => {
            let extras: HashMap<String, String> = kv
                .iter()
                .filter(|(k, _)| k.to_lowercase().starts_with("extras."))
                .map(|(k, v)| (k["extras.".len()..].to_string(), v.clone()))
                .collect();
            Action {
                id: generate_id(),
                action: "broadcast".into(),
                label,
                url: None,
                method: None,
                headers: None,
                body: None,
                intent: kv_get("intent"),
                extras: if extras.is_empty() { None } else { Some(extras) },
                clear,
            }
        }
        _ => return None,
    };

    Some(action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_actions_view() {
        let actions = parse_actions("view, Open website, https://example.com");
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, "view");
        assert_eq!(actions[0].label, "Open website");
        assert_eq!(actions[0].url.as_deref(), Some("https://example.com"));
        assert!(!actions[0].clear);
    }

    #[test]
    fn test_parse_actions_view_clear() {
        let actions = parse_actions("view, Open, https://example.com, clear=true");
        assert!(actions[0].clear);
    }

    #[test]
    fn test_parse_actions_http() {
        let actions = parse_actions(
            "http, Restart, https://example.com/restart, method=POST, body={}, clear=true",
        );
        assert_eq!(actions.len(), 1);
        let a = &actions[0];
        assert_eq!(a.action, "http");
        assert_eq!(a.label, "Restart");
        assert_eq!(a.url.as_deref(), Some("https://example.com/restart"));
        assert_eq!(a.method.as_deref(), Some("POST"));
        assert_eq!(a.body.as_deref(), Some("{}"));
        assert!(a.clear);
    }

    #[test]
    fn test_parse_actions_http_headers() {
        let actions = parse_actions(
            "http, Ping, https://example.com, headers.Authorization=Bearer tok",
        );
        let headers = actions[0].headers.as_ref().unwrap();
        assert_eq!(headers.get("Authorization").map(String::as_str), Some("Bearer tok"));
    }

    #[test]
    fn test_parse_actions_broadcast() {
        let actions = parse_actions(
            "broadcast, Take photo, intent=io.example.ACTION, extras.cmd=snap",
        );
        assert_eq!(actions[0].action, "broadcast");
        assert_eq!(actions[0].intent.as_deref(), Some("io.example.ACTION"));
        let extras = actions[0].extras.as_ref().unwrap();
        assert_eq!(extras.get("cmd").map(String::as_str), Some("snap"));
    }

    #[test]
    fn test_parse_actions_multiple() {
        let actions = parse_actions(
            "view, Docs, https://example.com; http, Restart, https://example.com/restart",
        );
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].action, "view");
        assert_eq!(actions[1].action, "http");
    }

    #[test]
    fn test_parse_actions_unknown_type_skipped() {
        let actions = parse_actions("unknown, label, url; view, Docs, https://example.com");
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, "view");
    }

    #[test]
    fn test_parse_actions_too_few_fields_skipped() {
        let actions = parse_actions("view");
        assert!(actions.is_empty());
    }
}
