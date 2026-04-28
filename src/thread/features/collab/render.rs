//! Рендер collaboration/sub-agent карточек для live и replay потоков.

use std::collections::HashMap;

use crate::thread::{SessionClient, ThreadInner};
use agent_client_protocol::schema::{
    ToolCall, ToolCallId, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use codex_app_server_protocol::{CollabAgentTool, CollabAgentToolCallStatus};

use super::{CollabAgentLabel, CollabToolCallData};

// Collab события отображаем как think-card (оркестрация агентов).
pub(super) fn collab_tool_kind() -> ToolKind {
    ToolKind::Think
}

// Заголовок карточки зависит от типа collab-инструмента и фазы (start/complete).
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
        (CollabAgentTool::ResumeAgent, true) => "Agent resumed",
        (CollabAgentTool::ResumeAgent, false) => "Resume agent",
        (CollabAgentTool::CloseAgent, true) => "Agent closed",
        (CollabAgentTool::CloseAgent, false) => "Close agent",
    }
}

pub(in crate::thread) fn collab_tool_title_with_context(
    tool: &CollabAgentTool,
    completed: bool,
    receiver_thread_ids: &[String],
    agent_labels: &HashMap<String, CollabAgentLabel>,
) -> String {
    let single_target = (receiver_thread_ids.len() == 1)
        .then(|| receiver_thread_ids[0].as_str())
        .map(|thread_id| short_thread_label(thread_id, agent_labels));

    match (tool, completed, single_target) {
        (CollabAgentTool::SpawnAgent, true, Some(target)) => format!("Spawned {target}"),
        (CollabAgentTool::SendInput, false, Some(target)) => format!("Send input to {target}"),
        (CollabAgentTool::SendInput, true, Some(target)) => format!("Input sent to {target}"),
        (CollabAgentTool::ResumeAgent, false, Some(target)) => format!("Resume {target}"),
        (CollabAgentTool::ResumeAgent, true, Some(target)) => format!("Resumed {target}"),
        (CollabAgentTool::CloseAgent, false, Some(target)) => format!("Close {target}"),
        (CollabAgentTool::CloseAgent, true, Some(target)) => format!("Closed {target}"),
        (CollabAgentTool::Wait, false, Some(target)) => format!("Waiting for {target}"),
        (CollabAgentTool::Wait, true, Some(target)) => format!("Finished waiting for {target}"),
        (CollabAgentTool::Wait, false, None) if receiver_thread_ids.len() > 1 => {
            format!("Waiting for {} agents", receiver_thread_ids.len())
        }
        (CollabAgentTool::Wait, true, None) if receiver_thread_ids.len() > 1 => {
            format!("Finished waiting for {} agents", receiver_thread_ids.len())
        }
        _ => collab_tool_title(tool, completed).to_string(),
    }
}

fn short_thread_label(thread_id: &str, agent_labels: &HashMap<String, CollabAgentLabel>) -> String {
    let Some(label) = agent_labels.get(thread_id) else {
        return thread_id.to_string();
    };

    match (
        label
            .nickname
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        label
            .role
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    ) {
        (Some(nickname), Some(role)) => format!("{nickname} [{role}]"),
        (Some(nickname), None) => nickname.to_string(),
        (None, Some(role)) => format!("[{role}]"),
        (None, None) => thread_id.to_string(),
    }
}

// Публикуем единый рендер для live-start события collab tool-call.
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
            ToolCall::new(
                ToolCallId::new(id),
                collab_tool_title_with_context(
                    &tool,
                    false,
                    &receiver_thread_ids,
                    &inner.agent_labels,
                ),
            )
            .kind(collab_tool_kind())
            .status(super::status::map_collab_status(status, true))
            .raw_input(super::content::collab_tool_raw_input(
                &tool,
                &inner.agent_labels,
                &receiver_thread_ids,
                prompt.as_deref(),
            ))
            .content(super::content::collab_tool_content(
                &tool,
                &inner.agent_labels,
                &sender_thread_id,
                &receiver_thread_ids,
                prompt.as_deref(),
                &agents_states,
                false,
            )),
        )
        .await;
}

// Публикуем единый рендер для live-complete/update события collab tool-call.
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
                .title(collab_tool_title_with_context(
                    &tool,
                    completed,
                    &receiver_thread_ids,
                    &inner.agent_labels,
                ))
                .status(super::status::map_collab_status(status, false))
                .raw_input(super::content::collab_tool_raw_input(
                    &tool,
                    &inner.agent_labels,
                    &receiver_thread_ids,
                    prompt.as_deref(),
                ))
                .raw_output(super::content::collab_tool_raw_output(
                    &inner.agent_labels,
                    &receiver_thread_ids,
                    &agents_states,
                ))
                .content(super::content::collab_tool_content(
                    &tool,
                    &inner.agent_labels,
                    &sender_thread_id,
                    &receiver_thread_ids,
                    prompt.as_deref(),
                    &agents_states,
                    completed,
                )),
        ))
        .await;
    inner.started_tool_calls.remove(&id);
}

// В replay используем те же правила заголовка/статуса/контента, как в live-потоке.
pub(in crate::thread) async fn replay_collab_tool_call(
    client: &SessionClient,
    agent_labels: &HashMap<String, CollabAgentLabel>,
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
            ToolCall::new(
                ToolCallId::new(id),
                collab_tool_title_with_context(
                    &tool,
                    completed,
                    &receiver_thread_ids,
                    agent_labels,
                ),
            )
            .kind(collab_tool_kind())
            .status(super::status::map_collab_status(status, false))
            .raw_input(super::content::collab_tool_raw_input(
                &tool,
                agent_labels,
                &receiver_thread_ids,
                prompt.as_deref(),
            ))
            .raw_output(super::content::collab_tool_raw_output(
                agent_labels,
                &receiver_thread_ids,
                &agents_states,
            ))
            .content(super::content::collab_tool_content(
                &tool,
                agent_labels,
                &sender_thread_id,
                &receiver_thread_ids,
                prompt.as_deref(),
                &agents_states,
                completed,
            )),
        )
        .await;
}
