//! Fallback state-machine plan-режима (инициализация и переходы фаз).

use agent_client_protocol::{Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus, SessionUpdate};
use codex_protocol::config_types::ModeKind;

use crate::thread::{FallbackPlanPhase, FallbackPlanState, MAX_VISIBLE_PLAN_ENTRIES, ThreadInner};

pub(in crate::thread) fn should_clear_visible_plan_for_mode_change(
    current_mode: ModeKind,
    next_mode: ModeKind,
    has_visible_plan_state: bool,
) -> bool {
    next_mode != ModeKind::Plan && (current_mode == ModeKind::Plan || has_visible_plan_state)
}

pub(in crate::thread) fn has_visible_plan_state(inner: &ThreadInner) -> bool {
    inner.fallback_plan.is_some()
        || !inner.last_plan_steps.is_empty()
        || inner.carryover_plan_steps.is_some()
}

pub(in crate::thread) async fn clear_visible_plan_state(inner: &mut ThreadInner) {
    inner.fallback_plan = None;
    inner.carryover_plan_steps = None;
    clear_plan_output(inner).await;
}

pub(in crate::thread) async fn initialize_fallback_plan_for_turn(
    inner: &mut ThreadInner,
    turn_id: &str,
    collaboration_mode_kind: ModeKind,
) {
    if collaboration_mode_kind == ModeKind::Plan {
        inner.fallback_plan = None;
        clear_plan_output(inner).await;
        return;
    }

    let Some(steps) = inner
        .carryover_plan_steps
        .take()
        .filter(|steps| !steps.is_empty())
    else {
        inner.fallback_plan = None;
        clear_plan_output(inner).await;
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
    true
}

pub(in crate::thread) fn fallback_plan_entries_for_steps(
    phase: FallbackPlanPhase,
    steps: &[String],
) -> Vec<PlanEntry> {
    fn in_progress_step_index(phase: FallbackPlanPhase, step_count: usize) -> Option<usize> {
        if step_count == 0 || phase == FallbackPlanPhase::Done {
            return None;
        }

        if step_count == 1 {
            return Some(0);
        }

        let last_index = step_count - 1;
        let distributed_index = |phase_slot: usize| last_index * phase_slot / 3;

        Some(match phase {
            FallbackPlanPhase::Planning => 0,
            FallbackPlanPhase::Implementing => distributed_index(1).max(1),
            FallbackPlanPhase::Verifying => distributed_index(2).max(1),
            FallbackPlanPhase::Summarizing => last_index,
            FallbackPlanPhase::Done => unreachable!("handled above"),
        })
    }

    fn status_for_step(
        phase: FallbackPlanPhase,
        index: usize,
        step_count: usize,
    ) -> PlanEntryStatus {
        if phase == FallbackPlanPhase::Done {
            return PlanEntryStatus::Completed;
        }

        let Some(target) = in_progress_step_index(phase, step_count) else {
            return PlanEntryStatus::Pending;
        };

        if index < target {
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
    let step_count = labels.len();

    labels
        .into_iter()
        .enumerate()
        .map(|(index, label)| {
            PlanEntry::new(
                label,
                PlanEntryPriority::Medium,
                status_for_step(phase, index, step_count),
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

async fn clear_plan_output(inner: &mut ThreadInner) {
    inner.last_plan_steps.clear();
    inner
        .client
        .send_notification(SessionUpdate::Plan(Plan::new(Vec::new())))
        .await;
}
