//! Auth types, middleware, and ACL enforcement.
//!
//! # Flow
//!
//! Every request goes through `maybe_authenticate`:
//! - No `Authorization` header → anonymous `AuthUser` (IP-only)
//! - `Authorization: Basic <b64>` → bcrypt verify against users table
//! - `Authorization: Bearer <token>` → token lookup in tokens table
//! - `?auth=<b64(Basic b64(user:pass))>` → same as Basic (WebSocket compat)
//!
//! The resolved `AuthUser` is stored in request extensions and read by
//! `authorize_topic` before each publish/subscribe handler runs.
//!
//! When the `auth` feature is disabled, authentication is always anonymous
//! and authorization always succeeds — no bcrypt or user DB required.

use crate::{
    db::DbPool,
    error::AppError,
};
#[cfg(feature = "auth")]
use crate::config::{Config, DefaultAccess};
#[cfg(feature = "auth")]
use crate::db::users as db_users;
use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use std::{net::IpAddr, sync::Arc};

// ── domain types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Role {
    Admin,
    User,
}

impl Role {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "admin" => Role::Admin,
            _ => Role::User,
        }
    }

    pub fn is_admin(&self) -> bool {
        *self == Role::Admin
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Permission {
    Read,
    Write,
}

/// A user row from the database.
#[derive(Debug, Clone)]
pub struct User {
    pub id: String,
    #[allow(dead_code)]
    pub username: String,
    pub hash: String,
    pub role: Role,
}

/// The resolved identity attached to every request.
/// Anonymous when no valid credentials were supplied.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: Option<User>,
    pub ip: IpAddr,
}

impl AuthUser {
    pub fn anonymous(ip: IpAddr) -> Self {
        AuthUser { user: None, ip }
    }

    pub fn authenticated(user: User, ip: IpAddr) -> Self {
        AuthUser {
            user: Some(user),
            ip,
        }
    }

    pub fn is_admin(&self) -> bool {
        self.user.as_ref().map(|u| u.role.is_admin()).unwrap_or(false)
    }

    pub fn user_id(&self) -> Option<&str> {
        self.user.as_ref().map(|u| u.id.as_str())
    }
}

// ── axum middleware ───────────────────────────────────────────────────────────

/// Return a tower Layer that injects `AuthUser` into request extensions.
/// Captures db and config by value; no axum State extraction needed.
#[cfg(feature = "auth")]
pub fn make_auth_layer(db: DbPool, config: Arc<Config>) -> impl tower::Layer<
    axum::routing::Route,
    Service = impl tower::Service<
        Request,
        Response = Response,
        Error = std::convert::Infallible,
        Future = impl std::future::Future<Output = Result<Response, std::convert::Infallible>> + Send,
    > + Clone + Send + 'static,
> + Clone + 'static {
    axum::middleware::from_fn(move |mut req: Request, next: Next| {
        let db = db.clone();
        let config = Arc::clone(&config);
        async move {
            // Extract everything we need from req before any await point
            // so the &Request borrow doesn't cross an await boundary.
            let ip = extract_ip(&req);
            let auth_header = read_auth_header(&req);
            let auth_user = resolve_auth_parts(&db, &config, auth_header, ip).await;
            req.extensions_mut().insert(auth_user);
            next.run(req).await
        }
    })
}

/// No-op auth layer when the `auth` feature is disabled — always injects an anonymous user.
#[cfg(not(feature = "auth"))]
pub fn make_auth_layer(_db: DbPool, _config: Arc<()>) -> impl tower::Layer<
    axum::routing::Route,
    Service = impl tower::Service<
        Request,
        Response = Response,
        Error = std::convert::Infallible,
        Future = impl std::future::Future<Output = Result<Response, std::convert::Infallible>> + Send,
    > + Clone + Send + 'static,
> + Clone + 'static {
    axum::middleware::from_fn(move |mut req: Request, next: Next| {
        async move {
            let ip = extract_ip(&req);
            req.extensions_mut().insert(AuthUser::anonymous(ip));
            next.run(req).await
        }
    })
}

#[cfg(feature = "auth")]
async fn resolve_auth_parts(
    db: &DbPool,
    config: &Config,
    auth_header: Option<String>,
    ip: IpAddr,
) -> AuthUser {
    if !config.auth_enabled {
        return AuthUser::anonymous(ip);
    }
    let header = match auth_header {
        Some(h) => h,
        None => return AuthUser::anonymous(ip),
    };
    match authenticate(db, &header).await {
        Ok(user) => AuthUser::authenticated(user, ip),
        Err(_) => AuthUser::anonymous(ip),
    }
}

