//! Context/status selector helper-ы для session_config.

use std::path::Path;

use codex_app_server_protocol::{Account, RateLimitSnapshot, TokenUsageBreakdown};
use codex_protocol::account::PlanType;

use super::limits::{
    combined_limits_reset_message, five_hour_status_label, limits_status_description,
};
use crate::thread::{
    ContextDisplayStyle, ContextUsageSource, LimitsDisplayStyle, SessionConfigSelectGroup,
    SessionConfigSelectOption,
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

pub(in crate::thread) const SESSION_STATUS_VALUE: &str = "session_status";
pub(in crate::thread) const CONTEXT_BRAILLE_VALUE: &str = "context_braille_status";
pub(in crate::thread) const CONTEXT_PERCENT_VALUE: &str = "context_percent_status";
pub(in crate::thread) const CONTEXT_STATUS_VALUE: &str = "context_status";
pub(in crate::thread) const MCP_STATUS_VALUE: &str = "mcp_status";
pub(in crate::thread) const SKILLS_STATUS_VALUE: &str = "skills_status";
pub(in crate::thread) const PLUGINS_STATUS_VALUE: &str = "plugins_status";
pub(in crate::thread) const CONTEXT_LIMITS_VALUE: &str = "limits_status";
pub(in crate::thread) const CONTEXT_LIMITS_TEXT_VALUE: &str = "limits_text_status";
pub(in crate::thread) const CONTEXT_LIMITS_BARS_VALUE: &str = "limits_bars_status";
pub(in crate::thread) const CONTEXT_LIMITS_BLOCK_VALUE: &str = "limits_block_status";
pub(in crate::thread) const CONTEXT_COMBINED_VALUE: &str = "context_limits_status";
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
    context_display_style: ContextDisplayStyle,
    limits_display_style: LimitsDisplayStyle,
    rate_limits: Option<&RateLimitSnapshot>,
    compaction_in_progress: bool,
    mcp_summary: &ContextSelectorSummary,
    skills_summary: &ContextSelectorSummary,
    plugins_summary: &ContextSelectorSummary,
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
            context_display_label(
                used,
                size,
                usage_percent,
                compaction_in_progress,
                context_display_style,
            ),
        )
        .description(context_status_description(
            used,
            size,
            usage_percent,
            usage_source,
            compaction_in_progress,
        )),
        SessionConfigSelectOption::new(
            CONTEXT_LIMITS_VALUE,
            limits_display_label(rate_limits, limits_display_style),
        )
        .description(limits_status_description(rate_limits)),
        SessionConfigSelectOption::new(
            CONTEXT_COMBINED_VALUE,
            combined_context_limits_label(
                used,
                size,
                usage_percent,
                compaction_in_progress,
                context_display_style,
                limits_display_style,
                rate_limits,
            ),
        )
        .description(combined_context_limits_description(
            used,
            size,
            usage_percent,
            usage_source,
            compaction_in_progress,
            rate_limits,
        )),
        SessionConfigSelectOption::new(
            CONTEXT_PERCENT_VALUE,
            context_style_option_label(ContextDisplayStyle::Percent, used, size, usage_percent),
        )
        .description(context_style_description(ContextDisplayStyle::Percent)),
        SessionConfigSelectOption::new(
            CONTEXT_BRAILLE_VALUE,
            context_style_option_label(ContextDisplayStyle::Braille, used, size, usage_percent),
        )
        .description(context_style_description(ContextDisplayStyle::Braille)),
        SessionConfigSelectOption::new(
            CONTEXT_LIMITS_TEXT_VALUE,
            limits_style_option_label(LimitsDisplayStyle::Text, rate_limits),
        )
        .description(limits_style_description(LimitsDisplayStyle::Text)),
        SessionConfigSelectOption::new(
            CONTEXT_LIMITS_BARS_VALUE,
            limits_style_option_label(LimitsDisplayStyle::Bars, rate_limits),
        )
        .description(limits_style_description(LimitsDisplayStyle::Bars)),
        SessionConfigSelectOption::new(
            CONTEXT_LIMITS_BLOCK_VALUE,
            limits_style_option_label(LimitsDisplayStyle::Block, rate_limits),
        )
        .description(limits_style_description(LimitsDisplayStyle::Block)),
        SessionConfigSelectOption::new(SESSION_STATUS_VALUE, status_summary.label)
            .description(status_summary.description),
        SessionConfigSelectOption::new(MCP_STATUS_VALUE, &mcp_summary.label)
            .description(mcp_summary.description.clone()),
        SessionConfigSelectOption::new(SKILLS_STATUS_VALUE, &skills_summary.label)
            .description(skills_summary.description.clone()),
        SessionConfigSelectOption::new(PLUGINS_STATUS_VALUE, &plugins_summary.label)
            .description(plugins_summary.description.clone()),
        SessionConfigSelectOption::new(CONTEXT_COMPACT_VALUE, "Compact now")
            .description("Summarize the conversation to free context window"),
    ]
}

