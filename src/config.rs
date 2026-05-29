use clap::{Parser, Subcommand};
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
pub const DEFAULT_ATTACHMENT_FILE_SIZE_LIMIT: u64  = 15 * 1024 * 1024;        // 15 MiB
pub const DEFAULT_ATTACHMENT_TOTAL_SIZE_LIMIT: u64 = 5 * 1024 * 1024 * 1024; // 5 GiB
pub const DEFAULT_ATTACHMENT_EXPIRY_SECS: u64      = 3 * 60 * 60;            // 3 hours

/// Top-level CLI. Use `ntfy-rs serve` to start the server.
#[derive(Parser, Debug)]
#[command(name = "ntfy-rs", about = "Lightweight pub/sub notification server")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the ntfy-rs server
    Serve(ServeArgs),
}

/// Arguments for the `serve` subcommand. Values here override the config file.
#[derive(Parser, Debug)]
pub struct ServeArgs {
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

    /// HTTPS listen address, e.g. ":443"
    #[arg(long, env = "NTFY_LISTEN_HTTPS")]
    pub listen_https: Option<String>,

    /// Path to PEM TLS certificate file
    #[arg(long, env = "NTFY_CERT_FILE")]
    pub cert_file: Option<PathBuf>,

    /// Path to PEM TLS private key file
    #[arg(long, env = "NTFY_KEY_FILE")]
    pub key_file: Option<PathBuf>,

    /// Unix domain socket path
    #[arg(long, env = "NTFY_LISTEN_UNIX")]
    pub listen_unix: Option<PathBuf>,

    /// Upstream ntfy server for iOS poll-forward (e.g. https://ntfy.sh)
    #[arg(long, env = "NTFY_UPSTREAM_BASE_URL")]
    pub upstream_base_url: Option<String>,

    /// Bearer token for the upstream server
    #[arg(long, env = "NTFY_UPSTREAM_ACCESS_TOKEN")]
    pub upstream_access_token: Option<String>,
}

/// File-based config (TOML). All fields are optional; defaults apply when absent.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
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

    /// Upstream ntfy server for iOS poll-forward (e.g. "https://ntfy.sh").
    /// When absent, upstream forwarding is disabled.
    pub upstream_base_url: Option<String>,
    /// Optional Bearer token for the upstream server.
    pub upstream_access_token: Option<String>,

    /// Maximum delay for scheduled messages (seconds). Default: 3 days.
    pub max_delay_secs: Option<u64>,

    /// Directory for storing attachment files. Attachments are disabled when absent.
    pub attachment_cache_dir: Option<PathBuf>,
    /// Maximum size of a single uploaded file (bytes). Default: 15 MiB.
    pub attachment_file_size_limit: Option<u64>,
    /// Maximum total storage used by all attachments (bytes). Default: 5 GiB.
    pub attachment_total_size_limit: Option<u64>,
    /// How long attachment files are retained (seconds). Default: 3 hours.
    pub attachment_expiry_duration: Option<u64>,

    /// HTTPS listen address (e.g. ":443"). Requires cert_file + key_file.
    pub listen_https: Option<String>,
    /// Path to PEM-encoded TLS certificate (or full chain).
    pub cert_file: Option<PathBuf>,
    /// Path to PEM-encoded TLS private key.
    pub key_file: Option<PathBuf>,

    /// Unix domain socket path. When set, the server also listens on this socket.
    pub listen_unix: Option<PathBuf>,

    // ── Outbound email (SMTP) ─────────────────────────────────────────────
    /// SMTP server hostname (e.g. "smtp.gmail.com"). Email is disabled when absent.
    pub smtp_host: Option<String>,
    /// SMTP server port. Default: 587 (STARTTLS).
    pub smtp_port: Option<u16>,
    /// SMTP login username.
    pub smtp_username: Option<String>,
    /// SMTP login password (plaintext convenience — prefer smtp_password_file or
    /// the NTFY_SMTP_PASSWORD env var for anything security-sensitive).
    pub smtp_password: Option<String>,
    /// Path to a file containing the SMTP password (takes precedence over smtp_password).
    pub smtp_password_file: Option<PathBuf>,
    /// "From" address, e.g. "ntfy-rs <you@example.com>".
    pub smtp_from: Option<String>,
    /// Recipient addresses — every published message is emailed to all of them.
    pub smtp_to: Option<Vec<String>>,
    /// Only send email for messages with priority >= this value (1–5). 0 = send all.
    pub smtp_min_priority: Option<u8>,
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

    /// Upstream ntfy server for iOS poll-forward. None = disabled.
    pub upstream_base_url: Option<String>,
    pub upstream_access_token: Option<String>,

    /// Maximum allowed delay for scheduled messages (seconds).
    pub max_delay_secs: u64,

    /// HTTPS listen address. None = TLS disabled.
    pub listen_https: Option<String>,
    /// PEM certificate file path.
    pub cert_file: Option<PathBuf>,
    /// PEM private key file path.
    pub key_file: Option<PathBuf>,

    /// Unix domain socket path. None = disabled.
    pub listen_unix: Option<PathBuf>,

    /// Directory for attachment files. None = attachments disabled.
    pub attachment_cache_dir: Option<PathBuf>,
    /// Per-file size limit (bytes).
    pub attachment_file_size_limit: u64,
    /// Total storage limit across all attachments (bytes).
    pub attachment_total_size_limit: u64,
    /// Attachment retention period (seconds).
    pub attachment_expiry_secs: u64,

    // ── Outbound email (SMTP) ─────────────────────────────────────────────
    /// Resolved SMTP config. None = email disabled.
    pub smtp: Option<SmtpConfig>,
}

