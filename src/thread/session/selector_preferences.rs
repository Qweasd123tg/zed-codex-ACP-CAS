//! Persistent adapter-side defaults for nested selector state that ACP cannot model directly.

use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::thread::{AppModel, ReasoningEffort, ThreadInner};

#[path = "selector_preferences/jsonc.rs"]
mod jsonc;
#[path = "selector_preferences/materialize.rs"]
mod materialize;

use self::materialize::{
    materialized_model_selector, materialized_selector_layout,
    normalize_reasoning_effort_for_preferences,
};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct SelectorPreferences {
    pub(in crate::thread) defaults: Option<SelectorDefaultPreferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) model_selector: Option<ModelSelectorPreferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) layout: Option<SelectorLayoutPreferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) slash_commands: Option<SlashCommandPreferences>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct SelectorDefaultPreferences {
    #[serde(
        default,
        deserialize_with = "deserialize_double_option",
        serialize_with = "serialize_double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub(in crate::thread) model: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_double_option",
        serialize_with = "serialize_double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub(in crate::thread) reasoning_effort: Option<Option<ReasoningEffort>>,
    #[serde(
        default,
        deserialize_with = "deserialize_double_option",
        serialize_with = "serialize_double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub(in crate::thread) service_tier: Option<Option<String>>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct ModelSelectorPreferences {
    #[serde(skip)]
    pub(in crate::thread) default_model: Option<String>,
    #[serde(skip)]
    pub(in crate::thread) default_reasoning_effort: Option<ReasoningEffort>,
    #[serde(skip)]
    pub(in crate::thread) default_service_tier: Option<String>,
    #[serde(default)]
    pub(in crate::thread) models: Vec<ModelSelectorModelEntry>,
    #[serde(default)]
    pub(in crate::thread) reasoning_efforts: Vec<ModelSelectorReasoningEffortEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(in crate::thread) enum ModelSelectorModelEntry {
    Id(String),
    Details(ModelSelectorModelDetails),
}

impl Default for ModelSelectorModelEntry {
    fn default() -> Self {
        Self::Details(ModelSelectorModelDetails::default())
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct ModelSelectorModelDetails {
    pub(in crate::thread) id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) description: Option<String>,
}

impl ModelSelectorModelEntry {
    fn id(&self) -> &str {
        match self {
            Self::Id(id) => id,
            Self::Details(details) => &details.id,
        }
    }

    fn name_override(&self) -> Option<&str> {
        match self {
            Self::Id(_) => None,
            Self::Details(details) => non_empty_str(details.name.as_deref()),
        }
    }

    fn description_override(&self) -> Option<&str> {
        match self {
            Self::Id(_) => None,
            Self::Details(details) => non_empty_str(details.description.as_deref()),
        }
    }

    fn materialized(model_id: &str, existing: Option<&Self>) -> Self {
        let name = existing
            .and_then(Self::name_override)
            .map(ToString::to_string);
        let description = existing
            .and_then(Self::description_override)
            .map(ToString::to_string);
        if name.is_none() && description.is_none() {
            Self::Id(model_id.to_string())
        } else {
            Self::Details(ModelSelectorModelDetails {
                id: model_id.to_string(),
                name,
                description,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(in crate::thread) enum ModelSelectorReasoningEffortEntry {
    Id(ReasoningEffort),
    Details(ModelSelectorReasoningEffortDetails),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct ModelSelectorReasoningEffortDetails {
    pub(in crate::thread) id: ReasoningEffort,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) description: Option<String>,
}

impl ModelSelectorReasoningEffortEntry {
    fn id(&self) -> &ReasoningEffort {
        match self {
            Self::Id(effort) => effort,
            Self::Details(details) => &details.id,
        }
    }

    fn name_override(&self) -> Option<&str> {
        match self {
            Self::Id(_) => None,
            Self::Details(details) => non_empty_str(details.name.as_deref()),
        }
    }

    fn description_override(&self) -> Option<&str> {
        match self {
            Self::Id(_) => None,
            Self::Details(details) => non_empty_str(details.description.as_deref()),
        }
    }

    fn materialized(effort: &ReasoningEffort, existing: Option<&Self>) -> Self {
        let name = existing
            .and_then(Self::name_override)
            .map(ToString::to_string);
        let description = existing
            .and_then(Self::description_override)
            .map(ToString::to_string);
        if name.is_none() && description.is_none() {
            Self::Id(effort.clone())
        } else {
            Self::Details(ModelSelectorReasoningEffortDetails {
                id: effort.clone(),
                name,
                description,
            })
        }
    }
}

fn non_empty_str(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

impl ModelSelectorPreferences {
    pub(in crate::thread) fn ordered_models<'a>(
        &self,
        models: &'a [AppModel],
    ) -> Vec<&'a AppModel> {
        if self.models.is_empty() {
            return models.iter().filter(|model| !model.hidden).collect();
        }

        let mut ordered = Vec::new();
        for entry in &self.models {
            let model_id = entry.id();
            if let Some(model) = models
                .iter()
                .find(|model| id_matches(&model.id, model_id) || id_matches(&model.model, model_id))
                && !ordered.iter().any(|selected: &&AppModel| {
                    id_matches(&selected.id, &model.id) || id_matches(&selected.model, &model.model)
                })
            {
                ordered.push(model);
            }
        }
        ordered
    }

    pub(in crate::thread) fn hides_model(&self, model_id: &str, current_model_id: &str) -> bool {
        if model_id == current_model_id {
            return false;
        }
        !self.models.is_empty() && self.model_entry(model_id).is_none()
    }

    pub(in crate::thread) fn hides_reasoning_effort(
        &self,
        effort: &ReasoningEffort,
        current_effort: &ReasoningEffort,
    ) -> bool {
        if effort == current_effort {
            return false;
        }
        !self.reasoning_efforts.is_empty() && self.reasoning_effort_entry(effort).is_none()
    }

    pub(in crate::thread) fn explicitly_enables_reasoning_effort(
        &self,
        effort: &ReasoningEffort,
    ) -> bool {
        self.reasoning_effort_entry(effort).is_some()
    }

    pub(in crate::thread) fn configured_visible_reasoning_efforts(&self) -> Vec<ReasoningEffort> {
        self.reasoning_efforts
            .iter()
            .map(|entry| entry.id().clone())
            .collect()
    }

    pub(in crate::thread) fn reasoning_effort_name_override(
        &self,
        effort: &ReasoningEffort,
    ) -> Option<&str> {
        self.reasoning_effort_entry(effort)
            .and_then(ModelSelectorReasoningEffortEntry::name_override)
    }

    pub(in crate::thread) fn reasoning_effort_description_override(
        &self,
        effort: &ReasoningEffort,
    ) -> Option<&str> {
        self.reasoning_effort_entry(effort)
            .and_then(ModelSelectorReasoningEffortEntry::description_override)
    }

    pub(in crate::thread) fn model_name_override(&self, model_id: &str) -> Option<&str> {
        self.model_entry(model_id)
            .and_then(ModelSelectorModelEntry::name_override)
    }

    pub(in crate::thread) fn model_description_override(&self, model_id: &str) -> Option<&str> {
        self.model_entry(model_id)
            .and_then(ModelSelectorModelEntry::description_override)
    }

    fn model_entry(&self, model_id: &str) -> Option<&ModelSelectorModelEntry> {
        self.models
            .iter()
            .find(|entry| id_matches(entry.id(), model_id))
    }

    fn reasoning_effort_entry(
        &self,
        effort: &ReasoningEffort,
    ) -> Option<&ModelSelectorReasoningEffortEntry> {
        self.reasoning_efforts
            .iter()
            .find(|entry| entry.id() == effort)
    }
}

fn id_matches(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct SelectorLayoutPreferences {
    pub(in crate::thread) order: Option<Vec<String>>,
    pub(in crate::thread) permissions: Option<SelectorLayoutEntry>,
    pub(in crate::thread) model: Option<SelectorLayoutEntry>,
    pub(in crate::thread) status: Option<SelectorLayoutEntry>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct SelectorLayoutEntry {
    pub(in crate::thread) visible: Option<bool>,
    pub(in crate::thread) name: Option<String>,
    pub(in crate::thread) groups: Option<Vec<String>>,
}

impl SelectorLayoutPreferences {
    pub(in crate::thread) fn entry(&self, selector_id: &str) -> Option<&SelectorLayoutEntry> {
        match selector_id {
            "permissions" => self.permissions.as_ref(),
            "model" => self.model.as_ref(),
            "status" => self.status.as_ref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::thread) struct SlashCommandPreferences {
    commands: Vec<String>,
}

impl Default for SlashCommandPreferences {
    fn default() -> Self {
        Self::from_commands(
            default_slash_commands()
                .into_iter()
                .map(str::to_string)
                .collect(),
        )
    }
}

impl SlashCommandPreferences {
    pub(in crate::thread) fn from_commands(commands: Vec<String>) -> Self {
        Self { commands }
    }

    pub(in crate::thread) fn is_enabled(&self, command: &str) -> bool {
        self.commands
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(command))
    }

    pub(in crate::thread) fn command_order(&self, command: &str) -> Option<usize> {
        self.commands
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(command))
    }
}

fn default_slash_commands() -> [&'static str; 11] {
    [
        "init",
        "status",
        "review",
        "fork",
        "archive",
        "unarchive",
        "compact",
        "undo",
        "plan",
        "rename",
        "diff",
    ]
}

pub(in crate::thread) fn selector_preferences_path(cas_home: &Path) -> PathBuf {
    cas_home.join("selector-preferences.json")
}

pub(in crate::thread) fn restore_selector_preferences(
    preferences_path: &Path,
) -> std::io::Result<SelectorPreferences> {
    read_selector_preferences(preferences_path)
}

pub(in crate::thread) fn apply_selector_preferences(
    inner: &mut ThreadInner,
    preferences: SelectorPreferences,
) {
    if let Some(value) = preferences.model_selector {
        let default_model = inner.model_selector.default_model.clone();
        let default_reasoning_effort = inner.model_selector.default_reasoning_effort.clone();
        let default_service_tier = inner.model_selector.default_service_tier.clone();
        inner.model_selector = value;
        inner.model_selector.default_model = default_model;
        inner.model_selector.default_reasoning_effort = default_reasoning_effort;
        inner.model_selector.default_service_tier = default_service_tier;
    }
    if let Some(defaults) = preferences.defaults {
        if let Some(model) = defaults.model {
            inner.model_selector.default_model = model.clone();
            if let Some(model) = model
                && inner
                    .models
                    .iter()
                    .any(|candidate| candidate.id == model || candidate.model == model)
            {
                inner.current_model = model;
            }
        }
        if let Some(effort) = defaults.reasoning_effort {
            inner.model_selector.default_reasoning_effort = effort.clone();
            if let Some(effort) = effort {
                inner.reasoning_effort = normalize_reasoning_effort_for_preferences(
                    &inner.models,
                    &inner.current_model,
                    &inner.model_selector,
                    effort,
                );
            }
        }
        if let Some(service_tier) = defaults.service_tier {
            inner.model_selector.default_service_tier = service_tier.clone();
            inner.service_tier = service_tier;
        }
    }
    if let Some(value) = preferences.layout {
        inner.selector_layout = value;
    }
    if let Some(value) = preferences.slash_commands {
        inner.slash_commands = value;
    }
}

pub(in crate::thread) fn persist_selector_preferences(inner: &ThreadInner) -> std::io::Result<()> {
    let existing = read_selector_preferences(&inner.selector_preferences_path)?;
    let model_selector = materialized_model_selector(
        existing
            .model_selector
            .as_ref()
            .unwrap_or(&inner.model_selector),
        &inner.models,
    );
    let preferences = SelectorPreferences {
        defaults: Some(SelectorDefaultPreferences {
            model: Some(inner.model_selector.default_model.clone()),
            reasoning_effort: Some(inner.model_selector.default_reasoning_effort.clone()),
            service_tier: Some(inner.model_selector.default_service_tier.clone()),
        }),
        model_selector: Some(model_selector),
        layout: Some(materialized_selector_layout(
            existing.layout.as_ref().unwrap_or(&inner.selector_layout),
        )),
        slash_commands: Some(
            existing
                .slash_commands
                .unwrap_or_else(|| inner.slash_commands.clone()),
        ),
    };
    write_selector_preferences(&inner.selector_preferences_path, &preferences)
}

fn read_selector_preferences(preferences_path: &Path) -> std::io::Result<SelectorPreferences> {
    match fs::read_to_string(preferences_path) {
        Ok(contents) => jsonc::parse_selector_preferences_jsonc(&contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(SelectorPreferences::default())
        }
        Err(error) => Err(error),
    }
}

fn write_selector_preferences(
    preferences_path: &Path,
    preferences: &SelectorPreferences,
) -> std::io::Result<()> {
    if let Some(parent) = preferences_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = preferences_path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let payload = jsonc::selector_preferences_jsonc(preferences)?;
    fs::write(&tmp_path, payload)?;
    fs::rename(tmp_path, preferences_path)
}

fn deserialize_double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: DeserializeOwned,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

fn serialize_double_option<S, T>(
    value: &Option<Option<T>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    T: Serialize,
{
    match value {
        Some(value) => value.serialize(serializer),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ModelSelectorModelDetails, ModelSelectorModelEntry, ModelSelectorPreferences,
        ModelSelectorReasoningEffortDetails, ModelSelectorReasoningEffortEntry,
        SelectorDefaultPreferences, SelectorLayoutEntry, SelectorLayoutPreferences,
        SlashCommandPreferences, materialized_model_selector, materialized_selector_layout,
        restore_selector_preferences, selector_preferences_path, write_selector_preferences,
    };
    use crate::thread::{AppModel, ReasoningEffort};
    use codex_app_server_protocol::ReasoningEffortOption;
    use std::path::Path;

    fn test_model(id: &str, hidden: bool, efforts: Vec<ReasoningEffort>) -> AppModel {
        AppModel {
            id: id.to_string(),
            model: id.to_string(),
            upgrade: None,
            upgrade_info: None,
            availability_nux: None,
            display_name: id.to_string(),
            description: format!("{id} description"),
            hidden,
            supported_reasoning_efforts: efforts
                .into_iter()
                .map(|reasoning_effort| ReasoningEffortOption {
                    reasoning_effort,
                    description: "test effort".to_string(),
                })
                .collect(),
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
    fn preferences_path_uses_cas_home() {
        let path = selector_preferences_path(Path::new("/tmp/.codex-cas"));
        assert_eq!(
            path,
            Path::new("/tmp/.codex-cas").join("selector-preferences.json")
        );
    }

    #[test]
    fn writes_and_restores_preferences() {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "codex-acp-selector-preferences-{}.json",
            uuid::Uuid::new_v4()
        ));
        let preferences = super::SelectorPreferences {
            defaults: Some(SelectorDefaultPreferences {
                model: Some(Some("gpt-5.5".to_string())),
                reasoning_effort: Some(Some(ReasoningEffort::High)),
                service_tier: Some(Some("fast".to_string())),
            }),
            model_selector: Some(ModelSelectorPreferences {
                models: vec![ModelSelectorModelEntry::Details(
                    ModelSelectorModelDetails {
                        id: "gpt-5.2".to_string(),
                        name: Some("5.2".to_string()),
                        description: Some("manual description".to_string()),
                    },
                )],
                reasoning_efforts: vec![ModelSelectorReasoningEffortEntry::Details(
                    ModelSelectorReasoningEffortDetails {
                        id: ReasoningEffort::XHigh,
                        name: Some("максимум".to_string()),
                        description: Some("manual effort description".to_string()),
                    },
                )],
                ..Default::default()
            }),
            layout: Some(SelectorLayoutPreferences {
                order: Some(vec!["model".to_string(), "permissions".to_string()]),
                permissions: Some(SelectorLayoutEntry {
                    visible: Some(true),
                    name: Some("Mode".to_string()),
                    groups: Some(vec!["workflow".to_string(), "guarded".to_string()]),
                }),
                model: None,
                status: None,
            }),
            slash_commands: Some(SlashCommandPreferences::from_commands(vec![
                "status".to_string(),
                "unarchive".to_string(),
            ])),
        };

        write_selector_preferences(&path, &preferences).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("\"defaults\""));
        let restored = restore_selector_preferences(&path).unwrap();

        let defaults = restored.defaults.expect("defaults should restore");
        assert_eq!(defaults.model, Some(Some("gpt-5.5".to_string())));
        assert_eq!(defaults.reasoning_effort, Some(Some(ReasoningEffort::High)));
        assert_eq!(defaults.service_tier, Some(Some("fast".to_string())));
        let model_selector = restored
            .model_selector
            .expect("model selector preferences should restore");
        let model_entry = model_selector
            .model_entry("gpt-5.2")
            .expect("model entry should restore");
        assert_eq!(model_entry.name_override(), Some("5.2"));
        assert_eq!(
            model_entry.description_override(),
            Some("manual description")
        );
        assert_eq!(
            model_selector.reasoning_effort_name_override(&ReasoningEffort::XHigh),
            Some("максимум")
        );
        assert_eq!(
            model_selector.reasoning_effort_description_override(&ReasoningEffort::XHigh),
            Some("manual effort description")
        );
        let layout = restored.layout.expect("layout should restore");
        assert_eq!(
            layout.order,
            Some(vec!["model".to_string(), "permissions".to_string()])
        );
        assert_eq!(
            layout.permissions.and_then(|entry| entry.name),
            Some("Mode".to_string())
        );
        let slash_commands = restored
            .slash_commands
            .expect("slash commands should restore");
        assert!(slash_commands.is_enabled("status"));
        assert!(!slash_commands.is_enabled("review"));
        assert!(!slash_commands.is_enabled("archive"));
        drop(std::fs::remove_file(path));
    }

    #[test]
    fn open_reasoning_efforts_round_trip_without_normalization() {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "codex-acp-selector-open-reasoning-{}.json",
            uuid::Uuid::new_v4()
        ));
        let future_effort = ReasoningEffort::Custom("future-effort".to_string());
        let preferences = super::SelectorPreferences {
            defaults: Some(SelectorDefaultPreferences {
                model: None,
                reasoning_effort: Some(Some(future_effort.clone())),
                service_tier: None,
            }),
            model_selector: Some(ModelSelectorPreferences {
                reasoning_efforts: vec![
                    ModelSelectorReasoningEffortEntry::Id(ReasoningEffort::Max),
                    ModelSelectorReasoningEffortEntry::Id(ReasoningEffort::Ultra),
                    ModelSelectorReasoningEffortEntry::Id(future_effort.clone()),
                ],
                ..Default::default()
            }),
            ..Default::default()
        };

        write_selector_preferences(&path, &preferences).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains(r#""max""#));
        assert!(written.contains(r#""ultra""#));
        assert!(written.contains(r#""future-effort""#));

        let restored = restore_selector_preferences(&path).unwrap();
        assert_eq!(
            restored
                .defaults
                .expect("defaults should restore")
                .reasoning_effort,
            Some(Some(future_effort.clone()))
        );
        let model_selector = restored
            .model_selector
            .expect("model selector should restore");
        assert_eq!(
            model_selector.configured_visible_reasoning_efforts(),
            vec![ReasoningEffort::Max, ReasoningEffort::Ultra, future_effort]
        );

        drop(std::fs::remove_file(path));
    }

    #[test]
    fn restores_jsonc_sections_and_model_overrides() {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "codex-acp-selector-preferences-jsonc-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &path,
            r#"{
              // display settings from older configs are ignored
              "display": {
                "context_control": "context_and_limits"
              },
              "defaults": {
                "model": "gpt-5.5",
                "reasoning_effort": "none",
                "service_tier": null
              },
              "model_selector": {
                "models": [
                  {
                    "id": "gpt-5.5",
                    "name": "главная",
                    "description": "кастомное описание"
                  },
                  "gpt-5.2",
                ],
                "reasoning_efforts": [
                  "none",
                  {
                    "id": "minimal",
                    "name": "минимум",
                    "description": "кастомное описание effort"
                  },
                ]
              }
            }"#,
        )
        .unwrap();

        let restored = restore_selector_preferences(&path).unwrap();
        let defaults = restored.defaults.expect("defaults should restore");
        assert_eq!(defaults.model, Some(Some("gpt-5.5".to_string())));
        assert_eq!(defaults.reasoning_effort, Some(Some(ReasoningEffort::None)));
        assert_eq!(defaults.service_tier, Some(None));
        let model_selector = restored
            .model_selector
            .expect("model selector should restore");
        assert_eq!(model_selector.default_model, None);
        assert_eq!(model_selector.default_reasoning_effort, None);
        assert_eq!(model_selector.default_service_tier, None);
        assert_eq!(
            model_selector.model_name_override("gpt-5.5"),
            Some("главная")
        );
        assert_eq!(
            model_selector.model_description_override("gpt-5.5"),
            Some("кастомное описание")
        );
        assert!(model_selector.model_entry("gpt-5.2").is_some());
        assert!(model_selector.explicitly_enables_reasoning_effort(&ReasoningEffort::None));
        assert!(model_selector.explicitly_enables_reasoning_effort(&ReasoningEffort::Minimal));
        assert_eq!(
            model_selector.reasoning_effort_name_override(&ReasoningEffort::Minimal),
            Some("минимум")
        );
        assert_eq!(
            model_selector.reasoning_effort_description_override(&ReasoningEffort::Minimal),
            Some("кастомное описание effort")
        );

        drop(std::fs::remove_file(path));
    }

    #[test]
    fn materialized_model_selector_preserves_manual_model_labels() {
        let preferences = ModelSelectorPreferences {
            models: vec![ModelSelectorModelEntry::Details(
                ModelSelectorModelDetails {
                    id: "gpt-5.5".to_string(),
                    name: Some("главная".to_string()),
                    description: Some("кастомное описание".to_string()),
                },
            )],
            ..Default::default()
        };
        let models = vec![AppModel {
            id: "gpt-5.5".to_string(),
            model: "gpt-5.5".to_string(),
            upgrade: None,
            upgrade_info: None,
            availability_nux: None,
            display_name: "GPT-5.5".to_string(),
            description: "Frontier model".to_string(),
            hidden: false,
            supported_reasoning_efforts: Vec::new(),
            default_reasoning_effort: ReasoningEffort::High,
            input_modalities: Vec::new(),
            supports_personality: false,
            additional_speed_tiers: Vec::new(),
            service_tiers: Vec::new(),
            default_service_tier: None,
            is_default: true,
        }];

        let materialized = materialized_model_selector(&preferences, &models);

        assert_eq!(materialized.model_name_override("gpt-5.5"), Some("главная"));
        assert_eq!(
            materialized.model_description_override("gpt-5.5"),
            Some("кастомное описание")
        );
    }

    #[test]
    fn default_model_selector_excludes_hidden_models_and_their_efforts() {
        let preferences = ModelSelectorPreferences::default();
        let models = vec![
            test_model("gpt-visible", false, vec![ReasoningEffort::Medium]),
            test_model("gpt-hidden", true, vec![ReasoningEffort::Ultra]),
        ];

        assert_eq!(
            preferences
                .ordered_models(&models)
                .into_iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-visible"]
        );

        let materialized = materialized_model_selector(&preferences, &models);
        assert_eq!(
            materialized
                .models
                .iter()
                .map(ModelSelectorModelEntry::id)
                .collect::<Vec<_>>(),
            vec!["gpt-visible"]
        );
        assert_eq!(
            materialized.configured_visible_reasoning_efforts(),
            vec![ReasoningEffort::Medium]
        );
    }

    #[test]
    fn explicit_model_selector_includes_and_materializes_hidden_models() {
        let preferences = ModelSelectorPreferences {
            models: vec![ModelSelectorModelEntry::Details(
                ModelSelectorModelDetails {
                    id: "gpt-hidden".to_string(),
                    name: Some("Hidden opt-in".to_string()),
                    description: Some("Explicit hidden model".to_string()),
                },
            )],
            ..Default::default()
        };
        let models = vec![
            test_model("gpt-visible", false, vec![ReasoningEffort::Medium]),
            test_model("gpt-hidden", true, vec![ReasoningEffort::Ultra]),
        ];

        assert_eq!(
            preferences
                .ordered_models(&models)
                .into_iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-hidden"]
        );

        let materialized = materialized_model_selector(&preferences, &models);
        assert_eq!(
            materialized.model_name_override("gpt-hidden"),
            Some("Hidden opt-in")
        );
        assert_eq!(
            materialized.model_description_override("gpt-hidden"),
            Some("Explicit hidden model")
        );
        assert_eq!(
            materialized.configured_visible_reasoning_efforts(),
            vec![ReasoningEffort::Ultra]
        );
    }

    #[test]
    fn invalid_existing_preferences_return_error_without_rewrite() {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "codex-acp-selector-preferences-invalid-{}.json",
            uuid::Uuid::new_v4()
        ));
        let original = r#"{
          "model_selector": {
            "models": [
              { "id": "gpt-5.5", "name": "главная модель" }
            ]
          }"#;
        std::fs::write(&path, original).unwrap();

        let error = restore_selector_preferences(&path).expect_err("invalid JSONC should fail");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, original);

        drop(std::fs::remove_file(path));
    }

    #[test]
    fn materialized_layout_fills_defaults_for_manual_config_template() {
        let layout = materialized_selector_layout(&SelectorLayoutPreferences {
            order: None,
            permissions: Some(SelectorLayoutEntry {
                visible: Some(false),
                name: Some("Mode".to_string()),
                groups: None,
            }),
            model: None,
            status: None,
        });

        assert_eq!(
            layout.order,
            Some(vec![
                "permissions".to_string(),
                "model".to_string(),
                "status".to_string()
            ])
        );
        assert_eq!(
            layout.permissions,
            Some(SelectorLayoutEntry {
                visible: Some(false),
                name: Some("Mode".to_string()),
                groups: Some(vec![
                    "workflow".to_string(),
                    "guarded".to_string(),
                    "bypass".to_string()
                ])
            })
        );
        assert_eq!(
            layout.model.and_then(|entry| entry.groups),
            Some(vec![
                "models".to_string(),
                "effort".to_string(),
                "speed".to_string()
            ])
        );
        assert_eq!(
            layout.status.and_then(|entry| entry.groups),
            Some(vec!["status".to_string(), "actions".to_string()])
        );
    }
}
