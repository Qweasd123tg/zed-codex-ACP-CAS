//! Context usage / compaction helper-ы для session_config.

use codex_app_server_protocol::RateLimitSnapshot;

use super::limits::{combined_limits_reset_message, combined_limits_status_label};
use crate::thread::{ContextUsageSource, SessionConfigSelectOption};

pub(in crate::thread) const CONTEXT_STATUS_VALUE: &str = "status";
pub(in crate::thread) const CONTEXT_LIMITS_VALUE: &str = "limits_status";
pub(in crate::thread) const CONTEXT_COMPACT_VALUE: &str = "compact_now";

pub(in crate::thread) fn context_control_options(
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
    usage_source: Option<ContextUsageSource>,
    rate_limits: Option<&RateLimitSnapshot>,
    compaction_in_progress: bool,
) -> Vec<SessionConfigSelectOption> {
    vec![
        SessionConfigSelectOption::new(
            CONTEXT_STATUS_VALUE,
            context_status_label(used, size, usage_percent, compaction_in_progress),
        )
        .description(context_usage_message(used, size, usage_source)),
        SessionConfigSelectOption::new(
            CONTEXT_LIMITS_VALUE,
            combined_limits_status_label(rate_limits),
        )
        .description(combined_limits_reset_message(rate_limits)),
        SessionConfigSelectOption::new(CONTEXT_COMPACT_VALUE, "Compact now")
            .description("Summarize the conversation to free context window"),
    ]
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
        return "Compacting...".to_string();
    }
    match (used, size, usage_percent) {
        (_, Some(_), Some(percent)) => format!("{percent}% ctx"),
        (Some(used), None, _) => format!("{used} tok"),
        _ => "---".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CONTEXT_COMPACT_VALUE, CONTEXT_LIMITS_VALUE, CONTEXT_STATUS_VALUE, context_control_options,
        context_usage_message,
    };
    use crate::thread::ContextUsageSource;
    use codex_app_server_protocol::{RateLimitSnapshot, RateLimitWindow};
    use codex_protocol::account::PlanType;

    #[test]
    fn context_messages_include_usage() {
        assert_eq!(
            context_usage_message(Some(157_835), Some(258_400), Some(ContextUsageSource::Live)),
            "Context usage: 157835/258400 tokens.\nSource: live."
        );
    }

    #[test]
    fn context_options_include_status_actions_and_compact() {
        let options = context_control_options(
            Some(157_835),
            Some(258_400),
            Some(61),
            Some(ContextUsageSource::Live),
            None,
            false,
        );
        assert_eq!(options[0].value.0.as_ref(), CONTEXT_STATUS_VALUE);
        assert_eq!(options[1].value.0.as_ref(), CONTEXT_LIMITS_VALUE);
        assert_eq!(options[2].value.0.as_ref(), CONTEXT_COMPACT_VALUE);
    }

    #[test]
    fn context_options_use_dashes_when_usage_is_unknown() {
        let options = context_control_options(None, None, None, None, None, false);
        assert_eq!(options[0].name, "---");
        assert_eq!(options[1].name, "5h -- · wk --");
        assert_eq!(options[2].name, "Compact now");
    }

    #[test]
    fn context_status_shows_percentage_only() {
        let options = context_control_options(
            Some(195_499),
            Some(258_400),
            Some(76),
            Some(ContextUsageSource::Cached),
            None,
            false,
        );
        assert_eq!(options[0].name, "76% ctx");
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

        let options = context_control_options(
            Some(195_499),
            Some(258_400),
            Some(76),
            Some(ContextUsageSource::Live),
            Some(&snapshot),
            false,
        );
        assert_eq!(options[1].name, "5h 80% · wk 94%");
    }
}
