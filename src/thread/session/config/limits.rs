//! Account rate-limit helper-ы для session_config.

use crate::thread::{
    features::resume::common::format_relative_timestamp, session_display_maps::DisplayMapsConfig,
};
use codex_app_server_protocol::{RateLimitSnapshot, RateLimitWindow};
use std::time::{SystemTime, UNIX_EPOCH};

const RATE_LIMIT_WARNING_THRESHOLDS: [i32; 4] = [75, 90, 95, 100];

#[derive(Clone, Debug, Default)]
pub(in crate::thread) struct RateLimitWarningState {
    primary_index: usize,
    secondary_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::thread) struct RateLimitWarning {
    pub(in crate::thread) label: String,
    pub(in crate::thread) remaining_percent: Option<i32>,
}

pub(in crate::thread) fn combined_limits_status_label(
    snapshot: Option<&RateLimitSnapshot>,
    display_maps: &DisplayMapsConfig,
) -> String {
    display_maps.render_limits_summary(
        snapshot
            .and_then(|snapshot| snapshot.primary.as_ref())
            .map(|window| remaining_percent(window.used_percent)),
        snapshot
            .and_then(|snapshot| snapshot.secondary.as_ref())
            .map(|window| remaining_percent(window.used_percent)),
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

pub(in crate::thread) fn limits_status_description(
    snapshot: Option<&RateLimitSnapshot>,
    display_maps: &DisplayMapsConfig,
) -> String {
    let mut lines = Vec::new();
    lines.push(combined_limits_status_label(snapshot, display_maps));
    if let Some(snapshot) = snapshot {
        if let Some(identity) = rate_limit_identity_line(snapshot) {
            lines.push(identity);
        }
        if let Some(credits) = credits_line(snapshot) {
            lines.push(credits);
        }
    }
    lines.push(combined_limits_reset_message(snapshot));
    lines.join("\n")
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

pub(in crate::thread) fn take_rate_limit_warnings(
    state: &mut RateLimitWarningState,
    snapshot: &RateLimitSnapshot,
) -> Vec<RateLimitWarning> {
    let mut warnings = Vec::new();
    if let Some(warning) = take_window_warning(
        &mut state.secondary_index,
        snapshot.secondary.as_ref(),
        "weekly",
    ) {
        warnings.push(warning);
    }
    if let Some(warning) = take_window_warning(
        &mut state.primary_index,
        snapshot.primary.as_ref(),
        "5-hour",
    ) {
        warnings.push(warning);
    }
    warnings
}

pub(in crate::thread) fn observe_rate_limit_snapshot(
    state: &mut RateLimitWarningState,
    snapshot: &RateLimitSnapshot,
) {
    observe_window_warning_index(&mut state.secondary_index, snapshot.secondary.as_ref());
    observe_window_warning_index(&mut state.primary_index, snapshot.primary.as_ref());
}

fn take_window_warning(
    warning_index: &mut usize,
    window: Option<&RateLimitWindow>,
    fallback_label: &str,
) -> Option<RateLimitWarning> {
    let window = window?;
    let used_percent = clamp_percent(window.used_percent);
    reset_window_warning_index_after_reset(warning_index, used_percent);
    let mut highest_threshold = None;
    while *warning_index < RATE_LIMIT_WARNING_THRESHOLDS.len()
        && used_percent >= RATE_LIMIT_WARNING_THRESHOLDS[*warning_index]
    {
        highest_threshold = Some(RATE_LIMIT_WARNING_THRESHOLDS[*warning_index]);
        *warning_index += 1;
    }

    let threshold = highest_threshold?;
    let label = limit_duration_label(window).unwrap_or_else(|| fallback_label.to_string());
    if threshold >= 100 {
        return Some(RateLimitWarning {
            label,
            remaining_percent: None,
        });
    }

    Some(RateLimitWarning {
        label,
        remaining_percent: Some(100 - threshold),
    })
}

fn reset_window_warning_index_after_reset(warning_index: &mut usize, used_percent: i32) {
    if crossed_threshold_count(used_percent) == 0 {
        *warning_index = 0;
    }
}

fn observe_window_warning_index(warning_index: &mut usize, window: Option<&RateLimitWindow>) {
    if let Some(window) = window {
        *warning_index = crossed_threshold_count(clamp_percent(window.used_percent));
    }
}

fn crossed_threshold_count(used_percent: i32) -> usize {
    RATE_LIMIT_WARNING_THRESHOLDS
        .iter()
        .take_while(|threshold| used_percent >= **threshold)
        .count()
}

fn format_reset_line(label: &str, window: Option<&RateLimitWindow>) -> String {
    let Some(window) = window else {
        return format!("{label}: unavailable");
    };

    let reset = window
        .resets_at
        .map(format_reset_timestamp)
        .unwrap_or_else(|| "-".to_string());
    format!("{label}: resets {reset}")
}

fn format_reset_timestamp(unix_seconds: i64) -> String {
    if unix_seconds <= 0 {
        return "-".to_string();
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();

    if unix_seconds > now {
        return format_reset_delta((unix_seconds - now) as u64);
    }

    format_relative_timestamp(unix_seconds)
}

fn rate_limit_identity_line(snapshot: &RateLimitSnapshot) -> Option<String> {
    match (
        snapshot.limit_name.as_deref().map(str::trim),
        snapshot.limit_id.as_deref().map(str::trim),
    ) {
        (Some(name), Some(id)) if !name.is_empty() && !id.is_empty() && name != id => {
            Some(format!("Limit: {name} ({id})"))
        }
        (Some(name), _) if !name.is_empty() => Some(format!("Limit: {name}")),
        (_, Some(id)) if !id.is_empty() => Some(format!("Limit: {id}")),
        _ => None,
    }
}

fn credits_line(snapshot: &RateLimitSnapshot) -> Option<String> {
    let credits = snapshot.credits.as_ref()?;
    if credits.unlimited {
        return Some("Credits: unlimited".to_string());
    }
    if credits.has_credits {
        return Some(
            credits
                .balance
                .as_deref()
                .map(str::trim)
                .filter(|balance| !balance.is_empty())
                .map(|balance| format!("Credits: {balance}"))
                .unwrap_or_else(|| "Credits: available".to_string()),
        );
    }
    Some("Credits: unavailable".to_string())
}

fn format_reset_delta(delta: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;

    if delta < MINUTE {
        return "soon".to_string();
    }
    if delta < HOUR {
        return format!("in {}m", delta / MINUTE);
    }
    if delta < DAY {
        let hours = delta / HOUR;
        let minutes = (delta % HOUR) / MINUTE;
        if minutes == 0 {
            return format!("in {hours}h");
        }
        return format!("in {hours}h {minutes}m");
    }

    let days = delta / DAY;
    let hours = (delta % DAY) / HOUR;
    if hours == 0 {
        return format!("in {days}d");
    }
    format!("in {days}d {hours}h")
}

fn limit_duration_label(window: &RateLimitWindow) -> Option<String> {
    let minutes = window.window_duration_mins?;
    if minutes <= 0 {
        return None;
    }

    const MINUTES_PER_HOUR: i64 = 60;
    const MINUTES_PER_DAY: i64 = 24 * MINUTES_PER_HOUR;
    const MINUTES_PER_WEEK: i64 = 7 * MINUTES_PER_DAY;
    const ROUNDING_BIAS_MINUTES: i64 = 3;

    if minutes <= MINUTES_PER_DAY.saturating_add(ROUNDING_BIAS_MINUTES) {
        let hours = ((minutes + MINUTES_PER_HOUR / 2) / MINUTES_PER_HOUR).max(1);
        return Some(format!("{hours}h"));
    }
    if (minutes - MINUTES_PER_WEEK).abs() <= ROUNDING_BIAS_MINUTES {
        return Some("weekly".to_string());
    }

    let days = ((minutes + MINUTES_PER_DAY / 2) / MINUTES_PER_DAY).max(1);
    Some(format!("{days}d"))
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
        RateLimitWarning, RateLimitWarningState, combined_limits_reset_message,
        combined_limits_status_label, five_hour_reset_message, format_reset_delta,
        limits_status_description, observe_rate_limit_snapshot, take_rate_limit_warnings,
        weekly_reset_message,
    };
    use crate::thread::session_display_maps::{
        DisplayMapsConfig, LimitSummaryWindow, LimitsDisplayMapSelection,
    };
    use codex_app_server_protocol::{CreditsSnapshot, RateLimitSnapshot, RateLimitWindow};
    use codex_protocol::account::PlanType;

    #[test]
    fn rate_limit_status_labels_show_remaining_buckets() {
        let display_maps = DisplayMapsConfig::default();
        assert_eq!(
            display_maps.render_limits_summary(Some(80), Some(94)),
            "5h 80% · wk 94%"
        );
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
        let display_maps = DisplayMapsConfig::default();
        assert_eq!(
            combined_limits_status_label(Some(&snapshot), &display_maps),
            "5h 58% · wk 95%"
        );
    }

    #[test]
    fn rate_limit_summary_can_show_only_five_hour_window() {
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
        let mut display_maps = DisplayMapsConfig::default();
        display_maps.limits = LimitsDisplayMapSelection {
            summary: vec![LimitSummaryWindow::Primary],
            ..LimitsDisplayMapSelection::default()
        };

        assert_eq!(
            combined_limits_status_label(Some(&snapshot), &display_maps),
            "5h 58%"
        );
    }

    #[test]
    fn rate_limit_description_includes_identity_and_credits() {
        let snapshot = RateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: Some("Codex".to_string()),
            primary: Some(RateLimitWindow {
                used_percent: 42,
                window_duration_mins: Some(300),
                resets_at: Some(4_102_444_800),
            }),
            secondary: None,
            credits: Some(CreditsSnapshot {
                has_credits: true,
                unlimited: false,
                balance: Some("$12.34".to_string()),
            }),
            plan_type: Some(PlanType::Pro),
        };

        let description = limits_status_description(Some(&snapshot), &DisplayMapsConfig::default());

        assert!(description.contains("Limit: Codex (codex)"));
        assert!(description.contains("Credits: $12.34"));
        assert!(description.contains("5-hour: resets"));
    }

    #[test]
    fn rate_limit_reset_delta_keeps_minutes_for_hourly_windows() {
        assert_eq!(format_reset_delta(4 * 60 * 60 + 23 * 60 + 12), "in 4h 23m");
        assert_eq!(format_reset_delta(4 * 60 * 60), "in 4h");
        assert_eq!(format_reset_delta(42 * 60 + 59), "in 42m");
        assert_eq!(format_reset_delta(26 * 60 * 60 + 10 * 60), "in 1d 2h");
    }

    #[test]
    fn rate_limit_warnings_fire_once_per_threshold() {
        let mut state = RateLimitWarningState::default();
        let mut snapshot = RateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: None,
            primary: Some(RateLimitWindow {
                used_percent: 74,
                window_duration_mins: Some(300),
                resets_at: Some(4_102_444_800),
            }),
            secondary: Some(RateLimitWindow {
                used_percent: 10,
                window_duration_mins: Some(10_080),
                resets_at: Some(4_102_531_200),
            }),
            credits: None,
            plan_type: Some(PlanType::Plus),
        };

        assert!(take_rate_limit_warnings(&mut state, &snapshot).is_empty());

        snapshot.primary.as_mut().unwrap().used_percent = 91;
        let warnings = take_rate_limit_warnings(&mut state, &snapshot);
        assert_eq!(
            warnings,
            vec![RateLimitWarning {
                label: "5h".to_string(),
                remaining_percent: Some(10),
            }]
        );

        assert!(take_rate_limit_warnings(&mut state, &snapshot).is_empty());

        snapshot.primary.as_mut().unwrap().used_percent = 100;
        let warnings = take_rate_limit_warnings(&mut state, &snapshot);
        assert_eq!(
            warnings,
            vec![RateLimitWarning {
                label: "5h".to_string(),
                remaining_percent: None,
            }]
        );
    }

    #[test]
    fn observed_rate_limits_do_not_warn_again_until_a_new_threshold_crossing() {
        let mut state = RateLimitWarningState::default();
        let mut snapshot = RateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: None,
            primary: Some(RateLimitWindow {
                used_percent: 100,
                window_duration_mins: Some(300),
                resets_at: Some(4_102_444_800),
            }),
            secondary: None,
            credits: None,
            plan_type: Some(PlanType::Plus),
        };

        observe_rate_limit_snapshot(&mut state, &snapshot);
        assert!(take_rate_limit_warnings(&mut state, &snapshot).is_empty());

        snapshot.primary.as_mut().unwrap().used_percent = 10;
        assert!(take_rate_limit_warnings(&mut state, &snapshot).is_empty());

        snapshot.primary.as_mut().unwrap().used_percent = 76;
        assert_eq!(
            take_rate_limit_warnings(&mut state, &snapshot),
            vec![RateLimitWarning {
                label: "5h".to_string(),
                remaining_percent: Some(25),
            }]
        );
    }

    #[test]
    fn rate_limit_warnings_do_not_repeat_after_small_usage_dips() {
        let mut state = RateLimitWarningState::default();
        let mut snapshot = RateLimitSnapshot {
            limit_id: Some("codex".to_string()),
            limit_name: None,
            primary: Some(RateLimitWindow {
                used_percent: 91,
                window_duration_mins: Some(300),
                resets_at: Some(4_102_444_800),
            }),
            secondary: None,
            credits: None,
            plan_type: Some(PlanType::Plus),
        };

        assert_eq!(
            take_rate_limit_warnings(&mut state, &snapshot),
            vec![RateLimitWarning {
                label: "5h".to_string(),
                remaining_percent: Some(10),
            }]
        );

        snapshot.primary.as_mut().unwrap().used_percent = 89;
        assert!(take_rate_limit_warnings(&mut state, &snapshot).is_empty());

        snapshot.primary.as_mut().unwrap().used_percent = 91;
        assert!(take_rate_limit_warnings(&mut state, &snapshot).is_empty());
    }
}
