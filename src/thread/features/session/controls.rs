//! Обработчики slash-команд управления сессией (без `/resume` и `/undo`).
//! Сюда вынесены compact/archive/rename/fork ветки.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::thread::features::collab::remember_agent_label;
use crate::thread::features::resume::common::{
    format_relative_timestamp, list_all_threads_with_archived, thread_display_title,
};
use crate::thread::features::session::thread_switch::flush_thread_switch_transport_state;
use crate::thread::features::session::{
    session_info_title_update_from_unix, session_info_title_update_now,
};
use crate::thread::session_lifecycle::is_missing_rollout_thread_error;
use crate::thread::{
    Thread as SessionThread, ThreadInner, session_config::service_tier_override_from_session,
    turn_notify::notify_config_update,
};
use agent_client_protocol::{
    Error, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    SelectedPermissionOutcome, SessionUpdate, StopReason, ToolCallId, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use codex_app_server_protocol::{
    AskForApproval as AppAskForApproval, RateLimitSnapshot as AppRateLimitSnapshot,
    SandboxPolicy as AppSandboxPolicy, Thread as AppThread, ThreadArchiveParams,
    ThreadCompactStartParams, ThreadForkParams, ThreadSetNameParams, ThreadSortKey,
    ThreadStartParams, ThreadUnarchiveParams,
};
use codex_protocol::openai_models::ReasoningEffort;
use serde_json::json;
use tracing::warn;

struct ThreadSwitchState {
    approval_policy: AppAskForApproval,
    sandbox_policy: AppSandboxPolicy,
    model: String,
    model_provider: String,
    service_tier: Option<codex_protocol::config_types::ServiceTier>,
    reasoning_effort: Option<ReasoningEffort>,
}

impl SessionThread {
    pub(in crate::thread) async fn handle_archive_command_ext(
        &self,
        thread_id: Option<String>,
    ) -> Result<StopReason, Error> {
        let (selected, is_current_thread, app, client) = {
            let mut inner = self.inner.lock().await;
            let selected =
                resolve_thread_for_archive(&mut inner, thread_id.as_deref(), false).await?;
            let is_current_thread = selected
                .as_ref()
                .is_some_and(|thread| thread.id == inner.thread_id);
            (
                selected,
                is_current_thread,
                inner.app.clone(),
                inner.client.clone(),
            )
        };
        let Some(selected) = selected else {
            return Ok(StopReason::EndTurn);
        };
        let title = thread_display_title(&selected);

        if is_current_thread {
            flush_thread_switch_transport_state(&app).await?;
        }

        app.lock()
            .await
            .thread_archive(ThreadArchiveParams {
                thread_id: selected.id.clone(),
            })
            .await?;

        if is_current_thread {
            if let Err(error) = self.start_replacement_thread_ext().await {
                warn!(
                    thread_id = %selected.id,
                    error = %error,
                    "failed to start replacement thread after archiving current thread; attempting restore"
                );
                match app
                    .lock()
                    .await
                    .thread_unarchive(ThreadUnarchiveParams {
                        thread_id: selected.id.clone(),
                    })
                    .await
                {
                    Ok(_) => {
                        client
                            .send_agent_text(format!(
                                "Failed to start a fresh session after archiving `{title}`. Restored the original thread.\n\nError: {error}"
                            ))
                            .await;
                        return Ok(StopReason::EndTurn);
                    }
                    Err(unarchive_error) => {
                        return Err(Error::internal_error().data(format!(
                            "Archived current thread `{title}` but failed to start a fresh session ({error}) and failed to restore it ({unarchive_error})."
                        )));
                    }
                }
            }
            client
                .send_agent_text(format!(
                    "Archived current thread `{title}` and started a fresh session."
                ))
                .await;
        } else {
            client
                .send_agent_text(format!("Archived thread `{title}`."))
                .await;
        }

        Ok(StopReason::EndTurn)
    }

    pub(in crate::thread) async fn handle_fork_command_ext(
        &self,
        args: Option<String>,
    ) -> Result<StopReason, Error> {
        let (app, client, fork_params) = {
            let inner = self.inner.lock().await;
            if args.is_some() {
                inner.client.send_agent_text("Usage: `/fork`").await;
                return Ok(StopReason::EndTurn);
            }

            (
                inner.app.clone(),
                inner.client.clone(),
                ThreadForkParams {
                    thread_id: inner.thread_id.clone(),
                    model: Some(inner.current_model.clone()),
                    model_provider: Some(inner.current_model_provider.clone()),
                    service_tier: service_tier_override_from_session(inner.service_tier),
                    cwd: Some(inner.workspace_cwd.to_string_lossy().to_string()),
                    approval_policy: Some(inner.approval_policy),
                    sandbox: Some(inner.sandbox_mode),
                    config: inner.session_mcp_config_overrides.clone(),
                    ..Default::default()
                },
            )
        };

        flush_thread_switch_transport_state(&app).await?;
        let fork = match app.lock().await.thread_fork(fork_params).await {
            Ok(fork) => fork,
            Err(error) if is_missing_rollout_thread_error(&error) => {
                client
                    .send_agent_text(
                        "Current thread is not ready to fork yet. Send at least one prompt first, then try `/fork` again.",
                    )
                    .await;
                return Ok(StopReason::EndTurn);
            }
            Err(error) => return Err(error),
        };

        self.apply_thread_switch_ext(
            fork.thread,
            ThreadSwitchState {
                approval_policy: fork.approval_policy,
                sandbox_policy: fork.sandbox,
                model: fork.model,
                model_provider: fork.model_provider,
                service_tier: fork.service_tier,
                reasoning_effort: fork.reasoning_effort,
            },
            "handle_fork_command_ext",
        )
        .await?;
        client
            .send_agent_text(
                "Forked the current backend thread and switched this ACP session to the fork. Existing sidebar history remains visible because Zed does not clear it for in-place thread switches.",
            )
            .await;
        Ok(StopReason::EndTurn)
    }

    async fn start_replacement_thread_ext(&self) -> Result<(), Error> {
        let (app, start_params) = {
            let inner = self.inner.lock().await;
            (
                inner.app.clone(),
                ThreadStartParams {
                    model: Some(inner.current_model.clone()),
                    model_provider: Some(inner.current_model_provider.clone()),
                    service_tier: service_tier_override_from_session(inner.service_tier),
                    cwd: Some(inner.workspace_cwd.to_string_lossy().to_string()),
                    approval_policy: Some(inner.approval_policy),
                    sandbox: Some(inner.sandbox_mode),
                    config: inner.session_mcp_config_overrides.clone(),
                    ..Default::default()
                },
            )
        };

        let start = app.lock().await.thread_start(start_params).await?;
        self.apply_thread_switch_ext(
            start.thread,
            ThreadSwitchState {
                approval_policy: start.approval_policy,
                sandbox_policy: start.sandbox,
                model: start.model,
                model_provider: start.model_provider,
                service_tier: start.service_tier,
                reasoning_effort: start.reasoning_effort,
            },
            "start_replacement_thread_ext",
        )
        .await
    }

    async fn apply_thread_switch_ext(
        &self,
        thread: AppThread,
        state: ThreadSwitchState,
        sync_reason: &'static str,
    ) -> Result<(), Error> {
        let app = {
            let inner = self.inner.lock().await;
            inner.app.clone()
        };

        flush_thread_switch_transport_state(&app).await?;
        let account_rate_limits = match app.lock().await.get_account_rate_limits().await {
            Ok(response) => Some(response.rate_limits),
            Err(_) => None,
        };

        let mut inner = self.inner.lock().await;
        apply_thread_switch(&mut inner, thread, state, account_rate_limits, sync_reason).await
    }
}

pub(in crate::thread) async fn handle_compact_command(
    inner: &mut ThreadInner,
) -> Result<StopReason, Error> {
    let message = start_context_compaction(inner).await?;
    inner.client.send_agent_text(message).await;
    Ok(StopReason::EndTurn)
}

pub(in crate::thread) async fn start_context_compaction(
    inner: &mut ThreadInner,
) -> Result<String, Error> {
    if inner.compaction_in_progress {
        return Ok("Context compaction is already running.".to_string());
    }

    inner
        .app
        .lock()
        .await
        .thread_compact_start(ThreadCompactStartParams {
            thread_id: inner.thread_id.clone(),
        })
        .await?;
    inner.compaction_in_progress = true;
    // Статистика токенов может оставаться устаревшей (часто 100%) до следующего завершённого turn модели.
    // Сразу после /compact очищаем кэш usage, чтобы процент контекста не вводил в заблуждение.
    inner.last_used_tokens = None;
    inner.context_usage_source = None;
    notify_config_update(inner).await;
    Ok("Context compaction started. Wait for \"Context compacted.\" before sending the next prompt.".to_string())
}

pub(in crate::thread) async fn handle_unarchive_command(
    inner: &mut ThreadInner,
    thread_id: Option<String>,
) -> Result<StopReason, Error> {
    let Some(selected) = resolve_thread_for_archive(inner, thread_id.as_deref(), true).await?
    else {
        return Ok(StopReason::EndTurn);
    };
    let title = thread_display_title(&selected);

    inner
        .app
        .lock()
        .await
        .thread_unarchive(ThreadUnarchiveParams {
            thread_id: selected.id,
        })
        .await?;

    inner
        .client
        .send_agent_text(format!("Unarchived thread `{title}`."))
        .await;

    Ok(StopReason::EndTurn)
}

pub(in crate::thread) async fn handle_rename_command(
    inner: &mut ThreadInner,
    name: Option<String>,
) -> Result<StopReason, Error> {
    let Some(name) = name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        inner
            .client
            .send_agent_text("Usage: `/rename <new thread name>`")
            .await;
        return Ok(StopReason::EndTurn);
    };

    inner
        .app
        .lock()
        .await
        .thread_set_name(ThreadSetNameParams {
            thread_id: inner.thread_id.clone(),
            name: name.clone(),
        })
        .await?;

    inner
        .client
        .send_notification(SessionUpdate::SessionInfoUpdate(
            session_info_title_update_now(name.clone()),
        ))
        .await;
    inner
        .client
        .send_agent_text(format!("Thread renamed to `{name}`."))
        .await;
    Ok(StopReason::EndTurn)
}

