//! Domain rules for the file-change flow:
//! path handling, preview and replay diffs, and edit-approval logic.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use codex_app_server_protocol::{FileUpdateChange, PatchChangeKind};

use crate::thread::{
    Diff, EditApprovalMode, ModeKind, ToolCallLocation, apply_unified_diff_to_text,
    first_hunk_line, unified_diff_to_old_new,
};

pub(in crate::thread) fn should_prompt_file_change_approval(
    collaboration_mode_kind: ModeKind,
    edit_approval_mode: EditApprovalMode,
) -> bool {
    if collaboration_mode_kind == ModeKind::Plan {
        return true;
    }
    matches!(edit_approval_mode, EditApprovalMode::AskEveryEdit)
}

pub(in crate::thread) fn resolve_workspace_path(workspace_cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_cwd.join(path)
    }
}

pub(in crate::thread) fn read_file_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

pub(in crate::thread) fn file_change_target_path(
    workspace_cwd: &Path,
    change: &FileUpdateChange,
) -> PathBuf {
    let source_path = resolve_workspace_path(workspace_cwd, Path::new(&change.path));
    match &change.kind {
        PatchChangeKind::Update {
            move_path: Some(move_path),
        } => resolve_workspace_path(workspace_cwd, move_path),
        _ => source_path,
    }
}

fn file_change_location_line(change: &FileUpdateChange) -> Option<u32> {
    match &change.kind {
        PatchChangeKind::Add => first_hunk_line(&change.diff, true).or(Some(0)),
        PatchChangeKind::Delete => first_hunk_line(&change.diff, false).or(Some(0)),
        PatchChangeKind::Update { .. } => {
            first_hunk_line(&change.diff, true).or_else(|| first_hunk_line(&change.diff, false))
        }
    }
}

pub(in crate::thread) fn file_change_tool_location(
    workspace_cwd: &Path,
    change: &FileUpdateChange,
) -> ToolCallLocation {
    let path = file_change_target_path(workspace_cwd, change);
    let location = ToolCallLocation::new(path);
    if let Some(line) = file_change_location_line(change) {
        location.line(line)
    } else {
        location
    }
}

pub(in crate::thread) fn file_change_to_tool_diff(
    workspace_cwd: &Path,
    before_contents: &HashMap<PathBuf, Option<String>>,
    change: FileUpdateChange,
) -> Option<Diff> {
    let source_path = resolve_workspace_path(workspace_cwd, Path::new(&change.path));
    match change.kind {
        PatchChangeKind::Add => {
            let new_text = read_file_text(&source_path).unwrap_or(change.diff);
            Some(Diff::new(source_path, new_text))
        }
        PatchChangeKind::Delete => {
            let old_text = before_contents.get(&source_path).cloned().flatten().or(
                if change.diff.is_empty() {
                    None
                } else {
                    Some(change.diff)
                },
            );
            Some(Diff::new(source_path, String::new()).old_text(old_text))
        }
        PatchChangeKind::Update { move_path } => {
            let target_path = move_path
                .as_ref()
                .map(|path| resolve_workspace_path(workspace_cwd, path))
                .unwrap_or_else(|| source_path.clone());
            let new_text = read_file_text(&target_path).unwrap_or(change.diff);
            let old_text = before_contents.get(&source_path).cloned().flatten();
            Some(Diff::new(target_path, new_text).old_text(old_text))
        }
    }
}

pub(in crate::thread) fn file_change_to_preview_diff(
    workspace_cwd: &Path,
    before_contents: &HashMap<PathBuf, Option<String>>,
    change: &FileUpdateChange,
) -> Diff {
    let source_path = resolve_workspace_path(workspace_cwd, Path::new(&change.path));
    match &change.kind {
        PatchChangeKind::Add => Diff::new(source_path, change.diff.clone()),
        PatchChangeKind::Delete => {
            let old_text = before_contents.get(&source_path).cloned().flatten().or(
                if change.diff.is_empty() {
                    None
                } else {
                    Some(change.diff.clone())
                },
            );
            Diff::new(source_path, String::new()).old_text(old_text)
        }
        PatchChangeKind::Update { move_path } => {
            let target_path = move_path
                .as_ref()
                .map(|path| resolve_workspace_path(workspace_cwd, path))
                .unwrap_or_else(|| source_path.clone());
            let old_text = before_contents.get(&source_path).cloned().flatten();
            if change.diff.is_empty() {
                let new_text = old_text.as_deref().unwrap_or_default().to_owned();
                return Diff::new(target_path, new_text).old_text(old_text);
            }

            // Prefer exact reconstruction from captured pre-edit content.
            if let Some(existing_old_text) = old_text.as_deref()
                && let Some(new_text) = apply_unified_diff_to_text(existing_old_text, &change.diff)
            {
                return Diff::new(target_path, new_text)
                    .old_text(Some(existing_old_text.to_owned()));
            }

            // Fall back when the old snapshot is missing or incompatible, such as restored history:
            // render both sides directly from the unified-diff hunk.
            if let Some((parsed_old_text, parsed_new_text)) = unified_diff_to_old_new(&change.diff)
            {
                return Diff::new(target_path, parsed_new_text).old_text(Some(parsed_old_text));
            }

            Diff::new(target_path, change.diff.clone()).old_text(old_text)
        }
    }
}

pub(in crate::thread) fn file_change_to_replay_diff(
    workspace_cwd: &Path,
    change: FileUpdateChange,
) -> Diff {
    let source_path = resolve_workspace_path(workspace_cwd, Path::new(&change.path));
    match change.kind {
        PatchChangeKind::Add => {
            // Some historical events store add/delete as a unified hunk instead of raw content.
            // Prefer parsed old/new sides when possible so replay diffs keep rich highlighting.
            if let Some((old_text, new_text)) = unified_diff_to_old_new(&change.diff) {
                let old_text = if old_text.is_empty() {
                    None
                } else {
                    Some(old_text)
                };
                Diff::new(source_path, new_text).old_text(old_text)
            } else {
                Diff::new(source_path, change.diff)
            }
        }
        PatchChangeKind::Delete => {
            if let Some((old_text, new_text)) = unified_diff_to_old_new(&change.diff) {
                let old_text = if old_text.is_empty() {
                    None
                } else {
                    Some(old_text)
                };
                Diff::new(source_path, new_text).old_text(old_text)
            } else {
                let old_text = if change.diff.is_empty() {
                    None
                } else {
                    Some(change.diff)
                };
                Diff::new(source_path, String::new()).old_text(old_text)
            }
        }
        PatchChangeKind::Update { move_path } => {
            let target_path = move_path
                .as_ref()
                .map(|path| resolve_workspace_path(workspace_cwd, path))
                .unwrap_or_else(|| source_path.clone());

            // Replay update events carry only patch text, so reconstruct old/new from unified diff
            // to preserve line-level UI markers after `/resume`.
            if let Some((old_text, new_text)) = unified_diff_to_old_new(&change.diff) {
                Diff::new(target_path, new_text).old_text(Some(old_text))
            } else {
                Diff::new(target_path, change.diff)
            }
        }
    }
}
