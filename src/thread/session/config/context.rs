//! Context/status selector helper-ы для session_config.

use std::collections::HashMap;
use std::path::Path;

use codex_app_server_protocol::{Account, RateLimitSnapshot, TokenUsageBreakdown};
use codex_core::config::types::{McpServerConfig, McpServerTransportConfig};
use codex_core::skills::SkillLoadOutcome;
use codex_protocol::account::PlanType;
use codex_protocol::protocol::SkillScope;

use super::limits::{combined_limits_reset_message, combined_limits_status_label};
use crate::thread::{ContextUsageSource, SessionConfigSelectOption};

pub(in crate::thread) const SESSION_STATUS_VALUE: &str = "session_status";
pub(in crate::thread) const CONTEXT_STATUS_VALUE: &str = "context_status";
pub(in crate::thread) const MCP_STATUS_VALUE: &str = "mcp_status";
pub(in crate::thread) const SKILLS_STATUS_VALUE: &str = "skills_status";
pub(in crate::thread) const CONTEXT_LIMITS_VALUE: &str = "limits_status";
pub(in crate::thread) const CONTEXT_COMPACT_VALUE: &str = "compact_now";

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
pub(in crate::thread) fn context_control_options(
    workspace_cwd: &Path,
    account_status: &AccountStatus,
    total_token_usage: Option<&TokenUsageBreakdown>,
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
    usage_source: Option<ContextUsageSource>,
    rate_limits: Option<&RateLimitSnapshot>,
    compaction_in_progress: bool,
    mcp_summary: &ContextSelectorSummary,
    skills_summary: &ContextSelectorSummary,
) -> Vec<SessionConfigSelectOption> {
    let status_summary = session_status_summary(
        workspace_cwd,
        account_status,
        total_token_usage,
        rate_limits,
    );

    vec![
        SessionConfigSelectOption::new(
            CONTEXT_STATUS_VALUE,
            context_status_label(used, size, usage_percent, compaction_in_progress),
        )
        .description(context_usage_message(used, size, usage_source)),
        SessionConfigSelectOption::new(SESSION_STATUS_VALUE, status_summary.label)
            .description(status_summary.description),
        SessionConfigSelectOption::new(MCP_STATUS_VALUE, &mcp_summary.label)
            .description(mcp_summary.description.clone()),
        SessionConfigSelectOption::new(SKILLS_STATUS_VALUE, &skills_summary.label)
            .description(skills_summary.description.clone()),
        SessionConfigSelectOption::new(CONTEXT_LIMITS_VALUE, limits_status_label(rate_limits))
            .description(combined_limits_reset_message(rate_limits)),
        SessionConfigSelectOption::new(CONTEXT_COMPACT_VALUE, "Compact now")
            .description("Summarize the conversation to free context window"),
    ]
}

pub(in crate::thread) fn session_status_message(
    workspace_cwd: &Path,
    account_status: &AccountStatus,
    total_token_usage: Option<&TokenUsageBreakdown>,
    rate_limits: Option<&RateLimitSnapshot>,
) -> String {
    format!(
        "Chat status\n\nWorkspace: {}\nAccount: {}\nTokens: {}",
        workspace_cwd.display(),
        account_detail(account_status, rate_limits),
        token_usage_summary_line(total_token_usage)
    )
}

