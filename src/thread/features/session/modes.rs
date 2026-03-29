//! Режимные slash-команды сессии: `/plan on|off`.

use agent_client_protocol::{Error, Plan, SessionUpdate, StopReason};
use codex_protocol::config_types::ModeKind;

use crate::thread::{
    APPROVAL_PRESETS, AUTO_MODE_ID, EditApprovalMode, ThreadInner,
    features::plan::collaboration_mode_label, turn_notify::notify_mode_and_config_update,
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
                "Unsupported plan mode `{raw_value}`.\nUse one of: `on`, `off`, `plan`, `default`."
            ))
            .await;
        return Ok(StopReason::EndTurn);
    }

    if let Some(mode) = mode {
        if mode == ModeKind::Plan
            && let Some(default_preset) = APPROVAL_PRESETS
                .iter()
                .find(|preset| preset.id == AUTO_MODE_ID)
        {
            inner.apply_mode_preset(
                default_preset,
                EditApprovalMode::AutoApprove,
                ModeKind::Plan,
            );
        } else {
            inner.collaboration_mode_kind = mode;
        }
        if mode == ModeKind::Default {
            inner.last_plan_steps.clear();
            inner.carryover_plan_steps = None;
            inner
                .client
                .send_notification(SessionUpdate::Plan(Plan::new(Vec::new())))
                .await;
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
