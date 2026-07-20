//! Live/replay рендер file-change tool-call веток.
//! Держит полную lifecycle-логику карточек правок отдельно от item-маршрутизации.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::{
    ToolCall, ToolCallContent, ToolCallId, ToolCallLocation, ToolCallUpdate, ToolCallUpdateFields,
    ToolKind,
};
use codex_app_server_protocol::{FileUpdateChange, PatchApplyStatus, PatchChangeKind};

use crate::thread::features::status_mapping;
use crate::thread::{SessionClient, ThreadInner};

pub(in crate::thread) struct FileChangeStartedSnapshot {
    pub(in crate::thread) client: SessionClient,
    pub(in crate::thread) id: String,
    pub(in crate::thread) status: PatchApplyStatus,
    pub(in crate::thread) locations: Vec<ToolCallLocation>,
    pub(in crate::thread) preview_content: Vec<ToolCallContent>,
    pub(in crate::thread) title: String,
}

pub(in crate::thread) struct FileChangeCompletedSnapshot {
    pub(in crate::thread) client: SessionClient,
    pub(in crate::thread) id: String,
    pub(in crate::thread) status: PatchApplyStatus,
    pub(in crate::thread) title: String,
    pub(in crate::thread) locations: Vec<ToolCallLocation>,
    pub(in crate::thread) workspace_cwd: PathBuf,
    pub(in crate::thread) changes: Vec<FileUpdateChange>,
    pub(in crate::thread) before_contents: HashMap<PathBuf, Option<String>>,
}

pub(in crate::thread) fn prepare_file_change_started_snapshot(
    inner: &mut ThreadInner,
    id: String,
    changes: Vec<FileUpdateChange>,
    status: PatchApplyStatus,
) -> FileChangeStartedSnapshot {
    inner.started_tool_calls.insert(id.clone());
    inner
        .file_change_started_changes
        .insert(id.clone(), changes.clone());
    let locations = changes
        .iter()
        .map(|change| super::changes::file_change_tool_location(&inner.workspace_cwd, change))
        .collect();
    let mut target_paths = Vec::new();
    let mut seen_target_paths = HashSet::new();
    for change in &changes {
        let path = super::changes::file_change_target_path(&inner.workspace_cwd, change);
        if seen_target_paths.insert(path.clone()) {
            target_paths.push(path);
        }
    }
    inner
        .file_change_paths_this_turn
        .extend(target_paths.iter().cloned());
    inner
        .file_change_locations
        .insert(id.clone(), target_paths.clone());

    let mut before_contents = HashMap::new();
    for change in &changes {
        if matches!(change.kind, PatchChangeKind::Add) {
            continue;
        }
        let path =
            super::changes::resolve_workspace_path(&inner.workspace_cwd, Path::new(&change.path));
        before_contents
            .entry(path.clone())
            .or_insert_with(|| super::changes::read_file_text(&path));
    }
    inner
        .file_change_before_contents
        .insert(id.clone(), before_contents.clone());

    let preview_content = changes
        .iter()
        .map(|change| {
            ToolCallContent::Diff(super::changes::file_change_to_preview_diff(
                &inner.workspace_cwd,
                &before_contents,
                change,
            ))
        })
        .collect::<Vec<_>>();
    let title = file_change_tool_title(&inner.workspace_cwd, &changes);

    FileChangeStartedSnapshot {
        client: inner.client.clone(),
        id,
        status,
        locations,
        preview_content,
        title,
    }
}

pub(in crate::thread) async fn emit_file_change_started_snapshot(
    snapshot: FileChangeStartedSnapshot,
) {
    snapshot
        .client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(snapshot.id), snapshot.title)
                .kind(ToolKind::Edit)
                .status(status_mapping::map_patch_status(snapshot.status, true))
                .locations(snapshot.locations)
                .content(snapshot.preview_content),
        )
        .await;
}

// Публикуем старт file-change: превью и locations.
pub(in crate::thread) async fn emit_file_change_started(
    inner: &mut ThreadInner,
    id: String,
    changes: Vec<FileUpdateChange>,
    status: PatchApplyStatus,
) {
    let snapshot = prepare_file_change_started_snapshot(inner, id, changes, status);
    emit_file_change_started_snapshot(snapshot).await;
}

pub(in crate::thread) fn prepare_file_change_completed_snapshot(
    inner: &mut ThreadInner,
    id: String,
    changes: Vec<FileUpdateChange>,
    status: PatchApplyStatus,
) -> FileChangeCompletedSnapshot {
    let workspace_cwd = inner.workspace_cwd.clone();
    let title = file_change_tool_title(&workspace_cwd, &changes);
    let locations = changes
        .iter()
        .map(|change| super::changes::file_change_tool_location(&workspace_cwd, change))
        .collect();
    let before_contents = inner
        .file_change_before_contents
        .remove(&id)
        .unwrap_or_default();

    inner.started_tool_calls.remove(&id);
    inner.file_change_locations.remove(&id);
    inner.file_change_started_changes.remove(&id);

    FileChangeCompletedSnapshot {
        client: inner.client.clone(),
        id,
        status,
        title,
        locations,
        workspace_cwd,
        changes,
        before_contents,
    }
}

