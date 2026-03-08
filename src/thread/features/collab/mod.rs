//! Facade for collaboration and sub-agent support.
//! The public API exposed to the thread layer stays stable while implementation is split
//! across status, content, and render modules.

use std::collections::HashMap;

use crate::thread::{SessionClient, ThreadInner, ThreadItem};
use codex_app_server_protocol::{CollabAgentState, CollabAgentTool, CollabAgentToolCallStatus};

#[path = "content.rs"]
pub(in crate::thread) mod content;
#[path = "render.rs"]
pub(in crate::thread) mod render;
#[path = "status.rs"]
pub(in crate::thread) mod status;

#[derive(Debug, Clone)]
// Collab tool-call payload used to keep facade and renderer signatures narrow.
pub(in crate::thread) struct CollabToolCallData {
    pub(in crate::thread) id: String,
    pub(in crate::thread) tool: CollabAgentTool,
    pub(in crate::thread) status: CollabAgentToolCallStatus,
    pub(in crate::thread) sender_thread_id: String,
    pub(in crate::thread) receiver_thread_ids: Vec<String>,
    pub(in crate::thread) prompt: Option<String>,
    pub(in crate::thread) agents_states: HashMap<String, CollabAgentState>,
}

// Started-item router for collab and sub-agent cards.
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

// Completed-item router for collab and sub-agent cards.
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

// Replay-item router for collab and sub-agent cards.
pub(in crate::thread) async fn replay_item(
    client: &SessionClient,
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
