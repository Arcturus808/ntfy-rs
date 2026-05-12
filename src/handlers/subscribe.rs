use crate::{
    auth::{authorize, AuthUser, Permission},
    db::cache,
    error::AppError,
    message::{parse_topics, valid_topic, Message},
    state::AppState,
};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Extension,
};
use futures_util::stream::{self, Stream, StreamExt};
use serde::Deserialize;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

/// Query parameters shared by all subscribe endpoints.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SubscribeParams {
    #[serde(default)]
    pub poll: Option<String>,
    pub since: Option<String>,
    pub priority: Option<String>,
    pub tags: Option<String>,
    pub message: Option<String>,
    pub title: Option<String>,
}

/// How far back to look for cached messages.
#[allow(dead_code)]
pub enum Since {
    /// Return messages with time >= this Unix timestamp.
    Time(i64),
    /// Return messages after the message with this ID (exclusive).
    Id(String),
    /// Return no cached messages (live stream only).
    None,
}

impl SubscribeParams {
    pub fn is_poll(&self) -> bool {
        self.poll
            .as_deref()
            .map(|v| matches!(v, "1" | "true" | "yes"))
            .unwrap_or(false)
    }

    /// Resolve the `since` parameter into a `Since` variant.
    ///
    /// Matching ntfy's parseSince() logic:
    /// - No `since` on a live stream → Since::None (no cached messages)
    /// - No `since` on a poll       → Since::Time(0) (all cached messages)
    /// - `since=all`                → Since::Time(0)
    /// - `since=none`               → Since::None
    /// - `since=<unix timestamp>`   → Since::Time(ts)
    /// - `since=<message id>`       → Since::Id(id)  ← fixes the re-appear bug
    pub fn since(&self) -> Since {
        match self.since.as_deref() {
            None => {
                if self.is_poll() {
                    Since::Time(0) // bare ?poll=1 → return all cached
                } else {
                    Since::None   // live stream → no cached messages
                }
            }
            Some("all")  => Since::Time(0),
            Some("none") => Since::None,
            Some(s) => {
                // Unix timestamp?
                if let Ok(ts) = s.parse::<i64>() {
                    return Since::Time(ts);
                }
                // Message ID (12-char alphanumeric)?
                if crate::message::valid_message_id(s) {
                    return Since::Id(s.to_string());
                }
                // Unrecognised — treat as "no cached messages"
                Since::None
            }
        }
    }
}

// ── SSE subscribe: GET /{topics}/sse ─────────────────────────────────────────

pub async fn subscribe_sse(
    State(state): State<AppState>,
    Path(topics_raw): Path<String>,
    Query(params): Query<SubscribeParams>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response, AppError> {
    if topics_raw.contains(',') {
        return subscribe_multi_sse(
            State(state), Path(topics_raw), Query(params), Extension(auth_user),
        ).await;
    }
    let topic = topics_raw;
    if !valid_topic(&topic) {
        return Err(AppError::TopicInvalid);
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
        Permission::Read,
    )?;

    if params.is_poll() {
        let msgs = resolve_since(&state, &topic, &params)?;
        let stream = stream::iter(msgs.into_iter().map(|m| {
            let data = serde_json::to_string(&m).unwrap_or_default();
            Ok::<Event, Infallible>(Event::default().data(data))
        }));
        return Ok(Sse::new(stream).into_response());
    }

    let cached = resolve_since(&state, &topic, &params)?;

    let t = state.topics.get_or_create(&topic);
    let rx = t.tx.subscribe();
    visitor.increment_subscriptions();

    // Decrement subscription count when the stream ends.
    // We wrap the stream in a guard that decrements on drop.
    let keepalive_secs = state.config.keepalive_secs;
    let visitor_clone = Arc::clone(&visitor);
    let stream = build_sse_stream(topic.clone(), cached, rx, visitor_clone);

    Ok(Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(keepalive_secs))
                .text("keepalive"),
        )
        .into_response())
}

