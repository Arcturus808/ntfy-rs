mod auth;
pub mod config;
mod db;
#[cfg(feature = "email")]
mod email;
mod emoji;
mod error;
mod handlers;
mod manager;
mod message;
mod router;
mod state;
mod topic;
mod upstream;
mod visitor;
#[cfg(feature = "webpush")]
mod webpush;

pub use config::{Config, FileConfig, ServeArgs};

use state::AppState;
use std::net::SocketAddr;

/// Handle to a running ntfy-rs server. Call `shutdown()` to stop it.
pub struct ServerHandle {
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    port: u16,
}

impl ServerHandle {
    /// Shut down the server gracefully.
    pub fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Get the port the server is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Publish a message directly via HTTP POST to localhost.
    ///
    /// Uses a raw `TcpStream` rather than an HTTP client library so that no
    /// additional thread-pool or async-runtime machinery is introduced — keeping
    /// the import footprint minimal and avoiding false positives from
    /// behaviour-based AV scanners.
    pub fn publish(
        &self,
        topic: &str,
        title: &str,
        message: &str,
        priority: &str,
    ) -> anyhow::Result<()> {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        let body = message.as_bytes();
        let timeout = Duration::from_secs(10);

        // Build request headers. `Connection: close` causes the server to close
        // the connection after the response, so read_to_end() terminates cleanly.
        let mut request = format!(
            "POST /{topic} HTTP/1.1\r\n\
             Host: 127.0.0.1:{port}\r\n\
             Connection: close\r\n\
             Content-Length: {len}\r\n",
            topic = topic,
            port = self.port,
            len = body.len(),
        );
        if !title.is_empty() {
            request.push_str(&format!("Title: {title}\r\n"));
        }
        if !priority.is_empty() {
            request.push_str(&format!("Priority: {priority}\r\n"));
        }
        request.push_str("\r\n");

        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", self.port))
            .map_err(|e| anyhow::anyhow!("failed to connect to ntfy-rs: {e}"))?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;

        stream.write_all(request.as_bytes())?;
        stream.write_all(body)?;

        // Read the response and check the status line.
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .map_err(|e| anyhow::anyhow!("failed to read response: {e}"))?;

        let status = std::str::from_utf8(&response)
            .ok()
            .and_then(|s| s.lines().next())
            .unwrap_or("");
        if !status.contains(" 200 ") {
            return Err(anyhow::anyhow!("publish failed: {status}"));
        }

        Ok(())
    }
}

/// Start the ntfy-rs server on a background thread.
///
/// Blocks until the server has bound its port (or returns an error).
/// Returns a handle that can be used to publish messages and shut down the server.
///
/// Prefer [`start_async`] when embedding ntfy-rs inside an application that
/// already has a tokio runtime. `start` spawns a new OS thread and creates
/// its own `tokio::runtime::Runtime`, which is necessary for sync callers but
/// can be misread by behaviour-based AV scanners as an embedded backdoor pattern.
pub fn start(config: Config) -> anyhow::Result<ServerHandle> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (started_tx, started_rx) = std::sync::mpsc::channel::<anyhow::Result<u16>>();

    std::thread::Builder::new()
        .name("ntfy-rs-server".to_string())
        .spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            rt.block_on(async move {
                if let Err(e) = run_server(config, shutdown_rx, started_tx).await {
                    tracing::error!(error = %e, "ntfy-rs server error");
                }
            });
        })
        .map_err(|e| anyhow::anyhow!("failed to spawn server thread: {}", e))?;

    // Wait for the server to bind (or fail).
    let port = started_rx
        .recv()
        .map_err(|_| anyhow::anyhow!("server thread exited before startup"))?
        .map_err(|e| anyhow::anyhow!("server failed to start: {}", e))?;

    Ok(ServerHandle {
        shutdown_tx,
        port,
    })
}

/// Start the ntfy-rs server as an async task within the **current** tokio runtime.
///
/// Preferred over [`start`] when the caller already has a tokio runtime
/// (e.g. an `#[tokio::main]` application or an embedded async app). Uses
/// `tokio::spawn` rather than spawning a dedicated OS thread with its own
/// runtime, which avoids the "hidden thread + private runtime + network
/// listener" pattern that behaviour-based AV scanners can misclassify as an
/// embedded backdoor.
///
/// Awaits until the server has bound its port, then returns a [`ServerHandle`].
pub async fn start_async(config: Config) -> anyhow::Result<ServerHandle> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (started_tx, started_rx) = std::sync::mpsc::channel::<anyhow::Result<u16>>();

    tokio::spawn(async move {
        if let Err(e) = run_server(config, shutdown_rx, started_tx).await {
            tracing::error!(error = %e, "ntfy-rs server error");
        }
    });

    // `mpsc::Receiver::recv` is blocking. Offload to the blocking thread pool
    // so the async executor is not stalled while the server binds its port.
    // The send happens almost immediately after `TcpListener::bind`, so this
    // completes in microseconds in practice.
    let port = tokio::task::spawn_blocking(move || started_rx.recv())
        .await
        .map_err(|_| anyhow::anyhow!("startup task panicked"))?
        .map_err(|_| anyhow::anyhow!("server task exited before startup"))?
        .map_err(|e| anyhow::anyhow!("server failed to start: {}", e))?;

    Ok(ServerHandle { shutdown_tx, port })
}

