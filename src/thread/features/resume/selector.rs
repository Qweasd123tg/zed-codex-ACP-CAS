//! Выбор thread для `/resume`: фильтрация, picker-карточка и делегирование в apply.

use std::collections::HashMap;

use agent_client_protocol::{
    Error, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    SelectedPermissionOutcome, StopReason, ToolCallId, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};
use codex_app_server_protocol::{Thread, ThreadListParams, ThreadSortKey};
use tracing::warn;

use crate::thread::prompt_commands::normalize_preview;
use crate::thread::{RESUME_CANCEL_OPTION_ID, RESUME_PICK_LIMIT, ThreadInner};

pub(in crate::thread) async fn handle_resume_selector_command(
    inner: &mut ThreadInner,
    query: Option<&str>,
) -> Result<StopReason, Error> {
    let all_threads = inner
        .app
        .thread_list(ThreadListParams {
            cursor: None,
            limit: Some(100),
            sort_key: Some(ThreadSortKey::UpdatedAt),
            model_providers: None,
            source_kinds: None,
            archived: Some(false),
        })
        .await?
        .data;

    if all_threads.is_empty() {
        inner
            .client
            .send_agent_text("No saved threads found. Create one prompt first.")
            .await;
        return Ok(StopReason::EndTurn);
    }

    let normalized_query = query
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .map(ToString::to_string);

    if let Some(query) = normalized_query.as_deref()
        && all_threads.iter().any(|thread| thread.id == query)
    {
        return super::apply::handle_resume_command(inner, query).await;
    }

    let candidates = if let Some(query) = normalized_query.as_deref() {
        let mut in_workspace = all_threads
            .iter()
            .filter(|thread| {
                thread.cwd == inner.workspace_cwd && thread_matches_query(thread, query)
            })
            .cloned()
            .collect::<Vec<_>>();
        if in_workspace.is_empty() {
            in_workspace = all_threads
                .iter()
                .filter(|thread| thread_matches_query(thread, query))
                .cloned()
                .collect::<Vec<_>>();
        }
        in_workspace
    } else {
        all_threads
            .iter()
            .filter(|thread| thread.cwd == inner.workspace_cwd)
            .cloned()
            .collect::<Vec<_>>()
    };

    if candidates.is_empty() {
        let message = if let Some(query) = normalized_query {
            format!(
                "No threads found for `{query}`.\nTry `/resume` for current workspace threads or `/threads` to list all."
            )
        } else {
            format!(
                "No saved threads for current workspace `{}`.\nUse `/threads` to list all threads.",
                inner.workspace_cwd.display()
            )
        };
        inner.client.send_agent_text(message).await;
        return Ok(StopReason::EndTurn);
    }

    if candidates.len() == 1 {
        return super::apply::handle_resume_command(inner, &candidates[0].id).await;
    }

    show_resume_picker(inner, candidates, normalized_query.as_deref()).await
}

async fn show_resume_picker(
    inner: &mut ThreadInner,
    mut candidates: Vec<Thread>,
    query: Option<&str>,
) -> Result<StopReason, Error> {
    let total = candidates.len();
    candidates.truncate(RESUME_PICK_LIMIT);

    let title = match query {
        Some(query) => format!("Resume thread for `{query}`"),
        None => "Resume thread from current workspace".to_string(),
    };

    let mut lines = Vec::new();
    lines.push(format!("Select a thread to resume ({total} match(es)):"));
    if total > RESUME_PICK_LIMIT {
        lines.push(format!(
            "Showing the newest {RESUME_PICK_LIMIT} matches. Narrow with `/resume <partial_id>` if needed."
        ));
    }
    for thread in &candidates {
        lines.push(format!(
            "- `{}` | {} | cwd: `{}` | updated_at: {}",
            thread.id,
            normalize_preview(&thread.preview),
            thread.cwd.display(),
            thread.updated_at
        ));
    }

    let mut options = Vec::new();
    let mut id_by_option = HashMap::new();
    for (idx, thread) in candidates.into_iter().enumerate() {
        let option_id = format!("resume-thread-{}", idx + 1);
        let label = format!(
            "{} · {}",
            shorten_thread_id(&thread.id),
            normalize_preview(&thread.preview)
        );
        options.push(PermissionOption::new(
            option_id.clone(),
            label,
            PermissionOptionKind::AllowOnce,
        ));
        id_by_option.insert(option_id, thread.id);
    }
    options.push(PermissionOption::new(
        RESUME_CANCEL_OPTION_ID,
        "Cancel",
        PermissionOptionKind::RejectOnce,
    ));

    let outcome = inner
        .client
        .request_permission(
            ToolCallUpdate::new(
                ToolCallId::new("resume-selector"),
                ToolCallUpdateFields::new()
                    .title(title)
                    .kind(ToolKind::Think)
                    .status(ToolCallStatus::Pending)
                    .content(vec![lines.join("\n").into()]),
            ),
            options,
        )
        .await?;

    let selected_option_id = match outcome {
        RequestPermissionOutcome::Cancelled => {
            inner.client.send_agent_text("Resume cancelled.").await;
            return Ok(StopReason::EndTurn);
        }
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            option_id.0.to_string()
        }
        _ => {
            inner.client.send_agent_text("Resume cancelled.").await;
            return Ok(StopReason::EndTurn);
        }
    };

    if selected_option_id == RESUME_CANCEL_OPTION_ID {
        inner.client.send_agent_text("Resume cancelled.").await;
        return Ok(StopReason::EndTurn);
    }

    let Some(selected_thread_id) = id_by_option.get(&selected_option_id).cloned() else {
        warn!(
            selected_option_id,
            "resume selector returned unknown option id"
        );
        inner
            .client
            .send_agent_text("Could not resolve selected thread. Run `/resume` again.")
            .await;
        return Ok(StopReason::EndTurn);
    };

    super::apply::handle_resume_command(inner, &selected_thread_id).await
}

fn thread_matches_query(thread: &Thread, query: &str) -> bool {
    if thread.id.contains(query) {
        return true;
    }
    let needle = query.to_lowercase();
    thread.preview.to_lowercase().contains(&needle)
}

fn shorten_thread_id(thread_id: &str) -> String {
    if thread_id.chars().count() <= 12 {
        thread_id.to_string()
    } else {
        format!("{}…", thread_id.chars().take(12).collect::<String>())
    }
}
