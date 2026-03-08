//! Session-level item events outside the tool-card lifecycle.
//! Keep compaction, review, and simple replay branches here so routers stay thin.

use std::fmt::Write as _;

use codex_app_server_protocol::UserInput;

use crate::thread::{SessionClient, ThreadInner, turn_notify::notify_config_update};

// Replay branch for a user message: flatten mixed input into one text block.
pub(in crate::thread) async fn replay_user_message(
    client: &SessionClient,
    content: Vec<UserInput>,
) {
    let text = render_user_inputs(content);
    if !text.is_empty() {
        client.send_user_text(text).await;
    }
}

// Replay branch for a plain assistant message.
pub(in crate::thread) async fn replay_agent_message(client: &SessionClient, text: String) {
    client.send_agent_text(text).await;
}

// Replay branch for reasoning: render summary and content as thought lines.
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

// Mark compaction start so prompt flow can correctly block the next input.
pub(in crate::thread) fn mark_context_compaction_started(inner: &mut ThreadInner) {
    inner.compaction_in_progress = true;
}

// Complete compaction and sync config state for the UI.
pub(in crate::thread) async fn emit_context_compaction_completed(inner: &mut ThreadInner) {
    inner.compaction_in_progress = false;
    inner.last_used_tokens = None;
    notify_config_update(inner).await;
    inner.client.send_agent_thought("Context compacted.").await;
}

// Replay branch for a Plan item: just regular agent text without a tool card.
pub(in crate::thread) async fn replay_plan_text(client: &SessionClient, text: String) {
    if !text.is_empty() {
        client.send_agent_text(text).await;
    }
}

// Replay branch for entering review mode.
pub(in crate::thread) async fn replay_entered_review_mode(client: &SessionClient, review: String) {
    client
        .send_agent_thought(format!("Entered review mode: {review}"))
        .await;
}

// Replay branch for leaving review mode.
pub(in crate::thread) async fn replay_exited_review_mode(client: &SessionClient, review: String) {
    client
        .send_agent_thought(format!("Exited review mode: {review}"))
        .await;
}

// Replay branch for a compaction item.
pub(in crate::thread) async fn replay_context_compaction(client: &SessionClient) {
    client.send_agent_thought("Context compacted.").await;
}

// Convert mixed user input into flat text for replay.
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
