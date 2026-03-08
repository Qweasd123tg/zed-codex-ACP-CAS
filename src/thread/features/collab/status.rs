//! Status mapping and human-readable labels for collaboration and sub-agent tool calls.

use std::collections::HashMap;

use agent_client_protocol::ToolCallStatus;
use codex_app_server_protocol::{CollabAgentState, CollabAgentStatus, CollabAgentToolCallStatus};

// Map app-server collab tool-call statuses into ACP card statuses.
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

// Build a compact summary of all agent states for card content.
pub(in crate::thread) fn collab_status_summary_line(
    agents_states: &HashMap<String, CollabAgentState>,
) -> String {
    let mut pending_init = 0usize;
    let mut running = 0usize;
    let mut completed = 0usize;
    let mut errored = 0usize;
    let mut shutdown = 0usize;
    let mut not_found = 0usize;

    for state in agents_states.values() {
        match state.status {
            CollabAgentStatus::PendingInit => pending_init += 1,
            CollabAgentStatus::Running => running += 1,
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
    if pending_init > 0 {
        parts.push(format!("{pending_init} pending_init"));
    }
    if errored > 0 {
        parts.push(format!("{errored} errored"));
    }
    if shutdown > 0 {
        parts.push(format!("{shutdown} shutdown"));
    }
    if not_found > 0 {
        parts.push(format!("{not_found} not_found"));
    }

    format!("Agents: {}", parts.join(" · "))
}

// Local label for a single agent status.
pub(super) fn collab_agent_status_label(status: &CollabAgentStatus) -> &'static str {
    match status {
        CollabAgentStatus::PendingInit => "pending_init",
        CollabAgentStatus::Running => "running",
        CollabAgentStatus::Completed => "completed",
        CollabAgentStatus::Errored => "errored",
        CollabAgentStatus::Shutdown => "shutdown",
        CollabAgentStatus::NotFound => "not_found",
    }
}
