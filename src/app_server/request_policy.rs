use std::env;
use std::time::Duration;

pub(super) const STARTUP_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const STARTUP_METADATA_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

const STARTUP_REQUEST_TIMEOUT_ENV: &str = "CODEX_ACP_STARTUP_TIMEOUT_MS";
const STARTUP_METADATA_REQUEST_TIMEOUT_ENV: &str = "CODEX_ACP_STARTUP_METADATA_TIMEOUT_MS";

fn parse_timeout_override(
    env_name: &str,
    value: Option<&str>,
    fallback: Duration,
) -> Result<Duration, String> {
    let Some(value) = value else {
        return Ok(fallback);
    };
    let trimmed = value.trim();
    let millis = trimmed.parse::<u64>().map_err(|_| {
        format!("{env_name} must be a positive integer number of milliseconds, got `{value}`")
    })?;
    if millis == 0 {
        return Err(format!(
            "{env_name} must be a positive integer number of milliseconds, got `0`"
        ));
    }
    Ok(Duration::from_millis(millis))
}

fn timeout_override_value(
    env_name: &str,
    value: Result<String, env::VarError>,
) -> Result<Option<String>, String> {
    let value = match value {
        Ok(value) => Some(value),
        Err(env::VarError::NotPresent) => None,
        Err(env::VarError::NotUnicode(_)) => {
            return Err(format!(
                "{env_name} must be valid Unicode containing a positive integer number of milliseconds"
            ));
        }
    };
    Ok(value)
}

fn configured_timeout(env_name: &str, fallback: Duration) -> Result<Duration, String> {
    let value = timeout_override_value(env_name, env::var(env_name))?;
    parse_timeout_override(env_name, value.as_deref(), fallback)
}

pub(super) fn request_timeout(method_name: &str) -> Result<Option<Duration>, String> {
    match method_name {
        "initialize"
        | "thread/start"
        | "thread/resume"
        | "thread/list"
        | "thread/compact/start"
        | "turn/start" => {
            configured_timeout(STARTUP_REQUEST_TIMEOUT_ENV, STARTUP_REQUEST_TIMEOUT).map(Some)
        }
        "model/list"
        | "account/rateLimits/read"
        | "account/read"
        | "thread/read"
        | "plugin/list" => configured_timeout(
            STARTUP_METADATA_REQUEST_TIMEOUT_ENV,
            STARTUP_METADATA_REQUEST_TIMEOUT,
        )
        .map(Some),
        _ => Ok(None),
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
        timeout_override_value,
    };
    use std::ffi::OsString;
    use std::time::Duration;

    #[test]
    fn applies_longer_timeout_to_critical_startup_requests() {
        assert_eq!(
            request_timeout("initialize").unwrap(),
            Some(STARTUP_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("thread/start").unwrap(),
            Some(STARTUP_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("thread/resume").unwrap(),
            Some(STARTUP_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("thread/list").unwrap(),
            Some(STARTUP_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("thread/compact/start").unwrap(),
            Some(STARTUP_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("turn/start").unwrap(),
            Some(STARTUP_REQUEST_TIMEOUT)
        );
    }

    #[test]
    fn applies_shorter_timeout_to_startup_metadata_requests() {
        assert_eq!(
            request_timeout("model/list").unwrap(),
            Some(STARTUP_METADATA_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("account/rateLimits/read").unwrap(),
            Some(STARTUP_METADATA_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("thread/read").unwrap(),
            Some(STARTUP_METADATA_REQUEST_TIMEOUT)
        );
    }

    #[test]
    fn leaves_runtime_stream_requests_unbounded() {
        assert_eq!(request_timeout("turn/interrupt").unwrap(), None);
    }

    #[test]
    fn configured_timeout_uses_default_only_when_env_is_missing() {
        let fallback = Duration::from_secs(7);
        assert_eq!(
            configured_timeout("__MISSING__", fallback).unwrap(),
            fallback
        );
        assert_eq!(
            parse_timeout_override("TEST_TIMEOUT_MS", Some("1500"), fallback).unwrap(),
            Duration::from_millis(1500)
        );
    }

    #[test]
    fn timeout_overrides_reject_invalid_or_zero_values() {
        let fallback = Duration::from_secs(7);
        assert!(
            parse_timeout_override("TEST_TIMEOUT_MS", Some("oops"), fallback)
                .unwrap_err()
                .contains("positive integer")
        );
        assert!(
            parse_timeout_override("TEST_TIMEOUT_MS", Some("0"), fallback)
                .unwrap_err()
                .contains("positive integer")
        );
        assert!(
            timeout_override_value(
                "TEST_TIMEOUT_MS",
                Err(std::env::VarError::NotUnicode(OsString::from("500")))
            )
            .unwrap_err()
            .contains("valid Unicode")
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
