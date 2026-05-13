use clap::Parser;
use tracing_subscriber::{fmt, EnvFilter};

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
        listen_https = ?cfg.listen_https,
        listen_unix  = ?cfg.listen_unix,
        auth         = cfg.auth_enabled,
        "starting ntfy-rs"
    );

    let handle = ntfy_rs::start_async(cfg).await?;

    // Wait for Ctrl-C, then shut down gracefully.
    tokio::signal::ctrl_c().await?;
    tracing::info!("shutting down");
    handle.shutdown();

    Ok(())
}
