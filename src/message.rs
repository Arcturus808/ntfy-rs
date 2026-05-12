use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};

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

    #[serde(skip_serializing_if = "is_zero", default)]
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
}

fn is_zero(v: &i32) -> bool {
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

    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,

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
