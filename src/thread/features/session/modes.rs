//! Session mode commands such as `/reasoning` and `/plan on|off`.

use agent_client_protocol::{Error, Plan, SessionUpdate, StopReason};
use codex_protocol::config_types::ModeKind;
use codex_protocol::openai_models::ReasoningEffort;

use crate::thread::{
    APPROVAL_PRESETS, AUTO_MODE_ID, EditApprovalMode, ThreadInner,
    features::plan::collaboration_mode_label,
    session_config::{find_model_for_current, reasoning_effort_value},
    turn_notify::{notify_config_update, notify_mode_and_config_update},
};

pub(in crate::thread) async fn handle_reasoning_command(
    inner: &mut ThreadInner,
    raw_value: Option<String>,
    effort: Option<ReasoningEffort>,
) -> Result<StopReason, Error> {
    if let (Some(raw_value), None) = (&raw_value, effort) {
        inner
            .client
            .send_agent_text(format!(
                "Unsupported reasoning effort `{raw_value}`.\nUse one of: `none`, `minimal`, `low`, `medium`, `high`, `xhigh`."
            ))
            .await;
        return Ok(StopReason::EndTurn);
    }

    let model_name = find_model_for_current(&inner.models, &inner.current_model)
        .map(|model| model.display_name.clone())
        .unwrap_or_else(|| inner.current_model.clone());

    if let Some(effort) = effort {
        if let Some(model) = find_model_for_current(&inner.models, &inner.current_model)
            && !model
                .supported_reasoning_efforts
                .iter()
                .any(|option| option.reasoning_effort == effort)
        {
            let supported = model
                .supported_reasoning_efforts
                .iter()
                .map(|option| format!("`{}`", reasoning_effort_value(option.reasoning_effort)))
                .collect::<Vec<_>>()
                .join(", ");
            inner
                .client
                .send_agent_text(format!(
                    "Model `{}` does not support `{}`.\nSupported values: {}",
                    model.display_name,
                    reasoning_effort_value(effort),
                    supported,
                ))
                .await;
            return Ok(StopReason::EndTurn);
        }

        inner.reasoning_effort = effort;
        notify_config_update(inner).await;

        inner
            .client
            .send_agent_text(format!(
                "Reasoning effort set to `{}` for model `{}`.",
                reasoning_effort_value(effort),
                model_name,
            ))
            .await;
        return Ok(StopReason::EndTurn);
    }

    let supported = find_model_for_current(&inner.models, &inner.current_model)
        .map(|model| {
            model
                .supported_reasoning_efforts
                .iter()
                .map(|option| format!("`{}`", reasoning_effort_value(option.reasoning_effort)))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "`none`, `minimal`, `low`, `medium`, `high`, `xhigh`".to_string());

    inner
        .client
        .send_agent_text(format!(
            "Current reasoning effort: `{}`\nModel: `{}`\nSupported: {}\nSet with `/reasoning <value>`.",
            reasoning_effort_value(inner.reasoning_effort),
            model_name,
            supported,
        ))
        .await;
    Ok(StopReason::EndTurn)
}

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
