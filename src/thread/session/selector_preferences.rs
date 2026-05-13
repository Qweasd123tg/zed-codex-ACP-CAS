//! Persistent adapter-side defaults for nested selector state that ACP cannot model directly.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::thread::{
    ContextControlDisplay, ContextDisplayStyle, LimitsDisplayStyle, ModelDisplayStyle,
    ReasoningEffortDisplayStyle, ThreadInner,
};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct SelectorPreferences {
    pub(in crate::thread) context_control_display: Option<ContextControlDisplay>,
    pub(in crate::thread) context_display_style: Option<ContextDisplayStyle>,
    pub(in crate::thread) limits_display_style: Option<LimitsDisplayStyle>,
    pub(in crate::thread) model_display_style: Option<ModelDisplayStyle>,
    pub(in crate::thread) reasoning_effort_display_style: Option<ReasoningEffortDisplayStyle>,
    pub(in crate::thread) layout: Option<SelectorLayoutPreferences>,
    pub(in crate::thread) slash_commands: Option<SlashCommandPreferences>,
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
pub(in crate::thread) struct SlashCommandPreferences {
    #[serde(default = "default_enabled")]
    pub(in crate::thread) init: bool,
    #[serde(default = "default_enabled")]
    pub(in crate::thread) status: bool,
    #[serde(default = "default_enabled")]
    pub(in crate::thread) review: bool,
    #[serde(default = "default_enabled")]
    pub(in crate::thread) threads: bool,
    #[serde(default = "default_enabled")]
    pub(in crate::thread) resume: bool,
    #[serde(default = "default_enabled")]
    pub(in crate::thread) fork: bool,
    #[serde(default = "default_enabled")]
    pub(in crate::thread) archive: bool,
    #[serde(default = "default_enabled")]
    pub(in crate::thread) unarchive: bool,
    #[serde(default = "default_enabled")]
    pub(in crate::thread) compact: bool,
    #[serde(default = "default_enabled")]
    pub(in crate::thread) undo: bool,
    #[serde(default = "default_enabled")]
    pub(in crate::thread) plan: bool,
    #[serde(default = "default_enabled")]
    pub(in crate::thread) rename: bool,
    #[serde(default = "default_enabled")]
    pub(in crate::thread) diff: bool,
}

impl Default for SlashCommandPreferences {
    fn default() -> Self {
        Self {
            init: true,
            status: true,
            review: true,
            threads: true,
            resume: true,
            fork: true,
            archive: true,
            unarchive: true,
            compact: true,
            undo: true,
            plan: true,
            rename: true,
            diff: true,
        }
    }
}

impl SlashCommandPreferences {
    pub(in crate::thread) fn is_enabled(&self, command: &str) -> bool {
        match command {
            "init" => self.init,
            "status" => self.status,
            "review" => self.review,
            "threads" => self.threads,
            "resume" => self.resume,
            "fork" => self.fork,
            "archive" | "delete" => self.archive,
            "unarchive" => self.unarchive,
            "compact" => self.compact,
            "undo" => self.undo,
            "plan" => self.plan,
            "rename" => self.rename,
            "diff" => self.diff,
            _ => true,
        }
    }
}

fn default_enabled() -> bool {
    true
}

pub(in crate::thread) fn selector_preferences_path(codex_home: &Path) -> PathBuf {
    codex_home
        .join("codex-acp")
        .join("selector-preferences.json")
}

pub(in crate::thread) fn legacy_selector_preferences_path(codex_home: &Path) -> PathBuf {
    codex_home
        .join("memories")
        .join("codex-acp")
        .join("selector-preferences.json")
}

pub(in crate::thread) fn restore_selector_preferences(
    preferences_path: &Path,
    legacy_preferences_path: &Path,
) -> SelectorPreferences {
    if preferences_path.exists() {
        read_selector_preferences(preferences_path).unwrap_or_default()
    } else {
        read_selector_preferences(legacy_preferences_path).unwrap_or_default()
    }
}

pub(in crate::thread) fn apply_selector_preferences(
    inner: &mut ThreadInner,
    preferences: SelectorPreferences,
) {
    if let Some(value) = preferences.context_control_display {
        inner.context_control_display = value;
    }
    if let Some(value) = preferences.context_display_style {
        inner.context_display_style = value;
    }
    if let Some(value) = preferences.limits_display_style {
        inner.limits_display_style = value;
    }
    if let Some(value) = preferences.model_display_style {
        inner.model_display_style = value;
    }
    if let Some(value) = preferences.reasoning_effort_display_style {
        inner.reasoning_effort_display_style = value;
    }
    if let Some(value) = preferences.layout {
        inner.selector_layout = value;
    }
    if let Some(value) = preferences.slash_commands {
        inner.slash_commands = value;
    }
}

pub(in crate::thread) fn persist_selector_preferences(inner: &ThreadInner) -> std::io::Result<()> {
    let preferences = SelectorPreferences {
        context_control_display: Some(inner.context_control_display),
        context_display_style: Some(inner.context_display_style),
        limits_display_style: Some(inner.limits_display_style),
        model_display_style: Some(inner.model_display_style),
        reasoning_effort_display_style: Some(inner.reasoning_effort_display_style),
        layout: Some(materialized_selector_layout(&inner.selector_layout)),
        slash_commands: Some(inner.slash_commands.clone()),
    };
    write_selector_preferences(&inner.selector_preferences_path, &preferences)
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
        Ok(contents) => serde_json::from_str(&contents)
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
    let payload = serde_json::to_vec_pretty(preferences)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    fs::write(&tmp_path, payload)?;
    fs::rename(tmp_path, preferences_path)
}

#[cfg(test)]
mod tests {
    use super::{
        SelectorLayoutEntry, SelectorLayoutPreferences, SlashCommandPreferences,
        legacy_selector_preferences_path, materialized_selector_layout,
        restore_selector_preferences, selector_preferences_path, write_selector_preferences,
    };
    use crate::thread::{
        ContextControlDisplay, ContextDisplayStyle, LimitsDisplayStyle, ModelDisplayStyle,
        ReasoningEffortDisplayStyle,
    };
    use std::path::Path;

    #[test]
    fn preferences_path_uses_codex_memories_subdir() {
        let path = selector_preferences_path(Path::new("/tmp/codex-home"));
        assert_eq!(
            path,
            Path::new("/tmp/codex-home")
                .join("codex-acp")
                .join("selector-preferences.json")
        );
        assert_eq!(
            legacy_selector_preferences_path(Path::new("/tmp/codex-home")),
            Path::new("/tmp/codex-home")
                .join("memories")
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
            context_control_display: Some(ContextControlDisplay::Limits),
            context_display_style: Some(ContextDisplayStyle::Braille),
            limits_display_style: Some(LimitsDisplayStyle::Block),
            model_display_style: Some(ModelDisplayStyle::WithoutPrefix),
            reasoning_effort_display_style: Some(ReasoningEffortDisplayStyle::Text),
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
            slash_commands: Some(SlashCommandPreferences {
                review: false,
                archive: false,
                ..Default::default()
            }),
        };

        write_selector_preferences(&path, &preferences).unwrap();
        let restored = restore_selector_preferences(&path, Path::new("/tmp/missing-legacy.json"));

        assert_eq!(
            restored.context_control_display,
            Some(ContextControlDisplay::Limits)
        );
        assert_eq!(
            restored.context_display_style,
            Some(ContextDisplayStyle::Braille)
        );
        assert_eq!(
            restored.limits_display_style,
            Some(LimitsDisplayStyle::Block)
        );
        assert_eq!(
            restored.model_display_style,
            Some(ModelDisplayStyle::WithoutPrefix)
        );
        assert_eq!(
            restored.reasoning_effort_display_style,
            Some(ReasoningEffortDisplayStyle::Text)
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
        assert!(!slash_commands.review);
        assert!(!slash_commands.archive);
        assert!(slash_commands.status);
        assert!(!slash_commands.is_enabled("delete"));

        drop(std::fs::remove_file(path));
    }

    #[test]
    fn restores_legacy_preferences_when_new_file_is_missing() {
        let mut legacy_path = std::env::temp_dir();
        legacy_path.push(format!(
            "codex-acp-selector-preferences-legacy-{}.json",
            uuid::Uuid::new_v4()
        ));
        let preferences = super::SelectorPreferences {
            context_control_display: Some(ContextControlDisplay::ContextAndLimits),
            ..Default::default()
        };

        write_selector_preferences(&legacy_path, &preferences).unwrap();
        let restored =
            restore_selector_preferences(Path::new("/tmp/missing-new.json"), &legacy_path);

        assert_eq!(
            restored.context_control_display,
            Some(ContextControlDisplay::ContextAndLimits)
        );

        drop(std::fs::remove_file(legacy_path));
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
