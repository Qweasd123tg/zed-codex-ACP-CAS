//! Утилиты replay для пересборки представления ACP-клиента из ранее отправленных thread-item.

use std::path::Path;

use super::{AppTurn, SessionClient, ThreadItem};
use crate::thread::features::collab::{CollabToolCallData, render};
use crate::thread::features::file::events::replay_file_change;
use crate::thread::features::session::events::{
    replay_agent_message, replay_context_compaction, replay_entered_review_mode,
    replay_exited_review_mode, replay_plan_text, replay_reasoning, replay_user_message,
};
use crate::thread::features::tool_events::command::{
    ReplayCommandExecution, replay_command_execution,
};
use crate::thread::features::tool_events::mcp::{ReplayMcpToolCall, replay_mcp_tool_call};
use crate::thread::features::tool_events::web_image::{replay_image_view, replay_web_search};

// Воспроизводим исторические turn в исходном порядке, чтобы восстановление состояния оставалось детерминированным.
pub(super) async fn replay_turns(
    client: &SessionClient,
    workspace_cwd: &Path,
    turns: Vec<AppTurn>,
) {
    for turn in turns {
        for item in turn.items {
            replay_thread_item(client, workspace_cwd, item).await;
        }
    }
}

async fn replay_thread_item(client: &SessionClient, workspace_cwd: &Path, item: ThreadItem) {
    match item {
        ThreadItem::UserMessage { content, .. } => {
            replay_user_message(client, content).await;
        }
        ThreadItem::AgentMessage { text, .. } => {
            replay_agent_message(client, text).await;
        }
        ThreadItem::Reasoning {
            summary, content, ..
        } => {
            replay_reasoning(client, summary, content).await;
        }
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
            replay_command_execution(
                client,
                ReplayCommandExecution {
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
        }
        ThreadItem::FileChange {
            id,
            changes,
            status,
        } => {
            replay_file_change(client, workspace_cwd, id, changes, status).await;
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
            replay_mcp_tool_call(
                client,
                ReplayMcpToolCall {
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
            render::replay_collab_tool_call(
                client,
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
            replay_web_search(client, id, query).await;
        }
        ThreadItem::ImageView { id, path } => {
            replay_image_view(client, id, path).await;
        }
        ThreadItem::Plan { text, .. } => {
            replay_plan_text(client, text).await;
        }
        ThreadItem::EnteredReviewMode { review, .. } => {
            replay_entered_review_mode(client, review).await;
        }
        ThreadItem::ExitedReviewMode { review, .. } => {
            replay_exited_review_mode(client, review).await;
        }
        ThreadItem::ContextCompaction { .. } => {
            replay_context_compaction(client).await;
        }
    }
}
