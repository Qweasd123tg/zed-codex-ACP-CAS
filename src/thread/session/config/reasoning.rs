//! Reasoning/model helper-ы для session_config.

use crate::thread::{AppModel, ReasoningEffort, ReasoningEffortDisplayStyle};

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

pub(in crate::thread) fn parse_reasoning_effort_display_style(
    value: &str,
) -> Option<ReasoningEffortDisplayStyle> {
    match value {
        "circle" => Some(ReasoningEffortDisplayStyle::Circle),
        "text" => Some(ReasoningEffortDisplayStyle::Text),
        _ => None,
    }
}

pub(in crate::thread) fn reasoning_effort_display_style_value(
    style: ReasoningEffortDisplayStyle,
) -> &'static str {
    match style {
        ReasoningEffortDisplayStyle::Circle => "circle",
        ReasoningEffortDisplayStyle::Text => "text",
    }
}

fn reasoning_effort_display_style_label(style: ReasoningEffortDisplayStyle) -> &'static str {
    match style {
        ReasoningEffortDisplayStyle::Circle => "Circle",
        ReasoningEffortDisplayStyle::Text => "Text",
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

pub(in crate::thread) fn reasoning_effort_icon(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::None => "○",
        ReasoningEffort::Minimal | ReasoningEffort::Low => "◔",
        ReasoningEffort::Medium => "◑",
        ReasoningEffort::High => "◕",
        ReasoningEffort::XHigh => "●",
    }
}

pub(in crate::thread) fn reasoning_effort_option_label(
    effort: ReasoningEffort,
    style: ReasoningEffortDisplayStyle,
) -> String {
    match style {
        ReasoningEffortDisplayStyle::Circle => reasoning_effort_icon(effort).to_string(),
        ReasoningEffortDisplayStyle::Text => reasoning_effort_label(effort).to_string(),
    }
}

pub(in crate::thread) fn reasoning_effort_description_label(effort: ReasoningEffort) -> String {
    reasoning_effort_label(effort).to_string()
}

pub(in crate::thread) fn reasoning_effort_style_option_label(
    style: ReasoningEffortDisplayStyle,
) -> String {
    reasoning_effort_display_style_label(style).to_string()
}
