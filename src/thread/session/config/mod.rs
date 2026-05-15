//! Маппинг конфигурации сессии между ACP-опциями и runtime-настройками Codex app-server.

use crate::thread::session_selector_preferences::{
    ModelSelectorPreferences, SelectorLayoutEntry, SelectorLayoutPreferences,
};
use crate::thread::{
    AppAskForApproval, AppModel, AppSandboxMode, ContextControlDisplay, ContextDisplayStyle,
    ContextUsageSource, LimitsDisplayStyle, ModeKind, PLAN_SESSION_MODE_ID, ReasoningEffort,
    ServiceTier, SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectGroup,
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

const MODEL_REASONING_VALUE_PREFIX: &str = "reasoning:";
const MODEL_SPEED_VALUE_PREFIX: &str = "speed:";

impl ContextControlDisplay {
    fn value_id(self) -> &'static str {
        match self {
            Self::Context => context::CONTEXT_STATUS_VALUE,
            Self::Limits => context::CONTEXT_LIMITS_VALUE,
            Self::ContextAndLimits => context::CONTEXT_COMBINED_VALUE,
        }
    }
}

#[derive(Clone, Copy, Debug)]
// Входные параметры для сборки списка session config options.
pub(in crate::thread) struct ConfigOptionsInput<'a> {
    pub(in crate::thread) workspace_cwd: &'a std::path::Path,
    pub(in crate::thread) models: &'a [AppModel],
    pub(in crate::thread) current_model: &'a str,
    pub(in crate::thread) current_service_tier: Option<ServiceTier>,
    pub(in crate::thread) current_reasoning_effort: ReasoningEffort,
    pub(in crate::thread) model_selector: &'a ModelSelectorPreferences,
    pub(in crate::thread) current_used_tokens: Option<u64>,
    pub(in crate::thread) current_context_window_size: Option<u64>,
    pub(in crate::thread) current_usage_percent: Option<u64>,
    pub(in crate::thread) current_context_usage_source: Option<ContextUsageSource>,
    pub(in crate::thread) current_context_display: ContextControlDisplay,
    pub(in crate::thread) current_context_display_style: ContextDisplayStyle,
    pub(in crate::thread) current_limits_display_style: LimitsDisplayStyle,
    pub(in crate::thread) current_account_rate_limits:
        Option<&'a codex_app_server_protocol::RateLimitSnapshot>,
    pub(in crate::thread) compaction_in_progress: bool,
    pub(in crate::thread) approval: AppAskForApproval,
    pub(in crate::thread) sandbox: AppSandboxMode,
    pub(in crate::thread) collaboration_mode_kind: ModeKind,
    pub(in crate::thread) account_status: &'a context::AccountStatus,
    pub(in crate::thread) total_token_usage:
        Option<&'a codex_app_server_protocol::TokenUsageBreakdown>,
    pub(in crate::thread) session_mcp_summary: &'a context::ContextSelectorSummary,
    pub(in crate::thread) session_skills_summary: &'a context::ContextSelectorSummary,
    pub(in crate::thread) session_plugins_summary: &'a context::ContextSelectorSummary,
    pub(in crate::thread) selector_layout: &'a SelectorLayoutPreferences,
}

pub(in crate::thread) fn config_options_input(inner: &ThreadInner) -> ConfigOptionsInput<'_> {
    ConfigOptionsInput {
        workspace_cwd: &inner.workspace_cwd,
        models: &inner.models,
        current_model: &inner.current_model,
        current_service_tier: inner.service_tier,
        current_reasoning_effort: inner.reasoning_effort,
        model_selector: &inner.model_selector,
        current_used_tokens: inner.last_used_tokens,
        current_context_window_size: inner.context_window_size,
        current_usage_percent: usage_percent(inner.last_used_tokens, inner.context_window_size),
        current_context_usage_source: inner.context_usage_source,
        current_context_display: inner.context_control_display,
        current_context_display_style: inner.context_display_style,
        current_limits_display_style: inner.limits_display_style,
        current_account_rate_limits: inner.account_rate_limits.as_ref(),
        compaction_in_progress: inner.compaction_in_progress,
        approval: inner.approval_policy,
        sandbox: inner.sandbox_mode,
        collaboration_mode_kind: inner.collaboration_mode_kind,
        account_status: &inner.account_status,
        total_token_usage: inner.total_token_usage.as_ref(),
        session_mcp_summary: &inner.session_mcp_summary,
        session_skills_summary: &inner.session_skills_summary,
        session_plugins_summary: &inner.session_plugins_summary,
        selector_layout: &inner.selector_layout,
    }
}

