//! Загрузчики session view и метаданных, которые ACP использует для resume и восстановления контекста.

use std::time::Instant;

use tracing::{info, warn};

use super::{
    AvailableCommandsUpdate, ConfigOptionUpdate, CurrentModeUpdate, Error, LoadSessionResponse,
    SessionConfigOption, SessionUpdate, Thread, session_config, turn_notify,
};
use crate::thread::{
    features::collab, prompt_commands, replay, session_config::build_account_status,
    session_lifecycle::load_session_skills_summary_for_cwd,
};

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

    pub async fn refresh_startup_metadata(&self) {
        let started_at = Instant::now();
        let (thread_id, workspace_cwd, codex_home, bundled_skills_enabled) = {
            let inner = self.inner.lock().await;
            (
                inner.thread_id.clone(),
                inner.workspace_cwd.clone(),
                inner.codex_home.clone(),
                inner.bundled_skills_enabled,
            )
        };

        let session_skills_summary = load_session_skills_summary_for_cwd(
            &codex_home,
            bundled_skills_enabled,
            &workspace_cwd,
        )
        .await;

        let (account_rate_limits, account_status) = {
            let mut inner = self.inner.lock().await;
            let account_rate_limits = match inner.app.get_account_rate_limits().await {
                Ok(response) => Some(response.rate_limits),
                Err(error) => {
                    warn!(
                        error = %error,
                        thread_id,
                        "Failed to read rate limits during deferred startup metadata refresh"
                    );
                    None
                }
            };
            let account_status = match inner.app.get_account().await {
                Ok(response) => build_account_status(response.account),
                Err(error) => {
                    warn!(
                        error = %error,
                        thread_id,
                        "Failed to read account status during deferred startup metadata refresh"
                    );
                    Default::default()
                }
            };
            (account_rate_limits, account_status)
        };

        let mut inner = self.inner.lock().await;
        inner.account_rate_limits = account_rate_limits;
        inner.account_status = account_status;
        if inner.workspace_cwd == workspace_cwd {
            inner.session_skills_summary = session_skills_summary;
        }
        turn_notify::notify_config_update(&inner).await;

        info!(
            thread_id = %inner.thread_id,
            elapsed_ms = started_at.elapsed().as_millis() as u64,
            "Finished deferred session startup metadata refresh"
        );
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
