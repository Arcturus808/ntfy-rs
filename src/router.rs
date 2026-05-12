use crate::{
    auth,
    handlers::{account, admin, health, matrix, publish, subscribe, ws},
    state::AppState,
};
use axum::{routing::{delete, get, post, put}, Router};
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

pub fn build(state: AppState) -> Router {
    let auth_layer = auth::make_auth_layer(
        state.effective_auth_db().clone(),
        Arc::clone(&state.config),
    );

    // A single path parameter captures both "topic" and "topic1,topic2,...".
    // The handlers themselves detect commas and dispatch to single vs multi logic.
    let protected = Router::new()
        // ── publish ───────────────────────────────────────────────────────
        .route("/:topic",       put(publish::publish).post(publish::publish))
        // ── subscribe ─────────────────────────────────────────────────────
        .route("/:topics/json", get(subscribe::subscribe_ndjson))
        .route("/:topics/sse",  get(subscribe::subscribe_sse))
        .route("/:topics/ws",   get(ws::subscribe_ws))
        // ── client auth check ─────────────────────────────────────────────
        .route("/:topic/auth",  get(health::topic_auth))
        // ── self-service account ──────────────────────────────────────────
        .route("/v1/account",                    get(account::get_account).delete(account::delete_account))
        .route("/v1/account/password",           put(account::change_password))
        .route("/v1/account/token",              post(account::create_token))
        .route("/v1/account/token/:token",       delete(account::delete_token))
        .route("/v1/account/access",             get(account::get_access).post(account::set_access))
        .route("/v1/account/access/:topic",      delete(account::delete_access))
        // ── Matrix Push Gateway ───────────────────────────────────────────
        .route("/_matrix/push/v1/notify",  post(matrix::notify))
        // ── admin ─────────────────────────────────────────────────────────
        .route("/v1/admin/users",                          get(admin::list_users).post(admin::create_user))
        .route("/v1/admin/users/:username",                delete(admin::delete_user))
        .route("/v1/admin/users/:username/role",           put(admin::set_role))
        .route("/v1/admin/users/:username/access",         post(admin::set_access))
        .route("/v1/admin/users/:username/access/:topic",  delete(admin::delete_access))
        .route_layer(auth_layer)
        .with_state(state.clone());

    Router::new()
        // Unauthenticated endpoints (outside the auth layer).
        .route("/v1/account",                post(account::register))
        .route("/v1/health",                 get(health::health))
        .route("/v1/version",                get(health::version))
        .route("/v1/config",                 get(health::config))
        .route("/v1/stats",                  get(health::stats))
        // Matrix Push Gateway discovery (unauthenticated GET).
        .route("/_matrix/push/v1/notify",    get(matrix::discovery))
        .with_state(state)
        .merge(protected)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}
