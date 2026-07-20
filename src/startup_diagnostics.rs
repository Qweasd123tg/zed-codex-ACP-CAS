//! Startup identity and config-load diagnostics.
//!
//! Keep this boundary independent from the pinned config schema so a schema
//! mismatch can still produce actionable output before ACP starts.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

pub(crate) const CODEX_APP_SERVER_BIN_ENV: &str = "CODEX_ACP_CODEX_BIN";
pub(crate) const CODEX_SCHEMA_RELEASE: &str = "rust-v0.144.6";
pub(crate) const CODEX_SCHEMA_REVISION: &str = "5d1fbf26c43abc65a203928b2e31561cb039e06d";

const BACKEND_VERSION_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PROBE_DETAIL_CHARS: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigLoadFailureKind {
    UnsupportedSchemaValue,
    InvalidConfiguration,
}

impl ConfigLoadFailureKind {
    fn classify(parse_error: &str) -> Self {
        let parse_error = parse_error.to_ascii_lowercase();
        if parse_error.contains("unknown variant") || parse_error.contains("unknown field") {
            Self::UnsupportedSchemaValue
        } else {
            Self::InvalidConfiguration
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::UnsupportedSchemaValue => {
                "unsupported-config-schema-value (adapter/backend version skew is possible)"
            }
            Self::InvalidConfiguration => "invalid-config",
        }
    }
}

#[derive(Debug)]
struct BackendIdentity {
    command: String,
    resolved_path: Option<PathBuf>,
    version: Result<String, String>,
}

impl BackendIdentity {
    async fn probe(command: String) -> Self {
        let resolved_path = resolve_backend_path(&command);
        let version = probe_backend_version(&command).await;
        Self {
            command,
            resolved_path,
            version,
        }
    }
}

#[derive(Debug)]
struct CodexHomeIdentity {
    display: String,
    config_path: String,
}

impl CodexHomeIdentity {
    fn resolve() -> Self {
        match codex_core::config::find_codex_home() {
            Ok(path) => Self::from_path(path.to_path_buf()),
            Err(error) => {
                let configured = std::env::var_os("CODEX_HOME")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from);
                match configured {
                    Some(path) => Self {
                        display: format!("{} (unresolved: {error})", path.display()),
                        config_path: path.join("config.toml").display().to_string(),
                    },
                    None => Self {
                        display: format!("<unresolved: {error}>"),
                        config_path: "<unresolved>".to_string(),
                    },
                }
            }
        }
    }

    fn from_path(path: PathBuf) -> Self {
        Self {
            config_path: path.join("config.toml").display().to_string(),
            display: path.display().to_string(),
        }
    }
}

#[derive(Debug)]
struct ConfigLoadReport {
    adapter_version: &'static str,
    backend: BackendIdentity,
    codex_home: CodexHomeIdentity,
    failure_kind: ConfigLoadFailureKind,
    parse_error: String,
}

impl ConfigLoadReport {
    async fn capture(parse_error: String) -> Self {
        let backend = BackendIdentity::probe(configured_backend_binary()).await;
        Self {
            adapter_version: env!("CARGO_PKG_VERSION"),
            backend,
            codex_home: CodexHomeIdentity::resolve(),
            failure_kind: ConfigLoadFailureKind::classify(&parse_error),
            parse_error,
        }
    }
}

impl fmt::Display for ConfigLoadReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "error loading Codex config")?;
        writeln!(
            formatter,
            "Classification: {}",
            self.failure_kind.description()
        )?;
        writeln!(
            formatter,
            "Adapter version: codex-acp {}",
            self.adapter_version
        )?;
        writeln!(
            formatter,
            "Adapter Codex schema: {CODEX_SCHEMA_RELEASE} (openai/codex {CODEX_SCHEMA_REVISION})"
        )?;
        writeln!(formatter, "Backend command: {}", self.backend.command)?;
        match &self.backend.resolved_path {
            Some(path) => writeln!(formatter, "Backend path: {}", path.display())?,
            None => writeln!(formatter, "Backend path: <not resolved from PATH>")?,
        }
        match &self.backend.version {
            Ok(version) => writeln!(formatter, "Backend version: {version}")?,
            Err(error) => writeln!(formatter, "Backend version: <unavailable: {error}>")?,
        }
        writeln!(formatter, "CODEX_HOME: {}", self.codex_home.display)?;
        writeln!(formatter, "Config path: {}", self.codex_home.config_path)?;
        writeln!(formatter, "Parse error: {}", self.parse_error)?;
        write!(
            formatter,
            "The adapter did not modify the config file. Update the adapter/backend together or use a value supported by the adapter's pinned config schema."
        )
    }
}

