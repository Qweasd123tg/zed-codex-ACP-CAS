//! Поток старта и остановки сессии: создание session, bootstrap capability и очистка.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::session_config::{
    policy_to_mode, resolve_reasoning_effort, to_app_approval, to_app_sandbox_mode,
};
use super::{
    AppServerProcess, ClientCapabilities, Config, EditApprovalMode, Error, ListSessionsResponse,
    ModeKind, SessionClient, SessionId, Thread, ThreadInner, ThreadListParams, ThreadResumeParams,
    ThreadSortKey, ThreadStartParams,
};
use crate::thread::features::collab::remember_agent_label;
use codex_app_server_protocol::ThreadStartResponse;
use tracing::warn;

const RESUME_STARTUP_RETRY_ATTEMPTS: usize = 6;
const RESUME_STARTUP_RETRY_DELAY_MS: u64 = 300;

fn is_retryable_missing_rollout_resume_error(error: &Error) -> bool {
    let message = error.to_string();
    message.contains("no rollout found for thread id")
}

pub(in crate::thread) async fn thread_resume_with_startup_retry(
    app: &mut AppServerProcess,
    params: ThreadResumeParams,
) -> Result<codex_app_server_protocol::ThreadResumeResponse, Error> {
    for attempt in 0..=RESUME_STARTUP_RETRY_ATTEMPTS {
        match app.thread_resume(params.clone()).await {
            Ok(response) => return Ok(response),
            Err(error)
                if is_retryable_missing_rollout_resume_error(&error)
                    && attempt < RESUME_STARTUP_RETRY_ATTEMPTS =>
            {
                let retry_number = attempt + 1;
                warn!(
                    retry_number,
                    thread_id = params.thread_id,
                    "thread/resume reported missing rollout during startup; retrying"
                );
                tokio::time::sleep(Duration::from_millis(
                    RESUME_STARTUP_RETRY_DELAY_MS * retry_number as u64,
                ))
                .await;
            }
            Err(error) => return Err(error),
        }
    }

    unreachable!("retry loop must return on success or final error")
}

impl Thread {
    async fn build_started_thread(
        session_id: SessionId,
        cwd: PathBuf,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
        mut app: AppServerProcess,
        start: ThreadStartResponse,
    ) -> Self {
        let models = app
            .model_list()
            .await
            .map(|response| response.data)
            .unwrap_or_default();
        let reasoning_effort =
            resolve_reasoning_effort(&models, &start.model, start.reasoning_effort);

        let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(0_u64);
        let mut agent_labels = HashMap::new();
        remember_agent_label(
            &mut agent_labels,
            start.thread.id.clone(),
            start.thread.agent_nickname.clone(),
            start.thread.agent_role.clone(),
        );

        Thread {
            inner: tokio::sync::Mutex::new(ThreadInner {
                session_id: session_id.clone(),
                app,
                thread_id: start.thread.id,
                workspace_cwd: cwd,
                client: SessionClient::new(session_id, client_capabilities),
                approval_policy: start.approval_policy,
                sandbox_policy: start.sandbox.clone(),
                sandbox_mode: policy_to_mode(&start.sandbox),
                edit_approval_mode: EditApprovalMode::AutoApprove,
                collaboration_mode_kind: ModeKind::Default,
                current_model: start.model,
                reasoning_effort,
                agent_labels,
                compaction_in_progress: false,
                last_used_tokens: None,
                context_window_size: None,
                models,
                active_turn_id: None,
                active_turn_mode_kind: None,
                active_turn_saw_plan_item: false,
                active_turn_saw_plan_delta: false,
                started_tool_calls: HashSet::new(),
                completed_turn_ids: HashSet::new(),
                turn_plan_updates_seen: HashSet::new(),
                fallback_plan: None,
                file_change_locations: HashMap::new(),
                file_change_started_changes: HashMap::new(),
                file_change_before_contents: HashMap::new(),
                latest_turn_diff: None,
                file_change_paths_this_turn: HashSet::new(),
                synced_paths_this_turn: HashSet::new(),
                last_plan_steps: Vec::new(),
                carryover_plan_steps: None,
                replay_turns: vec![],
                turn_last_progress_at: std::time::Instant::now(),
                turn_reconnect_warning_count: 0,
                turn_reconnect_retry_limit_hit: false,
            }),
            cancel_tx,
        }
    }

