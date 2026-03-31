//! Live/replay рендер file-change tool-call веток.
//! Держит полную lifecycle-логику карточек правок отдельно от item-маршрутизации.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use agent_client_protocol::{
    ToolCall, ToolCallContent, ToolCallId, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use codex_app_server_protocol::{FileUpdateChange, PatchApplyStatus, PatchChangeKind};
use tracing::warn;

use crate::thread::features::status_mapping;
use crate::thread::{SessionClient, ThreadInner};

// Публикуем старт file-change: превью, locations и pre-edit snapshot для корректного writeback.
pub(in crate::thread) async fn emit_file_change_started(
    inner: &mut ThreadInner,
    id: String,
    changes: Vec<FileUpdateChange>,
    status: PatchApplyStatus,
) {
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
    inner.file_change_locations.insert(id.clone(), target_paths);
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

    if inner.client.supports_read_text_file() {
        let mut primed_paths = HashSet::new();
        for change in &changes {
            // Подготавливаем snapshot общего буфера Zed до подтверждения/применения патча.
            // Это помогает последующему write_text_file формировать реальные построчные правки для маркеров.
            let source_path = super::changes::resolve_workspace_path(
                &inner.workspace_cwd,
                Path::new(&change.path),
            );
            if primed_paths.insert(source_path.clone())
                && let Err(err) = inner.client.prime_file_snapshot(source_path.clone()).await
            {
                warn!(
                    "Failed to prime ACP snapshot for {}: {err:?}",
                    source_path.display()
                );
            }

            if let PatchChangeKind::Update {
                move_path: Some(move_path),
            } = &change.kind
            {
                let target_path =
                    super::changes::resolve_workspace_path(&inner.workspace_cwd, move_path);
                if target_path != source_path
                    && primed_paths.insert(target_path.clone())
                    && let Err(err) = inner.client.prime_file_snapshot(target_path.clone()).await
                {
                    warn!(
                        "Failed to prime ACP snapshot for {}: {err:?}",
                        target_path.display()
                    );
                }
            }
        }
    }
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
    inner
        .file_change_before_contents
        .insert(id.clone(), before_contents);
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

    inner
        .client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id), title)
                .kind(ToolKind::Edit)
                .status(status_mapping::map_patch_status(status, true))
                .locations(locations)
                .content(preview_content),
        )
        .await;
}

// Публикуем завершение file-change: финальный diff и writeback в ACP text-buffer.
pub(in crate::thread) async fn emit_file_change_completed(
    inner: &mut ThreadInner,
    id: String,
    changes: Vec<FileUpdateChange>,
    status: PatchApplyStatus,
) {
    let mut writeback_targets = Vec::new();
    if matches!(status, PatchApplyStatus::Completed) && inner.client.supports_write_text_file() {
        let mut seen_writeback_paths = HashSet::new();
        for change in &changes {
            if matches!(change.kind, PatchChangeKind::Delete) {
                continue;
            }
            let path = super::changes::file_change_target_path(&inner.workspace_cwd, change);
            if !seen_writeback_paths.insert(path.clone()) {
                continue;
            }
            if let Some(content) = super::changes::read_file_text(&path) {
                writeback_targets.push((path, content));
            }
        }
    }

    let before_contents = inner
        .file_change_before_contents
        .remove(&id)
        .unwrap_or_default();
    let content = changes
        .into_iter()
        .filter_map(|change| {
            super::changes::file_change_to_tool_diff(&inner.workspace_cwd, &before_contents, change)
        })
        .map(ToolCallContent::Diff)
        .collect::<Vec<_>>();

    inner
        .client
        .send_tool_call_update(ToolCallUpdate::new(
            ToolCallId::new(id.clone()),
            ToolCallUpdateFields::new()
                .status(status_mapping::map_patch_status(status, false))
                .content(content),
        ))
        .await;

    for (path, content) in writeback_targets {
        match inner.client.write_text_file(path.clone(), content).await {
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

    inner.started_tool_calls.remove(&id);
    inner.file_change_locations.remove(&id);
    inner.file_change_started_changes.remove(&id);
    inner.file_change_before_contents.remove(&id);
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
