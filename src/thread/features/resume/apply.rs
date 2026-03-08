//! Apply a selected thread: `thread_resume`, config sync, and UI updates.

use agent_client_protocol::{Error, StopReason};
use codex_app_server_protocol::ThreadResumeParams;

use crate::thread::{ThreadInner, replay, session_config, turn_notify};

pub(in crate::thread) async fn handle_resume_command(
    inner: &mut ThreadInner,
    thread_id: &str,
    include_history: bool,
) -> Result<StopReason, Error> {
    let resume = inner
        .app
        .thread_resume(ThreadResumeParams {
            thread_id: thread_id.to_string(),
            ..Default::default()
        })
        .await?;

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
    inner.reset_turn_transient_state();

    if let Ok(models) = inner.app.model_list().await {
        inner.models = models.data;
    }
    inner.reasoning_effort = session_config::resolve_reasoning_effort(
        &inner.models,
        &inner.current_model,
        resume.reasoning_effort,
    );
    if include_history {
        let workspace_cwd = inner.workspace_cwd.clone();
        replay::replay_turns(&inner.client, &workspace_cwd, resume.thread.turns).await;
    }

    turn_notify::notify_config_update(inner).await;

    Ok(StopReason::EndTurn)
}
