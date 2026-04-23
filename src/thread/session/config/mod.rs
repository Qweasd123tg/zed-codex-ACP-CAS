//! Маппинг конфигурации сессии между ACP-опциями и runtime-настройками Codex app-server.

use crate::thread::{
    AppAskForApproval, AppModel, AppSandboxMode, ContextUsageSource, EditApprovalMode, ModeKind,
    ReasoningEffort, ServiceTier, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOption, ThreadInner,
};

#[path = "context.rs"]
mod context;
#[path = "fast_mode.rs"]
mod fast_mode;
#[path = "limits.rs"]
mod limits;
#[path = "modes.rs"]
mod modes;
#[path = "reasoning.rs"]
mod reasoning;

pub(super) use modes::{
    current_permission_mode_id, i64_to_u64_saturating, mode_state, permission_modes,
    policy_to_mode, session_model_state, to_app_approval, to_app_sandbox_mode,
};

#[derive(Clone, Copy, Debug)]
// Входные параметры для сборки списка session config options.
pub(in crate::thread) struct ConfigOptionsInput<'a> {
    pub(in crate::thread) workspace_cwd: &'a std::path::Path,
    pub(in crate::thread) models: &'a [AppModel],
    pub(in crate::thread) current_model: &'a str,
    pub(in crate::thread) current_service_tier: Option<ServiceTier>,
    pub(in crate::thread) current_reasoning_effort: ReasoningEffort,
    pub(in crate::thread) current_used_tokens: Option<u64>,
    pub(in crate::thread) current_context_window_size: Option<u64>,
    pub(in crate::thread) current_usage_percent: Option<u64>,
    pub(in crate::thread) current_context_usage_source: Option<ContextUsageSource>,
    pub(in crate::thread) current_account_rate_limits:
        Option<&'a codex_app_server_protocol::RateLimitSnapshot>,
    pub(in crate::thread) compaction_in_progress: bool,
    pub(in crate::thread) approval: AppAskForApproval,
    pub(in crate::thread) sandbox: AppSandboxMode,
    pub(in crate::thread) edit_approval_mode: EditApprovalMode,
    pub(in crate::thread) collaboration_mode_kind: ModeKind,
    pub(in crate::thread) account_status: &'a context::AccountStatus,
    pub(in crate::thread) total_token_usage:
        Option<&'a codex_app_server_protocol::TokenUsageBreakdown>,
    pub(in crate::thread) session_mcp_summary: &'a context::ContextSelectorSummary,
    pub(in crate::thread) session_skills_summary: &'a context::ContextSelectorSummary,
    pub(in crate::thread) session_plugins_summary: &'a context::ContextSelectorSummary,
}

pub(in crate::thread) fn config_options_input(inner: &ThreadInner) -> ConfigOptionsInput<'_> {
    ConfigOptionsInput {
        workspace_cwd: &inner.workspace_cwd,
        models: &inner.models,
        current_model: &inner.current_model,
        current_service_tier: inner.service_tier,
        current_reasoning_effort: inner.reasoning_effort,
        current_used_tokens: inner.last_used_tokens,
        current_context_window_size: inner.context_window_size,
        current_usage_percent: usage_percent(inner.last_used_tokens, inner.context_window_size),
        current_context_usage_source: inner.context_usage_source,
        current_account_rate_limits: inner.account_rate_limits.as_ref(),
        compaction_in_progress: inner.compaction_in_progress,
        approval: inner.approval_policy,
        sandbox: inner.sandbox_mode,
        edit_approval_mode: inner.edit_approval_mode,
        collaboration_mode_kind: inner.collaboration_mode_kind,
        account_status: &inner.account_status,
        total_token_usage: inner.total_token_usage.as_ref(),
        session_mcp_summary: &inner.session_mcp_summary,
        session_skills_summary: &inner.session_skills_summary,
        session_plugins_summary: &inner.session_plugins_summary,
    }
}

