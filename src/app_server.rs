//! Тонкий асинхронный JSON-RPC-клиент вокруг `codex app-server --listen stdio://`.
//! Отвечает только за транспорт/мультиплексирование между логикой ACP-thread
//! и протоколом Codex app-server.

use std::env;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::Error;
use anyhow::Context;
use codex_app_server_protocol::{
    ClientInfo, ClientNotification, ClientRequest, FileChangeRequestApprovalResponse,
    GetAccountParams, GetAccountRateLimitsResponse, GetAccountResponse, InitializeCapabilities,
    InitializeParams, InitializeResponse, JSONRPCError, JSONRPCErrorError, JSONRPCMessage,
    JSONRPCResponse, ModelListParams, ModelListResponse, PermissionsRequestApprovalResponse,
    PluginListParams, PluginListResponse, RequestId, ReviewStartParams, ReviewStartResponse,
    ThreadArchiveParams, ThreadArchiveResponse, ThreadCompactStartParams,
    ThreadCompactStartResponse, ThreadForkParams, ThreadForkResponse, ThreadListParams,
    ThreadListResponse, ThreadReadParams, ThreadReadResponse, ThreadResumeParams,
    ThreadResumeResponse, ThreadRollbackParams, ThreadRollbackResponse, ThreadSetNameParams,
    ThreadSetNameResponse, ThreadStartParams, ThreadStartResponse, ThreadUnarchiveParams,
    ThreadUnarchiveResponse, ToolRequestUserInputResponse, TurnInterruptParams,
    TurnInterruptResponse, TurnStartParams, TurnStartResponse,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::Command as TokioCommand;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
use tracing::{debug, info, warn};

const STARTUP_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const STARTUP_METADATA_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const STARTUP_REQUEST_TIMEOUT_ENV: &str = "CODEX_ACP_STARTUP_TIMEOUT_MS";
const STARTUP_METADATA_REQUEST_TIMEOUT_ENV: &str = "CODEX_ACP_STARTUP_METADATA_TIMEOUT_MS";
const JSONRPC_INVALID_REQUEST: i64 = -32600;

type SharedAppStdin = Arc<AsyncMutex<ChildStdin>>;
pub(crate) type SharedAppMessageInbox =
    Arc<AsyncMutex<mpsc::UnboundedReceiver<Result<JSONRPCMessage, Error>>>>;
type ActiveRequestSlot = Arc<Mutex<Option<ActiveRequest>>>;

// Нормализуем I/O-сбои дочернего процесса в ошибки уровня протокола.
fn io_error(message: impl Into<String>) -> Error {
    Error::internal_error().data(message.into())
}

fn parse_timeout_override(value: Option<&str>, fallback: Duration) -> Duration {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .map(Duration::from_millis)
        .unwrap_or(fallback)
}

fn configured_timeout(env_name: &str, fallback: Duration) -> Duration {
    parse_timeout_override(env::var(env_name).ok().as_deref(), fallback)
}

fn request_timeout(method_name: &str) -> Option<Duration> {
    match method_name {
        "initialize" | "thread/start" | "thread/resume" | "thread/list" | "turn/start" => Some(
            configured_timeout(STARTUP_REQUEST_TIMEOUT_ENV, STARTUP_REQUEST_TIMEOUT),
        ),
        "model/list"
        | "account/rateLimits/read"
        | "account/read"
        | "thread/read"
        | "plugin/list" => Some(configured_timeout(
            STARTUP_METADATA_REQUEST_TIMEOUT_ENV,
            STARTUP_METADATA_REQUEST_TIMEOUT,
        )),
        _ => None,
    }
}

fn should_reject_request_during_startup(method_name: &str) -> bool {
    matches!(
        method_name,
        "mcpServer/elicitation/request"
            | "account/chatgptAuthTokens/refresh"
            | "applyPatchApproval"
            | "execCommandApproval"
    )
}

struct ActiveRequest {
    request_id: RequestId,
    method_name: String,
    reject_startup_requests: bool,
    response_tx: oneshot::Sender<Result<JSONRPCMessage, Error>>,
}

struct ActiveRequestGuard {
    active_request: ActiveRequestSlot,
    request_id: RequestId,
}

impl ActiveRequestGuard {
    fn install(
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

pub struct AppServerProcess {
    _child: Child,
    _reader_task: tokio::task::JoinHandle<()>,
    stdin: SharedAppStdin,
    message_inbox: SharedAppMessageInbox,
    active_request: ActiveRequestSlot,
    next_request_id: i64,
}

impl AppServerProcess {
    pub async fn spawn(codex_bin: &str) -> Result<Self, Error> {
        info!(codex_bin, "Starting codex app-server process");
        let mut cmd: TokioCommand = Command::new(codex_bin);
        cmd.arg("app-server")
            .arg("--listen")
            .arg("stdio://")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .with_context(|| {
                format!(
                    "failed to start `{codex_bin}` app-server; ensure the `codex` binary is installed and available in PATH"
                )
            })
            .map_err(|err| io_error(err.to_string()))?;
        if let Some(pid) = child.id() {
            info!(pid, codex_bin, "Spawned codex app-server child process");
        }

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io_error("codex app-server stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io_error("codex app-server stdout unavailable"))?;

        let stdin = Arc::new(AsyncMutex::new(stdin));
        let active_request = Arc::new(Mutex::new(None));
        let (message_tx, message_rx) = mpsc::unbounded_channel();
        let reader_task = tokio::spawn(drive_app_server_stdout(
            BufReader::new(stdout).lines(),
            Arc::clone(&stdin),
            message_tx,
            Arc::clone(&active_request),
        ));

        Ok(Self {
            _child: child,
            _reader_task: reader_task,
            stdin,
            message_inbox: Arc::new(AsyncMutex::new(message_rx)),
            active_request,
            next_request_id: 1,
        })
    }

    pub async fn initialize(
        &mut self,
        client_name: &str,
        client_title: &str,
    ) -> Result<InitializeResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::Initialize {
            request_id: request_id.clone(),
            params: InitializeParams {
                client_info: ClientInfo {
                    name: client_name.to_string(),
                    title: Some(client_title.to_string()),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
                capabilities: Some(InitializeCapabilities {
                    experimental_api: true,
                    opt_out_notification_methods: None,
                }),
            },
        };

        let response: InitializeResponse = self.request(request, request_id, "initialize").await?;
        self.send_client_notification(ClientNotification::Initialized)
            .await?;
        Ok(response)
    }

    pub async fn model_list(&mut self) -> Result<ModelListResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::ModelList {
            request_id: request_id.clone(),
            params: ModelListParams::default(),
        };
        self.request(request, request_id, "model/list").await
    }

    pub async fn get_account_rate_limits(&mut self) -> Result<GetAccountRateLimitsResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::GetAccountRateLimits {
            request_id: request_id.clone(),
            params: None,
        };
        self.request(request, request_id, "account/rateLimits/read")
            .await
    }

    pub async fn get_account(&mut self) -> Result<GetAccountResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::GetAccount {
            request_id: request_id.clone(),
            params: GetAccountParams {
                refresh_token: false,
            },
        };
        self.request(request, request_id, "account/read").await
    }

    pub async fn thread_start(
        &mut self,
        params: ThreadStartParams,
    ) -> Result<ThreadStartResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::ThreadStart {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "thread/start").await
    }

    pub async fn thread_resume(
        &mut self,
        params: ThreadResumeParams,
    ) -> Result<ThreadResumeResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::ThreadResume {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "thread/resume").await
    }

    pub async fn thread_fork(
        &mut self,
        params: ThreadForkParams,
    ) -> Result<ThreadForkResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::ThreadFork {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "thread/fork").await
    }

    pub async fn review_start(
        &mut self,
        params: ReviewStartParams,
    ) -> Result<ReviewStartResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::ReviewStart {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "review/start").await
    }

    pub async fn thread_list(
        &mut self,
        params: ThreadListParams,
    ) -> Result<ThreadListResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::ThreadList {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "thread/list").await
    }

    pub async fn thread_read(
        &mut self,
        params: ThreadReadParams,
    ) -> Result<ThreadReadResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::ThreadRead {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "thread/read").await
    }

    pub async fn plugin_list(
        &mut self,
        params: PluginListParams,
    ) -> Result<PluginListResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::PluginList {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "plugin/list").await
    }

    pub async fn thread_compact_start(
        &mut self,
        params: ThreadCompactStartParams,
    ) -> Result<ThreadCompactStartResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::ThreadCompactStart {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "thread/compact/start")
            .await
    }

    pub async fn thread_rollback(
        &mut self,
        params: ThreadRollbackParams,
    ) -> Result<ThreadRollbackResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::ThreadRollback {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "thread/rollback").await
    }

    pub async fn thread_set_name(
        &mut self,
        params: ThreadSetNameParams,
    ) -> Result<ThreadSetNameResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::ThreadSetName {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "thread/name/set").await
    }

    pub async fn thread_archive(
        &mut self,
        params: ThreadArchiveParams,
    ) -> Result<ThreadArchiveResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::ThreadArchive {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "thread/archive").await
    }

    pub async fn thread_unarchive(
        &mut self,
        params: ThreadUnarchiveParams,
    ) -> Result<ThreadUnarchiveResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::ThreadUnarchive {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "thread/unarchive").await
    }

    pub async fn turn_start(
        &mut self,
        params: TurnStartParams,
    ) -> Result<TurnStartResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::TurnStart {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "turn/start").await
    }

    pub async fn turn_interrupt(
        &mut self,
        params: TurnInterruptParams,
    ) -> Result<TurnInterruptResponse, Error> {
        let request_id = self.next_request_id();
        let request = ClientRequest::TurnInterrupt {
            request_id: request_id.clone(),
            params,
        };
        self.request(request, request_id, "turn/interrupt").await
    }

    pub fn message_inbox(&self) -> SharedAppMessageInbox {
        Arc::clone(&self.message_inbox)
    }

    pub async fn send_command_approval_response(
        &mut self,
        request_id: RequestId,
        response: codex_app_server_protocol::CommandExecutionRequestApprovalResponse,
    ) -> Result<(), Error> {
        self.send_server_request_response(request_id, response)
            .await
    }

    pub async fn send_file_change_approval_response(
        &mut self,
        request_id: RequestId,
        response: FileChangeRequestApprovalResponse,
    ) -> Result<(), Error> {
        self.send_server_request_response(request_id, response)
            .await
    }

    pub async fn send_tool_request_user_input_response(
        &mut self,
        request_id: RequestId,
        response: ToolRequestUserInputResponse,
    ) -> Result<(), Error> {
        self.send_server_request_response(request_id, response)
            .await
    }

    pub async fn send_permissions_request_approval_response(
        &mut self,
        request_id: RequestId,
        response: PermissionsRequestApprovalResponse,
    ) -> Result<(), Error> {
        self.send_server_request_response(request_id, response)
            .await
    }

    pub async fn send_server_request_error(
        &mut self,
        request_id: RequestId,
        code: i64,
        message: impl Into<String>,
        data: Option<serde_json::Value>,
    ) -> Result<(), Error> {
        self.write_json(&JSONRPCMessage::Error(JSONRPCError {
            id: request_id,
            error: JSONRPCErrorError {
                code,
                data,
                message: message.into(),
            },
        }))
        .await
    }

    fn next_request_id(&mut self) -> RequestId {
        let id = self.next_request_id;
        self.next_request_id += 1;
        RequestId::Integer(id)
    }

    async fn request<T>(
        &mut self,
        request: ClientRequest,
        request_id: RequestId,
        method_name: &str,
    ) -> Result<T, Error>
    where
        T: DeserializeOwned,
    {
        if let Some(timeout) = request_timeout(method_name) {
            debug!(
                method = method_name,
                timeout_ms = timeout.as_millis() as u64,
                "Sending startup-sensitive app-server request"
            );
            let response = tokio::time::timeout(
                timeout,
                self.request_inner(request, request_id, method_name, true),
            )
            .await
            .map_err(|_| {
                warn!(
                    method = method_name,
                    timeout_ms = timeout.as_millis() as u64,
                    "Timed out waiting for app-server startup response"
                );
                io_error(format!(
                    "timed out waiting for `{method_name}` response after {}s; codex app-server may be stuck during startup, auth, or early handshake",
                    timeout.as_secs()
                ))
            })??;
            debug!(method = method_name, "Received app-server response");
            return Ok(response);
        }

        self.request_inner(request, request_id, method_name, false)
            .await
    }

    async fn request_inner<T>(
        &mut self,
        request: ClientRequest,
        request_id: RequestId,
        method_name: &str,
        reject_startup_requests: bool,
    ) -> Result<T, Error>
    where
        T: DeserializeOwned,
    {
        let (response_tx, response_rx) = oneshot::channel();
        let _active_request = ActiveRequestGuard::install(
            &self.active_request,
            request_id.clone(),
            method_name,
            reject_startup_requests,
            response_tx,
        )?;
        self.write_json(&request).await?;
        let message = response_rx.await.map_err(|_| {
            io_error(format!(
                "app-server dropped `{method_name}` response waiter"
            ))
        })??;
        match message {
            JSONRPCMessage::Response(JSONRPCResponse { id, result }) if id == request_id => {
                let parsed = serde_json::from_value(result)
                    .with_context(|| format!("failed to decode `{method_name}` response payload"));
                parsed.map_err(|err| io_error(err.to_string()))
            }
            JSONRPCMessage::Error(JSONRPCError { id, error }) if id == request_id => {
                Err(io_error(format!(
                    "{method_name} failed: {} (code {})",
                    error.message, error.code
                )))
            }
            other => Err(io_error(format!(
                "unexpected app-server message while awaiting `{method_name}` response: {other:?}"
            ))),
        }
    }

    async fn send_client_notification(
        &mut self,
        notification: ClientNotification,
    ) -> Result<(), Error> {
        self.write_json(&notification).await
    }

    async fn send_server_request_response<T>(
        &mut self,
        request_id: RequestId,
        response: T,
    ) -> Result<(), Error>
    where
        T: Serialize,
    {
        let result = serde_json::to_value(response).map_err(|err| {
            io_error(format!(
                "failed to serialize server request response: {err}"
            ))
        })?;
        self.write_json(&JSONRPCMessage::Response(JSONRPCResponse {
            id: request_id,
            result,
        }))
        .await
    }

    async fn write_json<T>(&mut self, payload: &T) -> Result<(), Error>
    where
        T: Serialize,
    {
        let mut line = serde_json::to_string(payload)
            .map_err(|err| io_error(format!("failed to serialize JSON-RPC payload: {err}")))?;
        line.push('\n');
        write_line_to_stdin(&self.stdin, line).await
    }
}

