mod config;
mod db;
mod error;
mod handlers;
mod manager;
mod message;
mod router;
mod state;
mod topic;

use clap::Parser;
use config::{load_file_config, Config};
use state::AppState;
use std::net::SocketAddr;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = config::Cli::parse();

    // Initialise logging. RUST_LOG overrides --log-level.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&cli.log_level));
    fmt().with_env_filter(filter).init();

    // Load config file (missing file is not an error).
    let file_cfg = load_file_config(&cli.config)?;
    let cfg = Config::resolve(file_cfg, &cli);

    tracing::info!(listen = %cfg.listen_http, "starting ntfy-rs");

    // Open database.
    let db = db::open(cfg.cache_file.as_ref())?;

    // Build shared state.
    let state = AppState::new(cfg.clone(), db);

    // Spawn background manager.
    {
        let s = state.clone();
        tokio::spawn(async move { manager::run(s).await });
    }

    // Build router.
    let app = router::build(state);

    // Resolve listen address. Support ":port" shorthand.
    let addr: SocketAddr = normalise_addr(&cfg.listen_http)?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "listening");

    axum::serve(listener, app).await?;

    Ok(())
}

/// Convert ":2586" -> "0.0.0.0:2586", leave "127.0.0.1:2586" unchanged.
fn normalise_addr(s: &str) -> anyhow::Result<SocketAddr> {
    let s = if s.starts_with(':') {
        format!("0.0.0.0{s}")
    } else {
        s.to_string()
    };
    Ok(s.parse()?)
}