pub(in crate::thread) fn build_file_change_completed_update(
    snapshot: &FileChangeCompletedSnapshot,
) -> ToolCallUpdate {
    let content = snapshot
        .changes
        .iter()
        .cloned()
        .filter_map(|change| {
            super::changes::file_change_to_tool_diff(
                &snapshot.workspace_cwd,
                &snapshot.before_contents,
                change,
            )
        })
        .map(ToolCallContent::Diff)
        .collect::<Vec<_>>();

    ToolCallUpdate::new(
        ToolCallId::new(snapshot.id.clone()),
        ToolCallUpdateFields::new()
            .title(snapshot.title.clone())
            .status(status_mapping::map_patch_status(
                snapshot.status.clone(),
                false,
            ))
            .locations(snapshot.locations.clone())
            .content(content),
    )
}

fn file_change_tool_title(workspace_cwd: &Path, changes: &[FileUpdateChange]) -> String {
    let paths = unique_display_paths(workspace_cwd, changes);
    match paths.as_slice() {
        [] => "Apply edits".to_string(),
        [path] => format!("Edit {path}"),
        [first, second] => format!("Edit {first}, {second}"),
        _ => format!("Edit {} files", paths.len()),
    }
}

fn unique_display_paths(workspace_cwd: &Path, changes: &[FileUpdateChange]) -> Vec<String> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for change in changes {
        let path = super::changes::file_change_target_path(workspace_cwd, change);
        if !seen.insert(path.clone()) {
            continue;
        }
        paths.push(display_path(workspace_cwd, &path));
    }
    paths
}

fn display_path(workspace_cwd: &Path, path: &Path) -> String {
    let display_path = path.strip_prefix(workspace_cwd).unwrap_or(path);
    display_path.display().to_string()
}

// Публикуем завершение file-change: финальный diff без повторной full-buffer записи в редактор.
pub(in crate::thread) async fn emit_file_change_completed(
    inner: &mut ThreadInner,
    id: String,
    changes: Vec<FileUpdateChange>,
    status: PatchApplyStatus,
) {
    let snapshot = prepare_file_change_completed_snapshot(inner, id, changes, status);
    let tool_call_update = build_file_change_completed_update(&snapshot);

    snapshot
        .client
        .send_tool_call_update(tool_call_update)
        .await;
}

// Replay-рендер file-change карточки с восстановленным unified-diff.
pub(in crate::thread) async fn replay_file_change(
    client: &SessionClient,
    workspace_cwd: &Path,
    id: String,
    changes: Vec<FileUpdateChange>,
    status: PatchApplyStatus,
) {
    let locations = changes
        .iter()
        .map(|change| super::changes::file_change_tool_location(workspace_cwd, change))
        .collect::<Vec<_>>();
    let content = changes
        .into_iter()
        .map(|change| {
            ToolCallContent::Diff(super::changes::file_change_to_replay_diff(
                workspace_cwd,
                change,
            ))
        })
        .collect::<Vec<_>>();

    client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id), "Apply edits")
                .kind(ToolKind::Edit)
                .status(status_mapping::map_patch_status(status, false))
                .locations(locations)
                .content(content),
        )
        .await;
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use codex_app_server_protocol::{FileUpdateChange, PatchChangeKind};

    use super::file_change_tool_title;

    fn update(path: &str) -> FileUpdateChange {
        FileUpdateChange {
            path: path.to_string(),
            kind: PatchChangeKind::Update { move_path: None },
            diff: String::new(),
        }
    }

    #[test]
    fn file_change_title_uses_relative_paths_for_small_groups() {
        let workspace = Path::new("/home/me/work");
        let changes = vec![
            update("/home/me/work/Cargo.toml"),
            update("/home/me/work/README.md"),
        ];

        assert_eq!(
            file_change_tool_title(workspace, &changes),
            "Edit Cargo.toml, README.md"
        );
    }

    #[test]
    fn file_change_title_summarizes_large_groups() {
        let workspace = Path::new("/home/me/work");
        let changes = vec![
            update("Cargo.toml"),
            update("Cargo.lock"),
            FileUpdateChange {
                path: "src/old.rs".to_string(),
                kind: PatchChangeKind::Update {
                    move_path: Some(PathBuf::from("src/new.rs")),
                },
                diff: String::new(),
            },
        ];

        assert_eq!(file_change_tool_title(workspace, &changes), "Edit 3 files");
    }
}
