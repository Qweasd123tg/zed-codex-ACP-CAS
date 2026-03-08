//! Notification-related feature slices and notification-event handling.

use agent_client_protocol::Error;
use codex_app_server_protocol::{
    FileChangeOutputDeltaNotification, JSONRPCNotification, McpToolCallProgressNotification,
    ReasoningSummaryTextDeltaNotification, ReasoningTextDeltaNotification, ServerNotification,
    ThreadTokenUsageUpdatedNotification,
};

use crate::thread::{
    StopReason, ThreadInner, handle_command_output_delta, handle_item_completed,
    handle_item_started, handle_terminal_interaction, handle_turn_diff_updated,
};

pub(in crate::thread) mod events;

pub(in crate::thread) async fn handle_notification(
    inner: &mut ThreadInner,
    notification: JSONRPCNotification,
    expected_turn_id: &str,
) -> Result<Option<StopReason>, Error> {
    let Ok(notification) = ServerNotification::try_from(notification) else {
        return Ok(None);
    };

    match notification {
        ServerNotification::AgentMessageDelta(delta) => {
            events::deltas::emit_agent_message_delta(
                inner,
                expected_turn_id,
                delta.turn_id,
                delta.delta,
            )
            .await;
            Ok(None)
        }
        ServerNotification::ReasoningTextDelta(ReasoningTextDeltaNotification {
            turn_id,
            delta,
            ..
        }) => {
            events::deltas::emit_reasoning_delta(inner, expected_turn_id, turn_id, delta).await;
            Ok(None)
        }
        ServerNotification::ReasoningSummaryTextDelta(ReasoningSummaryTextDeltaNotification {
            turn_id,
            delta,
            ..
        }) => {
            events::deltas::emit_reasoning_delta(inner, expected_turn_id, turn_id, delta).await;
            Ok(None)
        }
        ServerNotification::ThreadTokenUsageUpdated(ThreadTokenUsageUpdatedNotification {
            thread_id,
            token_usage,
            ..
        }) => {
            events::usage::emit_thread_token_usage_updated(
                inner,
                thread_id,
                token_usage.last.total_tokens,
                token_usage.model_context_window,
            )
            .await;
            Ok(None)
        }
        ServerNotification::TurnPlanUpdated(payload) => {
            events::turn::emit_turn_plan_updated(
                inner,
                expected_turn_id,
                payload.turn_id,
                payload.plan,
            )
            .await;
            Ok(None)
        }
        ServerNotification::PlanDelta(payload) => {
            events::deltas::emit_plan_delta(
                inner,
                expected_turn_id,
                payload.turn_id,
                payload.delta,
            )
            .await;
            Ok(None)
        }
        ServerNotification::TurnDiffUpdated(payload) => {
            handle_turn_diff_updated(inner, payload, expected_turn_id).await;
            Ok(None)
        }
        ServerNotification::ItemStarted(payload) => {
            handle_item_started(inner, payload).await;
            Ok(None)
        }
        ServerNotification::ItemCompleted(payload) => {
            handle_item_completed(inner, payload, expected_turn_id).await;
            Ok(None)
        }
        ServerNotification::CommandExecutionOutputDelta(payload) => {
            handle_command_output_delta(inner, payload).await;
            Ok(None)
        }
        ServerNotification::TerminalInteraction(payload) => {
            handle_terminal_interaction(inner, payload).await;
            Ok(None)
        }
        ServerNotification::FileChangeOutputDelta(FileChangeOutputDeltaNotification {
            item_id,
            turn_id,
            delta,
            ..
        }) => {
            events::deltas::emit_file_change_output_delta(
                inner,
                expected_turn_id,
                item_id,
                turn_id,
                delta,
            )
            .await;
            Ok(None)
        }
        ServerNotification::McpToolCallProgress(McpToolCallProgressNotification {
            item_id,
            turn_id,
            message,
            ..
        }) => {
            events::deltas::emit_mcp_tool_call_progress(
                inner,
                expected_turn_id,
                item_id,
                turn_id,
                message,
            )
            .await;
            Ok(None)
        }
        ServerNotification::TurnCompleted(payload) => {
            Ok(events::turn::emit_turn_completed(inner, expected_turn_id, payload.turn).await)
        }
        ServerNotification::Error(error) => {
            events::turn::emit_turn_error(
                inner,
                expected_turn_id,
                error.turn_id,
                error.error.message,
            )
            .await;
            Ok(None)
        }
        _ => Ok(None),
    }
}
