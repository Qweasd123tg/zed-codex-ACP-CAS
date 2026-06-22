//! Context/status selector helper-ы для session_config.

use std::path::Path;

use codex_app_server_protocol::{Account, RateLimitSnapshot, TokenUsageBreakdown};
use codex_protocol::account::PlanType;

use super::limits::{combined_limits_status_label, limits_status_description};
use crate::thread::{
    SessionConfigSelectGroup, SessionConfigSelectOption, session_display_maps::DisplayMapsConfig,
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
    workspace_cwd: &Path,
    backend_cli_version: &str,
    account_status: &AccountStatus,
    total_token_usage: Option<&TokenUsageBreakdown>,
    rate_limits: Option<&RateLimitSnapshot>,
    display_maps: &DisplayMapsConfig,
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
    vec![
        SessionConfigSelectGroup::new(
            "status",
            "Status",
            vec![
                SessionConfigSelectOption::new(
                    STATUS_LIMITS_VALUE,
                    combined_limits_status_label(rate_limits, display_maps),
                )
                .description(limits_status_description(rate_limits, display_maps)),
                SessionConfigSelectOption::new(SESSION_STATUS_VALUE, "Session").description(
                    session_status_detail(
                        workspace_cwd,
                        backend_cli_version,
                        account_status,
                        total_token_usage,
                        rate_limits,
                    ),
                ),
            ],
        ),
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
        "Adapter: codex-acp-cas {}\nBackend: {}\nWorkspace: {}\nAccount: {}\nTokens: {}",
        env!("CARGO_PKG_VERSION"),
        backend_version_detail(backend_cli_version),
        workspace_cwd.display(),
        account_detail(account_status, rate_limits),
        token_usage_summary_line(total_token_usage)
    )
}

