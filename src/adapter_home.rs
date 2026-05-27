use std::ffi::OsString;
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

fn non_empty_env(key: &str) -> Option<OsString> {
    std::env::var_os(key).filter(|value| !value.is_empty())
}
