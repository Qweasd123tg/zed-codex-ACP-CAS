//! Обновления runtime-настроек сессии (mode/model/reasoning/context), доступные ACP-клиентам.

use super::session_config::{
    CONTEXT_COMPACT_VALUE, CONTEXT_LIMITS_VALUE, CONTEXT_STATUS_VALUE, MCP_STATUS_VALUE,
    PLUGINS_STATUS_VALUE, SESSION_STATUS_VALUE, SKILLS_STATUS_VALUE, combined_limits_reset_message,
    context_usage_message, find_model_for_current, full_status_report,
    normalize_reasoning_effort_for_model, parse_fast_mode_value, parse_model_reasoning_value,
    parse_model_speed_value, parse_reasoning_effort, reasoning_effort_value,
};
use super::{
    APPROVAL_PRESETS, AUTO_ASK_EDITS_MODE_ID, AUTO_MODE_ID, DEFAULT_SESSION_MODE_ID,
    EditApprovalMode, Error, ModeKind, ModelId, PLAN_SESSION_MODE_ID, ReasoningEffort,
    SessionConfigId, SessionModeId, Thread, replay,
};
use crate::thread::features::session::controls::start_context_compaction;
use crate::thread::features::{
    collab::{remember_agent_label, warm_agent_labels_for_turns},
    plan::{
        clear_visible_plan_state, has_visible_plan_state, should_clear_visible_plan_for_mode_change,
    },
};
use agent_client_protocol::schema::SessionConfigValueId;
use codex_app_server_protocol::ThreadRollbackParams;
use tracing::warn;

const COMPACTION_SELECTOR_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

impl Thread {
    pub async fn set_mode(&self, mode: SessionModeId) -> Result<(), Error> {
        let next_mode = match mode.0.as_ref() {
            PLAN_SESSION_MODE_ID => ModeKind::Plan,
            DEFAULT_SESSION_MODE_ID => ModeKind::Default,
            _ => return Err(Error::invalid_params()),
        };
        let mut inner = self.inner.lock().await;
        let previous_mode = inner.collaboration_mode_kind;
        let had_visible_plan_state = has_visible_plan_state(&inner);
        inner.collaboration_mode_kind = next_mode;
        if should_clear_visible_plan_for_mode_change(
            previous_mode,
            next_mode,
            had_visible_plan_state,
        ) {
            clear_visible_plan_state(&mut inner).await;
        }
        Ok(())
    }

    pub async fn set_permission_mode(&self, mode: SessionModeId) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        let collaboration_mode_kind = inner.collaboration_mode_kind;
        if mode.0.as_ref() == AUTO_ASK_EDITS_MODE_ID {
            let default_preset = APPROVAL_PRESETS
                .iter()
                .find(|preset| preset.id == AUTO_MODE_ID)
                .ok_or_else(Error::invalid_params)?;
            inner.apply_mode_preset(
                default_preset,
                EditApprovalMode::AskEveryEdit,
                collaboration_mode_kind,
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
        inner.apply_mode_preset(preset, edit_approval_mode, collaboration_mode_kind);
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
        inner.context_usage_source = None;
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

    pub async fn set_fast_mode(
        &self,
        service_tier: Option<codex_protocol::config_types::ServiceTier>,
    ) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        inner.service_tier = service_tier;
        Ok(())
    }

    pub async fn set_context_control(&self, value: SessionConfigValueId) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        match value.0.as_ref() {
            SESSION_STATUS_VALUE => {
                inner
                    .client
                    .send_agent_text(full_status_report(
                        &inner.workspace_cwd,
                        &inner.account_status,
                        inner.total_token_usage.as_ref(),
                        inner.last_used_tokens,
                        inner.context_window_size,
                        inner.context_usage_source,
                        inner.account_rate_limits.as_ref(),
                        inner.compaction_in_progress,
                        &inner.session_mcp_summary,
                        &inner.session_skills_summary,
                        &inner.session_plugins_summary,
                    ))
                    .await;
                Ok(())
            }
            CONTEXT_STATUS_VALUE => {
                if inner.last_used_tokens.is_none() && inner.context_window_size.is_none() {
                    return Ok(());
                }
                inner
                    .client
                    .send_agent_text(context_usage_message(
                        inner.last_used_tokens,
                        inner.context_window_size,
                        inner.context_usage_source,
                    ))
                    .await;
                Ok(())
            }
            MCP_STATUS_VALUE => {
                inner
                    .client
                    .send_agent_text(inner.session_mcp_summary.report.clone())
                    .await;
                Ok(())
            }
            SKILLS_STATUS_VALUE => {
                inner
                    .client
                    .send_agent_text(inner.session_skills_summary.report.clone())
                    .await;
                Ok(())
            }
            PLUGINS_STATUS_VALUE => {
                inner
                    .client
                    .send_agent_text(inner.session_plugins_summary.report.clone())
                    .await;
                Ok(())
            }
            CONTEXT_LIMITS_VALUE => {
                inner
                    .client
                    .send_agent_text(combined_limits_reset_message(
                        inner.account_rate_limits.as_ref(),
                    ))
                    .await;
                Ok(())
            }
            CONTEXT_COMPACT_VALUE => {
                let message = start_context_compaction(&mut inner).await?;
                inner.client.send_agent_text(message).await;
                drop(inner);
                self.spawn_compaction_drain_task();
                let drain_outcome = self
                    .drain_background_notifications_for_ext(COMPACTION_SELECTOR_DRAIN_TIMEOUT)
                    .await?;
                if drain_outcome.was_truncated() {
                    warn!(
                        processed_messages = drain_outcome.processed(),
                        outcome = ?drain_outcome,
                        "context compact action drain stopped before the queue went quiet"
                    );
                }
                Ok(())
            }
            _ => Err(Error::invalid_params().data("Unsupported context control action")),
        }
    }

