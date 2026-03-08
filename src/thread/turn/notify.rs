//! ACP notifications for session mode and config changes.
//! Split out of `turn/execution` so the turn loop stays simpler and more isolated.

use super::session_config::{config_options, config_options_input, mode_state};
use super::{ConfigOptionUpdate, CurrentModeUpdate, SessionUpdate, ThreadInner};

// Send the current mode and immediately follow it with a config update.
pub(super) async fn notify_mode_and_config_update(inner: &ThreadInner) {
    let current_mode_id = mode_state(
        inner.approval_policy,
        inner.sandbox_mode,
        inner.edit_approval_mode,
        inner.collaboration_mode_kind,
    )
    .current_mode_id;
    inner
        .client
        .send_notification(SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(
            current_mode_id,
        )))
        .await;
    notify_config_update(inner).await;
}

// Publish the full config-options set for the ACP client.
pub(super) async fn notify_config_update(inner: &ThreadInner) {
    inner
        .client
        .send_notification(SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(
            config_options(config_options_input(inner)),
        )))
        .await;
}
