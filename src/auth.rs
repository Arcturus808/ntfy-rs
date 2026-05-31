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

use crate::{
    config::{Config, DefaultAccess},
    db::{users as db_users, DbPool},
    error::AppError,
};
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

async fn authenticate(db: &DbPool, header: &str) -> Result<User, AppError> {
    if let Some(token) = header.strip_prefix("Bearer ").map(str::trim) {
        return authenticate_token(db, token);
    }
    if header.to_lowercase().starts_with("basic ") {
        return authenticate_basic(db, header);
    }
    Err(AppError::Unauthorized)
}

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
    use crate::config::DefaultAccess;
    use std::net::IpAddr;

    fn localhost() -> IpAddr {
        "127.0.0.1".parse().unwrap()
    }

    // ── Role ────────────────────────────────────────────────────────────

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

    // ── AuthUser ────────────────────────────────────────────────────────

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

    // ── authorize (unit-level) ───────────────────────────────────────────

    fn make_config(auth_enabled: bool, default_access: DefaultAccess) -> Config {
        Config {
            auth_enabled,
            default_access,
            ..crate::config::Config::resolve(
                crate::config::FileConfig::default(),
                &crate::config::ServeArgs {
                    config: std::path::PathBuf::from("server.toml"),
                    listen_http: None,
                    cache_file: None,
                    log_level: "info".to_string(),
                    base_url: None,
                    listen_https: None,
                    cert_file: None,
                    key_file: None,
                    listen_unix: None,
                    upstream_base_url: None,
                    upstream_access_token: None,
                },
            )
        }
    }

    fn test_db() -> DbPool {
        crate::db::open(None).unwrap()
    }

    #[test]
    fn test_authorize_auth_disabled_always_allowed() {
        let config = make_config(false, DefaultAccess::DenyAll);
        let user = AuthUser::anonymous(localhost());
        let db = test_db();
        assert!(authorize(&db, &config, &user, "test", Permission::Read).is_ok());
        assert!(authorize(&db, &config, &user, "test", Permission::Write).is_ok());
    }

    #[test]
    fn test_authorize_admin_always_allowed() {
        let config = make_config(true, DefaultAccess::DenyAll);
        let admin = AuthUser::authenticated(
            User {
                id: "1".to_string(),
                username: "admin".to_string(),
                hash: String::new(),
                role: Role::Admin,
            },
            localhost(),
        );
        let db = test_db();
        assert!(authorize(&db, &config, &admin, "test", Permission::Write).is_ok());
    }

    #[test]
    fn test_authorize_anonymous_read_write() {
        let config = make_config(true, DefaultAccess::ReadWrite);
        let anon = AuthUser::anonymous(localhost());
        let db = test_db();
        assert!(authorize(&db, &config, &anon, "test", Permission::Read).is_ok());
        assert!(authorize(&db, &config, &anon, "test", Permission::Write).is_ok());
    }

    #[test]
    fn test_authorize_anonymous_read_only() {
        let config = make_config(true, DefaultAccess::ReadOnly);
        let anon = AuthUser::anonymous(localhost());
        let db = test_db();
        assert!(authorize(&db, &config, &anon, "test", Permission::Read).is_ok());
        assert!(authorize(&db, &config, &anon, "test", Permission::Write).is_err());
    }

    #[test]
    fn test_authorize_anonymous_deny_all() {
        let config = make_config(true, DefaultAccess::DenyAll);
        let anon = AuthUser::anonymous(localhost());
        let db = test_db();
        assert!(authorize(&db, &config, &anon, "test", Permission::Read).is_err());
        assert!(authorize(&db, &config, &anon, "test", Permission::Write).is_err());
    }
}
