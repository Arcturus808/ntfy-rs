use crate::{db::{self, cache}, state::AppState, upstream};
use std::sync::Arc;
use std::time::Duration;
use tokio::time;

/// Periodic background task that:
/// 1. Fires delayed messages whose delivery time has arrived.
/// 2. Deletes expired messages from the cache.
/// 3. Prunes stale topics from the in-memory map.
pub async fn run(state: AppState) {
    let interval = Duration::from_secs(state.config.manager_interval_secs);
    let mut ticker = time::interval(interval);
    ticker.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;

        let now = chrono::Utc::now().timestamp();

        match state.db.get() {
            Ok(conn) => {
                // Fire delayed messages that are due.
                match cache::due_messages(&conn, now) {
                    Ok(due) if !due.is_empty() => {
                        tracing::debug!(count = due.len(), "firing delayed messages");
                        for msg in due {
                            if let Err(e) = cache::mark_published(&conn, &msg.id) {
                                tracing::warn!(id = %msg.id, error = %e, "failed to mark message published");
                                continue;
                            }
                            let arc_msg = Arc::new(msg.clone());
                            state.topics.publish(&msg.topic, arc_msg);

                            // iOS upstream poll-forward for delayed messages.
                            if state.config.upstream_base_url.is_some() {
                                let state2 = state.clone();
                                let topic2 = msg.topic.clone();
                                let msg_id2 = msg.id.clone();
                                tokio::spawn(async move {
                                    upstream::forward_poll(
                                        &state2.config,
                                        &topic2,
                                        &msg_id2,
                                        &state2.http,
                                    )
                                    .await;
                                });
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "failed to query due messages"),
                }

                // Expire old messages.
                match cache::delete_expired(&conn, now) {
                    Ok(n) if n > 0 => tracing::debug!(deleted = n, "expired messages pruned"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "failed to prune expired messages"),
                }

                // Expire old attachment files.
                match db::attachments::delete_expired(&conn, now) {
                    Ok(paths) if !paths.is_empty() => {
                        for path in &paths {
                            if let Err(e) = std::fs::remove_file(path) {
                                tracing::warn!(path = %path, error = %e, "failed to delete attachment file");
                            }
                        }
                        tracing::debug!(deleted = paths.len(), "expired attachment files pruned");
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "failed to prune expired attachments"),
                }
            }
            Err(e) => tracing::warn!(error = %e, "failed to get db connection for manager"),
        }

        // Prune stale topics.
        let pruned = state.topics.prune_stale();
        if pruned > 0 {
            tracing::debug!(pruned, "stale topics removed");
        }

        tracing::debug!(
            topics = state.topics.topic_count(),
            subs   = state.topics.subscriber_count(),
            "manager tick"
        );
    }
}
