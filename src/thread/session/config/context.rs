//! Context/status selector helper-ы для session_config.

use std::path::Path;

use codex_app_server_protocol::{Account, RateLimitSnapshot, TokenUsageBreakdown};
use codex_protocol::account::PlanType;

use super::limits::{
    combined_limits_reset_message, five_hour_reset_message, limits_status_description,
    weekly_reset_message,
};
use crate::thread::{
    SessionConfigSelectGroup, SessionConfigSelectOption,
    session_display_maps::{DisplayMapsConfig, LimitSummaryOption, LimitSummaryWindow},
};

#[path = "context/mcp.rs"]
mod mcp;
#[path = "context/plugins.rs"]
mod plugins;
#[path = "context/skills.rs"]
mod skills;

pub(in crate::thread) use mcp::build_mcp_summary;
pub(in crate::thread) use plugins::build_plugins_summary;
pub(in crate::thread) use skills::build_skills_summary;

pub(in crate::thread) const STATUS_LIMITS_VALUE: &str = "limits_status";
pub(in crate::thread) const SESSION_STATUS_VALUE: &str = "session_status";
pub(in crate::thread) const MCP_STATUS_VALUE: &str = "mcp_status";
pub(in crate::thread) const SKILLS_STATUS_VALUE: &str = "skills_status";
pub(in crate::thread) const PLUGINS_STATUS_VALUE: &str = "plugins_status";
pub(in crate::thread) const STATUS_COMPACT_VALUE: &str = "compact_now";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ContextSelectorSummary {
    pub(crate) label: String,
    pub(crate) description: String,
    pub(crate) report: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum AccountAuthKind {
    Chatgpt,
    ApiKey,
    AmazonBedrock,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AccountStatus {
    pub(crate) auth_kind: AccountAuthKind,
    pub(crate) email: Option<String>,
    pub(crate) plan_type: Option<PlanType>,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::thread) fn status_control_option_groups(
    rate_limits: Option<&RateLimitSnapshot>,
    display_maps: &DisplayMapsConfig,
    workspace_cwd: &Path,
    backend_cli_version: &str,
    account_status: &AccountStatus,
    compaction_in_progress: bool,
    mcp_summary: &ContextSelectorSummary,
    skills_summary: &ContextSelectorSummary,
    plugins_summary: &ContextSelectorSummary,
) -> Vec<SessionConfigSelectGroup> {
    let compact_label = if compaction_in_progress {
        "Compacting..."
    } else {
        "Compact now"
    };
    let primary_remaining_percent = primary_remaining(rate_limits);
    let secondary_remaining_percent = secondary_remaining(rate_limits);
    let mut status_options = display_maps
        .limits_summary_options()
        .iter()
        .map(|option| {
            SessionConfigSelectOption::new(
                limits_summary_value(&option.id),
                display_maps.render_limits_summary_option(
                    option,
                    primary_remaining_percent,
                    secondary_remaining_percent,
                ),
            )
            .description(limit_summary_option_description(
                option,
                rate_limits,
                display_maps,
            ))
        })
        .collect::<Vec<_>>();
    status_options.push(
        SessionConfigSelectOption::new(SESSION_STATUS_VALUE, "Session").description(
            session_status_description(
                workspace_cwd,
                backend_cli_version,
                account_status,
                rate_limits,
            ),
        ),
    );
    vec![
        SessionConfigSelectGroup::new("status", "Status", status_options),
        SessionConfigSelectGroup::new(
            "integrations",
            "Integrations",
            vec![
                SessionConfigSelectOption::new(MCP_STATUS_VALUE, &mcp_summary.label)
                    .description(mcp_summary.description.clone()),
                SessionConfigSelectOption::new(SKILLS_STATUS_VALUE, &skills_summary.label)
                    .description(skills_summary.description.clone()),
                SessionConfigSelectOption::new(PLUGINS_STATUS_VALUE, &plugins_summary.label)
                    .description(plugins_summary.description.clone()),
            ],
        ),
        SessionConfigSelectGroup::new(
            "actions",
            "Actions",
            vec![
                SessionConfigSelectOption::new(STATUS_COMPACT_VALUE, compact_label)
                    .description("Summarize the conversation to free context window"),
            ],
        ),
    ]
}

pub(in crate::thread) fn status_current_value(
    display_maps: &DisplayMapsConfig,
    compaction_in_progress: bool,
) -> String {
    if compaction_in_progress {
        return STATUS_COMPACT_VALUE.to_string();
    }

    limits_summary_value(display_maps.selected_limits_summary_option_id())
}

pub(in crate::thread) fn status_selector_description(
    rate_limits: Option<&RateLimitSnapshot>,
    display_maps: &DisplayMapsConfig,
) -> String {
    let selected = display_maps
        .limits_summary_options()
        .iter()
        .find(|option| option.id == display_maps.selected_limits_summary_option_id());

    selected
        .map(|option| limit_summary_option_description(option, rate_limits, display_maps))
        .unwrap_or_else(|| combined_limits_reset_message(rate_limits))
}

pub(in crate::thread) fn limits_summary_value(option_id: &str) -> String {
    format!("limits_summary:{option_id}")
}

pub(in crate::thread) fn parse_limits_summary_value(value: &str) -> Option<&str> {
    value.strip_prefix("limits_summary:")
}

fn limit_summary_option_description(
    option: &LimitSummaryOption,
    rate_limits: Option<&RateLimitSnapshot>,
    display_maps: &DisplayMapsConfig,
) -> String {
    if let Some(description) = option
        .description
        .as_deref()
        .filter(|description| !description.trim().is_empty())
    {
        return description.to_string();
    }

    let reset_lines = option
        .summary
        .iter()
        .map(|window| match window {
            LimitSummaryWindow::Primary => five_hour_reset_message(rate_limits),
            LimitSummaryWindow::Secondary => weekly_reset_message(rate_limits),
        })
        .collect::<Vec<_>>();

    [
        display_maps.render_limits_summary_for_windows(
            &option.summary,
            primary_remaining(rate_limits),
            secondary_remaining(rate_limits),
        ),
        reset_lines.join("\n"),
    ]
    .join("\n")
}

fn primary_remaining(rate_limits: Option<&RateLimitSnapshot>) -> Option<i32> {
    rate_limits
        .and_then(|snapshot| snapshot.primary.as_ref())
        .map(|window| 100 - window.used_percent.clamp(0, 100))
}

fn secondary_remaining(rate_limits: Option<&RateLimitSnapshot>) -> Option<i32> {
    rate_limits
        .and_then(|snapshot| snapshot.secondary.as_ref())
        .map(|window| 100 - window.used_percent.clamp(0, 100))
}

#[allow(clippy::too_many_arguments)]
pub(in crate::thread) fn full_status_report(
    workspace_cwd: &Path,
    backend_cli_version: &str,
    account_status: &AccountStatus,
    total_token_usage: Option<&TokenUsageBreakdown>,
    rate_limits: Option<&RateLimitSnapshot>,
    display_maps: &DisplayMapsConfig,
    mcp_summary: &ContextSelectorSummary,
    skills_summary: &ContextSelectorSummary,
    plugins_summary: &ContextSelectorSummary,
) -> String {
    let sections = [
        format!(
            "Limits\n{}",
            limits_status_description(rate_limits, display_maps)
        ),
        format!(
            "Session\n{}",
            session_status_detail(
                workspace_cwd,
                backend_cli_version,
                account_status,
                total_token_usage,
                rate_limits
            )
        ),
        format!(
            "Integrations\n\n{}\n\n{}\n\n{}",
            mcp_summary.report, skills_summary.report, plugins_summary.report
        ),
    ];

    sections.join("\n\n")
}

fn session_status_detail(
    workspace_cwd: &Path,
    backend_cli_version: &str,
    account_status: &AccountStatus,
    total_token_usage: Option<&TokenUsageBreakdown>,
    rate_limits: Option<&RateLimitSnapshot>,
) -> String {
    format!(
        "{}\nTokens: {}",
        session_status_description(
            workspace_cwd,
            backend_cli_version,
            account_status,
            rate_limits,
        ),
        token_usage_summary_line(total_token_usage)
    )
}

pub(in crate::thread) fn session_status_description(
    workspace_cwd: &Path,
    backend_cli_version: &str,
    account_status: &AccountStatus,
    rate_limits: Option<&RateLimitSnapshot>,
) -> String {
    format!(
        "Adapter: codex-acp-cas {}\nBackend: {}\nWorkspace: {}\nAccount: {}",
        env!("CARGO_PKG_VERSION"),
        backend_version_detail(backend_cli_version),
        workspace_cwd.display(),
        account_detail(account_status, rate_limits),
    )
}

pub(in crate::thread) fn build_account_status(account: Option<Account>) -> AccountStatus {
    match account {
        Some(Account::Chatgpt { email, plan_type }) => AccountStatus {
            auth_kind: AccountAuthKind::Chatgpt,
            email,
            plan_type: Some(plan_type),
        },
        Some(Account::ApiKey {}) => AccountStatus {
            auth_kind: AccountAuthKind::ApiKey,
            email: None,
            plan_type: None,
        },
        Some(Account::AmazonBedrock { .. }) => AccountStatus {
            auth_kind: AccountAuthKind::AmazonBedrock,
            email: None,
            plan_type: None,
        },
        None => AccountStatus::default(),
    }
}

fn backend_version_detail(backend_cli_version: &str) -> String {
    let version = backend_cli_version.trim();
    if version.is_empty() {
        "codex unknown".to_string()
    } else {
        format!("codex {version}")
    }
}

fn account_detail(
    account_status: &AccountStatus,
    rate_limits: Option<&RateLimitSnapshot>,
) -> String {
    let plan = effective_plan_type(account_status, rate_limits).map(plan_type_label);
    match account_status.auth_kind {
        AccountAuthKind::Chatgpt => match (&account_status.email, plan) {
            (Some(email), Some(plan)) => format!("{email} · ChatGPT {plan}"),
            (Some(email), None) => format!("{email} · ChatGPT"),
            (None, Some(plan)) => format!("ChatGPT {plan}"),
            (None, None) => "ChatGPT".to_string(),
        },
        AccountAuthKind::ApiKey => plan
            .map(|plan| format!("API key auth · {plan}"))
            .unwrap_or_else(|| "API key auth".to_string()),
        AccountAuthKind::AmazonBedrock => "Amazon Bedrock auth".to_string(),
        AccountAuthKind::Unknown => plan
            .map(|plan| format!("Account details unavailable · {plan}"))
            .unwrap_or_else(|| "Account details unavailable".to_string()),
    }
}

fn effective_plan_type(
    account_status: &AccountStatus,
    rate_limits: Option<&RateLimitSnapshot>,
) -> Option<PlanType> {
    account_status
        .plan_type
        .or_else(|| rate_limits.and_then(|snapshot| snapshot.plan_type))
}

fn plan_type_label(plan_type: PlanType) -> &'static str {
    match plan_type {
        PlanType::Free => "Free",
        PlanType::Go => "Go",
        PlanType::Plus => "Plus",
        PlanType::Pro => "Pro",
        PlanType::ProLite => "Pro Lite",
        PlanType::Team => "Team",
        PlanType::SelfServeBusinessUsageBased => "Business",
        PlanType::Business => "Business",
        PlanType::EnterpriseCbpUsageBased => "Enterprise",
        PlanType::Enterprise => "Enterprise",
        PlanType::Edu => "Edu",
        PlanType::Unknown => "Unknown",
    }
}

fn token_usage_summary_line(total_token_usage: Option<&TokenUsageBreakdown>) -> String {
    total_token_usage
        .map(|usage| {
            [
                format!(
                    "{} used",
                    format_compact_token_count(blended_total_tokens(usage))
                ),
                format!(
                    "{} in",
                    format_compact_token_count(non_cached_input_tokens(usage))
                ),
                format!("{} out", format_compact_token_count(usage.output_tokens)),
            ]
            .join(" · ")
        })
        .unwrap_or_else(|| "waiting for usage update".to_string())
}

fn non_cached_input_tokens(usage: &TokenUsageBreakdown) -> i64 {
    (usage.input_tokens - usage.cached_input_tokens.max(0)).max(0)
}

fn blended_total_tokens(usage: &TokenUsageBreakdown) -> i64 {
    (non_cached_input_tokens(usage) + usage.output_tokens.max(0)).max(0)
}

fn format_compact_token_count(value: i64) -> String {
    let value = value.max(0) as f64;
    if value >= 1_000_000_000.0 {
        return format_compact_with_suffix(value / 1_000_000_000.0, "B");
    }
    if value >= 1_000_000.0 {
        return format_compact_with_suffix(value / 1_000_000.0, "M");
    }
    if value >= 1_000.0 {
        return format_compact_with_suffix(value / 1_000.0, "K");
    }
    format!("{:.0}", value)
}

fn format_compact_with_suffix(value: f64, suffix: &str) -> String {
    let formatted = if value < 10.0 {
        format!("{value:.2}")
    } else if value < 100.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    };
    let trimmed = if formatted.contains('.') {
        formatted.trim_end_matches('0').trim_end_matches('.')
    } else {
        formatted.as_str()
    };
    format!("{trimmed}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::{
        AccountStatus, build_mcp_summary, build_plugins_summary, build_skills_summary,
        full_status_report,
    };
    use codex_app_server_protocol::{PluginListResponse, SkillsListResponse};
    use codex_config::{
        DEFAULT_MCP_SERVER_ENVIRONMENT_ID, McpServerConfig, McpServerTransportConfig,
    };
    use codex_utils_path_uri::LegacyAppPathString;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    fn summary(label: &str, description: &str, report: &str) -> super::ContextSelectorSummary {
        super::ContextSelectorSummary {
            label: label.to_string(),
            description: description.to_string(),
            report: report.to_string(),
        }
    }

    #[test]
    fn mcp_summary_includes_server_count() {
        let mut servers = HashMap::new();
        servers.insert(
            "local_files".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::Stdio {
                    command: "/bin/mcp".to_string(),
                    args: vec!["--root".to_string(), "/tmp".to_string()],
                    env: None,
                    env_vars: vec![],
                    cwd: Some(LegacyAppPathString::from_path(Path::new("/tmp/workspace"))),
                },
                auth: Default::default(),
                environment_id: DEFAULT_MCP_SERVER_ENVIRONMENT_ID.to_string(),
                enabled: true,
                required: false,
                supports_parallel_tool_calls: false,
                disabled_reason: None,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                default_tools_approval_mode: None,
                enabled_tools: None,
                disabled_tools: None,
                scopes: None,
                oauth: None,
                oauth_resource: None,
                tools: HashMap::new(),
            },
        );

        let summary = build_mcp_summary(&servers);
        assert_eq!(summary.label, "MCP · 1 srv");
        assert!(summary.description.contains("1 MCP server(s)"));
        assert!(summary.report.contains("local_files"));
    }

    #[test]
    fn skills_summary_counts_enabled_and_errors() {
        let response: SkillsListResponse = serde_json::from_value(serde_json::json!({
            "data": [{
                "cwd": "/tmp/workspace",
                "skills": [{
                    "name": "frontend-skill",
                    "description": "Design a strong landing page",
                    "shortDescription": "Bold UI work",
                    "path": "/tmp/skills/frontend/SKILL.md",
                    "scope": "user",
                    "enabled": true
                }],
                "errors": [{
                    "path": "/tmp/skills/broken/SKILL.md",
                    "message": "invalid frontmatter"
                }]
            }]
        }))
        .expect("valid skills list response");

        let summary = build_skills_summary(&response);
        assert_eq!(summary.label, "Skills · 1 on · 1 err");
        assert!(summary.description.contains("1/1 skill(s) enabled"));
        assert!(summary.report.contains("frontend-skill"));
        assert!(summary.report.contains("invalid frontmatter"));
    }

    #[test]
    fn plugins_summary_counts_enabled_and_available_plugins() {
        let plugin = |id: &str,
                      display_name: &str,
                      description: &str,
                      category: &str,
                      capabilities: Vec<&str>,
                      installed: bool,
                      enabled: bool| {
            serde_json::json!({
                "id": id,
                "name": id,
                "source": {
                    "type": "local",
                    "path": format!("/tmp/plugins/{id}")
                },
                "installed": installed,
                "enabled": enabled,
                "installPolicy": "AVAILABLE",
                "authPolicy": "ON_USE",
                "interface": {
                    "displayName": display_name,
                    "shortDescription": description,
                    "developerName": (id == "github").then_some("OpenAI"),
                    "category": category,
                    "capabilities": capabilities,
                    "screenshots": [],
                    "screenshotUrls": []
                }
            })
        };
        let response: PluginListResponse = serde_json::from_value(serde_json::json!({
            "marketplaces": [{
                "name": "openai-curated",
                "path": "/tmp/plugins/marketplace.json",
                "plugins": [
                    plugin(
                        "github",
                        "GitHub",
                        "Issues and pull requests",
                        "Engineering",
                        vec!["skills", "github-mcp"],
                        true,
                        true,
                    ),
                    plugin(
                        "slack",
                        "Slack",
                        "Workspace messaging",
                        "Communication",
                        vec!["connector"],
                        true,
                        false,
                    ),
                    plugin(
                        "figma",
                        "Figma",
                        "Design workflows",
                        "Design",
                        vec!["skills"],
                        false,
                        false,
                    )
                ]
            }],
            "marketplaceLoadErrors": [{
                "marketplacePath": "/tmp/plugins/broken-marketplace.json",
                "message": "network unavailable"
            }]
        }))
        .expect("valid plugin list response");

        let summary = build_plugins_summary(&response);
        assert_eq!(summary.label, "Plugins · 1 on · 1 off · 1 load err");
        assert!(
            summary
                .description
                .contains("1/2 installed plugin(s) enabled")
        );
        assert!(summary.description.contains("1 marketplace load error(s)"));
        assert!(summary.report.contains("GitHub"));
        assert!(summary.report.contains("Available but not installed"));
        assert!(summary.report.contains("Marketplace load errors:"));
        assert!(summary.report.contains("network unavailable"));
    }

    #[test]
    fn full_status_report_includes_limits_session_and_integrations() {
        let report = full_status_report(
            PathBuf::from("/tmp/workspace").as_path(),
            "0.0.0",
            &AccountStatus::default(),
            None,
            None,
            &Default::default(),
            &summary("MCP · 1 srv", "one", "MCP report"),
            &summary("Skills · 1 on", "one", "Skills report"),
            &summary("Plugins · none", "none", "Plugins report"),
        );

        assert!(report.starts_with("Limits\n"));
        assert!(report.contains("Limits\n5h -- · wk --"));
        assert!(report.contains("Session\nAdapter: codex-acp-cas "));
        assert!(report.contains("Backend: codex 0.0.0"));
        assert!(report.contains("Workspace: /tmp/workspace"));
        assert!(report.contains("Integrations\n\nMCP report"));
        assert!(report.contains("MCP report"));
        assert!(report.contains("Skills report"));
        assert!(report.contains("Plugins report"));

        let limits_index = report.find("Limits\n").unwrap();
        let session_index = report.find("\n\nSession\n").unwrap();
        let integrations_index = report.find("\n\nIntegrations\n").unwrap();
        assert!(limits_index < session_index);
        assert!(session_index < integrations_index);
    }

    #[test]
    fn full_status_report_omits_context_usage_display() {
        let report = full_status_report(
            PathBuf::from("/tmp/workspace").as_path(),
            "0.0.0",
            &AccountStatus::default(),
            None,
            None,
            &Default::default(),
            &summary("MCP · none", "none", "MCP report"),
            &summary("Skills · none", "none", "Skills report"),
            &summary("Plugins · none", "none", "Plugins report"),
        );

        assert!(!report.contains("Context\n"));
    }
}
