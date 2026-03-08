//! Session lifecycle flow: creation, capability bootstrap, resume, and teardown.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::session_config::{
    policy_to_mode, resolve_reasoning_effort, to_app_approval, to_app_sandbox_mode,
};
use super::{
    AppServerProcess, ClientCapabilities, Config, EditApprovalMode, Error, ListSessionsResponse,
    ModeKind, SessionClient, SessionId, Thread, ThreadInner, ThreadListParams, ThreadResumeParams,
    ThreadSortKey, ThreadStartParams,
};

impl Thread {
    // Start the app-server session first so later capability calls have a valid session id.
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
                approval_policy: Some(to_app_approval(*config.approval_policy.get())),
                sandbox: Some(to_app_sandbox_mode(config.sandbox_policy.get())),
                ..Default::default()
            })
            .await?;

        let session_id = SessionId::new(start.thread.id.clone());
        let models = app
            .model_list()
            .await
            .map(|response| response.data)
            .unwrap_or_default();
        let reasoning_effort =
            resolve_reasoning_effort(&models, &start.model, start.reasoning_effort);

        let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(0_u64);
        let thread = Thread {
            inner: tokio::sync::Mutex::new(ThreadInner {
                session_id: session_id.clone(),
                app,
                thread_id: start.thread.id,
                workspace_cwd: cwd,
                client: SessionClient::new(session_id.clone(), client_capabilities),
                approval_policy: start.approval_policy,
                sandbox_policy: start.sandbox.clone(),
                sandbox_mode: policy_to_mode(&start.sandbox),
                edit_approval_mode: EditApprovalMode::AutoApprove,
                collaboration_mode_kind: ModeKind::Default,
                current_model: start.model,
                reasoning_effort,
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
            }),
            cancel_tx,
        };

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

        let resume = app
            .thread_resume(ThreadResumeParams {
                thread_id: session_id.0.to_string(),
                model: config.model.clone(),
                model_provider: Some(config.model_provider_id.clone()),
                cwd: Some(cwd.to_string_lossy().to_string()),
                approval_policy: Some(to_app_approval(*config.approval_policy.get())),
                sandbox: Some(to_app_sandbox_mode(config.sandbox_policy.get())),
                ..Default::default()
            })
            .await?;

        let models = app
            .model_list()
            .await
            .map(|response| response.data)
            .unwrap_or_default();
        let reasoning_effort =
            resolve_reasoning_effort(&models, &resume.model, resume.reasoning_effort);
        let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(0_u64);

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
            }),
            cancel_tx,
        })
    }

    pub async fn list_sessions(
        config: &Config,
        cwd: Option<PathBuf>,
        cursor: Option<String>,
    ) -> Result<ListSessionsResponse, Error> {
        // ACP clients, including Zed, often call session/list without `cwd`.
        // Default to CLI-like resume behavior by showing sessions for the current workspace.
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
            })
            .await?;

        let sessions = response
            .data
            .into_iter()
            .filter_map(|thread| {
                if let Some(expected_cwd) = effective_cwd.as_ref()
                    && thread.cwd != *expected_cwd
                {
                    return None;
                }

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
