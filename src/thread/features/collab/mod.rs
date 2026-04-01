//! Фасад collaboration/sub-agent feature.
//! Публичный API для thread-слоя остаётся стабильным, реализация разбита на status/content/render.

use std::collections::{HashMap, HashSet};

use crate::thread::{AppTurn, SessionClient, ThreadInner, ThreadItem, ThreadReadParams};
use codex_app_server_protocol::{CollabAgentState, CollabAgentTool, CollabAgentToolCallStatus};

#[path = "content.rs"]
pub(in crate::thread) mod content;
#[path = "render.rs"]
pub(in crate::thread) mod render;
#[path = "status.rs"]
pub(in crate::thread) mod status;

#[derive(Debug, Clone)]
// Пакет данных collab tool-call, чтобы не раздувать сигнатуры в фасаде и рендере.
pub(in crate::thread) struct CollabToolCallData {
    pub(in crate::thread) id: String,
    pub(in crate::thread) tool: CollabAgentTool,
    pub(in crate::thread) status: CollabAgentToolCallStatus,
    pub(in crate::thread) sender_thread_id: String,
    pub(in crate::thread) receiver_thread_ids: Vec<String>,
    pub(in crate::thread) prompt: Option<String>,
    pub(in crate::thread) agents_states: HashMap<String, CollabAgentState>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::thread) struct CollabAgentLabel {
    pub(in crate::thread) nickname: Option<String>,
    pub(in crate::thread) role: Option<String>,
}

pub(in crate::thread) fn remember_agent_label(
    labels: &mut HashMap<String, CollabAgentLabel>,
    thread_id: String,
    nickname: Option<String>,
    role: Option<String>,
) {
    labels.insert(thread_id, CollabAgentLabel { nickname, role });
}

pub(in crate::thread) async fn warm_agent_labels_for_turns(
    inner: &mut ThreadInner,
    turns: &[AppTurn],
) {
    let mut thread_ids = HashSet::new();
    for turn in turns {
        for item in &turn.items {
            if let ThreadItem::CollabAgentToolCall {
                sender_thread_id,
                receiver_thread_ids,
                ..
            } = item
            {
                thread_ids.insert(sender_thread_id.clone());
                thread_ids.extend(receiver_thread_ids.iter().cloned());
            }
        }
    }

    for thread_id in thread_ids {
        ensure_agent_label(inner, &thread_id).await;
    }
}

async fn ensure_agent_label(inner: &mut ThreadInner, thread_id: &str) {
    if inner.agent_labels.contains_key(thread_id) || thread_id.is_empty() {
        return;
    }

    match inner
        .app
        .lock()
        .await
        .thread_read(ThreadReadParams {
            thread_id: thread_id.to_string(),
            include_turns: false,
        })
        .await
    {
        Ok(response) => remember_agent_label(
            &mut inner.agent_labels,
            response.thread.id,
            response.thread.agent_nickname,
            response.thread.agent_role,
        ),
        Err(_) => {
            inner
                .agent_labels
                .insert(thread_id.to_string(), CollabAgentLabel::default());
        }
    }
}

async fn ensure_agent_labels_for_item(
    inner: &mut ThreadInner,
    sender_thread_id: &str,
    receiver_thread_ids: &[String],
) {
    ensure_agent_label(inner, sender_thread_id).await;
    for receiver_thread_id in receiver_thread_ids {
        ensure_agent_label(inner, receiver_thread_id).await;
    }
}

// Роутер started-item для collab/subagents карточек.
pub(in crate::thread) async fn handle_item_started(
    inner: &mut ThreadInner,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::CollabAgentToolCall {
            id,
            tool,
            status,
            sender_thread_id,
            receiver_thread_ids,
            prompt,
            agents_states,
        } => {
            ensure_agent_labels_for_item(inner, &sender_thread_id, &receiver_thread_ids).await;
            render::emit_collab_tool_call_started(
                inner,
                CollabToolCallData {
                    id,
                    tool,
                    status,
                    sender_thread_id,
                    receiver_thread_ids,
                    prompt,
                    agents_states,
                },
            )
            .await;
            None
        }
        _ => Some(item),
    }
}

// Роутер completed-item для collab/subagents карточек.
pub(in crate::thread) async fn handle_item_completed(
    inner: &mut ThreadInner,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::CollabAgentToolCall {
            id,
            tool,
            status,
            sender_thread_id,
            receiver_thread_ids,
            prompt,
            agents_states,
        } => {
            ensure_agent_labels_for_item(inner, &sender_thread_id, &receiver_thread_ids).await;
            render::emit_collab_tool_call_completed(
                inner,
                CollabToolCallData {
                    id,
                    tool,
                    status,
                    sender_thread_id,
                    receiver_thread_ids,
                    prompt,
                    agents_states,
                },
            )
            .await;
            None
        }
        _ => Some(item),
    }
}

// Роутер replay-item для collab/subagents карточек.
pub(in crate::thread) async fn replay_item(
    client: &SessionClient,
    agent_labels: &HashMap<String, CollabAgentLabel>,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::CollabAgentToolCall {
            id,
            tool,
            status,
            sender_thread_id,
            receiver_thread_ids,
            prompt,
            agents_states,
        } => {
            render::replay_collab_tool_call(
                client,
                agent_labels,
                CollabToolCallData {
                    id,
                    tool,
                    status,
                    sender_thread_id,
                    receiver_thread_ids,
                    prompt,
                    agents_states,
                },
            )
            .await;
            None
        }
        _ => Some(item),
    }
}