async fn drive_app_server_stdout(
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

async fn write_line_to_stdin(stdin: &SharedAppStdin, line: String) -> Result<(), Error> {
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

#[cfg(test)]
mod tests {
    use super::{
        STARTUP_METADATA_REQUEST_TIMEOUT, STARTUP_REQUEST_TIMEOUT, configured_timeout,
        parse_timeout_override, request_timeout, should_reject_request_during_startup,
    };
    use std::time::Duration;

    #[test]
    fn applies_longer_timeout_to_critical_startup_requests() {
        assert_eq!(request_timeout("initialize"), Some(STARTUP_REQUEST_TIMEOUT));
        assert_eq!(
            request_timeout("thread/start"),
            Some(STARTUP_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("thread/resume"),
            Some(STARTUP_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("thread/list"),
            Some(STARTUP_REQUEST_TIMEOUT)
        );
        assert_eq!(request_timeout("turn/start"), Some(STARTUP_REQUEST_TIMEOUT));
    }

    #[test]
    fn applies_shorter_timeout_to_startup_metadata_requests() {
        assert_eq!(
            request_timeout("model/list"),
            Some(STARTUP_METADATA_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("account/rateLimits/read"),
            Some(STARTUP_METADATA_REQUEST_TIMEOUT)
        );
        assert_eq!(
            request_timeout("thread/read"),
            Some(STARTUP_METADATA_REQUEST_TIMEOUT)
        );
    }

    #[test]
    fn leaves_runtime_stream_requests_unbounded() {
        assert_eq!(request_timeout("turn/interrupt"), None);
    }

    #[test]
    fn configured_timeout_falls_back_for_missing_invalid_or_zero_values() {
        let fallback = Duration::from_secs(7);
        assert_eq!(configured_timeout("__MISSING__", fallback), fallback);
        assert_eq!(parse_timeout_override(Some("oops"), fallback), fallback);
        assert_eq!(parse_timeout_override(Some("0"), fallback), fallback);
        assert_eq!(
            parse_timeout_override(Some("1500"), fallback),
            Duration::from_millis(1500)
        );
    }

    #[test]
    fn rejects_only_known_unsupported_startup_requests() {
        assert!(should_reject_request_during_startup(
            "mcpServer/elicitation/request"
        ));
        assert!(should_reject_request_during_startup(
            "account/chatgptAuthTokens/refresh"
        ));
        assert!(should_reject_request_during_startup("applyPatchApproval"));
        assert!(should_reject_request_during_startup("execCommandApproval"));
        assert!(!should_reject_request_during_startup(
            "toolRequest/userInput"
        ));
        assert!(!should_reject_request_during_startup("dynamicToolCall"));
    }
}
