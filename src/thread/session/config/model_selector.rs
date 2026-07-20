use crate::thread::session_selector_preferences::ModelSelectorPreferences;
use crate::thread::{
    AppModel, ReasoningEffort, SessionConfigSelectGroup, SessionConfigSelectOption,
};

use super::{fast_mode, reasoning};

const MODEL_REASONING_VALUE_PREFIX: &str = "reasoning:";
const MODEL_SPEED_VALUE_PREFIX: &str = "speed:";

pub(in crate::thread) fn parse_model_reasoning_value(value: &str) -> Option<ReasoningEffort> {
    value
        .strip_prefix(MODEL_REASONING_VALUE_PREFIX)
        .and_then(reasoning::parse_reasoning_effort)
}

pub(in crate::thread) fn parse_model_speed_value(value: &str) -> Option<Option<String>> {
    value
        .strip_prefix(MODEL_SPEED_VALUE_PREFIX)
        .and_then(fast_mode::parse_fast_mode_value)
}

pub(in crate::thread) fn model_reasoning_value(effort: &ReasoningEffort) -> String {
    format!(
        "{MODEL_REASONING_VALUE_PREFIX}{}",
        reasoning::reasoning_effort_value(effort)
    )
}

pub(in crate::thread) fn model_speed_value(value: &str) -> String {
    format!("{MODEL_SPEED_VALUE_PREFIX}{value}")
}

fn model_label_for_selector(
    current_model_entry: Option<&AppModel>,
    current_model_id: &str,
    model_selector: &ModelSelectorPreferences,
) -> String {
    model_selector
        .model_name_override(current_model_id)
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            current_model_entry
                .map(|model| model.display_name.as_str())
                .unwrap_or(current_model_id)
                .trim()
                .to_string()
        })
}

fn current_model_label_for_selector(
    current_model_entry: Option<&AppModel>,
    current_model_id: &str,
    current_reasoning_effort: &ReasoningEffort,
    model_selector: &ModelSelectorPreferences,
) -> String {
    [
        model_label_for_selector(current_model_entry, current_model_id, model_selector),
        reasoning_effort_label_for_selector(current_reasoning_effort, model_selector),
    ]
    .join(" ")
}

fn reasoning_effort_label_for_selector(
    effort: &ReasoningEffort,
    model_selector: &ModelSelectorPreferences,
) -> String {
    model_selector
        .reasoning_effort_name_override(effort)
        .map(str::to_string)
        .unwrap_or_else(|| reasoning::reasoning_effort_label(effort))
}

pub(super) struct ModelOptionGroupsInput<'a> {
    pub(super) models: &'a [AppModel],
    pub(super) current_model_entry: Option<&'a AppModel>,
    pub(super) current_model_id: &'a str,
    pub(super) current_reasoning_effort: ReasoningEffort,
    pub(super) model_selector: &'a ModelSelectorPreferences,
    pub(super) current_service_tier: Option<String>,
}

pub(super) fn model_option_groups(
    input: ModelOptionGroupsInput<'_>,
) -> Vec<SessionConfigSelectGroup> {
    let ModelOptionGroupsInput {
        models,
        current_model_entry,
        current_model_id,
        current_reasoning_effort,
        model_selector,
        current_service_tier,
    } = input;
    let mut model_options = Vec::with_capacity(models.len() + 1);
    let mut has_current_model = false;
    let current_speed_value = fast_mode::fast_mode_value(current_service_tier.as_deref());
    let current_speed_label = fast_mode_label(current_service_tier.as_deref());
    let current_model_label = current_model_label_for_selector(
        current_model_entry,
        current_model_id,
        &current_reasoning_effort,
        model_selector,
    );
    for model in model_selector.ordered_models(models) {
        if model_selector.hides_model(&model.id, current_model_id) {
            continue;
        }
        if model.id == current_model_id {
            has_current_model = true;
        }
        let is_current_model = model.id == current_model_id;
        let model_name = if is_current_model {
            current_model_label.clone()
        } else {
            model_label_for_selector(Some(model), &model.id, model_selector)
        };
        let description = if is_current_model {
            let model_description = model_selector
                .model_description_override(&model.id)
                .unwrap_or(&model.description);
            format!(
                "{}\nSelected: reasoning effort {}, speed {}.",
                model_description,
                reasoning::reasoning_effort_description_label(&current_reasoning_effort),
                current_speed_label
            )
        } else {
            model_selector
                .model_description_override(&model.id)
                .unwrap_or(&model.description)
                .to_string()
        };
        model_options.push(
            SessionConfigSelectOption::new(model.id.clone(), model_name).description(description),
        );
    }

    if !has_current_model {
        model_options.push(SessionConfigSelectOption::new(
            current_model_id.to_string(),
            current_model_label,
        ));
    }

    let reasoning_options = reasoning_effort_option_groups(
        current_model_entry,
        &current_reasoning_effort,
        model_selector,
    );
    let speed_options = fast_mode::fast_mode_options(current_service_tier.as_deref())
        .into_iter()
        .map(|option| {
            let value = option.value.0.to_string();
            let name = if value == current_speed_value {
                format!("★ {}", option.name)
            } else {
                option.name
            };
            SessionConfigSelectOption::new(model_speed_value(&value), name)
                .description(option.description)
        })
        .collect();

    vec![
        SessionConfigSelectGroup::new("models", "Models", model_options),
        SessionConfigSelectGroup::new("effort", "Effort", reasoning_options),
        SessionConfigSelectGroup::new("speed", "Speed", speed_options),
    ]
}

