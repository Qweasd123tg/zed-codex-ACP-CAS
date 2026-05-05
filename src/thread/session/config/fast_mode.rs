//! Fast-mode/service-tier helpers for session_config.

use crate::thread::{ServiceTier, SessionConfigSelectOption};

const FAST_MODE_OFF_VALUE: &str = "standard";
const FAST_MODE_ON_VALUE: &str = "fast";
const FAST_MODE_FLEX_VALUE: &str = "flex";

pub(in crate::thread) fn fast_mode_value(service_tier: Option<ServiceTier>) -> &'static str {
    match service_tier {
        Some(ServiceTier::Fast) => FAST_MODE_ON_VALUE,
        Some(ServiceTier::Flex) => FAST_MODE_FLEX_VALUE,
        None => FAST_MODE_OFF_VALUE,
    }
}

pub(in crate::thread) fn parse_fast_mode_value(value: &str) -> Option<Option<ServiceTier>> {
    match value {
        FAST_MODE_OFF_VALUE | "off" => Some(None),
        FAST_MODE_ON_VALUE | "on" => Some(Some(ServiceTier::Fast)),
        FAST_MODE_FLEX_VALUE => Some(Some(ServiceTier::Flex)),
        _ => None,
    }
}

pub(in crate::thread) fn service_tier_override_from_config(
    service_tier: Option<ServiceTier>,
) -> Option<Option<ServiceTier>> {
    service_tier.map(Some)
}

pub(in crate::thread) fn service_tier_override_from_session(
    service_tier: Option<ServiceTier>,
) -> Option<Option<ServiceTier>> {
    Some(service_tier)
}

pub(in crate::thread) fn fast_mode_options(
    _current_service_tier: Option<ServiceTier>,
) -> Vec<SessionConfigSelectOption> {
    vec![
        SessionConfigSelectOption::new(FAST_MODE_OFF_VALUE, "Standard")
            .description("Use the default service tier."),
        SessionConfigSelectOption::new(FAST_MODE_ON_VALUE, "Fast")
            .description("Request the Fast service tier for new turns when available."),
        SessionConfigSelectOption::new(FAST_MODE_FLEX_VALUE, "Flex")
            .description("Request the Flex service tier for new turns when available."),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        fast_mode_options, fast_mode_value, parse_fast_mode_value,
        service_tier_override_from_config, service_tier_override_from_session,
    };
    use crate::thread::ServiceTier;

    #[test]
    fn fast_mode_values_parse_standard_and_fast() {
        assert_eq!(fast_mode_value(None), "standard");
        assert_eq!(fast_mode_value(Some(ServiceTier::Fast)), "fast");
        assert_eq!(parse_fast_mode_value("standard"), Some(None));
        assert_eq!(parse_fast_mode_value("fast"), Some(Some(ServiceTier::Fast)));
    }

    #[test]
    fn fast_mode_parser_accepts_on_off_aliases() {
        assert_eq!(parse_fast_mode_value("off"), Some(None));
        assert_eq!(parse_fast_mode_value("on"), Some(Some(ServiceTier::Fast)));
        assert_eq!(parse_fast_mode_value("invalid"), None);
    }

    #[test]
    fn fast_mode_options_keep_flex_reachable() {
        for tier in [None, Some(ServiceTier::Fast), Some(ServiceTier::Flex)] {
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
            service_tier_override_from_config(Some(ServiceTier::Fast)),
            Some(Some(ServiceTier::Fast))
        );
        assert_eq!(
            service_tier_override_from_config(Some(ServiceTier::Flex)),
            Some(Some(ServiceTier::Flex))
        );
    }

    #[test]
    fn session_service_tier_override_sends_explicit_clear_when_unset() {
        assert_eq!(service_tier_override_from_session(None), Some(None));
        assert_eq!(
            service_tier_override_from_session(Some(ServiceTier::Fast)),
            Some(Some(ServiceTier::Fast))
        );
        assert_eq!(
            service_tier_override_from_session(Some(ServiceTier::Flex)),
            Some(Some(ServiceTier::Flex))
        );
    }
}
