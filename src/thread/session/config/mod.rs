//! Маппинг конфигурации сессии между ACP-опциями и runtime-настройками Codex app-server.

use crate::thread::session_selector_preferences::{
    ModelSelectorPreferences, SelectorLayoutPreferences,
};
use crate::thread::{
    AppAskForApproval, AppModel, AppSandboxMode, ModeKind, PLAN_SESSION_MODE_ID, ReasoningEffort,
    ServiceTier, SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectGroup,
    SessionConfigSelectOption, ThreadInner,
};

#[path = "context.rs"]
mod context;
#[path = "fast_mode.rs"]
mod fast_mode;
#[path = "layout.rs"]
mod layout;
#[path = "limits.rs"]
mod limits;
#[path = "model_selector.rs"]
mod model_selector;
#[path = "modes.rs"]
mod modes;
#[path = "reasoning.rs"]
mod reasoning;

use self::layout::{apply_group_layout, apply_selector_order, selector_name};
use self::model_selector::{
    ModelOptionGroupsInput, model_option_groups, model_selector_description,
};
pub(super) use modes::{
    current_permission_mode_id, i64_to_u64_saturating, mode_state, permission_modes,
    policy_to_mode, to_app_approval, to_app_sandbox_mode,
};

#[derive(Clone, Copy, Debug)]
// Входные параметры для сборки списка session config options.
pub(in crate::thread) struct ConfigOptionsInput<'a> {
    pub(in crate::thread) workspace_cwd: &'a std::path::Path,
    pub(in crate::thread) backend_cli_version: &'a str,
    pub(in crate::thread) models: &'a [AppModel],
    pub(in crate::thread) current_model: &'a str,
    pub(in crate::thread) current_service_tier: Option<ServiceTier>,
    pub(in crate::thread) current_reasoning_effort: ReasoningEffort,
    pub(in crate::thread) model_selector: &'a ModelSelectorPreferences,
    pub(in crate::thread) display_maps: &'a crate::thread::session_display_maps::DisplayMapsConfig,
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
        backend_cli_version: &inner.backend_cli_version,
        models: &inner.models,
        current_model: &inner.current_model,
        current_service_tier: inner.service_tier,
        current_reasoning_effort: inner.reasoning_effort,
        model_selector: &inner.model_selector,
        display_maps: &inner.display_maps,
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
        backend_cli_version,
        models,
        current_model,
        current_service_tier,
        current_reasoning_effort,
        model_selector,
        display_maps,
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
        "status",
        SessionConfigOption::select(
            "status",
            selector_name(selector_layout, "status", "Status"),
            if compaction_in_progress {
                context::STATUS_COMPACT_VALUE.to_string()
            } else {
                context::STATUS_LIMITS_VALUE.to_string()
            },
            apply_group_layout(
                selector_layout,
                "status",
                context::status_control_option_groups(
                    workspace_cwd,
                    backend_cli_version,
                    account_status,
                    total_token_usage,
                    current_account_rate_limits,
                    display_maps,
                    compaction_in_progress,
                    session_mcp_summary,
                    session_skills_summary,
                    session_plugins_summary,
                ),
            ),
        )
        .description(context::full_status_report(
            workspace_cwd,
            backend_cli_version,
            account_status,
            total_token_usage,
            current_account_rate_limits,
            display_maps,
            session_mcp_summary,
            session_skills_summary,
            session_plugins_summary,
        )),
    ));

    apply_selector_order(selector_layout, options)
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

pub(super) use context::{
    AccountStatus, ContextSelectorSummary, MCP_STATUS_VALUE, PLUGINS_STATUS_VALUE,
    SESSION_STATUS_VALUE, SKILLS_STATUS_VALUE, STATUS_COMPACT_VALUE, STATUS_LIMITS_VALUE,
    build_account_status, build_mcp_summary, build_plugins_summary, build_skills_summary,
    full_status_report,
};
pub(super) use fast_mode::{service_tier_override_from_config, service_tier_override_from_session};
pub(super) use limits::{
    RateLimitWarning, RateLimitWarningState, observe_rate_limit_snapshot, take_rate_limit_warnings,
};
pub(super) use model_selector::{parse_model_reasoning_value, parse_model_speed_value};
#[cfg(test)]
pub(super) use reasoning::parse_reasoning_effort;
pub(super) use reasoning::{
    find_model_for_current, normalize_reasoning_effort_for_model, reasoning_effort_value,
    resolve_reasoning_effort,
};

#[cfg(test)]
mod tests {
    use super::model_selector::{model_reasoning_value, model_speed_value};
    use super::{
        ConfigOptionsInput, config_options, parse_model_reasoning_value, parse_model_speed_value,
    };
    use crate::thread::session_selector_preferences::{
        ModelSelectorModelDetails, ModelSelectorModelEntry, ModelSelectorPreferences,
        ModelSelectorReasoningEffortDetails, ModelSelectorReasoningEffortEntry,
        SelectorLayoutEntry, SelectorLayoutPreferences,
    };
    use crate::thread::{
        AppAskForApproval, AppModel, AppSandboxMode, ModeKind, ReasoningEffort, ServiceTier,
    };
    use agent_client_protocol::schema::v1::{
        SessionConfigKind, SessionConfigOptionCategory, SessionConfigSelectOptions,
    };
    use codex_app_server_protocol::ReasoningEffortOption;