fn build_sse_stream(
    topic: String,
    cached: Vec<Message>,
    rx: broadcast::Receiver<Arc<Message>>,
    visitor: Arc<crate::visitor::Visitor>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let open_msg = Message::new_open(&topic);
    let open_event = stream::once(async move {
        let data = serde_json::to_string(&open_msg).unwrap_or_default();
        Ok::<Event, Infallible>(Event::default().data(data))
    });

    let cached_stream = stream::iter(cached.into_iter().map(|m| {
        let data = serde_json::to_string(&m).unwrap_or_default();
        Ok::<Event, Infallible>(Event::default().data(data))
    }));

    let live_stream = BroadcastStream::new(rx).filter_map(|result| async move {
        match result {
            Ok(msg) => {
                let data = serde_json::to_string(&*msg).unwrap_or_default();
                Some(Ok::<Event, Infallible>(Event::default().data(data)))
            }
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                tracing::warn!(skipped = n, "subscriber lagged");
                None
            }
        }
    });

    // Decrement subscription count when the stream is dropped (client disconnects).
    let guard_stream = stream::once(async move {
        let _guard = SubscriptionGuard(visitor);
        // Yield nothing — just hold the guard until the stream ends.
        futures_util::future::pending::<Option<Result<Event, Infallible>>>().await
    })
    .filter_map(|x| async move { x });

    open_event
        .chain(cached_stream)
        .chain(live_stream)
        .chain(guard_stream)
}

use crate::visitor::SubscriptionGuard;

/// Resolve `params.since()` into a list of cached messages for a single topic.
pub fn resolve_since(
    state: &AppState,
    topic: &str,
    params: &SubscribeParams,
) -> Result<Vec<Message>, AppError> {
    let conn = state.db.get()?;
    match params.since() {
        Since::None      => Ok(vec![]),
        Since::Time(ts)  => Ok(cache::since_time(&conn, topic, ts)?),
        Since::Id(id)    => Ok(cache::since_id(&conn, topic, &id)?),
    }
}

// ── NDJSON: GET /{topics}/json (raw newline-delimited, no SSE framing) ────────
//
// ntfy clients use this as the primary streaming format. Each line is a
// complete JSON object followed by '\n'. No "data:" prefix.
// Handles both single topic and comma-separated multi-topic.

pub async fn subscribe_ndjson(
    State(state): State<AppState>,
    Path(topics_raw): Path<String>,
    Query(params): Query<SubscribeParams>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response, AppError> {
    if topics_raw.contains(',') {
        return subscribe_multi_ndjson(
            State(state), Path(topics_raw), Query(params), Extension(auth_user),
        ).await;
    }
    let topic = topics_raw;
    if !valid_topic(&topic) {
        return Err(AppError::TopicInvalid);
    }

    let visitor = state.visitors.get_or_create(auth_user.ip);
    if !visitor.request_allowed() {
        return Err(AppError::TooManyRequests);
    }

    authorize(
        state.effective_auth_db(),
        &state.config,
        &auth_user,
        &topic,
        Permission::Read,
    )?;

    let cached = resolve_since(&state, &topic, &params)?;

    if params.is_poll() {
        // Poll: return cached messages and close.
        let body = cached
            .iter()
            .filter_map(|m| {
                let mut line = serde_json::to_string(m).ok()?;
                line.push('\n');
                Some(line)
            })
            .collect::<String>();
        return Ok(Response::builder()
            .header("Content-Type", "application/x-ndjson")
            .body(Body::from(body))
            .unwrap());
    }

    // Streaming: open event + cached + live.
    let t = state.topics.get_or_create(&topic);
    let rx = t.tx.subscribe();
    visitor.increment_subscriptions();

    let visitor_clone = Arc::clone(&visitor);
    let stream = build_ndjson_stream(topic.clone(), cached, rx, visitor_clone);

    Ok(Response::builder()
        .header("Content-Type", "application/x-ndjson")
        .header("Transfer-Encoding", "chunked")
        .header("X-Accel-Buffering", "no") // disable nginx buffering
        .body(Body::from_stream(stream))
        .unwrap())
}

fn build_ndjson_stream(
    topic: String,
    cached: Vec<Message>,
    rx: broadcast::Receiver<Arc<Message>>,
    visitor: Arc<crate::visitor::Visitor>,
) -> impl Stream<Item = Result<String, std::convert::Infallible>> {
    let open_msg = Message::new_open(&topic);
    let open = stream::once(async move {
        let mut s = serde_json::to_string(&open_msg).unwrap_or_default();
        s.push('\n');
        Ok::<String, Infallible>(s)
    });

    let cached_stream = stream::iter(cached.into_iter().filter_map(|m| {
        let mut s = serde_json::to_string(&m).ok()?;
        s.push('\n');
        Some(Ok::<String, Infallible>(s))
    }));

    let live_stream = BroadcastStream::new(rx).filter_map(|result| async move {
        match result {
            Ok(msg) => {
                let mut s = serde_json::to_string(&*msg).ok()?;
                s.push('\n');
                Some(Ok::<String, Infallible>(s))
            }
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                tracing::warn!(skipped = n, "ndjson subscriber lagged");
                None
            }
        }
    });

    let guard_stream = stream::once(async move {
        let _guard = SubscriptionGuard(visitor);
        futures_util::future::pending::<Option<Result<String, Infallible>>>().await
    })
    .filter_map(|x| async move { x });

    open.chain(cached_stream).chain(live_stream).chain(guard_stream)
}

