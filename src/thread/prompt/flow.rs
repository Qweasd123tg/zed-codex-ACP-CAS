//! Main prompt execution pipeline: command parsing, turn startup, and stop-reason mapping.

use super::{
    Error, ModeKind, PLAN_IMPLEMENTATION_PROMPT, StopReason, Thread, notification_dispatch,
    prompt_commands, turn_execution, turn_notify,
};
use agent_client_protocol::ContentBlock;

impl Thread {
    // Handle slash commands first so control commands are not sent as normal user prompts.
    pub async fn prompt(
        &self,
        request: agent_client_protocol::PromptRequest,
    ) -> Result<StopReason, Error> {
        let command = prompt_commands::parse_session_command(&request.prompt);
        let mut plan_prompt: Option<String> = None;
        let mut inner = self.inner.lock().await;
        notification_dispatch::drain_background_notifications(&mut inner).await?;
        if let Some(command) = command {
            match prompt_commands::dispatch_session_command(&mut inner, command).await? {
                prompt_commands::CommandDispatchOutcome::Stop(stop_reason) => {
                    return Ok(stop_reason);
                }
                prompt_commands::CommandDispatchOutcome::PlanPrompt(prompt) => {
                    plan_prompt = Some(prompt);
                }
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
            prompt_commands::build_prompt_items(vec![ContentBlock::from(prompt.clone())])
        } else {
            prompt_commands::build_prompt_items(request.prompt)
        };
        if input.is_empty() {
            return Err(Error::invalid_params().data("prompt is empty"));
        }

        let collaboration_mode_kind = if plan_prompt.is_some() {
            ModeKind::Plan
        } else {
            inner.collaboration_mode_kind
        };
        let stop_reason = turn_execution::run_single_turn(
            &mut inner,
            &self.cancel_tx,
            input,
            collaboration_mode_kind,
        )
        .await?;

        if stop_reason == StopReason::EndTurn
            && collaboration_mode_kind == ModeKind::Plan
            && (inner.active_turn_saw_plan_item
                || inner.active_turn_saw_plan_delta
                || inner
                    .last_plan_steps
                    .iter()
                    .any(|step| !step.trim().is_empty()))
        {
            let implement_now = turn_execution::prompt_plan_implementation(&mut inner).await?;
            if implement_now {
                if !inner.last_plan_steps.is_empty() {
                    inner.carryover_plan_steps = Some(inner.last_plan_steps.clone());
                }
                inner.collaboration_mode_kind = ModeKind::Default;
                turn_notify::notify_mode_and_config_update(&inner).await;
                let implementation_input =
                    prompt_commands::build_prompt_items(vec![ContentBlock::from(
                        PLAN_IMPLEMENTATION_PROMPT,
                    )]);
                if !implementation_input.is_empty() {
                    inner
                        .client
                        .send_agent_text("Switching to default mode and implementing the plan.")
                        .await;
                    return turn_execution::run_single_turn(
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
