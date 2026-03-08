//! Reasoning and model helpers for `session_config`.

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
    let effort = current.unwrap_or_default();
    normalize_reasoning_effort_for_model(models, current_model, effort)
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
    model.default_reasoning_effort
}

pub(in crate::thread) fn parse_reasoning_effort(value: &str) -> Option<ReasoningEffort> {
    match value {
        "none" => Some(ReasoningEffort::None),
        "minimal" => Some(ReasoningEffort::Minimal),
        "low" => Some(ReasoningEffort::Low),
        "medium" => Some(ReasoningEffort::Medium),
        "high" => Some(ReasoningEffort::High),
        "xhigh" => Some(ReasoningEffort::XHigh),
        _ => None,
    }
}

pub(in crate::thread) fn reasoning_effort_value(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::None => "none",
        ReasoningEffort::Minimal => "minimal",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::XHigh => "xhigh",
    }
}

fn reasoning_effort_label(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::None => "None",
        ReasoningEffort::Minimal => "Minimal",
        ReasoningEffort::Low => "Low",
        ReasoningEffort::Medium => "Medium",
        ReasoningEffort::High => "High",
        ReasoningEffort::XHigh => "Extra High",
    }
}

pub(in crate::thread) fn reasoning_effort_option_label(
    effort: ReasoningEffort,
    current_effort: ReasoningEffort,
    current_usage_percent: Option<u64>,
) -> String {
    if effort == current_effort
        && let Some(percent) = current_usage_percent
    {
        return format!("{} · {}% ctx", reasoning_effort_label(effort), percent);
    }
    reasoning_effort_label(effort).to_string()
}