// ── Multi-topic SSE: GET /{topic1},{topic2}/sse ───────────────────────────────

pub async fn subscribe_multi_sse(
    State(state): State<AppState>,
    Path(topics_raw): Path<String>,
    Query(params): Query<SubscribeParams>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response, AppError> {
    let topics = parse_topics(&topics_raw).ok_or(AppError::TopicInvalid)?;

    let visitor = state.visitors.get_or_create(auth_user.ip);
    if !visitor.request_allowed() {
        return Err(AppError::TooManyRequests);
    }

    for topic in &topics {
        authorize(
            state.effective_auth_db(),
            &state.config,
            &auth_user,
            topic,
            Permission::Read,
        )?;
    }

    let mut cached: Vec<Message> = Vec::new();
    let mut receivers: Vec<broadcast::Receiver<Arc<Message>>> = Vec::new();

    for topic in &topics {
        let mut msgs = resolve_since(&state, topic, &params)?;
        cached.append(&mut msgs);
        let t = state.topics.get_or_create(topic);
        receivers.push(t.tx.subscribe());
    }

    cached.sort_by_key(|m| m.time);
    visitor.increment_subscriptions();

    let first_topic = topics[0].clone();
    let keepalive_secs = state.config.keepalive_secs;
    let visitor_clone = Arc::clone(&visitor);

    let stream = build_multi_sse_stream(first_topic, cached, receivers, visitor_clone);

    Ok(Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(keepalive_secs))
                .text("keepalive"),
        )
        .into_response())
}

fn build_multi_sse_stream(
    first_topic: String,
    cached: Vec<Message>,
    receivers: Vec<broadcast::Receiver<Arc<Message>>>,
    visitor: Arc<crate::visitor::Visitor>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let open_msg = Message::new_open(&first_topic);
    let open_event = stream::once(async move {
        let data = serde_json::to_string(&open_msg).unwrap_or_default();
        Ok::<Event, Infallible>(Event::default().data(data))
    });

    let cached_stream = stream::iter(cached.into_iter().map(|m| {
        let data = serde_json::to_string(&m).unwrap_or_default();
        Ok::<Event, Infallible>(Event::default().data(data))
    }));

    // Merge all broadcast receivers into one stream.
    let boxed: Vec<_> = receivers
        .into_iter()
        .map(|rx| {
            Box::pin(BroadcastStream::new(rx).filter_map(
                |result| async move {
                    match result {
                        Ok(msg) => {
                            let data = serde_json::to_string(&*msg).unwrap_or_default();
                            Some(Ok::<Event, Infallible>(Event::default().data(data)))
                        }
                        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                            tracing::warn!(skipped = n, "multi-sse subscriber lagged");
                            None
                        }
                    }
                },
            ))
        })
        .collect();
    let live_stream = futures_util::stream::select_all(boxed);

    let guard_stream = stream::once(async move {
        let _guard = SubscriptionGuard(visitor);
        futures_util::future::pending::<Option<Result<Event, Infallible>>>().await
    })
    .filter_map(|x| async move { x });

    open_event
        .chain(cached_stream)
        .chain(live_stream)
        .chain(guard_stream)
}