fn reasoning_effort_option_groups(
    current_model_entry: Option<&AppModel>,
    current_reasoning_effort: &ReasoningEffort,
    model_selector: &ModelSelectorPreferences,
) -> Vec<SessionConfigSelectOption> {
    let current_effort_value = reasoning::reasoning_effort_value(current_reasoning_effort);
    let mut effort_options = Vec::new();
    let mut has_current_effort = false;
    let mut advertised_efforts = Vec::new();
    if let Some(model) = current_model_entry {
        effort_options.reserve(model.supported_reasoning_efforts.len() + 1);
        for option in &model.supported_reasoning_efforts {
            advertised_efforts.push(option.reasoning_effort.clone());
            if model_selector
                .hides_reasoning_effort(&option.reasoning_effort, current_reasoning_effort)
            {
                continue;
            }
            let effort_value = reasoning::reasoning_effort_value(&option.reasoning_effort);
            has_current_effort |= effort_value == current_effort_value;
            let label = model_selector
                .reasoning_effort_name_override(&option.reasoning_effort)
                .map(str::to_string)
                .unwrap_or_else(|| reasoning::reasoning_effort_label(&option.reasoning_effort));
            let name = if effort_value == current_effort_value {
                format!("★ {label}")
            } else {
                label
            };
            let description = model_selector
                .reasoning_effort_description_override(&option.reasoning_effort)
                .unwrap_or(&option.description)
                .to_string();
            effort_options.push(
                SessionConfigSelectOption::new(
                    model_reasoning_value(&option.reasoning_effort),
                    name,
                )
                .description(description),
            );
        }
        for effort in model_selector.configured_visible_reasoning_efforts() {
            if advertised_efforts.contains(&effort)
                || model_selector.hides_reasoning_effort(&effort, current_reasoning_effort)
            {
                continue;
            }
            let effort_value = reasoning::reasoning_effort_value(&effort);
            has_current_effort |= effort_value == current_effort_value;
            let label = model_selector
                .reasoning_effort_name_override(&effort)
                .map(str::to_string)
                .unwrap_or_else(|| reasoning::reasoning_effort_label(&effort));
            let name = if effort_value == current_effort_value {
                format!("★ {label}")
            } else {
                label
            };
            let description = model_selector
                .reasoning_effort_description_override(&effort)
                .map(str::to_string)
                .unwrap_or_else(|| configured_reasoning_effort_description(&effort).to_string());
            effort_options.push(
                SessionConfigSelectOption::new(model_reasoning_value(&effort), name)
                    .description(description),
            );
        }
    } else {
        effort_options.reserve(1);
    }

    if effort_options.is_empty() || !has_current_effort {
        effort_options.push(SessionConfigSelectOption::new(
            model_reasoning_value(current_reasoning_effort),
            format!(
                "★ {}",
                reasoning_effort_label_for_selector(current_reasoning_effort, model_selector)
            ),
        ));
    }

    effort_options
}

fn configured_reasoning_effort_description(_effort: &ReasoningEffort) -> &'static str {
    "Configured manually in selector-preferences.json. The backend or current model may reject this reasoning effort if it is unsupported."
}

fn fast_mode_label(service_tier: Option<&str>) -> String {
    match service_tier {
        Some("fast" | "priority") => "Fast".to_string(),
        Some("flex") => "Flex".to_string(),
        Some(service_tier) => service_tier.to_string(),
        None => "Standard".to_string(),
    }
}

pub(super) fn model_selector_description(
    current_model_entry: Option<&AppModel>,
    current_model_id: &str,
    current_reasoning_effort: &ReasoningEffort,
    current_service_tier: Option<&str>,
    model_selector: &ModelSelectorPreferences,
) -> String {
    let current_effort_label =
        reasoning_effort_label_for_selector(current_reasoning_effort, model_selector);
    let current_model_name =
        model_label_for_selector(current_model_entry, current_model_id, model_selector);
    let current_speed_label = fast_mode_label(current_service_tier);
    format!(
        "{current_model_name}\nReasoning effort: {current_effort_label}\nSpeed: {current_speed_label}"
    )
}

#[cfg(test)]
mod tests {
    use super::{ModelOptionGroupsInput, model_option_groups};
    use crate::thread::session_selector_preferences::ModelSelectorPreferences;
    use crate::thread::{AppModel, ReasoningEffort};

    fn test_model(id: &str, hidden: bool) -> AppModel {
        AppModel {
            id: id.to_string(),
            model: id.to_string(),
            upgrade: None,
            upgrade_info: None,
            availability_nux: None,
            display_name: id.to_string(),
            description: format!("{id} description"),
            hidden,
            supported_reasoning_efforts: Vec::new(),
            default_reasoning_effort: ReasoningEffort::Medium,
            input_modalities: Vec::new(),
            supports_personality: false,
            additional_speed_tiers: Vec::new(),
            service_tiers: Vec::new(),
            default_service_tier: None,
            is_default: !hidden,
        }
    }

    #[test]
    fn hidden_current_model_remains_a_matching_selector_option() {
        let models = vec![
            test_model("gpt-visible", false),
            test_model("gpt-hidden", true),
        ];
        let groups = model_option_groups(ModelOptionGroupsInput {
            models: &models,
            current_model_entry: Some(&models[1]),
            current_model_id: "gpt-hidden",
            current_reasoning_effort: ReasoningEffort::Medium,
            model_selector: &ModelSelectorPreferences::default(),
            current_service_tier: None,
        });

        assert_eq!(
            groups[0]
                .options
                .iter()
                .map(|option| option.value.0.as_ref())
                .collect::<Vec<_>>(),
            vec!["gpt-visible", "gpt-hidden"]
        );
    }
}
