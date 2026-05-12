use crate::message::Message;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;

/// Capacity of the per-topic broadcast channel.
/// Slow subscribers that fall more than this many messages behind receive a
/// `RecvError::Lagged` and must reconnect.
const BROADCAST_CAPACITY: usize = 64;

/// How long a topic with no subscribers and no recent activity is considered
/// stale and eligible for removal from the map.
const STALE_SECS: u64 = 16 * 60 * 60; // 16 hours

/// A single pub/sub topic.
pub struct Topic {
    #[allow(dead_code)]
    pub id: String,
    /// Sender half of the broadcast channel. Cloning it gives a new Sender
    /// that shares the same channel; calling `.subscribe()` gives a Receiver.
    pub tx: broadcast::Sender<Arc<Message>>,
    /// Last time a message was published or a subscriber connected.
    pub last_access: Instant,
}

impl Topic {
    fn new(id: String) -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Topic {
            id,
            tx,
            last_access: Instant::now(),
        }
    }

    pub fn is_stale(&self) -> bool {
        self.tx.receiver_count() == 0
            && self.last_access.elapsed().as_secs() > STALE_SECS
    }

    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

/// The shared, concurrent map of all live topics.
///
/// `DashMap` shards its internal lock so reads and writes on different keys
/// do not contend. We wrap each `Topic` in an `Arc` so callers can hold a
/// reference to a topic without keeping the map shard locked.
pub struct TopicMap {
    inner: DashMap<String, Arc<Topic>>,
}

impl TopicMap {
    pub fn new() -> Self {
        TopicMap {
            inner: DashMap::new(),
        }
    }

    /// Return the topic for `id`, creating it if it does not exist.
    pub fn get_or_create(&self, id: &str) -> Arc<Topic> {
        if let Some(t) = self.inner.get(id) {
            return Arc::clone(&t);
        }
        // Entry API avoids a race between the get and the insert.
        let topic = Arc::new(Topic::new(id.to_string()));
        self.inner
            .entry(id.to_string())
            .or_insert_with(|| Arc::clone(&topic));
        // Return whatever is now in the map (another thread may have won).
        Arc::clone(&self.inner.get(id).unwrap())
    }

    /// Publish a message to a topic's broadcast channel.
    /// Returns the number of active receivers that received it (0 is fine —
    /// the message is still persisted to the cache by the caller).
    pub fn publish(&self, topic_id: &str, msg: Arc<Message>) -> usize {
        if let Some(t) = self.inner.get(topic_id) {
            // send() only errors when there are zero receivers, which is not
            // an error condition for us.
            t.tx.send(msg).unwrap_or(0)
        } else {
            0
        }
    }

    /// Remove topics that have no subscribers and haven't been accessed
    /// recently. Called periodically by the background manager task.
    pub fn prune_stale(&self) -> usize {
        let stale: Vec<String> = self
            .inner
            .iter()
            .filter(|e| e.value().is_stale())
            .map(|e| e.key().clone())
            .collect();
        let count = stale.len();
        for key in stale {
            self.inner.remove(&key);
        }
        count
    }

    pub fn topic_count(&self) -> usize {
        self.inner.len()
    }

    pub fn subscriber_count(&self) -> usize {
        self.inner.iter().map(|e| e.value().subscriber_count()).sum()
    }
}

impl Default for TopicMap {
    fn default() -> Self {
        Self::new()
    }
}
