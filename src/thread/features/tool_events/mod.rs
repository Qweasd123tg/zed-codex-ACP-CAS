//! Рендер и обновление command/mcp/web/image tool-call событий.
//! Модуль оставлен фасадом; конкретные сценарии вынесены по типам tool-call.

use crate::thread::{SessionClient, ThreadInner, ThreadItem};
use std::path::Path;

#[path = "command.rs"]
pub(in crate::thread) mod command;
#[path = "mcp.rs"]
pub(in crate::thread) mod mcp;
#[path = "web_image.rs"]
pub(in crate::thread) mod web_image;

// Роутер started-item для tool-событий: возвращает остаток, если item не относится к tool-витрине.
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
        ThreadItem::ImageGeneration { id, status, .. } => {
            web_image::emit_image_generation_started(inner, id, status).await;
            None
        }
        _ => Some(item),
    }
}

// Роутер completed-item для tool-событий.
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
        ThreadItem::ImageGeneration {
            id,
            status,
            revised_prompt,
            result,
        } => {
            web_image::emit_image_generation_completed(inner, id, status, revised_prompt, result)
                .await;
            None
        }
        _ => Some(item),
    }
}

// Роутер replay-item для tool-событий.
pub(in crate::thread) async fn replay_item(
    client: &SessionClient,
    cas_home: &Path,
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
        ThreadItem::ImageGeneration {
            id,
            status,
            revised_prompt,
            result,
        } => {
            web_image::replay_image_generation(
                client,
                cas_home,
                id,
                status,
                revised_prompt,
                result,
            )
            .await;
            None
        }
        _ => Some(item),
    }
}
