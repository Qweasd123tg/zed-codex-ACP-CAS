//! Delta and progress notification branches for text deltas and tool progress updates.

use agent_client_protocol::{ToolCallId, ToolCallUpdate, ToolCallUpdateFields};
use codex_protocol::config_types::ModeKind;

use crate::thread::{
    FallbackPlanPhase, ThreadInner, fallback_plan_can_enter_summarizing,
    maybe_advance_fallback_plan,
};

// Handle assistant text deltas: the first meaningful text moves fallback-plan into summarizing.
pub(in crate::thread) async fn emit_agent_message_delta(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    turn_id: String,
    delta: String,
) {
    if turn_id != expected_turn_id {
        return;
    }

    if !delta.trim().is_empty()
        && fallback_plan_can_enter_summarizing(
            inner.fallback_plan.as_ref(),
            expected_turn_id,
            !inner.started_tool_calls.is_empty(),
        )
    {
        maybe_advance_fallback_plan(inner, expected_turn_id, FallbackPlanPhase::Summarizing).await;
    }

    inner.client.send_agent_text(delta).await;
}

// Treat reasoning deltas as thought messages for the current turn.
pub(in crate::thread) async fn emit_reasoning_delta(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    turn_id: String,
    delta: String,
) {
    if turn_id == expected_turn_id {
        inner.client.send_agent_thought(delta).await;
    }
}

// Handle plan deltas for the active turn.
pub(in crate::thread) async fn emit_plan_delta(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    turn_id: String,
    delta: String,
) {
    if turn_id == expected_turn_id {
        // In plan mode we render a structured SessionUpdate::Plan,
        // so we do not duplicate the raw markdown stream from plan/delta.
        if inner.active_turn_mode_kind == Some(ModeKind::Plan) {
            return;
        }
        inner.active_turn_saw_plan_delta = true;
        inner.client.send_agent_text(delta).await;
    }
}

// Forward file-change progress into a content tool-call update.
pub(in crate::thread) async fn emit_file_change_output_delta(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    item_id: String,
    turn_id: String,
    delta: String,
) {
    if turn_id == expected_turn_id {
        inner
            .client
            .send_tool_call_update(ToolCallUpdate::new(
                ToolCallId::new(item_id),
                ToolCallUpdateFields::new().content(vec![delta.into()]),
            ))
            .await;
    }
}

// Forward MCP tool-call progress into a content tool-call update.
pub(in crate::thread) async fn emit_mcp_tool_call_progress(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    item_id: String,
    turn_id: String,
    message: String,
) {
    if turn_id == expected_turn_id {
        inner
            .client
            .send_tool_call_update(ToolCallUpdate::new(
                ToolCallId::new(item_id),
                ToolCallUpdateFields::new().content(vec![message.into()]),
            ))
            .await;
    }
}
