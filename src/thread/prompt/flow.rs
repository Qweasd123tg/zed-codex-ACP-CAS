//! Основной конвейер выполнения промпта: парсинг команд, старт turn и маппинг завершения.

use super::notification_dispatch::drain_background_notifications;
use super::prompt_commands::{build_prompt_items, parse_session_command};
use super::turn_execution::{
    notify_mode_and_config_update, prompt_plan_implementation, run_single_turn,
};
use super::{Error, ModeKind, PLAN_IMPLEMENTATION_PROMPT, SessionCommand, StopReason, Thread};
use crate::thread::features::resume::listing::handle_threads_command;
use crate::thread::features::resume::selector::handle_resume_selector_command;
use crate::thread::features::session::controls::{
    handle_compact_command, handle_context_command, handle_undo_command,
};
use crate::thread::features::session::modes::{handle_plan_mode_command, handle_reasoning_command};
use agent_client_protocol::ContentBlock;

impl Thread {
    // Сначала обрабатываем slash-команды, чтобы не отправлять управляющие команды как пользовательский промпт.
    pub async fn prompt(
        &self,
        request: agent_client_protocol::PromptRequest,
    ) -> Result<StopReason, Error> {
        let command = parse_session_command(&request.prompt);
        let mut plan_prompt: Option<String> = None;
        let mut inner = self.inner.lock().await;
        drain_background_notifications(&mut inner).await?;
        if let Some(command) = command {
            match command {
                SessionCommand::Threads => return handle_threads_command(&mut inner).await,
                SessionCommand::Resume { thread_id } => {
                    return handle_resume_selector_command(&mut inner, thread_id.as_deref()).await;
                }
                SessionCommand::Compact => return handle_compact_command(&mut inner).await,
                SessionCommand::Undo { num_turns } => {
                    return handle_undo_command(&mut inner, num_turns).await;
                }
                SessionCommand::Reasoning { raw_value, effort } => {
                    return handle_reasoning_command(&mut inner, raw_value, effort).await;
                }
                SessionCommand::PlanMode { raw_value, mode } => {
                    return handle_plan_mode_command(&mut inner, raw_value, mode).await;
                }
                SessionCommand::PlanPrompt { prompt } => {
                    plan_prompt = Some(prompt);
                }
                SessionCommand::Context => return handle_context_command(&mut inner).await,
            }
        }

        if inner.compaction_in_progress {
            inner
                .client
                .send_agent_text(
                    "Context compaction is still running. Wait for \"Context compacted.\" and send your prompt again.",
                )
                .await;
            return Ok(StopReason::EndTurn);
        }

        let input = if let Some(prompt) = plan_prompt.as_ref() {
            build_prompt_items(vec![ContentBlock::from(prompt.clone())])
        } else {
            build_prompt_items(request.prompt)
        };
        if input.is_empty() {
            return Err(Error::invalid_params().data("prompt is empty"));
        }

        let collaboration_mode_kind = if plan_prompt.is_some() {
            ModeKind::Plan
        } else {
            inner.collaboration_mode_kind
        };
        let stop_reason =
            run_single_turn(&mut inner, &self.cancel_tx, input, collaboration_mode_kind).await?;

        if stop_reason == StopReason::EndTurn
            && collaboration_mode_kind == ModeKind::Plan
            && inner.active_turn_saw_plan_item
        {
            let implement_now = prompt_plan_implementation(&mut inner).await?;
            if implement_now {
                if !inner.last_plan_steps.is_empty() {
                    inner.carryover_plan_steps = Some(inner.last_plan_steps.clone());
                }
                inner.collaboration_mode_kind = ModeKind::Default;
                notify_mode_and_config_update(&inner).await;
                let implementation_input =
                    build_prompt_items(vec![ContentBlock::from(PLAN_IMPLEMENTATION_PROMPT)]);
                if !implementation_input.is_empty() {
                    inner
                        .client
                        .send_agent_text("Switching to default mode and implementing the plan.")
                        .await;
                    return run_single_turn(
                        &mut inner,
                        &self.cancel_tx,
                        implementation_input,
                        ModeKind::Default,
                    )
                    .await;
                }
            }
        }

        Ok(stop_reason)
    }
}
