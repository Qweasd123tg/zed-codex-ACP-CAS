//! Session control slash-command handlers, excluding `/resume`.
//! Compact, undo, reasoning, plan, and context branches live here.

use crate::thread::{ThreadInner, replay::replay_turns, turn_notify::notify_config_update};
use agent_client_protocol::{Error, StopReason};
use codex_app_server_protocol::{ThreadCompactStartParams, ThreadRollbackParams};

pub(in crate::thread) async fn handle_compact_command(
    inner: &mut ThreadInner,
) -> Result<StopReason, Error> {
    if inner.compaction_in_progress {
        inner
            .client
            .send_agent_text("Context compaction is already running.")
            .await;
        return Ok(StopReason::EndTurn);
    }

    inner
        .app
        .thread_compact_start(ThreadCompactStartParams {
            thread_id: inner.thread_id.clone(),
        })
        .await?;
    inner.compaction_in_progress = true;
    // Token usage may remain stale, often at 100%, until the next completed model turn.
    // Clear the cached usage immediately after /compact so the context percentage is not misleading.
    inner.last_used_tokens = None;
    notify_config_update(inner).await;
    inner
        .client
        .send_agent_text(
            "Context compaction started. Wait for \"Context compacted.\" before sending the next prompt.",
        )
        .await;
    Ok(StopReason::EndTurn)
}

pub(in crate::thread) async fn handle_undo_command(
    inner: &mut ThreadInner,
    num_turns: u32,
) -> Result<StopReason, Error> {
    let response = inner
        .app
        .thread_rollback(ThreadRollbackParams {
            thread_id: inner.thread_id.clone(),
            num_turns,
        })
        .await?;

    let workspace_cwd = inner.workspace_cwd.clone();
    replay_turns(&inner.client, &workspace_cwd, response.thread.turns).await;
    inner
        .client
        .send_agent_text(format!("Rolled back last {num_turns} turn(s)."))
        .await;
    Ok(StopReason::EndTurn)
}

pub(in crate::thread) async fn handle_context_command(
    inner: &mut ThreadInner,
) -> Result<StopReason, Error> {
    match (inner.last_used_tokens, inner.context_window_size) {
        (Some(used), Some(size)) if size > 0 => {
            let percent = (used as f64 / size as f64) * 100.0;
            inner
                .client
                .send_agent_text(format!(
                    "Context usage: {used}/{size} tokens ({percent:.1}%)."
                ))
                .await;
        }
        (Some(used), None) => {
            inner
                .client
                .send_agent_text(format!(
                    "Context usage: {used} tokens (window size is not available yet)."
                ))
                .await;
        }
        _ => {
            inner
                .client
                .send_agent_text(
                    "Context usage is not available yet. App-server sends it only after the first completed model turn in this session.",
                )
                .await;
        }
    }
    Ok(StopReason::EndTurn)
}
