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
}

pub(in crate::thread) fn selector_preferences_path(codex_home: &Path) -> PathBuf {
    codex_home
        .join("memories")
        .join("codex-acp")
        .join("selector-preferences.json")
}

pub(in crate::thread) fn restore_selector_preferences(
    preferences_path: &Path,
) -> SelectorPreferences {
    read_selector_preferences(preferences_path).unwrap_or_default()
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
}

pub(in crate::thread) fn persist_selector_preferences(inner: &ThreadInner) -> std::io::Result<()> {
    let preferences = SelectorPreferences {
        context_control_display: Some(inner.context_control_display),
        context_display_style: Some(inner.context_display_style),
        limits_display_style: Some(inner.limits_display_style),
        model_display_style: Some(inner.model_display_style),
        reasoning_effort_display_style: Some(inner.reasoning_effort_display_style),
    };
    write_selector_preferences(&inner.selector_preferences_path, &preferences)
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
        };

        write_selector_preferences(&path, &preferences).unwrap();
        let restored = restore_selector_preferences(&path);

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

        drop(std::fs::remove_file(path));
    }
}
