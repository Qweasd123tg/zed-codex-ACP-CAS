//! Общие helper-ы для переключения backend-thread внутри одной ACP-сессии.

use std::time::Duration;

use agent_client_protocol::Error;
use codex_app_server_protocol::JSONRPCMessage;
use tracing::warn;

use crate::app_server::recv_message_from_inbox;
use crate::thread::{SharedAppServer, notification_dispatch::DrainOutcome};

const THREAD_SWITCH_TRANSPORT_FLUSH_TOTAL_TIMEOUT_MS: u64 = 300;
const THREAD_SWITCH_TRANSPORT_FLUSH_TIMEOUT_MS: u64 = 20;
const THREAD_SWITCH_TRANSPORT_FLUSH_IDLE_POLLS: usize = 2;
const THREAD_SWITCH_TRANSPORT_FLUSH_MAX_MESSAGES: usize = 256;

pub(in crate::thread) async fn flush_thread_switch_transport_state(
    app: &SharedAppServer,
) -> Result<(), Error> {
    let inbox = {
        let app = app.lock().await;
        app.message_inbox()
    };
    let deadline = tokio::time::Instant::now()
        + Duration::from_millis(THREAD_SWITCH_TRANSPORT_FLUSH_TOTAL_TIMEOUT_MS);
    let mut processed = 0;
    let mut quiet_polls = 0;

    let outcome = loop {
        if processed >= THREAD_SWITCH_TRANSPORT_FLUSH_MAX_MESSAGES {
            break DrainOutcome::HitLimit { processed };
        }

        let now = tokio::time::Instant::now();
        if now >= deadline {
            break DrainOutcome::TimedOut { processed };
        }

        let remaining = deadline - now;
        let wait_for = remaining.min(Duration::from_millis(
            THREAD_SWITCH_TRANSPORT_FLUSH_TIMEOUT_MS,
        ));
        let message = match tokio::time::timeout(wait_for, recv_message_from_inbox(&inbox)).await {
            Ok(message) => {
                quiet_polls = 0;
                message?
            }
            Err(_) => {
                quiet_polls += 1;
                if quiet_polls >= THREAD_SWITCH_TRANSPORT_FLUSH_IDLE_POLLS {
                    break DrainOutcome::Drained { processed };
                }
                continue;
            }
        };
        processed += 1;
        handle_stale_thread_switch_message(app, message).await?;
    };

    if matches!(
        outcome,
        DrainOutcome::TimedOut { .. } | DrainOutcome::HitLimit { .. }
    ) {
        warn!(
            processed_messages = processed,
            timeout_ms = THREAD_SWITCH_TRANSPORT_FLUSH_TIMEOUT_MS,
            total_timeout_ms = THREAD_SWITCH_TRANSPORT_FLUSH_TOTAL_TIMEOUT_MS,
            outcome = ?outcome,
            "thread-switch transport flush stopped before the queue went quiet"
        );
    }
    Ok(())
}

async fn handle_stale_thread_switch_message(
    app: &SharedAppServer,
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
            app.lock()
                .await
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