#[allow(clippy::too_many_arguments)]
pub(in crate::thread) fn context_control_option_groups(
    workspace_cwd: &Path,
    account_status: &AccountStatus,
    total_token_usage: Option<&TokenUsageBreakdown>,
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
    usage_source: Option<ContextUsageSource>,
    context_display_style: ContextDisplayStyle,
    limits_display_style: LimitsDisplayStyle,
    rate_limits: Option<&RateLimitSnapshot>,
    compaction_in_progress: bool,
    mcp_summary: &ContextSelectorSummary,
    skills_summary: &ContextSelectorSummary,
    plugins_summary: &ContextSelectorSummary,
) -> Vec<SessionConfigSelectGroup> {
    let mut options = context_control_options(
        workspace_cwd,
        account_status,
        total_token_usage,
        used,
        size,
        usage_percent,
        usage_source,
        context_display_style,
        limits_display_style,
        rate_limits,
        compaction_in_progress,
        mcp_summary,
        skills_summary,
        plugins_summary,
    )
    .into_iter();

    let display_options = vec![
        options.next().expect("context display option should exist"),
        options.next().expect("limits display option should exist"),
        options
            .next()
            .expect("combined display option should exist"),
    ];
    let context_style_options = vec![
        options.next().expect("context percent option should exist"),
        options.next().expect("context braille option should exist"),
    ];
    let limit_style_options = vec![
        options.next().expect("limits text option should exist"),
        options.next().expect("limits bars option should exist"),
        options.next().expect("limits block option should exist"),
    ];
    let integration_options = vec![
        options.next().expect("session status option should exist"),
        options.next().expect("MCP status option should exist"),
        options.next().expect("skills status option should exist"),
        options.next().expect("plugins status option should exist"),
    ];
    let action_options = vec![options.next().expect("compact option should exist")];

    vec![
        SessionConfigSelectGroup::new("display", "Display", display_options),
        SessionConfigSelectGroup::new("context", "Context", context_style_options),
        SessionConfigSelectGroup::new("limits", "Limits", limit_style_options),
        SessionConfigSelectGroup::new("integrations", "Integrations", integration_options),
        SessionConfigSelectGroup::new("actions", "Actions", action_options),
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

#[allow(clippy::too_many_arguments)]
pub(in crate::thread) fn full_status_report(
    workspace_cwd: &Path,
    account_status: &AccountStatus,
    total_token_usage: Option<&TokenUsageBreakdown>,
    used: Option<u64>,
    size: Option<u64>,
    usage_source: Option<ContextUsageSource>,
    rate_limits: Option<&RateLimitSnapshot>,
    compaction_in_progress: bool,
    mcp_summary: &ContextSelectorSummary,
    skills_summary: &ContextSelectorSummary,
    plugins_summary: &ContextSelectorSummary,
) -> String {
    let mut sections = vec![
        session_status_message(
            workspace_cwd,
            account_status,
            total_token_usage,
            rate_limits,
        ),
        context_usage_message(used, size, usage_source),
        combined_limits_reset_message(rate_limits),
        mcp_summary.report.clone(),
        skills_summary.report.clone(),
        plugins_summary.report.clone(),
    ];

    if compaction_in_progress {
        sections.push("Context compaction is currently running.".to_string());
    }

    sections.join("\n\n")
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
    if compaction_in_progress {
        "Compacting...".to_string()
    } else {
        match (used, size, usage_percent) {
            (_, Some(_), Some(percent)) => format!("{percent}%"),
            (Some(used), None, _) => {
                format!(
                    "{} tok",
                    format_compact_token_count(u64_to_i64_saturating(used))
                )
            }
            _ => "---".to_string(),
        }
    }
}

fn context_display_label(
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
    compaction_in_progress: bool,
    style: ContextDisplayStyle,
) -> String {
    match style {
        ContextDisplayStyle::Percent => {
            context_status_label(used, size, usage_percent, compaction_in_progress)
        }
        ContextDisplayStyle::Braille => {
            if compaction_in_progress {
                "Compacting...".to_string()
            } else {
                context_braille_label(used, size, usage_percent).to_string()
            }
        }
    }
}

fn context_braille_label(
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
) -> &'static str {
    context_usage_braille(used, size, usage_percent)
}

fn context_style_option_label(
    style: ContextDisplayStyle,
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
) -> String {
    match style {
        ContextDisplayStyle::Percent => format!(
            "Percent {}",
            context_display_label(
                used,
                size,
                usage_percent,
                false,
                ContextDisplayStyle::Percent,
            )
        ),
        ContextDisplayStyle::Braille => format!(
            "Braille {}",
            context_display_label(
                used,
                size,
                usage_percent,
                false,
                ContextDisplayStyle::Braille,
            )
        ),
    }
}

fn context_style_description(style: ContextDisplayStyle) -> &'static str {
    match style {
        ContextDisplayStyle::Percent => {
            "Show context usage as a compact percentage in the lower selector."
        }
        ContextDisplayStyle::Braille => {
            "Show context usage as a single braille cell in eighth-step buckets."
        }
    }
}

