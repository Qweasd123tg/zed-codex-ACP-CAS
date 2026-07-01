//! Обновления runtime-настроек сессии (mode/model/reasoning/context), доступные ACP-клиентам.

use super::session_config::{
    MCP_STATUS_VALUE, PLUGINS_STATUS_VALUE, SESSION_STATUS_VALUE, SKILLS_STATUS_VALUE,
    STATUS_COMPACT_VALUE, STATUS_LIMITS_VALUE, find_model_for_current,
    normalize_reasoning_effort_for_model, parse_limits_summary_value, parse_model_reasoning_value,
    parse_model_speed_value, reasoning_effort_value,
};
use super::{
    APPROVAL_PRESETS, DEFAULT_SESSION_MODE_ID, Error, ModeKind, PLAN_SESSION_MODE_ID,
    ReasoningEffort, SessionConfigId, SessionModeId, Thread, replay,
};
use crate::thread::features::{
    collab::{remember_agent_label, warm_agent_labels_for_turns},
    plan::{
        clear_visible_plan_state, has_visible_plan_state, should_clear_visible_plan_for_mode_change,
    },
};
use crate::thread::session_display_maps::persist_display_maps;
use crate::thread::session_selector_preferences::persist_selector_preferences;
use agent_client_protocol::schema::v1::SessionConfigValueId;
use codex_app_server_protocol::ThreadRollbackParams;
use tracing::warn;

const COMPACTION_SELECTOR_DRAIN_TIMEOUT: std::time::Duration =
    std::time::Duration::from_millis(100);

fn persist_selector_preferences_or_warn(inner: &crate::thread::ThreadInner) {
    if let Err(error) = persist_selector_preferences(inner) {
        warn!(
            %error,
            path = %inner.selector_preferences_path.display(),
            "failed to persist selector preferences"
        );
    }
}

fn persist_display_maps_or_warn(inner: &crate::thread::ThreadInner) {
    if let Err(error) = persist_display_maps(&inner.display_maps_path, &inner.display_maps) {
        warn!(
            %error,
            path = %inner.display_maps_path.display(),
            "failed to persist display maps"
        );
    }
}

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
        let previous_mode = inner.collaboration_mode_kind;
        let had_visible_plan_state = has_visible_plan_state(&inner);
        if mode.0.as_ref() == PLAN_SESSION_MODE_ID {
            inner.collaboration_mode_kind = ModeKind::Plan;
            return Ok(());
        }

        let next_mode = ModeKind::Default;
        let preset = APPROVAL_PRESETS
            .iter()
            .find(|preset| preset.id == mode.0.as_ref())
            .ok_or_else(Error::invalid_params)?;
        inner.apply_mode_preset(preset, next_mode);
        if should_clear_visible_plan_for_mode_change(
            previous_mode,
            next_mode,
            had_visible_plan_state,
        ) {
            clear_visible_plan_state(&mut inner).await;
        }
        Ok(())
    }

    pub async fn set_model(&self, model: String) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        inner.current_model = model;
        if !inner
            .model_selector
            .explicitly_enables_reasoning_effort(inner.reasoning_effort)
        {
            inner.reasoning_effort = normalize_reasoning_effort_for_model(
                &inner.models,
                &inner.current_model,
                inner.reasoning_effort,
            );
        }
        inner.model_selector.default_model = Some(inner.current_model.clone());
        inner.last_used_tokens = None;
        inner.context_window_size = None;
        inner.context_usage_source = None;
        persist_selector_preferences_or_warn(&inner);
        Ok(())
    }

    pub async fn set_reasoning_effort(&self, effort: ReasoningEffort) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        if let Some(model) = find_model_for_current(&inner.models, &inner.current_model)
            && !model
                .supported_reasoning_efforts
                .iter()
                .any(|option| option.reasoning_effort == effort)
            && !inner
                .model_selector
                .explicitly_enables_reasoning_effort(effort)
        {
            return Err(Error::invalid_params().data(format!(
                "Reasoning effort `{}` is not supported by model `{}`",
                reasoning_effort_value(effort),
                model.display_name,
            )));
        }
        inner.reasoning_effort = effort;
        inner.model_selector.default_reasoning_effort = Some(effort);
        persist_selector_preferences_or_warn(&inner);
        Ok(())
    }

    pub async fn set_fast_mode(
        &self,
        service_tier: Option<codex_protocol::config_types::ServiceTier>,
    ) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        inner.service_tier = service_tier;
        inner.model_selector.default_service_tier = service_tier;
        persist_selector_preferences_or_warn(&inner);
        Ok(())
    }

    pub async fn set_status_control(&self, value: SessionConfigValueId) -> Result<(), Error> {
        if let Some(option_id) = parse_limits_summary_value(&value.0) {
            {
                let mut inner = self.inner.lock().await;
                if !inner
                    .display_maps
                    .set_limits_summary_by_option_id(option_id)
                {
                    return Err(
                        Error::invalid_params().data("Unsupported status limits summary option")
                    );
                }
                persist_display_maps_or_warn(&inner);
            }
            self.notify_config_options_update().await;
            return Ok(());
        }

        match value.0.as_ref() {
            STATUS_LIMITS_VALUE | SESSION_STATUS_VALUE | MCP_STATUS_VALUE | SKILLS_STATUS_VALUE
            | PLUGINS_STATUS_VALUE => Ok(()),
            STATUS_COMPACT_VALUE => {
                if self.request_context_compaction_ext().await? {
                    self.spawn_compaction_drain_task();
                    let drain_outcome = self
                        .drain_background_notifications_for_ext(COMPACTION_SELECTOR_DRAIN_TIMEOUT)
                        .await?;
                    if drain_outcome.was_truncated() {
                        warn!(
                            processed_messages = drain_outcome.processed(),
                            outcome = ?drain_outcome,
                            "status compact action drain stopped before the queue went quiet"
                        );
                    }
                }
                Ok(())
            }
            _ => Err(Error::invalid_params().data("Unsupported status action")),
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
            "status" => self.set_status_control(value).await,
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
                    self.set_model(value.0.to_string()).await
                }
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
                        inner.cas_home.clone(),
                        inner.workspace_cwd.clone(),
                        inner.agent_labels.clone(),
                        response.thread.turns,
                    )
                }),
            )
        };

        if let Some((client, cas_home, workspace_cwd, agent_labels, turns)) = replay_data {
            replay::replay_turns(&client, &cas_home, &workspace_cwd, &agent_labels, turns).await;
            let mut inner = self.inner.lock().await;
            inner.history_replay_in_progress = false;
        }

        Ok(remaining_turns)
    }
}
