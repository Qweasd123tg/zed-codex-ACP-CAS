//! Context usage / compaction helper-ы для session_config.

use crate::thread::SessionConfigSelectOption;

pub(in crate::thread) const CONTEXT_STATUS_VALUE: &str = "status";
pub(in crate::thread) const CONTEXT_USAGE_VALUE: &str = "show_usage";
pub(in crate::thread) const CONTEXT_COMPACT_VALUE: &str = "compact_now";

pub(in crate::thread) fn context_control_options(
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
    compaction_in_progress: bool,
) -> Vec<SessionConfigSelectOption> {
    vec![
        SessionConfigSelectOption::new(
            CONTEXT_STATUS_VALUE,
            context_status_label(used, size, usage_percent, compaction_in_progress),
        )
        .description(context_usage_message(used, size)),
        SessionConfigSelectOption::new(
            CONTEXT_USAGE_VALUE,
            context_usage_option_label(used, size, usage_percent),
        )
        .description("Show the exact used context tokens in chat"),
        SessionConfigSelectOption::new(CONTEXT_COMPACT_VALUE, "Compact now")
            .description("Summarize the conversation to free context window"),
    ]
}

pub(in crate::thread) fn context_usage_message(used: Option<u64>, size: Option<u64>) -> String {
    match (used, size) {
        (Some(used), Some(size)) if size > 0 => {
            let percent = (used as f64 / size as f64) * 100.0;
            format!("Context usage: {used}/{size} tokens ({percent:.1}%).")
        }
        (Some(used), None) => {
            format!("Context usage: {used} tokens (window size is not available yet).")
        }
        _ => {
            "Context usage is not available yet. App-server reports it after the first completed model turn, and resume restores the last cached value when available.".to_string()
        }
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

fn context_usage_option_label(
    used: Option<u64>,
    size: Option<u64>,
    usage_percent: Option<u64>,
) -> String {
    match (used, size, usage_percent) {
        (Some(used), Some(size), Some(_percent)) => format!("Used: {used}/{size} tokens"),
        (Some(used), None, _) => format!("Used: {used} tokens"),
        _ => "Used: ---".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CONTEXT_COMPACT_VALUE, CONTEXT_STATUS_VALUE, CONTEXT_USAGE_VALUE, context_control_options,
        context_usage_message,
    };

    #[test]
    fn context_messages_include_usage() {
        assert_eq!(
            context_usage_message(Some(157_835), Some(258_400)),
            "Context usage: 157835/258400 tokens (61.1%)."
        );
    }

    #[test]
    fn context_options_include_status_actions_and_compact() {
        let options = context_control_options(Some(157_835), Some(258_400), Some(61), false);
        assert_eq!(options[0].value.0.as_ref(), CONTEXT_STATUS_VALUE);
        assert_eq!(options[1].value.0.as_ref(), CONTEXT_USAGE_VALUE);
        assert_eq!(options[2].value.0.as_ref(), CONTEXT_COMPACT_VALUE);
    }

    #[test]
    fn context_options_use_dashes_when_usage_is_unknown() {
        let options = context_control_options(None, None, None, false);
        assert_eq!(options[0].name, "---");
        assert_eq!(options[1].name, "Used: ---");
        assert_eq!(options[2].name, "Compact now");
    }

    #[test]
    fn context_detail_labels_do_not_repeat_percentages() {
        let options = context_control_options(Some(195_499), Some(258_400), Some(76), false);
        assert_eq!(options[1].name, "Used: 195499/258400 tokens");
        assert_eq!(options[2].name, "Compact now");
    }
}
