use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;

/// What unauthenticated (anonymous) callers may do when auth is enabled.
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum DefaultAccess {
    /// Anyone may read and write (default when auth is disabled).
    #[default]
    ReadWrite,
    /// Anyone may read; only authenticated users may write.
    ReadOnly,
    /// Only authenticated users may read or write.
    DenyAll,
}

/// Default values
pub const DEFAULT_LISTEN_HTTP: &str = ":2586";
pub const DEFAULT_CACHE_DURATION_SECS: u64 = 12 * 60 * 60; // 12 hours
pub const DEFAULT_MESSAGE_SIZE_LIMIT: usize = 4096;         // 4 KiB
pub const DEFAULT_REQUEST_LIMIT_BURST: u32 = 60;
pub const DEFAULT_REQUEST_LIMIT_REPLENISH_SECS: u64 = 5;
pub const DEFAULT_SUBSCRIPTION_LIMIT: u32 = 30;
pub const DEFAULT_KEEPALIVE_SECS: u64 = 45;
pub const DEFAULT_MANAGER_INTERVAL_SECS: u64 = 3 * 60; // 3 minutes

/// CLI arguments. Values here override the config file.
#[derive(Parser, Debug)]
#[command(name = "ntfy-rs", about = "Lightweight pub/sub notification server")]
pub struct Cli {
    /// Path to config file (TOML)
    #[arg(short, long, env = "NTFY_CONFIG_FILE", default_value = "server.toml")]
    pub config: PathBuf,

    /// HTTP listen address, e.g. ":2586" or "127.0.0.1:8080"
    #[arg(long, env = "NTFY_LISTEN_HTTP")]
    pub listen_http: Option<String>,

    /// SQLite database file path (empty = in-memory)
    #[arg(long, env = "NTFY_CACHE_FILE")]
    pub cache_file: Option<PathBuf>,

    /// Log level: trace, debug, info, warn, error
    #[arg(long, env = "NTFY_LOG_LEVEL", default_value = "info")]
    pub log_level: String,

    /// Base URL of this server (e.g. https://ntfy.example.com)
    #[arg(long, env = "NTFY_BASE_URL")]
    pub base_url: Option<String>,
}

/// File-based config (TOML). All fields are optional; defaults apply when absent.
#[derive(Debug, Deserialize, Default)]
pub struct FileConfig {
    pub listen_http: Option<String>,
    pub base_url: Option<String>,
    pub cache_file: Option<PathBuf>,
    pub cache_duration: Option<u64>,
    pub message_size_limit: Option<usize>,
    pub request_limit_burst: Option<u32>,
    pub request_limit_replenish: Option<u64>,
    pub subscription_limit: Option<u32>,
    pub keepalive_interval: Option<u64>,
    pub manager_interval: Option<u64>,
    /// When set, auth is enabled and the SQLite auth DB is stored here.
    /// When absent, auth is disabled and all requests are allowed.
    pub auth_file: Option<PathBuf>,
    pub default_access: Option<DefaultAccess>,
}

/// Resolved, fully-populated config used at runtime.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Config {
    pub listen_http: String,
    pub base_url: String,
    pub cache_file: Option<PathBuf>,
    /// How long messages are retained (seconds)
    pub cache_duration_secs: u64,
    pub message_size_limit: usize,
    pub request_limit_burst: u32,
    pub request_limit_replenish_secs: u64,
    pub subscription_limit: u32,
    pub keepalive_secs: u64,
    pub manager_interval_secs: u64,
    /// Auth is active only when auth_file is set.
    pub auth_enabled: bool,
    pub auth_file: Option<PathBuf>,
    pub default_access: DefaultAccess,
}

impl Config {
    /// Build a resolved Config by merging file config with CLI overrides.
    pub fn resolve(file: FileConfig, cli: &Cli) -> Self {
        let listen_http = cli
            .listen_http
            .clone()
            .or(file.listen_http)
            .unwrap_or_else(|| DEFAULT_LISTEN_HTTP.to_string());

        let base_url = cli
            .base_url
            .clone()
            .or(file.base_url)
            .unwrap_or_default();

        let cache_file = cli.cache_file.clone().or(file.cache_file);

        let auth_file = file.auth_file;
        let auth_enabled = auth_file.is_some();

        Config {
            listen_http,
            base_url,
            cache_file,
            cache_duration_secs: file
                .cache_duration
                .unwrap_or(DEFAULT_CACHE_DURATION_SECS),
            message_size_limit: file
                .message_size_limit
                .unwrap_or(DEFAULT_MESSAGE_SIZE_LIMIT),
            request_limit_burst: file
                .request_limit_burst
                .unwrap_or(DEFAULT_REQUEST_LIMIT_BURST),
            request_limit_replenish_secs: file
                .request_limit_replenish
                .unwrap_or(DEFAULT_REQUEST_LIMIT_REPLENISH_SECS),
            subscription_limit: file
                .subscription_limit
                .unwrap_or(DEFAULT_SUBSCRIPTION_LIMIT),
            keepalive_secs: file
                .keepalive_interval
                .unwrap_or(DEFAULT_KEEPALIVE_SECS),
            manager_interval_secs: file
                .manager_interval
                .unwrap_or(DEFAULT_MANAGER_INTERVAL_SECS),
            auth_enabled,
            auth_file,
            default_access: file.default_access.unwrap_or_default(),
        }
    }
}

/// Load FileConfig from a TOML file. Missing file is not an error — returns defaults.
pub fn load_file_config(path: &PathBuf) -> anyhow::Result<FileConfig> {
    if !path.exists() {
        return Ok(FileConfig::default());
    }
    let text = std::fs::read_to_string(path)?;
    let cfg: FileConfig = toml::from_str(&text)?;
    Ok(cfg)
}
