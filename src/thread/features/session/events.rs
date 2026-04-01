//! Сессионные item-события вне tool-card lifecycle.
//! Держим здесь compaction/review/simple replay ветки, чтобы роутеры были тоньше.

use std::fmt::Write as _;

use agent_client_protocol::SessionUpdate;
use codex_app_server_protocol::UserInput;

use super::session_info_title_update_now;
use crate::thread::{SessionClient, ThreadInner, turn_notify::notify_config_update};

// Replay-ветка пользовательского сообщения: сворачиваем mixed input в единый текстовый блок.
pub(in crate::thread) async fn replay_user_message(
    client: &SessionClient,
    content: Vec<UserInput>,
) {
    let text = render_user_inputs(content);
    if !text.is_empty() {
        client.send_user_text(text).await;
    }
}

// Replay-ветка обычного текста ассистента.
pub(in crate::thread) async fn replay_agent_message(client: &SessionClient, text: String) {
    client.send_agent_text(text).await;
}

// Replay-ветка reasoning: summary и content рендерим как thought-строки.
pub(in crate::thread) async fn replay_reasoning(
    client: &SessionClient,
    summary: Vec<String>,
    content: Vec<String>,
) {
    for part in summary {
        if !part.is_empty() {
            client.send_agent_thought(part).await;
        }
    }
    for part in content {
        if !part.is_empty() {
            client.send_agent_thought(part).await;
        }
    }
}

// Помечаем начало compaction, чтобы prompt-flow мог корректно блокировать следующий ввод.
pub(in crate::thread) fn mark_context_compaction_started(inner: &mut ThreadInner) {
    inner.compaction_in_progress = true;
}

// Завершаем compaction и синхронизируем состояние конфигурации для UI.
pub(in crate::thread) async fn emit_context_compaction_completed(inner: &mut ThreadInner) {
    inner.compaction_in_progress = false;
    inner.last_used_tokens = None;
    inner.context_usage_source = None;
    notify_config_update(inner).await;
    inner.client.send_agent_thought("Context compacted.").await;
}

// Replay-ветка для Plan item: это обычный agent-text без tool-card.
pub(in crate::thread) async fn replay_plan_text(client: &SessionClient, text: String) {
    if !text.is_empty() {
        client.send_agent_text(text).await;
    }
}

// Replay-ветка для входа в review mode.
pub(in crate::thread) async fn replay_entered_review_mode(client: &SessionClient, review: String) {
    client
        .send_agent_thought(format!("Entered review mode: {review}"))
        .await;
}

pub(in crate::thread) async fn emit_entered_review_mode(inner: &mut ThreadInner, review: String) {
    inner
        .client
        .send_agent_thought(format!("Entered review mode: {review}"))
        .await;
}

// Replay-ветка для выхода из review mode.
pub(in crate::thread) async fn replay_exited_review_mode(client: &SessionClient, review: String) {
    client
        .send_agent_thought(format!("Exited review mode: {review}"))
        .await;
}

pub(in crate::thread) async fn emit_exited_review_mode(inner: &mut ThreadInner, review: String) {
    inner
        .client
        .send_agent_thought(format!("Exited review mode: {review}"))
        .await;
}

// Replay-ветка для compaction item.
pub(in crate::thread) async fn replay_context_compaction(client: &SessionClient) {
    client.send_agent_thought("Context compacted.").await;
}

pub(in crate::thread) async fn emit_thread_name_updated(inner: &mut ThreadInner, title: String) {
    if inner.active_turn_id.is_some() {
        inner.pending_thread_title_update = Some(title);
        return;
    }

    inner
        .client
        .send_notification(SessionUpdate::SessionInfoUpdate(
            session_info_title_update_now(title),
        ))
        .await;
}

pub(in crate::thread) async fn flush_pending_thread_title_update(inner: &mut ThreadInner) {
    let Some(title) = inner.pending_thread_title_update.take() else {
        return;
    };

    inner
        .client
        .send_notification(SessionUpdate::SessionInfoUpdate(
            session_info_title_update_now(title),
        ))
        .await;
}

// Превращаем mixed user input в плоский текст для replay.
fn render_user_inputs(inputs: Vec<UserInput>) -> String {
    let mut rendered = String::new();

    for (index, input) in inputs.into_iter().enumerate() {
        if index > 0 {
            rendered.push('\n');
        }

        match input {
            UserInput::Text { text, .. } => rendered.push_str(&text),
            UserInput::Image { .. } => rendered.push_str("[image]"),
            UserInput::LocalImage { path } => {
                let _ = write!(rendered, "[image: {}]", path.display());
            }
            UserInput::Skill { name, .. } => {
                rendered.push_str("[skill: ");
                rendered.push_str(&name);
                rendered.push(']');
            }
            UserInput::Mention { name, .. } => {
                rendered.push('@');
                rendered.push_str(&name);
            }
        }
    }

    rendered
}
