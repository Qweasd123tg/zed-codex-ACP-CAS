//! Account rate-limit helper-ы для session_config.

use crate::thread::features::resume::common::format_relative_timestamp;
use codex_app_server_protocol::{RateLimitSnapshot, RateLimitWindow};

pub(in crate::thread) fn combined_limits_status_label(
    snapshot: Option<&RateLimitSnapshot>,
) -> String {
    format!(
        "{} · {}",
        five_hour_status_label(snapshot),
        weekly_status_label(snapshot),
    )
}

pub(in crate::thread) fn combined_limits_reset_message(
    snapshot: Option<&RateLimitSnapshot>,
) -> String {
    format!(
        "{}\n{}",
        five_hour_reset_message(snapshot),
        weekly_reset_message(snapshot),
    )
}

pub(in crate::thread) fn five_hour_status_label(snapshot: Option<&RateLimitSnapshot>) -> String {
    format!(
        "5h {}",
        short_window_remaining(snapshot.and_then(|snapshot| snapshot.primary.as_ref()))
    )
}

pub(in crate::thread) fn weekly_status_label(snapshot: Option<&RateLimitSnapshot>) -> String {
    format!(
        "wk {}",
        short_window_remaining(snapshot.and_then(|snapshot| snapshot.secondary.as_ref()))
    )
}

pub(in crate::thread) fn five_hour_reset_message(snapshot: Option<&RateLimitSnapshot>) -> String {
    match snapshot.and_then(|snapshot| snapshot.primary.as_ref()) {
        Some(window) => format_reset_line("5-hour", Some(window)),
        None => "5-hour: unavailable".to_string(),
    }
}

pub(in crate::thread) fn weekly_reset_message(snapshot: Option<&RateLimitSnapshot>) -> String {
    match snapshot.and_then(|snapshot| snapshot.secondary.as_ref()) {
        Some(window) => format_reset_line("Weekly", Some(window)),
        None => "Weekly: unavailable".to_string(),
    }
}

fn short_window_remaining(window: Option<&RateLimitWindow>) -> String {
    window
        .map(|window| format!("{}%", remaining_percent(window.used_percent)))
        .unwrap_or_else(|| "--".to_string())
}

fn format_reset_line(label: &str, window: Option<&RateLimitWindow>) -> String {
    let Some(window) = window else {
        return format!("{label}: unavailable");
    };

    let reset = window
        .resets_at
        .map(format_relative_timestamp)
        .unwrap_or_else(|| "-".to_string());
    format!("{label}: resets {reset}")
}

fn remaining_percent(used_percent: i32) -> i32 {
    100 - clamp_percent(used_percent)
}

fn clamp_percent(value: i32) -> i32 {
    value.clamp(0, 100)
}

#[cfg(test)]
mod tests {
    use super::{
        combined_limits_reset_message, combined_limits_status_label, five_hour_reset_message,
        five_hour_status_label, weekly_reset_message, weekly_status_label,
    };
    use codex_app_server_protocol::{RateLimitSnapshot, RateLimitWindow};
    use codex_protocol::account::PlanType;

    #[test]
    fn rate_limit_status_labels_show_remaining_buckets() {
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

        assert_eq!(five_hour_status_label(Some(&snapshot)), "5h 80%");
        assert_eq!(weekly_status_label(Some(&snapshot)), "wk 94%");
    }

    #[test]
    fn rate_limit_message_mentions_reset_times() {
        let snapshot = RateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: None,
            primary: Some(RateLimitWindow {
                used_percent: 42,
                window_duration_mins: Some(300),
                resets_at: Some(4_102_444_800),
            }),
            secondary: Some(RateLimitWindow {
                used_percent: 5,
                window_duration_mins: Some(10_080),
                resets_at: Some(4_102_531_200),
            }),
            credits: None,
            plan_type: Some(PlanType::Pro),
        };

        assert!(five_hour_reset_message(Some(&snapshot)).contains("5-hour: resets"));
        assert!(weekly_reset_message(Some(&snapshot)).contains("Weekly: resets"));
        assert!(combined_limits_reset_message(Some(&snapshot)).contains("5-hour: resets"));
        assert!(combined_limits_reset_message(Some(&snapshot)).contains("Weekly: resets"));
        assert_eq!(
            combined_limits_status_label(Some(&snapshot)),
            "5h 58% · wk 95%"
        );
    }
}
