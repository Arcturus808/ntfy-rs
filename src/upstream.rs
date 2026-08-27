//! iOS upstream poll-forward.
//!
//! When a message is published to a self-hosted ntfy-rs server, iOS clients
//! cannot receive push notifications directly because APNs requires a trusted
//! intermediary (ntfy.sh or a custom FCM project). The workaround is to send
//! a lightweight "poll request" to the upstream server on each publish. The
//! upstream server then triggers an APNs wake via FCM, and the iOS app wakes
//! and polls the self-hosted server for the actual message content.
//!
//! The poll request is a POST to `{upstream_base_url}/{sha256(topic)}` with a
//! JSON body containing a generic "New message" fallback (matching the Go ntfy
//! server format). The upstream server never sees the actual message content.

use crate::config::Config;
use sha2::{Digest, Sha256};

/// Fire-and-forget: forward a poll request to the upstream server.
/// Errors are logged and swallowed — upstream failure must never fail a publish.
///
/// The upstream server (ntfy.sh) uses the `X-Poll-ID` header to tell the iOS
/// app which message to fetch when APNs wakes it. The topic is hashed from its
/// full URL (`base_url/topic`) so ntfy.sh never learns the actual topic name.
pub async fn forward_poll(config: &Config, topic: &str, msg_id: &str, client: &reqwest::Client) {
    let upstream = match &config.upstream_base_url {
        Some(b) => b.trim_end_matches('/').to_string(),
        None => return,
    };

    let base = config.base_url.trim_end_matches('/');
    if base.is_empty() {
        tracing::warn!("upstream poll-forward skipped: base_url not configured");
        return;
    }

    // Hash the full topic URL — matches ntfy Go's sha256.Sum256([]byte(topicURL)).
    let topic_url = format!("{base}/{topic}");
    let topic_hash = sha256_hex(&topic_url);
    let url = format!("{upstream}/{topic_hash}");

    let body = format!(
        r#"{{"id":"{msg_id}","event":"poll_request","topic":"{topic_hash}","message":"New message","poll_id":"{msg_id}"}}"#
    );

    let mut req = client
        .post(&url)
        .header("X-Poll-ID", msg_id)
        .header("Content-Type", "application/json")
        .body(body);

    if let Some(token) = &config.upstream_access_token {
        req = req.bearer_auth(token);
    }

    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            tracing::debug!(topic = %topic, upstream = %url, "poll-forward sent");
        }
        Ok(resp) => {
            tracing::warn!(
                topic    = %topic,
                upstream = %url,
                status   = %resp.status(),
                "poll-forward non-2xx response"
            );
        }
        Err(e) => {
            tracing::warn!(topic = %topic, upstream = %url, error = %e, "poll-forward failed");
        }
    }
}

fn sha256_hex(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hasher
        .finalize()
        .as_slice()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
