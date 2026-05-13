//! Основной конвейер выполнения промпта: парсинг команд, старт turn и маппинг завершения.

use super::{
    Error, ModeKind, PLAN_IMPLEMENTATION_PROMPT, ReviewTarget, SessionCommand, StopReason, Thread,
    prompt_commands, turn_execution, turn_notify,
};
use agent_client_protocol::schema::{ContentBlock, PromptRequest};

const COMPACTION_PROMPT_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

impl Thread {
    // Сначала обрабатываем slash-команды, чтобы не отправлять управляющие команды как пользовательский промпт.
    pub async fn prompt(&self, request: PromptRequest) -> Result<StopReason, Error> {
        let command = prompt_commands::parse_session_command(&request.prompt);
        let resume_request = match &command {
            Some(SessionCommand::Resume {
                thread_id,
                include_history,
            }) => Some((thread_id.clone(), *include_history)),
            _ => None,
        };
        let undo_turns = match &command {
            Some(SessionCommand::Undo { num_turns }) => Some(*num_turns),
            _ => None,
        };
        let archive_thread_id = match &command {
            Some(SessionCommand::Archive { thread_id }) => Some(thread_id.clone()),
            _ => None,
        };
        let fork_args = match &command {
            Some(SessionCommand::Fork { args }) => Some(args.clone()),
            _ => None,
        };
        let mut prompt_override: Option<String> = None;
        let mut prompt_override_mode_kind: Option<ModeKind> = None;
        let mut review_target: Option<ReviewTarget> = None;
        {
            let inner = self.inner.lock().await;
            if inner.history_replay_in_progress {
                inner
                    .client
                    .send_system_message(
                        "status",
                        "History replay is still running",
                        "Wait for it to finish before sending the next prompt or session command.",
                    )
                    .await;
                return Ok(StopReason::EndTurn);
            }
            if let Some(command) = command.as_ref() {
                let command_name = prompt_commands::session_command_name(command);
                if !inner.slash_commands.is_enabled(command_name) {
                    inner
                        .client
                        .send_system_message(
                            "status",
                            "Slash command disabled",
                            format!(
                                "`/{command_name}` is disabled by codex-acp selector preferences."
                            ),
                        )
                        .await;
                    return Ok(StopReason::EndTurn);
                }
            }
        }
        if should_drain_background_notifications(command.as_ref()) {
            let drain_outcome = self.drain_background_notifications_ext().await?;
            if drain_outcome.was_truncated() {
                tracing::warn!(
                    processed_messages = drain_outcome.processed(),
                    outcome = ?drain_outcome,
                    "background transport drain stopped before the queue went quiet"
                );
            }
        }
        let mut inner = self.inner.lock().await;
        if inner.history_replay_in_progress {
            inner
                .client
                .send_system_message(
                    "status",
                    "History replay is still running",
                    "Wait for it to finish before sending the next prompt or session command.",
                )
                .await;
            return Ok(StopReason::EndTurn);
        }
        if let Some((thread_id, include_history)) = resume_request {
            drop(inner);
            return self
                .handle_resume_selector_command_ext(thread_id.as_deref(), include_history)
                .await;
        }
        if let Some(num_turns) = undo_turns {
            drop(inner);
            self.rollback_turns_ext(num_turns, true).await?;
            let client = {
                let inner = self.inner.lock().await;
                inner.client.clone()
            };
            client
                .send_system_message(
                    "status",
                    "Undo completed",
                    format!("Rolled back last {num_turns} turn(s)."),
                )
                .await;
            return Ok(StopReason::EndTurn);
        }
        if let Some(thread_id) = archive_thread_id {
            drop(inner);
            return self.handle_archive_command_ext(thread_id).await;
        }
        if let Some(args) = fork_args {
            drop(inner);
            return self.handle_fork_command_ext(args).await;
        }
        if matches!(command.as_ref(), Some(SessionCommand::Compact)) {
            drop(inner);
            return self.handle_compact_command_ext().await;
        }
        if let Some(command) = command {
            match prompt_commands::dispatch_session_command(&mut inner, command).await? {
                prompt_commands::CommandDispatchOutcome::Stop(stop_reason) => {
                    return Ok(stop_reason);
                }
                prompt_commands::CommandDispatchOutcome::PromptOverride { prompt, mode_kind } => {
                    prompt_override = Some(prompt);
                    prompt_override_mode_kind = Some(mode_kind);
                }
                prompt_commands::CommandDispatchOutcome::ReviewStart(target) => {
                    review_target = Some(target);
                }
            }
        }

        if inner.compaction_in_progress {
            drop(inner);
            let drain_outcome = self
                .drain_background_notifications_for_ext(COMPACTION_PROMPT_DRAIN_TIMEOUT)
                .await?;
            if drain_outcome.was_truncated() {
                tracing::warn!(
                    processed_messages = drain_outcome.processed(),
                    outcome = ?drain_outcome,
                    "compact prompt drain stopped before the queue went quiet"
                );
            }
            inner = self.inner.lock().await;
        }

        if inner.compaction_in_progress {
            inner
                .client
                .send_system_message(
                    "status",
                    "Context compaction is still running",
                    "Wait for \"Context compacted.\" and send your prompt again.",
                )
                .await;
            return Ok(StopReason::EndTurn);
        }

        if let Some(target) = review_target {
            inner
                .client
                .send_agent_text(crate::thread::features::session::review::review_user_hint(
                    &target,
                ))
                .await;
            drop(inner);
            return self.run_review_turn_ext(target).await;
        }

        let input = if let Some(prompt) = prompt_override.as_ref() {
            prompt_commands::build_prompt_items(vec![ContentBlock::from(prompt.clone())])
        } else {
            prompt_commands::build_prompt_items(request.prompt)
        };
        if input.is_empty() {
            return Err(Error::invalid_params().data("prompt is empty"));
        }

        let collaboration_mode_kind =
            prompt_override_mode_kind.unwrap_or(inner.collaboration_mode_kind);
        drop(inner);
        let stop_reason = self
            .run_single_turn_ext(input, collaboration_mode_kind)
            .await?;
        let mut inner = self.inner.lock().await;

        if let Some(implementation_input) =
            maybe_prepare_plan_implementation(&mut inner, stop_reason, collaboration_mode_kind)
                .await?
        {
            drop(inner);
            return self
                .run_single_turn_ext(implementation_input, ModeKind::Default)
                .await;
        }

        Ok(stop_reason)
    }
}

