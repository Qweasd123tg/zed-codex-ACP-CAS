//! Локальный кэш last-known context usage для мгновенного восстановления после resume.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use codex_app_server_protocol::Turn as AppTurn;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CachedUsageEntry {
    turn_id: String,
    used_tokens: u64,
    context_window_size: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ContextUsageCacheFile {
    threads: HashMap<String, CachedUsageEntry>,
}

pub(in crate::thread) fn context_usage_cache_path(cas_home: &Path) -> PathBuf {
    cas_home.join("context-usage-cache.json")
}

pub(in crate::thread) fn restore_cached_context_usage(
    cache_path: &Path,
    thread_id: &str,
    turns: &[AppTurn],
) -> Option<(u64, u64)> {
    let last_turn_id = last_turn_id(turns)?;
    let cache = read_context_usage_cache(cache_path).ok()?;
    let entry = cache.threads.get(thread_id)?;
    (entry.turn_id == last_turn_id).then_some((entry.used_tokens, entry.context_window_size))
}

pub(in crate::thread) fn persist_context_usage(
    cache_path: &Path,
    thread_id: &str,
    turn_id: &str,
    used_tokens: u64,
    context_window_size: u64,
) -> std::io::Result<()> {
    let mut cache = read_context_usage_cache(cache_path).unwrap_or_default();
    cache.threads.insert(
        thread_id.to_string(),
        CachedUsageEntry {
            turn_id: turn_id.to_string(),
            used_tokens,
            context_window_size,
        },
    );
    write_context_usage_cache(cache_path, &cache)
}

fn last_turn_id(turns: &[AppTurn]) -> Option<&str> {
    turns.last().map(|turn| turn.id.as_str())
}

fn read_context_usage_cache(cache_path: &Path) -> std::io::Result<ContextUsageCacheFile> {
    match fs::read_to_string(cache_path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(ContextUsageCacheFile::default())
        }
        Err(error) => Err(error),
    }
}

fn write_context_usage_cache(
    cache_path: &Path,
    cache: &ContextUsageCacheFile,
) -> std::io::Result<()> {
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = cache_path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let payload = serde_json::to_vec_pretty(cache)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    fs::write(&tmp_path, payload)?;
    fs::rename(tmp_path, cache_path)
}

#[cfg(test)]
mod tests {
    use super::{context_usage_cache_path, persist_context_usage, restore_cached_context_usage};
    use codex_app_server_protocol::Turn as AppTurn;
    use std::path::Path;

    fn turn(id: &str) -> AppTurn {
        AppTurn {
            id: id.to_string(),
            items: vec![],
            items_view: Default::default(),
            status: codex_app_server_protocol::TurnStatus::Completed,
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
        }
    }

    fn unique_cache_path(test_name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "codex-acp-{test_name}-{}.json",
            uuid::Uuid::new_v4()
        ));
        path
    }

    #[test]
    fn cache_path_uses_cas_home() {
        let path = context_usage_cache_path(Path::new("/tmp/.codex-cas"));
        assert_eq!(
            path,
            Path::new("/tmp/.codex-cas").join("context-usage-cache.json")
        );
    }

    #[test]
    fn persists_and_restores_cached_usage_for_matching_last_turn() {
        let cache_path = unique_cache_path("persist-roundtrip");
        persist_context_usage(&cache_path, "thread-1", "turn-2", 157_835, 258_400).unwrap();

        let restored = restore_cached_context_usage(
            &cache_path,
            "thread-1",
            &[turn("turn-1"), turn("turn-2")],
        );

        assert_eq!(restored, Some((157_835, 258_400)));

        drop(std::fs::remove_file(cache_path));
    }

    #[test]
    fn ignores_cache_when_last_turn_does_not_match() {
        let cache_path = unique_cache_path("turn-mismatch");
        persist_context_usage(&cache_path, "thread-1", "turn-2", 157_835, 258_400).unwrap();

        let restored = restore_cached_context_usage(&cache_path, "thread-1", &[turn("turn-3")]);

        assert_eq!(restored, None);

        drop(std::fs::remove_file(cache_path));
    }
}