pub(in crate::thread) fn context_status_description(
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
    usage_source: Option<ContextUsageSource>,
    compaction_in_progress: bool,
) -> String {
    let status = if compaction_in_progress {
        "compacting"
    } else {
        context_status_source_label(usage_source)
    };

    match (used, size, usage_percent) {
        (Some(used), Some(size), Some(percent)) if size > 0 => format!(
            "Context: {} {percent}%\nTokens: {used}/{size}\nStatus: {status}",
            context_usage_braille(Some(used), Some(size), Some(percent)),
        ),
        (Some(used), None, _) => {
            format!("Context: ?\nTokens: {used} (window size unavailable)\nStatus: {status}",)
        }
        _ => format!("Context: ⠀ --\nTokens: waiting for usage update\nStatus: {status}"),
    }
}

fn context_usage_braille(
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
) -> &'static str {
    const INDICATORS: [&str; 9] = ["⠀", "⢀", "⣀", "⣄", "⣤", "⣦", "⣶", "⣷", "⣿"];

    let bucket = match (used, size) {
        (Some(used), Some(size)) if size > 0 => {
            (((used as u128) * 8 + (size as u128 / 2)) / size as u128).min(8) as usize
        }
        _ => usage_percent
            .map(|percent| ((percent.min(100) * 8 + 50) / 100).min(8) as usize)
            .unwrap_or(0),
    };
    INDICATORS[bucket]
}

fn context_status_source_label(source: Option<ContextUsageSource>) -> &'static str {
    match source {
        Some(ContextUsageSource::Live) => "live",
        Some(ContextUsageSource::Cached) => "cached",
        None => "pending",
    }
}

fn u64_to_i64_saturating(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn limits_status_label(rate_limits: Option<&RateLimitSnapshot>) -> String {
    five_hour_status_label(rate_limits)
}

fn limits_bars_label(rate_limits: Option<&RateLimitSnapshot>) -> String {
    five_hour_limit_bars(rate_limits)
}

fn limits_block_label(rate_limits: Option<&RateLimitSnapshot>) -> String {
    five_hour_limit_block(rate_limits)
}

fn limits_display_label(
    rate_limits: Option<&RateLimitSnapshot>,
    style: LimitsDisplayStyle,
) -> String {
    match style {
        LimitsDisplayStyle::Text => limits_status_label(rate_limits),
        LimitsDisplayStyle::Bars => limits_bars_label(rate_limits),
        LimitsDisplayStyle::Block => limits_block_label(rate_limits),
    }
}

fn limits_style_option_label(
    style: LimitsDisplayStyle,
    rate_limits: Option<&RateLimitSnapshot>,
) -> String {
    match style {
        LimitsDisplayStyle::Text => format!("Text {}", limits_status_label(rate_limits)),
        LimitsDisplayStyle::Bars => format!("Bars {}", limits_bars_label(rate_limits)),
        LimitsDisplayStyle::Block => format!("Block {}", limits_block_label(rate_limits)),
    }
}

fn limits_style_description(style: LimitsDisplayStyle) -> &'static str {
    match style {
        LimitsDisplayStyle::Text => "Show remaining 5-hour quota as text, for example `5h 80%`.",
        LimitsDisplayStyle::Bars => {
            "Show remaining 5-hour quota as five cells: `▰▰▰▰▰` is full, `▱▱▱▱▱` is empty."
        }
        LimitsDisplayStyle::Block => "Show remaining 5-hour quota as one block from `▁` to `█`.",
    }
}

