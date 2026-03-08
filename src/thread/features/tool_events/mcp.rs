//! Live and replay handling for MCP tool-call items.

use agent_client_protocol::{ToolCall, ToolCallId, ToolCallUpdate, ToolCallUpdateFields, ToolKind};
use codex_app_server_protocol::McpToolCallStatus;
use serde_json::Value;

use crate::thread::features::status_mapping;
use crate::thread::{SessionClient, ThreadInner};

#[derive(Debug)]
// Replay payload for an MCP tool-call item so callers do not need wide argument lists.
pub(in crate::thread) struct ReplayMcpToolCall {
    pub(in crate::thread) id: String,
    pub(in crate::thread) server: String,
    pub(in crate::thread) tool: String,
    pub(in crate::thread) status: McpToolCallStatus,
    pub(in crate::thread) arguments: Value,
    pub(in crate::thread) result_raw_output: Option<Value>,
    pub(in crate::thread) error_raw_output: Option<Value>,
}

// Publish MCP tool-call start.
pub(in crate::thread) async fn emit_mcp_tool_call_started(
    inner: &mut ThreadInner,
    id: String,
    server: String,
    tool: String,
    status: McpToolCallStatus,
    arguments: Value,
) {
    inner.started_tool_calls.insert(id.clone());
    inner
        .client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id), format!("{server}.{tool}"))
                .kind(ToolKind::Execute)
                .status(status_mapping::map_mcp_status(status, true))
                .raw_input(arguments),
        )
        .await;
}

// Publish MCP tool-call completion with raw output.
pub(in crate::thread) async fn emit_mcp_tool_call_completed(
    inner: &mut ThreadInner,
    id: String,
    status: McpToolCallStatus,
    result_raw_output: Option<Value>,
    error_raw_output: Option<Value>,
) {
    let mut fields =
        ToolCallUpdateFields::new().status(status_mapping::map_mcp_status(status, false));
    if let Some(raw_output) = result_raw_output {
        fields = fields.raw_output(raw_output);
    }
    if let Some(raw_output) = error_raw_output {
        fields = fields.raw_output(raw_output);
    }

    inner
        .client
        .send_tool_call_update(ToolCallUpdate::new(ToolCallId::new(id.clone()), fields))
        .await;
    inner.started_tool_calls.remove(&id);
}

// Replay MCP tool-call by emitting start and update immediately.
pub(in crate::thread) async fn replay_mcp_tool_call(
    client: &SessionClient,
    data: ReplayMcpToolCall,
) {
    let ReplayMcpToolCall {
        id,
        server,
        tool,
        status,
        arguments,
        result_raw_output,
        error_raw_output,
    } = data;

    client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id.clone()), format!("{server}.{tool}"))
                .kind(ToolKind::Execute)
                .status(status_mapping::map_mcp_status(status.clone(), false))
                .raw_input(arguments),
        )
        .await;

    let mut fields =
        ToolCallUpdateFields::new().status(status_mapping::map_mcp_status(status, false));
    if let Some(raw_output) = result_raw_output {
        fields = fields.raw_output(raw_output);
    }
    if let Some(raw_output) = error_raw_output {
        fields = fields.raw_output(raw_output);
    }

    client
        .send_tool_call_update(ToolCallUpdate::new(ToolCallId::new(id), fields))
        .await;
}