/// Read the raw Authorization header value, falling back to the `?auth=`
/// query param (doubly-base64-encoded, for WebSocket JS clients that cannot
/// set headers on the initial upgrade request).
fn read_auth_header(req: &Request) -> Option<String> {
    // Try the real header first.
    if let Some(v) = req.headers().get("authorization") {
        if let Ok(s) = v.to_str() {
            let s = s.trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    // Fall back to ?auth= query param: base64(Basic base64(user:pass))
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(val) = pair.strip_prefix("auth=") {
                if let Ok(decoded) = B64.decode(val) {
                    if let Ok(s) = String::from_utf8(decoded) {
                        return Some(s.trim().to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(feature = "auth")]
async fn authenticate(db: &DbPool, header: &str) -> Result<User, AppError> {
    if let Some(token) = header.strip_prefix("Bearer ").map(str::trim) {
        return authenticate_token(db, token);
    }
    if header.to_lowercase().starts_with("basic ") {
        return authenticate_basic(db, header);
    }
    Err(AppError::Unauthorized)
}

#[cfg(feature = "auth")]
fn authenticate_basic(db: &DbPool, header: &str) -> Result<User, AppError> {
    // Decode "Basic <base64(user:pass)>"
    let encoded = header
        .get(6..) // strip "Basic "
        .ok_or(AppError::Unauthorized)?
        .trim();
    let decoded = B64
        .decode(encoded)
        .map_err(|_| AppError::Unauthorized)?;
    let s = String::from_utf8(decoded).map_err(|_| AppError::Unauthorized)?;

    let (username, password) = s.split_once(':').ok_or(AppError::Unauthorized)?;

    // Empty username → treat password as a Bearer token (ntfy compat).
    if username.is_empty() {
        return authenticate_token(db, password);
    }

    let conn = db.get().map_err(|_| AppError::Unauthorized)?;
    let user = db_users::user_by_name(&conn, username)
        .map_err(|_| AppError::Unauthorized)?
        .ok_or(AppError::Unauthorized)?;

    // bcrypt verify is CPU-bound; run it on a blocking thread.
    let hash = user.hash.clone();
    let password = password.to_string();
    let ok = tokio::task::block_in_place(|| {
        bcrypt::verify(&password, &hash).unwrap_or(false)
    });

    if ok {
        Ok(user)
    } else {
        Err(AppError::Unauthorized)
    }
}

#[cfg(feature = "auth")]
fn authenticate_token(db: &DbPool, token: &str) -> Result<User, AppError> {
    let conn = db.get().map_err(|_| AppError::Unauthorized)?;
    db_users::user_by_token(&conn, token)
        .map_err(|_| AppError::Unauthorized)?
        .ok_or(AppError::Unauthorized)
}

// ── ACL enforcement ───────────────────────────────────────────────────────────

/// Check whether `auth_user` may perform `perm` on `topic`.
///
/// Rules (in order):
/// 1. Auth disabled → always allowed.
/// 2. Admin → always allowed.
/// 3. Authenticated user → check topic_acl table.
/// 4. Anonymous → apply `default_access` config.
#[cfg(feature = "auth")]
pub fn authorize(
    db: &DbPool,
    config: &Config,
    auth_user: &AuthUser,
    topic: &str,
    perm: Permission,
) -> Result<(), AppError> {
    if !config.auth_enabled {
        return Ok(());
    }
    if auth_user.is_admin() {
        return Ok(());
    }
    if let Some(user_id) = auth_user.user_id() {
        let conn = db.get().map_err(|_| AppError::Unauthorized)?;
        let allowed = db_users::acl_allowed(&conn, user_id, topic, perm)
            .map_err(|_| AppError::Internal("acl check failed".into()))?;
        if allowed {
            return Ok(());
        }
        return Err(AppError::Forbidden);
    }
    // Anonymous — apply default_access.
    match (&config.default_access, &perm) {
        (DefaultAccess::ReadWrite, _) => Ok(()),
        (DefaultAccess::ReadOnly, Permission::Read) => Ok(()),
        _ => Err(AppError::Unauthorized),
    }
}

/// When auth feature is disabled, authorization always succeeds.
#[cfg(not(feature = "auth"))]
pub fn authorize(
    _db: &DbPool,
    _config: &crate::config::Config,
    _auth_user: &AuthUser,
    _topic: &str,
    _perm: Permission,
) -> Result<(), AppError> {
    Ok(())
}

// ── IP extraction ─────────────────────────────────────────────────────────────

fn extract_ip(req: &Request) -> IpAddr {
    // Try X-Forwarded-For first (set by reverse proxies).
    if let Some(xff) = req.headers().get("x-forwarded-for") {
        if let Ok(s) = xff.to_str() {
            if let Some(first) = s.split(',').next() {
                if let Ok(ip) = first.trim().parse() {
                    return ip;
                }
            }
        }
    }
    // Fall back to a placeholder; real peer addr requires ConnectInfo extractor
    // which is wired in Phase 7 (TLS / production hardening).
    "127.0.0.1".parse().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "auth")]
    use crate::config::DefaultAccess;
    use std::net::IpAddr;

    fn localhost() -> IpAddr {
        "127.0.0.1".parse().unwrap()
    }

    // ── Role ─────────────────────────────────────────────────────────────

    #[test]
    fn test_role_from_str() {
        assert_eq!(Role::from_str("admin"), Role::Admin);
        assert_eq!(Role::from_str("user"), Role::User);
        assert_eq!(Role::from_str("anything"), Role::User);
    }

    #[test]
    fn test_role_as_str() {
        assert_eq!(Role::Admin.as_str(), "admin");
        assert_eq!(Role::User.as_str(), "user");
    }

    #[test]
    fn test_role_is_admin() {
        assert!(Role::Admin.is_admin());
        assert!(!Role::User.is_admin());
    }

    // ── AuthUser ────────────────────────────────────────────────────────────

    #[test]
    fn test_auth_user_anonymous() {
        let user = AuthUser::anonymous(localhost());
        assert!(user.user.is_none());
        assert!(!user.is_admin());
        assert!(user.user_id().is_none());
    }

    #[test]
    fn test_auth_user_authenticated_admin() {
        let user = AuthUser::authenticated(
            User {
                id: "abc".to_string(),
                username: "admin".to_string(),
                hash: String::new(),
                role: Role::Admin,
            },
            localhost(),
        );
        assert!(user.is_admin());
        assert_eq!(user.user_id(), Some("abc"));
    }

    #[test]
    fn test_auth_user_authenticated_non_admin() {
        let user = AuthUser::authenticated(
            User {
                id: "xyz".to_string(),
                username: "bob".to_string(),
                hash: String::new(),
                role: Role::User,
            },
            localhost(),
        );
        assert!(!user.is_admin());
        assert_eq!(user.user_id(), Some("xyz"));
    }

    // ── authorize (unit-level) ───────────────────────────────────────────────

    #[cfg(feature = "auth")]
    #[test]
    fn test_authorize_auth_disabled() {
        let config = Config {
            auth_enabled: false,
            default_access: DefaultAccess::ReadWrite,
            ..crate::config::Config::resolve(
                crate::config::FileConfig::default(),
                &crate::config::ServeArgs {
                    config: std::path::PathBuf::from("server.toml"),
                    listen_http: None,
                    cache_file: None,
                    log_level: "info".to_string(),
                    base_url: None,
                    upstream_base_url: None,
                    upstream_access_token: None,
                    #[cfg(feature = "tls")]
                    listen_https: None,
                    #[cfg(feature = "tls")]
                    cert_file: None,
                    #[cfg(feature = "tls")]
                    key_file: None,
                    #[cfg(feature = "unix-socket")]
                    listen_unix: None,
                },
            )
        };
        let user = AuthUser::anonymous(localhost());
        assert!(authorize(&crate::db::open(None).unwrap(), &config, &user, "t", Permission::Read).is_ok());
    }

    #[cfg(not(feature = "auth"))]
    #[test]
    fn test_authorize_always_ok_without_auth_feature() {
        let db = crate::db::open(None).unwrap();
        let config = crate::config::Config::resolve(
            crate::config::FileConfig::default(),
            &crate::config::ServeArgs::default(),
        );
        let user = AuthUser::anonymous(localhost());
        assert!(authorize(&db, &config, &user, "t", Permission::Read).is_ok());
    }
}