/// Fully resolved SMTP settings (present only when email is enabled).
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    /// Password resolved via env var > password_file > config field.
    pub password: String,
    pub from: String,
    pub to: Vec<String>,
    /// Minimum priority to trigger an email (0 = send for all priorities).
    pub min_priority: u8,
}

impl Config {
    /// Build a resolved Config by merging file config with CLI overrides.
    pub fn resolve(file: FileConfig, cli: &ServeArgs) -> Self {
        // Resolve smtp first — borrows `file` before any fields are moved out.
        let smtp = resolve_smtp(&file);

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

        let auth_file = file.auth_file.clone();
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
            upstream_base_url: cli.upstream_base_url.clone().or(file.upstream_base_url),
            upstream_access_token: cli.upstream_access_token.clone().or(file.upstream_access_token),
            max_delay_secs: file.max_delay_secs.unwrap_or(3 * 24 * 60 * 60), // 3 days
            listen_https: cli.listen_https.clone().or(file.listen_https),
            cert_file: cli.cert_file.clone().or(file.cert_file),
            key_file: cli.key_file.clone().or(file.key_file),
            listen_unix: cli.listen_unix.clone().or(file.listen_unix),
            attachment_cache_dir: file.attachment_cache_dir,
            attachment_file_size_limit: file.attachment_file_size_limit
                .unwrap_or(DEFAULT_ATTACHMENT_FILE_SIZE_LIMIT),
            attachment_total_size_limit: file.attachment_total_size_limit
                .unwrap_or(DEFAULT_ATTACHMENT_TOTAL_SIZE_LIMIT),
            attachment_expiry_secs: file.attachment_expiry_duration
                .unwrap_or(DEFAULT_ATTACHMENT_EXPIRY_SECS),
            smtp,
        }
    }
}

/// Resolve SMTP config from FileConfig, applying the password priority:
/// `NTFY_SMTP_PASSWORD` env var > `smtp_password_file` > `smtp_password` field.
/// Returns None if smtp_host or smtp_to are absent (email disabled).
fn resolve_smtp(file: &FileConfig) -> Option<SmtpConfig> {
    let host = file.smtp_host.clone()?;
    let to = file.smtp_to.clone().filter(|v| !v.is_empty())?;

    let password = if let Ok(p) = std::env::var("NTFY_SMTP_PASSWORD") {
        p
    } else if let Some(ref path) = file.smtp_password_file {
        std::fs::read_to_string(path)
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    } else {
        file.smtp_password.clone().unwrap_or_default()
    };

    Some(SmtpConfig {
        host,
        port: file.smtp_port.unwrap_or(587),
        username: file.smtp_username.clone().unwrap_or_default(),
        password,
        from: file.smtp_from.clone().unwrap_or_else(|| "ntfy-rs".to_string()),
        to,
        min_priority: file.smtp_min_priority.unwrap_or(0),
    })
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
