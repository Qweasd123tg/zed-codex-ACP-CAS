//! Группа notification-related feature-срезов и обработка notification-событий.

use agent_client_protocol::Error;
use codex_app_server_protocol::{
    AccountRateLimitsUpdatedNotification, FileChangeOutputDeltaNotification, JSONRPCNotification,
    McpToolCallProgressNotification, ReasoningSummaryTextDeltaNotification,
    ReasoningTextDeltaNotification, ServerNotification, ThreadNameUpdatedNotification,
    ThreadTokenUsageUpdatedNotification, TurnStatus,
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
            turn_id,
            token_usage,
            ..
        }) => {
            events::usage::emit_thread_token_usage_updated(inner, thread_id, turn_id, token_usage)
                .await;
            Ok(None)
        }
        ServerNotification::AccountRateLimitsUpdated(AccountRateLimitsUpdatedNotification {
            rate_limits,
            ..
        }) => {
            events::usage::emit_account_rate_limits_updated(inner, rate_limits).await;
            Ok(None)
        }
        ServerNotification::ConfigWarning(warning) => {
            events::warnings::emit_config_warning(inner, warning).await;
            Ok(None)
        }
        ServerNotification::DeprecationNotice(notice) => {
            events::warnings::emit_deprecation_notice(inner, notice).await;
            Ok(None)
        }
        ServerNotification::WindowsWorldWritableWarning(warning) => {
            events::warnings::emit_windows_world_writable_warning(inner, warning).await;
            Ok(None)
        }
        ServerNotification::ThreadNameUpdated(ThreadNameUpdatedNotification {
            thread_id,
            thread_name,
            ..
        }) => {
            if thread_id == inner.thread_id
                && let Some(title) = thread_name
            {
                crate::thread::features::session::events::emit_thread_name_updated(inner, title)
                    .await;
            }
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
            handle_item_started(inner, payload, expected_turn_id).await;
            Ok(None)
        }
        ServerNotification::ItemCompleted(payload) => {
            handle_item_completed(inner, payload, expected_turn_id).await;
            Ok(None)
        }
        ServerNotification::ContextCompacted(payload) => {
            if payload.thread_id == inner.thread_id && inner.compaction_in_progress {
                crate::thread::features::session::events::emit_context_compaction_completed(inner)
                    .await;
            }
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
            if payload.thread_id == inner.thread_id && inner.compaction_in_progress {
                match payload.turn.status {
                    TurnStatus::Completed => {
                        crate::thread::features::session::events::emit_context_compaction_completed(
                            inner,
                        )
                        .await;
                        return Ok(None);
                    }
                    TurnStatus::Failed | TurnStatus::Interrupted => {
                        let message = payload
                            .turn
                            .error
                            .map(|error| error.message)
                            .unwrap_or_else(|| "backend ended the compaction turn".to_string());
                        crate::thread::features::session::events::emit_context_compaction_failed(
                            inner, message,
                        )
                        .await;
                        return Ok(None);
                    }
                    TurnStatus::InProgress => {}
                }
            }
            Ok(events::turn::emit_turn_completed(inner, expected_turn_id, payload.turn).await)
        }
        ServerNotification::Error(error) => {
            if error.thread_id == inner.thread_id
                && inner.compaction_in_progress
                && !error.will_retry
            {
                crate::thread::features::session::events::emit_context_compaction_failed(
                    inner,
                    error.error.message,
                )
                .await;
                return Ok(None);
            }
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
