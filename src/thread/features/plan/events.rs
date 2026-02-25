//! Обработка Plan-item событий (live-complete) и их синхронизация с fallback-plan state.

use agent_client_protocol::{Plan, SessionUpdate};
use codex_protocol::config_types::ModeKind;

use crate::thread::{FallbackPlanPhase, FallbackPlanState, ThreadInner};

// Обрабатываем завершённый Plan item: парсинг, fallback-фаза, нормализация и вывод в ACP.
pub(in crate::thread) async fn emit_plan_item_completed(
    inner: &mut ThreadInner,
    turn_id: String,
    expected_turn_id: &str,
    text: String,
) {
    if turn_id == expected_turn_id {
        inner.active_turn_saw_plan_item = true;
    }
    if inner.turn_plan_updates_seen.contains(&turn_id) {
        return;
    }

    if let Some(plan) = super::plan_from_text(&text) {
        let is_active_plan_turn =
            inner.active_turn_mode_kind == Some(ModeKind::Plan) && turn_id == expected_turn_id;

        let plan = if is_active_plan_turn {
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
                .is_some_and(|state| state.saw_tool_activity);
            let steps = plan
                .entries
                .iter()
                .map(|entry| entry.content.clone())
                .collect::<Vec<_>>();

            inner.fallback_plan = Some(FallbackPlanState {
                turn_id: turn_id.clone(),
                phase,
                saw_tool_activity,
                steps: steps.clone(),
            });

            Plan::new(super::fallback_plan_entries_for_steps(phase, &steps))
        } else {
            inner.turn_plan_updates_seen.insert(turn_id.clone());
            if inner
                .fallback_plan
                .as_ref()
                .is_some_and(|state| state.turn_id == turn_id)
            {
                inner.fallback_plan = None;
            }
            super::promote_first_pending_step(plan)
        };

        inner.last_plan_steps = plan
            .entries
            .iter()
            .map(|entry| entry.content.clone())
            .collect();
        inner
            .client
            .send_notification(SessionUpdate::Plan(Plan::new(super::limit_plan_entries(
                plan.entries,
            ))))
            .await;
        return;
    }

    if !text.is_empty() {
        if inner.active_turn_mode_kind == Some(ModeKind::Plan) && turn_id == expected_turn_id {
            if !inner.active_turn_saw_plan_delta {
                inner.client.send_agent_text(text).await;
            }
        } else {
            inner.client.send_agent_thought(text).await;
        }
    }
}