pub(in crate::thread) fn build_mcp_summary(
    effective_mcp_servers: &HashMap<String, McpServerConfig>,
) -> ContextSelectorSummary {
    let mut entries = effective_mcp_servers
        .iter()
        .map(|(name, config)| {
            let (transport_label, target, extra) = match &config.transport {
                McpServerTransportConfig::Stdio { command, cwd, .. } => (
                    "stdio",
                    command.clone(),
                    cwd.as_ref()
                        .map(|path| format!("cwd {}", path.display()))
                        .unwrap_or_default(),
                ),
                McpServerTransportConfig::StreamableHttp { url, .. } => {
                    ("http", url.clone(), String::new())
                }
            };
            (
                name.clone(),
                format!(
                    "{} · {}{}",
                    transport_label,
                    truncate_middle(&target, 48),
                    if extra.is_empty() {
                        String::new()
                    } else {
                        format!(" · {extra}")
                    }
                ),
                format_mcp_report_entry(name, config),
                matches!(config.transport, McpServerTransportConfig::Stdio { .. }),
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    if entries.is_empty() {
        return ContextSelectorSummary {
            label: "MCP · none".to_string(),
            description: "No MCP servers are configured for this session.".to_string(),
            report: "No MCP servers are configured for this session.".to_string(),
        };
    }

    let total = entries.len();
    let stdio_count = entries.iter().filter(|entry| entry.3).count();
    let http_count = total.saturating_sub(stdio_count);

    let mut description_lines = vec![format!(
        "{total} MCP server(s) configured for this session ({stdio_count} stdio, {http_count} http)."
    )];
    description_lines.extend(
        entries
            .iter()
            .take(3)
            .map(|(name, preview, _, _)| format!("{name} · {preview}")),
    );
    if total > 3 {
        description_lines.push(format!("+{} more", total - 3));
    }

    let mut report_lines = vec![format!(
        "MCP servers configured for this session: {total} ({stdio_count} stdio, {http_count} http)."
    )];
    for (_, _, report, _) in entries {
        report_lines.push(String::new());
        report_lines.push(report);
    }

    ContextSelectorSummary {
        label: format!("MCP · {total} srv"),
        description: description_lines.join("\n"),
        report: report_lines.join("\n"),
    }
}

pub(in crate::thread) fn build_skills_summary(
    outcome: &SkillLoadOutcome,
) -> ContextSelectorSummary {
    let mut entries = outcome
        .skills_with_enabled()
        .map(|(skill, enabled)| {
            let display_name = skill
                .interface
                .as_ref()
                .and_then(|interface| interface.display_name.clone())
                .unwrap_or_else(|| skill.name.clone());
            let summary = skill
                .interface
                .as_ref()
                .and_then(|interface| interface.short_description.clone())
                .or_else(|| skill.short_description.clone())
                .unwrap_or_else(|| skill.description.clone());

            (
                display_name,
                summary,
                format_skill_report_entry(skill, enabled),
                enabled,
                skill.scope,
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.to_lowercase().cmp(&right.0.to_lowercase()));

    let total = entries.len();
    let enabled_count = entries.iter().filter(|entry| entry.3).count();
    let disabled_count = total.saturating_sub(enabled_count);
    let error_count = outcome.errors.len();

    if total == 0 && error_count == 0 {
        return ContextSelectorSummary {
            label: "Skills · none".to_string(),
            description: "No skills were discovered for the current workspace.".to_string(),
            report: "No skills were discovered for the current workspace.".to_string(),
        };
    }

    let mut description_lines = vec![format!(
        "{enabled_count}/{total} skill(s) enabled for this workspace."
    )];
    description_lines.extend(
        entries
            .iter()
            .take(3)
            .map(|(name, summary, _, enabled, scope)| {
                format!(
                    "{} · {} · {}",
                    name,
                    skill_scope_label(*scope),
                    if *enabled { summary } else { "disabled" }
                )
            }),
    );
    if disabled_count > 0 {
        description_lines.push(format!("{disabled_count} disabled"));
    }
    if error_count > 0 {
        description_lines.push(format!("{error_count} load error(s)"));
    }

    let mut report_lines = vec![format!(
        "Skills for current workspace: {enabled_count}/{total} enabled."
    )];
    if disabled_count > 0 {
        report_lines.push(format!("Disabled: {disabled_count}."));
    }
    if error_count > 0 {
        report_lines.push(format!("Load errors: {error_count}."));
        for error in &outcome.errors {
            report_lines.push(format!("- {}: {}", error.path.display(), error.message));
        }
    }
    for (_, _, report, _, _) in entries {
        report_lines.push(String::new());
        report_lines.push(report);
    }

    let mut label = format!("Skills · {enabled_count} on");
    if error_count > 0 {
        label.push_str(&format!(" · {error_count} err"));
    }

    ContextSelectorSummary {
        label,
        description: description_lines.join("\n"),
        report: report_lines.join("\n"),
    }
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

pub(in crate::thread) fn context_usage_message(
    used: Option<u64>,
    size: Option<u64>,
    usage_source: Option<ContextUsageSource>,
) -> String {
    match (used, size) {
        (Some(used), Some(size)) if size > 0 => {
            format!(
                "Context usage: {used}/{size} tokens.\nSource: {}.",
                context_usage_source_label(usage_source)
            )
        }
        (Some(used), None) => {
            format!(
                "Context usage: {used} tokens (window size is not available yet).\nSource: {}.",
                context_usage_source_label(usage_source)
            )
        }
        _ => {
            "Context usage is not available yet. App-server reports it after the first completed model turn, and resume restores the last cached value when available.".to_string()
        }
    }
}

fn session_status_summary(
    workspace_cwd: &Path,
    account_status: &AccountStatus,
    total_token_usage: Option<&TokenUsageBreakdown>,
    rate_limits: Option<&RateLimitSnapshot>,
) -> ContextSelectorSummary {
    ContextSelectorSummary {
        label: status_label(
            workspace_cwd,
            account_status,
            total_token_usage,
            rate_limits,
        ),
        description: format!(
            "Workspace: {}\nAccount: {}\nTokens: {}",
            workspace_cwd.display(),
            account_detail(account_status, rate_limits),
            token_usage_summary_line(total_token_usage),
        ),
        report: session_status_message(
            workspace_cwd,
            account_status,
            total_token_usage,
            rate_limits,
        ),
    }
}

fn context_usage_source_label(source: Option<ContextUsageSource>) -> &'static str {
    match source {
        Some(ContextUsageSource::Live) => "live",
        Some(ContextUsageSource::Cached) => "cached",
        None => "---",
    }
}

fn context_status_label(
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
    compaction_in_progress: bool,
) -> String {
    let short = if compaction_in_progress {
        "Compacting...".to_string()
    } else {
        match (used, size, usage_percent) {
            (_, Some(_), Some(percent)) => format!("{percent}%"),
            (Some(used), None, _) => format!("{used} tok"),
            _ => "---".to_string(),
        }
    };
    format!("ctx {short}")
}

fn limits_status_label(rate_limits: Option<&RateLimitSnapshot>) -> String {
    format!("Limits · {}", combined_limits_status_label(rate_limits))
}

fn status_label(
    _workspace_cwd: &Path,
    _account_status: &AccountStatus,
    total_token_usage: Option<&TokenUsageBreakdown>,
    _rate_limits: Option<&RateLimitSnapshot>,
) -> String {
    let total_label = total_token_usage
        .map(|usage| {
            format!(
                "{} used",
                format_compact_token_count(blended_total_tokens(usage))
            )
        })
        .unwrap_or_else(|| "usage pending".to_string());
    format!("Status · {total_label}")
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

fn format_mcp_report_entry(name: &str, config: &McpServerConfig) -> String {
    let mut lines = vec![format!(
        "- {name} [{}{}{}]",
        match &config.transport {
            McpServerTransportConfig::Stdio { .. } => "stdio",
            McpServerTransportConfig::StreamableHttp { .. } => "http",
        },
        if config.enabled { "" } else { ", disabled" },
        if config.required { ", required" } else { "" }
    )];

    match &config.transport {
        McpServerTransportConfig::Stdio {
            command, args, cwd, ..
        } => {
            lines.push(format!("  command: {}", truncate_middle(command, 120)));
            if !args.is_empty() {
                lines.push(format!("  args: {}", args.join(" ")));
            }
            if let Some(cwd) = cwd {
                lines.push(format!("  cwd: {}", cwd.display()));
            }
        }
        McpServerTransportConfig::StreamableHttp { url, .. } => {
            lines.push(format!("  url: {}", truncate_middle(url, 120)));
        }
    }

    lines.push(format!(
        "  tools: {}",
        config
            .enabled_tools
            .as_ref()
            .map(|tools| tools.join(", "))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "all".to_string())
    ));
    if let Some(disabled_tools) = &config.disabled_tools
        && !disabled_tools.is_empty()
    {
        lines.push(format!("  disabled tools: {}", disabled_tools.join(", ")));
    }

    lines.join("\n")
}

fn format_skill_report_entry(skill: &codex_core::skills::SkillMetadata, enabled: bool) -> String {
    let display_name = skill
        .interface
        .as_ref()
        .and_then(|interface| interface.display_name.clone())
        .unwrap_or_else(|| skill.name.clone());
    let summary = skill
        .interface
        .as_ref()
        .and_then(|interface| interface.short_description.clone())
        .or_else(|| skill.short_description.clone())
        .unwrap_or_else(|| skill.description.clone());

    let mut lines = vec![format!(
        "- {} [{}{}]",
        display_name,
        skill_scope_label(skill.scope),
        if enabled { "" } else { ", disabled" }
    )];
    if display_name != skill.name {
        lines.push(format!("  name: {}", skill.name));
    }
    lines.push(format!("  summary: {summary}"));
    lines.push(format!("  path: {}", skill.path_to_skills_md.display()));
    if let Some(dependencies) = &skill.dependencies
        && !dependencies.tools.is_empty()
    {
        lines.push(format!("  tool deps: {}", dependencies.tools.len()));
    }
    lines.join("\n")
}

fn skill_scope_label(scope: SkillScope) -> &'static str {
    match scope {
        SkillScope::User => "user",
        SkillScope::Repo => "repo",
        SkillScope::System => "system",
        SkillScope::Admin => "admin",
    }
}

fn truncate_middle(value: &str, max_chars: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return value.to_string();
    }
    if max_chars <= 3 {
        return chars.iter().take(max_chars).collect();
    }

    let head_len = max_chars / 2;
    let tail_len = max_chars.saturating_sub(head_len + 3);
    let head = chars.iter().take(head_len).collect::<String>();
    let tail = chars
        .iter()
        .rev()
        .take(tail_len)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{head}...{tail}")
}

#[cfg(test)]
mod tests {
    use super::{
        AccountStatus, CONTEXT_COMPACT_VALUE, CONTEXT_LIMITS_VALUE, CONTEXT_STATUS_VALUE,
        MCP_STATUS_VALUE, SESSION_STATUS_VALUE, SKILLS_STATUS_VALUE, build_mcp_summary,
        build_skills_summary, context_control_options, context_usage_message,
        session_status_message,
    };
    use crate::thread::ContextUsageSource;
    use codex_app_server_protocol::{RateLimitSnapshot, RateLimitWindow, TokenUsageBreakdown};
    use codex_core::config::types::{McpServerConfig, McpServerTransportConfig};
    use codex_core::skills::{SkillLoadOutcome, SkillMetadata};
    use codex_protocol::account::PlanType;
    use codex_protocol::protocol::SkillScope;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    #[test]
    fn context_messages_include_usage() {
        assert_eq!(
            context_usage_message(Some(157_835), Some(258_400), Some(ContextUsageSource::Live)),
            "Context usage: 157835/258400 tokens.\nSource: live."
        );
    }

    #[test]
    fn context_options_include_status_actions_and_compact() {
        let empty_summary = super::ContextSelectorSummary {
            label: "none".to_string(),
            description: "none".to_string(),
            report: "none".to_string(),
        };
        let options = context_control_options(
            PathBuf::from("/tmp/workspace").as_path(),
            &AccountStatus::default(),
            None,
            Some(157_835),
            Some(258_400),
            Some(61),
            Some(ContextUsageSource::Live),
            None,
            false,
            &empty_summary,
            &empty_summary,
        );
        assert_eq!(options[0].value.0.as_ref(), CONTEXT_STATUS_VALUE);
        assert_eq!(options[1].value.0.as_ref(), SESSION_STATUS_VALUE);
        assert_eq!(options[2].value.0.as_ref(), MCP_STATUS_VALUE);
        assert_eq!(options[3].value.0.as_ref(), SKILLS_STATUS_VALUE);
        assert_eq!(options[4].value.0.as_ref(), CONTEXT_LIMITS_VALUE);
        assert_eq!(options[5].value.0.as_ref(), CONTEXT_COMPACT_VALUE);
    }

    #[test]
    fn context_options_use_dashes_when_usage_is_unknown() {
        let empty_summary = super::ContextSelectorSummary {
            label: "MCP · none".to_string(),
            description: "none".to_string(),
            report: "none".to_string(),
        };
        let options = context_control_options(
            PathBuf::from("/tmp/workspace").as_path(),
            &AccountStatus::default(),
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            &empty_summary,
            &empty_summary,
        );
        assert_eq!(options[0].name, "ctx ---");
        assert_eq!(options[4].name, "Limits · 5h -- · wk --");
        assert_eq!(options[5].name, "Compact now");
    }

    #[test]
    fn context_status_shows_percentage_only() {
        let empty_summary = super::ContextSelectorSummary {
            label: "none".to_string(),
            description: "none".to_string(),
            report: "none".to_string(),
        };
        let options = context_control_options(
            PathBuf::from("/tmp/workspace").as_path(),
            &AccountStatus::default(),
            None,
            Some(195_499),
            Some(258_400),
            Some(76),
            Some(ContextUsageSource::Cached),
            None,
            false,
            &empty_summary,
            &empty_summary,
        );
        assert_eq!(options[0].name, "ctx 76%");
        assert_eq!(
            options[0].description.as_deref(),
            Some("Context usage: 195499/258400 tokens.\nSource: cached.")
        );
    }

    #[test]
    fn context_options_include_combined_limit_item() {
        let snapshot = RateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: None,
            primary: Some(RateLimitWindow {
                used_percent: 20,
                window_duration_mins: Some(300),
                resets_at: Some(4_102_444_800),
            }),
            secondary: Some(RateLimitWindow {
                used_percent: 6,
                window_duration_mins: Some(10_080),
                resets_at: Some(4_102_531_200),
            }),
            credits: None,
            plan_type: Some(PlanType::Plus),
        };

        let empty_summary = super::ContextSelectorSummary {
            label: "none".to_string(),
            description: "none".to_string(),
            report: "none".to_string(),
        };
        let options = context_control_options(
            PathBuf::from("/tmp/workspace").as_path(),
            &AccountStatus::default(),
            None,
            Some(195_499),
            Some(258_400),
            Some(76),
            Some(ContextUsageSource::Live),
            Some(&snapshot),
            false,
            &empty_summary,
            &empty_summary,
        );
        assert_eq!(options[4].name, "Limits · 5h 80% · wk 94%");
    }

    #[test]
    fn session_status_focuses_on_workspace_account_and_tokens() {
        let options = context_control_options(
            PathBuf::from("/tmp/workspace").as_path(),
            &AccountStatus {
                auth_kind: super::AccountAuthKind::Chatgpt,
                email: Some("dev@example.com".to_string()),
                plan_type: Some(PlanType::Plus),
            },
            Some(&TokenUsageBreakdown {
                total_tokens: 9_010_000,
                input_tokens: 8_970_000,
                cached_input_tokens: 0,
                output_tokens: 37_300,
                reasoning_output_tokens: 0,
            }),
            Some(195_499),
            Some(258_400),
            Some(76),
            Some(ContextUsageSource::Live),
            None,
            false,
            &super::ContextSelectorSummary::default(),
            &super::ContextSelectorSummary::default(),
        );

        assert_eq!(options[1].name, "Status · 9.01M used");
        assert_eq!(
            options[1].description.as_deref(),
            Some(
                "Workspace: /tmp/workspace\nAccount: dev@example.com · ChatGPT Plus\nTokens: 9.01M used · 8.97M in · 37.3K out"
            )
        );
    }

    #[test]
    fn session_status_message_shows_api_key_and_cached_reasoning_tokens() {
        let message = session_status_message(
            PathBuf::from("/tmp/workspace").as_path(),
            &AccountStatus {
                auth_kind: super::AccountAuthKind::ApiKey,
                email: None,
                plan_type: None,
            },
            Some(&TokenUsageBreakdown {
                total_tokens: 1_250_000,
                input_tokens: 900_000,
                cached_input_tokens: 300_000,
                output_tokens: 50_000,
                reasoning_output_tokens: 25_000,
            }),
            None,
        );

        assert_eq!(
            message,
            "Chat status\n\nWorkspace: /tmp/workspace\nAccount: API key auth\nTokens: 650K used · 600K in · 50K out"
        );
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
}
