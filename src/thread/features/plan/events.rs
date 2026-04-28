//! Обработка Plan-item событий (live-complete) и их синхронизация с fallback-plan state.

use agent_client_protocol::schema::{Plan, SessionUpdate};
use codex_protocol::config_types::ModeKind;

use crate::thread::ThreadInner;

// Обрабатываем завершённый Plan item: парсинг, fallback-фаза, нормализация и вывод в ACP.
pub(in crate::thread) async fn emit_plan_item_completed(
    inner: &mut ThreadInner,
    turn_id: String,
    expected_turn_id: &str,
    text: String,
) {
    let is_active_plan_turn =
        inner.active_turn_mode_kind == Some(ModeKind::Plan) && turn_id == expected_turn_id;

    if is_active_plan_turn && !text.trim().is_empty() {
        // Даже если строгий парсер шагов не сработал, фиксируем факт,
        // что turn отдал плановый item (для финального confirm-перехода).
        inner.active_turn_saw_plan_item = true;
    }

    if inner.turn_plan_updates_seen.contains(&turn_id) {
        if turn_id == expected_turn_id {
            inner.active_turn_saw_plan_item = true;
        }
        return;
    }

    let parsed_plan = if is_active_plan_turn {
        super::plan_from_plan_item_text(&text)
    } else {
        super::plan_from_text(&text)
    };

    if let Some(plan) = parsed_plan {
        if turn_id == expected_turn_id {
            inner.active_turn_saw_plan_item = true;
        }

        let plan = if is_active_plan_turn {
            // В plan-mode не прогоняем шаги через fallback state-machine:
            // иначе UI начинает автоматически проставлять InProgress/Completed.
            inner.fallback_plan = None;
            plan
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
