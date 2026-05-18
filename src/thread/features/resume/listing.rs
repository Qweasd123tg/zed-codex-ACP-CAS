//! Вывод списка доступных thread для `/threads`.

use agent_client_protocol::{Error, schema::StopReason};
use codex_app_server_protocol::{Thread, ThreadItem, ThreadReadParams, ThreadSortKey, UserInput};

use super::common::{
    format_relative_timestamp, list_all_threads, thread_display_title, thread_matches_query,
};
use crate::thread::ThreadInner;

// Получаем историю thread и рендерим полный список для `/threads`.
pub(in crate::thread) async fn handle_threads_command(
    inner: &mut ThreadInner,
    query: Option<String>,
) -> Result<StopReason, Error> {
    let threads = list_all_threads(inner, ThreadSortKey::UpdatedAt, None, None).await?;

    if threads.is_empty() {
        inner
            .client
            .send_agent_text("No saved threads found. Create one prompt first.")
            .await;
        return Ok(StopReason::EndTurn);
    }

    if let Some(query) = query
        .map(|query| query.trim().to_string())
        .filter(|query| !query.is_empty())
    {
        return handle_thread_preview_command(inner, threads, &query).await;
    }

    let mut lines = vec![format!(
        "Saved threads (newest first, {} total):",
        threads.len()
    )];
    for thread in threads {
        lines.push(format!(
            "- `{}` | created: {} | updated: {} | branch: {} | {}",
            thread.id,
            format_relative_timestamp(thread.created_at),
            format_relative_timestamp(thread.updated_at),
            thread
                .git_info
                .as_ref()
                .and_then(|git| git.branch.as_deref())
                .filter(|value| !value.is_empty())
                .unwrap_or("-"),
            thread_display_title(&thread)
        ));
    }
    lines.push(
        "Use `/resume` to choose a thread from this workspace, or `/resume <partial_id>` to search."
            .to_string(),
    );

    inner.client.send_agent_text(lines.join("\n")).await;
    Ok(StopReason::EndTurn)
}

async fn handle_thread_preview_command(
    inner: &mut ThreadInner,
    threads: Vec<Thread>,
    query: &str,
) -> Result<StopReason, Error> {
    let mut candidates = threads
        .iter()
        .filter(|thread| thread.id == query)
        .cloned()
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        candidates = threads
            .iter()
            .filter(|thread| {
                thread.cwd == inner.workspace_cwd && thread_matches_query(thread, query)
            })
            .cloned()
            .collect::<Vec<_>>();
    }
    if candidates.is_empty() {
        candidates = threads
            .iter()
            .filter(|thread| thread_matches_query(thread, query))
            .cloned()
            .collect::<Vec<_>>();
    }

    match candidates.len() {
        0 => {
            inner
                .client
                .send_agent_text(format!(
                    "No threads found for `{query}`.\nUse `/threads` to list saved threads."
                ))
                .await;
        }
        1 => {
            let thread_id = candidates[0].id.clone();
            let response = inner
                .app
                .lock()
                .await
                .thread_read(ThreadReadParams {
                    thread_id,
                    include_turns: true,
                })
                .await?;
            inner
                .client
                .send_agent_text(format_thread_preview(&response.thread))
                .await;
        }
        _ => {
            let mut lines = vec![format!(
                "Multiple threads matched `{query}` ({} matches):",
                candidates.len()
            )];
            for thread in candidates.iter().take(12) {
                lines.push(format!(
                    "- `{}` | updated: {} | branch: {} | {}",
                    thread.id,
                    format_relative_timestamp(thread.updated_at),
                    thread
                        .git_info
                        .as_ref()
                        .and_then(|git| git.branch.as_deref())
                        .filter(|value| !value.is_empty())
                        .unwrap_or("-"),
                    thread_display_title(thread)
                ));
            }
            if candidates.len() > 12 {
                lines.push(format!("- ... {} more", candidates.len() - 12));
            }
            lines.push(
                "Use `/threads <thread_id>` to preview one, or `/resume <thread_id>` to resume."
                    .to_string(),
            );
            inner.client.send_agent_text(lines.join("\n")).await;
        }
    }

    Ok(StopReason::EndTurn)
}

