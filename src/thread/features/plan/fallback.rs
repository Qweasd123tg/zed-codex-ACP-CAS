//! Fallback state machine for plan mode: initialization and phase transitions.

use agent_client_protocol::{Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus, SessionUpdate};
use codex_protocol::config_types::ModeKind;

use crate::thread::{FallbackPlanPhase, FallbackPlanState, MAX_VISIBLE_PLAN_ENTRIES, ThreadInner};

pub(in crate::thread) async fn initialize_fallback_plan_for_turn(
    inner: &mut ThreadInner,
    turn_id: &str,
    collaboration_mode_kind: ModeKind,
) {
    if collaboration_mode_kind == ModeKind::Plan {
        inner.fallback_plan = None;
        clear_plan(inner).await;
        return;
    }

    let Some(steps) = inner
        .carryover_plan_steps
        .take()
        .filter(|steps| !steps.is_empty())
    else {
        inner.fallback_plan = None;
        clear_plan(inner).await;
        return;
    };

    inner.fallback_plan = Some(FallbackPlanState {
        turn_id: turn_id.to_string(),
        phase: FallbackPlanPhase::Planning,
        saw_tool_activity: false,
        steps: steps.clone(),
    });
    inner.last_plan_steps = steps.clone();
    let entries = fallback_plan_entries_for_steps(FallbackPlanPhase::Planning, &steps);
    inner
        .client
        .send_notification(SessionUpdate::Plan(Plan::new(limit_plan_entries(entries))))
        .await;
}

pub(in crate::thread) async fn maybe_advance_fallback_plan(
    inner: &mut ThreadInner,
    turn_id: &str,
    next_phase: FallbackPlanPhase,
) {
    let mut entries_to_emit = None;
    if let Some(state) = inner.fallback_plan.as_mut()
        && state.turn_id == turn_id
    {
        if matches!(
            next_phase,
            FallbackPlanPhase::Implementing
                | FallbackPlanPhase::Verifying
                | FallbackPlanPhase::Summarizing
        ) {
            state.saw_tool_activity = true;
        }
        if !fallback_plan_should_advance(state, next_phase) {
            return;
        }
        state.phase = next_phase;
        entries_to_emit = Some(fallback_plan_entries_for_steps(state.phase, &state.steps));
    }

    if let Some(entries) = entries_to_emit {
        inner.last_plan_steps = entries.iter().map(|entry| entry.content.clone()).collect();
        inner
            .client
            .send_notification(SessionUpdate::Plan(Plan::new(limit_plan_entries(entries))))
            .await;
    }
}

pub(in crate::thread) fn fallback_plan_should_advance(
    state: &FallbackPlanState,
    next_phase: FallbackPlanPhase,
) -> bool {
    if next_phase <= state.phase {
        return false;
    }
    if next_phase == FallbackPlanPhase::Done && !state.saw_tool_activity {
        return false;
    }
    true
}

pub(in crate::thread) fn fallback_plan_entries_for_steps(
    phase: FallbackPlanPhase,
    steps: &[String],
) -> Vec<PlanEntry> {
    fn status_for_step(phase: FallbackPlanPhase, index: usize) -> PlanEntryStatus {
        let target = phase as usize;
        if phase == FallbackPlanPhase::Done || index < target {
            PlanEntryStatus::Completed
        } else if index == target {
            PlanEntryStatus::InProgress
        } else {
            PlanEntryStatus::Pending
        }
    }

    let default_steps = [
        "Decide implementation scope and approach",
        "Apply code changes",
        "Run checks and verification",
        "Review and summarize results",
    ];
    let labels = if steps.is_empty() {
        default_steps
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    } else {
        steps.to_vec()
    };

    labels
        .into_iter()
        .enumerate()
        .map(|(index, label)| {
            PlanEntry::new(
                label,
                PlanEntryPriority::Medium,
                status_for_step(phase, index),
            )
        })
        .collect()
}

pub(in crate::thread) fn fallback_plan_can_enter_summarizing(
    state: Option<&FallbackPlanState>,
    turn_id: &str,
    has_active_tool_calls: bool,
) -> bool {
    if has_active_tool_calls {
        return false;
    }
    let Some(state) = state else {
        return false;
    };
    state.turn_id == turn_id
        && state.saw_tool_activity
        && state.phase < FallbackPlanPhase::Summarizing
}

fn limit_plan_entries(mut entries: Vec<PlanEntry>) -> Vec<PlanEntry> {
    if entries.len() > MAX_VISIBLE_PLAN_ENTRIES {
        entries.truncate(MAX_VISIBLE_PLAN_ENTRIES);
    }
    entries
}

async fn clear_plan(inner: &mut ThreadInner) {
    inner.last_plan_steps.clear();
    inner
        .client
        .send_notification(SessionUpdate::Plan(Plan::new(Vec::new())))
        .await;
}
