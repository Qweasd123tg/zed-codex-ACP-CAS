//! Поэлементные обработчики событий для обновлений thread-потока из app-server.

use super::{
    FallbackPlanPhase, ItemCompletedNotification, ItemStartedNotification, ThreadInner, ThreadItem,
    command_looks_like_verification, maybe_advance_fallback_plan,
};
use crate::thread::features::collab::{CollabToolCallData, render};
use crate::thread::features::file::events::{emit_file_change_completed, emit_file_change_started};
use crate::thread::features::plan::events::emit_plan_item_completed;
use crate::thread::features::session::events::{
    emit_context_compaction_completed, mark_context_compaction_started,
};
use crate::thread::features::tool_events::command::{
    emit_command_execution_completed, emit_command_execution_started,
};
use crate::thread::features::tool_events::mcp::{
    emit_mcp_tool_call_completed, emit_mcp_tool_call_started,
};
use crate::thread::features::tool_events::web_image::{
    emit_image_view_started, emit_web_search_completed, emit_web_search_started,
};

// Помечаем turn-локальное состояние сразу при старте item, чтобы последующие дельты корректно маппились.
pub(super) async fn handle_item_started(inner: &mut ThreadInner, payload: ItemStartedNotification) {
    let turn_id = payload.turn_id.clone();
    match payload.item {
        ThreadItem::ContextCompaction { .. } => {
            mark_context_compaction_started(inner);
        }
        ThreadItem::CommandExecution {
            id,
            command,
            cwd,
            status,
            command_actions,
            ..
        } => {
            maybe_advance_fallback_plan(inner, &turn_id, FallbackPlanPhase::Implementing).await;
            if command_looks_like_verification(&command) {
                maybe_advance_fallback_plan(inner, &turn_id, FallbackPlanPhase::Verifying).await;
            }
            emit_command_execution_started(inner, id, command, cwd, status, command_actions).await;
        }
        ThreadItem::FileChange {
            id,
            changes,
            status,
        } => {
            maybe_advance_fallback_plan(inner, &turn_id, FallbackPlanPhase::Implementing).await;
            emit_file_change_started(inner, id, changes, status).await;
        }
        ThreadItem::McpToolCall {
            id,
            server,
            tool,
            status,
            arguments,
            ..
        } => {
            maybe_advance_fallback_plan(inner, &turn_id, FallbackPlanPhase::Implementing).await;
            emit_mcp_tool_call_started(inner, id, server, tool, status, arguments).await;
        }
        ThreadItem::CollabAgentToolCall {
            id,
            tool,
            status,
            sender_thread_id,
            receiver_thread_ids,
            prompt,
            agents_states,
        } => {
            maybe_advance_fallback_plan(inner, &turn_id, FallbackPlanPhase::Implementing).await;
            render::emit_collab_tool_call_started(
                inner,
                CollabToolCallData {
                    id,
                    tool,
                    status,
                    sender_thread_id,
                    receiver_thread_ids,
                    prompt,
                    agents_states,
                },
            )
            .await;
        }
        ThreadItem::WebSearch { id, query, .. } => {
            maybe_advance_fallback_plan(inner, &turn_id, FallbackPlanPhase::Implementing).await;
            emit_web_search_started(inner, id, query).await;
        }
        ThreadItem::ImageView { id, path } => {
            maybe_advance_fallback_plan(inner, &turn_id, FallbackPlanPhase::Implementing).await;
            emit_image_view_started(inner, id, path).await;
        }
        _ => {}
    }
}

pub(super) async fn handle_item_completed(
    inner: &mut ThreadInner,
    payload: ItemCompletedNotification,
    expected_turn_id: &str,
) {
    let turn_id = payload.turn_id.clone();
    match payload.item {
        ThreadItem::ContextCompaction { .. } => {
            emit_context_compaction_completed(inner).await;
        }
        ThreadItem::CommandExecution {
            id,
            command: _,
            status,
            aggregated_output,
            exit_code,
            ..
        } => {
            emit_command_execution_completed(
                inner,
                id,
                status,
                aggregated_output,
                exit_code.map(|code| serde_json::json!({ "exit_code": code })),
            )
            .await;
        }
        ThreadItem::FileChange {
            id,
            changes,
            status,
        } => {
            emit_file_change_completed(inner, id, changes, status).await;
        }
        ThreadItem::McpToolCall {
            id,
            status,
            result,
            error,
            ..
        } => {
            emit_mcp_tool_call_completed(
                inner,
                id,
                status,
                result.map(|result| serde_json::json!({ "result": result })),
                error.map(|error| serde_json::json!({ "error": error })),
            )
            .await;
        }
        ThreadItem::CollabAgentToolCall {
            id,
            tool,
            status,
            sender_thread_id,
            receiver_thread_ids,
            prompt,
            agents_states,
        } => {
            render::emit_collab_tool_call_completed(
                inner,
                CollabToolCallData {
                    id,
                    tool,
                    status,
                    sender_thread_id,
                    receiver_thread_ids,
                    prompt,
                    agents_states,
                },
            )
            .await;
        }
        ThreadItem::Plan { text, .. } => {
            emit_plan_item_completed(inner, turn_id, expected_turn_id, text).await;
        }
        ThreadItem::WebSearch { id, .. } => {
            emit_web_search_completed(inner, id).await;
        }
        _ => {}
    }
}
