//! Reasoning/model helper-ы для session_config.

use crate::thread::{AppModel, ReasoningEffort};

pub(in crate::thread) fn find_model_for_current<'a>(
    models: &'a [AppModel],
    current_model: &str,
) -> Option<&'a AppModel> {
    models
        .iter()
        .find(|model| model.id == current_model || model.model == current_model)
}

pub(in crate::thread) fn resolve_reasoning_effort(
    models: &[AppModel],
    current_model: &str,
    current: Option<ReasoningEffort>,
) -> ReasoningEffort {
    current.unwrap_or_else(|| {
        normalize_reasoning_effort_for_model(models, current_model, ReasoningEffort::default())
    })
}

pub(in crate::thread) fn normalize_reasoning_effort_for_model(
    models: &[AppModel],
    current_model: &str,
    current_effort: ReasoningEffort,
) -> ReasoningEffort {
    let Some(model) = find_model_for_current(models, current_model) else {
        return current_effort;
    };
    if model
        .supported_reasoning_efforts
        .iter()
        .any(|option| option.reasoning_effort == current_effort)
    {
        return current_effort;
    }
    model.default_reasoning_effort.clone()
}

pub(in crate::thread) fn parse_reasoning_effort(value: &str) -> Option<ReasoningEffort> {
    value.parse().ok()
}

pub(in crate::thread) fn reasoning_effort_value(effort: &ReasoningEffort) -> &str {
    effort.as_str()
}

pub(in crate::thread) fn reasoning_effort_label(effort: &ReasoningEffort) -> String {
    match effort {
        ReasoningEffort::None => "None".to_string(),
        ReasoningEffort::Minimal => "Minimal".to_string(),
        ReasoningEffort::Low => "Low".to_string(),
        ReasoningEffort::Medium => "Medium".to_string(),
        ReasoningEffort::High => "High".to_string(),
        ReasoningEffort::XHigh => "Extra High".to_string(),
        ReasoningEffort::Max => "Max".to_string(),
        ReasoningEffort::Ultra => "Ultra".to_string(),
        ReasoningEffort::Custom(value) => value.clone(),
    }
}

pub(in crate::thread) fn reasoning_effort_description_label(effort: &ReasoningEffort) -> String {
    reasoning_effort_label(effort)
}

#[cfg(test)]
mod tests {
    use super::resolve_reasoning_effort;
    use crate::thread::ReasoningEffort;

    #[test]
    fn backend_reasoning_effort_is_preserved_without_closed_enum_normalization() {
        for effort in [
            ReasoningEffort::Max,
            ReasoningEffort::Ultra,
            ReasoningEffort::Custom("future-effort".to_string()),
        ] {
            assert_eq!(
                resolve_reasoning_effort(&[], "future-model", Some(effort.clone())),
                effort
            );
        }
    }
}
