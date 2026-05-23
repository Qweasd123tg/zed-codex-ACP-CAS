use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) const CODEX_CAS_HOME_ENV: &str = "CODEX_CAS_HOME";

pub(crate) fn cas_home_from_codex_home(codex_home: &Path) -> PathBuf {
    if let Some(path) = non_empty_env(CODEX_CAS_HOME_ENV) {
        return PathBuf::from(path);
    }

    if let Some(home) = non_empty_env("HOME").or_else(|| non_empty_env("USERPROFILE")) {
        return PathBuf::from(home).join(".codex-cas");
    }

    codex_home
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".codex-cas")
}

pub(crate) fn legacy_codex_acp_home(codex_home: &Path) -> PathBuf {
    codex_home.join("codex-acp")
}

pub(crate) fn legacy_codex_acp_memories_home(codex_home: &Path) -> PathBuf {
    codex_home.join("memories").join("codex-acp")
}

pub(crate) fn migrate_file_if_missing(new_path: &Path, old_path: &Path) -> io::Result<bool> {
    if new_path.exists() || !old_path.exists() {
        return Ok(false);
    }

    if let Some(parent) = new_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::copy(old_path, new_path)?;
    Ok(true)
}

fn non_empty_env(key: &str) -> Option<OsString> {
    std::env::var_os(key).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{legacy_codex_acp_home, legacy_codex_acp_memories_home, migrate_file_if_missing};
    use std::fs;
    use std::path::Path;

    #[test]
    fn legacy_paths_stay_under_codex_home() {
        let codex_home = Path::new("/tmp/.codex");

        assert_eq!(
            legacy_codex_acp_home(codex_home),
            Path::new("/tmp/.codex/codex-acp")
        );
        assert_eq!(
            legacy_codex_acp_memories_home(codex_home),
            Path::new("/tmp/.codex/memories/codex-acp")
        );
    }

    #[test]
    fn migrate_file_if_missing_copies_without_overwriting() {
        let root =
            std::env::temp_dir().join(format!("codex-cas-migration-{}", uuid::Uuid::new_v4()));
        let old_path = root.join("old").join("selector-preferences.json");
        let new_path = root.join("new").join("selector-preferences.json");

        fs::create_dir_all(old_path.parent().unwrap()).unwrap();
        fs::write(&old_path, "legacy").unwrap();

        assert!(migrate_file_if_missing(&new_path, &old_path).unwrap());
        assert_eq!(fs::read_to_string(&new_path).unwrap(), "legacy");

        fs::write(&old_path, "changed legacy").unwrap();
        fs::write(&new_path, "current").unwrap();

        assert!(!migrate_file_if_missing(&new_path, &old_path).unwrap());
        assert_eq!(fs::read_to_string(&new_path).unwrap(), "current");

        drop(fs::remove_dir_all(root));
    }
}
