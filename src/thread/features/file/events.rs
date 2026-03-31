//! Live/replay рендер file-change tool-call веток.
//! Держит полную lifecycle-логику карточек правок отдельно от item-маршрутизации.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use agent_client_protocol::{
    ToolCall, ToolCallContent, ToolCallId, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use codex_app_server_protocol::{FileUpdateChange, PatchApplyStatus, PatchChangeKind};
use tracing::warn;

use crate::thread::features::status_mapping;
use crate::thread::{SessionClient, ThreadInner};

pub(in crate::thread) struct FileChangeStartedSnapshot {
    pub(in crate::thread) client: SessionClient,
    pub(in crate::thread) id: String,
    pub(in crate::thread) status: PatchApplyStatus,
    pub(in crate::thread) locations: Vec<agent_client_protocol::ToolCallLocation>,
    pub(in crate::thread) preview_content: Vec<ToolCallContent>,
    pub(in crate::thread) title: String,
    pub(in crate::thread) prime_paths: Vec<PathBuf>,
}

pub(in crate::thread) struct FileChangeCompletedSnapshot {
    pub(in crate::thread) client: SessionClient,
    pub(in crate::thread) id: String,
    pub(in crate::thread) status: PatchApplyStatus,
    pub(in crate::thread) workspace_cwd: PathBuf,
    pub(in crate::thread) changes: Vec<FileUpdateChange>,
    pub(in crate::thread) before_contents: HashMap<PathBuf, Option<String>>,
    pub(in crate::thread) writeback_paths: Vec<PathBuf>,
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
    let title = if changes.is_empty() {
        "Apply edits".to_string()
    } else {
        format!(
            "Edit {}",
            changes
                .iter()
                .map(|c| c.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    let mut prime_paths = Vec::new();
    if inner.client.supports_read_text_file() {
        let mut primed_paths = HashSet::new();
        for change in &changes {
            let source_path = super::changes::resolve_workspace_path(
                &inner.workspace_cwd,
                Path::new(&change.path),
            );
            if primed_paths.insert(source_path.clone()) {
                prime_paths.push(source_path.clone());
            }

            if let PatchChangeKind::Update {
                move_path: Some(move_path),
            } = &change.kind
            {
                let target_path =
                    super::changes::resolve_workspace_path(&inner.workspace_cwd, move_path);
                if target_path != source_path && primed_paths.insert(target_path.clone()) {
                    prime_paths.push(target_path);
                }
            }
        }
    }

    FileChangeStartedSnapshot {
        client: inner.client.clone(),
        id,
        status,
        locations,
        preview_content,
        title,
        prime_paths,
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

    for path in snapshot.prime_paths {
        if let Err(err) = snapshot.client.prime_file_snapshot(path.clone()).await {
            warn!(
                "Failed to prime ACP snapshot for {}: {err:?}",
                path.display()
            );
        }
    }
}

// Публикуем старт file-change: превью, locations и pre-edit snapshot для корректного writeback.
pub(in crate::thread) async fn emit_file_change_started(
    inner: &mut ThreadInner,
    id: String,
    changes: Vec<FileUpdateChange>,
    status: PatchApplyStatus,
) {
    let snapshot = prepare_file_change_started_snapshot(inner, id, changes, status);
    emit_file_change_started_snapshot(snapshot).await;
}

fn collect_file_change_writeback_paths(
    workspace_cwd: &Path,
    changes: &[FileUpdateChange],
) -> Vec<PathBuf> {
    let mut writeback_paths = Vec::new();
    let mut seen_writeback_paths = HashSet::new();
    for change in changes {
        if matches!(change.kind, PatchChangeKind::Delete) {
            continue;
        }
        let path = super::changes::file_change_target_path(workspace_cwd, change);
        if seen_writeback_paths.insert(path.clone()) {
            writeback_paths.push(path);
        }
    }
    writeback_paths
}

pub(in crate::thread) fn prepare_file_change_completed_snapshot(
    inner: &mut ThreadInner,
    id: String,
    changes: Vec<FileUpdateChange>,
    status: PatchApplyStatus,
) -> FileChangeCompletedSnapshot {
    let workspace_cwd = inner.workspace_cwd.clone();
    let writeback_paths = if matches!(status, PatchApplyStatus::Completed)
        && inner.client.supports_write_text_file()
    {
        collect_file_change_writeback_paths(&workspace_cwd, &changes)
    } else {
        Vec::new()
    };
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
        workspace_cwd,
        changes,
        before_contents,
        writeback_paths,
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
            .status(status_mapping::map_patch_status(
                snapshot.status.clone(),
                false,
            ))
            .content(content),
    )
}

pub(in crate::thread) fn collect_file_change_writeback_targets(
    snapshot: &FileChangeCompletedSnapshot,
) -> Vec<(PathBuf, String)> {
    snapshot
        .writeback_paths
        .iter()
        .filter_map(|path| {
            super::changes::read_file_text(path).map(|content| (path.clone(), content))
        })
        .collect()
}

// Публикуем завершение file-change: финальный diff и writeback в ACP text-buffer.
pub(in crate::thread) async fn emit_file_change_completed(
    inner: &mut ThreadInner,
    id: String,
    changes: Vec<FileUpdateChange>,
    status: PatchApplyStatus,
) {
    let snapshot = prepare_file_change_completed_snapshot(inner, id, changes, status);
    let tool_call_update = build_file_change_completed_update(&snapshot);
    let writeback_targets = collect_file_change_writeback_targets(&snapshot);

    snapshot
        .client
        .send_tool_call_update(tool_call_update)
        .await;

    for (path, content) in writeback_targets {
        match snapshot.client.write_text_file(path.clone(), content).await {
            Ok(()) => {
                inner.synced_paths_this_turn.insert(path);
            }
            Err(err) => {
                warn!(
                    "Failed to sync file change into ACP buffer for {}: {err:?}",
                    path.display()
                );
            }
        }
    }
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
