//! Turn-level notification-ветки (plan updates, turn completion/errors).

use agent_client_protocol::{Plan, SessionUpdate, StopReason};
use codex_app_server_protocol::{Turn as AppTurn, TurnPlanStep, TurnStatus};
use codex_protocol::config_types::ModeKind;

use crate::thread::{
    FallbackPlanPhase, FallbackPlanState, ThreadInner, fallback_plan_entries_for_steps,
    finalize_turn_diff, limit_plan_entries, maybe_advance_fallback_plan, plan_entries_all_pending,
    turn_plan_step_to_entry, turn_state, warn,
};

// Обрабатываем полное обновление плана turn и синхронизируем fallback-plan state.
pub(in crate::thread) async fn emit_turn_plan_updated(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    turn_id: String,
    plan: Vec<TurnPlanStep>,
) {
    if turn_id != expected_turn_id {
        return;
    }

    inner.turn_plan_updates_seen.insert(turn_id.clone());
    let mut entries = plan
        .into_iter()
        .map(turn_plan_step_to_entry)
        .collect::<Vec<_>>();
    let is_active_plan_turn =
        inner.active_turn_mode_kind == Some(ModeKind::Plan) && turn_id == expected_turn_id;
    if is_active_plan_turn && plan_entries_all_pending(&entries) {
        let phase = inner
            .fallback_plan
            .as_ref()
            .filter(|state| state.turn_id == turn_id)
            .map(|state| state.phase)
            .unwrap_or_else(|| {
                if inner.started_tool_calls.is_empty() {
                    FallbackPlanPhase::Planning
                } else {
                    FallbackPlanPhase::Implementing
                }
            });
        let saw_tool_activity = inner
            .fallback_plan
            .as_ref()
            .filter(|state| state.turn_id == turn_id)
            .is_some_and(|state| state.saw_tool_activity)
            || !inner.started_tool_calls.is_empty();
        let steps = entries
            .iter()
            .map(|entry| entry.content.clone())
            .collect::<Vec<_>>();
        inner.fallback_plan = Some(FallbackPlanState {
            turn_id: turn_id.clone(),
            phase,
            saw_tool_activity,
            steps: steps.clone(),
        });
        entries = fallback_plan_entries_for_steps(phase, &steps);
    } else if inner
        .fallback_plan
        .as_ref()
        .is_some_and(|state| state.turn_id == turn_id)
    {
        inner.fallback_plan = None;
    }
    inner.last_plan_steps = entries.iter().map(|entry| entry.content.clone()).collect();
    inner
        .client
        .send_notification(SessionUpdate::Plan(Plan::new(limit_plan_entries(entries))))
        .await;
}

// Завершаем turn: дедупликация completion, закрытие fallback-plan и вычисление stop reason.
pub(in crate::thread) async fn emit_turn_completed(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    turn: AppTurn,
) -> Option<StopReason> {
    match turn_state::register_turn_completion(
        &mut inner.completed_turn_ids,
        expected_turn_id,
        &turn.id,
    ) {
        turn_state::TurnCompletionDisposition::Accepted => {}
        turn_state::TurnCompletionDisposition::Duplicate => {
            warn!(
                turn_id = turn.id.as_str(),
                "Ignoring duplicate turn completion notification"
            );
            return None;
        }
        turn_state::TurnCompletionDisposition::UnexpectedTurnId => {
            return None;
        }
    }

    maybe_advance_fallback_plan(inner, expected_turn_id, FallbackPlanPhase::Done).await;
    if inner
        .fallback_plan
        .as_ref()
        .is_some_and(|state| state.turn_id == expected_turn_id)
    {
        inner.fallback_plan = None;
    }
    inner.turn_plan_updates_seen.remove(expected_turn_id);
    finalize_turn_diff(inner, expected_turn_id).await;

    let status = turn.status.clone();
    if status == TurnStatus::Failed
        && let Some(error) = turn.error
    {
        inner
            .client
            .send_agent_text(format!("\n[turn error] {}", error.message))
            .await;
    }

    Some(match status {
        TurnStatus::Interrupted => StopReason::Cancelled,
        TurnStatus::Completed | TurnStatus::Failed | TurnStatus::InProgress => StopReason::EndTurn,
    })
}

// Прокидываем серверную ошибку текущего turn в пользовательский вывод.
pub(in crate::thread) async fn emit_turn_error(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    turn_id: String,
    message: String,
) {
    if turn_id == expected_turn_id {
        inner
            .client
            .send_agent_text(format!("\n[error] {message}"))
            .await;
    }
}
