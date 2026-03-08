//! Per-item event handlers for thread-stream updates coming from app-server.

use super::{
    FallbackPlanPhase, ItemCompletedNotification, ItemStartedNotification, ThreadInner, ThreadItem,
    command_looks_like_verification, maybe_advance_fallback_plan,
};
use crate::thread::features::{collab, file, plan, session, tool_events};

// Mark turn-local state as soon as an item starts so later deltas map correctly.
pub(super) async fn handle_item_started(inner: &mut ThreadInner, payload: ItemStartedNotification) {
    let turn_id = payload.turn_id.clone();
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
        | ThreadItem::WebSearch { .. }
        | ThreadItem::ImageView { .. } => {
            maybe_advance_fallback_plan(inner, turn_id, FallbackPlanPhase::Implementing).await;
        }
        _ => {}
    }
}
