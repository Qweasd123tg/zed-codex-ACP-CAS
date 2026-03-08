//! Rendering and update flow for command, MCP, web, and image tool-call events.
//! This module stays as a facade; concrete scenarios live in per-tool submodules.

use crate::thread::{SessionClient, ThreadInner, ThreadItem};

#[path = "command.rs"]
pub(in crate::thread) mod command;
#[path = "mcp.rs"]
pub(in crate::thread) mod mcp;
#[path = "web_image.rs"]
pub(in crate::thread) mod web_image;

// Started-item router for tool events. Returns the item when it does not belong to tool UI.
pub(in crate::thread) async fn handle_item_started(
    inner: &mut ThreadInner,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::CommandExecution {
            id,
            command,
            cwd,
            status,
            command_actions,
            ..
        } => {
            command::emit_command_execution_started(
                inner,
                id,
                command,
                cwd,
                status,
                command_actions,
            )
            .await;
            None
        }
        ThreadItem::McpToolCall {
            id,
            server,
            tool,
            status,
            arguments,
            ..
        } => {
            mcp::emit_mcp_tool_call_started(inner, id, server, tool, status, arguments).await;
            None
        }
        ThreadItem::WebSearch { id, query, .. } => {
            web_image::emit_web_search_started(inner, id, query).await;
            None
        }
        ThreadItem::ImageView { id, path } => {
            web_image::emit_image_view_started(inner, id, path).await;
            None
        }
        _ => Some(item),
    }
}

// Completed-item router for tool events.
pub(in crate::thread) async fn handle_item_completed(
    inner: &mut ThreadInner,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::CommandExecution {
            id,
            status,
            aggregated_output,
            exit_code,
            ..
        } => {
            command::emit_command_execution_completed(
                inner,
                id,
                status,
                aggregated_output,
                exit_code.map(|code| serde_json::json!({ "exit_code": code })),
            )
            .await;
            None
        }
        ThreadItem::McpToolCall {
            id,
            status,
            result,
            error,
            ..
        } => {
            mcp::emit_mcp_tool_call_completed(
                inner,
                id,
                status,
                result.map(|result| serde_json::json!({ "result": result })),
                error.map(|error| serde_json::json!({ "error": error })),
            )
            .await;
            None
        }
        ThreadItem::WebSearch { id, .. } => {
            web_image::emit_web_search_completed(inner, id).await;
            None
        }
        _ => Some(item),
    }
}

// Replay-item router for tool events.
pub(in crate::thread) async fn replay_item(
    client: &SessionClient,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::CommandExecution {
            id,
            command,
            cwd,
            status,
            command_actions,
            aggregated_output,
            exit_code,
            ..
        } => {
            command::replay_command_execution(
                client,
                command::ReplayCommandExecution {
                    id,
                    command,
                    cwd,
                    status,
                    command_actions,
                    aggregated_output,
                    exit_code_raw_output: exit_code
                        .map(|code| serde_json::json!({ "exit_code": code })),
                },
            )
            .await;
            None
        }
        ThreadItem::McpToolCall {
            id,
            server,
            tool,
            status,
            arguments,
            result,
            error,
            ..
        } => {
            mcp::replay_mcp_tool_call(
                client,
                mcp::ReplayMcpToolCall {
                    id,
                    server,
                    tool,
                    status,
                    arguments,
                    result_raw_output: result.map(|result| serde_json::json!({ "result": result })),
                    error_raw_output: error.map(|error| serde_json::json!({ "error": error })),
                },
            )
            .await;
            None
        }
        ThreadItem::WebSearch { id, query, .. } => {
            web_image::replay_web_search(client, id, query).await;
            None
        }
        ThreadItem::ImageView { id, path } => {
            web_image::replay_image_view(client, id, path).await;
            None
        }
        _ => Some(item),
    }
}
