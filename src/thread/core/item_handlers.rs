//! Поэлементные обработчики событий для обновлений thread-потока из app-server.

use super::{
    FallbackPlanPhase, ItemCompletedNotification, ItemStartedNotification, ThreadInner, ThreadItem,
    command_looks_like_verification, maybe_advance_fallback_plan,
};
use crate::thread::features::{collab, file, plan, session, tool_events};

// Помечаем turn-локальное состояние сразу при старте item, чтобы последующие дельты корректно маппились.
pub(super) async fn handle_item_started(
    inner: &mut ThreadInner,
    payload: ItemStartedNotification,
    expected_turn_id: &str,
) {
    let turn_id = payload.turn_id.clone();
    if should_handle_session_item(&inner.thread_id, &payload.thread_id, &payload.item) {
        drop(session::handle_item_started(inner, payload.item).await);
        return;
    }
    if !should_handle_turn_item(expected_turn_id, &turn_id) {
        return;
    }
    maybe_advance_fallback_for_started_item(inner, &turn_id, &payload.item).await;

    let Some(item) = session::handle_item_started(inner, payload.item).await else {
        return;
    };
    let Some(item) = tool_events::handle_item_started(inner, item).await else {
        return;
    };
    let Some(item) = file::handle_item_started(inner, item).await else {
        return;
    };
    let _collab_item = collab::handle_item_started(inner, item).await;
}

pub(super) async fn handle_item_completed(
    inner: &mut ThreadInner,
    payload: ItemCompletedNotification,
    expected_turn_id: &str,
) {
    let turn_id = payload.turn_id.clone();
    if should_handle_session_item(&inner.thread_id, &payload.thread_id, &payload.item) {
        drop(session::handle_item_completed(inner, payload.item).await);
        return;
    }
    if !should_handle_turn_item(expected_turn_id, &turn_id) {
        return;
    }
    let Some(item) = session::handle_item_completed(inner, payload.item).await else {
        return;
    };
    let Some(item) = tool_events::handle_item_completed(inner, item).await else {
        return;
    };
    let Some(item) = file::handle_item_completed(inner, item).await else {
        return;
    };
    let Some(item) = collab::handle_item_completed(inner, item).await else {
        return;
    };

    if let ThreadItem::Plan { text, .. } = item {
        plan::events::emit_plan_item_completed(inner, turn_id, expected_turn_id, text).await;
    }
}

fn should_handle_turn_item(expected_turn_id: &str, turn_id: &str) -> bool {
    turn_id == expected_turn_id
}

fn should_handle_session_item(current_thread_id: &str, thread_id: &str, item: &ThreadItem) -> bool {
    current_thread_id == thread_id && matches!(item, ThreadItem::ContextCompaction { .. })
}

async fn maybe_advance_fallback_for_started_item(
    inner: &mut ThreadInner,
    turn_id: &str,
    item: &ThreadItem,
) {
    match item {
        ThreadItem::CommandExecution { command, .. } => {
            maybe_advance_fallback_plan(inner, turn_id, FallbackPlanPhase::Implementing).await;
            if command_looks_like_verification(command) {
                maybe_advance_fallback_plan(inner, turn_id, FallbackPlanPhase::Verifying).await;
            }
        }
        ThreadItem::FileChange { .. }
        | ThreadItem::McpToolCall { .. }
        | ThreadItem::CollabAgentToolCall { .. }
        | ThreadItem::WebSearch(_)
        | ThreadItem::ImageView { .. }
        | ThreadItem::ImageGeneration(_) => {
            maybe_advance_fallback_plan(inner, turn_id, FallbackPlanPhase::Implementing).await;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{should_handle_session_item, should_handle_turn_item};
    use crate::thread::ThreadItem;

    #[test]
    fn handles_only_matching_turn_items() {
        assert!(should_handle_turn_item("turn-1", "turn-1"));
        assert!(!should_handle_turn_item("turn-1", "turn-2"));
        assert!(!should_handle_turn_item("", "turn-1"));
    }

    #[test]
    fn handles_context_compaction_as_thread_scoped_session_item() {
        let item = ThreadItem::ContextCompaction {
            id: "compact-1".to_string(),
        };
        assert!(should_handle_session_item("thread-1", "thread-1", &item));
        assert!(!should_handle_session_item("thread-1", "thread-2", &item));
        assert!(!should_handle_session_item(
            "thread-1",
            "thread-1",
            &ThreadItem::Plan {
                id: "plan-1".to_string(),
                text: "plan".to_string(),
            }
        ));
    }
}
