//! Загрузчики session view и метаданных, которые ACP использует для resume и восстановления контекста.

use std::time::Instant;

use tracing::{info, warn};

use super::{
    AvailableCommandsUpdate, ConfigOptionUpdate, CurrentModeUpdate, Error, LoadSessionResponse,
    SessionConfigOption, SessionUpdate, Thread, session_config, turn_notify,
};
use crate::thread::{
    features::collab,
    prompt_commands, replay,
    session_config::{build_account_status, build_plugins_summary},
    session_lifecycle::load_session_skills_summary_for_cwd,
};
use codex_app_server_protocol::PluginListParams;

impl Thread {
    pub async fn mark_history_replay_pending(&self) {
        let mut inner = self.inner.lock().await;
        if !inner.replay_turns.is_empty() {
            inner.history_replay_in_progress = true;
        }
    }

    // Не делаем лишний blocking model/list на bootstrap, если модели уже пришли из start/resume.
    pub async fn load(&self) -> Result<LoadSessionResponse, Error> {
        let app = {
            let inner = self.inner.lock().await;
            inner.app.clone()
        };

        let models = {
            let inner = self.inner.lock().await;
            (!inner.models.is_empty()).then(|| inner.models.clone())
        };

        if models.is_none() {
            match app.lock().await.model_list().await {
                Ok(response) => {
                    let mut inner = self.inner.lock().await;
                    if inner.models.is_empty() {
                        inner.models = response.data;
                    }
                }
                Err(error) => {
                    warn!(
                        error = %error,
                        "Failed to refresh model list while loading session view"
                    );
                }
            }
        }

        let inner = self.inner.lock().await;
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
        let (thread_id, workspace_cwd, codex_home, bundled_skills_enabled, app) = {
            let inner = self.inner.lock().await;
            (
                inner.thread_id.clone(),
                inner.workspace_cwd.clone(),
                inner.codex_home.clone(),
                inner.bundled_skills_enabled,
                inner.app.clone(),
            )
        };

        let session_skills_summary = load_session_skills_summary_for_cwd(
            &codex_home,
            bundled_skills_enabled,
            &workspace_cwd,
        )
        .await;

        let plugin_cwds = workspace_cwd.clone().try_into().ok().map(|cwd| vec![cwd]);

        let (account_rate_limits, account_status, session_plugins_summary) = {
            let mut app = app.lock().await;
            let account_rate_limits = match app.get_account_rate_limits().await {
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
            let account_status = match app.get_account().await {
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
            let session_plugins_summary = match app
                .plugin_list(PluginListParams {
                    cwds: plugin_cwds,
                    force_remote_sync: false,
                })
                .await
            {
                Ok(response) => build_plugins_summary(&response),
                Err(error) => {
                    warn!(
                        error = %error,
                        thread_id,
                        "Failed to read plugins during deferred startup metadata refresh"
                    );
                    session_config::ContextSelectorSummary {
                        label: "Plugins · unavailable".to_string(),
                        description: "Failed to read plugin status for this session.".to_string(),
                        report: format!(
                            "Failed to read plugin status for this session.\n\nError: {error}"
                        ),
                    }
                }
            };
            (account_rate_limits, account_status, session_plugins_summary)
        };

        let mut inner = self.inner.lock().await;
        inner.account_rate_limits = account_rate_limits;
        inner.account_status = account_status;
        if inner.workspace_cwd == workspace_cwd {
            inner.session_skills_summary = session_skills_summary;
            inner.session_plugins_summary = session_plugins_summary;
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
            if inner.replay_turns.is_empty() {
                // A duplicate replay task can arrive while the first one is still
                // streaming history. Keep the guard up until the owning task finishes.
                return;
            }
            let turns = std::mem::take(&mut inner.replay_turns);
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
