use crate::{db::cache, state::AppState};
use std::time::Duration;
use tokio::time;

/// Periodic background task that:
/// 1. Deletes expired messages from the cache.
/// 2. Prunes stale topics from the in-memory map.
pub async fn run(state: AppState) {
    let interval = Duration::from_secs(state.config.manager_interval_secs);
    let mut ticker = time::interval(interval);
    ticker.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;

        let now = chrono::Utc::now().timestamp();

        // Expire old messages.
        match state.db.get() {
            Ok(conn) => match cache::delete_expired(&conn, now) {
                Ok(n) if n > 0 => tracing::debug!(deleted = n, "expired messages pruned"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "failed to prune expired messages"),
            },
            Err(e) => tracing::warn!(error = %e, "failed to get db connection for manager"),
        }

        // Prune stale topics.
        let pruned = state.topics.prune_stale();
        if pruned > 0 {
            tracing::debug!(pruned, "stale topics removed");
        }

        tracing::debug!(
            topics   = state.topics.topic_count(),
            subs     = state.topics.subscriber_count(),
            "manager tick"
        );
    }
}