pub(super) fn config_options(input: ConfigOptionsInput<'_>) -> Vec<SessionConfigOption> {
    let ConfigOptionsInput {
        workspace_cwd,
        models,
        current_model,
        current_service_tier,
        current_reasoning_effort,
        model_selector,
        current_used_tokens,
        current_context_window_size,
        current_usage_percent,
        current_context_usage_source,
        current_context_display,
        current_context_display_style,
        current_limits_display_style,
        current_account_rate_limits,
        compaction_in_progress,
        approval,
        sandbox,
        collaboration_mode_kind,
        account_status,
        total_token_usage,
        session_mcp_summary,
        session_skills_summary,
        session_plugins_summary,
        selector_layout,
    } = input;

    let current_permissions_id = current_permission_mode_id(approval, sandbox);
    let current_permissions_value = if collaboration_mode_kind == ModeKind::Plan {
        PLAN_SESSION_MODE_ID.to_string()
    } else {
        current_permissions_id.0.to_string()
    };
    let current_model_entry = find_model_for_current(models, current_model);
    let current_model_id = current_model_entry
        .map(|model| model.id.clone())
        .unwrap_or_else(|| current_model.to_string());
    let mut options = Vec::with_capacity(3);

    let workflow_options = vec![
        SessionConfigSelectOption::new(PLAN_SESSION_MODE_ID, "Plan")
            .description("Plan-first mode with visible step tracking."),
    ];
    let mut guarded_permission_options = Vec::new();
    let mut bypass_permission_options = Vec::new();
    for permission_mode in permission_modes() {
        let option =
            SessionConfigSelectOption::new(permission_mode.id.0.clone(), permission_mode.name)
                .description(permission_mode.description);
        match permission_mode.id.0.as_ref() {
            "full-access" => bypass_permission_options.push(option),
            _ => guarded_permission_options.push(option),
        }
    }
    let mut permission_groups = Vec::new();
    permission_groups.push(SessionConfigSelectGroup::new(
        "workflow",
        "Workflow",
        workflow_options,
    ));
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
    options.push((
        "permissions",
        SessionConfigOption::select(
            "permissions",
            selector_name(selector_layout, "permissions", "Permissions"),
            current_permissions_value,
            apply_group_layout(selector_layout, "permissions", permission_groups),
        )
        .description(current_permission_description(
            approval,
            sandbox,
            collaboration_mode_kind,
        )),
    ));

    options.push((
        "model",
        SessionConfigOption::select(
            "model",
            selector_name(selector_layout, "model", "Model"),
            current_model_id.clone(),
            apply_group_layout(
                selector_layout,
                "model",
                model_option_groups(ModelOptionGroupsInput {
                    models,
                    current_model_entry,
                    current_model_id: &current_model_id,
                    current_reasoning_effort,
                    model_selector,
                    current_service_tier,
                }),
            ),
        )
        .category(SessionConfigOptionCategory::Model)
        .description(model_selector_description(
            current_model_entry,
            &current_model_id,
            current_reasoning_effort,
            current_service_tier,
            model_selector,
        )),
    ));

    options.push((
        "context_control",
        SessionConfigOption::select(
            "context_control",
            selector_name(selector_layout, "context_control", "Context"),
            current_context_display.value_id(),
            apply_group_layout(
                selector_layout,
                "context_control",
                context::context_control_option_groups(
                    workspace_cwd,
                    account_status,
                    total_token_usage,
                    current_used_tokens,
                    current_context_window_size,
                    current_usage_percent,
                    current_context_usage_source,
                    current_context_display_style,
                    current_limits_display_style,
                    current_account_rate_limits,
                    compaction_in_progress,
                    session_mcp_summary,
                    session_skills_summary,
                    session_plugins_summary,
                ),
            ),
        )
        .description(context_selector_description(
            current_used_tokens,
            current_context_window_size,
            current_usage_percent,
            current_context_usage_source,
            current_account_rate_limits,
            compaction_in_progress,
        )),
    ));

    apply_selector_order(selector_layout, options)
}

