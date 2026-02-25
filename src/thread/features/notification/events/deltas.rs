//! Delta/progress notification-ветки (text deltas и tool progress updates).

use agent_client_protocol::{ToolCallId, ToolCallUpdate, ToolCallUpdateFields};
use codex_protocol::config_types::ModeKind;

use crate::thread::{
    FallbackPlanPhase, ThreadInner, fallback_plan_can_enter_summarizing,
    maybe_advance_fallback_plan,
};

// Обрабатываем агентский text-delta: при первом осмысленном тексте переключаем fallback-plan в summarizing.
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

// Обрабатываем reasoning-delta как thought-сообщение текущего turn.
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

// Обрабатываем Plan delta для активного turn.
pub(in crate::thread) async fn emit_plan_delta(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    turn_id: String,
    delta: String,
) {
    if turn_id == expected_turn_id {
        // В plan-mode показываем структурированный SessionUpdate::Plan,
        // поэтому не дублируем сырой markdown-стрим из plan/delta.
        if inner.active_turn_mode_kind == Some(ModeKind::Plan) {
            return;
        }
        inner.active_turn_saw_plan_delta = true;
        inner.client.send_agent_text(delta).await;
    }
}

// Прокидываем прогресс file-change в content tool-call update.
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

// Прокидываем прогресс MCP tool-call в content tool-call update.
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
