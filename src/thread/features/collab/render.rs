//! Rendering for collaboration and sub-agent cards in live and replay flows.

use crate::thread::{SessionClient, ThreadInner};
use agent_client_protocol::{ToolCall, ToolCallId, ToolCallUpdate, ToolCallUpdateFields, ToolKind};
use codex_app_server_protocol::{CollabAgentTool, CollabAgentToolCallStatus};

use super::CollabToolCallData;

// Collab events render as think cards because they orchestrate agents.
pub(super) fn collab_tool_kind() -> ToolKind {
    ToolKind::Think
}

// Card title depends on the collab tool type and phase.
pub(in crate::thread) fn collab_tool_title(
    tool: &CollabAgentTool,
    completed: bool,
) -> &'static str {
    match (tool, completed) {
        (CollabAgentTool::SpawnAgent, true) => "Agent spawned",
        (CollabAgentTool::SpawnAgent, false) => "Spawn agent",
        (CollabAgentTool::SendInput, true) => "Input sent",
        (CollabAgentTool::SendInput, false) => "Send input to agent",
        (CollabAgentTool::Wait, true) => "Wait complete",
        (CollabAgentTool::Wait, false) => "Waiting for agents",
        (CollabAgentTool::CloseAgent, true) => "Agent closed",
        (CollabAgentTool::CloseAgent, false) => "Close agent",
    }
}

// Publish the shared rendering path for a live-start collab tool call.
pub(in crate::thread) async fn emit_collab_tool_call_started(
    inner: &mut ThreadInner,
    data: CollabToolCallData,
) {
    let CollabToolCallData {
        id,
        tool,
        status,
        sender_thread_id,
        receiver_thread_ids,
        prompt,
        agents_states,
    } = data;

    inner.started_tool_calls.insert(id.clone());
    inner
        .client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id), collab_tool_title(&tool, false))
                .kind(collab_tool_kind())
                .status(super::status::map_collab_status(status, true))
                .content(super::content::collab_tool_content(
                    &sender_thread_id,
                    &receiver_thread_ids,
                    prompt.as_deref(),
                    &agents_states,
                    true,
                )),
        )
        .await;
}

// Publish the shared rendering path for a live-complete or update collab tool call.
pub(in crate::thread) async fn emit_collab_tool_call_completed(
    inner: &mut ThreadInner,
    data: CollabToolCallData,
) {
    let CollabToolCallData {
        id,
        tool,
        status,
        sender_thread_id,
        receiver_thread_ids,
        prompt,
        agents_states,
    } = data;

    let completed = !matches!(status, CollabAgentToolCallStatus::InProgress);
    inner
        .client
        .send_tool_call_update(ToolCallUpdate::new(
            ToolCallId::new(id.clone()),
            ToolCallUpdateFields::new()
                .title(collab_tool_title(&tool, completed))
                .status(super::status::map_collab_status(status, false))
                .content(super::content::collab_tool_content(
                    &sender_thread_id,
                    &receiver_thread_ids,
                    prompt.as_deref(),
                    &agents_states,
                    false,
                ))
                .raw_output(serde_json::json!({
                    "senderThreadId": sender_thread_id,
                    "receiverThreadIds": receiver_thread_ids,
                    "prompt": prompt,
                    "agentsStates": agents_states,
                })),
        ))
        .await;
    inner.started_tool_calls.remove(&id);
}

// Replay uses the same title, status, and content rules as the live flow.
pub(in crate::thread) async fn replay_collab_tool_call(
    client: &SessionClient,
    data: CollabToolCallData,
) {
    let CollabToolCallData {
        id,
        tool,
        status,
        sender_thread_id,
        receiver_thread_ids,
        prompt,
        agents_states,
    } = data;

    let completed = !matches!(status, CollabAgentToolCallStatus::InProgress);
    client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id), collab_tool_title(&tool, completed))
                .kind(collab_tool_kind())
                .status(super::status::map_collab_status(status, false))
                .content(super::content::collab_tool_content(
                    &sender_thread_id,
                    &receiver_thread_ids,
                    prompt.as_deref(),
                    &agents_states,
                    !completed,
                )),
        )
        .await;
}
