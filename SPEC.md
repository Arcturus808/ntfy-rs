# ntfy-rs API Specification

Wire-compatible with the [ntfy HTTP API](https://ntfy.sh/docs/publish/). Existing ntfy clients work without modification.

---

## Conventions

- All request and response bodies are JSON unless noted otherwise.
- Timestamps are Unix seconds (integer) unless noted otherwise.
- Authentication is optional. When `auth_file` is not configured, all requests are allowed regardless of credentials.
- Rate limiting applies per source IP. Exceeding the limit returns `429`.

---

## Authentication

When auth is enabled, credentials are read from:

1. `Authorization: Basic <base64(user:pass)>` header
2. `Authorization: Bearer <token>` header
3. `?auth=<base64("Basic " + base64(user:pass))>` query param (WebSocket compat)
4. `Authorization: Basic <base64(:token)>` — empty username, password treated as Bearer token

Anonymous requests are allowed or denied based on `default_access`:

| `default_access` | Anonymous read | Anonymous write |
|---|---|---|
| `read-write` (default) | ✅ | ✅ |
| `read-only` | ✅ | ❌ |
| `deny-all` | ❌ | ❌ |

---

## Error responses

All errors return JSON:

```json
{
  "code":  40001,
  "http":  400,
  "error": "bad request: ...",
  "link":  "https://ntfy.sh/docs/publish/#response-codes"
}
```

| HTTP | Code | Meaning |
|---|---|---|
| 400 | 40001 | Bad request |
| 400 | 40002 | Topic name invalid |
| 401 | 40101 | Unauthorized |
| 403 | 40301 | Forbidden |
| 404 | 40401 | Not found |
| 413 | 41301 | Message too large |
| 429 | 42901 | Too many requests |
| 500 | 50001 | Internal error |

---

## Topic names

- 1–64 characters
- Alphanumeric, `-`, `_` only
- Case-sensitive

---

## Publish

### `PUT /{topic}` · `POST /{topic}`

Publish a message to a topic. Creates the topic if it does not exist.

**Request headers**

| Header | Aliases | Type | Description |
|---|---|---|---|
| `X-Title` | `Title`, `t` | string | Message title |
| `X-Priority` | `Priority`, `prio`, `p` | string/int | Priority: `1`/`min`, `2`/`low`, `3`/`default`, `4`/`high`, `5`/`urgent`/`max` |
| `X-Tags` | `Tags`, `tag`, `ta` | string | Comma-separated tag list |
| `X-Click` | `Click` | string | URL to open when notification is clicked |
| `X-Icon` | `Icon` | string | URL of notification icon |
| `X-Markdown` | `Markdown`, `md` | bool | `1`/`true`/`yes` to render body as Markdown |
| `X-Actions` | `Actions`, `action` | string | Action buttons (see below) |
| `X-Encoding` | `Encoding`, `enc`, `e` | string | `base64` to send a binary body (see below) |
| `X-Filename` | `Filename` | string | Upload request body as a file attachment (see below) |
| `X-Delay` | `Delay`, `X-At`, `At`, `X-In`, `In` | string | Scheduled delivery (see below) |
| `Content-Type` | | string | `text/markdown` sets Markdown rendering; any non-text type triggers attachment upload |

**Delay formats**

| Format | Example | Description |
|---|---|---|
| Duration string | `30s`, `5m`, `2h`, `1d` | Relative to now |
| Unix timestamp | `1712345678` | Absolute delivery time |
| RFC 3339 | `2024-04-05T12:00:00Z` | Absolute delivery time |

Delays in the past are treated as immediate. Delays beyond `max_delay_secs` (default 3 days) return `400`.

**Binary body encoding**

To publish a binary payload, base64-encode it and set `X-Encoding: base64`:

```bash
curl -H "X-Encoding: base64" \
     -d "$(echo -n 'binary data' | base64)" \
     ntfy.example.com/mytopic
```

The server stores and forwards the base64 string as-is. Subscribers see `"encoding": "base64"` in the message JSON and are responsible for decoding. The server never encodes or decodes the body. Only `base64` is accepted; any other value returns `400`.

**File attachments**

To attach a file to a message, send the file bytes as the request body. The server stores the file on disk and embeds a download URL in the message.

An upload is detected when **either** condition is true:
- The `X-Filename` header (or `filename` query param) is present.
- The `Content-Type` is present and is not `text/plain` or `text/markdown`.

Attachments require `attachment_cache_dir` to be set in the server config; requests are rejected with `400` otherwise.

```bash
# Upload an image with an explicit filename
curl -H "X-Filename: screenshot.png" \
     -H "Content-Type: image/png" \
     --data-binary @/path/to/screenshot.png \
     ntfy.example.com/mytopic

# Upload any file — Content-Type triggers attachment detection
curl -H "Content-Type: application/zip" \
     --data-binary @archive.zip \
     ntfy.example.com/mytopic
```

The message response includes an `attachment` object:

```json
{
  "id": "fBUMAXaH0XD3",
  "event": "message",
  "topic": "mytopic",
  "message": "",
  "attachment": {
    "name": "screenshot.png",
    "type": "image/png",
    "size": 54321,
    "expires": 1712399999,
    "url": "https://ntfy.example.com/file/xK8pQrZt2mVw"
  }
}
```

| Field | Type | Description |
|---|---|---|
| `name` | string | Filename (from `X-Filename` header, or `attachment-<id>` if absent) |
| `type` | string | MIME type from `Content-Type` header (or `application/octet-stream`) |
| `size` | integer | File size in bytes |
| `expires` | integer | Unix timestamp after which the file is deleted (default: 3 hours) |
| `url` | string | Download URL: `{base_url}/file/{id}` |

**Config options for attachments:**

| Option | Default | Description |
|---|---|---|
| `attachment_cache_dir` | *(none — attachments disabled)* | Directory where attachment files are stored |
| `attachment_file_size_limit` | `15728640` (15 MiB) | Maximum size of a single uploaded file (bytes) |
| `attachment_total_size_limit` | `5368709120` (5 GiB) | Maximum total storage across all attachments (bytes) |
| `attachment_expiry_duration` | `10800` (3 hours) | How long attachment files are retained (seconds) |

**Action button formats**

The `X-Actions` value is a semicolon-separated list of actions. Each action is a comma-separated list of fields:

```
X-Actions: <type>, <label>[, <url>][, key=value, ...][; ...]
```

| Type | Fields | Description |
|---|---|---|
| `view` | `view, <label>, <url>[, clear=true]` | Open a URL in the browser when tapped |
| `http` | `http, <label>, <url>[, method=POST][, headers.<Name>=<value>][, body=<body>][, clear=true]` | Fire an HTTP request from the client device |
| `broadcast` | `broadcast, <label>[, intent=<intent>][, extras.<key>=<value>][, clear=true]` | Send an Android broadcast intent (Android only) |

Examples:

```
# Single view action
X-Actions: view, Open dashboard, https://example.com/dashboard

# HTTP action that POSTs a restart command
X-Actions: http, Restart, https://example.com/api/restart, method=POST, body={}, clear=true

# HTTP action with a custom request header
X-Actions: http, Approve, https://example.com/approve, method=POST, headers.Authorization=Bearer mytoken

# Multiple actions separated by semicolons
X-Actions: view, Logs, https://example.com/logs; http, Restart, https://example.com/restart, method=POST

# Android broadcast
X-Actions: broadcast, Take photo, intent=io.example.ACTION_CAMERA, extras.cmd=snap, clear=true
```

Unknown action types and malformed entries are silently skipped. `clear=true` causes the notification to be dismissed on the device after the action fires.

**Important:** `http` and `broadcast` actions are executed entirely by the client app. The server is a dumb carrier — it does not proxy or execute HTTP requests.

**Request body**

Plain text message body. Maximum size: `message_size_limit` (default 4096 bytes).

**Response** `200 OK`

```json
{
  "id":           "fBUMAXaH0XD3",
  "time":         1712345678,
  "expires":      1712388878,
  "event":        "message",
  "topic":        "mytopic",
  "title":        "Hello",
  "message":      "World",
  "priority":     4,
  "tags":         ["tag1", "tag2"],
  "click":        "https://example.com",
  "icon":         "https://example.com/icon.png",
  "content_type": "text/markdown"
}
```

Fields with zero/empty values are omitted. `expires` is the Unix timestamp after which the message is removed from cache.

For delayed messages, `time` in the response reflects the publish time, not the delivery time.

---

## Subscribe

All subscribe endpoints accept the same query parameters:

| Parameter | Description |
|---|---|
| `poll=1` | Return cached messages and close (no streaming) |
| `since=<time>` | Return messages since Unix timestamp; `since=all` returns all cached |
| `since=<id>` | Return messages since message ID (exclusive) |

Multi-topic subscriptions use a comma-separated topic list: `/topic1,topic2/json`.

### `GET /{topics}/json` — NDJSON stream

Primary subscribe endpoint. Used by ntfy Android, iOS, and CLI clients.

Each message is a JSON object on its own line (newline-delimited JSON). The stream begins with an `open` event and includes periodic `keepalive` events.

**Open event**
```json
{"id":"...","time":1712345678,"event":"open","topic":"mytopic"}
```

**Message event**
```json
{"id":"...","time":1712345678,"expires":1712388878,"event":"message","topic":"mytopic","message":"Hello"}
```

**Keepalive event**
```json
{"id":"...","time":1712345678,"event":"keepalive","topic":"mytopic"}
```

### `GET /{topics}/sse` — Server-Sent Events

SSE stream for browser `EventSource`. Same event types as NDJSON, wrapped in SSE framing:

```
event: open
data: {"id":"...","time":...,"event":"open","topic":"mytopic"}

data: {"id":"...","time":...,"event":"message","topic":"mytopic","message":"Hello"}
```

### `GET /{topics}/ws` — WebSocket

WebSocket stream. Used by the ntfy Android app by default. Same JSON message format as NDJSON. Supports `?auth=` query param for authentication (required for WebSocket clients that cannot set headers on the upgrade request).

---

## File attachments

### `GET /file/:id` — download attachment

Download a previously uploaded file attachment. The opaque ID in the URL acts as the access token — anyone who knows the URL can download the file.

**Path parameters**

| Parameter | Description |
|---|---|
| `id` | Opaque attachment ID (12-char alphanumeric) |

**Responses**

| Status | Description |
|---|---|
| `200 OK` | File bytes with `Content-Type` and `Content-Disposition: attachment; filename="<name>"` headers |
| `404 Not Found` | ID does not exist or the attachment has expired |
| `500 Internal Server Error` | Could not read the file from disk |

Expired attachments return `404` immediately, even if the background cleanup task has not yet deleted the file.

---

## Health and stats

### `GET /v1/health`

```json
{ "healthy": true }
```

### `GET /v1/version`

```json
{ "version": "0.1.0", "sha256": "unknown" }
```

### `GET /v1/stats`

```json
{
  "messages":    1234,
  "topics":      42,
  "subscribers": 7
}
```

---

## Account (self-service)

All endpoints except `POST /v1/account` require authentication.

### `POST /v1/account` — register

No authentication required.

**Request**
```json
{ "username": "alice", "password": "secret" }
```

**Response** `200 OK`
```json
{ "username": "alice" }
```

Returns `400` if the username already exists.

### `GET /v1/account` — get own account

**Response** `200 OK`
```json
{
  "username": "alice",
  "role":     "user",
  "tokens": [
    { "token": "tk_abc...", "label": "my-app", "expires": null }
  ],
  "access": [
    { "topic": "mytopic", "read": true, "write": true }
  ]
}
```

### `DELETE /v1/account` — delete own account

Soft-deletes the account. All tokens are cascade-deleted. Returns `200`.

### `PUT /v1/account/password` — change password

**Request**
```json
{ "password": "newpassword" }
```

Returns `200`.

### `POST /v1/account/token` — create token

**Request**
```json
{
  "label":   "my-app",
  "expires": 1800000000
}
```

`expires` is an optional Unix timestamp. Omit for a non-expiring token.

**Response** `200 OK`
```json
{
  "token":   "tk_abc...",
  "label":   "my-app",
  "expires": 1800000000
}
```

### `DELETE /v1/account/token/:token` — revoke token

Returns `200`.

### `GET /v1/account/access` — list ACL entries

**Response** `200 OK`
```json
{
  "access": [
    { "topic": "mytopic", "read": true, "write": true }
  ]
}
```

### `POST /v1/account/access` — set topic access

**Request**
```json
{ "topic": "mytopic", "read": true, "write": false }
```

Upserts the ACL entry. Returns `200`.

### `DELETE /v1/account/access/:topic` — remove topic access

Returns `200`.

---

## Admin

All endpoints require `role = admin`. Non-admin authenticated users receive `403`.

### `GET /v1/admin/users` — list users

**Response** `200 OK`
```json
{
  "users": [
    {
      "username": "alice",
      "role":     "user",
      "tokens":   [...],
      "access":   [...]
    }
  ]
}
```

### `POST /v1/admin/users` — create user

**Request**
```json
{
  "username": "bob",
  "password": "secret",
  "role":     "user"
}
```

`role` is `"user"` (default) or `"admin"`. Returns `200` with `{"username":"bob"}`.

### `DELETE /v1/admin/users/:username` — delete user

Soft-deletes the user. Returns `200`. Returns `404` if not found.

### `PUT /v1/admin/users/:username/role` — change role

**Request**
```json
{ "role": "admin" }
```

Returns `200`.

### `POST /v1/admin/users/:username/access` — set ACL for user

**Request**
```json
{ "topic": "alerts", "read": true, "write": false }
```

Returns `200`. Returns `404` if user not found.

### `DELETE /v1/admin/users/:username/access/:topic` — remove ACL entry

Returns `200`. Returns `404` if user not found.

---

## UnifiedPush / Matrix Push Gateway

ntfy-rs implements the [Matrix Push Gateway API](https://spec.matrix.org/v1.2/push-gateway-api/) to act as a UnifiedPush relay. `base_url` must be configured.

### `GET /_matrix/push/v1/notify` — discovery

```json
{ "unifiedpush": { "gateway": "matrix" } }
```

### `POST /_matrix/push/v1/notify` — receive Matrix notification

**Request** (Matrix push gateway format)
```json
{
  "notification": {
    "devices": [
      { "pushkey": "https://ntfy.example.com/upABCDEF?up=1" }
    ],
    "event_id": "$abc123",
    "room_id":  "!xyz:matrix.org"
  }
}
```

The `pushkey` must be a full URL on this server. The raw request body is published as a message to the topic encoded in the pushkey.

**Response** `200 OK`
```json
{ "rejected": [] }
```

If the pushkey does not start with `base_url`, it is returned in `rejected` and the homeserver will stop sending to it:

```json
{ "rejected": ["https://ntfy.sh/someothertopic?up=1"] }
```

---

## Message object

All message events share this structure. Fields with zero/empty values are omitted.

| Field | Type | Description |
|---|---|---|
| `id` | string | 12-character random alphanumeric ID |
| `time` | int | Unix timestamp of publish time |
| `expires` | int | Unix timestamp after which the message is evicted from cache |
| `event` | string | `message`, `open`, or `keepalive` |
| `topic` | string | Topic name |
| `message` | string | Message body |
| `title` | string | Optional title |
| `priority` | int | 1–5; omitted when 0 (default) |
| `tags` | array | String tags |
| `click` | string | Click URL |
| `icon` | string | Icon URL |
| `actions` | array | Action buttons (parsed but not generated by server) |
| `content_type` | string | `text/markdown` when Markdown rendering is set |
| `encoding` | string | Reserved |

---

## Database schema

### Messages (`messages`)

| Column | Type | Description |
|---|---|---|
| `id` | TEXT | Message ID (PK with topic) |
| `sequence_id` | TEXT | Sequence ID (equals `id` when not set) |
| `time` | INTEGER | Publish time (or scheduled delivery time when `published=0`) |
| `expires` | INTEGER | Eviction timestamp |
| `topic` | TEXT | Topic name |
| `message` | TEXT | Body |
| `title` | TEXT | Title |
| `priority` | INTEGER | 1–5 |
| `tags` | TEXT | JSON array |
| `click` | TEXT | Click URL |
| `icon` | TEXT | Icon URL |
| `actions` | TEXT | JSON array |
| `content_type` | TEXT | MIME type |
| `encoding` | TEXT | Encoding |
| `published` | INTEGER | `1` = live, `0` = scheduled (not yet delivered) |

### Users (`users`)

| Column | Type | Description |
|---|---|---|
| `id` | TEXT | User ID (PK) |
| `username` | TEXT | Unique username |
| `hash` | TEXT | bcrypt hash (cost 10) |
| `role` | TEXT | `user` or `admin` |
| `deleted` | INTEGER | Soft-delete flag |

### Tokens (`tokens`)

| Column | Type | Description |
|---|---|---|
| `token` | TEXT | Bearer token string (PK) |
| `user_id` | TEXT | FK → users.id |
| `label` | TEXT | Human-readable label |
| `expires` | INTEGER | Expiry timestamp (NULL = no expiry) |
| `last_access` | INTEGER | Last use timestamp |
| `last_origin` | TEXT | Last use IP |

### ACL (`topic_acl`)

| Column | Type | Description |
|---|---|---|
| `user_id` | TEXT | FK → users.id (PK with topic) |
| `topic` | TEXT | Topic name (PK with user_id) |
| `read` | INTEGER | Read permission |
| `write` | INTEGER | Write permission |

---

## iOS upstream poll-forward

The upstream poll-forward sends a `POST` to `{upstream_base_url}/{sha256(base_url + "/" + topic)}` with an `X-Poll-ID: <message_id>` header. The topic is hashed from its full URL so the upstream server never learns the actual topic name.

`base_url` must be configured for this to work. The hash is derived from the full topic URL — if `base_url` is wrong or missing, the upstream server cannot route the wake signal to the correct subscriber and iOS notifications will not arrive.

## Known limitations

- Certificate hot-reload not supported; restart required for new TLS cert.
- `?scheduled=1` subscribe param (show pending delayed messages) not implemented.
- First admin user requires direct DB access to set `role='admin'`.
- Unix socket listener is Linux/macOS only; `listen_unix` has no effect on Windows.
- PostgreSQL not supported; SQLite only.
