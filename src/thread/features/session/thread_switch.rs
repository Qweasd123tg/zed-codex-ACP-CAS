//! Общие helper-ы для переключения backend-thread внутри одной ACP-сессии.

use std::time::Duration;

use agent_client_protocol::Error;
use codex_app_server_protocol::JSONRPCMessage;
use tracing::warn;

use crate::thread::ThreadInner;

const THREAD_SWITCH_TRANSPORT_FLUSH_TIMEOUT_MS: u64 = 20;
const THREAD_SWITCH_TRANSPORT_FLUSH_MAX_MESSAGES: usize = 64;

pub(in crate::thread) async fn flush_thread_switch_transport_state(
    inner: &mut ThreadInner,
) -> Result<(), Error> {
    for _ in 0..THREAD_SWITCH_TRANSPORT_FLUSH_MAX_MESSAGES {
        let message = match tokio::time::timeout(
            Duration::from_millis(THREAD_SWITCH_TRANSPORT_FLUSH_TIMEOUT_MS),
            inner.app.next_message(),
        )
        .await
        {
            Ok(message) => message?,
            Err(_) => break,
        };
        handle_stale_thread_switch_message(inner, message).await?;
    }
    Ok(())
}

async fn handle_stale_thread_switch_message(
    inner: &mut ThreadInner,
    message: JSONRPCMessage,
) -> Result<(), Error> {
    match message {
        JSONRPCMessage::Notification(notification) => {
            warn!(
                method = %notification.method,
                "dropping stale app-server notification during thread switch"
            );
        }
        JSONRPCMessage::Request(request) => {
            warn!(
                method = %request.method,
                "rejecting stale app-server request during thread switch"
            );
            inner
                .app
                .send_server_request_error(
                    request.id,
                    -32600,
                    format!(
                        "Dropping stale app-server request `{}` during thread switch",
                        request.method
                    ),
                    None,
                )
                .await?;
        }
        JSONRPCMessage::Response(response) => {
            warn!(
                id = ?response.id,
                "dropping unexpected app-server response during thread switch"
            );
        }
        JSONRPCMessage::Error(error) => {
            warn!(
                id = ?error.id,
                message = error.error.message,
                "dropping unexpected app-server error during thread switch"
            );
        }
    }

    Ok(())
}