async fn apply_thread_switch(
    inner: &mut ThreadInner,
    thread: AppThread,
    state: ThreadSwitchState,
    account_rate_limits: Option<AppRateLimitSnapshot>,
    sync_reason: &'static str,
) -> Result<(), Error> {
    inner.thread_id = thread.id.clone();
    inner.workspace_cwd = thread.cwd.clone();
    inner.approval_policy = state.approval_policy;
    inner.sandbox_policy = state.sandbox_policy.clone();
    inner.sandbox_mode = crate::thread::session_config::policy_to_mode(&state.sandbox_policy);
    inner.sync_sandbox_mode_from_policy(sync_reason);
    inner.current_model = state.model;
    inner.current_model_provider = state.model_provider;
    inner.service_tier = state.service_tier;
    inner.compaction_in_progress = false;
    inner.last_used_tokens = None;
    inner.total_token_usage = None;
    inner.context_window_size = None;
    inner.context_usage_source = None;
    inner.agent_labels = HashMap::new();
    remember_agent_label(
        &mut inner.agent_labels,
        inner.thread_id.clone(),
        thread.agent_nickname.clone(),
        thread.agent_role.clone(),
    );
    inner.carryover_plan_steps = None;
    inner.reset_turn_transient_state();
    inner.reasoning_effort = crate::thread::session_config::resolve_reasoning_effort(
        &inner.models,
        &inner.current_model,
        state.reasoning_effort,
    );
    if let Some(account_rate_limits) = account_rate_limits {
        inner.account_rate_limits = Some(account_rate_limits);
    }

    inner
        .client
        .send_notification(SessionUpdate::SessionInfoUpdate(
            session_info_title_update_from_unix(thread_display_title(&thread), thread.updated_at),
        ))
        .await;
    notify_config_update(inner).await;
    Ok(())
}

