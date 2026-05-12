use crate::{
    handlers::{health, publish, subscribe},
    state::AppState,
};
use axum::{
    routing::{get, put},
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

pub fn build(state: AppState) -> Router {
    Router::new()
        // ── health / meta ─────────────────────────────────────────────────
        .route("/v1/health", get(health::health))
        .route("/v1/version", get(health::version))
        .route("/v1/stats", get(health::stats))
        // ── publish ───────────────────────────────────────────────────────
        // Single topic: PUT or POST /{topic}
        .route("/:topic", put(publish::publish).post(publish::publish))
        // ── subscribe (SSE) ───────────────────────────────────────────────
        // GET /{topic}/json  — SSE stream (or poll with ?poll=1)
        .route("/:topic/json", get(subscribe::subscribe_sse))
        // ── with_state ────────────────────────────────────────────────────
        .with_state(state)
        // ── middleware ────────────────────────────────────────────────────
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}
