//! Runtime session-setting updates (mode/model/reasoning) exposed to ACP clients.

use super::session_config::{
    find_model_for_current, normalize_reasoning_effort_for_model, parse_reasoning_effort,
    reasoning_effort_value,
};
use super::{
    APPROVAL_PRESETS, AUTO_ASK_EDITS_MODE_ID, AUTO_MODE_ID, EditApprovalMode, Error, ModeKind,
    ModelId, PLAN_SESSION_MODE_ID, ReasoningEffort, SessionConfigId, SessionModeId, Thread, replay,
};
use codex_app_server_protocol::ThreadRollbackParams;

impl Thread {
    // Switch collaboration mode atomically to avoid mixed event rendering.
    pub async fn set_mode(&self, mode: SessionModeId) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        if mode.0.as_ref() == PLAN_SESSION_MODE_ID {
            let default_preset = APPROVAL_PRESETS
                .iter()
                .find(|preset| preset.id == AUTO_MODE_ID)
                .ok_or_else(Error::invalid_params)?;
            inner.apply_mode_preset(
                default_preset,
                EditApprovalMode::AutoApprove,
                ModeKind::Plan,
            );
            return Ok(());
        }

        if mode.0.as_ref() == AUTO_ASK_EDITS_MODE_ID {
            let default_preset = APPROVAL_PRESETS
                .iter()
                .find(|preset| preset.id == AUTO_MODE_ID)
                .ok_or_else(Error::invalid_params)?;
            inner.apply_mode_preset(
                default_preset,
                EditApprovalMode::AskEveryEdit,
                ModeKind::Default,
            );
            return Ok(());
        }

        let preset = APPROVAL_PRESETS
            .iter()
            .find(|preset| preset.id == mode.0.as_ref())
            .ok_or_else(Error::invalid_params)?;
        let edit_approval_mode = if preset.id == AUTO_MODE_ID {
            EditApprovalMode::AutoApprove
        } else {
            EditApprovalMode::AskEveryEdit
        };
        inner.apply_mode_preset(preset, edit_approval_mode, ModeKind::Default);
        Ok(())
    }

    pub async fn set_model(&self, model: ModelId) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        inner.current_model = model.0.to_string();
        inner.reasoning_effort = normalize_reasoning_effort_for_model(
            &inner.models,
            &inner.current_model,
            inner.reasoning_effort,
        );
        inner.last_used_tokens = None;
        inner.context_window_size = None;
        Ok(())
    }

    pub async fn set_reasoning_effort(&self, effort: ReasoningEffort) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        if let Some(model) = find_model_for_current(&inner.models, &inner.current_model)
            && !model
                .supported_reasoning_efforts
                .iter()
                .any(|option| option.reasoning_effort == effort)
        {
            return Err(Error::invalid_params().data(format!(
                "Reasoning effort `{}` is not supported by model `{}`",
                reasoning_effort_value(effort),
                model.display_name,
            )));
        }
        inner.reasoning_effort = effort;
        Ok(())
    }

    pub async fn set_config_option(
        &self,
        config_id: SessionConfigId,
        value: agent_client_protocol::SessionConfigValueId,
    ) -> Result<(), Error> {
        match config_id.0.as_ref() {
            "mode" => self.set_mode(SessionModeId::new(value.0)).await,
            "model" => self.set_model(ModelId::new(value.0)).await,
            "reasoning_effort" => {
                let effort = parse_reasoning_effort(&value.0)
                    .ok_or_else(|| Error::invalid_params().data("Unsupported reasoning effort"))?;
                self.set_reasoning_effort(effort).await
            }
            _ => Err(Error::invalid_params().data("Unsupported config option")),
        }
    }

    pub async fn cancel(&self) -> Result<(), Error> {
        let current = *self.cancel_tx.borrow();
        self.cancel_tx
            .send(current.saturating_add(1))
            .map_err(|err| Error::internal_error().data(err.to_string()))
    }

    pub async fn rollback_turns_ext(
        &self,
        num_turns: u32,
        replay_history: bool,
    ) -> Result<usize, Error> {
        if num_turns == 0 {
            return Err(Error::invalid_params().data("num_turns must be >= 1"));
        }

        let mut inner = self.inner.lock().await;
        let thread_id = inner.thread_id.clone();
        let response = inner
            .app
            .thread_rollback(ThreadRollbackParams {
                thread_id,
                num_turns,
            })
            .await?;
        let remaining_turns = response.thread.turns.len();

        if replay_history {
            let workspace_cwd = inner.workspace_cwd.clone();
            replay::replay_turns(&inner.client, &workspace_cwd, response.thread.turns).await;
        }

        Ok(remaining_turns)
    }
}
