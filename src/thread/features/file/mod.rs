//! File-related feature slice: preview and replay diffs plus file-change card lifecycle.

use std::path::Path;

use crate::thread::{SessionClient, ThreadInner, ThreadItem};

pub(in crate::thread) mod changes;
pub(in crate::thread) mod events;

// Started-item router for the file-change branch.
pub(in crate::thread) async fn handle_item_started(
    inner: &mut ThreadInner,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::FileChange {
            id,
            changes,
            status,
        } => {
            events::emit_file_change_started(inner, id, changes, status).await;
            None
        }
        _ => Some(item),
    }
}

// Completed-item router for the file-change branch.
pub(in crate::thread) async fn handle_item_completed(
    inner: &mut ThreadInner,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::FileChange {
            id,
            changes,
            status,
        } => {
            events::emit_file_change_completed(inner, id, changes, status).await;
            None
        }
        _ => Some(item),
    }
}

// Replay-item router for the file-change branch.
pub(in crate::thread) async fn replay_item(
    client: &SessionClient,
    workspace_cwd: &Path,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::FileChange {
            id,
            changes,
            status,
        } => {
            events::replay_file_change(client, workspace_cwd, id, changes, status).await;
            None
        }
        _ => Some(item),
    }
}
