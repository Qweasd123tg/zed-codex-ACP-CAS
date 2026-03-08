//! Domain-specific status mappers from app-server statuses to ACP tool-call statuses.

use agent_client_protocol::ToolCallStatus;
use codex_app_server_protocol::{CommandExecutionStatus, McpToolCallStatus, PatchApplyStatus};

// Keep command-status mapping explicit so UI badges remain predictable.
pub(in crate::thread) fn map_command_status(
    status: CommandExecutionStatus,
    assume_in_progress: bool,
) -> ToolCallStatus {
    if assume_in_progress {
        return ToolCallStatus::InProgress;
    }
    match status {
        CommandExecutionStatus::Completed => ToolCallStatus::Completed,
        CommandExecutionStatus::InProgress
        | CommandExecutionStatus::Failed
        | CommandExecutionStatus::Declined => ToolCallStatus::Failed,
    }
}

pub(in crate::thread) fn map_patch_status(
    status: PatchApplyStatus,
    assume_in_progress: bool,
) -> ToolCallStatus {
    if assume_in_progress {
        return ToolCallStatus::InProgress;
    }
    match status {
        PatchApplyStatus::Completed => ToolCallStatus::Completed,
        PatchApplyStatus::InProgress | PatchApplyStatus::Failed | PatchApplyStatus::Declined => {
            ToolCallStatus::Failed
        }
    }
}

pub(in crate::thread) fn map_mcp_status(
    status: McpToolCallStatus,
    assume_in_progress: bool,
) -> ToolCallStatus {
    if assume_in_progress {
        return ToolCallStatus::InProgress;
    }
    match status {
        McpToolCallStatus::Completed => ToolCallStatus::Completed,
        McpToolCallStatus::InProgress | McpToolCallStatus::Failed => ToolCallStatus::Failed,
    }
}
