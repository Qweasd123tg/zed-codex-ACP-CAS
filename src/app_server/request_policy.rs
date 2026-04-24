use std::env;
use std::time::Duration;

pub(super) const STARTUP_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const STARTUP_METADATA_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

const STARTUP_REQUEST_TIMEOUT_ENV: &str = "CODEX_ACP_STARTUP_TIMEOUT_MS";
const STARTUP_METADATA_REQUEST_TIMEOUT_ENV: &str = "CODEX_ACP_STARTUP_METADATA_TIMEOUT_MS";

fn parse_timeout_override(value: Option<&str>, fallback: Duration) -> Duration {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(Duration::from_millis)
        .unwrap_or(fallback)
}

fn configured_timeout(env_name: &str, fallback: Duration) -> Duration {
    parse_timeout_override(env::var(env_name).ok().as_deref(), fallback)
}

pub(super) fn request_timeout(method_name: &str) -> Option<Duration> {
    match method_name {
        "initialize" | "thread/start" | "thread/resume" | "thread/list" | "turn/start" => Some(
            configured_timeout(STARTUP_REQUEST_TIMEOUT_ENV, STARTUP_REQUEST_TIMEOUT),
        ),
        "model/list"
        | "account/rateLimits/read"
        | "account/read"
        | "thread/read"
        | "plugin/list" => Some(configured_timeout(
            STARTUP_METADATA_REQUEST_TIMEOUT_ENV,
            STARTUP_METADATA_REQUEST_TIMEOUT,
        )),
        _ => None,
    }
}

pub(super) fn should_reject_request_during_startup(method_name: &str) -> bool {
    matches!(
        method_name,
        "mcpServer/elicitation/request"
            | "account/chatgptAuthTokens/refresh"
            | "applyPatchApproval"
            | "execCommandApproval"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        STARTUP_METADATA_REQUEST_TIMEOUT, STARTUP_REQUEST_TIMEOUT, configured_timeout,
        parse_timeout_override, request_timeout, should_reject_request_during_startup,
    };
    use std::time::Duration;

    #[test]
    fn applies_longer_timeout_to_critical_startup_requests() {
        assert_eq!(request_timeout("initialize"), Some(STARTUP_REQUEST_TIMEOUT));
        assert_eq!(
            request_timeout("thread/start"),
            Some(STARTUP_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("thread/resume"),
            Some(STARTUP_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("thread/list"),
            Some(STARTUP_REQUEST_TIMEOUT)
        );
        assert_eq!(request_timeout("turn/start"), Some(STARTUP_REQUEST_TIMEOUT));
    }

    #[test]
    fn applies_shorter_timeout_to_startup_metadata_requests() {
        assert_eq!(
            request_timeout("model/list"),
            Some(STARTUP_METADATA_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("account/rateLimits/read"),
            Some(STARTUP_METADATA_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("thread/read"),
            Some(STARTUP_METADATA_REQUEST_TIMEOUT)
        );
    }

    #[test]
    fn leaves_runtime_stream_requests_unbounded() {
        assert_eq!(request_timeout("turn/interrupt"), None);
    }

    #[test]
    fn configured_timeout_falls_back_for_missing_invalid_or_zero_values() {
        let fallback = Duration::from_secs(7);
        assert_eq!(configured_timeout("__MISSING__", fallback), fallback);
        assert_eq!(parse_timeout_override(Some("oops"), fallback), fallback);
        assert_eq!(parse_timeout_override(Some("0"), fallback), fallback);
        assert_eq!(
            parse_timeout_override(Some("1500"), fallback),
            Duration::from_millis(1500)
        );
    }

    #[test]
    fn rejects_only_known_unsupported_startup_requests() {
        assert!(should_reject_request_during_startup(
            "mcpServer/elicitation/request"
        ));
        assert!(should_reject_request_during_startup(
            "account/chatgptAuthTokens/refresh"
        ));
        assert!(should_reject_request_during_startup("applyPatchApproval"));
        assert!(should_reject_request_during_startup("execCommandApproval"));
        assert!(!should_reject_request_during_startup(
            "toolRequest/userInput"
        ));
        assert!(!should_reject_request_during_startup("dynamicToolCall"));
    }
}