fn selector_name(
    layout: &SelectorLayoutPreferences,
    selector_id: &str,
    default_name: &str,
) -> String {
    layout
        .entry(selector_id)
        .and_then(|entry| entry.name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(default_name)
        .to_string()
}

fn selector_visible(layout: &SelectorLayoutPreferences, selector_id: &str) -> bool {
    layout
        .entry(selector_id)
        .and_then(|entry| entry.visible)
        .unwrap_or(true)
}

fn apply_group_layout(
    layout: &SelectorLayoutPreferences,
    selector_id: &str,
    groups: Vec<SessionConfigSelectGroup>,
) -> Vec<SessionConfigSelectGroup> {
    let Some(SelectorLayoutEntry {
        groups: Some(group_order),
        ..
    }) = layout.entry(selector_id)
    else {
        return groups;
    };
    let mut remaining = groups;
    let mut ordered = Vec::new();

    for group_id in group_order {
        if let Some(index) = remaining
            .iter()
            .position(|group| group.group.0.as_ref() == group_id.as_str())
        {
            ordered.push(remaining.remove(index));
        }
    }

    if ordered.is_empty() {
        remaining
    } else {
        ordered
    }
}

fn apply_selector_order(
    layout: &SelectorLayoutPreferences,
    options: Vec<(&'static str, SessionConfigOption)>,
) -> Vec<SessionConfigOption> {
    let mut remaining = options
        .into_iter()
        .filter(|(selector_id, _)| selector_visible(layout, selector_id))
        .collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(remaining.len());

    if let Some(selector_order) = &layout.order {
        for selector_id in selector_order {
            if let Some(index) = remaining
                .iter()
                .position(|(candidate_id, _)| *candidate_id == selector_id.as_str())
            {
                ordered.push(remaining.remove(index).1);
            }
        }
    }

    ordered.extend(remaining.into_iter().map(|(_, option)| option));
    ordered
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

fn model_label_for_selector(
    current_model_entry: Option<&AppModel>,
    current_model_id: &str,
    model_selector: &ModelSelectorPreferences,
) -> String {
    model_selector
        .model_name_override(current_model_id)
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            current_model_entry
                .map(|model| model.display_name.as_str())
                .unwrap_or(current_model_id)
                .trim()
                .to_string()
        })
}

fn current_model_label_for_selector(
    current_model_entry: Option<&AppModel>,
    current_model_id: &str,
    current_reasoning_effort: ReasoningEffort,
    model_selector: &ModelSelectorPreferences,
) -> String {
    [
        model_label_for_selector(current_model_entry, current_model_id, model_selector),
        reasoning_effort_label_for_selector(current_reasoning_effort, model_selector),
    ]
    .join(" ")
}

fn reasoning_effort_label_for_selector(
    effort: ReasoningEffort,
    model_selector: &ModelSelectorPreferences,
) -> String {
    model_selector
        .reasoning_effort_name_override(effort)
        .map(str::to_string)
        .unwrap_or_else(|| reasoning::reasoning_effort_label(effort).to_string())
}

struct ModelOptionGroupsInput<'a> {
    models: &'a [AppModel],
    current_model_entry: Option<&'a AppModel>,
    current_model_id: &'a str,
    current_reasoning_effort: ReasoningEffort,
    model_selector: &'a ModelSelectorPreferences,
    current_service_tier: Option<ServiceTier>,
}

fn model_option_groups(input: ModelOptionGroupsInput<'_>) -> Vec<SessionConfigSelectGroup> {
    let ModelOptionGroupsInput {
        models,
        current_model_entry,
        current_model_id,
        current_reasoning_effort,
        model_selector,
        current_service_tier,
    } = input;
    let mut model_options = Vec::with_capacity(models.len() + 1);
    let mut has_current_model = false;
    let current_speed_value = fast_mode::fast_mode_value(current_service_tier);
    let current_speed_label = fast_mode_label(current_service_tier);
    let current_model_label = current_model_label_for_selector(
        current_model_entry,
        current_model_id,
        current_reasoning_effort,
        model_selector,
    );
    for model in model_selector.ordered_models(models) {
        if model_selector.hides_model(&model.id, current_model_id) {
            continue;
        }
        if model.id == current_model_id {
            has_current_model = true;
        }
        let is_current_model = model.id == current_model_id;
        let model_name = if is_current_model {
            current_model_label.clone()
        } else {
            model_label_for_selector(Some(model), &model.id, model_selector)
        };
        let description = if is_current_model {
            let model_description = model_selector
                .model_description_override(&model.id)
                .unwrap_or(&model.description);
            format!(
                "{}\nSelected: reasoning effort {}, speed {}.",
                model_description,
                reasoning::reasoning_effort_description_label(current_reasoning_effort),
                current_speed_label
            )
        } else {
            model_selector
                .model_description_override(&model.id)
                .unwrap_or(&model.description)
                .to_string()
        };
        model_options.push(
            SessionConfigSelectOption::new(model.id.clone(), model_name).description(description),
        );
    }

    if !has_current_model {
        model_options.push(SessionConfigSelectOption::new(
            current_model_id.to_string(),
            current_model_label,
        ));
    }

    let reasoning_options = reasoning_effort_option_groups(
        current_model_entry,
        current_reasoning_effort,
        model_selector,
    );
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
        SessionConfigSelectGroup::new("effort", "Effort", reasoning_options),
        SessionConfigSelectGroup::new("speed", "Speed", speed_options),
    ]
}