fn combined_context_limits_label(
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
    compaction_in_progress: bool,
    context_style: ContextDisplayStyle,
    limits_style: LimitsDisplayStyle,
    rate_limits: Option<&RateLimitSnapshot>,
) -> String {
    format!(
        "{} {}",
        context_display_label(
            used,
            size,
            usage_percent,
            compaction_in_progress,
            context_style
        ),
        limits_display_label(rate_limits, limits_style),
    )
}

fn combined_context_limits_description(
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
    usage_source: Option<ContextUsageSource>,
    compaction_in_progress: bool,
    rate_limits: Option<&RateLimitSnapshot>,
) -> String {
    format!(
        "{}\n\n{}",
        context_status_description(
            used,
            size,
            usage_percent,
            usage_source,
            compaction_in_progress
        ),
        limits_status_description(rate_limits)
    )
}

fn five_hour_limit_bars(rate_limits: Option<&RateLimitSnapshot>) -> String {
    match rate_limits
        .and_then(|snapshot| snapshot.primary.as_ref())
        .map(|window| window.used_percent.clamp(0, 100))
    {
        Some(used_percent) => limit_bars_for_remaining_percent(100 - used_percent),
        None => "▱▱▱▱▱".to_string(),
    }
}

fn five_hour_limit_block(rate_limits: Option<&RateLimitSnapshot>) -> String {
    match rate_limits
        .and_then(|snapshot| snapshot.primary.as_ref())
        .map(|window| window.used_percent.clamp(0, 100))
    {
        Some(used_percent) => limit_block_for_remaining_percent(100 - used_percent).to_string(),
        None => "▁".to_string(),
    }
}

fn limit_bars_for_remaining_percent(remaining_percent: i32) -> String {
    let filled = ((remaining_percent.clamp(0, 100) + 19) / 20).clamp(0, 5) as usize;
    format!("{}{}", "▰".repeat(filled), "▱".repeat(5 - filled))
}

fn limit_block_for_remaining_percent(remaining_percent: i32) -> &'static str {
    const BLOCKS: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    let bucket = ((remaining_percent.clamp(0, 100) * 7 + 50) / 100).clamp(0, 7) as usize;
    BLOCKS[bucket]
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

#[cfg(test)]
mod tests {
    use super::{
        AccountStatus, CONTEXT_BRAILLE_VALUE, CONTEXT_COMBINED_VALUE, CONTEXT_COMPACT_VALUE,
        CONTEXT_LIMITS_BARS_VALUE, CONTEXT_LIMITS_BLOCK_VALUE, CONTEXT_LIMITS_TEXT_VALUE,
        CONTEXT_LIMITS_VALUE, CONTEXT_PERCENT_VALUE, CONTEXT_STATUS_VALUE, MCP_STATUS_VALUE,
        PLUGINS_STATUS_VALUE, SESSION_STATUS_VALUE, SKILLS_STATUS_VALUE, build_mcp_summary,
        build_plugins_summary, build_skills_summary, context_control_option_groups,
        context_control_options, context_usage_braille, context_usage_message, full_status_report,
        session_status_message,
    };
    use crate::thread::{ContextDisplayStyle, ContextUsageSource, LimitsDisplayStyle};
    use codex_app_server_protocol::{
        PluginInterface, PluginListResponse, PluginMarketplaceEntry, PluginSource, PluginSummary,
        RateLimitSnapshot, RateLimitWindow, TokenUsageBreakdown,
    };
    use codex_core::config::types::{McpServerConfig, McpServerTransportConfig};
    use codex_core::skills::{SkillLoadOutcome, SkillMetadata};
    use codex_protocol::account::PlanType;
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

    fn empty_summary(label: &str) -> super::ContextSelectorSummary {
        summary(label, "none", "none")
    }

    #[test]
    fn context_messages_include_usage() {
        assert_eq!(
            context_usage_message(Some(157_835), Some(258_400), Some(ContextUsageSource::Live)),
            "Context usage: 157835/258400 tokens.\nSource: live."
        );
    }

