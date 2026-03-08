//! Replay helpers for rebuilding ACP client state from previously emitted thread items.

use std::path::Path;

use super::{AppTurn, SessionClient, ThreadItem};
use crate::thread::features::{collab, file, session, tool_events};

// Replay historical turns in source order so state restoration stays deterministic.
pub(super) async fn replay_turns(
    client: &SessionClient,
    workspace_cwd: &Path,
    turns: Vec<AppTurn>,
) {
    for turn in turns {
        for item in turn.items {
            replay_thread_item(client, workspace_cwd, item).await;
        }
    }
}

async fn replay_thread_item(client: &SessionClient, workspace_cwd: &Path, item: ThreadItem) {
    let Some(item) = session::replay_item(client, item).await else {
        return;
    };
    let Some(item) = tool_events::replay_item(client, item).await else {
        return;
    };
    let Some(item) = file::replay_item(client, workspace_cwd, item).await else {
        return;
    };
    let _collab_item = collab::replay_item(client, item).await;
}