async fn maybe_prepare_plan_implementation(
    inner: &mut crate::thread::ThreadInner,
    stop_reason: StopReason,
    collaboration_mode_kind: ModeKind,
) -> Result<Option<Vec<codex_app_server_protocol::UserInput>>, Error> {
    if !should_offer_plan_implementation(inner, stop_reason, collaboration_mode_kind) {
        return Ok(None);
    }

    if !turn_execution::prompt_plan_implementation(inner).await? {
        return Ok(None);
    }

    if !inner.last_plan_steps.is_empty() {
        inner.carryover_plan_steps = Some(inner.last_plan_steps.clone());
    }
    inner.collaboration_mode_kind = ModeKind::Default;
    turn_notify::notify_mode_and_config_update(inner).await;

    let implementation_input =
        prompt_commands::build_prompt_items(vec![ContentBlock::from(PLAN_IMPLEMENTATION_PROMPT)]);
    if implementation_input.is_empty() {
        return Ok(None);
    }

    inner
        .client
        .send_system_message(
            "status",
            "Switching to default mode",
            "Implementing the plan.",
        )
        .await;
    Ok(Some(implementation_input))
}

fn should_offer_plan_implementation(
    inner: &crate::thread::ThreadInner,
    stop_reason: StopReason,
    collaboration_mode_kind: ModeKind,
) -> bool {
    stop_reason == StopReason::EndTurn
        && collaboration_mode_kind == ModeKind::Plan
        && (inner.active_turn_saw_plan_item
            || inner.active_turn_saw_plan_delta
            || inner
                .last_plan_steps
                .iter()
                .any(|step| !step.trim().is_empty()))
}

fn should_drain_background_notifications(command: Option<&SessionCommand>) -> bool {
    !matches!(
        command,
        Some(SessionCommand::Resume { .. } | SessionCommand::Fork { .. })
    )
}
