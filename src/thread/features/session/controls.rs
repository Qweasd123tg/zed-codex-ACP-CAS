//! Обработчики slash-команд управления сессией (без `/resume`).
//! Сюда вынесены compact/undo/reasoning/plan/context ветки.

use crate::thread::features::collab::{remember_agent_label, warm_agent_labels_for_turns};
use crate::thread::{ThreadInner, replay::replay_turns, turn_notify::notify_config_update};
use agent_client_protocol::{Error, SessionInfoUpdate, SessionUpdate, StopReason};
use codex_app_server_protocol::{
    ThreadCompactStartParams, ThreadRollbackParams, ThreadSetNameParams,
};

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
    // Статистика токенов может оставаться устаревшей (часто 100%) до следующего завершённого turn модели.
    // Сразу после /compact очищаем кэш usage, чтобы процент контекста не вводил в заблуждение.
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
    remember_agent_label(
        &mut inner.agent_labels,
        response.thread.id.clone(),
        response.thread.agent_nickname.clone(),
        response.thread.agent_role.clone(),
    );
    warm_agent_labels_for_turns(inner, &response.thread.turns).await;
    let agent_labels = inner.agent_labels.clone();
    replay_turns(
        &inner.client,
        &workspace_cwd,
        &agent_labels,
        response.thread.turns,
    )
    .await;
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
        .thread_set_name(ThreadSetNameParams {
            thread_id: inner.thread_id.clone(),
            name: name.clone(),
        })
        .await?;

    inner
        .client
        .send_notification(SessionUpdate::SessionInfoUpdate(
            SessionInfoUpdate::new().title(name.clone()),
        ))
        .await;
    inner
        .client
        .send_agent_text(format!("Thread renamed to `{name}`."))
        .await;
    Ok(StopReason::EndTurn)
}
