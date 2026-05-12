use crate::{
    auth::{authorize, AuthUser, Permission},
    error::AppError,
    message::{parse_topics, valid_topic, Message},
    state::AppState,
};
use super::subscribe::{resolve_since, SubscribeParams};
use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    response::Response,
    Extension,
};
use std::{sync::Arc, time::Duration};
use tokio::{sync::broadcast, time};
use tokio_stream::{wrappers::BroadcastStream, StreamExt as _};



// ── Single-topic WebSocket ────────────────────────────────────────────────────

/// GET /{topics}/ws — single or comma-separated multi-topic
pub async fn subscribe_ws(
    State(state): State<AppState>,
    Path(topics_raw): Path<String>,
    Query(params): Query<SubscribeParams>,
    Extension(auth_user): Extension<AuthUser>,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    if topics_raw.contains(',') {
        return subscribe_ws_multi(
            State(state), Path(topics_raw), Query(params), Extension(auth_user), ws,
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

    let t = state.topics.get_or_create(&topic);
    let rx = t.tx.subscribe();
    visitor.increment_subscriptions();

    let keepalive_secs = state.config.keepalive_secs;
    let visitor_clone = Arc::clone(&visitor);

    Ok(ws.on_upgrade(move |socket| {
        handle_ws(socket, topic, cached, rx, keepalive_secs, visitor_clone)
    }))
}

/// GET /{topics}/ws  (comma-separated, multi-topic) — called internally
pub async fn subscribe_ws_multi(
    State(state): State<AppState>,
    Path(topics_raw): Path<String>,
    Query(params): Query<SubscribeParams>,
    Extension(auth_user): Extension<AuthUser>,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    let topics = parse_topics(&topics_raw).ok_or(AppError::TopicInvalid)?;

    let visitor = state.visitors.get_or_create(auth_user.ip);
    if !visitor.request_allowed() {
        return Err(AppError::TooManyRequests);
    }

    // Authorize read on every requested topic before upgrading.
    for topic in &topics {
        authorize(
            state.effective_auth_db(),
            &state.config,
            &auth_user,
            topic,
            Permission::Read,
        )?;
    }

    // Collect cached messages and subscribe to each topic's broadcast channel.
    let mut cached: Vec<Message> = Vec::new();
    let mut receivers: Vec<broadcast::Receiver<Arc<Message>>> = Vec::new();

    for topic in &topics {
        let mut msgs = resolve_since(&state, topic, &params)?;
        cached.append(&mut msgs);
        let t = state.topics.get_or_create(topic);
        receivers.push(t.tx.subscribe());
    }

    // Sort cached messages by time so multi-topic history is chronological.
    cached.sort_by_key(|m| m.time);

    visitor.increment_subscriptions();

    let keepalive_secs = state.config.keepalive_secs;
    let visitor_clone = Arc::clone(&visitor);
    // Use the first topic name for the open event (matches ntfy behaviour).
    let first_topic = topics[0].clone();

    Ok(ws.on_upgrade(move |socket| {
        handle_ws_multi(
            socket,
            first_topic,
            cached,
            receivers,
            keepalive_secs,
            visitor_clone,
        )
    }))
}

// ── WebSocket session handlers ────────────────────────────────────────────────

async fn handle_ws(
    mut socket: WebSocket,
    topic: String,
    cached: Vec<Message>,
    rx: broadcast::Receiver<Arc<Message>>,
    keepalive_secs: u64,
    visitor: Arc<crate::visitor::Visitor>,
) {
    let _guard = crate::visitor::SubscriptionGuard(Arc::clone(&visitor));

    // Send open event.
    let open = Message::new_open(&topic);
    if send_json(&mut socket, &open).await.is_err() {
        return;
    }

    // Replay cached messages.
    for msg in &cached {
        if send_json(&mut socket, msg).await.is_err() {
            return;
        }
    }

    // Stream live messages, interleaved with keepalive pings.
    let mut live = BroadcastStream::new(rx);
    let mut keepalive = time::interval(Duration::from_secs(keepalive_secs));
    keepalive.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            // Incoming frame from client (ping/pong/close).
            client_msg = socket.recv() => {
                match client_msg {
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    Some(Ok(WsMessage::Ping(data))) => {
                        if socket.send(WsMessage::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    _ => {} // ignore text/binary frames from client
                }
            }
            // Live broadcast message.
            item = live.next() => {
                match item {
                    Some(Ok(msg)) => {
                        if send_json(&mut socket, &msg).await.is_err() {
                            break;
                        }
                    }
                    Some(Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n))) => {
                        tracing::warn!(skipped = n, "ws subscriber lagged");
                        // Continue — client can reconnect with ?since= to recover.
                    }
                    None => break, // channel closed
                }
            }
            // Keepalive ping to prevent proxy timeouts.
            _ = keepalive.tick() => {
                let ka = Message::new_keepalive(&topic);
                if send_json(&mut socket, &ka).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn handle_ws_multi(
    mut socket: WebSocket,
    first_topic: String,
    cached: Vec<Message>,
    receivers: Vec<broadcast::Receiver<Arc<Message>>>,
    keepalive_secs: u64,
    visitor: Arc<crate::visitor::Visitor>,
) {
    let _guard = crate::visitor::SubscriptionGuard(Arc::clone(&visitor));

    let open = Message::new_open(&first_topic);
    if send_json(&mut socket, &open).await.is_err() {
        return;
    }

    for msg in &cached {
        if send_json(&mut socket, msg).await.is_err() {
            return;
        }
    }

    // Merge all receivers into one stream via select_all (fair round-robin).
    let boxed: Vec<_> = receivers
        .into_iter()
        .map(|rx| Box::pin(BroadcastStream::new(rx)))
        .collect();
    let mut merged = futures_util::stream::select_all(boxed);

    let mut keepalive = time::interval(Duration::from_secs(keepalive_secs));
    keepalive.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            client_msg = socket.recv() => {
                match client_msg {
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    Some(Ok(WsMessage::Ping(data))) => {
                        if socket.send(WsMessage::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            item = futures_util::StreamExt::next(&mut merged) => {
                match item {
                    Some(Ok(msg)) => {
                        if send_json(&mut socket, &msg).await.is_err() {
                            break;
                        }
                    }
                    Some(Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n))) => {
                        tracing::warn!(skipped = n, "ws multi subscriber lagged");
                    }
                    None => break,
                }
            }
            _ = keepalive.tick() => {
                let ka = Message::new_keepalive(&first_topic);
                if send_json(&mut socket, &ka).await.is_err() {
                    break;
                }
            }
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

async fn send_json(socket: &mut WebSocket, msg: &Message) -> Result<(), ()> {
    let text = serde_json::to_string(msg).map_err(|_| ())?;
    socket
        .send(WsMessage::Text(text.into()))
        .await
        .map_err(|_| ())
}
