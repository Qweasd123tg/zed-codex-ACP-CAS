//! Usage notification-ветки (token usage / context window updates).

use codex_app_server_protocol::{RateLimitSnapshot, ThreadTokenUsage};

use crate::thread::{
    ContextUsageSource, ThreadInner,
    features::notification::events::warnings::emit_account_rate_limit_warnings,
    session_config::{i64_to_u64_saturating, take_rate_limit_warnings},
    session_usage_cache::persist_context_usage,
    turn_notify::notify_config_update,
};

// Синхронизируем usage-конфиг при очередном token-usage update для активного thread.
pub(in crate::thread) async fn emit_thread_token_usage_updated(
    inner: &mut ThreadInner,
    thread_id: String,
    turn_id: String,
    token_usage: ThreadTokenUsage,
) {
    if thread_id != inner.thread_id {
        return;
    }

    // `total` накапливается между turn. Для заполненности контекста нужен
    // in-window total последнего turn.
    inner.total_token_usage = Some(token_usage.total.clone());
    let mut used = i64_to_u64_saturating(token_usage.last.total_tokens);
    inner.last_used_tokens = Some(used);
    inner.context_usage_source = Some(ContextUsageSource::Live);
    let size = token_usage
        .model_context_window
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
    let warnings = take_rate_limit_warnings(&mut inner.rate_limit_warning_state, &rate_limits);
    inner.account_rate_limits = Some(rate_limits);
    notify_config_update(inner).await;
    emit_account_rate_limit_warnings(inner, warnings).await;
}
