//! Session-config mapping between ACP options and Codex app-server runtime settings.

use crate::thread::{
    AppAskForApproval, AppModel, AppSandboxMode, EditApprovalMode, ModeKind, ReasoningEffort,
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption, ThreadInner,
};

#[path = "modes.rs"]
mod modes;
#[path = "reasoning.rs"]
mod reasoning;

pub(super) use modes::{
    i64_to_u64_saturating, mode_state, policy_to_mode, session_model_state, to_app_approval,
    to_app_sandbox_mode,
};

#[derive(Clone, Copy, Debug)]
// Input bundle used to build the session config-options list.
pub(in crate::thread) struct ConfigOptionsInput<'a> {
    pub(in crate::thread) models: &'a [AppModel],
    pub(in crate::thread) current_model: &'a str,
    pub(in crate::thread) current_reasoning_effort: ReasoningEffort,
    pub(in crate::thread) current_usage_percent: Option<u64>,
    pub(in crate::thread) approval: AppAskForApproval,
    pub(in crate::thread) sandbox: AppSandboxMode,
    pub(in crate::thread) edit_approval_mode: EditApprovalMode,
    pub(in crate::thread) collaboration_mode_kind: ModeKind,
}

pub(in crate::thread) fn config_options_input(inner: &ThreadInner) -> ConfigOptionsInput<'_> {
    ConfigOptionsInput {
        models: &inner.models,
        current_model: &inner.current_model,
        current_reasoning_effort: inner.reasoning_effort,
        current_usage_percent: usage_percent(inner.last_used_tokens, inner.context_window_size),
        approval: inner.approval_policy,
        sandbox: inner.sandbox_mode,
        edit_approval_mode: inner.edit_approval_mode,
        collaboration_mode_kind: inner.collaboration_mode_kind,
    }
}

pub(super) fn config_options(input: ConfigOptionsInput<'_>) -> Vec<SessionConfigOption> {
    let ConfigOptionsInput {
        models,
        current_model,
        current_reasoning_effort,
        current_usage_percent,
        approval,
        sandbox,
        edit_approval_mode,
        collaboration_mode_kind,
    } = input;

    let mode_state = mode_state(
        approval,
        sandbox,
        edit_approval_mode,
        collaboration_mode_kind,
    );
    let current_model_entry = find_model_for_current(models, current_model);
    let current_model_id = current_model_entry
        .map(|model| model.id.clone())
        .unwrap_or_else(|| current_model.to_string());
    let current_effort_value = reasoning_effort_value(current_reasoning_effort);
    let current_effort_label = reasoning::reasoning_effort_option_label(
        current_reasoning_effort,
        current_reasoning_effort,
        current_usage_percent,
    );

    let mut options = Vec::with_capacity(3);
    let mut mode_options = Vec::with_capacity(mode_state.available_modes.len());
    for mode in mode_state.available_modes {
        mode_options.push(
            SessionConfigSelectOption::new(mode.id.0, mode.name).description(mode.description),
        );
    }
    options.push(
        SessionConfigOption::select(
            "mode",
            "Approval Preset",
            mode_state.current_mode_id.0,
            mode_options,
        )
        .category(SessionConfigOptionCategory::Mode)
        .description("Choose an approval and sandboxing preset for your session"),
    );

    let mut model_options = Vec::with_capacity(models.len() + 1);
    let mut has_current_model = false;
    for model in models {
        if model.id == current_model_id {
            has_current_model = true;
        }
        model_options.push(
            SessionConfigSelectOption::new(model.id.clone(), model.display_name.clone())
                .description(model.description.clone()),
        );
    }

    if !has_current_model {
        model_options.push(SessionConfigSelectOption::new(
            current_model_id.clone(),
            current_model_id.clone(),
        ));
    }

    options.push(
        SessionConfigOption::select("model", "Model", current_model_id.clone(), model_options)
            .category(SessionConfigOptionCategory::Model)
            .description("Choose which model Codex should use"),
    );

    let mut reasoning_options = Vec::new();
    let mut has_current_effort = false;
    if let Some(model) = current_model_entry {
        reasoning_options.reserve(model.supported_reasoning_efforts.len() + 1);
        for option in &model.supported_reasoning_efforts {
            let effort_value = reasoning_effort_value(option.reasoning_effort);
            has_current_effort |= effort_value == current_effort_value;
            reasoning_options.push(
                SessionConfigSelectOption::new(
                    effort_value,
                    reasoning::reasoning_effort_option_label(
                        option.reasoning_effort,
                        current_reasoning_effort,
                        current_usage_percent,
                    ),
                )
                .description(option.description.clone()),
            );
        }
    } else {
        reasoning_options.reserve(1);
    }

    if reasoning_options.is_empty() || !has_current_effort {
        reasoning_options.push(SessionConfigSelectOption::new(
            current_effort_value,
            current_effort_label,
        ));
    }

    options.push(
        SessionConfigOption::select(
            "reasoning_effort",
            "Reasoning Effort",
            current_effort_value,
            reasoning_options,
        )
        .category(SessionConfigOptionCategory::Model)
        .description("Choose how much reasoning effort Codex should use"),
    );

    options
}

pub(super) use reasoning::{
    find_model_for_current, normalize_reasoning_effort_for_model, parse_reasoning_effort,
    reasoning_effort_value, resolve_reasoning_effort,
};

pub(super) fn usage_percent(used: Option<u64>, size: Option<u64>) -> Option<u64> {
    let used = used?;
    let size = size?;
    if size == 0 {
        return None;
    }
    // Round to nearest integer without floating point: floor((used*100 + size/2) / size).
    let clamped = used.min(size);
    let numerator = clamped.saturating_mul(100).saturating_add(size / 2);
    Some(numerator / size)
}