pub(in crate::thread) fn build_account_status(account: Option<Account>) -> AccountStatus {
    match account {
        Some(Account::Chatgpt { email, plan_type }) => AccountStatus {
            auth_kind: AccountAuthKind::Chatgpt,
            email: Some(email),
            plan_type: Some(plan_type),
        },
        Some(Account::ApiKey {}) => AccountStatus {
            auth_kind: AccountAuthKind::ApiKey,
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
        PlanType::Team => "Team",
        PlanType::Business => "Business",
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
    use codex_app_server_protocol::{
        PluginInterface, PluginListResponse, PluginMarketplaceEntry, PluginSource, PluginSummary,
    };
    use codex_core::config::types::{McpServerConfig, McpServerTransportConfig};
    use codex_core::skills::{SkillLoadOutcome, SkillMetadata};
    use codex_protocol::protocol::SkillScope;
    use std::collections::{HashMap, HashSet};
    use std::convert::TryInto;
    use std::path::PathBuf;

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
                    cwd: Some(PathBuf::from("/tmp/workspace")),
                },
                enabled: true,
                required: false,
                disabled_reason: None,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
                scopes: None,
                oauth_resource: None,
            },
        );

        let summary = build_mcp_summary(&servers);
        assert_eq!(summary.label, "MCP · 1 srv");
        assert!(summary.description.contains("1 MCP server(s)"));
        assert!(summary.report.contains("local_files"));
    }

    #[test]
    fn skills_summary_counts_enabled_and_errors() {
        let mut outcome = SkillLoadOutcome::default();
        outcome.skills = vec![SkillMetadata {
            name: "frontend-skill".to_string(),
            description: "Design a strong landing page".to_string(),
            short_description: Some("Bold UI work".to_string()),
            interface: None,
            dependencies: None,
            policy: None,
            permission_profile: None,
            path_to_skills_md: PathBuf::from("/tmp/skills/frontend/SKILL.md"),
            scope: SkillScope::User,
        }];
        outcome.errors = vec![codex_core::skills::SkillError {
            path: PathBuf::from("/tmp/skills/broken/SKILL.md"),
            message: "invalid frontmatter".to_string(),
        }];
        outcome.disabled_paths = HashSet::new();

        let summary = build_skills_summary(&outcome);
        assert_eq!(summary.label, "Skills · 1 on · 1 err");
        assert!(summary.description.contains("1/1 skill(s) enabled"));
        assert!(summary.report.contains("frontend-skill"));
        assert!(summary.report.contains("invalid frontmatter"));
    }

    #[test]
    fn plugins_summary_counts_enabled_and_available_plugins() {
        let response = PluginListResponse {
            marketplaces: vec![PluginMarketplaceEntry {
                name: "openai-curated".to_string(),
                path: PathBuf::from("/tmp/plugins/marketplace.json")
                    .try_into()
                    .expect("absolute marketplace path"),
                plugins: vec![
                    PluginSummary {
                        id: "github".to_string(),
                        name: "github".to_string(),
                        source: PluginSource::Local {
                            path: PathBuf::from("/tmp/plugins/github")
                                .try_into()
                                .expect("absolute plugin path"),
                        },
                        installed: true,
                        enabled: true,
                        interface: Some(PluginInterface {
                            display_name: Some("GitHub".to_string()),
                            short_description: Some("Issues and pull requests".to_string()),
                            long_description: None,
                            developer_name: Some("OpenAI".to_string()),
                            category: Some("Engineering".to_string()),
                            capabilities: vec!["skills".to_string(), "github-mcp".to_string()],
                            website_url: None,
                            privacy_policy_url: None,
                            terms_of_service_url: None,
                            default_prompt: None,
                            brand_color: None,
                            composer_icon: None,
                            logo: None,
                            screenshots: vec![],
                        }),
                    },
                    PluginSummary {
                        id: "slack".to_string(),
                        name: "slack".to_string(),
                        source: PluginSource::Local {
                            path: PathBuf::from("/tmp/plugins/slack")
                                .try_into()
                                .expect("absolute plugin path"),
                        },
                        installed: true,
                        enabled: false,
                        interface: Some(PluginInterface {
                            display_name: Some("Slack".to_string()),
                            short_description: Some("Workspace messaging".to_string()),
                            long_description: None,
                            developer_name: None,
                            category: Some("Communication".to_string()),
                            capabilities: vec!["connector".to_string()],
                            website_url: None,
                            privacy_policy_url: None,
                            terms_of_service_url: None,
                            default_prompt: None,
                            brand_color: None,
                            composer_icon: None,
                            logo: None,
                            screenshots: vec![],
                        }),
                    },
                    PluginSummary {
                        id: "figma".to_string(),
                        name: "figma".to_string(),
                        source: PluginSource::Local {
                            path: PathBuf::from("/tmp/plugins/figma")
                                .try_into()
                                .expect("absolute plugin path"),
                        },
                        installed: false,
                        enabled: false,
                        interface: Some(PluginInterface {
                            display_name: Some("Figma".to_string()),
                            short_description: Some("Design workflows".to_string()),
                            long_description: None,
                            developer_name: None,
                            category: Some("Design".to_string()),
                            capabilities: vec!["skills".to_string()],
                            website_url: None,
                            privacy_policy_url: None,
                            terms_of_service_url: None,
                            default_prompt: None,
                            brand_color: None,
                            composer_icon: None,
                            logo: None,
                            screenshots: vec![],
                        }),
                    },
                ],
            }],
            remote_sync_error: Some("network unavailable".to_string()),
        };

        let summary = build_plugins_summary(&response);
        assert_eq!(summary.label, "Plugins · 1 on · 1 off · sync err");
        assert!(
            summary
                .description
                .contains("1/2 installed plugin(s) enabled")
        );
        assert!(summary.description.contains("remote sync unavailable"));
        assert!(summary.report.contains("GitHub"));
        assert!(summary.report.contains("Available but not installed"));
        assert!(
            summary
                .report
                .contains("Remote sync error: network unavailable")
        );
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