    pub(crate) async fn start_session_for_existing_session_id(
        session_id: SessionId,
        config: &Config,
        cwd: PathBuf,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
    ) -> Result<Self, Error> {
        let mut app = AppServerProcess::spawn("codex").await?;
        app.initialize("codex-acp-cas", "Codex ACP CAS").await?;

        let start = app
            .thread_start(ThreadStartParams {
                model: config.model.clone(),
                model_provider: Some(config.model_provider_id.clone()),
                cwd: Some(cwd.to_string_lossy().to_string()),
                approval_policy: Some(to_app_approval(*config.permissions.approval_policy.get())),
                sandbox: Some(to_app_sandbox_mode(config.permissions.sandbox_policy.get())),
                ..Default::default()
            })
            .await?;

        Ok(Self::build_started_thread(session_id, cwd, client_capabilities, app, start).await)
    }

    // Сначала запускаем сессию app-server, чтобы последующие capability-вызовы имели валидный session id.
    pub async fn start_session(
        config: &Config,
        cwd: PathBuf,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
    ) -> Result<(SessionId, Self), Error> {
        let mut app = AppServerProcess::spawn("codex").await?;
        app.initialize("codex-acp-cas", "Codex ACP CAS").await?;

        let start = app
            .thread_start(ThreadStartParams {
                model: config.model.clone(),
                model_provider: Some(config.model_provider_id.clone()),
                cwd: Some(cwd.to_string_lossy().to_string()),
                approval_policy: Some(to_app_approval(*config.permissions.approval_policy.get())),
                sandbox: Some(to_app_sandbox_mode(config.permissions.sandbox_policy.get())),
                ..Default::default()
            })
            .await?;

        let session_id = SessionId::new(start.thread.id.clone());
        let thread =
            Self::build_started_thread(session_id.clone(), cwd, client_capabilities, app, start)
                .await;

        Ok((session_id, thread))
    }