async fn resolve_thread_for_archive(
    inner: &mut ThreadInner,
    query: Option<&str>,
    archived: bool,
) -> Result<Option<AppThread>, Error> {
    let all_threads =
        list_all_threads_with_archived(inner, ThreadSortKey::UpdatedAt, None, None, archived)
            .await?;

    if all_threads.is_empty() {
        let message = if archived {
            "No archived threads found."
        } else {
            "No active threads found."
        };
        inner.client.send_agent_text(message).await;
        return Ok(None);
    }

    let normalized_query = query
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    if !archived
        && normalized_query.is_none()
        && let Some(current) = all_threads
            .iter()
            .find(|thread| thread.id == inner.thread_id)
    {
        return Ok(Some(current.clone()));
    }

    if let Some(query) = normalized_query.as_deref()
        && let Some(exact) = all_threads.iter().find(|thread| thread.id == query)
    {
        return Ok(Some(exact.clone()));
    }

    let candidates = match normalized_query.as_deref() {
        Some(query) => all_threads
            .into_iter()
            .filter(|thread| thread_matches_query(thread, query))
            .collect::<Vec<_>>(),
        None => all_threads,
    };

    if candidates.is_empty() {
        let message = if archived {
            format!(
                "No archived threads found for `{}`.",
                normalized_query.unwrap_or_default()
            )
        } else {
            format!(
                "No active threads found for `{}`.",
                normalized_query.unwrap_or_default()
            )
        };
        inner.client.send_agent_text(message).await;
        return Ok(None);
    }

    if candidates.len() == 1 {
        return Ok(Some(candidates[0].clone()));
    }

    pick_thread_from_candidates(inner, candidates, archived).await
}

