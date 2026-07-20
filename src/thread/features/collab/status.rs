//! Статусы collaboration/sub-agent tool-calls и их человекочитаемые подписи.

use std::collections::HashMap;

use agent_client_protocol::schema::v1::ToolCallStatus;
use codex_app_server_protocol::{CollabAgentState, CollabAgentStatus, CollabAgentToolCallStatus};

// Маппим app-server статусы collab tool-call в статусы ACP-карточек.
pub(in crate::thread) fn map_collab_status(
    status: CollabAgentToolCallStatus,
    assume_in_progress: bool,
) -> ToolCallStatus {
    if assume_in_progress {
        return ToolCallStatus::InProgress;
    }
    match status {
        CollabAgentToolCallStatus::Completed => ToolCallStatus::Completed,
        CollabAgentToolCallStatus::InProgress | CollabAgentToolCallStatus::Failed => {
            ToolCallStatus::Failed
        }
    }
}

// Строим компактную сводку состояний всех агентов для заголовка карточки.
pub(in crate::thread) fn collab_status_summary_line(
    agents_states: &HashMap<String, CollabAgentState>,
) -> String {
    let mut pending_init = 0usize;
    let mut running = 0usize;
    let mut interrupted = 0usize;
    let mut completed = 0usize;
    let mut errored = 0usize;
    let mut shutdown = 0usize;
    let mut not_found = 0usize;

    for state in agents_states.values() {
        match state.status {
            CollabAgentStatus::PendingInit => pending_init += 1,
            CollabAgentStatus::Running => running += 1,
            CollabAgentStatus::Interrupted => interrupted += 1,
            CollabAgentStatus::Completed => completed += 1,
            CollabAgentStatus::Errored => errored += 1,
            CollabAgentStatus::Shutdown => shutdown += 1,
            CollabAgentStatus::NotFound => not_found += 1,
        }
    }

    let mut parts = vec![format!("{} total", agents_states.len())];
    if running > 0 {
        parts.push(format!("{running} running"));
    }
    if completed > 0 {
        parts.push(format!("{completed} completed"));
    }
    if interrupted > 0 {
        parts.push(format!("{interrupted} interrupted"));
    }
    if pending_init > 0 {
        parts.push(format!("{pending_init} pending init"));
    }
    if errored > 0 {
        parts.push(format!("{errored} errored"));
    }
    if shutdown > 0 {
        parts.push(format!("{shutdown} shutdown"));
    }
    if not_found > 0 {
        parts.push(format!("{not_found} not found"));
    }

    format!("Agents: {}", parts.join(" · "))
}

const COMPLETED_MESSAGE_PREVIEW_LIMIT: usize = 240;
const ERROR_MESSAGE_PREVIEW_LIMIT: usize = 160;

// Человекочитаемая строка состояния отдельного агента, включая preview message/error.
pub(in crate::thread) fn collab_agent_state_summary(state: &CollabAgentState) -> String {
    match state.status {
        CollabAgentStatus::PendingInit => "Pending init".to_string(),
        CollabAgentStatus::Running => "Running".to_string(),
        CollabAgentStatus::Interrupted => "Interrupted".to_string(),
        CollabAgentStatus::Completed => format_state_with_message(
            "Completed",
            state.message.as_deref(),
            COMPLETED_MESSAGE_PREVIEW_LIMIT,
        ),
        CollabAgentStatus::Errored => format_state_with_message(
            "Error",
            state.message.as_deref(),
            ERROR_MESSAGE_PREVIEW_LIMIT,
        ),
        CollabAgentStatus::Shutdown => "Shutdown".to_string(),
        CollabAgentStatus::NotFound => "Not found".to_string(),
    }
}

fn format_state_with_message(label: &str, message: Option<&str>, limit: usize) -> String {
    let Some(message_preview) = preview_message(message, limit) else {
        return label.to_string();
    };
    format!("{label} - {message_preview}")
}

fn preview_message(message: Option<&str>, limit: usize) -> Option<String> {
    let message = message?.split_whitespace().collect::<Vec<_>>().join(" ");
    if message.is_empty() {
        return None;
    }

    let mut preview = String::new();
    let mut chars = message.chars();
    for _ in 0..limit {
        let Some(ch) = chars.next() else {
            return Some(message);
        };
        preview.push(ch);
    }

    if chars.next().is_some() {
        preview.push_str("...");
    }

    Some(preview)
}
