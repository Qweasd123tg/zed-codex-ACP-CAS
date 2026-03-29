//! Применение выбранного thread: `thread_resume`, sync конфигурации и UI-уведомление.

use agent_client_protocol::{Error, SessionInfoUpdate, SessionUpdate, StopReason};
use codex_app_server_protocol::ThreadResumeParams;
use std::time::Duration;

use crate::thread::features::collab::{remember_agent_label, warm_agent_labels_for_turns};
use crate::thread::features::resume::common::thread_display_title;
use crate::thread::session_lifecycle::thread_resume_with_startup_retry;
use crate::thread::{ThreadInner, replay, session_config, turn_notify};

const RESUME_TRANSPORT_FLUSH_TIMEOUT_MS: u64 = 20;
const RESUME_TRANSPORT_FLUSH_MAX_MESSAGES: usize = 64;

pub(in crate::thread) async fn handle_resume_command(
    inner: &mut ThreadInner,
    thread_id: &str,
    include_history: bool,
) -> Result<StopReason, Error> {
    flush_resume_transport_state(inner).await?;

    let resume = thread_resume_with_startup_retry(
        &mut inner.app,
        ThreadResumeParams {
            thread_id: thread_id.to_string(),
            ..Default::default()
        },
    )
    .await?;

    flush_resume_transport_state(inner).await?;

    inner.thread_id = resume.thread.id.clone();
    inner.workspace_cwd = resume.thread.cwd.clone();
    inner.approval_policy = resume.approval_policy;
    inner.sandbox_policy = resume.sandbox.clone();
    inner.sandbox_mode = session_config::policy_to_mode(&resume.sandbox);
    inner.sync_sandbox_mode_from_policy("handle_resume_command");
    inner.current_model = resume.model;
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
    flush_resume_transport_state(inner).await?;
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

async fn flush_resume_transport_state(inner: &mut ThreadInner) -> Result<(), Error> {
    let _ = inner
        .app
        .discard_background_messages(
            Duration::from_millis(RESUME_TRANSPORT_FLUSH_TIMEOUT_MS),
            RESUME_TRANSPORT_FLUSH_MAX_MESSAGES,
        )
        .await?;
    Ok(())
}
