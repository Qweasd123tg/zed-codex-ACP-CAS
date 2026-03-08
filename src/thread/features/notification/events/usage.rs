//! Usage-notification branches for token usage and context-window updates.

use crate::thread::{
    ThreadInner, session_config::i64_to_u64_saturating, turn_notify::notify_config_update,
};

// Synchronize usage config on each token-usage update for the active thread.
pub(in crate::thread) async fn emit_thread_token_usage_updated(
    inner: &mut ThreadInner,
    thread_id: String,
    last_total_tokens: i64,
    model_context_window: Option<i64>,
) {
    if thread_id != inner.thread_id {
        return;
    }

    // `total` accumulates across turns. Context fullness needs the
    // in-window total from the latest turn.
    let mut used = i64_to_u64_saturating(last_total_tokens);
    inner.last_used_tokens = Some(used);
    let size = model_context_window
        .map(i64_to_u64_saturating)
        .filter(|size| *size > 0);
    if let Some(size) = size {
        if used > size {
            used = size;
            inner.last_used_tokens = Some(used);
        }
        inner.context_window_size = Some(size);
    }
    if let Some(size) = inner.context_window_size {
        inner.client.send_usage_update(used, size).await;
    }
    notify_config_update(inner).await;
}
