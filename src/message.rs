use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const MESSAGE_ID_LENGTH: usize = 12;
pub const MAX_TOPIC_LENGTH: usize = 64;
#[allow(dead_code)]
pub const TOPIC_REGEX: &str = r"^[-_A-Za-z0-9]{1,64}$";

// Event type constants — kept identical to ntfy for client compatibility.
pub const EVENT_OPEN: &str = "open";
#[allow(dead_code)]
pub const EVENT_KEEPALIVE: &str = "keepalive";
pub const EVENT_MESSAGE: &str = "message";

/// A notification message, wire-compatible with ntfy's JSON format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence_id: Option<String>,

    pub time: i64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<i64>,

    pub event: String,
    pub topic: String,

    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub title: String,

    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub message: String,

    #[serde(skip_serializing_if = "is_zero_i32", default)]
    pub priority: i32,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tags: Vec<String>,

    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub click: String,

    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub icon: String,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub actions: Vec<Action>,

    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub content_type: String,

    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub encoding: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment: Option<Attachment>,
}

/// File attachment metadata included in a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,

    /// MIME type of the attachment. Serialized as "type" for wire compatibility.
    #[serde(rename = "type", skip_serializing_if = "String::is_empty", default)]
    pub content_type: String,

    #[serde(skip_serializing_if = "is_zero_u64", default)]
    pub size: u64,
    #[serde(skip_serializing_if = "is_zero_i64", default)]
    pub expires: i64,
    pub url: String,
}

fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

fn is_zero_i64(v: &i64) -> bool {
    *v == 0
}

/// An action button attached to a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub action: String,
    pub label: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,

    /// Extra HTTP request headers (for `http` actions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,

    /// Android broadcast intent (for `broadcast` actions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,

    /// Android broadcast intent extras (for `broadcast` actions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extras: Option<HashMap<String, String>>,

    #[serde(default)]
    pub clear: bool,
}

impl Message {
    pub fn new_message(topic: &str, body: String) -> Self {
        Message {
            id: generate_id(),
            sequence_id: None,
            time: chrono::Utc::now().timestamp(),
            expires: None,
            event: EVENT_MESSAGE.to_string(),
            topic: topic.to_string(),
            message: body,
            title: String::new(),
            priority: 0,
            tags: vec![],
            click: String::new(),
            icon: String::new(),
            actions: vec![],
            content_type: String::new(),
            encoding: String::new(),
            attachment: None,
        }
    }

    pub fn new_open(topic: &str) -> Self {
        Message {
            id: generate_id(),
            sequence_id: None,
            time: chrono::Utc::now().timestamp(),
            expires: None,
            event: EVENT_OPEN.to_string(),
            topic: topic.to_string(),
            message: String::new(),
            title: String::new(),
            priority: 0,
            tags: vec![],
            click: String::new(),
            icon: String::new(),
            actions: vec![],
            content_type: String::new(),
            encoding: String::new(),
            attachment: None,
        }
    }

    #[allow(dead_code)]
    pub fn new_keepalive(topic: &str) -> Self {
        Message {
            id: generate_id(),
            sequence_id: None,
            time: chrono::Utc::now().timestamp(),
            expires: None,
            event: EVENT_KEEPALIVE.to_string(),
            topic: topic.to_string(),
            message: String::new(),
            title: String::new(),
            priority: 0,
            tags: vec![],
            click: String::new(),
            icon: String::new(),
            actions: vec![],
            content_type: String::new(),
            encoding: String::new(),
            attachment: None,
        }
    }
}

pub fn generate_id() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(MESSAGE_ID_LENGTH)
        .map(char::from)
        .collect()
}

