//! Применение выбранного thread: `thread_resume`, sync конфигурации и UI-уведомление.

use std::collections::HashMap;
use std::path::PathBuf;

use agent_client_protocol::{
    Error,
    schema::{SessionUpdate, StopReason},
};
use codex_app_server_protocol::{ThreadResumeParams, Turn as AppTurn};

use crate::thread::features::collab::CollabAgentLabel;
use crate::thread::features::collab::{remember_agent_label, warm_agent_labels_for_turns};
use crate::thread::features::resume::common::thread_display_title;
use crate::thread::features::session::session_info_title_update_from_unix;
use crate::thread::features::session::thread_switch::flush_thread_switch_transport_state;
use crate::thread::session_lifecycle::{
    load_session_skills_summary_for_cwd, thread_resume_with_startup_retry,
};
use crate::thread::session_usage_cache::restore_cached_context_usage;
use crate::thread::{
    ContextUsageSource, SessionClient, Thread, replay, session_config, turn_notify,
};

struct ResumeReplayData {
    client: SessionClient,
    codex_home: PathBuf,
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
        let (app, resume_params, context_usage_cache_path, codex_home, bundled_skills_enabled) = {
            let inner = self.inner.lock().await;
            if include_history && inner.history_replay_in_progress {
                return Err(Error::invalid_params().data(
                    "history replay is still running; wait for it to finish before resuming another thread",
                ));
            }

            (
                inner.app.clone(),
                ThreadResumeParams {
                    thread_id: thread_id.to_string(),
                    model: Some(inner.current_model.clone()),
                    model_provider: Some(inner.current_model_provider.clone()),
                    service_tier: session_config::service_tier_override_from_session(
                        inner.service_tier,
                    ),
                    cwd: Some(inner.workspace_cwd.to_string_lossy().to_string()),
                    approval_policy: Some(inner.approval_policy),
                    sandbox: Some(inner.sandbox_mode),
                    config: inner.session_mcp_config_overrides.clone(),
                    ..Default::default()
                },
                inner.context_usage_cache_path.clone(),
                inner.codex_home.clone(),
                inner.bundled_skills_enabled,
            )
        };

        flush_thread_switch_transport_state(&app).await?;
        let resume = {
            let mut app = app.lock().await;
            thread_resume_with_startup_retry(&mut app, resume_params).await?
        };
        flush_thread_switch_transport_state(&app).await?;

        let (models, account_rate_limits) = {
            let mut app = app.lock().await;
            let models = app.model_list().await.ok().map(|response| response.data);
            let account_rate_limits = match app.get_account_rate_limits().await {
                Ok(response) => Some(response.rate_limits),
                Err(_) => None,
            };
            (models, account_rate_limits)
        };
        let session_skills_summary = load_session_skills_summary_for_cwd(
            &codex_home,
            bundled_skills_enabled,
            &resume.thread.cwd,
        )
        .await;

        let outcome = {
            let mut inner = self.inner.lock().await;
            inner.thread_id = resume.thread.id.clone();
            inner.workspace_cwd = resume.thread.cwd.clone();
            inner.approval_policy = resume.approval_policy;
            inner.sandbox_policy = resume.sandbox.clone();
            inner.sandbox_mode = session_config::policy_to_mode(&resume.sandbox);
            inner.sync_sandbox_mode_from_policy("resume_thread_ext");
            inner.current_model = resume.model;
            inner.current_model_provider = resume.model_provider;
            inner.service_tier = resume.service_tier;
            inner.compaction_in_progress = false;
            inner.total_token_usage = None;
            let cached_context_usage = restore_cached_context_usage(
                &context_usage_cache_path,
                &resume.thread.id,
                &resume.thread.turns,
            );
            inner.last_used_tokens = cached_context_usage.map(|(used, _)| used);
            inner.context_window_size = cached_context_usage.map(|(_, size)| size);
            inner.context_usage_source = cached_context_usage.map(|_| ContextUsageSource::Cached);
            inner.agent_labels.clear();
            let resumed_thread_id = inner.thread_id.clone();
            remember_agent_label(
                &mut inner.agent_labels,
                resumed_thread_id,
                resume.thread.agent_nickname.clone(),
                resume.thread.agent_role.clone(),
            );
            inner.carryover_plan_steps = None;
            inner.reset_turn_transient_state();
            if let Some(models) = models {
                inner.models = models;
            }
            if let Some(rate_limits) = account_rate_limits.as_ref() {
                session_config::observe_rate_limit_snapshot(
                    &mut inner.rate_limit_warning_state,
                    rate_limits,
                );
            }
            inner.account_rate_limits = account_rate_limits;
            inner.reasoning_effort = session_config::resolve_reasoning_effort(
                &inner.models,
                &inner.current_model,
                resume.reasoning_effort,
            );
            inner.session_skills_summary = session_skills_summary;
            inner
                .client
                .send_notification(SessionUpdate::SessionInfoUpdate(
                    session_info_title_update_from_unix(
                        thread_display_title(&resume.thread),
                        resume.thread.updated_at,
                    ),
                ))
                .await;
            if include_history {
                warm_agent_labels_for_turns(&mut inner, &resume.thread.turns).await;
                if !resume.thread.turns.is_empty() {
                    inner.history_replay_in_progress = true;
                    Some(ResumeReplayData {
                        client: inner.client.clone(),
                        codex_home: inner.codex_home.clone(),
                        workspace_cwd: inner.workspace_cwd.clone(),
                        agent_labels: inner.agent_labels.clone(),
                        turns: resume.thread.turns,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(replay_data) = outcome {
            replay::replay_turns(
                &replay_data.client,
                &replay_data.codex_home,
                &replay_data.workspace_cwd,
                &replay_data.agent_labels,
                replay_data.turns,
            )
            .await;

            let mut inner = self.inner.lock().await;
            inner.history_replay_in_progress = false;
            turn_notify::notify_usage_update(&inner).await;
            turn_notify::notify_config_update(&inner).await;
        } else {
            let inner = self.inner.lock().await;
            turn_notify::notify_usage_update(&inner).await;
            turn_notify::notify_config_update(&inner).await;
        }

        Ok(StopReason::EndTurn)
    }
}
