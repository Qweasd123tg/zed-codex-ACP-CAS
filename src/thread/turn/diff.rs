//! Turn-diff tracking helpers: parse/apply unified diff and publish change summaries to ACP.

use std::path::{Path, PathBuf};

use tracing::warn;

use crate::thread::{
    DEV_NULL, Diff, TURN_DIFF_TOOL_CALL_PREFIX, ThreadInner, ToolCall, ToolCallContent, ToolCallId,
    ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
    TurnDiffUpdatedNotification, read_file_text, unified_diff_to_old_new,
};

#[derive(Clone, Debug)]
pub(super) struct TurnUnifiedDiffFile {
    pub(super) path: PathBuf,
    pub(super) old_text: String,
    pub(super) new_text: String,
    pub(super) is_delete: bool,
}

#[derive(Clone, Debug)]
struct ResolvedTurnDiffFile {
    path: PathBuf,
    old_text: String,
    new_text: String,
}

// Handle turn-diff updates in one place so patch-preview behavior stays consistent.
pub(super) async fn handle_turn_diff_updated(
    inner: &mut ThreadInner,
    payload: TurnDiffUpdatedNotification,
    expected_turn_id: &str,
) {
    if payload.turn_id != expected_turn_id {
        return;
    }

    inner.latest_turn_diff = Some(payload.diff);
}

pub(super) async fn finalize_turn_diff(inner: &mut ThreadInner, turn_id: &str) {
    let Some(diff) = inner.latest_turn_diff.take() else {
        return;
    };

    let parsed_files = parse_turn_unified_diff_files(&diff);
    if parsed_files.is_empty() {
        return;
    }
    let repo_root = find_repo_root(&inner.workspace_cwd);
    let mut resolved_files = Vec::with_capacity(parsed_files.len());
    let mut sync_paths = Vec::new();
    for file in parsed_files {
        let path = resolve_turn_diff_path(&inner.workspace_cwd, repo_root.as_deref(), &file.path);
        if !file.is_delete {
            sync_paths.push(path.clone());
        }
        resolved_files.push(ResolvedTurnDiffFile {
            path,
            old_text: file.old_text,
            new_text: file.new_text,
        });
    }
    if resolved_files.is_empty() {
        return;
    }

    update_turn_diff_tool_call(inner, turn_id, resolved_files, false).await;
    sync_turn_diff_files_to_acp(inner, &sync_paths).await;
}

async fn update_turn_diff_tool_call(
    inner: &mut ThreadInner,
    turn_id: &str,
    resolved_files: Vec<ResolvedTurnDiffFile>,
    in_progress: bool,
) {
    let tool_call_key = format!("{TURN_DIFF_TOOL_CALL_PREFIX}{turn_id}");
    let tool_call_id = ToolCallId::new(tool_call_key.clone());
    let status = if in_progress {
        ToolCallStatus::InProgress
    } else {
        ToolCallStatus::Completed
    };

    let mut content = Vec::new();
    let mut locations = Vec::new();
    for file in resolved_files {
        if inner.file_change_paths_this_turn.contains(&file.path) {
            continue;
        }

        let old_text = if file.old_text.is_empty() {
            None
        } else {
            Some(file.old_text)
        };
        let path = file.path.clone();
        content.push(ToolCallContent::Diff(
            Diff::new(path.clone(), file.new_text).old_text(old_text),
        ));
        locations.push(ToolCallLocation::new(path));
    }
    if content.is_empty() {
        return;
    }

    if inner.started_tool_calls.insert(tool_call_key.clone()) {
        inner
            .client
            .send_tool_call(
                ToolCall::new(tool_call_id, "Turn diff")
                    .kind(ToolKind::Edit)
                    .status(status)
                    .locations(locations)
                    .content(content),
            )
            .await;
    } else {
        inner
            .client
            .send_tool_call_update(ToolCallUpdate::new(
                tool_call_id,
                ToolCallUpdateFields::new()
                    .status(status)
                    .locations(locations)
                    .content(content),
            ))
            .await;
    }

    if !in_progress {
        inner.started_tool_calls.remove(&tool_call_key);
    }
}

