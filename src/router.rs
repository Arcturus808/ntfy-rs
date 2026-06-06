use crate::{
    state::AppState,
};
#[cfg(feature = "auth")]
use crate::auth;
#[cfg(feature = "auth")]
use crate::handlers::{account, admin};
use crate::handlers::{file, health, publish, subscribe, ws};
#[cfg(feature = "metrics")]
use crate::handlers::metrics as metrics_handler;
#[cfg(feature = "webpush")]
use crate::handlers::webpush;
use crate::handlers::matrix;
use axum::{routing::{get, post, put}, Router};
#[cfg(feature = "auth")]
use axum::routing::delete;
#[cfg(feature = "metrics")]
use metrics_exporter_prometheus::PrometheusHandle;
#[cfg(feature = "auth")]
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

#[cfg(feature = "metrics")]
pub fn build(state: AppState, metrics_handle: PrometheusHandle) -> Router {
    build_inner(state, Some(metrics_handle))
}

#[cfg(not(feature = "metrics"))]
pub fn build(state: AppState) -> Router {
    build_inner(state, None)
}

fn build_inner(state: AppState, _metrics_handle: Option<MetricsHandle>) -> Router {
    let body_limit = state.config.attachment_file_size_limit as usize;

    #[cfg(feature = "auth")]
    let auth_layer = auth::make_auth_layer(
        state.effective_auth_db().clone(),
        Arc::clone(&state.config),
    );

    // A single path parameter captures both "topic" and "topic1,topic2,...".
    // The handlers themselves detect commas and dispatch to single vs multi logic.
    let mut protected = Router::new()
        // ── publish ───────────────────────────────────────────────────────
        .route("/:topic",       put(publish::publish).post(publish::publish))
        // ── subscribe ─────────────────────────────────────────────────────
        .route("/:topics/json", get(subscribe::subscribe_ndjson))
        .route("/:topics/sse",  get(subscribe::subscribe_sse))
        .route("/:topics/ws",   get(ws::subscribe_ws))
        // ── client auth check ─────────────────────────────────────────────
        .route("/:topic/auth",  get(health::topic_auth))
        // ── Matrix Push Gateway ───────────────────────────────────────────
        .route("/_matrix/push/v1/notify",  post(matrix::notify));

    #[cfg(feature = "auth")]
    {
        protected = protected
            // ── self-service account ──────────────────────────────────────────
            .route("/v1/account",                    get(account::get_account).delete(account::delete_account))
            .route("/v1/account/password",           put(account::change_password))
            .route("/v1/account/token",              post(account::create_token))
            .route("/v1/account/token/:token",       delete(account::delete_token))
            .route("/v1/account/access",             get(account::get_access).post(account::set_access))
            .route("/v1/account/access/:topic",      delete(account::delete_access))
            // ── admin ─────────────────────────────────────────────────────────
            .route("/v1/admin/users",                          get(admin::list_users).post(admin::create_user))
            .route("/v1/admin/users/:username",                delete(admin::delete_user))
            .route("/v1/admin/users/:username/role",           put(admin::set_role))
            .route("/v1/admin/users/:username/access",         post(admin::set_access))
            .route("/v1/admin/users/:username/access/:topic",  delete(admin::delete_access));
    }

    #[cfg(feature = "auth")]
    let protected = protected
        .route_layer(auth_layer)
        .with_state(state.clone());

    #[cfg(not(feature = "auth"))]
    let protected = protected.with_state(state.clone());

    let mut unauthenticated = Router::new()
        .route("/v1/health",                 get(health::health))
        .route("/v1/version",                get(health::version))
        .route("/v1/config",                 get(health::config))
        .route("/v1/stats",                  get(health::stats))
        // Matrix Push Gateway discovery (unauthenticated GET).
        .route("/_matrix/push/v1/notify",    get(matrix::discovery))
        // File attachment downloads (unauthenticated — opaque ID is the access control).
        .route("/file/:id",                  get(file::serve_file));

    #[cfg(feature = "auth")]
    {
        unauthenticated = unauthenticated.route("/v1/account", post(account::register));
    }

    #[cfg(feature = "metrics")]
    {
        unauthenticated = unauthenticated.route("/metrics", get(metrics_handler::metrics));
    }

    #[cfg(feature = "webpush")]
    {
        // Web push: VAPID public key + subscription management.
        // All three are unauthenticated — browsers register subscriptions before
        // they have a session token, and the opaque UUID acts as access control
        // for deletion.
        unauthenticated = unauthenticated
            .route("/v1/webpush/vapid-key",           get(webpush::get_vapid_key))
            .route("/v1/webpush/subscriptions",       post(webpush::subscribe))
            .route("/v1/webpush/subscriptions/:id",   delete(webpush::unsubscribe));
    }

    let app = unauthenticated
        .with_state(state)
        .merge(protected);

    #[cfg(feature = "metrics")]
    let app = app.layer(axum::extract::Extension(_metrics_handle.unwrap()));

    app
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(axum::extract::DefaultBodyLimit::max(body_limit))
}

// Type alias to avoid repeating the Option<MetricsHandle> type.
#[cfg(feature = "metrics")]
type MetricsHandle = PrometheusHandle;
#[cfg(not(feature = "metrics"))]
type MetricsHandle = ();
