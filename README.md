# ntfy-rs

A Rust implementation of the [ntfy](https://ntfy.sh) pub/sub notification server. Wire-compatible with existing ntfy clients (Android, iOS, web, CLI).

No CGO, no system SQLite dependency, no Firebase requirement. Single static binary.

## Features

- Publish messages via HTTP PUT/POST
- Subscribe via NDJSON stream, SSE, or WebSocket
- Multi-topic subscriptions (`/topic1,topic2/json`)
- Scheduled/delayed message delivery (`X-Delay`, `X-At`, `X-In` headers)
- SQLite message cache (bundled, no system dep)
- Optional authentication: Basic, Bearer token, ACL per topic
- User management API (self-service + admin)
- TLS via rustls + aws-lc-rs (no OpenSSL, no ring)
- Unix domain socket listener
- iOS upstream poll-forward
- UnifiedPush / Matrix Push Gateway relay

## Build

### Linux / macOS

```bash
cargo build --release
# output: target/release/ntfy-rs
```

### Windows (native)

1. Install Rust from [rustup.rs](https://rustup.rs). Accept the defaults.

2. Install the C++ build tools — required to compile the bundled SQLite:
   - Download [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
   - Select the **"Desktop development with C++"** workload and install

3. Open a new terminal (PowerShell or Command Prompt) and build:

```powershell
cargo build --release
# output: target\release\ntfy-rs.exe
```

The first build takes a few minutes (SQLite is compiled from source). Subsequent builds are fast.

> **Troubleshooting**
> - `linker 'link.exe' not found` — the C++ build tools are not on your PATH. Re-open the terminal after installing them, or use the "x64 Native Tools Command Prompt" shortcut installed with Visual Studio.
> - `cargo not found` — close and reopen the terminal after installing rustup; it modifies `PATH` but the current session won't see the change.

> **Note:** the Unix domain socket listener is disabled on Windows (`listen_unix` has no effect). All other features work normally.

> **Windows AV note:** the release binary uses [aws-lc-rs](https://github.com/aws/aws-lc-rs) as the rustls crypto backend, which relies only on documented Windows APIs (`BCryptGenRandom`). Earlier builds used `ring`, which called the undocumented `SystemFunction036` (`RtlGenRandom`) and occasionally triggered false positives in behaviour-based AV scanners.

### Cross-compiling Windows binary from Linux

```bash
# With cross (requires Docker)
cargo install cross
cross build --release --target x86_64-pc-windows-gnu

# With MinGW (no Docker)
sudo apt install gcc-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
  cargo build --release --target x86_64-pc-windows-gnu
```

## Library usage

ntfy-rs can be used as a library crate embedded in other Rust applications. The server runs as a background thread with graceful shutdown support.

```rust
use ntfy_rs::{start, ServerHandle};
use ntfy_rs::config::{Config, ServeArgs};

// Build config
let args = ServeArgs {
    listen_http: Some(":8090".to_string()),
    base_url: Some("http://192.168.0.82:8090".to_string()),
    upstream_base_url: Some("https://ntfy.sh".to_string()),
    ..Default::default()
};
let config = Config::resolve(args)?;

// Start server on a background thread
let handle: ServerHandle = start(config)?;

// Publish a notification
handle.publish("mytopic", "Title", "Hello from Rust", "high")?;

// Graceful shutdown
handle.shutdown();
```

### `ServerHandle` API

| Method | Description |
|---|---|
| `publish(topic, title, message, priority)` | Send a notification via HTTP POST to the local server |
| `shutdown()` | Signal the server to stop and wait for it to finish |

The CLI binary (`ntfy-rs serve ...`) is a thin wrapper around this same library API.

## Quick start

```bash
# In-memory cache, no auth, port 2586 (see "Default port" below)
ntfy-rs serve

# Persistent cache
ntfy-rs serve --cache-file /var/lib/ntfy-rs/cache.db

# With config file
ntfy-rs serve --config /etc/ntfy-rs/server.toml
```

### Windows Firewall

On Windows, the server requires inbound TCP access on its port. On first launch, Windows Defender Firewall will show a prompt:

> "Windows Defender Firewall has blocked some features of this app"

- **Check "Private networks"** (your home/work LAN)
- **Uncheck "Public networks"** (coffee shops, hotels — no need to expose ntfy there)
- Click **Allow**

If you accidentally deny or miss the prompt, run these commands in elevated PowerShell:

```powershell
# Ensure your network is classified as Private
Set-NetConnectionProfile -Name "<NetworkName>" -NetworkCategory Private

# Remove any existing ntfy firewall rules (optional cleanup)
Get-NetFirewallRule | Where-Object { $_.DisplayName -like "*ntfy*" } | Remove-NetFirewallRule

# Add firewall rule
New-NetFirewallRule -DisplayName "ntfy-rs Server" -Direction Inbound -Protocol TCP -LocalPort 2586 -Action Allow -Profile Private
```

If you change the port, update the firewall rule to match.

## Configuration

All settings can be provided via config file (TOML), CLI flag, or `NTFY_*` environment variable. CLI flags override the config file.

### Config file

```toml
# /etc/ntfy-rs/server.toml

listen_http  = ":2586"
listen_https = ":443"          # optional; requires cert_file + key_file
listen_unix  = "/run/ntfy-rs/ntfy-rs.sock"  # optional

base_url     = "https://ntfy.example.com"
cache_file   = "/var/lib/ntfy-rs/cache.db"

# How long messages are retained (seconds). Default: 43200 (12 hours)
cache_duration = 43200

# Maximum message body size (bytes). Default: 4096
message_size_limit = 4096

# TLS
cert_file = "/etc/letsencrypt/live/ntfy.example.com/fullchain.pem"
key_file  = "/etc/letsencrypt/live/ntfy.example.com/privkey.pem"

# Auth (optional). When set, auth is enabled.
auth_file      = "/var/lib/ntfy-rs/auth.db"
default_access = "read-write"  # read-write | read-only | deny-all

# Rate limiting
request_limit_burst      = 60   # requests allowed in a burst
request_limit_replenish  = 5    # seconds to replenish one token

# Subscription limit per IP
subscription_limit = 30

# Keepalive interval for SSE/WS connections (seconds). Default: 45
keepalive_interval = 45

# Background manager interval (seconds). Default: 180
# Controls delayed message delivery granularity.
manager_interval = 180

# Delayed message maximum (seconds). Default: 259200 (3 days)
max_delay_secs = 259200

# iOS upstream poll-forward
upstream_base_url      = "https://ntfy.sh"
upstream_access_token  = ""    # optional Bearer token for upstream
```

### Environment variables

Every config key maps to `NTFY_<KEY>` in uppercase with underscores:

| Variable | Example |
|---|---|
| `NTFY_LISTEN_HTTP` | `:2586` |
| `NTFY_LISTEN_HTTPS` | `:443` |
| `NTFY_LISTEN_UNIX` | `/run/ntfy-rs/ntfy.sock` |
| `NTFY_BASE_URL` | `https://ntfy.example.com` |
| `NTFY_CACHE_FILE` | `/var/lib/ntfy-rs/cache.db` |
| `NTFY_AUTH_FILE` | `/var/lib/ntfy-rs/auth.db` |
| `NTFY_LOG_LEVEL` | `info` |
| `RUST_LOG` | `ntfy_rs=debug` |

## Publishing messages

```bash
# Minimal
curl -d "Hello" ntfy.example.com/mytopic

# With headers
curl -H "Title: Deployment done" \
     -H "Priority: high" \
     -H "Tags: white_check_mark" \
     -d "Server restarted" \
     ntfy.example.com/mytopic

# Delayed delivery (30 minutes from now)
curl -H "Delay: 30m" -d "Reminder" ntfy.example.com/mytopic

# Delay formats: 30s, 5m, 2h, 1d, Unix timestamp, RFC 3339
```

### Publish headers

| Header | Aliases | Description |
|---|---|---|
| `X-Title` | `Title`, `t` | Message title |
| `X-Priority` | `Priority`, `prio`, `p` | `1`/`min` · `2`/`low` · `3`/`default` · `4`/`high` · `5`/`urgent` |
| `X-Tags` | `Tags`, `tag`, `ta` | Comma-separated tags |
| `X-Click` | `Click` | URL to open on click |
| `X-Icon` | `Icon` | Icon URL |
| `X-Markdown` | `Markdown`, `md` | `1` to render body as Markdown |
| `X-Actions` | `Actions`, `action` | Action buttons (see SPEC.md for format) |
| `X-Encoding` | `Encoding`, `enc`, `e` | `base64` to send a binary body |
| `X-Filename` | `Filename` | Filename for file attachment upload |
| `X-Delay` | `Delay`, `X-At`, `At`, `X-In`, `In` | Scheduled delivery time |
| `Content-Type` | | `text/markdown` sets Markdown rendering; non-text type triggers attachment upload |

## Subscribing

```bash
# NDJSON stream (primary — used by ntfy clients)
curl -s ntfy.example.com/mytopic/json

# Poll (return cached messages and exit)
curl -s "ntfy.example.com/mytopic/json?poll=1"

# Since a specific time
curl -s "ntfy.example.com/mytopic/json?since=1712345678"

# All cached messages
curl -s "ntfy.example.com/mytopic/json?since=all"

# SSE (browser EventSource)
curl -s ntfy.example.com/mytopic/sse

# Multiple topics
curl -s ntfy.example.com/topic1,topic2/json

# WebSocket (used by ntfy Android app)
# ws://ntfy.example.com/mytopic/ws
```

## Authentication

Auth is disabled by default. Set `auth_file` to enable it.

```toml
auth_file      = "/var/lib/ntfy-rs/auth.db"
default_access = "deny-all"
```

### Bootstrap the first admin

```bash
# Register via API
curl -X POST ntfy.example.com/v1/account \
  -d '{"username":"admin","password":"secret"}'

# Promote to admin directly in the DB
sqlite3 /var/lib/ntfy-rs/auth.db \
  "UPDATE users SET role='admin' WHERE username='admin';"
```

### Account API

```bash
# Register
curl -X POST ntfy.example.com/v1/account \
  -d '{"username":"alice","password":"pass"}'

# Get own account info
curl -u alice:pass ntfy.example.com/v1/account

# Change password
curl -u alice:pass -X PUT ntfy.example.com/v1/account/password \
  -d '{"password":"newpass"}'

# Create Bearer token
curl -u alice:pass -X POST ntfy.example.com/v1/account/token \
  -d '{"label":"my-app","expires":1800000000}'

# Revoke token
curl -u alice:pass -X DELETE ntfy.example.com/v1/account/token/tk_...

# Grant topic access
curl -u alice:pass -X POST ntfy.example.com/v1/account/access \
  -d '{"topic":"mytopic","read":true,"write":true}'

# Delete own account
curl -u alice:pass -X DELETE ntfy.example.com/v1/account
```

### Admin API

```bash
# List all users
curl -u admin:secret ntfy.example.com/v1/admin/users

# Create user
curl -u admin:secret -X POST ntfy.example.com/v1/admin/users \
  -d '{"username":"bob","password":"pass","role":"user"}'

# Change role
curl -u admin:secret -X PUT ntfy.example.com/v1/admin/users/bob/role \
  -d '{"role":"admin"}'

# Set ACL for user
curl -u admin:secret -X POST ntfy.example.com/v1/admin/users/bob/access \
  -d '{"topic":"alerts","read":true,"write":false}'

# Delete user
curl -u admin:secret -X DELETE ntfy.example.com/v1/admin/users/bob
```

## TLS

```toml
listen_https = ":443"
cert_file    = "/etc/letsencrypt/live/ntfy.example.com/fullchain.pem"
key_file     = "/etc/letsencrypt/live/ntfy.example.com/privkey.pem"
```

If `listen_https` is set but `cert_file`/`key_file` are missing, a warning is logged and the server continues on HTTP only. Certificate hot-reload is not supported; restart the server to pick up a new cert.

## UnifiedPush / Matrix gateway

ntfy-rs acts as a [Matrix Push Gateway](https://spec.matrix.org/v1.2/push-gateway-api/) for UnifiedPush. Set `base_url` and point your Matrix homeserver's pusher URL to `/_matrix/push/v1/notify`.

```bash
# Discovery
curl ntfy.example.com/_matrix/push/v1/notify
# → {"unifiedpush":{"gateway":"matrix"}}
```

The pushkey must be a full ntfy topic URL on this server, e.g. `https://ntfy.example.com/upXXXXXXXX?up=1`. Pushkeys from other servers are returned as rejected per the Matrix spec.

## iOS upstream poll-forward

iOS clients cannot receive push notifications directly from a self-hosted server — APNs requires a trusted intermediary. ntfy-rs solves this by forwarding a lightweight wake signal to ntfy.sh on each publish. ntfy.sh triggers APNs, the iOS app wakes, and polls your server for the actual message. Message content never passes through ntfy.sh.

**`base_url` is required** — the topic hash sent to ntfy.sh is derived from the full topic URL (`base_url/topic`). Without it the wrong hash is sent and iOS notifications will not arrive.

```toml
base_url              = "http://192.168.0.82:2586"
upstream_base_url     = "https://ntfy.sh"
upstream_access_token = ""   # optional; set if you have a ntfy.sh account with higher rate limits
```

Or via CLI flags:

```powershell
.\ntfy-rs.exe serve --listen-http :2586 --base-url http://192.168.0.82:2586 --upstream-base-url https://ntfy.sh
```

## Outbound email notifications

ntfy-rs can email you (or any address) whenever a message is published. Useful for alerting, SMS gateways, or as a fallback channel.

```toml
smtp_host     = "smtp.gmail.com"
smtp_port     = 587                     # optional, default 587 (STARTTLS)
smtp_username = "you@gmail.com"
smtp_from     = "ntfy-rs <you@gmail.com>"
smtp_to       = ["you@gmail.com", "5551234567@txt.carrier.com"]
smtp_min_priority = 3                   # optional: only email priority >= 3 (default 0 = all)

# Password — choose the most secure option available:
smtp_password      = "app-password"          # least preferred (plaintext in config)
smtp_password_file = "/run/secrets/smtp_pw"  # preferred for Docker / systemd
# NTFY_SMTP_PASSWORD env var                 # most preferred
```

Email delivery is fire-and-forget — failures are logged but never cause a publish to fail. Email for delayed messages is sent at delivery time, not at scheduling time.

## Logging

```bash
# Log level via flag
ntfy-rs serve --log-level debug

# Per-module filtering via RUST_LOG
RUST_LOG=ntfy_rs=debug,tower_http=warn ntfy-rs serve
```

## Default port

ntfy-rs defaults to **`:2586`** rather than the Go ntfy default of `:80`. The reasons:

- **Privilege-free by default.** On Linux and macOS, binding ports below 1024 requires root or a specific capability (`CAP_NET_BIND_SERVICE`). Using `:80` as the default would cause an immediate `permission denied` error for any user running ntfy-rs as a normal process — the dominant use case for a single-binary server.
- **Windows parity.** On Windows, port 80 is frequently occupied by IIS, HTTP.sys, or other services. `:2586` works out-of-the-box without conflicts.
- **Protocol, not operational, compatibility.** ntfy-rs is wire-compatible with ntfy clients at the API level (message format, headers, routes). It does not aim to match the Go server's deployment assumptions, which are built around managed system packages, systemd units, and elevated privileges.

If you need to serve on port 80 or 443 without TLS termination by a reverse proxy, either run the binary with the required privilege or set the address explicitly:

```toml
# server.toml — match Go ntfy defaults
listen_http  = ":80"
listen_https = ":443"
```

```bash
# Linux: grant binding capability without running as root
sudo setcap 'cap_net_bind_service=+ep' /usr/local/bin/ntfy-rs
```

For production, the recommended approach on all platforms is to run ntfy-rs on its default port behind a reverse proxy (nginx, Caddy, Traefik) that handles TLS and listens on 80/443.

---

## Relation to ntfy (Go)

ntfy-rs is a ground-up Rust reimplementation targeting a smaller binary and zero system dependencies, while maintaining full wire compatibility with ntfy clients. It is not a port of the Go codebase.

### Feature comparison

#### Core messaging

| Feature | ntfy (Go) | ntfy-rs (Rust) |
|---|:---:|:---:|
| HTTP publish (`PUT`/`POST /{topic}`) | ✅ | ✅ |
| NDJSON stream (`/{topic}/json`) | ✅ | ✅ |
| SSE stream (`/{topic}/sse`) | ✅ | ✅ |
| WebSocket (`/{topic}/ws`) | ✅ | ✅ |
| Multi-topic subscriptions | ✅ | ✅ |
| Poll mode (`?poll=1`, `?since=`) | ✅ | ✅ |
| Scheduled / delayed delivery (`X-Delay`) | ✅ | ✅ |
| Title, priority, tags, click URL, icon | ✅ | ✅ |
| Markdown rendering (`X-Markdown`) | ✅ | ✅ |
| Action buttons (`X-Actions`) | ✅ | ✅ |
| File attachments (local disk storage) | ✅ | ✅ |
| File attachments (S3 / remote storage) | ✅ | ❌ |
| Base64-encoded binary body | ✅ | ✅ |

#### Authentication & authorization

| Feature | ntfy (Go) | ntfy-rs (Rust) |
|---|:---:|:---:|
| Basic auth & Bearer token | ✅ | ✅ |
| Per-topic ACL | ✅ | ✅ |
| Anonymous access tiers (read-write / read-only / deny-all) | ✅ | ✅ |
| User self-service API (`/v1/account`) | ✅ | ✅ |
| Admin user management API (`/v1/admin`) | ✅ | ✅ |
| Bearer token creation & revocation | ✅ | ✅ |

#### Transport & infrastructure

| Feature | ntfy (Go) | ntfy-rs (Rust) |
|---|:---:|:---:|
| HTTP listener | ✅ | ✅ |
| HTTPS / TLS (built-in) | ✅ (Go stdlib) | ✅ (rustls + aws-lc-rs) |
| Unix domain socket | ✅ | ✅ (Linux/macOS) |
| Per-IP rate limiting | ✅ | ✅ |
| Per-IP subscription limits | ✅ | ✅ |
| SQLite message cache | ✅ (CGO + system lib) | ✅ (bundled, no CGO) |
| Config file | ✅ (YAML) | ✅ (TOML) |
| Environment variables | ✅ | ✅ |
| CLI flags | ✅ | ✅ |
| Prometheus metrics endpoint | ✅ | ✅ |
| Web app (React SPA) | ✅ | ❌ |
| Embeddable library | ❌ | ✅ |
| Native Windows binary (no CGO) | ❌ | ✅ |
| Default port | `:80` (requires root/cap) | `:2586` (unprivileged) |
| Binary size (release, uncompressed) | ~21 MB | ~5–8 MB |

#### Push integrations

| Feature | ntfy (Go) | ntfy-rs (Rust) |
|---|:---:|:---:|
| iOS upstream poll-forward (APNs via ntfy.sh) | ✅ | ✅ |
| UnifiedPush / Matrix Push Gateway | ✅ | ✅ |
| Firebase Cloud Messaging (FCM) | ✅ (optional) | ❌ |
| Web Push / VAPID | ✅ | ❌ |
| SMTP ingress (publish via email) | ✅ | ❌ |
| Email notifications (outbound) | ✅ | ✅ |
| Phone call notifications (Twilio) | ✅ | ❌ |
| Stripe billing / usage tiers | ✅ (ntfy.sh only) | ❌ |
| PostgreSQL | supported | not yet |
