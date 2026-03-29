//! Применение выбранного thread: `thread_resume`, sync конфигурации и UI-уведомление.

use agent_client_protocol::{Error, SessionInfoUpdate, SessionUpdate, StopReason};
use codex_app_server_protocol::ThreadResumeParams;

use crate::thread::features::collab::{remember_agent_label, warm_agent_labels_for_turns};
use crate::thread::features::resume::common::thread_display_title;
use crate::thread::features::session::thread_switch::flush_thread_switch_transport_state;
use crate::thread::session_lifecycle::thread_resume_with_startup_retry;
use crate::thread::{ThreadInner, replay, session_config, turn_notify};

pub(in crate::thread) async fn handle_resume_command(
    inner: &mut ThreadInner,
    thread_id: &str,
    include_history: bool,
) -> Result<StopReason, Error> {
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
    inner.last_used_tokens = None;
    inner.context_window_size = None;
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
    flush_thread_switch_transport_state(inner).await?;
    inner.reasoning_effort = session_config::resolve_reasoning_effort(
        &inner.models,
        &inner.current_model,
        resume.reasoning_effort,
    );
    inner
        .client
        .send_notification(SessionUpdate::SessionInfoUpdate(
            SessionInfoUpdate::new().title(thread_display_title(&resume.thread)),
        ))
        .await;
    if include_history {
        let workspace_cwd = inner.workspace_cwd.clone();
        warm_agent_labels_for_turns(inner, &resume.thread.turns).await;
        let agent_labels = inner.agent_labels.clone();
        replay::replay_turns(
            &inner.client,
            &workspace_cwd,
            &agent_labels,
            resume.thread.turns,
        )
        .await;
    }

    turn_notify::notify_config_update(inner).await;

    Ok(StopReason::EndTurn)
}