// ── Multi-topic NDJSON: GET /{topic1},{topic2}/json ───────────────────────────

pub async fn subscribe_multi_ndjson(
    State(state): State<AppState>,
    Path(topics_raw): Path<String>,
    Query(params): Query<SubscribeParams>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Response, AppError> {
    let topics = parse_topics(&topics_raw).ok_or(AppError::TopicInvalid)?;

    let visitor = state.visitors.get_or_create(auth_user.ip);
    if !visitor.request_allowed() {
        return Err(AppError::TooManyRequests);
    }

    for topic in &topics {
        authorize(
            state.effective_auth_db(),
            &state.config,
            &auth_user,
            topic,
            Permission::Read,
        )?;
    }

    let mut cached: Vec<Message> = Vec::new();
    let mut receivers: Vec<broadcast::Receiver<Arc<Message>>> = Vec::new();

    for topic in &topics {
        let mut msgs = resolve_since(&state, topic, &params)?;
        cached.append(&mut msgs);
        let t = state.topics.get_or_create(topic);
        receivers.push(t.tx.subscribe());
    }

    cached.sort_by_key(|m| m.time);

    if params.is_poll() {
        let body = cached
            .iter()
            .filter_map(|m| {
                let mut line = serde_json::to_string(m).ok()?;
                line.push('\n');
                Some(line)
            })
            .collect::<String>();
        return Ok(Response::builder()
            .header("Content-Type", "application/x-ndjson")
            .body(Body::from(body))
            .unwrap());
    }

    visitor.increment_subscriptions();
    let first_topic = topics[0].clone();
    let visitor_clone = Arc::clone(&visitor);

    let open_msg = Message::new_open(&first_topic);
    let open = stream::once(async move {
        let mut s = serde_json::to_string(&open_msg).unwrap_or_default();
        s.push('\n');
        Ok::<String, Infallible>(s)
    });

    let cached_stream = stream::iter(cached.into_iter().filter_map(|m| {
        let mut s = serde_json::to_string(&m).ok()?;
        s.push('\n');
        Some(Ok::<String, Infallible>(s))
    }));

    let boxed: Vec<_> = receivers
        .into_iter()
        .map(|rx| {
            Box::pin(BroadcastStream::new(rx).filter_map(
                |result| async move {
                    match result {
                        Ok(msg) => {
                            let mut s = serde_json::to_string(&*msg).ok()?;
                            s.push('\n');
                            Some(Ok::<String, Infallible>(s))
                        }
                        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                            tracing::warn!(skipped = n, "multi-ndjson subscriber lagged");
                            None
                        }
                    }
                },
            ))
        })
        .collect();
    let live_stream = futures_util::stream::select_all(boxed);

    let guard_stream = stream::once(async move {
        let _guard = SubscriptionGuard(visitor_clone);
        futures_util::future::pending::<Option<Result<String, Infallible>>>().await
    })
    .filter_map(|x| async move { x });

    let full_stream = open.chain(cached_stream).chain(live_stream).chain(guard_stream);

    Ok(Response::builder()
        .header("Content-Type", "application/x-ndjson")
        .header("Transfer-Encoding", "chunked")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(full_stream))
        .unwrap())
}
