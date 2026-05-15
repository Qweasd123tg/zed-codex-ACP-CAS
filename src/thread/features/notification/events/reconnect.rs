//! Общие helper-ы reconnect/stall прогресса.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::thread) struct ReconnectProgress {
    pub(in crate::thread) current: u32,
    pub(in crate::thread) total: u32,
}

pub(in crate::thread) fn parse_reconnect_progress(message: &str) -> Option<ReconnectProgress> {
    let trimmed = message.trim();
    let progress = trimmed
        .find("Reconnecting... ")
        .map(|index| &trimmed[index + "Reconnecting... ".len()..])?;
    let (current, rest) = progress.split_once('/')?;
    let total = rest
        .split_whitespace()
        .next()
        .unwrap_or(rest)
        .trim_matches(|ch: char| !ch.is_ascii_digit());
    let current = current.trim().parse().ok()?;
    let total = total.parse().ok()?;
    Some(ReconnectProgress { current, total })
}

pub(in crate::thread) fn format_reconnect_status(progress: ReconnectProgress) -> String {
    crate::thread::session_client::format_system_message(
        "status",
        "Reconnecting to app-server",
        &format!(
            "Attempt {}/{}. Waiting for the turn to resume.",
            progress.current, progress.total
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::{ReconnectProgress, format_reconnect_status, parse_reconnect_progress};

    #[test]
    fn parses_reconnect_progress_with_error_prefix() {
        assert_eq!(
            parse_reconnect_progress("\n[error] Reconnecting... 4/5"),
            Some(ReconnectProgress {
                current: 4,
                total: 5,
            })
        );
    }

    #[test]
    fn parses_reconnect_progress_without_error_prefix() {
        assert_eq!(
            parse_reconnect_progress("Reconnecting... 5/5"),
            Some(ReconnectProgress {
                current: 5,
                total: 5,
            })
        );
    }

    #[test]
    fn parses_reconnect_progress_inside_error_text() {
        assert_eq!(
            parse_reconnect_progress("stream dropped; Reconnecting... 1/5 after transport error"),
            Some(ReconnectProgress {
                current: 1,
                total: 5,
            })
        );
    }

    #[test]
    fn formats_reconnect_status_consistently() {
        assert_eq!(
            format_reconnect_status(ReconnectProgress {
                current: 2,
                total: 5,
            }),
            "\n\n> **System / status: Reconnecting to app-server**\n>\n> Attempt 2/5. Waiting for the turn to resume.\n\n"
        );
    }
}