    #[test]
    fn context_options_include_status_actions_and_compact() {
        let empty_summary = empty_summary("none");
        let options = context_control_options(
            PathBuf::from("/tmp/workspace").as_path(),
            &AccountStatus::default(),
            None,
            Some(157_835),
            Some(258_400),
            Some(61),
            Some(ContextUsageSource::Live),
            ContextDisplayStyle::Percent,
            LimitsDisplayStyle::Text,
            None,
            false,
            &empty_summary,
            &empty_summary,
            &empty_summary,
        );
        assert_eq!(options[0].value.0.as_ref(), CONTEXT_STATUS_VALUE);
        assert_eq!(options[1].value.0.as_ref(), CONTEXT_LIMITS_VALUE);
        assert_eq!(options[2].value.0.as_ref(), CONTEXT_COMBINED_VALUE);
        assert_eq!(options[3].value.0.as_ref(), CONTEXT_PERCENT_VALUE);
        assert_eq!(options[4].value.0.as_ref(), CONTEXT_BRAILLE_VALUE);
        assert_eq!(options[5].value.0.as_ref(), CONTEXT_LIMITS_TEXT_VALUE);
        assert_eq!(options[6].value.0.as_ref(), CONTEXT_LIMITS_BARS_VALUE);
        assert_eq!(options[7].value.0.as_ref(), CONTEXT_LIMITS_BLOCK_VALUE);
        assert_eq!(options[8].value.0.as_ref(), SESSION_STATUS_VALUE);
        assert_eq!(options[9].value.0.as_ref(), MCP_STATUS_VALUE);
        assert_eq!(options[10].value.0.as_ref(), SKILLS_STATUS_VALUE);
        assert_eq!(options[11].value.0.as_ref(), PLUGINS_STATUS_VALUE);
        assert_eq!(options[12].value.0.as_ref(), CONTEXT_COMPACT_VALUE);
    }

    #[test]
    fn context_options_are_grouped_for_zed_picker() {
        let empty_summary = empty_summary("none");
        let groups = context_control_option_groups(
            PathBuf::from("/tmp/workspace").as_path(),
            &AccountStatus::default(),
            None,
            Some(157_835),
            Some(258_400),
            Some(61),
            Some(ContextUsageSource::Live),
            ContextDisplayStyle::Percent,
            LimitsDisplayStyle::Text,
            None,
            false,
            &empty_summary,
            &empty_summary,
            &empty_summary,
        );

        let names = groups
            .iter()
            .map(|group| group.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec!["Display", "Context", "Limits", "Integrations", "Actions"]
        );
        assert_eq!(groups[0].options.len(), 3);
        assert_eq!(groups[0].options[0].value.0.as_ref(), CONTEXT_STATUS_VALUE);
        assert_eq!(groups[0].options[1].value.0.as_ref(), CONTEXT_LIMITS_VALUE);
        assert_eq!(
            groups[0].options[2].value.0.as_ref(),
            CONTEXT_COMBINED_VALUE
        );
        assert_eq!(groups[1].options[0].value.0.as_ref(), CONTEXT_PERCENT_VALUE);
        assert_eq!(groups[1].options[1].value.0.as_ref(), CONTEXT_BRAILLE_VALUE);
        assert_eq!(
            groups[2].options[0].value.0.as_ref(),
            CONTEXT_LIMITS_TEXT_VALUE
        );
        assert_eq!(
            groups[2].options[1].value.0.as_ref(),
            CONTEXT_LIMITS_BARS_VALUE
        );
        assert_eq!(
            groups[2].options[2].value.0.as_ref(),
            CONTEXT_LIMITS_BLOCK_VALUE
        );
        assert_eq!(groups[3].options.len(), 4);
        assert_eq!(groups[4].options[0].value.0.as_ref(), CONTEXT_COMPACT_VALUE);
    }

    #[test]
    fn context_options_use_dashes_when_usage_is_unknown() {
        let empty_summary = empty_summary("MCP · none");
        let options = context_control_options(
            PathBuf::from("/tmp/workspace").as_path(),
            &AccountStatus::default(),
            None,
            None,
            None,
            None,
            None,
            ContextDisplayStyle::Percent,
            LimitsDisplayStyle::Text,
            None,
            false,
            &empty_summary,
            &empty_summary,
            &empty_summary,
        );
        assert_eq!(options[0].name, "---");
        assert_eq!(options[1].name, "5h --");
        assert_eq!(options[2].name, "--- 5h --");
        assert_eq!(options[3].name, "Percent ---");
        assert_eq!(options[4].name, "Braille ⠀");
        assert_eq!(options[5].name, "Text 5h --");
        assert_eq!(options[6].name, "Bars ▱▱▱▱▱");
        assert_eq!(options[7].name, "Block ▁");
        assert_eq!(options[12].name, "Compact now");
    }