    pub async fn resume_session(
        session_id: SessionId,
        config: &Config,
        cwd: PathBuf,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
    ) -> Result<Self, Error> {
        let mut app = AppServerProcess::spawn("codex").await?;
        app.initialize("codex-acp-cas", "Codex ACP CAS").await?;

        let resume_params = ThreadResumeParams {
            thread_id: session_id.0.to_string(),
            model: config.model.clone(),
            model_provider: Some(config.model_provider_id.clone()),
            cwd: Some(cwd.to_string_lossy().to_string()),
            approval_policy: Some(to_app_approval(*config.permissions.approval_policy.get())),
            sandbox: Some(to_app_sandbox_mode(config.permissions.sandbox_policy.get())),
            ..Default::default()
        };

        let resume = match thread_resume_with_startup_retry(&mut app, resume_params.clone()).await {
            Ok(resume) => resume,
            Err(error) if is_retryable_missing_rollout_resume_error(&error) => {
                warn!(
                    requested_thread_id = resume_params.thread_id,
                    "resume source is unavailable or not materialized; starting a fresh backend thread for this ACP session"
                );
                return Self::start_session_for_existing_session_id(
                    session_id,
                    config,
                    cwd,
                    client_capabilities,
                )
                .await;
            }
            Err(error) => return Err(error),
        };

        let models = app
            .model_list()
            .await
            .map(|response| response.data)
            .unwrap_or_default();
        let reasoning_effort =
            resolve_reasoning_effort(&models, &resume.model, resume.reasoning_effort);
        let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(0_u64);
        let mut agent_labels = HashMap::new();
        remember_agent_label(
            &mut agent_labels,
            resume.thread.id.clone(),
            resume.thread.agent_nickname.clone(),
            resume.thread.agent_role.clone(),
        );

        Ok(Thread {
            inner: tokio::sync::Mutex::new(ThreadInner {
                session_id: session_id.clone(),
                app,
                thread_id: resume.thread.id,
                workspace_cwd: cwd,
                client: SessionClient::new(session_id, client_capabilities),
                approval_policy: resume.approval_policy,
                sandbox_policy: resume.sandbox.clone(),
                sandbox_mode: policy_to_mode(&resume.sandbox),
                edit_approval_mode: EditApprovalMode::AutoApprove,
                collaboration_mode_kind: ModeKind::Default,
                current_model: resume.model,
                reasoning_effort,
                agent_labels,
                compaction_in_progress: false,
                last_used_tokens: None,
                context_window_size: None,
                models,
                active_turn_id: None,
                active_turn_mode_kind: None,
                active_turn_saw_plan_item: false,
                active_turn_saw_plan_delta: false,
                started_tool_calls: HashSet::new(),
                completed_turn_ids: HashSet::new(),
                turn_plan_updates_seen: HashSet::new(),
                fallback_plan: None,
                file_change_locations: HashMap::new(),
                file_change_started_changes: HashMap::new(),
                file_change_before_contents: HashMap::new(),
                latest_turn_diff: None,
                file_change_paths_this_turn: HashSet::new(),
                synced_paths_this_turn: HashSet::new(),
                last_plan_steps: Vec::new(),
                carryover_plan_steps: None,
                replay_turns: resume.thread.turns,
                turn_last_progress_at: std::time::Instant::now(),
                turn_reconnect_warning_count: 0,
                turn_reconnect_retry_limit_hit: false,
            }),
            cancel_tx,
        })
    }

    pub async fn list_sessions(
        config: &Config,
        cwd: Option<PathBuf>,
        cursor: Option<String>,
    ) -> Result<ListSessionsResponse, Error> {
        // ACP-клиенты (в т.ч. Zed) часто вызывают session/list без cwd.
        // По умолчанию ведём себя как CLI resume: показываем сессии текущего workspace.
        let effective_cwd = cwd.or_else(|| Some(config.cwd.clone()));

        let mut app = AppServerProcess::spawn("codex").await?;
        app.initialize("codex-acp-cas", "Codex ACP CAS").await?;

        let response = app
            .thread_list(ThreadListParams {
                cursor,
                limit: Some(25),
                sort_key: Some(ThreadSortKey::UpdatedAt),
                model_providers: None,
                source_kinds: None,
                archived: Some(false),
                cwd: effective_cwd
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
                search_term: None,
            })
            .await?;

        let sessions = response
            .data
            .into_iter()
            .filter_map(|thread| {
                Some(
                    agent_client_protocol::SessionInfo::new(SessionId::new(thread.id), thread.cwd)
                        .title(Some(thread.preview))
                        .updated_at(Some(thread.updated_at.to_string())),
                )
            })
            .collect();

        Ok(ListSessionsResponse::new(sessions).next_cursor(response.next_cursor))
    }
}

#[cfg(test)]
mod tests {
    use super::is_retryable_missing_rollout_resume_error;
    use agent_client_protocol::Error;

    #[test]
    fn detects_retryable_missing_rollout_resume_error() {
        let error = Error::internal_error()
            .data("thread/resume failed: no rollout found for thread id 019-test (code -32600)");
        assert!(is_retryable_missing_rollout_resume_error(&error));
    }

    #[test]
    fn ignores_other_resume_errors() {
        let error = Error::internal_error().data("thread/resume failed: auth required");
        assert!(!is_retryable_missing_rollout_resume_error(&error));
    }

    #[test]
    fn detects_retryable_missing_rollout_even_in_wrapped_error_text() {
        let error = Error::internal_error()
            .data("Internal error: \"no rollout found for thread id 019-test\"");
        assert!(is_retryable_missing_rollout_resume_error(&error));
    }
}
