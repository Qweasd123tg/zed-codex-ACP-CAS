use crate::thread::{
    AppModel, ReasoningEffort, session_config::normalize_reasoning_effort_for_model,
};

use super::{
    ModelSelectorModelEntry, ModelSelectorPreferences, ModelSelectorReasoningEffortEntry,
    SelectorLayoutEntry, SelectorLayoutPreferences,
};

pub(super) fn materialized_model_selector(
    preferences: &ModelSelectorPreferences,
    models: &[AppModel],
) -> ModelSelectorPreferences {
    let mut materialized = preferences.clone();
    if materialized.models.is_empty() {
        materialized.models = models
            .iter()
            .map(|model| ModelSelectorModelEntry::materialized(&model.id, None))
            .collect();
    } else {
        materialized.models = materialized
            .models
            .iter()
            .map(|entry| ModelSelectorModelEntry::materialized(entry.id(), Some(entry)))
            .collect();
    }
    if materialized.reasoning_efforts.is_empty() {
        materialized.reasoning_efforts = all_reasoning_efforts()
            .into_iter()
            .map(|effort| ModelSelectorReasoningEffortEntry::materialized(effort, None))
            .collect();
    } else {
        materialized.reasoning_efforts = materialized
            .reasoning_efforts
            .iter()
            .map(|entry| ModelSelectorReasoningEffortEntry::materialized(entry.id(), Some(entry)))
            .collect();
    }
    materialized
}

pub(super) fn normalize_reasoning_effort_for_preferences(
    models: &[AppModel],
    current_model: &str,
    preferences: &ModelSelectorPreferences,
    effort: ReasoningEffort,
) -> ReasoningEffort {
    if preferences.explicitly_enables_reasoning_effort(effort) {
        effort
    } else {
        normalize_reasoning_effort_for_model(models, current_model, effort)
    }
}

fn all_reasoning_efforts() -> [ReasoningEffort; 6] {
    [
        ReasoningEffort::None,
        ReasoningEffort::Minimal,
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
        ReasoningEffort::XHigh,
    ]
}

pub(super) fn materialized_selector_layout(
    layout: &SelectorLayoutPreferences,
) -> SelectorLayoutPreferences {
    SelectorLayoutPreferences {
        order: Some(layout.order.clone().unwrap_or_else(default_selector_order)),
        permissions: Some(materialized_selector_entry(
            layout.permissions.as_ref(),
            "Permissions",
            &["workflow", "guarded", "bypass"],
        )),
        model: Some(materialized_selector_entry(
            layout.model.as_ref(),
            "Model",
            &["models", "effort", "speed"],
        )),
        context_control: Some(materialized_selector_entry(
            layout.context_control.as_ref(),
            "Context",
            &["display", "integrations", "actions"],
        )),
    }
}

fn materialized_selector_entry(
    entry: Option<&SelectorLayoutEntry>,
    default_name: &str,
    default_groups: &[&str],
) -> SelectorLayoutEntry {
    SelectorLayoutEntry {
        visible: Some(entry.and_then(|entry| entry.visible).unwrap_or(true)),
        name: Some(
            entry
                .and_then(|entry| entry.name.clone())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| default_name.to_string()),
        ),
        groups: Some(
            entry
                .and_then(|entry| entry.groups.clone())
                .filter(|groups| !groups.is_empty())
                .unwrap_or_else(|| {
                    default_groups
                        .iter()
                        .map(|group| group.to_string())
                        .collect()
                }),
        ),
    }
}

fn default_selector_order() -> Vec<String> {
    ["permissions", "model", "context_control"]
        .into_iter()
        .map(str::to_string)
        .collect()
}
