use crate::{
    db::cache,
    error::AppError,
    message::{parse_topics, valid_topic, Message},
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
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
    /// Return cached messages and close (no streaming).
    #[serde(default)]
    pub poll: Option<String>,

    /// Return messages since this Unix timestamp or message ID.
    pub since: Option<String>,

    /// Client-side filters
    pub priority: Option<String>,
    pub tags: Option<String>,
    pub message: Option<String>,
    pub title: Option<String>,
}

impl SubscribeParams {
    fn is_poll(&self) -> bool {
        self.poll
            .as_deref()
            .map(|v| matches!(v, "1" | "true" | "yes"))
            .unwrap_or(false)
    }

    /// Resolve `since` to a Unix timestamp. Defaults to "now - 10s" when
    /// polling without an explicit since, so the client gets recent messages.
    fn since_time(&self) -> i64 {
        match self.since.as_deref() {
            Some("all") => 0,
            Some(s) => s.parse::<i64>().unwrap_or_else(|_| {
                // Treat as message ID — not yet supported in Phase 1, fall back to 0.
                0
            }),
            None => {
                if self.is_poll() {
                    // Default poll window: last 10 seconds
                    chrono::Utc::now().timestamp() - 10
                } else {
                    // Streaming: only new messages
                    chrono::Utc::now().timestamp()
                }
            }
        }
    }
}

// ── SSE subscribe: GET /{topic}/json ─────────────────────────────────────────

pub async fn subscribe_sse(
    State(state): State<AppState>,
    Path(topic): Path<String>,
    Query(params): Query<SubscribeParams>,
) -> Result<impl IntoResponse, AppError> {
    if !valid_topic(&topic) {
        return Err(AppError::TopicInvalid);
    }

    // Poll mode: return cached messages as a JSON stream and close.
    if params.is_poll() {
        let since = params.since_time();
        let msgs = {
            let conn = state.db.get()?;
            cache::since_time(&conn, &topic, since)?
        };
        let stream = stream::iter(msgs.into_iter().map(|m| {
            let data = serde_json::to_string(&m).unwrap_or_default();
            Ok::<Event, Infallible>(Event::default().data(data))
        }));
        return Ok(Sse::new(stream).into_response());
    }

    // Streaming mode: send cached messages first, then live ones.
    let since = params.since_time();
    let cached = {
        let conn = state.db.get()?;
        cache::since_time(&conn, &topic, since)?
    };

    // Subscribe to the broadcast channel before we start sending cached
    // messages, so we don't miss any messages published in between.
    let t = state.topics.get_or_create(&topic);
    let rx = t.tx.subscribe();

    let keepalive_secs = state.config.keepalive_secs;

    let stream = build_sse_stream(topic.clone(), cached, rx, keepalive_secs);

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
    _keepalive_secs: u64,
) -> impl Stream<Item = Result<Event, Infallible>> {
    // 1. Open event
    let open_msg = Message::new_open(&topic);
    let open_event = stream::once(async move {
        let data = serde_json::to_string(&open_msg).unwrap_or_default();
        Ok::<Event, Infallible>(Event::default().data(data))
    });

    // 2. Cached messages
    let cached_stream = stream::iter(cached.into_iter().map(|m| {
        let data = serde_json::to_string(&m).unwrap_or_default();
        Ok::<Event, Infallible>(Event::default().data(data))
    }));

    // 3. Live broadcast stream
    // BroadcastStream converts a broadcast::Receiver into a Stream.
    // Lagged errors are logged and skipped — the client can reconnect with
    // a `since` param to recover missed messages from the cache.
    let live_stream = BroadcastStream::new(rx).filter_map(|result| async move {
        match result {
            Ok(msg) => {
                let data = serde_json::to_string(&*msg).unwrap_or_default();
                Some(Ok::<Event, Infallible>(Event::default().data(data)))
            }
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                tracing::warn!(skipped = n, "subscriber lagged, skipping messages");
                None
            }
        }
    });

    open_event.chain(cached_stream).chain(live_stream)
}

// ── Poll: GET /{topic}/json?poll=1 ───────────────────────────────────────────
// (Handled by subscribe_sse above via the `poll` query param.)

// ── JSON poll convenience endpoint ───────────────────────────────────────────

/// GET /{topic}/json — returns a newline-delimited JSON stream (SSE-like but
/// without the `data:` prefix), matching ntfy's raw JSON subscribe format.
#[allow(dead_code)]
pub async fn subscribe_json(
    State(state): State<AppState>,
    Path(topic): Path<String>,
    Query(params): Query<SubscribeParams>,
) -> Result<impl IntoResponse, AppError> {
    // For Phase 1, the SSE and JSON endpoints share the same implementation.
    // The distinction (SSE framing vs raw NDJSON) will be split in Phase 4.
    subscribe_sse(State(state), Path(topic), Query(params)).await
}

// ── Multi-topic subscribe ─────────────────────────────────────────────────────

#[allow(dead_code)]
pub async fn subscribe_multi_sse(
    State(state): State<AppState>,
    Path(topics_raw): Path<String>,
    Query(params): Query<SubscribeParams>,
) -> Result<impl IntoResponse, AppError> {
    let topics = parse_topics(&topics_raw).ok_or(AppError::TopicInvalid)?;

    // For Phase 1, subscribe to the first topic only and note the limitation.
    // Full multi-topic fan-in (merging N broadcast streams) is Phase 4.
    let topic = topics.into_iter().next().ok_or(AppError::TopicInvalid)?;
    subscribe_sse(State(state), Path(topic), Query(params)).await
}
