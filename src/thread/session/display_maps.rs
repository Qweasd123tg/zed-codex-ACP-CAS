//! Configurable percent-to-label display maps for compact selector values.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[path = "display_maps/jsonc.rs"]
mod jsonc;

const DEFAULT_PRIMARY_LIMITS_MAP_ID: &str = "five_hour_percent";
const DEFAULT_SECONDARY_LIMITS_MAP_ID: &str = "weekly_percent";
const DEFAULT_CONTEXT_USAGE_MAP_ID: &str = "context_percent";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct DisplayMapsConfig {
    #[serde(default)]
    pub(in crate::thread) context: ContextDisplayMapSelection,
    #[serde(default)]
    pub(in crate::thread) limits: LimitsDisplayMapSelection,
    #[serde(default = "default_percent_maps")]
    maps: BTreeMap<String, PercentDisplayMap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct ContextDisplayMapSelection {
    #[serde(default = "default_context_usage_map_id")]
    usage: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct LimitsDisplayMapSelection {
    #[serde(default = "default_primary_limits_map_id")]
    primary: String,
    #[serde(default = "default_secondary_limits_map_id")]
    secondary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(in crate::thread) enum PercentDisplayMap {
    Template {
        template: String,
        #[serde(default)]
        unavailable: Option<String>,
    },
    Exact {
        values: BTreeMap<String, String>,
        #[serde(default)]
        fallback: Option<String>,
        #[serde(default)]
        unavailable: Option<String>,
    },
    Thresholds {
        values: Vec<PercentThresholdLabel>,
        #[serde(default)]
        fallback: Option<String>,
        #[serde(default)]
        unavailable: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::thread) struct PercentThresholdLabel {
    min: u8,
    label: String,
}

impl Default for DisplayMapsConfig {
    fn default() -> Self {
        Self {
            context: ContextDisplayMapSelection::default(),
            limits: LimitsDisplayMapSelection::default(),
            maps: default_percent_maps(),
        }
    }
}

impl Default for ContextDisplayMapSelection {
    fn default() -> Self {
        Self {
            usage: default_context_usage_map_id(),
        }
    }
}

impl Default for LimitsDisplayMapSelection {
    fn default() -> Self {
        Self {
            primary: default_primary_limits_map_id(),
            secondary: default_secondary_limits_map_id(),
        }
    }
}

impl DisplayMapsConfig {
    pub(in crate::thread) fn render_context_usage(
        &self,
        usage_percent: Option<u64>,
        unavailable_override: Option<&str>,
    ) -> String {
        let map = self
            .maps
            .get(self.context.usage.as_str())
            .expect("display maps config should validate context usage map");
        match usage_percent {
            Some(usage_percent) => {
                let value = clamp_percent_u64(usage_percent);
                map.render(value)
            }
            None => unavailable_override
                .map(ToString::to_string)
                .or_else(|| map.unavailable_label().map(ToString::to_string))
                .unwrap_or_else(|| "---".to_string()),
        }
    }

    pub(in crate::thread) fn render_primary_limit_remaining(
        &self,
        remaining_percent: Option<i32>,
    ) -> String {
        self.render_limit_remaining(remaining_percent, self.limits.primary.as_str())
    }

    pub(in crate::thread) fn render_secondary_limit_remaining(
        &self,
        remaining_percent: Option<i32>,
    ) -> String {
        self.render_limit_remaining(remaining_percent, self.limits.secondary.as_str())
    }

    fn render_limit_remaining(&self, remaining_percent: Option<i32>, map_id: &str) -> String {
        let map = self
            .maps
            .get(map_id)
            .expect("display maps config should validate limit map");
        match remaining_percent {
            Some(remaining_percent) => {
                let value = clamp_percent(remaining_percent);
                map.render(value)
            }
            None => map
                .unavailable_label()
                .map(ToString::to_string)
                .unwrap_or_else(|| "--".to_string()),
        }
    }
}

impl PercentDisplayMap {
    fn render(&self, value: u8) -> String {
        match self {
            Self::Template { template, .. } => render_template(template, value),
            Self::Exact {
                values, fallback, ..
            } => values
                .get(&value.to_string())
                .cloned()
                .or_else(|| {
                    fallback
                        .as_ref()
                        .map(|template| render_template(template, value))
                })
                .unwrap_or_else(|| fallback_percent_label(value)),
            Self::Thresholds {
                values, fallback, ..
            } => values
                .iter()
                .filter(|entry| value >= entry.min)
                .max_by_key(|entry| entry.min)
                .map(|entry| entry.label.clone())
                .or_else(|| {
                    fallback
                        .as_ref()
                        .map(|template| render_template(template, value))
                })
                .unwrap_or_else(|| fallback_percent_label(value)),
        }
    }

    fn unavailable_label(&self) -> Option<&str> {
        match self {
            Self::Template { unavailable, .. }
            | Self::Exact { unavailable, .. }
            | Self::Thresholds { unavailable, .. } => unavailable.as_deref(),
        }
    }
}

pub(in crate::thread) fn display_maps_path(cas_home: &Path) -> PathBuf {
    cas_home.join("display-maps.json")
}

pub(in crate::thread) fn restore_display_maps(path: &Path) -> std::io::Result<DisplayMapsConfig> {
    read_display_maps(path)
}

pub(in crate::thread) fn persist_display_maps(
    path: &Path,
    config: &DisplayMapsConfig,
) -> std::io::Result<()> {
    write_display_maps(path, config)
}

fn read_display_maps(path: &Path) -> std::io::Result<DisplayMapsConfig> {
    let config = match fs::read_to_string(path) {
        Ok(contents) => jsonc::parse_display_maps_jsonc(&contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(DisplayMapsConfig::default())
        }
        Err(error) => Err(error),
    }?;
    validate_display_maps_config(&config)?;
    Ok(config)
}

fn write_display_maps(path: &Path, config: &DisplayMapsConfig) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let payload = jsonc::display_maps_jsonc(config)?;
    fs::write(&tmp_path, payload)?;
    fs::rename(tmp_path, path)
}

fn default_percent_maps() -> BTreeMap<String, PercentDisplayMap> {
    BTreeMap::from([
        (
            DEFAULT_CONTEXT_USAGE_MAP_ID.to_string(),
            PercentDisplayMap::Template {
                template: "{value}%".to_string(),
                unavailable: Some("---".to_string()),
            },
        ),
        (
            "context_braille".to_string(),
            PercentDisplayMap::Thresholds {
                values: vec![
                    PercentThresholdLabel {
                        min: 0,
                        label: "⠀".to_string(),
                    },
                    PercentThresholdLabel {
                        min: 7,
                        label: "⢀".to_string(),
                    },
                    PercentThresholdLabel {
                        min: 19,
                        label: "⣀".to_string(),
                    },
                    PercentThresholdLabel {
                        min: 32,
                        label: "⣄".to_string(),
                    },
                    PercentThresholdLabel {
                        min: 44,
                        label: "⣤".to_string(),
                    },
                    PercentThresholdLabel {
                        min: 57,
                        label: "⣦".to_string(),
                    },
                    PercentThresholdLabel {
                        min: 69,
                        label: "⣶".to_string(),
                    },
                    PercentThresholdLabel {
                        min: 82,
                        label: "⣷".to_string(),
                    },
                    PercentThresholdLabel {
                        min: 94,
                        label: "⣿".to_string(),
                    },
                ],
                fallback: Some("{value}%".to_string()),
                unavailable: Some("⠀".to_string()),
            },
        ),
        (
            "percent".to_string(),
            PercentDisplayMap::Template {
                template: "{value}%".to_string(),
                unavailable: Some("--".to_string()),
            },
        ),
        (
            DEFAULT_PRIMARY_LIMITS_MAP_ID.to_string(),
            PercentDisplayMap::Template {
                template: "5h {value}%".to_string(),
                unavailable: Some("5h --".to_string()),
            },
        ),
        (
            DEFAULT_SECONDARY_LIMITS_MAP_ID.to_string(),
            PercentDisplayMap::Template {
                template: "wk {value}%".to_string(),
                unavailable: Some("wk --".to_string()),
            },
        ),
    ])
}

fn default_primary_limits_map_id() -> String {
    DEFAULT_PRIMARY_LIMITS_MAP_ID.to_string()
}

fn default_secondary_limits_map_id() -> String {
    DEFAULT_SECONDARY_LIMITS_MAP_ID.to_string()
}

fn default_context_usage_map_id() -> String {
    DEFAULT_CONTEXT_USAGE_MAP_ID.to_string()
}

fn render_template(template: &str, value: u8) -> String {
    template
        .replace("{value}", &value.to_string())
        .replace("{percent}", &value.to_string())
}

fn fallback_percent_label(value: u8) -> String {
    format!("{value}%")
}

fn clamp_percent(value: i32) -> u8 {
    value.clamp(0, 100) as u8
}

fn clamp_percent_u64(value: u64) -> u8 {
    value.min(100) as u8
}

fn validate_display_maps_config(config: &DisplayMapsConfig) -> std::io::Result<()> {
    validate_map_ref(&config.maps, "context.usage", config.context.usage.as_str())?;
    validate_map_ref(
        &config.maps,
        "limits.primary",
        config.limits.primary.as_str(),
    )?;
    validate_map_ref(
        &config.maps,
        "limits.secondary",
        config.limits.secondary.as_str(),
    )
}

fn validate_map_ref(
    maps: &BTreeMap<String, PercentDisplayMap>,
    field: &str,
    map_id: &str,
) -> std::io::Result<()> {
    if map_id.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("display maps `{field}` must not be empty"),
        ));
    }
    if !maps.contains_key(map_id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("display maps `{field}` references missing map `{map_id}`"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ContextDisplayMapSelection, DisplayMapsConfig, LimitsDisplayMapSelection,
        PercentDisplayMap, PercentThresholdLabel, display_maps_path, persist_display_maps,
        restore_display_maps,
    };
    use std::collections::BTreeMap;
    use std::path::Path;

    #[test]
    fn default_display_maps_render_percent_labels() {
        let maps = DisplayMapsConfig::default();

        assert_eq!(maps.render_primary_limit_remaining(Some(1)), "5h 1%");
        assert_eq!(maps.render_primary_limit_remaining(Some(100)), "5h 100%");
        assert_eq!(maps.render_primary_limit_remaining(Some(150)), "5h 100%");
        assert_eq!(maps.render_primary_limit_remaining(None), "5h --");
        assert_eq!(maps.render_secondary_limit_remaining(Some(42)), "wk 42%");
        assert_eq!(maps.render_secondary_limit_remaining(None), "wk --");
        assert_eq!(maps.render_context_usage(Some(42), None), "42%");
        assert_eq!(maps.render_context_usage(None, None), "---");
        assert_eq!(
            maps.render_context_usage(None, Some("157K tok")),
            "157K tok"
        );
    }

    #[test]
    fn threshold_maps_render_highest_matching_label() {
        let maps = DisplayMapsConfig {
            context: ContextDisplayMapSelection {
                usage: "braille".to_string(),
            },
            limits: LimitsDisplayMapSelection {
                primary: "braille".to_string(),
                secondary: "braille".to_string(),
            },
            maps: BTreeMap::from([(
                "braille".to_string(),
                PercentDisplayMap::Thresholds {
                    values: vec![
                        PercentThresholdLabel {
                            min: 0,
                            label: String::new(),
                        },
                        PercentThresholdLabel {
                            min: 1,
                            label: "a".to_string(),
                        },
                        PercentThresholdLabel {
                            min: 13,
                            label: "b".to_string(),
                        },
                    ],
                    fallback: Some("{value}%".to_string()),
                    unavailable: Some("?".to_string()),
                },
            )]),
        };

        assert_eq!(maps.render_primary_limit_remaining(Some(0)), "");
        assert_eq!(maps.render_primary_limit_remaining(Some(12)), "a");
        assert_eq!(maps.render_primary_limit_remaining(Some(13)), "b");
        assert_eq!(maps.render_primary_limit_remaining(None), "?");
        assert_eq!(maps.render_context_usage(Some(13), None), "b");
    }

    #[test]
    fn restores_jsonc_display_maps() {
        let path = std::env::temp_dir().join(format!(
            "codex-acp-display-maps-{}.json",
            uuid::Uuid::new_v4()
        ));
        let contents = r#"
        {
          // Active account limit maps.
          "context": {
            "usage": "dots",
          },
          "limits": {
            "primary": "dots",
            "secondary": "percent",
          },
          "maps": {
            "percent": { "kind": "template", "template": "{value}%" },
            "dots": {
              "kind": "exact",
              "values": {
                "0": "",
                "1": ".",
              },
              "fallback": "{value}%"
            }
          }
        }
        "#;
        std::fs::write(&path, contents).expect("display maps fixture should write");

        let restored = restore_display_maps(&path).expect("display maps should restore");
        assert_eq!(restored.render_context_usage(Some(1), None), ".");
        assert_eq!(restored.render_context_usage(Some(2), None), "2%");
        assert_eq!(restored.render_primary_limit_remaining(Some(1)), ".");
        assert_eq!(restored.render_primary_limit_remaining(Some(2)), "2%");
        assert_eq!(restored.render_secondary_limit_remaining(Some(2)), "2%");
        drop(std::fs::remove_file(path));
    }

    #[test]
    fn rejects_missing_selected_display_maps() {
        let path = std::env::temp_dir().join(format!(
            "codex-acp-display-maps-invalid-{}.json",
            uuid::Uuid::new_v4()
        ));
        let contents = r#"
        {
          "limits": {
            "primary": "percent",
            "secondary": "percent"
          },
          "maps": {
            "percent": { "kind": "template", "template": "{value}%" }
          }
        }
        "#;
        std::fs::write(&path, contents).expect("display maps fixture should write");

        let error = restore_display_maps(&path).expect_err("missing context map should fail");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            error
                .to_string()
                .contains("context.usage` references missing map `context_percent")
        );
        drop(std::fs::remove_file(path));
    }

    #[test]
    fn writes_default_display_maps_to_cas_home() {
        let path = display_maps_path(Path::new("/tmp/.codex-cas"));
        assert_eq!(path, Path::new("/tmp/.codex-cas").join("display-maps.json"));

        let temp_path = std::env::temp_dir().join(format!(
            "codex-acp-display-maps-write-{}.json",
            uuid::Uuid::new_v4()
        ));
        persist_display_maps(&temp_path, &DisplayMapsConfig::default())
            .expect("display maps should persist");
        let restored = restore_display_maps(&temp_path).expect("display maps should restore");
        assert_eq!(restored, DisplayMapsConfig::default());
        drop(std::fs::remove_file(temp_path));
    }

    #[test]
    fn documented_display_map_examples_restore() {
        let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("display-maps");

        let text =
            restore_display_maps(&examples_dir.join("text.jsonc")).expect("text example restores");
        assert_eq!(text.render_context_usage(Some(80), None), "80%");
        assert_eq!(text.render_primary_limit_remaining(Some(80)), "5h 80%");
        assert_eq!(text.render_secondary_limit_remaining(Some(6)), "wk 6%");

        let bars =
            restore_display_maps(&examples_dir.join("bars.jsonc")).expect("bars example restores");
        assert_eq!(bars.render_context_usage(Some(80), None), "80%");
        assert_eq!(bars.render_primary_limit_remaining(Some(0)), "▱▱▱▱▱");
        assert_eq!(bars.render_primary_limit_remaining(Some(1)), "▰▱▱▱▱");
        assert_eq!(bars.render_primary_limit_remaining(Some(80)), "▰▰▰▰▱");
        assert_eq!(bars.render_primary_limit_remaining(Some(100)), "▰▰▰▰▰");
        assert_eq!(bars.render_secondary_limit_remaining(Some(6)), "wk 6%");

        let block = restore_display_maps(&examples_dir.join("block.jsonc"))
            .expect("block example restores");
        assert_eq!(block.render_context_usage(Some(80), None), "80%");
        assert_eq!(block.render_primary_limit_remaining(Some(7)), "▁");
        assert_eq!(block.render_primary_limit_remaining(Some(8)), "▂");
        assert_eq!(block.render_primary_limit_remaining(Some(80)), "▇");
        assert_eq!(block.render_primary_limit_remaining(Some(93)), "█");
        assert_eq!(block.render_secondary_limit_remaining(Some(6)), "wk 6%");

        let context_percent = restore_display_maps(&examples_dir.join("context-percent.jsonc"))
            .expect("context percent example restores");
        assert_eq!(context_percent.render_context_usage(Some(76), None), "76%");
        assert_eq!(context_percent.render_context_usage(None, None), "---");

        let context_braille = restore_display_maps(&examples_dir.join("context-braille.jsonc"))
            .expect("context braille example restores");
        assert_eq!(context_braille.render_context_usage(Some(0), None), "⠀");
        assert_eq!(context_braille.render_context_usage(Some(12), None), "⢀");
        assert_eq!(context_braille.render_context_usage(Some(76), None), "⣶");
        assert_eq!(context_braille.render_context_usage(Some(100), None), "⣿");
    }
}
