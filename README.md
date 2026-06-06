<div align="center">
  <img src="assets/ntfy-rs_logo.png" alt="ntfy-rs logo" width="256">

  [![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20GPL--2.0-blue.svg)](LICENSE)
  [![CI](https://github.com/Arcturus808/ntfy-rs/actions/workflows/release.yml/badge.svg)](https://github.com/Arcturus808/ntfy-rs/actions/workflows/release.yml)
  [![Release](https://img.shields.io/github/v/tag/Arcturus808/ntfy-rs?label=release&color=green)](https://github.com/Arcturus808/ntfy-rs/releases)
  [![Sponsor](https://img.shields.io/badge/sponsor-GitHub-blueviolet)](https://github.com/sponsors/Arcturus808)
  [![Coverage](https://codecov.io/gh/Arcturus808/ntfy-rs/graph/badge.svg?token=7SVEQLL1A5)](https://codecov.io/gh/Arcturus808/ntfy-rs)
</div>

# ntfy-rs

A Rust implementation of the [ntfy](https://ntfy.sh) pub/sub notification server. Wire-compatible with existing ntfy clients (Android, iOS, web, CLI).

No cgo, no system SQLite dependency, no Firebase requirement. Single static binary.

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
- Web Push notifications (VAPID + RFC 8188 AES-128-GCM, pure Rust — no OpenSSL)

## Feature flags

ntfy-rs uses Cargo feature flags to allow consumers to disable unused functionality and reduce binary size. All features are enabled by default; disable them with `default-features = false` and enable only what you need.

| Feature | Default | Description | Key dependencies removed when disabled |
|---|---|---|---|
| `email` | yes | Outbound SMTP email notifications | `lettre` (+16 transitive deps) |
| `metrics` | yes | Prometheus metrics exposition (`/metrics`) | `metrics`, `metrics-exporter-prometheus` |
| `tls` | yes | HTTPS listener (rustls + aws-lc-rs) | `axum-server`, `rustls` |
| `webpush` | yes | Web Push notifications (VAPID/ECE) | `p256`, `aes-gcm`, `hkdf` |
| `auth` | yes | Authentication: Basic, Bearer, ACL | `bcrypt` |
| `unix-socket` | yes | Unix domain socket listener | `hyper`, `hyper-util` |
| `config-file` | yes | TOML config file + CLI arg parsing | `clap`, `config` |

**Note:** SQLite (`rusqlite`, `r2d2`, `r2d2_sqlite`) is always required — it is deeply embedded in the server's publish/subscribe/manager pipeline.

### Minimal embedded build (phone notifications only)

For an embedded use case like the DE-5000 Tauri app that only needs LAN phone notifications (no email, no metrics, no TLS, no web push, no auth, no Unix socket, no config file):

```toml
[dependencies]
ntfy-rs = { git = "...", default-features = false }
```

This removes ~10–15 MB of compiled dependencies from the final binary.

### Standalone server build

The standalone `ntfy-rs` binary requires the `config-file` feature (for CLI arg parsing). Building with `cargo build --release` uses all default features automatically.

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

> **Windows AV note:** the release binary uses [aws-lc-rs](https://github.com/aws/aws-lc-rs) as the crypto backend for all TLS operations across the entire dependency tree. It relies only on documented Windows APIs (`BCryptGenRandom`). The `ring` crate, which calls the undocumented `SystemFunction036` (`RtlGenRandom`) and can trigger false positives in behaviour-based AV scanners, is not present in the binary.

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

#### Public server (with real domain and Let's Encrypt)

```toml
# /etc/ntfy-rs/server.toml

listen_http  = ":2586"
listen_https = ":443"          # optional; requires cert_file + key_file
listen_unix  = "/run/ntfy-rs/ntfy-rs.sock"  # optional

# The public URL clients use to reach this server.
# MUST include the port number — the iOS app includes the port when
# registering with ntfy.sh for APNs, and the upstream poll-forward
# hashes the full URL. A mismatch means lock screen notifications
# will not arrive on iOS. (Android is unaffected — it uses a
# persistent WebSocket, not APNs.)
base_url     = "https://ntfy.example.com:443"
cache_file   = "/var/lib/ntfy-rs/cache.db"
attachment_cache_dir = "/var/lib/ntfy-rs/attachments"

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

# iOS upstream poll-forward — required for iOS lock screen notifications.
# Without this, iOS clients only receive notifications while the app is open.
upstream_base_url      = "https://ntfy.sh"
upstream_access_token  = ""    # optional Bearer token for upstream
```

#### Local/LAN server (HTTP only, no TLS)

```toml
# server.toml — LAN-only setup, no TLS

listen_http = ":2586"

# base_url must include the port for iOS lock screen notifications to work.
base_url             = "http://192.168.0.82:2586"
upstream_base_url    = "https://ntfy.sh"
cache_file           = "cache.db"
attachment_cache_dir  = "attachments"
```

#### Local/LAN server (HTTPS with self-signed certificate)

See the [TLS → With a self-signed certificate](#with-a-self-signed-certificate-locallan-only) section for the full step-by-step guide.

```toml
# server.toml — LAN with self-signed HTTPS

listen_http  = ":2586"
listen_https = ":443"

base_url             = "https://192.168.0.82:443"   # must include port
upstream_base_url    = "https://ntfy.sh"
cache_file           = "cache.db"
attachment_cache_dir  = "attachments"

cert_file = "server-fullchain.crt"   # server cert + CA cert
key_file  = "server.key"
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

# Upload a file attachment
curl -H "Title: Photo" -H "X-Filename: photo.jpg" \
     -H "Content-Type: image/jpeg" \
     --data-binary @photo.jpg \
     ntfy.example.com/mytopic

# External attachment URL (no upload)
curl -H "Title: Image test" -H "Attach: https://ntfy.sh/static/img/ntfy.png" \
     -d "Check this out" ntfy.example.com/mytopic

# Action buttons
curl -H "Title: Server down" \
     -H "Actions: view, Open dashboard, https://grafana.example.com; http, Restart, https://api.example.com/restart, method=POST, clear=true" \
     -d "CPU at 100%" ntfy.example.com/mytopic
```

### Publish headers

| Header | Aliases | Description |
|---|---|---|
| `X-Title` | `Title`, `t` | Message title |
| `X-Priority` | `Priority`, `prio`, `p` | `1`/`min` · `2`/`low` · `3`/`default` · `4`/`high` · `5`/`urgent` |
| `X-Tags` | `Tags`, `tag`, `ta` | Comma-separated emoji shortcodes (auto-resolved to unicode, e.g. `white_check_mark` → ✅) |
| `X-Click` | `Click` | URL to open on click |
| `X-Icon` | `Icon` | Icon URL |
| `X-Markdown` | `Markdown`, `md` | `1` to render body as Markdown |
| `X-Actions` | `Actions`, `action` | Action buttons (see SPEC.md for format) |
| `X-Encoding` | `Encoding`, `enc`, `e` | `base64` to send a binary body |
| `X-Attach` | `Attach` | External attachment URL (no file upload) |
| `X-Filename` | `Filename` | Filename for file attachment upload |
| `X-Delay` | `Delay`, `X-At`, `At`, `X-In`, `In` | Scheduled delivery time |
| `Content-Type` | | `text/markdown` sets Markdown rendering; non-text type triggers attachment upload |

> **iOS note:** The ntfy iOS app (as of v1.6) does not render attachment image previews in notifications or in-app. The Android app shows a download link but not an inline preview. Attachments are served correctly by ntfy-rs; image display is an app-side feature that has not shipped yet. Action buttons (`X-Actions`) render in-app on both platforms but not on the iOS lock screen or Notification Center — only Android shows them in notifications.

## Subscribing

### API

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

### Mobile apps

The subscription URL in the ntfy app **must match `base_url`** (including the port) for iOS lock screen notifications to work. The APNs hash is derived from the full URL, so a mismatch means the app won't be woken by push notifications.

| Scenario | Server `base_url` | App subscription URL | Extra steps |
|---|---|---|---|
| **Public server, real domain** | `https://ntfy.example.com:443` | `https://ntfy.example.com:443` | None — real cert is trusted automatically |
| **LAN, HTTP only** | `http://192.168.0.82:2586` | `http://192.168.0.82:2586` | None — simplest setup |
| **LAN, HTTPS with self-signed cert** | `https://192.168.0.82:443` | `https://192.168.0.82:443` | Install CA cert on every device (see [TLS section](#with-a-self-signed-certificate-locallan-only)) |

> **Tip:** For LAN-only setups, HTTP is the simplest option — no certificates to manage, no CA to install on devices. Everything works including iOS lock screen notifications.

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

### With a real domain (recommended)

Use a CA-signed certificate (e.g., Let's Encrypt). No special client configuration needed — iOS trusts it automatically.

```toml
listen_https = ":443"
cert_file    = "/etc/letsencrypt/live/ntfy.example.com/fullchain.pem"
key_file     = "/etc/letsencrypt/live/ntfy.example.com/privkey.pem"
```

### With a self-signed certificate (local/LAN only)

For servers on a local network without a public domain, you can use a self-signed certificate. This requires installing a custom CA on every iOS device that will connect — it is not practical for large deployments.

> **Tip:** If you don't need encryption on your local network, HTTP works fine for both in-app and lock screen notifications. Self-signed HTTPS is only needed if you want TLS on a LAN without a public domain.

#### Step 1: Generate the CA and server certificate

Run these commands in a terminal (Git Bash on Windows, or any shell on Linux/macOS):

```bash
# --- Create the local CA ---
openssl req -x509 -new -nodes -newkey rsa:2048 \
  -keyout localCA.key -out localCA.crt \
  -days 3650 \
  -subj "/C=US/ST=State/L=Local/O=MyPrivateCA/CN=My Local Root CA" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -addext "subjectKeyIdentifier=hash"

# --- Create the server certificate signing request ---
# Replace 192.168.0.82 with your server's IP address.
# You can also add a DNS name (e.g., DNS:myserver.local) to the SAN below.
openssl req -new -nodes -newkey rsa:2048 \
  -keyout server.key -out server.csr \
  -subj "/CN=192.168.0.82"

# --- Create the extensions config ---
cat > san.ext << 'EOF'
subjectAltName = IP:192.168.0.82
keyUsage = digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
basicConstraints = CA:FALSE
EOF

# --- Sign the server certificate with the CA ---
openssl x509 -req -in server.csr \
  -CA localCA.crt -CAkey localCA.key -CAcreateserial \
  -out server.crt -days 825 -extfile san.ext

# --- Build the fullchain certificate (server cert + CA) ---
cat server.crt localCA.crt > server-fullchain.crt
```

> **Windows (Git Bash):** Prefix the `openssl req` commands with `MSYS_NO_PATHCONV=1` to prevent Git Bash from mangling the `-subj` path argument. For example:
> ```bash
> MSYS_NO_PATHCONV=1 openssl req -x509 -new -nodes -newkey rsa:2048 \
>   -keyout localCA.key -out localCA.crt \
>   -days 3650 \
>   -subj "/C=US/ST=State/L=Local/O=MyPrivateCA/CN=My Local Root CA" \
>   -addext "basicConstraints=critical,CA:TRUE" \
>   -addext "keyUsage=critical,keyCertSign,cRLSign" \
>   -addext "subjectKeyIdentifier=hash"
>
> MSYS_NO_PATHCONV=1 openssl req -new -nodes -newkey rsa:2048 \
>   -keyout server.key -out server.csr \
>   -subj "/CN=192.168.0.82"
> ```

#### Step 2: Configure the server

```toml
listen_https = ":443"
cert_file    = "server-fullchain.crt"   # fullchain, not just server.crt
key_file     = "server.key"

base_url     = "https://192.168.0.82:443"   # must include port
```

#### Step 3: Install the CA on client devices

**iOS:**

1. Transfer `localCA.crt` to the iPhone (AirDrop, email, or host it on the server).
2. Open the file — iOS will prompt to install a configuration profile. Tap **Install** → **Install**.
3. Go to **Settings → General → About → Certificate Trust Settings**. Toggle **full trust** ON for "My Local Root CA".
4. Verify: open `https://192.168.0.82/v1/config` in Safari. It should load without any warning.

**Android:**

1. Transfer `localCA.crt` to the Android device.
2. Go to **Settings → Security → Install from storage** (path varies by device; may be under **Settings → Security & privacy → Encryption & credentials → Install a certificate → CA certificate**).
3. Select the `.crt` file and confirm.
4. Verify: open `https://192.168.0.82/v1/config` in Chrome. It should load without any warning.

> **Note:** On Android 7+, apps that don't explicitly opt in to trusting user-installed CAs will still reject self-signed certificates. The ntfy Android app does trust user CAs, so attachments and subscriptions work after installing the CA.

#### Step 4: Subscribe in the ntfy app

Use the full URL including port: `https://192.168.0.82:443`. The app must connect with the same URL that `base_url` is set to, or lock screen notifications will not work.

---

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

Apple's **Apple Push Notification service (APNs)** is the only way to deliver push notifications to iOS devices when an app is in the background. Unlike Android, which can maintain a persistent connection to any server, iOS apps can only be woken in the background by APNs — and only apps distributed through the App Store can use APNs with their own certificate.

This means a self-hosted ntfy-rs server **cannot wake the iOS ntfy app directly**. The ntfy iOS app is signed by the ntfy.sh developer and registered with APNs under the ntfy.sh certificate. Only ntfy.sh can trigger a wake-up.

ntfy-rs works around this with **upstream poll-forward**:

1. A message is published to your ntfy-rs server.
2. ntfy-rs sends a lightweight wake signal to `upstream_base_url` (ntfy.sh). The topic is hashed (`sha256(base_url + "/" + topic)`) so ntfy.sh never learns the actual topic name or message content.
3. ntfy.sh triggers APNs, which wakes the iOS app.
4. The app polls your ntfy-rs server for the actual message.

**Message content never passes through ntfy.sh** — it only carries the opaque hash and a message ID. This is why `upstream_base_url` and `base_url` must be configured correctly:

- **`base_url`** is required — the topic hash is derived from the full topic URL (`base_url/topic`). Without it the wrong hash is sent and iOS notifications will not arrive.
- **`base_url` must exactly match the URL the iOS app uses to connect, including the port number** — even for default ports like 443 or 80 (e.g. `https://192.168.0.82:443`, not `https://192.168.0.82`). The iOS app includes the port in its registration URL, so a mismatch causes the hash to differ and APNs wake-ups will not reach the device.
- **Android is unaffected** — Android clients maintain a persistent WebSocket to your server and don't use APNs.

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
smtp-host     = "smtp.gmail.com"
smtp-port     = 587                     # optional, default 587 (STARTTLS)
smtp-starttls = true                    # optional, default true. Set false for local testing (e.g. Mailpit)
smtp-username = "you@gmail.com"
smtp-from     = "ntfy-rs <you@gmail.com>"
smtp-to       = ["you@gmail.com", "5551234567@txt.carrier.com"]
smtp-min-priority = 3                   # optional: only email priority >= 3 (default 0 = all)

# Password — choose the most secure option available:
smtp-password      = "app-password"          # least preferred (plaintext in config)
smtp-password-file = "/run/secrets/smtp_pw"  # preferred for Docker / systemd
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

### Production security notes

- **CORS:** ntfy-rs uses permissive CORS by default (all origins allowed). In production, configure your reverse proxy to override CORS headers and restrict allowed origins.
- **Metrics endpoint:** The `/metrics` endpoint is unauthenticated and exposes message counts and topic activity. Restrict access via your reverse proxy or firewall rules.
- **Auth file:** When `auth-file` is set, all publish/subscribe and account endpoints require authentication. Admin endpoints additionally require an admin-role user.

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
| Title, priority, tags (emoji shortcode resolution), click URL, icon | ✅ | ✅ |
| Markdown rendering (`X-Markdown`) | ✅ | ✅ |
| Action buttons (`X-Actions`) | ✅ | ✅ |
| File attachments (local disk storage) | ✅ | ✅ |
| File attachments (external URL, `X-Attach`) | ✅ | ✅ |
| File attachments (S3 / remote storage) | ✅ | ❌ |
| Attachment image preview in iOS app | ❌ | ❌ |
| Base64-encoded binary body | ✅ | ✅ |

#### Authentication & authorization

| Feature | ntfy (Go) | ntfy-rs (Rust) |
|---|:---:|:---:|
| Basic auth & Bearer token | ✅ | ✅ |
| Per-topic ACL | ✅ | ✅ |
| Anonymous access tiers (read-write / read-only / deny-all) | ✅ | ✅ |
| In-memory ACL cache (`auth-access-cache`) | ✅ | ❌ |
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
| SQLite message cache | ✅ (cgo + system lib) | ✅ (bundled, no cgo) |
| Config file | ✅ (YAML) | ✅ (TOML) |
| Environment variables | ✅ | ✅ |
| CLI flags | ✅ | ✅ |
| `--version` flag | ✅ | ✅ |
| Prometheus metrics endpoint | ✅ | ✅ |
| Web app (React SPA) | ✅ | ❌ |
| Embeddable library | ❌ | ✅ |
| Native Windows binary (no cgo) | ❌ | ✅ |
| Default port | `:80` (requires root/cap) | `:2586` (unprivileged) |
| Binary size (release, uncompressed) | ~21 MB | ~5–8 MB |

#### Push integrations

| Feature | ntfy (Go) | ntfy-rs (Rust) |
|---|:---:|:---:|
| iOS upstream poll-forward (APNs via ntfy.sh) | ✅ | ✅ |
| UnifiedPush / Matrix Push Gateway | ✅ | ✅ |
| Firebase Cloud Messaging (FCM) | ✅ (optional) | ❌ |
| Web Push / VAPID | ✅ | ✅ |
| SMTP ingress (publish via email) | ✅ | ❌ |
| Email notifications (outbound) | ✅ | ✅ |
| Phone call notifications (Twilio) | ✅ | ❌ |
| Stripe billing / usage tiers | ✅ (ntfy.sh only) | ❌ |
| PostgreSQL | supported | not yet |

## Non-goals

Some features present in ntfy (Go) are intentionally absent and are unlikely to be added:

**Firebase Cloud Messaging (FCM)**
FCM requires a Google account, a project API key, and sending notifications through Google's servers. ntfy-rs is designed for self-hosted deployments where avoiding proprietary third-party infrastructure is the point. Android clients using [UnifiedPush](https://unifiedpush.org/) work without FCM.

**React web app**
The upstream ntfy.sh web app is a separate React SPA that works against any wire-compatible server, including ntfy-rs. Bundling it into the server binary would add significant build complexity for something that is already available and maintained upstream.

**Twilio voice calls and Stripe billing**
These are third-party paid services integrated into the ntfy.sh hosted offering. They have no role in a self-hosted server binary.

---

Features listed as "not yet" in the table above (PostgreSQL, SMTP ingress) are planned but not yet implemented.

## License

Dual-licensed under [Apache-2.0](LICENSE.Apache-2.0) and [GPL-2.0](LICENSE.GPL-2.0). You may use this software under the terms of either license, at your option.

This project is not affiliated with, endorsed by, or connected to the Rust Project or the Rust Foundation.

ntfy-rs is an independent reimplementation and is not affiliated with or endorsed by the original [ntfy](https://ntfy.sh) project (binwiederhier/ntfy).