/// Validate a message ID: exactly MESSAGE_ID_LENGTH alphanumeric characters.
pub fn valid_message_id(id: &str) -> bool {
    id.len() == MESSAGE_ID_LENGTH && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Validate a topic name: 1–64 chars, alphanumeric plus `-` and `_`.
pub fn valid_topic(topic: &str) -> bool {
    !topic.is_empty()
        && topic.len() <= MAX_TOPIC_LENGTH
        && topic
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parse a comma-separated list of topic names, validating each.
#[allow(dead_code)]
pub fn parse_topics(raw: &str) -> Option<Vec<String>> {
    let topics: Vec<String> = raw.split(',').map(|s| s.to_string()).collect();
    if topics.iter().all(|t| valid_topic(t)) {
        Some(topics)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── valid_topic ─────────────────────────────────────────────────────

    #[test]
    fn test_valid_topic_simple() {
        assert!(valid_topic("mytopic"));
    }

    #[test]
    fn test_valid_topic_with_dash_underscore() {
        assert!(valid_topic("my-topic_123"));
    }

    #[test]
    fn test_valid_topic_max_length() {
        let topic = "a".repeat(64);
        assert!(valid_topic(&topic));
    }

    #[test]
    fn test_valid_topic_too_long() {
        let topic = "a".repeat(65);
        assert!(!valid_topic(&topic));
    }

    #[test]
    fn test_valid_topic_empty() {
        assert!(!valid_topic(""));
    }

    #[test]
    fn test_valid_topic_invalid_chars() {
        assert!(!valid_topic("my topic"));
        assert!(!valid_topic("topic!"));
        assert!(!valid_topic("topic.json"));
    }

    // ── valid_message_id ────────────────────────────────────────────────

    #[test]
    fn test_valid_message_id_correct() {
        assert!(valid_message_id("AbC123xyz789"));
    }

    #[test]
    fn test_valid_message_id_wrong_length() {
        assert!(!valid_message_id("abc"));
        assert!(!valid_message_id(&"a".repeat(20)));
    }

    #[test]
    fn test_valid_message_id_invalid_chars() {
        assert!(!valid_message_id("abc-def-ghi-"));
    }

    // ── parse_topics ────────────────────────────────────────────────────

    #[test]
    fn test_parse_topics_single() {
        assert_eq!(parse_topics("mytopic"), Some(vec!["mytopic".to_string()]));
    }

    #[test]
    fn test_parse_topics_multiple() {
        let result = parse_topics("topic1,topic2,topic3");
        assert_eq!(result.unwrap().len(), 3);
    }

    #[test]
    fn test_parse_topics_invalid() {
        assert!(parse_topics("good,bad topic").is_none());
    }

    // ── generate_id ─────────────────────────────────────────────────────

    #[test]
    fn test_generate_id_length() {
        let id = generate_id();
        assert_eq!(id.len(), MESSAGE_ID_LENGTH);
        assert!(valid_message_id(&id));
    }

    #[test]
    fn test_generate_id_unique() {
        let a = generate_id();
        let b = generate_id();
        assert_ne!(a, b);
    }

    // ── Message serialization ──────────────────────────────────────────

    #[test]
    fn test_message_serialization_skips_empty_fields() {
        let msg = Message::new_message("test", "hello".to_string());
        let json = serde_json::to_value(&msg).unwrap();
        // Empty fields should be skipped
        assert!(json.get("title").is_none());
        assert!(json.get("tags").is_none());
        assert!(json.get("click").is_none());
        // Non-empty fields should be present
        assert_eq!(json["topic"], "test");
        assert_eq!(json["message"], "hello");
    }

    #[test]
    fn test_message_serialization_includes_non_empty() {
        let mut msg = Message::new_message("test", "hello".to_string());
        msg.title = "Title".to_string();
        msg.priority = 4;
        msg.tags = vec!["✅".to_string()];
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["title"], "Title");
        assert_eq!(json["priority"], 4);
        assert_eq!(json["tags"].as_array().unwrap().len(), 1);
    }

    // ── Attachment serialization ────────────────────────────────────────

    #[test]
    fn test_attachment_skips_zero_size_and_expires() {
        let att = Attachment {
            name: "file.jpg".to_string(),
            content_type: "image/jpeg".to_string(),
            size: 0,
            expires: 0,
            url: "https://example.com/file".to_string(),
        };
        let json = serde_json::to_value(&att).unwrap();
        assert!(json.get("size").is_none());
        assert!(json.get("expires").is_none());
        assert_eq!(json["name"], "file.jpg");
    }

    #[test]
    fn test_attachment_includes_nonzero_size_and_expires() {
        let att = Attachment {
            name: "file.jpg".to_string(),
            content_type: "image/jpeg".to_string(),
            size: 12345,
            expires: 9999,
            url: "https://example.com/file".to_string(),
        };
        let json = serde_json::to_value(&att).unwrap();
        assert_eq!(json["size"], 12345);
        assert_eq!(json["expires"], 9999);
    }
}
