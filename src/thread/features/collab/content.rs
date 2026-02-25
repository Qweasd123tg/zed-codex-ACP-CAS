//! Формирование текстового контента collaboration/sub-agent карточек.

use std::collections::HashMap;

use crate::thread::prompt_commands::normalize_preview;
use agent_client_protocol::ToolCallContent;
use codex_app_server_protocol::CollabAgentState;

// Собираем текстовый payload карточки: sender/receivers/prompt и статусы агентов.
pub(super) fn collab_tool_content(
    sender_thread_id: &str,
    receiver_thread_ids: &[String],
    prompt: Option<&str>,
    agents_states: &HashMap<String, CollabAgentState>,
    include_prompt: bool,
) -> Vec<ToolCallContent> {
    let mut lines = vec![format!("Sender: {sender_thread_id}")];
    if !receiver_thread_ids.is_empty() {
        lines.push(format!(
            "Receivers: {}",
            format_collab_receivers(receiver_thread_ids)
        ));
    }

    if include_prompt
        && let Some(prompt) = prompt.map(str::trim).filter(|prompt| !prompt.is_empty())
    {
        lines.push(format!("Prompt: {}", normalize_preview(prompt)));
    }

    if !agents_states.is_empty() {
        lines.push(super::status::collab_status_summary_line(agents_states));
        let mut statuses = agents_states.iter().collect::<Vec<_>>();
        statuses.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (thread_id, state) in statuses {
            lines.push(format!(
                "- {thread_id}: {}",
                super::status::collab_agent_status_label(&state.status)
            ));
        }
    }

    vec![lines.join("\n").into()]
}

// Ограничиваем количество receiver id в карточке, чтобы она оставалась читаемой.
pub(in crate::thread) fn format_collab_receivers(receiver_thread_ids: &[String]) -> String {
    const MAX_RECEIVER_IDS: usize = 3;
    if receiver_thread_ids.len() <= MAX_RECEIVER_IDS {
        return receiver_thread_ids.join(", ");
    }

    let visible = receiver_thread_ids
        .iter()
        .take(MAX_RECEIVER_IDS)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let remaining = receiver_thread_ids.len().saturating_sub(MAX_RECEIVER_IDS);
    format!("{visible}, ... (+{remaining} more)")
}
