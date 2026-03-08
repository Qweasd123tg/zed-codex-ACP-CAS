//! Live and replay handling for shell command tool-call items.

use std::path::PathBuf;

use agent_client_protocol::{
    ToolCall, ToolCallId, ToolCallLocation, ToolCallUpdate, ToolCallUpdateFields,
};
use codex_app_server_protocol::{CommandAction, CommandExecutionStatus};
use serde_json::Value;
use tracing::warn;

use crate::thread::{
    SessionClient, ThreadInner,
    features::{
        file::changes as file_changes,
        status_mapping,
        tool_call_ui::{kind, raw, title},
    },
};

#[derive(Debug)]
// Replay payload for a shell command item so callers do not need wide argument lists.
pub(in crate::thread) struct ReplayCommandExecution {
    pub(in crate::thread) id: String,
    pub(in crate::thread) command: String,
    pub(in crate::thread) cwd: PathBuf,
    pub(in crate::thread) status: CommandExecutionStatus,
    pub(in crate::thread) command_actions: Vec<CommandAction>,
    pub(in crate::thread) aggregated_output: Option<String>,
    pub(in crate::thread) exit_code_raw_output: Option<Value>,
}

// Publish shell command start and prime read snapshots when read actions are present.
pub(in crate::thread) async fn emit_command_execution_started(
    inner: &mut ThreadInner,
    id: String,
    command: String,
    cwd: PathBuf,
    status: CommandExecutionStatus,
    command_actions: Vec<CommandAction>,
) {
    inner.started_tool_calls.insert(id.clone());
    if inner.client.supports_read_text_file() {
        for action in &command_actions {
            let CommandAction::Read { path, .. } = action else {
                continue;
            };
            let read_path = file_changes::resolve_workspace_path(&inner.workspace_cwd, path);
            if let Err(err) = inner.client.prime_file_snapshot(read_path.clone()).await {
                warn!(
                    "Failed to prime ACP snapshot for command read {}: {err:?}",
                    read_path.display()
                );
            }
        }
    }
    let tool_status = status_mapping::map_command_status(status, true);
    let title = title::command_tool_title(&command, &command_actions);
    let raw_input = raw::command_tool_raw_input(&command, &command_actions);
    let tool_kind = kind::command_tool_kind(&command, &command_actions);
    inner
        .client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id), title)
                .kind(tool_kind)
                .status(tool_status)
                .locations(vec![ToolCallLocation::new(cwd)])
                .content(title::command_tool_placeholder_content())
                .raw_input(raw_input),
        )
        .await;
}

// Publish shell command completion with aggregated stdout/stderr and exit code.
pub(in crate::thread) async fn emit_command_execution_completed(
    inner: &mut ThreadInner,
    id: String,
    status: CommandExecutionStatus,
    aggregated_output: Option<String>,
    exit_code_raw_output: Option<Value>,
) {
    let mut fields =
        ToolCallUpdateFields::new().status(status_mapping::map_command_status(status, false));
    if let Some(output) = aggregated_output {
        fields = fields.content(vec![format!("```sh\n{}\n```", output.trim_end()).into()]);
    }
    if let Some(raw_output) = exit_code_raw_output {
        fields = fields.raw_output(raw_output);
    }

    inner
        .client
        .send_tool_call_update(ToolCallUpdate::new(ToolCallId::new(id.clone()), fields))
        .await;
    inner.started_tool_calls.remove(&id);
}

// Replay shell command by emitting start and update immediately.
pub(in crate::thread) async fn replay_command_execution(
    client: &SessionClient,
    data: ReplayCommandExecution,
) {
    let ReplayCommandExecution {
        id,
        command,
        cwd,
        status,
        command_actions,
        aggregated_output,
        exit_code_raw_output,
    } = data;

    let title = title::command_tool_title(&command, &command_actions);
    let raw_input = raw::command_tool_raw_input(&command, &command_actions);
    let tool_kind = kind::command_tool_kind(&command, &command_actions);
    client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id.clone()), title)
                .kind(tool_kind)
                .status(status_mapping::map_command_status(status.clone(), false))
                .locations(vec![ToolCallLocation::new(cwd)])
                .content(title::command_tool_placeholder_content())
                .raw_input(raw_input),
        )
        .await;

    let mut fields =
        ToolCallUpdateFields::new().status(status_mapping::map_command_status(status, false));
    if let Some(output) = aggregated_output {
        fields = fields.content(vec![format!("```sh\n{}\n```", output.trim_end()).into()]);
    }
    if let Some(raw_output) = exit_code_raw_output {
        fields = fields.raw_output(raw_output);
    }
    client
        .send_tool_call_update(ToolCallUpdate::new(ToolCallId::new(id), fields))
        .await;
}
