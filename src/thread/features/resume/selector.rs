//! Выбор thread для `/resume`: фильтрация, picker-карточка и делегирование в apply.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use agent_client_protocol::{
    Error, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    SelectedPermissionOutcome, StopReason, ToolCallId, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};
use codex_app_server_protocol::{Thread as AppThread, ThreadSortKey};
use serde_json::json;
use tracing::warn;

use super::common::{format_relative_timestamp, list_all_threads, thread_display_title};
use crate::thread::{RESUME_CANCEL_OPTION_ID, Thread, ThreadInner};

enum ResumeSelection {
    Stop(StopReason),
    Thread(String),
}

impl Thread {
    pub(in crate::thread) async fn handle_resume_selector_command_ext(
        &self,
        query: Option<&str>,
        include_history: bool,
    ) -> Result<StopReason, Error> {
        let selection = {
            let mut inner = self.inner.lock().await;
            resolve_resume_selection(&mut inner, query).await?
        };

        match selection {
            ResumeSelection::Stop(stop_reason) => Ok(stop_reason),
            ResumeSelection::Thread(thread_id) => {
                self.resume_thread_ext(&thread_id, include_history).await
            }
        }
    }
}

async fn resolve_resume_selection(
    inner: &mut ThreadInner,
    query: Option<&str>,
) -> Result<ResumeSelection, Error> {
    let all_threads = list_all_threads(inner, ThreadSortKey::UpdatedAt, None, None).await?;

    if all_threads.is_empty() {
        inner
            .client
            .send_agent_text("No saved threads found. Create one prompt first.")
            .await;
        return Ok(ResumeSelection::Stop(StopReason::EndTurn));
    }

    let normalized_query = query
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .map(ToString::to_string);

    if let Some(query) = normalized_query.as_deref()
        && all_threads.iter().any(|thread| thread.id == query)
    {
        return Ok(ResumeSelection::Thread(query.to_string()));
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
        return Ok(ResumeSelection::Stop(StopReason::EndTurn));
    }

    if candidates.len() == 1 {
        return Ok(ResumeSelection::Thread(candidates[0].id.clone()));
    }

    show_resume_picker(inner, candidates, normalized_query.as_deref()).await
}

async fn show_resume_picker(
    inner: &mut ThreadInner,
    candidates: Vec<AppThread>,
    query: Option<&str>,
) -> Result<ResumeSelection, Error> {
    let total = candidates.len();

    let title = match query {
        Some(query) => format!("Resume thread for `{query}` ({total} match(es))"),
        None => format!("Resume thread from current workspace ({total} match(es))"),
    };
    let raw_input = resume_picker_raw_input(&candidates, query);

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
                ToolCallId::new(next_resume_selector_tool_call_id()),
                ToolCallUpdateFields::new()
                    .title(title)
                    .kind(ToolKind::Think)
                    .status(ToolCallStatus::Pending)
                    .content(vec![
                        "Search in the picker list. Open View Raw Input for full previews and paths."
                            .into(),
                    ])
                    .raw_input(raw_input),
            ),
            options,
        )
        .await?;

    let selected_option_id = match outcome {
        RequestPermissionOutcome::Cancelled => {
            inner.client.send_agent_text("Resume cancelled.").await;
            return Ok(ResumeSelection::Stop(StopReason::EndTurn));
        }
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            option_id.0.to_string()
        }
        _ => {
            inner.client.send_agent_text("Resume cancelled.").await;
            return Ok(ResumeSelection::Stop(StopReason::EndTurn));
        }
    };

    if selected_option_id == RESUME_CANCEL_OPTION_ID {
        inner.client.send_agent_text("Resume cancelled.").await;
        return Ok(ResumeSelection::Stop(StopReason::EndTurn));
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
        return Ok(ResumeSelection::Stop(StopReason::EndTurn));
    };

    Ok(ResumeSelection::Thread(selected_thread_id))
}

fn next_resume_selector_tool_call_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("resume-selector-{nanos}")
}

fn thread_matches_query(thread: &AppThread, query: &str) -> bool {
    if thread.id.contains(query) {
        return true;
    }
    let needle = query.to_lowercase();
    thread.preview.to_lowercase().contains(&needle)
        || thread
            .name
            .as_ref()
            .is_some_and(|name| name.to_lowercase().contains(&needle))
}

fn format_resume_option_label(thread: &AppThread) -> String {
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
        thread_display_title(thread),
    )
}

fn resume_picker_raw_input(candidates: &[AppThread], query: Option<&str>) -> serde_json::Value {
    json!({
        "query": query,
        "count": candidates.len(),
        "threads": candidates.iter().map(|thread| {
            json!({
                "id": thread.id,
                "cwd": thread.cwd,
                "branch": thread
                    .git_info
                    .as_ref()
                    .and_then(|git| git.branch.as_deref())
                    .filter(|value| !value.is_empty())
                    .unwrap_or("-"),
                "created_at": thread.created_at,
                "created_at_relative": format_relative_timestamp(thread.created_at),
                "updated_at": thread.updated_at,
                "updated_at_relative": format_relative_timestamp(thread.updated_at),
                "name": thread.name,
                "preview": thread.preview,
                "display_title": thread_display_title(thread),
            })
        }).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use super::resume_picker_raw_input;
    use codex_app_server_protocol::{GitInfo, SessionSource, Thread as AppThread, ThreadStatus};
    use std::path::PathBuf;

    #[test]
    fn raw_input_keeps_full_preview_text() {
        let thread = AppThread {
            id: "019-test".to_string(),
            preview: "line one\n\nline   two".to_string(),
            ephemeral: false,
            model_provider: "openai".to_string(),
            created_at: 10,
            updated_at: 20,
            status: ThreadStatus::Idle,
            path: None,
            cwd: PathBuf::from("/tmp/workspace"),
            cli_version: "0.1.0".to_string(),
            source: SessionSource::AppServer,
            agent_nickname: None,
            agent_role: None,
            git_info: Some(GitInfo {
                sha: None,
                branch: Some("main".to_string()),
                origin_url: None,
            }),
            name: None,
            turns: vec![],
        };

        let raw = resume_picker_raw_input(&[thread], Some("019"));
        assert_eq!(raw["count"], 1);
        assert_eq!(raw["threads"][0]["preview"], "line one\n\nline   two");
        assert_eq!(raw["threads"][0]["display_title"], "line one line two");
        assert_eq!(raw["threads"][0]["branch"], "main");
    }
}
