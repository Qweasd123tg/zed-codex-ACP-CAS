//! Утилиты replay для пересборки представления ACP-клиента из ранее отправленных thread-item.

use std::collections::HashMap;
use std::path::Path;

use super::{AppTurn, SessionClient, ThreadItem};
use crate::thread::features::{collab, file, session, tool_events};

// Воспроизводим исторические turn в исходном порядке, чтобы восстановление состояния оставалось детерминированным.
pub(super) async fn replay_turns(
    client: &SessionClient,
    workspace_cwd: &Path,
    agent_labels: &HashMap<String, collab::CollabAgentLabel>,
    turns: Vec<AppTurn>,
) {
    for turn in turns {
        for item in turn.items {
            replay_thread_item(client, workspace_cwd, agent_labels, item).await;
        }
    }
}

async fn replay_thread_item(
    client: &SessionClient,
    workspace_cwd: &Path,
    agent_labels: &HashMap<String, collab::CollabAgentLabel>,
    item: ThreadItem,
) {
    let Some(item) = session::replay_item(client, item).await else {
        return;
    };
    let Some(item) = tool_events::replay_item(client, item).await else {
        return;
    };
    let Some(item) = file::replay_item(client, workspace_cwd, item).await else {
        return;
    };
    let _collab_item = collab::replay_item(client, agent_labels, item).await;
}