    #[test]
    fn context_status_shows_braille_indicator_and_percentage() {
        let empty_summary = empty_summary("none");
        let options = context_control_options(
            PathBuf::from("/tmp/workspace").as_path(),
            &AccountStatus::default(),
            None,
            Some(195_499),
            Some(258_400),
            Some(76),
            Some(ContextUsageSource::Cached),
            ContextDisplayStyle::Braille,
            LimitsDisplayStyle::Text,
            None,
            false,
            &empty_summary,
            &empty_summary,
            &empty_summary,
        );
        assert_eq!(options[0].name, "⣶");
        assert_eq!(options[3].name, "Percent 76%");
        assert_eq!(options[4].name, "Braille ⣶");
        assert_eq!(
            options[4].description.as_deref(),
            Some("Show context usage as a single braille cell in eighth-step buckets.")
        );
        assert_eq!(
            options[0].description.as_deref(),
            Some("Context: ⣶ 76%\nTokens: 195499/258400\nStatus: cached")
        );
    }

    #[test]
    fn context_braille_indicator_uses_eighth_steps() {
        assert_eq!(context_usage_braille(None, None, Some(0)), "⠀");
        assert_eq!(context_usage_braille(None, None, Some(12)), "⢀");
        assert_eq!(context_usage_braille(None, None, Some(25)), "⣀");
        assert_eq!(context_usage_braille(None, None, Some(38)), "⣄");
        assert_eq!(context_usage_braille(None, None, Some(50)), "⣤");
        assert_eq!(context_usage_braille(None, None, Some(63)), "⣦");
        assert_eq!(context_usage_braille(None, None, Some(75)), "⣶");
        assert_eq!(context_usage_braille(None, None, Some(88)), "⣷");
        assert_eq!(context_usage_braille(None, None, Some(100)), "⣿");
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

        let empty_summary = empty_summary("none");
        let options = context_control_options(
            PathBuf::from("/tmp/workspace").as_path(),
            &AccountStatus::default(),
            None,
            Some(195_499),
            Some(258_400),
            Some(76),
            Some(ContextUsageSource::Live),
            ContextDisplayStyle::Percent,
            LimitsDisplayStyle::Bars,
            Some(&snapshot),
            false,
            &empty_summary,
            &empty_summary,
            &empty_summary,
        );
        assert_eq!(options[1].name, "▰▰▰▰▱");
        assert_eq!(options[2].name, "76% ▰▰▰▰▱");
        assert_eq!(options[6].name, "Bars ▰▰▰▰▱");
        assert_eq!(options[7].name, "Block ▇");
        assert_eq!(
            options[6].description.as_deref(),
            Some("Show remaining 5-hour quota as five cells: `▰▰▰▰▰` is full, `▱▱▱▱▱` is empty.")
        );
        assert!(
            options[1]
                .description
                .as_deref()
                .is_some_and(|description| description.contains("5h 80% · wk 94%"))
        );
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
            ContextDisplayStyle::Percent,
            LimitsDisplayStyle::Text,
            None,
            false,
            &super::ContextSelectorSummary::default(),
            &super::ContextSelectorSummary::default(),
            &super::ContextSelectorSummary::default(),
        );

        assert_eq!(options[8].name, "Status · 9.01M used");
        assert_eq!(
            options[8].description.as_deref(),
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
    fn full_status_report_includes_plugins_and_compaction() {
        let report = full_status_report(
            PathBuf::from("/tmp/workspace").as_path(),
            &AccountStatus::default(),
            None,
            Some(1_000),
            Some(2_000),
            Some(ContextUsageSource::Live),
            None,
            true,
            &summary("MCP · 1 srv", "one", "MCP report"),
            &summary("Skills · 1 on", "one", "Skills report"),
            &summary("Plugins · none", "none", "Plugins report"),
        );

        assert!(report.contains("Chat status"));
        assert!(report.contains("Context usage: 1000/2000 tokens."));
        assert!(report.contains("MCP report"));
        assert!(report.contains("Skills report"));
        assert!(report.contains("Plugins report"));
        assert!(report.contains("Context compaction is currently running."));
    }
}
