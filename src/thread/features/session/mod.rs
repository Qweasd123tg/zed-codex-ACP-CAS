//! Session-related slash-command and item feature slices.

use crate::thread::{SessionClient, ThreadInner, ThreadItem};

pub(in crate::thread) mod controls;
pub(in crate::thread) mod events;
pub(in crate::thread) mod modes;

// Router for started-item session events outside the tool-card lifecycle.
pub(in crate::thread) async fn handle_item_started(
    inner: &mut ThreadInner,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::ContextCompaction { .. } => {
            events::mark_context_compaction_started(inner);
            None
        }
        _ => Some(item),
    }
}

// Router for completed-item session events.
pub(in crate::thread) async fn handle_item_completed(
    inner: &mut ThreadInner,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::ContextCompaction { .. } => {
            events::emit_context_compaction_completed(inner).await;
            None
        }
        _ => Some(item),
    }
}

// Router for replay-item session branches (user/agent/reasoning/plan/review/compaction).
pub(in crate::thread) async fn replay_item(
    client: &SessionClient,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::UserMessage { content, .. } => {
            events::replay_user_message(client, content).await;
            None
        }
        ThreadItem::AgentMessage { text, .. } => {
            events::replay_agent_message(client, text).await;
            None
        }
        ThreadItem::Reasoning {
            summary, content, ..
        } => {
            events::replay_reasoning(client, summary, content).await;
            None
        }
        ThreadItem::Plan { text, .. } => {
            events::replay_plan_text(client, text).await;
            None
        }
        ThreadItem::EnteredReviewMode { review, .. } => {
            events::replay_entered_review_mode(client, review).await;
            None
        }
        ThreadItem::ExitedReviewMode { review, .. } => {
            events::replay_exited_review_mode(client, review).await;
            None
        }
        ThreadItem::ContextCompaction { .. } => {
            events::replay_context_compaction(client).await;
            None
        }
        _ => Some(item),
    }
}