fn reasoning_effort_option_groups(
    current_model_entry: Option<&AppModel>,
    current_reasoning_effort: ReasoningEffort,
    model_selector: &ModelSelectorPreferences,
) -> Vec<SessionConfigSelectOption> {
    let current_effort_value = reasoning_effort_value(current_reasoning_effort);
    let mut effort_options = Vec::new();
    let mut has_current_effort = false;
    let mut advertised_efforts = Vec::new();
    if let Some(model) = current_model_entry {
        effort_options.reserve(model.supported_reasoning_efforts.len() + 1);
        for option in &model.supported_reasoning_efforts {
            advertised_efforts.push(option.reasoning_effort);
            if model_selector
                .hides_reasoning_effort(option.reasoning_effort, current_reasoning_effort)
            {
                continue;
            }
            let effort_value = reasoning_effort_value(option.reasoning_effort);
            has_current_effort |= effort_value == current_effort_value;
            let label = model_selector
                .reasoning_effort_name_override(option.reasoning_effort)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    reasoning::reasoning_effort_label(option.reasoning_effort).to_string()
                });
            let name = if effort_value == current_effort_value {
                format!("★ {label}")
            } else {
                label
            };
            let description = model_selector
                .reasoning_effort_description_override(option.reasoning_effort)
                .unwrap_or(&option.description)
                .to_string();
            effort_options.push(
                SessionConfigSelectOption::new(
                    model_reasoning_value(option.reasoning_effort),
                    name,
                )
                .description(description),
            );
        }
        for effort in model_selector.configured_visible_reasoning_efforts() {
            if advertised_efforts.contains(&effort)
                || model_selector.hides_reasoning_effort(effort, current_reasoning_effort)
            {
                continue;
            }
            let effort_value = reasoning_effort_value(effort);
            has_current_effort |= effort_value == current_effort_value;
            let label = model_selector
                .reasoning_effort_name_override(effort)
                .map(str::to_string)
                .unwrap_or_else(|| reasoning::reasoning_effort_label(effort).to_string());
            let name = if effort_value == current_effort_value {
                format!("★ {label}")
            } else {
                label
            };
            let description = model_selector
                .reasoning_effort_description_override(effort)
                .map(str::to_string)
                .unwrap_or_else(|| configured_reasoning_effort_description(effort).to_string());
            effort_options.push(
                SessionConfigSelectOption::new(model_reasoning_value(effort), name)
                    .description(description),
            );
        }
    } else {
        effort_options.reserve(1);
    }

    if effort_options.is_empty() || !has_current_effort {
        effort_options.push(SessionConfigSelectOption::new(
            model_reasoning_value(current_reasoning_effort),
            format!(
                "★ {}",
                reasoning_effort_label_for_selector(current_reasoning_effort, model_selector)
            ),
        ));
    }

    effort_options
}

fn configured_reasoning_effort_description(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Minimal => {
            "Configured manually in selector-preferences.json. Minimal reasoning can reduce latency, but the backend currently rejects it when tools such as image_gen or web_search are enabled."
        }
        ReasoningEffort::None => {
            "Configured manually in selector-preferences.json. None is a protocol-visible experimental effort that may work for simple turns, but the backend can reject unsupported combinations."
        }
        _ => {
            "Configured manually in selector-preferences.json. The backend may reject it if the current model does not support this reasoning effort."
        }
    }
}

fn fast_mode_label(service_tier: Option<ServiceTier>) -> &'static str {
    match service_tier {
        Some(ServiceTier::Fast) => "Fast",
        Some(ServiceTier::Flex) => "Flex",
        None => "Standard",
    }
}

fn current_permission_description(
    approval: AppAskForApproval,
    sandbox: AppSandboxMode,
    collaboration_mode_kind: ModeKind,
) -> String {
    let permission_description = current_permission_mode_description(approval, sandbox)
        .unwrap_or_else(|| "Choose file edit and sandbox permission behavior".to_string());
    if collaboration_mode_kind == ModeKind::Plan {
        return format!(
            "Plan-first mode with visible step tracking.\nPermissions: {permission_description}"
        );
    }

    permission_description
}

fn current_permission_mode_description(
    approval: AppAskForApproval,
    sandbox: AppSandboxMode,
) -> Option<String> {
    let current_permissions_id = current_permission_mode_id(approval, sandbox);
    permission_modes()
        .into_iter()
        .find(|mode| mode.id == current_permissions_id)
        .and_then(|mode| mode.description)
}

fn model_selector_description(
    current_model_entry: Option<&AppModel>,
    current_model_id: &str,
    current_reasoning_effort: ReasoningEffort,
    current_service_tier: Option<ServiceTier>,
    model_selector: &ModelSelectorPreferences,
) -> String {
    let current_effort_label =
        reasoning_effort_label_for_selector(current_reasoning_effort, model_selector);
    let current_model_name =
        model_label_for_selector(current_model_entry, current_model_id, model_selector);
    let current_speed_label = fast_mode_label(current_service_tier);
    format!(
        "{current_model_name}\nReasoning effort: {current_effort_label}\nSpeed: {current_speed_label}"
    )
}

fn context_selector_description(
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
    usage_source: Option<ContextUsageSource>,
    rate_limits: Option<&codex_app_server_protocol::RateLimitSnapshot>,
    compaction_in_progress: bool,
) -> String {
    let mut sections = vec![
        context::context_status_description(
            used,
            size,
            usage_percent,
            usage_source,
            compaction_in_progress,
        ),
        limits::limits_status_description(rate_limits),
    ];

    if compaction_in_progress {
        sections.push("Context compaction is currently running.".to_string());
    }

    sections.join("\n\n")
}