async fn sync_turn_diff_files_to_acp(inner: &mut ThreadInner, sync_paths: &[PathBuf]) {
    if !inner.client.supports_write_text_file() {
        return;
    }

    for path in sync_paths {
        if inner.synced_paths_this_turn.contains(path.as_path()) {
            continue;
        }

        let Some(content) = read_file_text(path) else {
            continue;
        };

        match inner.client.write_text_file(path.clone(), content).await {
            Ok(()) => {
                inner.synced_paths_this_turn.insert(path.clone());
            }
            Err(err) => {
                warn!(
                    "Failed to sync turn diff into ACP buffer for {}: {err:?}",
                    path.display()
                );
            }
        }
    }
}

pub(super) fn parse_turn_unified_diff_files(unified_diff: &str) -> Vec<TurnUnifiedDiffFile> {
    fn finalize_section(
        section: &mut String,
        old_path: &mut Option<String>,
        new_path: &mut Option<String>,
        output: &mut Vec<TurnUnifiedDiffFile>,
    ) {
        if section.trim().is_empty() {
            section.clear();
            *old_path = None;
            *new_path = None;
            return;
        }

        let old = old_path.take();
        let new = new_path.take();
        let new_is_dev_null = new.as_deref().is_some_and(|path| path.trim() == DEV_NULL);
        let chosen_path = if new_is_dev_null { old } else { new.or(old) };
        let Some(path) = chosen_path else {
            section.clear();
            return;
        };

        let normalized = normalize_unified_diff_path(&path);
        if normalized.is_empty() {
            section.clear();
            return;
        }
        if !section.contains("@@") {
            section.clear();
            return;
        }

        let Some((old_text, new_text)) = unified_diff_to_old_new(section) else {
            section.clear();
            return;
        };
        if old_text == new_text {
            section.clear();
            return;
        }

        output.push(TurnUnifiedDiffFile {
            path: PathBuf::from(normalized),
            old_text,
            new_text,
            is_delete: new_is_dev_null,
        });
        section.clear();
    }

    let mut files = Vec::new();
    let mut section = String::with_capacity(unified_diff.len().min(8192));
    let mut old_path: Option<String> = None;
    let mut new_path: Option<String> = None;
    let mut saw_file_header = false;

    for raw_line in unified_diff.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);

        if line.starts_with("diff --git ") {
            finalize_section(&mut section, &mut old_path, &mut new_path, &mut files);
            saw_file_header = true;
        }

        if let Some(path) = line.strip_prefix("--- ") {
            old_path = Some(path.trim().to_string());
        } else if let Some(path) = line.strip_prefix("+++ ") {
            new_path = Some(path.trim().to_string());
        }

        if saw_file_header
            || !section.is_empty()
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
        {
            section.push_str(raw_line);
        }
    }

    finalize_section(&mut section, &mut old_path, &mut new_path, &mut files);
    files
}

fn normalize_unified_diff_path(path: &str) -> String {
    let trimmed = path.trim().trim_matches('"');
    if trimmed == DEV_NULL {
        return String::new();
    }
    trimmed
        .strip_prefix("a/")
        .or_else(|| trimmed.strip_prefix("b/"))
        .unwrap_or(trimmed)
        .to_string()
}

fn resolve_turn_diff_path(workspace_cwd: &Path, repo_root: Option<&Path>, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }

    let direct = workspace_cwd.join(path);
    if direct.exists() {
        return direct;
    }

    if let Some(repo_root) = repo_root {
        let candidate = repo_root.join(path);
        if candidate.exists() {
            return candidate;
        }
    }

    direct
}

fn find_repo_root(workspace_cwd: &Path) -> Option<PathBuf> {
    workspace_cwd
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}
