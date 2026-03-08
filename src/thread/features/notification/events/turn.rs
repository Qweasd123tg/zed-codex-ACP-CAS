//! Turn-level notification branches for plan updates, completion, and errors.

use agent_client_protocol::{Plan, SessionUpdate, StopReason};
use codex_app_server_protocol::{Turn as AppTurn, TurnPlanStep, TurnStatus};
use codex_protocol::config_types::ModeKind;

use crate::thread::{
    FallbackPlanPhase, FallbackPlanState, ThreadInner, fallback_plan_entries_for_steps,
    finalize_turn_diff, limit_plan_entries, maybe_advance_fallback_plan, plan_entries_all_pending,
    turn_plan_step_to_entry, turn_state, warn,
};

// Handle full turn-plan updates and synchronize fallback-plan state.
pub(in crate::thread) async fn emit_turn_plan_updated(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    turn_id: String,
    plan: Vec<TurnPlanStep>,
) {
    if turn_id != expected_turn_id {
        return;
    }

    // In plan mode, the canonical stream is item/plan plus item/plan/delta from <proposed_plan>.
    // turn/plan/updated from the update_plan tool creates checklist UI artifacts in this mode.
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

// Complete a turn: deduplicate completion, close fallback-plan state, and compute stop reason.
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

// Surface the current turn's server error in user-visible output.
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
