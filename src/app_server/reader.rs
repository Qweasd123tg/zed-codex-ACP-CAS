use std::sync::{Arc, Mutex};

use agent_client_protocol::Error;
use codex_app_server_protocol::{
    JSONRPCError, JSONRPCErrorError, JSONRPCMessage, JSONRPCResponse, RequestId,
};
use tokio::io::{AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
use tracing::warn;

use super::io_error;
use super::request_policy::should_reject_request_during_startup;

const JSONRPC_INVALID_REQUEST: i64 = -32600;

pub(super) type SharedAppStdin = Arc<AsyncMutex<ChildStdin>>;
pub(crate) type SharedAppMessageInbox =
    Arc<AsyncMutex<mpsc::UnboundedReceiver<Result<JSONRPCMessage, Error>>>>;
pub(super) type ActiveRequestSlot = Arc<Mutex<Option<ActiveRequest>>>;

pub(super) struct ActiveRequest {
    request_id: RequestId,
    method_name: String,
    reject_startup_requests: bool,
    response_tx: oneshot::Sender<Result<JSONRPCMessage, Error>>,
}

pub(super) struct ActiveRequestGuard {
    active_request: ActiveRequestSlot,
    request_id: RequestId,
}

impl ActiveRequestGuard {
    pub(super) fn install(
        active_request: &ActiveRequestSlot,
        request_id: RequestId,
        method_name: &str,
        reject_startup_requests: bool,
        response_tx: oneshot::Sender<Result<JSONRPCMessage, Error>>,
    ) -> Result<Self, Error> {
        let mut slot = active_request
            .lock()
            .map_err(|_| io_error("app-server active request mutex poisoned"))?;
        if slot.is_some() {
            return Err(io_error(
                "concurrent app-server requests are not supported for one transport",
            ));
        }
        *slot = Some(ActiveRequest {
            request_id: request_id.clone(),
            method_name: method_name.to_string(),
            reject_startup_requests,
            response_tx,
        });
        drop(slot);
        Ok(Self {
            active_request: Arc::clone(active_request),
            request_id,
        })
    }
}

impl Drop for ActiveRequestGuard {
    fn drop(&mut self) {
        let Ok(mut slot) = self.active_request.lock() else {
            return;
        };
        if slot
            .as_ref()
            .is_some_and(|active| active.request_id == self.request_id)
        {
            slot.take();
        }
    }
}

enum ReaderAction {
    DeliverResponse {
        response_tx: oneshot::Sender<Result<JSONRPCMessage, Error>>,
        message: JSONRPCMessage,
    },
    RejectStartupRequest {
        request_id: RequestId,
        request_method: String,
        awaited_method: String,
    },
    Forward(JSONRPCMessage),
}

pub(super) async fn drive_app_server_stdout(
    mut stdout: Lines<BufReader<ChildStdout>>,
    stdin: SharedAppStdin,
    message_tx: mpsc::UnboundedSender<Result<JSONRPCMessage, Error>>,
    active_request: ActiveRequestSlot,
) {
    loop {
        let message = match read_message_from_stdout(&mut stdout).await {
            Ok(message) => message,
            Err(error) => {
                fail_active_request(&active_request, &error);
                drop(message_tx.send(Err(error)));
                return;
            }
        };

        match classify_reader_action(&active_request, message) {
            ReaderAction::DeliverResponse {
                response_tx,
                message,
            } => {
                drop(response_tx.send(Ok(message)));
            }
            ReaderAction::RejectStartupRequest {
                request_id,
                request_method,
                awaited_method,
            } => {
                warn!(
                    awaited_method,
                    request_method,
                    request_id = ?request_id,
                    "Rejecting unsupported app-server request during startup-sensitive handshake"
                );
                if let Err(error) = send_server_request_error_via_stdin(
                    &stdin,
                    request_id,
                    JSONRPC_INVALID_REQUEST,
                    format!(
                        "Cannot handle app-server request `{request_method}` while awaiting `{awaited_method}` during startup"
                    ),
                    None,
                )
                .await
                {
                    fail_active_request(&active_request, &error);
                    drop(message_tx.send(Err(error)));
                    return;
                }
            }
            ReaderAction::Forward(message) => {
                if message_tx.send(Ok(message)).is_err() {
                    return;
                }
            }
        }
    }
}

fn classify_reader_action(
    active_request: &ActiveRequestSlot,
    message: JSONRPCMessage,
) -> ReaderAction {
    match &message {
        JSONRPCMessage::Response(JSONRPCResponse { id, .. })
        | JSONRPCMessage::Error(JSONRPCError { id, .. }) => {
            if let Ok(mut slot) = active_request.lock()
                && slot.as_ref().is_some_and(|active| active.request_id == *id)
            {
                let active = slot.take().expect("active request should exist");
                return ReaderAction::DeliverResponse {
                    response_tx: active.response_tx,
                    message,
                };
            }
        }
        JSONRPCMessage::Request(request) => {
            if let Ok(slot) = active_request.lock()
                && let Some(active) = slot.as_ref()
                && active.reject_startup_requests
                && should_reject_request_during_startup(&request.method)
            {
                return ReaderAction::RejectStartupRequest {
                    request_id: request.id.clone(),
                    request_method: request.method.clone(),
                    awaited_method: active.method_name.clone(),
                };
            }
        }
        _ => {}
    }

    ReaderAction::Forward(message)
}

fn fail_active_request(active_request: &ActiveRequestSlot, error: &Error) {
    let Ok(mut slot) = active_request.lock() else {
        return;
    };
    if let Some(active) = slot.take() {
        drop(active.response_tx.send(Err(io_error(error.to_string()))));
    }
}

async fn read_message_from_stdout(
    stdout: &mut Lines<BufReader<ChildStdout>>,
) -> Result<JSONRPCMessage, Error> {
    loop {
        let line = stdout
            .next_line()
            .await
            .map_err(|err| io_error(format!("failed to read app-server output: {err}")))?;

        let Some(line) = line else {
            return Err(io_error("codex app-server closed stdout"));
        };

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<JSONRPCMessage>(line) {
            Ok(message) => return Ok(message),
            Err(err) => {
                warn!("Ignoring non JSON-RPC line from app-server: {err}");
            }
        }
    }
}

pub(super) async fn write_line_to_stdin(stdin: &SharedAppStdin, line: String) -> Result<(), Error> {
    let mut stdin = stdin.lock().await;
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|err| io_error(format!("failed to write app-server input: {err}")))?;
    stdin
        .flush()
        .await
        .map_err(|err| io_error(format!("failed to flush app-server input: {err}")))?;
    Ok(())
}

async fn send_server_request_error_via_stdin(
    stdin: &SharedAppStdin,
    request_id: RequestId,
    code: i64,
    message: impl Into<String>,
    data: Option<serde_json::Value>,
) -> Result<(), Error> {
    let payload = JSONRPCMessage::Error(JSONRPCError {
        id: request_id,
        error: JSONRPCErrorError {
            code,
            data,
            message: message.into(),
        },
    });
    let mut line = serde_json::to_string(&payload)
        .map_err(|err| io_error(format!("failed to serialize JSON-RPC payload: {err}")))?;
    line.push('\n');
    write_line_to_stdin(stdin, line).await
}

pub(crate) async fn recv_message_from_inbox(
    inbox: &SharedAppMessageInbox,
) -> Result<JSONRPCMessage, Error> {
    let mut inbox = inbox.lock().await;
    match inbox.recv().await {
        Some(message) => message,
        None => Err(io_error("codex app-server message inbox closed")),
    }
}
