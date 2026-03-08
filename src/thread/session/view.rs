//! Session view and metadata loaders used by ACP for resume and context restoration.

use super::{
    AvailableCommandsUpdate, ConfigOptionUpdate, CurrentModeUpdate, Error, LoadSessionResponse,
    SessionConfigOption, SessionUpdate, Thread, session_config,
};
use crate::thread::{prompt_commands, replay};

impl Thread {
    // Opportunistically refresh the model cache before serving load requests.
    pub async fn load(&self) -> Result<LoadSessionResponse, Error> {
        let mut inner = self.inner.lock().await;
        if let Ok(models) = inner.app.model_list().await {
            inner.models = models.data;
        }

        Ok(LoadSessionResponse::new()
            .models(session_config::session_model_state(
                &inner.models,
                &inner.current_model,
            ))
            .modes(Some(session_config::mode_state(
                inner.approval_policy,
                inner.sandbox_mode,
                inner.edit_approval_mode,
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
                session_config::mode_state(
                    inner.approval_policy,
                    inner.sandbox_mode,
                    inner.edit_approval_mode,
                    inner.collaboration_mode_kind,
                )
                .current_mode_id,
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
        let (client, workspace_cwd, turns) = {
            let mut inner = self.inner.lock().await;
            let turns = std::mem::take(&mut inner.replay_turns);
            (inner.client.clone(), inner.workspace_cwd.clone(), turns)
        };
        replay::replay_turns(&client, &workspace_cwd, turns).await;
    }
}
