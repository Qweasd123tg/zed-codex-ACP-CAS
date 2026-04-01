//! Верхнеуровневый диспетчер, который маршрутизирует JSON-RPC-уведомления в типизированные обработчики thread.

use std::time::Duration;

use codex_app_server_protocol::JSONRPCMessage;
use tracing::warn;

use crate::thread::{
    Error, SharedAppServer, StopReason, Thread, ThreadInner, features::notification,
    server_requests::handle_server_request,
};

const POST_TURN_DRAIN_POLL_TIMEOUT: Duration = Duration::from_millis(20);
const POST_TURN_DRAIN_IDLE_POLLS: usize = 2;
const POST_TURN_DRAIN_MAX_MESSAGES: usize = 256;
const BACKGROUND_DRAIN_TOTAL_TIMEOUT: Duration = Duration::from_millis(250);
const BACKGROUND_DRAIN_POLL_TIMEOUT: Duration = Duration::from_millis(10);
const BACKGROUND_DRAIN_IDLE_POLLS: usize = 2;
const BACKGROUND_DRAIN_MAX_MESSAGES: usize = 256;

struct DrainConfig {
    drain_context: &'static str,
    total_timeout: Duration,
    poll_timeout: Duration,
    idle_polls_to_drain: usize,
    max_messages: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::thread) enum DrainOutcome {
    Drained { processed: usize },
    TimedOut { processed: usize },
    HitLimit { processed: usize },
}

impl DrainOutcome {
    pub(super) fn processed(self) -> usize {
        match self {
            Self::Drained { processed }
            | Self::TimedOut { processed }
            | Self::HitLimit { processed } => processed,
        }
    }

    pub(super) fn was_truncated(self) -> bool {
        matches!(self, Self::TimedOut { .. } | Self::HitLimit { .. })
    }
}

// Рано отбрасываем шум вне текущего turn, чтобы состояние клиента оставалось консистентным.
pub(super) async fn handle_message(
    inner: &mut ThreadInner,
    message: codex_app_server_protocol::JSONRPCMessage,
    expected_turn_id: &str,
) -> Result<Option<StopReason>, Error> {
    match message {
        codex_app_server_protocol::JSONRPCMessage::Notification(notification) => {
            notification::handle_notification(inner, notification, expected_turn_id).await
        }
        codex_app_server_protocol::JSONRPCMessage::Request(request) => {
            handle_server_request(inner, request).await?;
            Ok(None)
        }
        codex_app_server_protocol::JSONRPCMessage::Response(response) => {
            warn!("Ignoring unexpected app-server response: {:?}", response.id);
            Ok(None)
        }
        codex_app_server_protocol::JSONRPCMessage::Error(error) => {
            warn!(
                "Ignoring unexpected app-server error: {}",
                error.error.message
            );
            Ok(None)
        }
    }
}

async fn handle_drain_message(
    inner: &mut ThreadInner,
    message: codex_app_server_protocol::JSONRPCMessage,
    expected_turn_id: &str,
    drain_context: &str,
) -> Result<(), Error> {
    match message {
        codex_app_server_protocol::JSONRPCMessage::Notification(notification) => {
            let _ =
                notification::handle_notification(inner, notification, expected_turn_id).await?;
        }
        codex_app_server_protocol::JSONRPCMessage::Request(request) => {
            warn!(
                method = %request.method,
                context = drain_context,
                "rejecting stale app-server request during transport drain"
            );
            inner
                .app
                .lock()
                .await
                .send_server_request_error(
                    request.id,
                    -32600,
                    format!(
                        "Dropping stale app-server request `{}` during {}",
                        request.method, drain_context
                    ),
                    None,
                )
                .await?;
        }
        codex_app_server_protocol::JSONRPCMessage::Response(response) => {
            warn!(
                id = ?response.id,
                context = drain_context,
                "ignoring unexpected app-server response during transport drain"
            );
        }
        codex_app_server_protocol::JSONRPCMessage::Error(error) => {
            warn!(
                id = ?error.id,
                message = error.error.message,
                context = drain_context,
                "ignoring unexpected app-server error during transport drain"
            );
        }
    }
    Ok(())
}

async fn receive_drain_message(
    app: &SharedAppServer,
    wait_for: Duration,
) -> Result<Option<JSONRPCMessage>, Error> {
    match tokio::time::timeout(wait_for, async {
        let mut app = app.lock().await;
        app.next_message().await
    })
    .await
    {
        Ok(message) => Ok(Some(message?)),
        Err(_) => Ok(None),
    }
}

async fn drain_transport_until_quiet_ext(
    thread: &Thread,
    app: &SharedAppServer,
    expected_turn_id: &str,
    config: DrainConfig,
) -> Result<DrainOutcome, Error> {
    let deadline = tokio::time::Instant::now() + config.total_timeout;
    let mut processed = 0;
    let mut quiet_polls = 0;

    loop {
        if processed >= config.max_messages {
            return Ok(DrainOutcome::HitLimit { processed });
        }

        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Ok(DrainOutcome::TimedOut { processed });
        }

        let remaining = deadline - now;
        let wait_for = remaining.min(config.poll_timeout);
        let Some(message) = receive_drain_message(app, wait_for).await? else {
            quiet_polls += 1;
            if quiet_polls >= config.idle_polls_to_drain {
                return Ok(DrainOutcome::Drained { processed });
            }
            continue;
        };

        quiet_polls = 0;
        processed += 1;
        let mut inner = thread.inner.lock().await;
        handle_drain_message(&mut inner, message, expected_turn_id, config.drain_context).await?;
    }
}

impl Thread {
    pub(super) async fn drain_post_turn_notifications_ext(
        &self,
        expected_turn_id: &str,
        timeout: Duration,
    ) -> Result<DrainOutcome, Error> {
        let app = {
            let inner = self.inner.lock().await;
            inner.app.clone()
        };

        drain_transport_until_quiet_ext(
            self,
            &app,
            expected_turn_id,
            DrainConfig {
                drain_context: "post-turn drain",
                total_timeout: timeout,
                poll_timeout: POST_TURN_DRAIN_POLL_TIMEOUT,
                idle_polls_to_drain: POST_TURN_DRAIN_IDLE_POLLS,
                max_messages: POST_TURN_DRAIN_MAX_MESSAGES,
            },
        )
        .await
    }

    pub(super) async fn drain_background_notifications_ext(&self) -> Result<DrainOutcome, Error> {
        let app = {
            let inner = self.inner.lock().await;
            inner.app.clone()
        };

        drain_transport_until_quiet_ext(
            self,
            &app,
            "",
            DrainConfig {
                drain_context: "background drain",
                total_timeout: BACKGROUND_DRAIN_TOTAL_TIMEOUT,
                poll_timeout: BACKGROUND_DRAIN_POLL_TIMEOUT,
                idle_polls_to_drain: BACKGROUND_DRAIN_IDLE_POLLS,
                max_messages: BACKGROUND_DRAIN_MAX_MESSAGES,
            },
        )
        .await
    }
}