pub(super) use context::{
    AccountStatus, CONTEXT_BRAILLE_VALUE, CONTEXT_COMBINED_VALUE, CONTEXT_COMPACT_VALUE,
    CONTEXT_LIMITS_BARS_VALUE, CONTEXT_LIMITS_BLOCK_VALUE, CONTEXT_LIMITS_TEXT_VALUE,
    CONTEXT_LIMITS_VALUE, CONTEXT_PERCENT_VALUE, CONTEXT_STATUS_VALUE, ContextSelectorSummary,
    MCP_STATUS_VALUE, PLUGINS_STATUS_VALUE, SESSION_STATUS_VALUE, SKILLS_STATUS_VALUE,
    build_account_status, build_mcp_summary, build_plugins_summary, build_skills_summary,
    full_status_report,
};
pub(super) use fast_mode::{
    parse_fast_mode_value, service_tier_override_from_config, service_tier_override_from_session,
};
pub(super) use limits::{
    RateLimitWarning, RateLimitWarningState, observe_rate_limit_snapshot, take_rate_limit_warnings,
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
    use crate::thread::session_selector_preferences::{
        ModelSelectorModelDetails, ModelSelectorModelEntry, ModelSelectorPreferences,
        ModelSelectorReasoningEffortDetails, ModelSelectorReasoningEffortEntry,
        SelectorLayoutEntry, SelectorLayoutPreferences,
    };
    use crate::thread::{
        AppAskForApproval, AppModel, AppSandboxMode, ContextControlDisplay, ContextDisplayStyle,
        ContextUsageSource, LimitsDisplayStyle, ModeKind, ReasoningEffort, ServiceTier,
    };
    use agent_client_protocol::schema::{
        SessionConfigKind, SessionConfigOptionCategory, SessionConfigSelectOptions,
    };
    use codex_app_server_protocol::{RateLimitSnapshot, RateLimitWindow, ReasoningEffortOption};
    use codex_protocol::account::PlanType;

    #[test]
    fn model_selector_uses_backend_model_names_by_default() {
        let account_status = super::AccountStatus::default();
        let mcp_summary = super::ContextSelectorSummary::default();
        let skills_summary = super::ContextSelectorSummary::default();
        let plugins_summary = super::ContextSelectorSummary::default();
        let selector_layout = SelectorLayoutPreferences::default();
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
            model_selector: &Default::default(),
            current_used_tokens: None,
            current_context_window_size: None,
            current_usage_percent: None,
            current_context_usage_source: None::<ContextUsageSource>,
            current_context_display: ContextControlDisplay::Context,
            current_context_display_style: ContextDisplayStyle::Percent,
            current_limits_display_style: LimitsDisplayStyle::Text,
            current_account_rate_limits: None,
            compaction_in_progress: false,
            approval: AppAskForApproval::OnRequest,
            sandbox: AppSandboxMode::WorkspaceWrite,
            collaboration_mode_kind: ModeKind::Default,
            account_status: &account_status,
            total_token_usage: None,
            session_mcp_summary: &mcp_summary,
            session_skills_summary: &skills_summary,
            session_plugins_summary: &plugins_summary,
            selector_layout: &selector_layout,
        });

        assert_eq!(options.len(), 3);
        assert!(options.iter().all(|option| option.id.0.as_ref() != "mode"));
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
        assert_eq!(
            model.description.as_deref(),
            Some("GPT-5.5\nReasoning effort: High\nSpeed: Fast")
        );
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
            vec!["Models", "Effort", "Speed"]
        );
        assert!(groups[0].options.iter().any(|option| {
            option.name == "GPT-5.5 High"
                && option.description.as_deref().is_some_and(|description| {
                    description.contains("Frontier model")
                        && description.contains("reasoning effort High, speed Fast")
                })
        }));
        assert!(groups[1].options.iter().any(|option| {
            option.value.0.as_ref() == "reasoning:high" && option.name == "★ High"
        }));
        assert!(groups[2].options.iter().any(|option| {
            option.value.0.as_ref() == "speed:fast" && option.name == "★ Fast"
        }));
    }

    #[test]
    fn model_selector_keeps_raw_display_names_without_formatting() {
        let account_status = super::AccountStatus::default();
        let mcp_summary = super::ContextSelectorSummary::default();
        let skills_summary = super::ContextSelectorSummary::default();
        let plugins_summary = super::ContextSelectorSummary::default();
        let selector_layout = SelectorLayoutPreferences::default();
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
            model_selector: &Default::default(),
            current_used_tokens: None,
            current_context_window_size: None,
            current_usage_percent: None,
            current_context_usage_source: None::<ContextUsageSource>,
            current_context_display: ContextControlDisplay::Context,
            current_context_display_style: ContextDisplayStyle::Percent,
            current_limits_display_style: LimitsDisplayStyle::Text,
            current_account_rate_limits: None,
            compaction_in_progress: false,
            approval: AppAskForApproval::OnRequest,
            sandbox: AppSandboxMode::WorkspaceWrite,
            collaboration_mode_kind: ModeKind::Default,
            account_status: &account_status,
            total_token_usage: None,
            session_mcp_summary: &mcp_summary,
            session_skills_summary: &skills_summary,
            session_plugins_summary: &plugins_summary,
            selector_layout: &selector_layout,
        });

        let model = options
            .iter()
            .find(|option| option.id.0.as_ref() == "model")
            .expect("model selector exists");
        assert_eq!(
            model.description.as_deref(),
            Some("GPT-5.5\nReasoning effort: High\nSpeed: Fast")
        );
        let SessionConfigKind::Select(select) = &model.kind else {
            panic!("model selector should be a select config option");
        };
        let SessionConfigSelectOptions::Grouped(groups) = &select.options else {
            panic!("model selector should use grouped options");
        };
        assert_eq!(groups[0].options[0].name, "GPT-5.5 High");
        assert!(
            groups[1].options.iter().any(
                |option| option.value.0.as_ref() == "reasoning:high" && option.name == "★ High"
            )
        );
    }

    #[test]
    fn model_selector_preferences_filter_models_and_efforts() {
        let account_status = super::AccountStatus::default();
        let mcp_summary = super::ContextSelectorSummary::default();
        let skills_summary = super::ContextSelectorSummary::default();
        let plugins_summary = super::ContextSelectorSummary::default();
        let selector_layout = SelectorLayoutPreferences::default();
        let model_selector = ModelSelectorPreferences {
            default_model: None,
            default_reasoning_effort: None,
            default_service_tier: None,
            models: vec![ModelSelectorModelEntry::Details(
                ModelSelectorModelDetails {
                    id: "gpt-5.5".to_string(),
                    name: Some("main".to_string()),
                    description: Some("Custom main model".to_string()),
                },
            )],
            reasoning_efforts: vec![ModelSelectorReasoningEffortEntry::Details(
                ModelSelectorReasoningEffortDetails {
                    id: ReasoningEffort::High,
                    name: Some("много".to_string()),
                    description: Some("Кастомное описание effort".to_string()),
                },
            )],
        };
        let models = vec![
            AppModel {
                id: "gpt-5.5".to_string(),
                model: "gpt-5.5".to_string(),
                upgrade: None,
                upgrade_info: None,
                availability_nux: None,
                display_name: "GPT-5.5".to_string(),
                description: "Frontier model".to_string(),
                hidden: false,
                supported_reasoning_efforts: vec![
                    ReasoningEffortOption {
                        reasoning_effort: ReasoningEffort::Low,
                        description: "Fast".to_string(),
                    },
                    ReasoningEffortOption {
                        reasoning_effort: ReasoningEffort::High,
                        description: "Deep".to_string(),
                    },
                    ReasoningEffortOption {
                        reasoning_effort: ReasoningEffort::XHigh,
                        description: "Max".to_string(),
                    },
                ],
                default_reasoning_effort: ReasoningEffort::High,
                input_modalities: Vec::new(),
                supports_personality: false,
                is_default: true,
            },
            AppModel {
                id: "gpt-5.2".to_string(),
                model: "gpt-5.2".to_string(),
                upgrade: None,
                upgrade_info: None,
                availability_nux: None,
                display_name: "gpt-5.2".to_string(),
                description: "Older model".to_string(),
                hidden: false,
                supported_reasoning_efforts: Vec::new(),
                default_reasoning_effort: ReasoningEffort::Medium,
                input_modalities: Vec::new(),
                supports_personality: false,
                is_default: false,
            },
        ];

        let options = config_options(ConfigOptionsInput {
            workspace_cwd: std::path::Path::new("/tmp"),
            models: &models,
            current_model: "gpt-5.5",
            current_service_tier: None,
            current_reasoning_effort: ReasoningEffort::High,
            model_selector: &model_selector,
            current_used_tokens: None,
            current_context_window_size: None,
            current_usage_percent: None,
            current_context_usage_source: None::<ContextUsageSource>,
            current_context_display: ContextControlDisplay::Context,
            current_context_display_style: ContextDisplayStyle::Percent,
            current_limits_display_style: LimitsDisplayStyle::Text,
            current_account_rate_limits: None,
            compaction_in_progress: false,
            approval: AppAskForApproval::OnRequest,
            sandbox: AppSandboxMode::WorkspaceWrite,
            collaboration_mode_kind: ModeKind::Default,
            account_status: &account_status,
            total_token_usage: None,
            session_mcp_summary: &mcp_summary,
            session_skills_summary: &skills_summary,
            session_plugins_summary: &plugins_summary,
            selector_layout: &selector_layout,
        });

        let model = options
            .iter()
            .find(|option| option.id.0.as_ref() == "model")
            .expect("model selector exists");
        let SessionConfigKind::Select(select) = &model.kind else {
            panic!("model selector should be a select config option");
        };
        let SessionConfigSelectOptions::Grouped(groups) = &select.options else {
            panic!("model selector should use grouped options");
        };
        assert_eq!(
            groups[0]
                .options
                .iter()
                .map(|option| option.value.0.as_ref())
                .collect::<Vec<_>>(),
            vec!["gpt-5.5"]
        );
        assert_eq!(groups[0].options[0].name, "main много");
        assert_eq!(
            groups[0].options[0].description.as_deref(),
            Some("Custom main model\nSelected: reasoning effort High, speed Standard.")
        );
        assert_eq!(
            groups[1]
                .options
                .iter()
                .map(|option| option.value.0.as_ref())
                .collect::<Vec<_>>(),
            vec!["reasoning:high"]
        );
        assert_eq!(groups[1].options[0].name, "★ много");
        assert_eq!(
            groups[1].options[0].description.as_deref(),
            Some("Кастомное описание effort")
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

    #[test]
    fn context_selector_current_value_tracks_display_preference() {
        let account_status = super::AccountStatus::default();
        let mcp_summary = super::ContextSelectorSummary::default();
        let skills_summary = super::ContextSelectorSummary::default();
        let plugins_summary = super::ContextSelectorSummary::default();
        let selector_layout = SelectorLayoutPreferences::default();
        let rate_limits = RateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: None,
            primary: Some(RateLimitWindow {
                used_percent: 20,
                window_duration_mins: Some(300),
                resets_at: None,
            }),
            secondary: Some(RateLimitWindow {
                used_percent: 6,
                window_duration_mins: Some(10_080),
                resets_at: None,
            }),
            credits: None,
            plan_type: Some(PlanType::Plus),
        };

        let options = config_options(ConfigOptionsInput {
            workspace_cwd: std::path::Path::new("/tmp"),
            models: &[],
            current_model: "gpt-5.5",
            current_service_tier: None,
            current_reasoning_effort: ReasoningEffort::High,
            model_selector: &Default::default(),
            current_used_tokens: Some(195_499),
            current_context_window_size: Some(258_400),
            current_usage_percent: Some(76),
            current_context_usage_source: Some(ContextUsageSource::Live),
            current_context_display: ContextControlDisplay::Limits,
            current_context_display_style: ContextDisplayStyle::Percent,
            current_limits_display_style: LimitsDisplayStyle::Text,
            current_account_rate_limits: Some(&rate_limits),
            compaction_in_progress: false,
            approval: AppAskForApproval::OnRequest,
            sandbox: AppSandboxMode::WorkspaceWrite,
            collaboration_mode_kind: ModeKind::Default,
            account_status: &account_status,
            total_token_usage: None,
            session_mcp_summary: &mcp_summary,
            session_skills_summary: &skills_summary,
            session_plugins_summary: &plugins_summary,
            selector_layout: &selector_layout,
        });

        let context = options
            .iter()
            .find(|option| option.id.0.as_ref() == "context_control")
            .expect("context selector exists");
        assert_eq!(context.name, "Context");
        assert!(context.description.as_deref().is_some_and(|description| {
            description.contains("Context: ⣶ 76%")
                && description.contains("Tokens: 195499/258400")
                && description.contains("Status: live")
                && description.contains("5h 80% · wk 94%")
                && description.contains("5-hour: resets -")
                && description.contains("Weekly: resets -")
        }));
        let SessionConfigKind::Select(select) = &context.kind else {
            panic!("context selector should be a select config option");
        };
        assert_eq!(select.current_value.0.as_ref(), "limits_status");
        let SessionConfigSelectOptions::Grouped(groups) = &select.options else {
            panic!("context selector should use grouped options");
        };
        assert!(
            groups
                .iter()
                .flat_map(|group| &group.options)
                .any(|option| option.value.0.as_ref() == "limits_status"
                    && option.name == "5h 80%")
        );
    }

    #[test]
    fn permissions_selector_embeds_plan_mode_without_losing_permission_state() {
        let account_status = super::AccountStatus::default();
        let mcp_summary = super::ContextSelectorSummary::default();
        let skills_summary = super::ContextSelectorSummary::default();
        let plugins_summary = super::ContextSelectorSummary::default();
        let selector_layout = SelectorLayoutPreferences::default();

        let options = config_options(ConfigOptionsInput {
            workspace_cwd: std::path::Path::new("/tmp"),
            models: &[],
            current_model: "gpt-5.5",
            current_service_tier: None,
            current_reasoning_effort: ReasoningEffort::High,
            model_selector: &Default::default(),
            current_used_tokens: Some(1_000),
            current_context_window_size: Some(2_000),
            current_usage_percent: Some(50),
            current_context_usage_source: Some(ContextUsageSource::Cached),
            current_context_display: ContextControlDisplay::Context,
            current_context_display_style: ContextDisplayStyle::Percent,
            current_limits_display_style: LimitsDisplayStyle::Text,
            current_account_rate_limits: None,
            compaction_in_progress: true,
            approval: AppAskForApproval::Never,
            sandbox: AppSandboxMode::ReadOnly,
            collaboration_mode_kind: ModeKind::Plan,
            account_status: &account_status,
            total_token_usage: None,
            session_mcp_summary: &mcp_summary,
            session_skills_summary: &skills_summary,
            session_plugins_summary: &plugins_summary,
            selector_layout: &selector_layout,
        });

        let permissions = options
            .iter()
            .find(|option| option.id.0.as_ref() == "permissions")
            .expect("permissions selector exists");
        assert!(
            permissions
                .description
                .as_deref()
                .is_some_and(|description| {
                    description.contains("Plan-first mode with visible step tracking.")
                        && description.contains("Permissions: Read-only sandbox.")
                })
        );
        let SessionConfigKind::Select(select) = &permissions.kind else {
            panic!("permissions selector should be a select config option");
        };
        assert_eq!(select.current_value.0.as_ref(), "plan");
        let SessionConfigSelectOptions::Grouped(groups) = &select.options else {
            panic!("permissions selector should use grouped options");
        };
        assert_eq!(
            groups
                .iter()
                .map(|group| group.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Workflow", "Guarded", "Bypass"]
        );
        assert!(
            groups[0]
                .options
                .iter()
                .any(|option| option.value.0.as_ref() == "plan" && option.name == "Plan")
        );
        assert!(groups[1].options.iter().any(|option| {
            option.value.0.as_ref() == "read-only" && option.name == "Read only"
        }));

        let context = options
            .iter()
            .find(|option| option.id.0.as_ref() == "context_control")
            .expect("context selector exists");
        assert!(context.description.as_deref().is_some_and(|description| {
            description.contains("Context: ⣤ 50%")
                && description.contains("Tokens: 1000/2000")
                && description.contains("Status: compacting")
                && description.contains("Context compaction is currently running.")
        }));
    }

    #[test]
    fn selector_layout_can_rename_reorder_hide_and_filter_groups() {
        let account_status = super::AccountStatus::default();
        let mcp_summary = super::ContextSelectorSummary::default();
        let skills_summary = super::ContextSelectorSummary::default();
        let plugins_summary = super::ContextSelectorSummary::default();
        let selector_layout = SelectorLayoutPreferences {
            order: Some(vec![
                "context_control".to_string(),
                "permissions".to_string(),
                "model".to_string(),
            ]),
            permissions: Some(SelectorLayoutEntry {
                visible: Some(true),
                name: Some("Mode".to_string()),
                groups: Some(vec!["guarded".to_string(), "workflow".to_string()]),
            }),
            model: Some(SelectorLayoutEntry {
                visible: Some(false),
                name: None,
                groups: None,
            }),
            context_control: Some(SelectorLayoutEntry {
                visible: Some(true),
                name: Some("Ctx".to_string()),
                groups: Some(vec!["actions".to_string(), "display".to_string()]),
            }),
        };

        let options = config_options(ConfigOptionsInput {
            workspace_cwd: std::path::Path::new("/tmp"),
            models: &[],
            current_model: "gpt-5.5",
            current_service_tier: None,
            current_reasoning_effort: ReasoningEffort::High,
            model_selector: &Default::default(),
            current_used_tokens: Some(1_000),
            current_context_window_size: Some(2_000),
            current_usage_percent: Some(50),
            current_context_usage_source: Some(ContextUsageSource::Cached),
            current_context_display: ContextControlDisplay::Context,
            current_context_display_style: ContextDisplayStyle::Percent,
            current_limits_display_style: LimitsDisplayStyle::Text,
            current_account_rate_limits: None,
            compaction_in_progress: false,
            approval: AppAskForApproval::OnRequest,
            sandbox: AppSandboxMode::WorkspaceWrite,
            collaboration_mode_kind: ModeKind::Default,
            account_status: &account_status,
            total_token_usage: None,
            session_mcp_summary: &mcp_summary,
            session_skills_summary: &skills_summary,
            session_plugins_summary: &plugins_summary,
            selector_layout: &selector_layout,
        });

        assert_eq!(
            options
                .iter()
                .map(|option| option.id.0.as_ref())
                .collect::<Vec<_>>(),
            vec!["context_control", "permissions"]
        );
        assert_eq!(options[0].name, "Ctx");
        let SessionConfigKind::Select(context_select) = &options[0].kind else {
            panic!("context selector should be a select config option");
        };
        let SessionConfigSelectOptions::Grouped(context_groups) = &context_select.options else {
            panic!("context selector should use grouped options");
        };
        assert_eq!(
            context_groups
                .iter()
                .map(|group| group.group.0.as_ref())
                .collect::<Vec<_>>(),
            vec!["actions", "display"]
        );

        assert_eq!(options[1].name, "Mode");
        let SessionConfigKind::Select(permissions_select) = &options[1].kind else {
            panic!("permissions selector should be a select config option");
        };
        let SessionConfigSelectOptions::Grouped(permission_groups) = &permissions_select.options
        else {
            panic!("permissions selector should use grouped options");
        };
        assert_eq!(
            permission_groups
                .iter()
                .map(|group| group.group.0.as_ref())
                .collect::<Vec<_>>(),
            vec!["guarded", "workflow"]
        );
    }
}