    #[test]
    fn model_selector_uses_backend_model_names_by_default() {
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
            models: &models,
            current_model: "gpt-5.5",
            current_service_tier: Some(ServiceTier::Fast),
            current_reasoning_effort: ReasoningEffort::High,
            model_selector: &Default::default(),
            workspace_cwd: std::path::Path::new("/tmp/workspace"),
            backend_cli_version: "0.0.0",
            display_maps: &Default::default(),
            current_account_rate_limits: None,
            compaction_in_progress: false,
            account_status: &Default::default(),
            total_token_usage: None,
            session_mcp_summary: &Default::default(),
            session_skills_summary: &Default::default(),
            session_plugins_summary: &Default::default(),
            approval: AppAskForApproval::OnRequest,
            sandbox: AppSandboxMode::WorkspaceWrite,
            collaboration_mode_kind: ModeKind::Default,
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

        let status = options
            .iter()
            .find(|option| option.id.0.as_ref() == "status")
            .expect("status selector exists");
        let SessionConfigKind::Select(select) = &status.kind else {
            panic!("status selector should be a select config option");
        };
        assert_eq!(select.current_value.0.as_ref(), "limits_status");
        let SessionConfigSelectOptions::Grouped(groups) = &select.options else {
            panic!("status selector should use grouped options");
        };
        assert_eq!(
            groups
                .iter()
                .map(|group| group.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Status", "Integrations", "Actions"]
        );
        assert!(groups[0].options.iter().any(|option| {
            option.value.0.as_ref() == "limits_status" && option.name == "5h -- · wk --"
        }));
        assert!(groups[2].options.iter().any(|option| {
            option.value.0.as_ref() == "compact_now" && option.name == "Compact now"
        }));
    }

    #[test]
    fn model_selector_keeps_raw_display_names_without_formatting() {
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
            models: &models,
            current_model: "gpt-5.5",
            current_service_tier: Some(ServiceTier::Fast),
            current_reasoning_effort: ReasoningEffort::High,
            model_selector: &Default::default(),
            workspace_cwd: std::path::Path::new("/tmp/workspace"),
            backend_cli_version: "0.0.0",
            display_maps: &Default::default(),
            current_account_rate_limits: None,
            compaction_in_progress: false,
            account_status: &Default::default(),
            total_token_usage: None,
            session_mcp_summary: &Default::default(),
            session_skills_summary: &Default::default(),
            session_plugins_summary: &Default::default(),
            approval: AppAskForApproval::OnRequest,
            sandbox: AppSandboxMode::WorkspaceWrite,
            collaboration_mode_kind: ModeKind::Default,
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
            models: &models,
            current_model: "gpt-5.5",
            current_service_tier: None,
            current_reasoning_effort: ReasoningEffort::High,
            model_selector: &model_selector,
            workspace_cwd: std::path::Path::new("/tmp/workspace"),
            backend_cli_version: "0.0.0",
            display_maps: &Default::default(),
            current_account_rate_limits: None,
            compaction_in_progress: false,
            account_status: &Default::default(),
            total_token_usage: None,
            session_mcp_summary: &Default::default(),
            session_skills_summary: &Default::default(),
            session_plugins_summary: &Default::default(),
            approval: AppAskForApproval::OnRequest,
            sandbox: AppSandboxMode::WorkspaceWrite,
            collaboration_mode_kind: ModeKind::Default,
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
    fn permissions_selector_embeds_plan_mode_without_losing_permission_state() {
        let selector_layout = SelectorLayoutPreferences::default();

        let options = config_options(ConfigOptionsInput {
            models: &[],
            current_model: "gpt-5.5",
            current_service_tier: None,
            current_reasoning_effort: ReasoningEffort::High,
            model_selector: &Default::default(),
            workspace_cwd: std::path::Path::new("/tmp/workspace"),
            backend_cli_version: "0.0.0",
            display_maps: &Default::default(),
            current_account_rate_limits: None,
            compaction_in_progress: false,
            account_status: &Default::default(),
            total_token_usage: None,
            session_mcp_summary: &Default::default(),
            session_skills_summary: &Default::default(),
            session_plugins_summary: &Default::default(),
            approval: AppAskForApproval::Never,
            sandbox: AppSandboxMode::ReadOnly,
            collaboration_mode_kind: ModeKind::Plan,
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
    }

    #[test]
    fn selector_layout_can_rename_reorder_hide_and_filter_groups() {
        let selector_layout = SelectorLayoutPreferences {
            order: Some(vec!["permissions".to_string(), "model".to_string()]),
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
            status: Some(SelectorLayoutEntry {
                visible: Some(false),
                name: None,
                groups: None,
            }),
        };

        let options = config_options(ConfigOptionsInput {
            models: &[],
            current_model: "gpt-5.5",
            current_service_tier: None,
            current_reasoning_effort: ReasoningEffort::High,
            model_selector: &Default::default(),
            workspace_cwd: std::path::Path::new("/tmp/workspace"),
            backend_cli_version: "0.0.0",
            display_maps: &Default::default(),
            current_account_rate_limits: None,
            compaction_in_progress: false,
            account_status: &Default::default(),
            total_token_usage: None,
            session_mcp_summary: &Default::default(),
            session_skills_summary: &Default::default(),
            session_plugins_summary: &Default::default(),
            approval: AppAskForApproval::OnRequest,
            sandbox: AppSandboxMode::WorkspaceWrite,
            collaboration_mode_kind: ModeKind::Default,
            selector_layout: &selector_layout,
        });

        assert_eq!(
            options
                .iter()
                .map(|option| option.id.0.as_ref())
                .collect::<Vec<_>>(),
            vec!["permissions"]
        );

        assert_eq!(options[0].name, "Mode");
        let SessionConfigKind::Select(permissions_select) = &options[0].kind else {
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
