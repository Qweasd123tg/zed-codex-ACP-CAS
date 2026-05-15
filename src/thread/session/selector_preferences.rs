//! Persistent adapter-side defaults for nested selector state that ACP cannot model directly.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::thread::{
    AppModel, ContextControlDisplay, ContextDisplayStyle, LimitsDisplayStyle, ReasoningEffort,
    ServiceTier, ThreadInner, session_config::normalize_reasoning_effort_for_model,
};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct SelectorPreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) display: Option<SelectorDisplayPreferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) defaults: Option<SelectorDefaultPreferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) model_selector: Option<ModelSelectorPreferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) layout: Option<SelectorLayoutPreferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) slash_commands: Option<SlashCommandPreferences>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct SelectorDisplayPreferences {
    pub(in crate::thread) context_control: Option<ContextControlDisplay>,
    pub(in crate::thread) context: Option<ContextDisplayStyle>,
    pub(in crate::thread) limits: Option<LimitsDisplayStyle>,
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
    pub(in crate::thread) service_tier: Option<Option<ServiceTier>>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct ModelSelectorPreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) default_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) default_reasoning_effort: Option<ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::thread) default_service_tier: Option<ServiceTier>,
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
    fn id(&self) -> ReasoningEffort {
        match self {
            Self::Id(effort) => *effort,
            Self::Details(details) => details.id,
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

    fn materialized(effort: ReasoningEffort, existing: Option<&Self>) -> Self {
        let name = existing
            .and_then(Self::name_override)
            .map(ToString::to_string);
        let description = existing
            .and_then(Self::description_override)
            .map(ToString::to_string);
        if name.is_none() && description.is_none() {
            Self::Id(effort)
        } else {
            Self::Details(ModelSelectorReasoningEffortDetails {
                id: effort,
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
            return models.iter().collect();
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
        effort: ReasoningEffort,
        current_effort: ReasoningEffort,
    ) -> bool {
        if effort == current_effort {
            return false;
        }
        !self.reasoning_efforts.is_empty() && self.reasoning_effort_entry(effort).is_none()
    }

    pub(in crate::thread) fn explicitly_enables_reasoning_effort(
        &self,
        effort: ReasoningEffort,
    ) -> bool {
        self.reasoning_effort_entry(effort).is_some()
    }

    pub(in crate::thread) fn configured_visible_reasoning_efforts(&self) -> Vec<ReasoningEffort> {
        self.reasoning_efforts
            .iter()
            .map(ModelSelectorReasoningEffortEntry::id)
            .collect()
    }

    pub(in crate::thread) fn reasoning_effort_name_override(
        &self,
        effort: ReasoningEffort,
    ) -> Option<&str> {
        self.reasoning_effort_entry(effort)
            .and_then(ModelSelectorReasoningEffortEntry::name_override)
    }

    pub(in crate::thread) fn reasoning_effort_description_override(
        &self,
        effort: ReasoningEffort,
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
        effort: ReasoningEffort,
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
    pub(in crate::thread) context_control: Option<SelectorLayoutEntry>,
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
            "context_control" => self.context_control.as_ref(),
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
        let canonical = if command == "delete" {
            "archive"
        } else {
            command
        };
        self.commands
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(canonical))
    }

    pub(in crate::thread) fn command_order(&self, command: &str) -> Option<usize> {
        let canonical = if command == "delete" {
            "archive"
        } else {
            command
        };
        self.commands
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(canonical))
    }
}

fn default_slash_commands() -> [&'static str; 13] {
    [
        "init",
        "status",
        "review",
        "threads",
        "resume",
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

pub(in crate::thread) fn selector_preferences_path(codex_home: &Path) -> PathBuf {
    codex_home
        .join("codex-acp")
        .join("selector-preferences.json")
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
    if let Some(display) = preferences.display {
        if let Some(value) = display.context_control {
            inner.context_control_display = value;
        }
        if let Some(value) = display.context {
            inner.context_display_style = value;
        }
        if let Some(value) = display.limits {
            inner.limits_display_style = value;
        }
    }
    if let Some(value) = preferences.model_selector {
        if let Some(model) = &value.default_model
            && inner
                .models
                .iter()
                .any(|candidate| candidate.id == *model || candidate.model == *model)
        {
            inner.current_model = model.clone();
        }
        if let Some(effort) = value.default_reasoning_effort {
            inner.reasoning_effort = normalize_reasoning_effort_for_preferences(
                &inner.models,
                &inner.current_model,
                &value,
                effort,
            );
        }
        inner.service_tier = value.default_service_tier;
        inner.model_selector = value;
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
            inner.model_selector.default_reasoning_effort = effort;
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
            inner.model_selector.default_service_tier = service_tier;
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
    let mut model_selector = materialized_model_selector(
        existing
            .model_selector
            .as_ref()
            .unwrap_or(&inner.model_selector),
        &inner.models,
    );
    model_selector.default_model = None;
    model_selector.default_reasoning_effort = None;
    model_selector.default_service_tier = None;
    let preferences = SelectorPreferences {
        display: Some(SelectorDisplayPreferences {
            context_control: Some(inner.context_control_display),
            context: Some(inner.context_display_style),
            limits: Some(inner.limits_display_style),
        }),
        defaults: Some(SelectorDefaultPreferences {
            model: Some(inner.model_selector.default_model.clone()),
            reasoning_effort: Some(inner.model_selector.default_reasoning_effort),
            service_tier: Some(inner.model_selector.default_service_tier),
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

fn materialized_model_selector(
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

fn normalize_reasoning_effort_for_preferences(
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

fn materialized_selector_layout(layout: &SelectorLayoutPreferences) -> SelectorLayoutPreferences {
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

fn read_selector_preferences(preferences_path: &Path) -> std::io::Result<SelectorPreferences> {
    match fs::read_to_string(preferences_path) {
        Ok(contents) => serde_json::from_str(&strip_json_trailing_commas(&strip_json_comments(
            &contents,
        )?))
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
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
    let payload = selector_preferences_jsonc(preferences)?;
    fs::write(&tmp_path, payload)?;
    fs::rename(tmp_path, preferences_path)
}

fn selector_preferences_jsonc(preferences: &SelectorPreferences) -> io::Result<Vec<u8>> {
    let mut sections = Vec::new();
    if let Some(display) = &preferences.display {
        sections.push(jsonc_section(
            "Display styles for compact lower-panel selectors.",
            "display",
            display,
        )?);
    }
    if let Some(defaults) = &preferences.defaults {
        sections.push(jsonc_section(
            "Defaults applied when a new ACP session starts. null keeps the app-server default.",
            "defaults",
            defaults,
        )?);
    }
    if let Some(model_selector) = &preferences.model_selector {
        sections.push(jsonc_section(
            "Model selector controls. Comment out list rows to hide them; row order controls menu order.",
            "model_selector",
            model_selector,
        )?);
    }
    if let Some(layout) = &preferences.layout {
        sections.push(jsonc_section(
            "Lower selector order, titles, visibility, and group order.",
            "layout",
            layout,
        )?);
    }
    if let Some(slash_commands) = &preferences.slash_commands {
        sections.push(jsonc_section(
            "Slash commands. Comment out list rows to hide/block them; row order controls Zed command order.",
            "slash_commands",
            slash_commands,
        )?);
    }

    let mut output = String::from("{\n");
    output.push_str(&sections.join(",\n\n"));
    output.push_str("\n}\n");
    Ok(output.into_bytes())
}

fn jsonc_section<T: Serialize>(comment: &str, key: &str, value: &T) -> io::Result<String> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let mut lines = json.lines();
    let mut section = format!("  // {comment}\n  \"{key}\": ");
    if let Some(first) = lines.next() {
        section.push_str(first);
    }
    for line in lines {
        section.push('\n');
        section.push_str("  ");
        section.push_str(line);
    }
    Ok(section)
}

fn strip_json_comments(input: &str) -> io::Result<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        String,
        LineComment,
        BlockComment,
    }

    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut state = State::Normal;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        match state {
            State::Normal => {
                if ch == '"' {
                    output.push(ch);
                    state = State::String;
                } else if ch == '/' && chars.peek() == Some(&'/') {
                    chars.next();
                    output.push(' ');
                    output.push(' ');
                    state = State::LineComment;
                } else if ch == '/' && chars.peek() == Some(&'*') {
                    chars.next();
                    output.push(' ');
                    output.push(' ');
                    state = State::BlockComment;
                } else {
                    output.push(ch);
                }
            }
            State::String => {
                output.push(ch);
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    state = State::Normal;
                }
            }
            State::LineComment => {
                if ch == '\n' {
                    output.push('\n');
                    state = State::Normal;
                } else {
                    output.push(' ');
                }
            }
            State::BlockComment => {
                if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    output.push(' ');
                    output.push(' ');
                    state = State::Normal;
                } else if ch == '\n' {
                    output.push('\n');
                } else {
                    output.push(' ');
                }
            }
        }
    }

    if state == State::BlockComment {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unterminated block comment in selector preferences",
        ));
    }

    Ok(output)
}

fn strip_json_trailing_commas(input: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        String,
    }

    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut state = State::Normal;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        match state {
            State::Normal => {
                if ch == '"' {
                    output.push(ch);
                    state = State::String;
                } else if ch == ',' {
                    let mut lookahead = chars.clone();
                    while matches!(lookahead.peek(), Some(next) if next.is_whitespace()) {
                        lookahead.next();
                    }
                    if matches!(lookahead.peek(), Some(']' | '}')) {
                        output.push(' ');
                    } else {
                        output.push(ch);
                    }
                } else {
                    output.push(ch);
                }
            }
            State::String => {
                output.push(ch);
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    state = State::Normal;
                }
            }
        }
    }

    output
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
        SelectorDefaultPreferences, SelectorDisplayPreferences, SelectorLayoutEntry,
        SelectorLayoutPreferences, SlashCommandPreferences, materialized_model_selector,
        materialized_selector_layout, restore_selector_preferences, selector_preferences_path,
        write_selector_preferences,
    };
    use crate::thread::{
        AppModel, ContextControlDisplay, ContextDisplayStyle, LimitsDisplayStyle, ReasoningEffort,
        ServiceTier,
    };
    use std::path::Path;

    #[test]
    fn preferences_path_uses_codex_acp_subdir() {
        let path = selector_preferences_path(Path::new("/tmp/codex-home"));
        assert_eq!(
            path,
            Path::new("/tmp/codex-home")
                .join("codex-acp")
                .join("selector-preferences.json")
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
            display: Some(SelectorDisplayPreferences {
                context_control: Some(ContextControlDisplay::Limits),
                context: Some(ContextDisplayStyle::Braille),
                limits: Some(LimitsDisplayStyle::Block),
            }),
            defaults: Some(SelectorDefaultPreferences {
                model: Some(Some("gpt-5.5".to_string())),
                reasoning_effort: Some(Some(ReasoningEffort::High)),
                service_tier: Some(Some(ServiceTier::Fast)),
            }),
            model_selector: Some(ModelSelectorPreferences {
                models: vec![ModelSelectorModelEntry::Details(
                    ModelSelectorModelDetails {
                        id: "gpt-5.2".to_string(),
                        name: Some("old 5.2".to_string()),
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
                order: Some(vec![
                    "context_control".to_string(),
                    "model".to_string(),
                    "permissions".to_string(),
                ]),
                permissions: Some(SelectorLayoutEntry {
                    visible: Some(true),
                    name: Some("Mode".to_string()),
                    groups: Some(vec!["workflow".to_string(), "guarded".to_string()]),
                }),
                model: None,
                context_control: Some(SelectorLayoutEntry {
                    visible: Some(false),
                    name: None,
                    groups: None,
                }),
            }),
            slash_commands: Some(SlashCommandPreferences::from_commands(vec![
                "status".to_string(),
                "unarchive".to_string(),
            ])),
        };

        write_selector_preferences(&path, &preferences).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("// Display styles"));
        assert!(written.contains("\"defaults\""));
        let restored = restore_selector_preferences(&path).unwrap();

        let display = restored
            .display
            .expect("display preferences should restore");
        assert_eq!(display.context_control, Some(ContextControlDisplay::Limits));
        assert_eq!(display.context, Some(ContextDisplayStyle::Braille));
        assert_eq!(display.limits, Some(LimitsDisplayStyle::Block));
        let defaults = restored.defaults.expect("defaults should restore");
        assert_eq!(defaults.model, Some(Some("gpt-5.5".to_string())));
        assert_eq!(defaults.reasoning_effort, Some(Some(ReasoningEffort::High)));
        assert_eq!(defaults.service_tier, Some(Some(ServiceTier::Fast)));
        let model_selector = restored
            .model_selector
            .expect("model selector preferences should restore");
        let model_entry = model_selector
            .model_entry("gpt-5.2")
            .expect("model entry should restore");
        assert_eq!(model_entry.name_override(), Some("old 5.2"));
        assert_eq!(
            model_entry.description_override(),
            Some("manual description")
        );
        assert_eq!(
            model_selector.reasoning_effort_name_override(ReasoningEffort::XHigh),
            Some("максимум")
        );
        assert_eq!(
            model_selector.reasoning_effort_description_override(ReasoningEffort::XHigh),
            Some("manual effort description")
        );
        let layout = restored.layout.expect("layout should restore");
        assert_eq!(
            layout.order,
            Some(vec![
                "context_control".to_string(),
                "model".to_string(),
                "permissions".to_string()
            ])
        );
        assert_eq!(
            layout.permissions.and_then(|entry| entry.name),
            Some("Mode".to_string())
        );
        assert_eq!(
            layout.context_control.and_then(|entry| entry.visible),
            Some(false)
        );
        let slash_commands = restored
            .slash_commands
            .expect("slash commands should restore");
        assert!(slash_commands.is_enabled("status"));
        assert!(!slash_commands.is_enabled("review"));
        assert!(!slash_commands.is_enabled("archive"));
        assert!(!slash_commands.is_enabled("delete"));
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
              // grouped display settings
                "display": {
                  "context_control": "context_and_limits",
                  "context": "braille",
                  "limits": "block"
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
        let display = restored.display.expect("display should restore");
        assert_eq!(
            display.context_control,
            Some(ContextControlDisplay::ContextAndLimits)
        );
        assert_eq!(display.context, Some(ContextDisplayStyle::Braille));
        assert_eq!(display.limits, Some(LimitsDisplayStyle::Block));
        let defaults = restored.defaults.expect("defaults should restore");
        assert_eq!(defaults.model, Some(Some("gpt-5.5".to_string())));
        assert_eq!(defaults.reasoning_effort, Some(Some(ReasoningEffort::None)));
        assert_eq!(defaults.service_tier, Some(None));
        let model_selector = restored
            .model_selector
            .expect("model selector should restore");
        assert_eq!(
            model_selector.model_name_override("gpt-5.5"),
            Some("главная")
        );
        assert_eq!(
            model_selector.model_description_override("gpt-5.5"),
            Some("кастомное описание")
        );
        assert!(model_selector.model_entry("gpt-5.2").is_some());
        assert!(model_selector.explicitly_enables_reasoning_effort(ReasoningEffort::None));
        assert!(model_selector.explicitly_enables_reasoning_effort(ReasoningEffort::Minimal));
        assert_eq!(
            model_selector.reasoning_effort_name_override(ReasoningEffort::Minimal),
            Some("минимум")
        );
        assert_eq!(
            model_selector.reasoning_effort_description_override(ReasoningEffort::Minimal),
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
            context_control: Some(SelectorLayoutEntry {
                visible: None,
                name: None,
                groups: Some(vec!["actions".to_string(), "display".to_string()]),
            }),
        });

        assert_eq!(
            layout.order,
            Some(vec![
                "permissions".to_string(),
                "model".to_string(),
                "context_control".to_string()
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
            layout.context_control.and_then(|entry| entry.groups),
            Some(vec!["actions".to_string(), "display".to_string()])
        );
    }
}
