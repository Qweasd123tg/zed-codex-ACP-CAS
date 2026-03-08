//! Thread selection for `/resume`: filtering, picker UI, and delegation to apply.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

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
    include_history: bool,
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
        return super::apply::handle_resume_command(inner, query, include_history).await;
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
        return super::apply::handle_resume_command(inner, &candidates[0].id, include_history)
            .await;
    }

    show_resume_picker(
        inner,
        candidates,
        normalized_query.as_deref(),
        include_history,
    )
    .await
}

async fn show_resume_picker(
    inner: &mut ThreadInner,
    mut candidates: Vec<Thread>,
    query: Option<&str>,
    include_history: bool,
) -> Result<StopReason, Error> {
    let total = candidates.len();
    candidates.truncate(RESUME_PICK_LIMIT);

    let title = match query {
        Some(query) => format!("Resume thread for `{query}` ({total} match(es))"),
        None => format!("Resume thread from current workspace ({total} match(es))"),
    };
    let hint = if total > RESUME_PICK_LIMIT {
        Some(format!(
            "Showing newest {RESUME_PICK_LIMIT}. Narrow with `/resume <partial_id>`."
        ))
    } else {
        None
    };

    let mut options = Vec::new();
    let mut id_by_option = HashMap::new();
    for (idx, thread) in candidates.into_iter().enumerate() {
        let option_id = format!("resume-thread-{}", idx + 1);
        let label = format_resume_option_label(&thread);
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
                    .content(hint.map_or_else(Vec::new, |line| vec![line.into()])),
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

    super::apply::handle_resume_command(inner, &selected_thread_id, include_history).await
}

fn thread_matches_query(thread: &Thread, query: &str) -> bool {
    if thread.id.contains(query) {
        return true;
    }
    let needle = query.to_lowercase();
    thread.preview.to_lowercase().contains(&needle)
}

fn format_resume_option_label(thread: &Thread) -> String {
    let branch = thread
        .git_info
        .as_ref()
        .and_then(|git| git.branch.as_deref())
        .filter(|value| !value.is_empty())
        .unwrap_or("-");

    format!(
        "created {} · updated {} · {} · {}",
        format_relative_timestamp(thread.created_at),
        format_relative_timestamp(thread.updated_at),
        branch,
        shorten_preview(&normalize_preview(&thread.preview), 72),
    )
}

fn shorten_preview(preview: &str, max_chars: usize) -> String {
    let chars = preview.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return preview.to_string();
    }

    let keep = max_chars.saturating_sub(1);
    format!("{}…", chars[..keep].iter().collect::<String>())
}

fn format_relative_timestamp(unix_seconds: i64) -> String {
    if unix_seconds <= 0 {
        return "-".to_string();
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();

    if unix_seconds > now {
        return format_future_duration((unix_seconds - now) as u64);
    }

    format_past_duration((now - unix_seconds) as u64)
}

fn format_past_duration(delta: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    const MONTH: u64 = 30 * DAY;
    const YEAR: u64 = 365 * DAY;

    if delta < MINUTE {
        "just now".to_string()
    } else if delta < HOUR {
        format!("{}m ago", delta / MINUTE)
    } else if delta < DAY {
        format!("{}h ago", delta / HOUR)
    } else if delta < MONTH {
        format!("{}d ago", delta / DAY)
    } else if delta < YEAR {
        format!("{}mo ago", delta / MONTH)
    } else {
        format!("{}y ago", delta / YEAR)
    }
}

fn format_future_duration(delta: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;

    if delta < MINUTE {
        "soon".to_string()
    } else if delta < HOUR {
        format!("in {}m", delta / MINUTE)
    } else if delta < DAY {
        format!("in {}h", delta / HOUR)
    } else {
        format!("in {}d", delta / DAY)
    }
}
