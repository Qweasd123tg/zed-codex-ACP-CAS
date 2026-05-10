//! Уведомления ACP о смене режима и конфигурации сессии.
//! Выделено из `turn/execution`, чтобы цикл выполнения turn был проще и изолированнее.

use super::session_config::{config_options, config_options_input, mode_state};
use super::{ConfigOptionUpdate, CurrentModeUpdate, SessionUpdate, ThreadInner};

// Отправляем текущий режим и сразу следом обновление конфигурации.
pub(super) async fn notify_mode_and_config_update(inner: &ThreadInner) {
    let current_mode_id = mode_state(inner.collaboration_mode_kind).current_mode_id;
    inner
        .client
        .send_notification(SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(
            current_mode_id,
        )))
        .await;
    notify_config_update(inner).await;
}

// Публикуем полный набор config options для ACP-клиента.
pub(super) async fn notify_config_update(inner: &ThreadInner) {
    inner
        .client
        .send_notification(SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(
            config_options(config_options_input(inner)),
        )))
        .await;
}

// Публикуем ACP usage_update для клиентов, которые умеют рисовать нативный context indicator.
pub(super) async fn notify_usage_update(inner: &ThreadInner) {
    let (Some(used), Some(size)) = (inner.last_used_tokens, inner.context_window_size) else {
        return;
    };
    if size == 0 {
        return;
    }

    inner.client.send_usage_update(used.min(size), size).await;
}
