//! Live/replay обработка MCP tool-call веток.

use agent_client_protocol::{ToolCall, ToolCallId, ToolCallUpdate, ToolCallUpdateFields, ToolKind};
use codex_app_server_protocol::McpToolCallStatus;
use serde_json::Value;

use crate::thread::features::status_mapping;
use crate::thread::prompt_commands::normalize_preview;
use crate::thread::{SessionClient, ThreadInner};

#[derive(Debug)]
// Replay-пакет для MCP tool-call item, чтобы не передавать много аргументов.
pub(in crate::thread) struct ReplayMcpToolCall {
    pub(in crate::thread) id: String,
    pub(in crate::thread) server: String,
    pub(in crate::thread) tool: String,
    pub(in crate::thread) status: McpToolCallStatus,
    pub(in crate::thread) arguments: Value,
    pub(in crate::thread) result_raw_output: Option<Value>,
    pub(in crate::thread) error_raw_output: Option<Value>,
}

// Публикуем старт MCP tool-call.
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

// Публикуем завершение MCP tool-call с raw output.
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
        fields = fields.content(vec![mcp_result_summary(&raw_output).into()]);
        fields = fields.raw_output(raw_output);
    }
    if let Some(raw_output) = error_raw_output {
        fields = fields.content(vec![mcp_error_summary(&raw_output).into()]);
        fields = fields.raw_output(raw_output);
    }

    inner
        .client
        .send_tool_call_update(ToolCallUpdate::new(ToolCallId::new(id.clone()), fields))
        .await;
    inner.started_tool_calls.remove(&id);
}

// Replay-рендер MCP tool-call: start + update.
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
        fields = fields.content(vec![mcp_result_summary(&raw_output).into()]);
        fields = fields.raw_output(raw_output);
    }
    if let Some(raw_output) = error_raw_output {
        fields = fields.content(vec![mcp_error_summary(&raw_output).into()]);
        fields = fields.raw_output(raw_output);
    }

    client
        .send_tool_call_update(ToolCallUpdate::new(ToolCallId::new(id), fields))
        .await;
}

fn mcp_result_summary(raw_output: &Value) -> String {
    format!("Result: {}", mcp_value_summary(raw_output))
}

fn mcp_error_summary(raw_output: &Value) -> String {
    format!("Error: {}", mcp_value_summary(raw_output))
}

fn mcp_value_summary(value: &Value) -> String {
    match value {
        Value::String(text) => normalize_preview(text),
        Value::Array(items) => format!("{} item(s). Open Raw Output for details.", items.len()),
        Value::Object(map) => {
            if let Some(text) = map
                .get("content")
                .and_then(|content| content.as_str())
                .or_else(|| map.get("message").and_then(|message| message.as_str()))
            {
                return normalize_preview(text);
            }
            format!("{} field(s). Open Raw Output for details.", map.len())
        }
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
    }
}