fn format_thread_preview(thread: &Thread) -> String {
    let branch = thread
        .git_info
        .as_ref()
        .and_then(|git| git.branch.as_deref())
        .filter(|value| !value.is_empty())
        .unwrap_or("-");
    let mut lines = vec![
        format!("Thread preview: {}", thread_display_title(thread)),
        format!("- id: `{}`", thread.id),
        format!("- cwd: `{}`", thread.cwd.display()),
        format!("- branch: `{branch}`"),
        format!(
            "- created: {}",
            format_relative_timestamp(thread.created_at)
        ),
        format!(
            "- updated: {}",
            format_relative_timestamp(thread.updated_at)
        ),
        format!("- turns: {}", thread.turns.len()),
    ];

    let snippets = recent_thread_snippets(thread, 6);
    if snippets.is_empty() {
        lines.push(String::new());
        lines.push("No turn history was available for preview.".to_string());
    } else {
        lines.push(String::new());
        lines.push("Recent activity:".to_string());
        lines.extend(snippets.into_iter().map(|snippet| format!("- {snippet}")));
    }
    lines.push(String::new());
    lines.push(format!(
        "Use `/resume {}` to resume this thread.",
        thread.id
    ));
    lines.join("\n")
}

fn recent_thread_snippets(thread: &Thread, limit: usize) -> Vec<String> {
    let mut snippets = Vec::new();
    for turn in thread.turns.iter().rev() {
        for item in turn.items.iter().rev() {
            if let Some(snippet) = thread_item_snippet(item) {
                snippets.push(snippet);
                if snippets.len() >= limit {
                    snippets.reverse();
                    return snippets;
                }
            }
        }
    }
    snippets.reverse();
    snippets
}

fn thread_item_snippet(item: &ThreadItem) -> Option<String> {
    match item {
        ThreadItem::UserMessage { content, .. } => Some(format!(
            "User: {}",
            crate::thread::prompt_commands::normalize_preview(&user_input_summary(content))
        )),
        ThreadItem::AgentMessage { text, .. } => Some(format!(
            "Agent: {}",
            crate::thread::prompt_commands::normalize_preview(text)
        )),
        ThreadItem::Plan { text, .. } => Some(format!(
            "Plan: {}",
            crate::thread::prompt_commands::normalize_preview(text)
        )),
        ThreadItem::CommandExecution {
            command, status, ..
        } => Some(format!(
            "Command ({status:?}): {}",
            crate::thread::prompt_commands::normalize_preview(command)
        )),
        ThreadItem::FileChange {
            changes, status, ..
        } => Some(format!(
            "File change ({status:?}): {} file(s)",
            changes.len()
        )),
        ThreadItem::McpToolCall {
            server,
            tool,
            status,
            ..
        } => Some(format!("MCP ({status:?}): {server}/{tool}")),
        ThreadItem::WebSearch { query, .. } => Some(format!(
            "Web search: {}",
            crate::thread::prompt_commands::normalize_preview(query)
        )),
        ThreadItem::ImageGeneration { status, .. } => Some(format!("Image generation ({status})")),
        ThreadItem::Reasoning { .. }
        | ThreadItem::ImageView { .. }
        | ThreadItem::DynamicToolCall { .. }
        | ThreadItem::CollabAgentToolCall { .. }
        | ThreadItem::EnteredReviewMode { .. }
        | ThreadItem::ExitedReviewMode { .. }
        | ThreadItem::ContextCompaction { .. } => None,
    }
}

fn user_input_summary(content: &[UserInput]) -> String {
    let mut parts = Vec::new();
    for item in content {
        match item {
            UserInput::Text { text, .. } => parts.push(text.clone()),
            UserInput::Image { .. } | UserInput::LocalImage { .. } => {
                parts.push("[image]".to_string())
            }
            UserInput::Skill { name, .. } => parts.push(format!("[skill: {name}]")),
            UserInput::Mention { name, .. } => parts.push(format!("[mention: {name}]")),
        }
    }
    parts.join(" ")
}