// ── Internal server runner ───────────────────────────────────────────────

async fn run_server(
    config: Config,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    started_tx: std::sync::mpsc::Sender<anyhow::Result<u16>>,
) -> anyhow::Result<()> {
    // Install the aws-lc-rs crypto provider for rustls.
    #[cfg(feature = "tls")]
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Open message cache DB.
    let db = db::open(config.cache_file.as_ref())?;

    // Open auth DB (separate file when auth is enabled).
    #[cfg(feature = "auth")]
    let auth_db = if config.auth_enabled {
        Some(db::open(config.auth_file.as_ref())?)
    } else {
        None
    };
    #[cfg(not(feature = "auth"))]
    let auth_db: Option<crate::db::DbPool> = None;
    // non-fatal: web push is simply disabled for this run.
    #[cfg(feature = "webpush")]
    let vapid = match webpush::load_or_generate(&db) {
        Ok(v) => {
            tracing::info!(public_key = %v.public_key_b64, "VAPID key loaded");
            Some(std::sync::Arc::new(v))
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to load VAPID keys — web push disabled");
            None
        }
    };
    #[cfg(not(feature = "webpush"))]
    let vapid: Option<std::sync::Arc<()>> = None;

    let state = AppState::new(config.clone(), db, auth_db, vapid);

    // Install Prometheus metrics recorder.
    // Non-fatal on restart: the recorder is a process-global singleton
    // that can only be installed once, so a second call returns an error.
    #[cfg(feature = "metrics")]
    let metrics_handle = match metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
    {
        Ok(handle) => handle,
        Err(_) => {
            // Recorder already installed globally; build a local recorder
            // and use its handle for rendering scrape output.
            metrics_exporter_prometheus::PrometheusBuilder::new()
                .build_recorder()
                .handle()
        }
    };

    // Background manager.
    {
        let s = state.clone();
        tokio::spawn(async move { manager::run(s).await });
    }

    #[cfg(feature = "metrics")]
    let app = router::build(state, metrics_handle);
    #[cfg(not(feature = "metrics"))]
    let app = router::build(state);

    // ── HTTP listener ─────────────────────────────────────────────────────
    let http_addr: SocketAddr = normalise_addr(&config.listen_http)?;
    let http_listener = tokio::net::TcpListener::bind(http_addr).await;
    match http_listener {
        Ok(listener) => {
            let local_addr = listener.local_addr()?;
            let port = local_addr.port();
            tracing::info!(addr = %local_addr, "HTTP listening");
            let _ = started_tx.send(Ok(port));
            run_select(listener, app, config, shutdown_rx).await?;
        }
        Err(e) => {
            let _ = started_tx.send(Err(anyhow::anyhow!("bind failed: {}", e)));
        }
    }

    Ok(())
}

async fn run_select(
    http_listener: tokio::net::TcpListener,
    app: axum::Router,
    config: Config,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    // ── HTTPS listener (optional) ─────────────────────────────────────────
    #[cfg(feature = "tls")]
    let tls_server = match (&config.listen_https, &config.cert_file, &config.key_file) {
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

    #[cfg(feature = "unix-socket")]
    #[allow(unused_variables)]
    let unix_path = config.listen_unix.clone();

    tokio::select! {
        res = axum::serve(http_listener, app.clone())
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            }) => {
            res?;
        }
        res = async {
            #[cfg(feature = "tls")]
            if let Some((tls_addr, tls_cfg)) = tls_server {
                axum_server::bind_rustls(tls_addr, tls_cfg)
                    .serve(app.clone().into_make_service())
                    .await
            } else {
                std::future::pending::<Result<(), std::io::Error>>().await
            }
            #[cfg(not(feature = "tls"))]
            std::future::pending::<Result<(), std::io::Error>>().await
        } => {
            res?;
        }
        res = async {
            #[cfg(all(unix, feature = "unix-socket"))]
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
#[cfg(all(unix, feature = "unix-socket"))]
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
                .with_upgrades()
                .await;
        });
    }
}
