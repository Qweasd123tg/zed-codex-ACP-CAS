//! Warning/advisory notification-ветки.

use codex_app_server_protocol::{
    ConfigWarningNotification, DeprecationNoticeNotification,
    WindowsWorldWritableWarningNotification,
};

use crate::thread::{ThreadInner, session_config::RateLimitWarning};

pub(in crate::thread) async fn emit_account_rate_limit_warnings(
    inner: &mut ThreadInner,
    warnings: Vec<RateLimitWarning>,
) {
    let Some(notice) = format_account_rate_limit_warning_notice(&warnings) else {
        return;
    };

    inner.client.send_agent_text(notice).await;
}

pub(in crate::thread) async fn emit_config_warning(
    inner: &mut ThreadInner,
    notification: ConfigWarningNotification,
) {
    inner
        .client
        .send_agent_text(format_config_warning(notification))
        .await;
}

pub(in crate::thread) async fn emit_deprecation_notice(
    inner: &mut ThreadInner,
    notification: DeprecationNoticeNotification,
) {
    inner
        .client
        .send_agent_text(format_deprecation_notice(notification))
        .await;
}

pub(in crate::thread) async fn emit_windows_world_writable_warning(
    inner: &mut ThreadInner,
    notification: WindowsWorldWritableWarningNotification,
) {
    inner
        .client
        .send_agent_text(format_windows_world_writable_warning(notification))
        .await;
}

fn format_account_rate_limit_warning_notice(warnings: &[RateLimitWarning]) -> Option<String> {
    let active: Vec<&RateLimitWarning> = warnings
        .iter()
        .filter(|warning| !warning.label.trim().is_empty())
        .collect();
    if active.is_empty() {
        return None;
    }

    let exhausted: Vec<&str> = active
        .iter()
        .filter(|warning| warning.remaining_percent.is_none())
        .map(|warning| warning.label.trim())
        .collect();
    if !exhausted.is_empty() {
        return Some(format!(
            "\n\n⛔ Лимиты Codex: {} исчерпан{}. Подробности: `/status`.\n\n",
            format_label_list(&exhausted),
            if exhausted.len() == 1 { "" } else { "ы" }
        ));
    }

    let percent = active
        .iter()
        .filter_map(|warning| warning.remaining_percent)
        .min()?;
    let labels: Vec<&str> = active.iter().map(|warning| warning.label.trim()).collect();
    Some(format!(
        "\n\n⚠️ Лимиты Codex: осталось меньше {percent}% для {}. Подробности: `/status`.\n\n",
        format_label_list(&labels)
    ))
}

fn format_label_list(labels: &[&str]) -> String {
    match labels {
        [] => String::new(),
        [label] => (*label).to_string(),
        [first, second] => format!("{first} и {second}"),
        _ => {
            let mut formatted = labels[..labels.len() - 1].join(", ");
            formatted.push_str(" и ");
            formatted.push_str(labels[labels.len() - 1]);
            formatted
        }
    }
}

fn format_config_warning(notification: ConfigWarningNotification) -> String {
    let ConfigWarningNotification {
        summary,
        details,
        path,
        range,
    } = notification;

    let mut lines = vec![format!("[warning] {summary}")];
    if let Some(location) = format_config_warning_location(path.as_deref(), range.as_ref()) {
        lines.push(format!("Location: {location}"));
    }
    if let Some(details) = details
        && !details.trim().is_empty()
    {
        lines.push(details);
    }
    lines.join("\n")
}

fn format_config_warning_location(
    path: Option<&str>,
    range: Option<&codex_app_server_protocol::TextRange>,
) -> Option<String> {
    match (path, range) {
        (Some(path), Some(range)) => Some(format!(
            "{path}:{}:{}",
            range.start.line, range.start.column
        )),
        (Some(path), None) => Some(path.to_string()),
        (None, Some(range)) => Some(format!("line {}:{}", range.start.line, range.start.column)),
        (None, None) => None,
    }
}