pub(super) fn config_options(input: ConfigOptionsInput<'_>) -> Vec<SessionConfigOption> {
    let ConfigOptionsInput {
        workspace_cwd,
        models,
        current_model,
        current_service_tier,
        current_reasoning_effort,
        current_used_tokens,
        current_context_window_size,
        current_usage_percent,
        current_context_usage_source,
        current_account_rate_limits,
        compaction_in_progress,
        approval,
        sandbox,
        edit_approval_mode,
        collaboration_mode_kind,
        account_status,
        total_token_usage,
        session_mcp_summary,
        session_skills_summary,
        session_plugins_summary,
    } = input;

    let mode_state = mode_state(collaboration_mode_kind);
    let current_permissions_id = current_permission_mode_id(approval, sandbox, edit_approval_mode);
    let current_model_entry = find_model_for_current(models, current_model);
    let current_model_id = current_model_entry
        .map(|model| model.id.clone())
        .unwrap_or_else(|| current_model.to_string());
    let current_effort_value = reasoning_effort_value(current_reasoning_effort);
    let current_effort_label = reasoning::reasoning_effort_option_label(current_reasoning_effort);

    let mut options = Vec::with_capacity(6);
    let mut mode_options = Vec::with_capacity(mode_state.available_modes.len());
    for mode in mode_state.available_modes {
        mode_options.push(
            SessionConfigSelectOption::new(mode.id.0, mode.name).description(mode.description),
        );
    }
    options.push(
        SessionConfigOption::select("mode", "Mode", mode_state.current_mode_id.0, mode_options)
            .category(SessionConfigOptionCategory::Mode)
            .description("Choose how the agent should collaborate in this session"),
    );

    let mut permission_options = Vec::new();
    for permission_mode in permission_modes(approval, sandbox, edit_approval_mode) {
        permission_options.push(
            SessionConfigSelectOption::new(permission_mode.id.0, permission_mode.name)
                .description(permission_mode.description),
        );
    }
    options.push(
        SessionConfigOption::select(
            "permissions",
            "Permissions",
            current_permissions_id.0,
            permission_options,
        )
        .description("Choose file edit and sandbox permission behavior"),
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

    options.push(
        SessionConfigOption::select(
            "fast_mode",
            "Fast Mode",
            fast_mode::fast_mode_value(current_service_tier),
            fast_mode::fast_mode_options(current_service_tier),
        )
        .category(SessionConfigOptionCategory::Model)
        .description("Choose whether Codex should request the Fast service tier for new turns"),
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
                    reasoning::reasoning_effort_option_label(option.reasoning_effort),
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
        .category(SessionConfigOptionCategory::ThoughtLevel)
        .description("Choose how much reasoning effort Codex should use"),
    );

    options.push(
        SessionConfigOption::select(
            "context_control",
            "Context",
            context::CONTEXT_STATUS_VALUE,
            context::context_control_options(
                workspace_cwd,
                account_status,
                total_token_usage,
                current_used_tokens,
                current_context_window_size,
                current_usage_percent,
                current_context_usage_source,
                current_account_rate_limits,
                compaction_in_progress,
                session_mcp_summary,
                session_skills_summary,
                session_plugins_summary,
            ),
        )
        .description(
            "Inspect session status, context usage, MCP, skills, plugins, limits, or start compaction",
        ),
    );

    options
}

pub(super) use context::{
    AccountStatus, CONTEXT_COMPACT_VALUE, CONTEXT_LIMITS_VALUE, CONTEXT_STATUS_VALUE,
    ContextSelectorSummary, MCP_STATUS_VALUE, PLUGINS_STATUS_VALUE, SESSION_STATUS_VALUE,
    SKILLS_STATUS_VALUE, build_account_status, build_mcp_summary, build_plugins_summary,
    build_skills_summary, context_usage_message, full_status_report,
};
pub(super) use fast_mode::{
    parse_fast_mode_value, service_tier_override_from_config, service_tier_override_from_session,
};
pub(super) use limits::combined_limits_reset_message;
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
