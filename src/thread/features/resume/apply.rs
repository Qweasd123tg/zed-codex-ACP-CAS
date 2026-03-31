//! Применение выбранного thread: `thread_resume`, sync конфигурации и UI-уведомление.

use std::collections::HashMap;
use std::path::PathBuf;

use agent_client_protocol::{Error, SessionInfoUpdate, SessionUpdate, StopReason};
use codex_app_server_protocol::{ThreadResumeParams, Turn as AppTurn};

use crate::thread::features::collab::CollabAgentLabel;
use crate::thread::features::collab::{remember_agent_label, warm_agent_labels_for_turns};
use crate::thread::features::resume::common::thread_display_title;
use crate::thread::features::session::thread_switch::flush_thread_switch_transport_state;
use crate::thread::session_lifecycle::{
    load_session_skills_summary_for_cwd, thread_resume_with_startup_retry,
};
use crate::thread::session_usage_cache::restore_cached_context_usage;
use crate::thread::{
    ContextUsageSource, SessionClient, Thread, ThreadInner, replay, session_config, turn_notify,
};

enum ResumeApplyOutcome {
    NoReplay,
    Replay(ResumeReplayData),
}

struct ResumeReplayData {
    client: SessionClient,
    workspace_cwd: PathBuf,
    agent_labels: HashMap<String, CollabAgentLabel>,
    turns: Vec<AppTurn>,
}

impl Thread {
    pub(in crate::thread) async fn resume_thread_ext(
        &self,
        thread_id: &str,
        include_history: bool,
    ) -> Result<StopReason, Error> {
        let outcome = {
            let mut inner = self.inner.lock().await;
            apply_resumed_thread(&mut inner, thread_id, include_history).await?
        };

        if let ResumeApplyOutcome::Replay(replay_data) = outcome {
            replay::replay_turns(
                &replay_data.client,
                &replay_data.workspace_cwd,
                &replay_data.agent_labels,
                replay_data.turns,
            )
            .await;

            let mut inner = self.inner.lock().await;
            inner.history_replay_in_progress = false;
            turn_notify::notify_config_update(&inner).await;
        }

        Ok(StopReason::EndTurn)
    }
}

pub(in crate::thread) async fn handle_resume_command(
    inner: &mut ThreadInner,
    thread_id: &str,
    include_history: bool,
) -> Result<StopReason, Error> {
    match apply_resumed_thread(inner, thread_id, include_history).await? {
        ResumeApplyOutcome::NoReplay => {}
        ResumeApplyOutcome::Replay(replay_data) => {
            replay::replay_turns(
                &replay_data.client,
                &replay_data.workspace_cwd,
                &replay_data.agent_labels,
                replay_data.turns,
            )
            .await;
            inner.history_replay_in_progress = false;
            turn_notify::notify_config_update(inner).await;
        }
    }
    Ok(StopReason::EndTurn)
}

async fn apply_resumed_thread(
    inner: &mut ThreadInner,
    thread_id: &str,
    include_history: bool,
) -> Result<ResumeApplyOutcome, Error> {
    if include_history && inner.history_replay_in_progress {
        return Err(Error::invalid_params().data(
            "history replay is still running; wait for it to finish before resuming another thread",
        ));
    }

    flush_thread_switch_transport_state(inner).await?;

    let resume = thread_resume_with_startup_retry(
        &mut inner.app,
        ThreadResumeParams {
            thread_id: thread_id.to_string(),
            model: Some(inner.current_model.clone()),
            model_provider: Some(inner.current_model_provider.clone()),
            cwd: Some(inner.workspace_cwd.to_string_lossy().to_string()),
            approval_policy: Some(inner.approval_policy),
            sandbox: Some(inner.sandbox_mode),
            config: inner.session_mcp_config_overrides.clone(),
            ..Default::default()
        },
    )
    .await?;

    flush_thread_switch_transport_state(inner).await?;

    inner.thread_id = resume.thread.id.clone();
    inner.workspace_cwd = resume.thread.cwd.clone();
    inner.approval_policy = resume.approval_policy;
    inner.sandbox_policy = resume.sandbox.clone();
    inner.sandbox_mode = session_config::policy_to_mode(&resume.sandbox);
    inner.sync_sandbox_mode_from_policy("handle_resume_command");
    inner.current_model = resume.model;
    inner.current_model_provider = resume.model_provider;
    inner.compaction_in_progress = false;
    inner.total_token_usage = None;
    let cached_context_usage = restore_cached_context_usage(
        &inner.context_usage_cache_path,
        &resume.thread.id,
        &resume.thread.turns,
    );
    inner.last_used_tokens = cached_context_usage.map(|(used, _)| used);
    inner.context_window_size = cached_context_usage.map(|(_, size)| size);
    inner.context_usage_source = cached_context_usage.map(|_| ContextUsageSource::Cached);
    inner.agent_labels.clear();
    remember_agent_label(
        &mut inner.agent_labels,
        inner.thread_id.clone(),
        resume.thread.agent_nickname.clone(),
        resume.thread.agent_role.clone(),
    );
    inner.carryover_plan_steps = None;
    inner.reset_turn_transient_state();

    if let Ok(models) = inner.app.model_list().await {
        inner.models = models.data;
    }
    match inner.app.get_account_rate_limits().await {
        Ok(response) => {
            inner.account_rate_limits = Some(response.rate_limits);
        }
        Err(_) => {
            inner.account_rate_limits = None;
        }
    }
    flush_thread_switch_transport_state(inner).await?;
    inner.reasoning_effort = session_config::resolve_reasoning_effort(
        &inner.models,
        &inner.current_model,
        resume.reasoning_effort,
    );
    inner.session_skills_summary = load_session_skills_summary_for_cwd(
        &inner.codex_home,
        inner.bundled_skills_enabled,
        &inner.workspace_cwd,
    )
    .await;
    inner
        .client
        .send_notification(SessionUpdate::SessionInfoUpdate(
            SessionInfoUpdate::new().title(thread_display_title(&resume.thread)),
        ))
        .await;
    if include_history {
        warm_agent_labels_for_turns(inner, &resume.thread.turns).await;
        if !resume.thread.turns.is_empty() {
            inner.history_replay_in_progress = true;
            return Ok(ResumeApplyOutcome::Replay(ResumeReplayData {
                client: inner.client.clone(),
                workspace_cwd: inner.workspace_cwd.clone(),
                agent_labels: inner.agent_labels.clone(),
                turns: resume.thread.turns,
            }));
        }
    }

    turn_notify::notify_config_update(inner).await;
    Ok(ResumeApplyOutcome::NoReplay)
}
