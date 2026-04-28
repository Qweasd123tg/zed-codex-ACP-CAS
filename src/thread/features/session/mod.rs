//! Группа session-related slash и item feature-срезов.

use agent_client_protocol::schema::SessionInfoUpdate;
use chrono::Utc;

use crate::thread::{SessionClient, ThreadInner, ThreadItem};

pub(in crate::thread) mod controls;
pub(in crate::thread) mod diff;
pub(in crate::thread) mod events;
pub(in crate::thread) mod modes;
pub(in crate::thread) mod review;
pub(in crate::thread) mod thread_switch;

pub(in crate::thread) fn session_info_title_update_now(
    title: impl Into<String>,
) -> SessionInfoUpdate {
    SessionInfoUpdate::new()
        .title(title.into())
        .updated_at(Utc::now().to_rfc3339())
}

pub(in crate::thread) fn session_info_title_update_from_unix(
    title: impl Into<String>,
    updated_at: i64,
) -> SessionInfoUpdate {
    let update = SessionInfoUpdate::new().title(title.into());
    match chrono::DateTime::<Utc>::from_timestamp(updated_at, 0) {
        Some(value) => update.updated_at(value.to_rfc3339()),
        None => update.updated_at(Utc::now().to_rfc3339()),
    }
}

// Роутер started-item для session-level событий вне tool-card жизненного цикла.
pub(in crate::thread) async fn handle_item_started(
    inner: &mut ThreadInner,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::ContextCompaction { .. } => {
            events::mark_context_compaction_started(inner);
            None
        }
        ThreadItem::EnteredReviewMode { review, .. } => {
            events::emit_entered_review_mode(inner, review).await;
            None
        }
        _ => Some(item),
    }
}

// Роутер completed-item для session-level событий.
pub(in crate::thread) async fn handle_item_completed(
    inner: &mut ThreadInner,
    item: ThreadItem,
) -> Option<ThreadItem> {
    match item {
        ThreadItem::ContextCompaction { .. } => {
            events::emit_context_compaction_completed(inner).await;
            None
        }
        ThreadItem::ExitedReviewMode { review, .. } => {
            events::emit_exited_review_mode(inner, review).await;
            None
        }
        _ => Some(item),
    }
}

// Роутер replay-item для session-level веток (user/agent/reasoning/plan/review/compaction).
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

#[cfg(test)]
mod tests {
    use super::{session_info_title_update_from_unix, session_info_title_update_now};
    use chrono::DateTime;

    #[test]
    fn session_info_update_from_unix_uses_rfc3339() {
        let update = session_info_title_update_from_unix("demo", 1_775_014_896);
        let updated_at = update.updated_at.value().expect("updated_at present");
        let parsed = DateTime::parse_from_rfc3339(updated_at).expect("valid rfc3339");
        assert_eq!(parsed.timestamp(), 1_775_014_896);
    }

    #[test]
    fn session_info_update_now_uses_rfc3339() {
        let update = session_info_title_update_now("demo");
        let updated_at = update.updated_at.value().expect("updated_at present");
        DateTime::parse_from_rfc3339(updated_at).expect("valid rfc3339");
    }
}
