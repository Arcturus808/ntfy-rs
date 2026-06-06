#[cfg(feature = "config-file")]
use clap::Parser;
use tracing_subscriber::{fmt, EnvFilter};

#[cfg(feature = "config-file")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = ntfy_rs::config::Cli::parse();

    let serve_args = match cli.command {
        ntfy_rs::config::Commands::Serve(args) => args,
    };

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&serve_args.log_level));
    fmt().with_env_filter(filter).init();

    let file_cfg = ntfy_rs::config::load_file_config(&serve_args.config)?;
    let cfg = ntfy_rs::Config::resolve(file_cfg, &serve_args);

    tracing::info!(
        listen_http  = %cfg.listen_http,
        auth         = cfg.auth_enabled,
        "starting ntfy-rs"
    );

    #[cfg(feature = "tls")]
    tracing::info!(listen_https = ?cfg.listen_https, "tls configured");

    #[cfg(feature = "unix-socket")]
    tracing::info!(listen_unix = ?cfg.listen_unix, "unix socket configured");

    let handle = ntfy_rs::start_async(cfg).await?;

    // Wait for Ctrl-C, then shut down gracefully.
    tokio::signal::ctrl_c().await?;
    tracing::info!("shutting down");
    handle.shutdown();

    Ok(())
}

#[cfg(not(feature = "config-file"))]
fn main() {
    eprintln!("ntfy-rs: the standalone binary requires the 'config-file' feature. \
               Use the library API (ntfy_rs::start / start_async) to embed the server.");
    std::process::exit(1);
}
