//! Режимные slash-команды сессии: `/plan on|off`.

use agent_client_protocol::{Error, schema::StopReason};
use codex_protocol::config_types::ModeKind;

use crate::thread::{
    ThreadInner,
    features::plan::{
        clear_visible_plan_state, collaboration_mode_label, has_visible_plan_state,
        should_clear_visible_plan_for_mode_change,
    },
    turn_notify::notify_mode_and_config_update,
};

pub(in crate::thread) async fn handle_plan_mode_command(
    inner: &mut ThreadInner,
    raw_value: Option<String>,
    mode: Option<ModeKind>,
) -> Result<StopReason, Error> {
    if let (Some(raw_value), None) = (&raw_value, mode) {
        inner
            .client
            .send_agent_text(format!(
                "Unsupported plan mode `{raw_value}`.\nUse one of: `on`, `off`, `plan`."
            ))
            .await;
        return Ok(StopReason::EndTurn);
    }

    if let Some(mode) = mode {
        let previous_mode = inner.collaboration_mode_kind;
        let had_visible_plan_state = has_visible_plan_state(inner);
        inner.collaboration_mode_kind = mode;
        if should_clear_visible_plan_for_mode_change(previous_mode, mode, had_visible_plan_state) {
            clear_visible_plan_state(inner).await;
        }
        inner.sync_sandbox_mode_from_policy("handle_plan_mode_command");
        notify_mode_and_config_update(inner).await;
        inner
            .client
            .send_agent_text(format!(
                "Collaboration mode set to `{}`.",
                collaboration_mode_label(mode),
            ))
            .await;
        return Ok(StopReason::EndTurn);
    }

    inner
        .client
        .send_agent_text(format!(
            "Current collaboration mode: `{}`.\nSet with `/plan on` or `/plan off`, or run a one-shot planning turn with `/plan <your request>`.",
            collaboration_mode_label(inner.collaboration_mode_kind),
        ))
        .await;
    Ok(StopReason::EndTurn)
}