    pub async fn set_config_option(
        &self,
        config_id: SessionConfigId,
        value: SessionConfigValueId,
    ) -> Result<(), Error> {
        let drain_outcome = self.drain_background_notifications_ext().await?;
        if drain_outcome.was_truncated() {
            warn!(
                processed_messages = drain_outcome.processed(),
                outcome = ?drain_outcome,
                "set config background drain stopped before the queue went quiet"
            );
        }

        match config_id.0.as_ref() {
            "mode" => self.set_mode(SessionModeId::new(value.0)).await,
            "permissions" => self.set_permission_mode(SessionModeId::new(value.0)).await,
            "model" => {
                if let Some(effort) = parse_model_reasoning_value(&value.0) {
                    self.set_reasoning_effort(effort).await?;
                    self.notify_config_options_update().await;
                    Ok(())
                } else if let Some(service_tier) = parse_model_speed_value(&value.0) {
                    self.set_fast_mode(service_tier).await?;
                    self.notify_config_options_update().await;
                    Ok(())
                } else {
                    self.set_model(ModelId::new(value.0)).await
                }
            }
            "fast_mode" => {
                let service_tier = parse_fast_mode_value(&value.0)
                    .ok_or_else(|| Error::invalid_params().data("Unsupported fast mode value"))?;
                self.set_fast_mode(service_tier).await
            }
            "reasoning_effort" => {
                let effort = parse_reasoning_effort(&value.0)
                    .ok_or_else(|| Error::invalid_params().data("Unsupported reasoning effort"))?;
                self.set_reasoning_effort(effort).await
            }
            "context_control" => self.set_context_control(value).await,
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

        let (app, thread_id) = {
            let inner = self.inner.lock().await;
            if replay_history && inner.history_replay_in_progress {
                return Err(Error::invalid_params().data(
                    "history replay is still running; wait for it to finish before rolling back again",
                ));
            }
            (inner.app.clone(), inner.thread_id.clone())
        };
        let response = app
            .lock()
            .await
            .thread_rollback(ThreadRollbackParams {
                thread_id,
                num_turns,
            })
            .await?;

        let (remaining_turns, replay_data) = {
            let mut inner = self.inner.lock().await;
            let remaining_turns = response.thread.turns.len();

            if replay_history {
                remember_agent_label(
                    &mut inner.agent_labels,
                    response.thread.id.clone(),
                    response.thread.agent_nickname.clone(),
                    response.thread.agent_role.clone(),
                );
                warm_agent_labels_for_turns(&mut inner, &response.thread.turns).await;
                if !response.thread.turns.is_empty() {
                    inner.history_replay_in_progress = true;
                }
            }

            (
                remaining_turns,
                replay_history.then(|| {
                    (
                        inner.client.clone(),
                        inner.workspace_cwd.clone(),
                        inner.agent_labels.clone(),
                        response.thread.turns,
                    )
                }),
            )
        };

        if let Some((client, workspace_cwd, agent_labels, turns)) = replay_data {
            replay::replay_turns(&client, &workspace_cwd, &agent_labels, turns).await;
            let mut inner = self.inner.lock().await;
            inner.history_replay_in_progress = false;
        }

        Ok(remaining_turns)
    }
}
