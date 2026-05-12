mod auth;
mod config;
mod db;
mod error;
mod handlers;
mod manager;
mod message;
mod router;
mod state;
mod topic;
mod upstream;
mod visitor;

use clap::Parser;
use config::{load_file_config, Config};
use state::AppState;
use std::net::SocketAddr;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = config::Cli::parse();

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&cli.log_level));
    fmt().with_env_filter(filter).init();

    // Install the ring crypto provider for rustls. Must happen before any TLS
    // config is loaded. Safe to call multiple times (subsequent calls are no-ops).
    let _ = rustls::crypto::ring::default_provider().install_default();

    let file_cfg = load_file_config(&cli.config)?;
    let cfg = Config::resolve(file_cfg, &cli);

    tracing::info!(
        listen_http  = %cfg.listen_http,
        listen_https = ?cfg.listen_https,
        listen_unix  = ?cfg.listen_unix,
        auth         = cfg.auth_enabled,
        "starting ntfy-rs"
    );

    // Open message cache DB.
    let db = db::open(cfg.cache_file.as_ref())?;

    // Open auth DB (separate file when auth is enabled).
    let auth_db = if cfg.auth_enabled {
        Some(db::open(cfg.auth_file.as_ref())?)
    } else {
        None
    };

    let state = AppState::new(cfg.clone(), db, auth_db);

    // Background manager.
    {
        let s = state.clone();
        tokio::spawn(async move { manager::run(s).await });
    }

    let app = router::build(state);

    // ── HTTP listener ─────────────────────────────────────────────────────
    let http_addr: SocketAddr = normalise_addr(&cfg.listen_http)?;
    let http_listener = tokio::net::TcpListener::bind(http_addr).await?;
    tracing::info!(addr = %http_listener.local_addr()?, "HTTP listening");

    // ── HTTPS listener (optional) ─────────────────────────────────────────
    let tls_server = match (&cfg.listen_https, &cfg.cert_file, &cfg.key_file) {
        (Some(addr), Some(cert), Some(key)) => {
            let tls_cfg = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key)
                .await
                .map_err(|e| anyhow::anyhow!("failed to load TLS cert/key: {e}"))?;
            let tls_addr: SocketAddr = normalise_addr(addr)?;
            tracing::info!(%tls_addr, "HTTPS listening");
            Some((tls_addr, tls_cfg))
        }
        (Some(_), _, _) => {
            tracing::warn!("listen_https set but cert_file/key_file missing — TLS disabled");
            None
        }
        _ => None,
    };

    // ── Unix socket listener (optional, Unix only) ───────────────────────
    #[allow(unused_variables)]
    let unix_path = cfg.listen_unix.clone();

    // Serve all listeners concurrently; if any fails the process exits.
    tokio::select! {
        res = axum::serve(http_listener, app.clone()) => {
            res?;
        }
        res = async {
            if let Some((tls_addr, tls_cfg)) = tls_server {
                axum_server::bind_rustls(tls_addr, tls_cfg)
                    .serve(app.clone().into_make_service())
                    .await
            } else {
                // Park this branch forever so select! doesn't immediately resolve.
                std::future::pending::<Result<(), std::io::Error>>().await
            }
        } => {
            res?;
        }
        res = async {
            #[cfg(unix)]
            if let Some(path) = unix_path {
                return serve_unix(path, app.clone()).await;
            }
            std::future::pending::<anyhow::Result<()>>().await
        } => {
            res?;
        }
    }

    Ok(())
}

fn normalise_addr(s: &str) -> anyhow::Result<SocketAddr> {
    let s = if s.starts_with(':') {
        format!("0.0.0.0{s}")
    } else {
        s.to_string()
    };
    Ok(s.parse()?)
}

/// Serve an axum Router over a Unix domain socket.
///
/// axum::serve only accepts TcpListener, so we drive hyper directly —
/// the same approach axum uses internally for TCP.
#[cfg(unix)]
async fn serve_unix(path: std::path::PathBuf, app: axum::Router) -> anyhow::Result<()> {
    use hyper::server::conn::http1;
    use hyper_util::rt::TokioIo;
    use tower::ServiceExt as _;

    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path)
        .map_err(|e| anyhow::anyhow!("failed to bind Unix socket {}: {e}", path.display()))?;
    tracing::info!(path = %path.display(), "Unix socket listening");

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let tower_svc = app.clone();
        tokio::spawn(async move {
            let _ = http1::Builder::new()
                .serve_connection(
                    io,
                    hyper::service::service_fn(move |req| {
                        let svc = tower_svc.clone();
                        async move { svc.oneshot(req).await }
                    }),
                )
                .with_upgrades() // needed for WebSocket upgrades
                .await;
        });
    }
}
