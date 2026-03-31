//! Верхнеуровневый диспетчер, который маршрутизирует JSON-RPC-уведомления в типизированные обработчики thread.

use tracing::warn;

use crate::thread::{
    Error, StopReason, ThreadInner, features::notification, server_requests::handle_server_request,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DrainOutcome {
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

pub(super) async fn drain_post_turn_notifications(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    timeout: std::time::Duration,
) -> Result<DrainOutcome, Error> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut processed = 0;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Ok(DrainOutcome::TimedOut { processed });
        }
        let remaining = deadline - now;
        let message = match tokio::time::timeout(remaining, inner.app.next_message()).await {
            Ok(message) => message?,
            Err(_) => return Ok(DrainOutcome::Drained { processed }),
        };
        processed += 1;
        handle_drain_message(inner, message, expected_turn_id, "post-turn drain").await?;
    }
}

pub(super) async fn drain_background_notifications(
    inner: &mut ThreadInner,
) -> Result<DrainOutcome, Error> {
    // Выгружаем уже буферизованные уведомления app-server перед стартом нового turn.
    // Это синхронизирует состояние compaction и индикаторы usage между промптами.
    let mut processed = 0;
    for _ in 0..64 {
        let message = match tokio::time::timeout(
            std::time::Duration::from_millis(5),
            inner.app.next_message(),
        )
        .await
        {
            Ok(message) => message?,
            Err(_) => return Ok(DrainOutcome::Drained { processed }),
        };
        processed += 1;
        handle_drain_message(inner, message, "", "background drain").await?;
    }
    Ok(DrainOutcome::HitLimit { processed })
}
