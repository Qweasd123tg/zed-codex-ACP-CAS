//! Fast-mode/service-tier helpers for session_config.

use crate::thread::SessionConfigSelectOption;

const FAST_MODE_OFF_VALUE: &str = "standard";
const FAST_MODE_ON_VALUE: &str = "fast";
const FAST_MODE_FLEX_VALUE: &str = "flex";

pub(in crate::thread) fn fast_mode_value(service_tier: Option<&str>) -> &str {
    match service_tier {
        Some("fast" | "priority") => FAST_MODE_ON_VALUE,
        Some("flex") => FAST_MODE_FLEX_VALUE,
        Some(service_tier) => service_tier,
        None => FAST_MODE_OFF_VALUE,
    }
}

pub(in crate::thread) fn parse_fast_mode_value(value: &str) -> Option<Option<String>> {
    match value {
        FAST_MODE_OFF_VALUE => Some(None),
        FAST_MODE_ON_VALUE => Some(Some(FAST_MODE_ON_VALUE.to_string())),
        FAST_MODE_FLEX_VALUE => Some(Some(FAST_MODE_FLEX_VALUE.to_string())),
        value if !value.trim().is_empty() => Some(Some(value.to_string())),
        _ => None,
    }
}

pub(in crate::thread) fn service_tier_override_from_config(
    service_tier: Option<&str>,
) -> Option<Option<String>> {
    service_tier.map(|service_tier| Some(service_tier.to_string()))
}

pub(in crate::thread) fn service_tier_override_from_session(
    service_tier: Option<&str>,
) -> Option<Option<String>> {
    Some(service_tier.map(ToString::to_string))
}

pub(in crate::thread) fn fast_mode_options(
    current_service_tier: Option<&str>,
) -> Vec<SessionConfigSelectOption> {
    let mut options = vec![
        SessionConfigSelectOption::new(FAST_MODE_OFF_VALUE, "Standard")
            .description("Use the default service tier."),
        SessionConfigSelectOption::new(FAST_MODE_ON_VALUE, "Fast")
            .description("Request the Fast service tier for new turns when available."),
        SessionConfigSelectOption::new(FAST_MODE_FLEX_VALUE, "Flex")
            .description("Request the Flex service tier for new turns when available."),
    ];
    let current_value = fast_mode_value(current_service_tier);
    if !matches!(
        current_value,
        FAST_MODE_OFF_VALUE | FAST_MODE_ON_VALUE | FAST_MODE_FLEX_VALUE
    ) {
        options.push(
            SessionConfigSelectOption::new(current_value.to_string(), current_value.to_string())
                .description("Service tier reported by the current Codex backend."),
        );
    }
    options
}

#[cfg(test)]
mod tests {
    use super::{
        fast_mode_options, fast_mode_value, parse_fast_mode_value,
        service_tier_override_from_config, service_tier_override_from_session,
    };
    #[test]
    fn fast_mode_values_parse_standard_and_fast() {
        assert_eq!(fast_mode_value(None), "standard");
        assert_eq!(fast_mode_value(Some("fast")), "fast");
        assert_eq!(parse_fast_mode_value("standard"), Some(None));
        assert_eq!(
            parse_fast_mode_value("fast"),
            Some(Some("fast".to_string()))
        );
    }

    #[test]
    fn fast_mode_options_keep_flex_reachable() {
        for tier in [None, Some("fast"), Some("flex")] {
            let values: Vec<_> = fast_mode_options(tier)
                .into_iter()
                .map(|option| option.value.0.to_string())
                .collect();
            assert_eq!(values, vec!["standard", "fast", "flex"]);
        }
    }

    #[test]
    fn fast_mode_options_use_speed_labels() {
        let labels: Vec<_> = fast_mode_options(None)
            .into_iter()
            .map(|option| option.name)
            .collect();
        assert_eq!(labels, vec!["Standard", "Fast", "Flex"]);
    }

    #[test]
    fn config_service_tier_override_preserves_app_server_default_when_unset() {
        assert_eq!(service_tier_override_from_config(None), None);
        assert_eq!(
            service_tier_override_from_config(Some("fast")),
            Some(Some("fast".to_string()))
        );
        assert_eq!(
            service_tier_override_from_config(Some("flex")),
            Some(Some("flex".to_string()))
        );
    }

    #[test]
    fn session_service_tier_override_sends_explicit_clear_when_unset() {
        assert_eq!(service_tier_override_from_session(None), Some(None));
        assert_eq!(
            service_tier_override_from_session(Some("fast")),
            Some(Some("fast".to_string()))
        );
        assert_eq!(
            service_tier_override_from_session(Some("flex")),
            Some(Some("flex".to_string()))
        );
    }
}
