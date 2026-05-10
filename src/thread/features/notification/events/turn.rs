//! Turn-level notification-ветки (plan updates, turn completion/errors).

use agent_client_protocol::schema::{Plan, SessionUpdate, StopReason};
use codex_app_server_protocol::{Turn as AppTurn, TurnPlanStep, TurnStatus};
use codex_protocol::config_types::ModeKind;

use crate::thread::features::notification::events::reconnect::{
    format_reconnect_status, parse_reconnect_progress,
};
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

    inner.mark_turn_progress();

    // В plan-mode канонический поток — это item/plan + item/plan/delta из <proposed_plan>.
    // turn/plan/updated (update_plan tool) в этом режиме ведёт к UI-артефактам checklist.
    if inner.active_turn_mode_kind == Some(ModeKind::Plan) {
        return;
    }

    inner.turn_plan_updates_seen.insert(turn_id.clone());
    let mut entries = plan
        .into_iter()
        .map(turn_plan_step_to_entry)
        .collect::<Vec<_>>();
    if !entries.is_empty() {
        inner.active_turn_saw_plan_item = true;
    }
    if plan_entries_all_pending(&entries) {
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
        &mut inner.last_completed_turn_id,
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
    inner.mark_turn_progress();
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
        && let Some(message) = inner.record_turn_error_notice(&turn.id, error.message)
    {
        inner
            .client
            .send_system_message("error", "Turn failed", message)
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
        if let Some(progress) = parse_reconnect_progress(&message) {
            let warning_count = inner.note_reconnect_warning(progress.current >= progress.total);
            if warning_count == 1 {
                inner
                    .client
                    .send_agent_text(format_reconnect_status(progress))
                    .await;
            }
        } else if !message.trim().is_empty() {
            inner.mark_turn_progress();
            if let Some(message) = inner.record_turn_error_notice(&turn_id, message) {
                inner
                    .client
                    .send_system_message("error", "Turn error", message)
                    .await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::thread::features::notification::events::reconnect::{
        ReconnectProgress, parse_reconnect_progress,
    };

    #[test]
    fn parses_reconnect_turn_error_progress() {
        assert_eq!(
            parse_reconnect_progress("Reconnecting... 5/5"),
            Some(ReconnectProgress {
                current: 5,
                total: 5,
            })
        );
    }

    #[test]
    fn ignores_regular_turn_error_messages() {
        assert_eq!(parse_reconnect_progress("network unavailable"), None);
    }
}
