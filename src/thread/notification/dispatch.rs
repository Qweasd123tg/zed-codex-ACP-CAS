//! Top-level dispatcher that routes JSON-RPC notifications to typed thread handlers.

use tracing::warn;

use crate::thread::{
    Error, StopReason, ThreadInner, features::notification, server_requests::handle_server_request,
};

// Drop out-of-turn noise early so client state stays consistent.
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

pub(super) async fn drain_post_turn_notifications(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    timeout: std::time::Duration,
) -> Result<(), Error> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline - now;
        let message = match tokio::time::timeout(remaining, inner.app.next_message()).await {
            Ok(message) => message?,
            Err(_) => break,
        };
        let _ = handle_message(inner, message, expected_turn_id).await?;
    }
    Ok(())
}

pub(super) async fn drain_background_notifications(inner: &mut ThreadInner) -> Result<(), Error> {
    // Drain already buffered app-server notifications before starting a new turn.
    // This keeps compaction state and usage indicators synchronized between prompts.
    for _ in 0..64 {
        let message = match tokio::time::timeout(
            std::time::Duration::from_millis(5),
            inner.app.next_message(),
        )
        .await
        {
            Ok(message) => message?,
            Err(_) => break,
        };
        let _ = handle_message(inner, message, "").await?;
    }
    Ok(())
}