fn format_deprecation_notice(notification: DeprecationNoticeNotification) -> String {
    let DeprecationNoticeNotification { summary, details } = notification;
    match details {
        Some(details) if !details.trim().is_empty() => {
            format!("[deprecated] {summary}\n{details}")
        }
        _ => format!("[deprecated] {summary}"),
    }
}

fn format_windows_world_writable_warning(
    notification: WindowsWorldWritableWarningNotification,
) -> String {
    let WindowsWorldWritableWarningNotification {
        sample_paths,
        extra_count,
        failed_scan,
    } = notification;

    let mut lines =
        vec!["[warning] Windows sandbox cannot protect world-writable directories.".to_string()];
    if !sample_paths.is_empty() {
        lines.push(format!("Examples: {}", sample_paths.join(", ")));
    }
    if extra_count > 0 {
        lines.push(format!("And {extra_count} more path(s)."));
    }
    if failed_scan {
        lines.push("Directory scan was incomplete, so the warning set may be partial.".to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        format_account_rate_limit_warning_notice, format_config_warning, format_deprecation_notice,
        format_windows_world_writable_warning,
    };
    use crate::thread::session_config::RateLimitWarning;
    use codex_app_server_protocol::{
        ConfigWarningNotification, DeprecationNoticeNotification, TextPosition, TextRange,
        WindowsWorldWritableWarningNotification,
    };

    #[test]
    fn formats_config_warning_with_location_and_details() {
        let rendered = format_config_warning(ConfigWarningNotification {
            summary: "Unknown field".to_string(),
            details: Some("Remove it or rename it.".to_string()),
            path: Some("/tmp/config.toml".to_string()),
            range: Some(TextRange {
                start: TextPosition { line: 3, column: 5 },
                end: TextPosition {
                    line: 3,
                    column: 10,
                },
            }),
        });

        assert_eq!(
            rendered,
            "[warning] Unknown field\nLocation: /tmp/config.toml:3:5\nRemove it or rename it."
        );
    }

    #[test]
    fn formats_account_rate_limit_warnings_as_compact_notice() {
        let rendered = format_account_rate_limit_warning_notice(&[
            RateLimitWarning {
                label: "weekly".to_string(),
                remaining_percent: Some(25),
            },
            RateLimitWarning {
                label: "5h".to_string(),
                remaining_percent: Some(25),
            },
        ]);

        assert_eq!(
            rendered.as_deref(),
            Some(
                "\n\n⚠️ Лимиты Codex: осталось меньше 25% для weekly и 5h. Подробности: `/status`.\n\n"
            )
        );
    }

    #[test]
    fn formats_exhausted_account_rate_limit_warning_as_compact_notice() {
        let rendered = format_account_rate_limit_warning_notice(&[RateLimitWarning {
            label: "5h".to_string(),
            remaining_percent: None,
        }]);

        assert_eq!(
            rendered.as_deref(),
            Some("\n\n⛔ Лимиты Codex: 5h исчерпан. Подробности: `/status`.\n\n")
        );
    }

    #[test]
    fn skips_empty_account_rate_limit_warning_notice() {
        let rendered = format_account_rate_limit_warning_notice(&[RateLimitWarning {
            label: "  ".to_string(),
            remaining_percent: Some(25),
        }]);

        assert_eq!(rendered, None);
    }

    #[test]
    fn formats_deprecation_notice_with_optional_details() {
        assert_eq!(
            format_deprecation_notice(DeprecationNoticeNotification {
                summary: "Legacy flag is deprecated".to_string(),
                details: Some("Use the new selector instead.".to_string()),
            }),
            "[deprecated] Legacy flag is deprecated\nUse the new selector instead."
        );
    }

    #[test]
    fn formats_windows_world_writable_warning_with_examples() {
        let rendered =
            format_windows_world_writable_warning(WindowsWorldWritableWarningNotification {
                sample_paths: vec!["C:\\temp".to_string(), "D:\\shared".to_string()],
                extra_count: 2,
                failed_scan: true,
            });

        assert_eq!(
            rendered,
            "[warning] Windows sandbox cannot protect world-writable directories.\nExamples: C:\\temp, D:\\shared\nAnd 2 more path(s).\nDirectory scan was incomplete, so the warning set may be partial."
        );
    }
}
