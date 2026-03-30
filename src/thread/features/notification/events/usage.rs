//! Usage notification-ветки (token usage / context window updates).

use codex_app_server_protocol::RateLimitSnapshot;

use crate::thread::{
    ThreadInner, session_config::i64_to_u64_saturating, session_usage_cache::persist_context_usage,
    turn_notify::notify_config_update,
};

// Синхронизируем usage-конфиг при очередном token-usage update для активного thread.
pub(in crate::thread) async fn emit_thread_token_usage_updated(
    inner: &mut ThreadInner,
    thread_id: String,
    turn_id: String,
    last_total_tokens: i64,
    model_context_window: Option<i64>,
) {
    if thread_id != inner.thread_id {
        return;
    }

    // `total` накапливается между turn. Для заполненности контекста нужен
    // in-window total последнего turn.
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
        if let Err(error) = persist_context_usage(
            &inner.context_usage_cache_path,
            &thread_id,
            &turn_id,
            used,
            size,
        ) {
            tracing::warn!(
                thread_id,
                turn_id,
                %error,
                "failed to persist context usage cache"
            );
        }
    }
    notify_config_update(inner).await;
}

// Лимиты аккаунта отображаются в config selector и должны обновляться live.
pub(in crate::thread) async fn emit_account_rate_limits_updated(
    inner: &mut ThreadInner,
    rate_limits: RateLimitSnapshot,
) {
    inner.account_rate_limits = Some(rate_limits);
    notify_config_update(inner).await;
}
