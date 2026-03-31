//! Верхнеуровневый диспетчер, который маршрутизирует JSON-RPC-уведомления в типизированные обработчики thread.

use std::time::Duration;

use tracing::warn;

use crate::thread::{
    Error, StopReason, ThreadInner, features::notification, server_requests::handle_server_request,
};

const POST_TURN_DRAIN_POLL_TIMEOUT: Duration = Duration::from_millis(20);
const POST_TURN_DRAIN_IDLE_POLLS: usize = 2;
const POST_TURN_DRAIN_MAX_MESSAGES: usize = 256;
const BACKGROUND_DRAIN_TOTAL_TIMEOUT: Duration = Duration::from_millis(250);
const BACKGROUND_DRAIN_POLL_TIMEOUT: Duration = Duration::from_millis(10);
const BACKGROUND_DRAIN_IDLE_POLLS: usize = 2;
const BACKGROUND_DRAIN_MAX_MESSAGES: usize = 256;

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

async fn drain_transport_until_quiet(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    drain_context: &str,
    total_timeout: Duration,
    poll_timeout: Duration,
    idle_polls_to_drain: usize,
    max_messages: usize,
) -> Result<DrainOutcome, Error> {
    let deadline = tokio::time::Instant::now() + total_timeout;
    let mut processed = 0;
    let mut quiet_polls = 0;

    loop {
        if processed >= max_messages {
            return Ok(DrainOutcome::HitLimit { processed });
        }

        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Ok(DrainOutcome::TimedOut { processed });
        }

        let remaining = deadline - now;
        let wait_for = remaining.min(poll_timeout);
        let message = match tokio::time::timeout(wait_for, inner.app.next_message()).await {
            Ok(message) => {
                quiet_polls = 0;
                message?
            }
            Err(_) => {
                quiet_polls += 1;
                if quiet_polls >= idle_polls_to_drain {
                    return Ok(DrainOutcome::Drained { processed });
                }
                continue;
            }
        };
        processed += 1;
        handle_drain_message(inner, message, expected_turn_id, drain_context).await?;
    }
}

pub(super) async fn drain_post_turn_notifications(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    timeout: std::time::Duration,
) -> Result<DrainOutcome, Error> {
    drain_transport_until_quiet(
        inner,
        expected_turn_id,
        "post-turn drain",
        timeout,
        POST_TURN_DRAIN_POLL_TIMEOUT,
        POST_TURN_DRAIN_IDLE_POLLS,
        POST_TURN_DRAIN_MAX_MESSAGES,
    )
    .await
}

pub(super) async fn drain_background_notifications(
    inner: &mut ThreadInner,
) -> Result<DrainOutcome, Error> {
    // Выгружаем уже буферизованные уведомления app-server перед стартом нового turn.
    // Это синхронизирует состояние compaction и индикаторы usage между промптами.
    drain_transport_until_quiet(
        inner,
        "",
        "background drain",
        BACKGROUND_DRAIN_TOTAL_TIMEOUT,
        BACKGROUND_DRAIN_POLL_TIMEOUT,
        BACKGROUND_DRAIN_IDLE_POLLS,
        BACKGROUND_DRAIN_MAX_MESSAGES,
    )
    .await
}
