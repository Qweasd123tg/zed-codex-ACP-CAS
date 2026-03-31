//! Загрузчики session view и метаданных, которые ACP использует для resume и восстановления контекста.

use tracing::warn;

use super::{
    AvailableCommandsUpdate, ConfigOptionUpdate, CurrentModeUpdate, Error, LoadSessionResponse,
    SessionConfigOption, SessionUpdate, Thread, session_config,
};
use crate::thread::{features::collab, prompt_commands, replay};

impl Thread {
    pub async fn mark_history_replay_pending(&self) {
        let mut inner = self.inner.lock().await;
        if !inner.replay_turns.is_empty() {
            inner.history_replay_in_progress = true;
        }
    }

    // Не делаем лишний blocking model/list на bootstrap, если модели уже пришли из start/resume.
    pub async fn load(&self) -> Result<LoadSessionResponse, Error> {
        let mut inner = self.inner.lock().await;
        if inner.models.is_empty() {
            match inner.app.model_list().await {
                Ok(models) => inner.models = models.data,
                Err(error) => {
                    warn!(
                        error = %error,
                        "Failed to refresh model list while loading session view"
                    );
                }
            }
        }

        Ok(LoadSessionResponse::new()
            .models(session_config::session_model_state(
                &inner.models,
                &inner.current_model,
            ))
            .modes(Some(session_config::mode_state(
                inner.collaboration_mode_kind,
            )))
            .config_options(session_config::config_options(
                session_config::config_options_input(&inner),
            )))
    }

    pub async fn config_options(&self) -> Result<Vec<SessionConfigOption>, Error> {
        let inner = self.inner.lock().await;
        Ok(session_config::config_options(
            session_config::config_options_input(&inner),
        ))
    }

    pub async fn notify_config_options_update(&self) {
        let (client, options) = {
            let inner = self.inner.lock().await;
            (
                inner.client.clone(),
                session_config::config_options(session_config::config_options_input(&inner)),
            )
        };
        client
            .send_notification(SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(
                options,
            )))
            .await;
    }

    pub async fn notify_current_mode_update(&self) {
        let (client, current_mode_id) = {
            let inner = self.inner.lock().await;
            (
                inner.client.clone(),
                session_config::mode_state(inner.collaboration_mode_kind).current_mode_id,
            )
        };
        client
            .send_notification(SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(
                current_mode_id,
            )))
            .await;
    }

    pub async fn notify_available_commands(&self) {
        let client = {
            let inner = self.inner.lock().await;
            inner.client.clone()
        };
        client
            .send_notification(SessionUpdate::AvailableCommandsUpdate(
                AvailableCommandsUpdate::new(prompt_commands::builtin_commands()),
            ))
            .await;
    }

    pub async fn replay_loaded_history(&self) {
        let replay = {
            let mut inner = self.inner.lock().await;
            let turns = std::mem::take(&mut inner.replay_turns);
            if turns.is_empty() {
                inner.history_replay_in_progress = false;
                return;
            }
            inner.history_replay_in_progress = true;
            collab::warm_agent_labels_for_turns(&mut inner, &turns).await;
            Some((
                inner.client.clone(),
                inner.workspace_cwd.clone(),
                turns,
                inner.agent_labels.clone(),
            ))
        };

        let Some((client, workspace_cwd, turns, agent_labels)) = replay else {
            return;
        };
        replay::replay_turns(&client, &workspace_cwd, &agent_labels, turns).await;

        let mut inner = self.inner.lock().await;
        inner.history_replay_in_progress = false;
    }
}