pub(crate) async fn config_load_error(parse_error: impl ToString) -> std::io::Error {
    let report = ConfigLoadReport::capture(parse_error.to_string()).await;
    std::io::Error::new(std::io::ErrorKind::InvalidData, report.to_string())
}

pub(crate) fn configured_backend_binary() -> String {
    let configured = std::env::var(CODEX_APP_SERVER_BIN_ENV).ok();
    configured_backend_binary_from_env_value(configured.as_deref())
}

fn configured_backend_binary_from_env_value(value: Option<&str>) -> String {
    match value {
        Some(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => "codex".to_string(),
    }
}

fn resolve_backend_path(command: &str) -> Option<PathBuf> {
    let command_path = Path::new(command);
    if command_path.is_absolute() || command_path.components().count() > 1 {
        return Some(canonicalize_if_possible(command_path));
    }

    let search_path = std::env::var_os("PATH")?;
    std::env::split_paths(&search_path)
        .flat_map(|directory| executable_candidates(&directory, command))
        .find(|candidate| candidate.is_file())
        .map(|candidate| canonicalize_if_possible(&candidate))
}

fn executable_candidates(directory: &Path, command: &str) -> Vec<PathBuf> {
    let candidate = directory.join(command);
    #[cfg(windows)]
    {
        if candidate.extension().is_none() {
            return vec![candidate.clone(), candidate.with_extension("exe")];
        }
    }
    vec![candidate]
}

fn canonicalize_if_possible(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

async fn probe_backend_version(command: &str) -> Result<String, String> {
    let mut process = Command::new(command);
    process
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let output = tokio::time::timeout(BACKEND_VERSION_TIMEOUT, process.output())
        .await
        .map_err(|_| "version probe timed out after 2s".to_string())?
        .map_err(|error| compact_probe_detail(&error.to_string()))?;

    if output.status.success() {
        first_non_empty_line(&output.stdout)
            .or_else(|| first_non_empty_line(&output.stderr))
            .ok_or_else(|| "version probe returned no output".to_string())
    } else {
        let detail = first_non_empty_line(&output.stderr)
            .or_else(|| first_non_empty_line(&output.stdout))
            .unwrap_or_else(|| "no output".to_string());
        Err(format!("probe exited with {}: {detail}", output.status))
    }
}

fn first_non_empty_line(output: &[u8]) -> Option<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(compact_probe_detail)
}

fn compact_probe_detail(detail: &str) -> String {
    let compact: String = detail.chars().take(MAX_PROBE_DETAIL_CHARS).collect();
    if detail.chars().count() > MAX_PROBE_DETAIL_CHARS {
        format!("{compact}...")
    } else {
        compact
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BackendIdentity, CODEX_SCHEMA_RELEASE, CODEX_SCHEMA_REVISION, CodexHomeIdentity,
        ConfigLoadFailureKind, ConfigLoadReport, configured_backend_binary_from_env_value,
        first_non_empty_line,
    };
    use std::path::PathBuf;

    #[test]
    fn backend_binary_defaults_to_codex() {
        assert_eq!(configured_backend_binary_from_env_value(None), "codex");
        assert_eq!(configured_backend_binary_from_env_value(Some("")), "codex");
        assert_eq!(
            configured_backend_binary_from_env_value(Some("   ")),
            "codex"
        );
    }

    #[test]
    fn backend_binary_uses_trimmed_configured_path() {
        assert_eq!(
            configured_backend_binary_from_env_value(Some("  /opt/codex/bin/codex  ")),
            "/opt/codex/bin/codex"
        );
    }

    #[test]
    fn unknown_enum_value_is_classified_as_schema_compatibility_failure() {
        assert_eq!(
            ConfigLoadFailureKind::classify(
                "config.toml:1:26: unknown variant `max`, expected one of `high`, `xhigh`"
            ),
            ConfigLoadFailureKind::UnsupportedSchemaValue
        );
        assert_eq!(
            ConfigLoadFailureKind::classify("config.toml:3:1: expected a string"),
            ConfigLoadFailureKind::InvalidConfiguration
        );
    }

    #[test]
    fn report_contains_versions_paths_original_error_and_no_write_guarantee() {
        let report = ConfigLoadReport {
            adapter_version: "0.26.7",
            backend: BackendIdentity {
                command: "codex".to_string(),
                resolved_path: Some(PathBuf::from("/usr/bin/codex")),
                version: Ok("codex-cli 1.2.3".to_string()),
            },
            codex_home: CodexHomeIdentity::from_path(PathBuf::from("/home/test/.codex")),
            failure_kind: ConfigLoadFailureKind::UnsupportedSchemaValue,
            parse_error: "unknown variant `ultra`".to_string(),
        }
        .to_string();

        assert!(report.contains("Adapter version: codex-acp 0.26.7"));
        assert!(report.contains(concat!(
            "Adapter Codex schema: rust-v0.144.6 (openai/codex ",
            "5d1fbf26c43abc65a203928b2e31561cb039e06d)"
        )));
        assert!(report.contains("Backend path: /usr/bin/codex"));
        assert!(report.contains("Backend version: codex-cli 1.2.3"));
        assert!(report.contains("CODEX_HOME: /home/test/.codex"));
        assert!(report.contains("Config path: /home/test/.codex/config.toml"));
        assert!(report.contains("Parse error: unknown variant `ultra`"));
        assert!(report.contains("did not modify the config file"));
    }

    #[test]
    fn version_probe_uses_first_non_empty_line() {
        assert_eq!(
            first_non_empty_line(b"\n  codex-cli 1.2.3  \nignored\n"),
            Some("codex-cli 1.2.3".to_string())
        );
    }

    #[test]
    fn schema_identity_matches_codex_dependencies_and_lockfile() {
        let manifest: toml::Value =
            toml::from_str(include_str!("../Cargo.toml")).expect("Cargo.toml should parse");
        let dependencies = manifest
            .get("dependencies")
            .and_then(toml::Value::as_table)
            .expect("Cargo.toml should contain dependencies");
        let codex_dependencies = dependencies
            .iter()
            .filter(|(name, _)| name.starts_with("codex-"))
            .collect::<Vec<_>>();
        assert!(!codex_dependencies.is_empty());
        for (name, dependency) in codex_dependencies {
            let dependency = dependency
                .as_table()
                .unwrap_or_else(|| panic!("{name} should use a detailed dependency spec"));
            assert_eq!(
                dependency.get("git").and_then(toml::Value::as_str),
                Some("https://github.com/openai/codex"),
                "{name} should come from the coordinated official Codex source"
            );
            assert_eq!(
                dependency.get("rev").and_then(toml::Value::as_str),
                Some(CODEX_SCHEMA_REVISION),
                "{name} should match the schema revision shown in diagnostics"
            );
        }

        let lockfile: toml::Value =
            toml::from_str(include_str!("../Cargo.lock")).expect("Cargo.lock should parse");
        let packages = lockfile
            .get("package")
            .and_then(toml::Value::as_array)
            .expect("Cargo.lock should contain packages");
        let source_suffix = format!("#{CODEX_SCHEMA_REVISION}");
        let protocol_package = packages
            .iter()
            .filter_map(toml::Value::as_table)
            .find(|package| {
                package.get("name").and_then(toml::Value::as_str) == Some("codex-protocol")
                    && package
                        .get("source")
                        .and_then(toml::Value::as_str)
                        .is_some_and(|source| source.ends_with(&source_suffix))
            })
            .expect("Cargo.lock should contain codex-protocol at the schema revision");
        let protocol_version = protocol_package
            .get("version")
            .and_then(toml::Value::as_str)
            .expect("locked codex-protocol should have a version");
        assert_eq!(CODEX_SCHEMA_RELEASE, format!("rust-v{protocol_version}"));
    }
}