async fn pick_thread_from_candidates(
    inner: &mut ThreadInner,
    candidates: Vec<AppThread>,
    archived: bool,
) -> Result<Option<AppThread>, Error> {
    let tool_call_id = format!(
        "{}-selector-{}",
        if archived { "unarchive" } else { "archive" },
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let title = if archived {
        format!(
            "Select archived thread to restore ({} match(es))",
            candidates.len()
        )
    } else {
        format!("Select thread to archive ({} match(es))", candidates.len())
    };

    let mut options = Vec::new();
    let mut id_by_option = HashMap::new();
    for (idx, thread) in candidates.iter().enumerate() {
        let option_id = format!("thread-select-{}", idx + 1);
        options.push(PermissionOption::new(
            option_id.clone(),
            thread_picker_label(thread),
            PermissionOptionKind::AllowOnce,
        ));
        id_by_option.insert(option_id, thread.id.clone());
    }
    options.push(PermissionOption::new(
        "thread-select-cancel",
        "Cancel",
        PermissionOptionKind::RejectOnce,
    ));

    let outcome = inner
        .client
        .request_permission(
            ToolCallUpdate::new(
                ToolCallId::new(tool_call_id),
                ToolCallUpdateFields::new()
                    .title(title)
                    .kind(ToolKind::Think)
                    .status(ToolCallStatus::Pending)
                    .content(vec![
                        "Search in the picker list. Open View Raw Input for full previews and paths."
                            .into(),
                    ])
                    .raw_input(thread_picker_raw_input(&candidates, archived)),
            ),
            options,
        )
        .await?;

    let selected_option_id = match outcome {
        RequestPermissionOutcome::Cancelled => return Ok(None),
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            option_id.0.to_string()
        }
        _ => return Ok(None),
    };

    let Some(selected_thread_id) = id_by_option.get(&selected_option_id) else {
        warn!(
            selected_option_id,
            "archive picker returned unknown option id"
        );
        return Ok(None);
    };

    Ok(candidates
        .into_iter()
        .find(|thread| thread.id == *selected_thread_id))
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

fn thread_picker_label(thread: &AppThread) -> String {
    let branch = thread
        .git_info
        .as_ref()
        .and_then(|git| git.branch.as_deref())
        .filter(|value| !value.is_empty())
        .unwrap_or("-");

    format!(
        "{} · {} · {}",
        format_relative_timestamp(thread.updated_at),
        branch,
        thread_display_title(thread)
    )
}

fn thread_picker_raw_input(candidates: &[AppThread], archived: bool) -> serde_json::Value {
    json!({
        "archived": archived,
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
    use super::thread_picker_raw_input;
    use codex_app_server_protocol::{SessionSource, Thread, ThreadStatus};
    use std::path::PathBuf;

    #[test]
    fn thread_picker_raw_input_keeps_original_preview_text() {
        let thread = Thread {
            id: "019-test".to_string(),
            preview: "line one\n\nline   two".to_string(),
            ephemeral: false,
            model_provider: "openai".to_string(),
            created_at: 10,
            updated_at: 20,
            status: ThreadStatus::Idle,
            path: None,
            cwd: PathBuf::from("/tmp/workspace"),
            cli_version: "0.0.0".to_string(),
            source: SessionSource::Cli,
            agent_nickname: None,
            agent_role: None,
            git_info: None,
            name: None,
            turns: Vec::new(),
        };

        let raw = thread_picker_raw_input(&[thread], false);
        assert_eq!(raw["threads"][0]["preview"], "line one\n\nline   two");
    }
}
