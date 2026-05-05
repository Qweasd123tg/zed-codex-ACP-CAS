//! Маппинг конфигурации сессии между ACP-опциями и runtime-настройками Codex app-server.

use crate::thread::{
    AppAskForApproval, AppModel, AppSandboxMode, ContextUsageSource, EditApprovalMode, ModeKind,
    ReasoningEffort, ServiceTier, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectGroup, SessionConfigSelectOption, ThreadInner,
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

const MODEL_REASONING_VALUE_PREFIX: &str = "reasoning:";
const MODEL_SPEED_VALUE_PREFIX: &str = "speed:";

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
    let mut options = Vec::with_capacity(4);
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

    let mut guarded_permission_options = Vec::new();
    let mut bypass_permission_options = Vec::new();
    for permission_mode in permission_modes(approval, sandbox, edit_approval_mode) {
        let option =
            SessionConfigSelectOption::new(permission_mode.id.0.clone(), permission_mode.name)
                .description(permission_mode.description);
        match permission_mode.id.0.as_ref() {
            "full-access" => bypass_permission_options.push(option),
            _ => guarded_permission_options.push(option),
        }
    }
    let mut permission_groups = Vec::new();
    if !guarded_permission_options.is_empty() {
        permission_groups.push(SessionConfigSelectGroup::new(
            "guarded",
            "Guarded",
            guarded_permission_options,
        ));
    }
    if !bypass_permission_options.is_empty() {
        permission_groups.push(SessionConfigSelectGroup::new(
            "bypass",
            "Bypass",
            bypass_permission_options,
        ));
    }
    options.push(
        SessionConfigOption::select(
            "permissions",
            "Permissions",
            current_permissions_id.0,
            permission_groups,
        )
        .description("Choose file edit and sandbox permission behavior"),
    );

    options.push(
        SessionConfigOption::select(
            "model",
            "Model",
            current_model_id.clone(),
            model_option_groups(
                models,
                current_model_entry,
                &current_model_id,
                current_reasoning_effort,
                current_service_tier,
            ),
        )
        .category(SessionConfigOptionCategory::Model)
        .description("Choose model, reasoning effort, or speed"),
    );

    options.push(
        SessionConfigOption::select(
            "context_control",
            " ",
            context::CONTEXT_STATUS_VALUE,
            context::context_control_option_groups(
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

pub(super) fn parse_model_reasoning_value(value: &str) -> Option<ReasoningEffort> {
    value
        .strip_prefix(MODEL_REASONING_VALUE_PREFIX)
        .and_then(parse_reasoning_effort)
}

pub(super) fn parse_model_speed_value(value: &str) -> Option<Option<ServiceTier>> {
    value
        .strip_prefix(MODEL_SPEED_VALUE_PREFIX)
        .and_then(parse_fast_mode_value)
}

fn model_reasoning_value(effort: ReasoningEffort) -> String {
    format!(
        "{MODEL_REASONING_VALUE_PREFIX}{}",
        reasoning_effort_value(effort)
    )
}

fn model_speed_value(value: &str) -> String {
    format!("{MODEL_SPEED_VALUE_PREFIX}{value}")
}

fn model_option_groups(
    models: &[AppModel],
    current_model_entry: Option<&AppModel>,
    current_model_id: &str,
    current_reasoning_effort: ReasoningEffort,
    current_service_tier: Option<ServiceTier>,
) -> Vec<SessionConfigSelectGroup> {
    let mut model_options = Vec::with_capacity(models.len() + 1);
    let mut has_current_model = false;
    let current_effort_label = reasoning::reasoning_effort_option_label(current_reasoning_effort);
    let current_speed_value = fast_mode::fast_mode_value(current_service_tier);
    let current_speed_label = fast_mode_label(current_service_tier);
    for model in models {
        if model.id == current_model_id {
            has_current_model = true;
        }
        let is_current_model = model.id == current_model_id;
        let model_name = if is_current_model {
            format!("★ {}", model.display_name)
        } else {
            model.display_name.clone()
        };
        let description = if is_current_model {
            format!(
                "{}\nSelected: reasoning {}, speed {}.",
                model.description, current_effort_label, current_speed_label
            )
        } else {
            model.description.clone()
        };
        model_options.push(
            SessionConfigSelectOption::new(model.id.clone(), model_name).description(description),
        );
    }

    if !has_current_model {
        model_options.push(SessionConfigSelectOption::new(
            current_model_id.to_string(),
            format!("★ {current_model_id}"),
        ));
    }

    let current_effort_value = reasoning_effort_value(current_reasoning_effort);
    let mut reasoning_options = Vec::new();
    let mut has_current_effort = false;
    if let Some(model) = current_model_entry {
        reasoning_options.reserve(model.supported_reasoning_efforts.len() + 1);
        for option in &model.supported_reasoning_efforts {
            let effort_value = reasoning_effort_value(option.reasoning_effort);
            has_current_effort |= effort_value == current_effort_value;
            let label = reasoning::reasoning_effort_option_label(option.reasoning_effort);
            let name = if effort_value == current_effort_value {
                format!("★ {label}")
            } else {
                label
            };
            reasoning_options.push(
                SessionConfigSelectOption::new(
                    model_reasoning_value(option.reasoning_effort),
                    name,
                )
                .description(option.description.clone()),
            );
        }
    } else {
        reasoning_options.reserve(1);
    }

    if reasoning_options.is_empty() || !has_current_effort {
        reasoning_options.push(SessionConfigSelectOption::new(
            model_reasoning_value(current_reasoning_effort),
            format!("★ {current_effort_label}"),
        ));
    }

    let speed_options = fast_mode::fast_mode_options(current_service_tier)
        .into_iter()
        .map(|option| {
            let value = option.value.0.to_string();
            let name = if value == current_speed_value {
                format!("★ {}", option.name)
            } else {
                option.name
            };
            SessionConfigSelectOption::new(model_speed_value(&value), name)
                .description(option.description)
        })
        .collect();

    vec![
        SessionConfigSelectGroup::new("models", "Models", model_options),
        SessionConfigSelectGroup::new("reasoning", "Reasoning", reasoning_options),
        SessionConfigSelectGroup::new("speed", "Speed", speed_options),
    ]
}

fn fast_mode_label(service_tier: Option<ServiceTier>) -> &'static str {
    match service_tier {
        Some(ServiceTier::Fast) => "Fast",
        Some(ServiceTier::Flex) => "Flex",
        None => "Standard",
    }
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
pub(super) use limits::{
    RateLimitWarning, RateLimitWarningState, combined_limits_reset_message,
    take_rate_limit_warnings,
};
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

#[cfg(test)]
mod tests {
    use super::{
        ConfigOptionsInput, config_options, model_reasoning_value, model_speed_value,
        parse_model_reasoning_value, parse_model_speed_value,
    };
    use crate::thread::{
        AppAskForApproval, AppModel, AppSandboxMode, ContextUsageSource, EditApprovalMode,
        ModeKind, ReasoningEffort, ServiceTier,
    };
    use agent_client_protocol::schema::{
        SessionConfigKind, SessionConfigOptionCategory, SessionConfigSelectOptions,
    };
    use codex_app_server_protocol::ReasoningEffortOption;

    #[test]
    fn model_selector_groups_model_reasoning_and_speed() {
        let account_status = super::AccountStatus::default();
        let mcp_summary = super::ContextSelectorSummary::default();
        let skills_summary = super::ContextSelectorSummary::default();
        let plugins_summary = super::ContextSelectorSummary::default();
        let models = vec![AppModel {
            id: "gpt-5.5".to_string(),
            model: "gpt-5.5".to_string(),
            upgrade: None,
            upgrade_info: None,
            availability_nux: None,
            display_name: "GPT-5.5".to_string(),
            description: "Frontier model".to_string(),
            hidden: false,
            supported_reasoning_efforts: vec![ReasoningEffortOption {
                reasoning_effort: ReasoningEffort::High,
                description: "Deeper reasoning".to_string(),
            }],
            default_reasoning_effort: ReasoningEffort::High,
            input_modalities: Vec::new(),
            supports_personality: false,
            is_default: true,
        }];

        let options = config_options(ConfigOptionsInput {
            workspace_cwd: std::path::Path::new("/tmp"),
            models: &models,
            current_model: "gpt-5.5",
            current_service_tier: Some(ServiceTier::Fast),
            current_reasoning_effort: ReasoningEffort::High,
            current_used_tokens: None,
            current_context_window_size: None,
            current_usage_percent: None,
            current_context_usage_source: None::<ContextUsageSource>,
            current_account_rate_limits: None,
            compaction_in_progress: false,
            approval: AppAskForApproval::OnRequest,
            sandbox: AppSandboxMode::WorkspaceWrite,
            edit_approval_mode: EditApprovalMode::AskEveryEdit,
            collaboration_mode_kind: ModeKind::Default,
            account_status: &account_status,
            total_token_usage: None,
            session_mcp_summary: &mcp_summary,
            session_skills_summary: &skills_summary,
            session_plugins_summary: &plugins_summary,
        });

        assert_eq!(options.len(), 4);
        assert!(
            options
                .iter()
                .all(|option| option.id.0.as_ref() != "reasoning_effort")
        );
        assert!(
            options
                .iter()
                .all(|option| option.id.0.as_ref() != "fast_mode")
        );

        let model = options
            .iter()
            .find(|option| option.id.0.as_ref() == "model")
            .expect("model selector exists");
        assert_eq!(model.category, Some(SessionConfigOptionCategory::Model));
        let SessionConfigKind::Select(select) = &model.kind else {
            panic!("model selector should be a select config option");
        };
        assert_eq!(select.current_value.0.as_ref(), "gpt-5.5");
        let SessionConfigSelectOptions::Grouped(groups) = &select.options else {
            panic!("model selector should use grouped options");
        };
        assert_eq!(
            groups
                .iter()
                .map(|group| group.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Models", "Reasoning", "Speed"]
        );
        assert!(groups[0].options.iter().any(|option| {
            option.name == "★ GPT-5.5"
                && option
                    .description
                    .as_deref()
                    .is_some_and(|description| description.contains("reasoning High, speed Fast"))
        }));
        assert!(
            groups[1].options.iter().any(
                |option| option.value.0.as_ref() == "reasoning:high" && option.name == "★ High"
            )
        );
        assert!(
            groups[2]
                .options
                .iter()
                .any(|option| option.value.0.as_ref() == "speed:fast" && option.name == "★ Fast")
        );
    }

    #[test]
    fn model_selector_composite_values_parse_back_to_settings() {
        assert_eq!(
            parse_model_reasoning_value(&model_reasoning_value(ReasoningEffort::XHigh)),
            Some(ReasoningEffort::XHigh)
        );
        assert_eq!(
            parse_model_speed_value(&model_speed_value("flex")),
            Some(Some(ServiceTier::Flex))
        );
        assert_eq!(parse_model_reasoning_value("gpt-5.5"), None);
        assert_eq!(parse_model_speed_value("gpt-5.5"), None);
    }
}
