//! Тонкий асинхронный JSON-RPC-клиент вокруг `codex app-server --listen stdio://`.
//! Отвечает только за транспорт/мультиплексирование между логикой ACP-thread
//! и протоколом Codex app-server.

use std::collections::VecDeque;
use std::env;
use std::process::Stdio;
use std::time::Duration;

use agent_client_protocol::Error;
use anyhow::Context;
use codex_app_server_protocol::{
    ClientInfo, ClientNotification, ClientRequest, FileChangeRequestApprovalResponse,
    GetAccountRateLimitsResponse, InitializeCapabilities, InitializeParams, InitializeResponse,
    JSONRPCError, JSONRPCErrorError, JSONRPCMessage, JSONRPCResponse, ModelListParams,
    ModelListResponse, PermissionsRequestApprovalResponse, RequestId, ThreadArchiveParams,
    ThreadArchiveResponse, ThreadCompactStartParams, ThreadCompactStartResponse, ThreadListParams,
    ThreadListResponse, ThreadReadParams, ThreadReadResponse, ThreadResumeParams,
    ThreadResumeResponse, ThreadRollbackParams, ThreadRollbackResponse, ThreadSetNameParams,
    ThreadSetNameResponse, ThreadStartParams, ThreadStartResponse, ThreadUnarchiveParams,
    ThreadUnarchiveResponse, ToolRequestUserInputResponse, TurnInterruptParams,
    TurnInterruptResponse, TurnStartParams, TurnStartResponse,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tracing::{debug, info, warn};

const STARTUP_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const STARTUP_METADATA_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const STARTUP_REQUEST_TIMEOUT_ENV: &str = "CODEX_ACP_STARTUP_TIMEOUT_MS";
const STARTUP_METADATA_REQUEST_TIMEOUT_ENV: &str = "CODEX_ACP_STARTUP_METADATA_TIMEOUT_MS";
const JSONRPC_INVALID_REQUEST: i64 = -32600;

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
        "initialize" | "thread/start" | "thread/resume" | "thread/list" => Some(
            configured_timeout(STARTUP_REQUEST_TIMEOUT_ENV, STARTUP_REQUEST_TIMEOUT),
        ),
        "model/list" | "account/rateLimits/read" | "thread/read" => Some(configured_timeout(
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

pub struct AppServerProcess {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    // Пока ждём конкретный response id, app-server может прислать несвязанные
    // уведомления и server request. Мы ставим их в очередь и воспроизводим позже.
    pending_messages: VecDeque<JSONRPCMessage>,
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

        Ok(Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
            pending_messages: VecDeque::new(),
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

    pub async fn next_message(&mut self) -> Result<JSONRPCMessage, Error> {
        // Сначала выгружаем queued out-of-band сообщения, потом читаем stdout.
        if let Some(message) = self.pending_messages.pop_front() {
            return Ok(message);
        }
        self.read_message().await
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
                    queued_messages = self.pending_messages.len(),
                    "Timed out waiting for app-server startup response"
                );
                io_error(format!(
                    "timed out waiting for `{method_name}` response after {}s with {} queued out-of-band messages; codex app-server may be stuck during startup, auth, or early handshake",
                    timeout.as_secs(),
                    self.pending_messages.len()
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
        self.write_json(&request).await?;
        loop {
            let message = self.read_message().await?;
            match message {
                JSONRPCMessage::Response(JSONRPCResponse { id, result }) if id == request_id => {
                    let parsed = serde_json::from_value(result).with_context(|| {
                        format!("failed to decode `{method_name}` response payload")
                    });
                    return parsed.map_err(|err| io_error(err.to_string()));
                }
                JSONRPCMessage::Error(JSONRPCError { id, error }) if id == request_id => {
                    return Err(io_error(format!(
                        "{method_name} failed: {} (code {})",
                        error.message, error.code
                    )));
                }
                JSONRPCMessage::Request(request) => {
                    if reject_startup_requests
                        && self
                            .reject_unsupported_request_during_startup(&request, method_name)
                            .await?
                    {
                        continue;
                    } else {
                        warn!(
                            awaiting_method = method_name,
                            queued_request_method = %request.method,
                            request_id = ?request.id,
                            "Queued app-server request while waiting for a response"
                        );
                        // Сохраняем стабильный порядок протокола: цикл событий thread затем
                        // обработает эти сообщения как обычные события потока.
                        self.pending_messages
                            .push_back(JSONRPCMessage::Request(request));
                    }
                }
                other => {
                    // Сохраняем стабильный порядок протокола: цикл событий thread затем
                    // обработает эти сообщения как обычные события потока.
                    self.pending_messages.push_back(other);
                }
            }
        }
    }

    async fn reject_unsupported_request_during_startup(
        &mut self,
        request: &codex_app_server_protocol::JSONRPCRequest,
        awaited_method: &str,
    ) -> Result<bool, Error> {
        if !should_reject_request_during_startup(&request.method) {
            return Ok(false);
        }
        warn!(
            awaited_method,
            request_method = %request.method,
            request_id = ?request.id,
            "Rejecting unsupported app-server request during startup-sensitive handshake"
        );
        self.send_server_request_error(
            request.id.clone(),
            JSONRPC_INVALID_REQUEST,
            format!(
                "Cannot handle app-server request `{}` while awaiting `{awaited_method}` during startup",
                request.method
            ),
            None,
        )
        .await?;
        Ok(true)
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

    async fn read_message(&mut self) -> Result<JSONRPCMessage, Error> {
        loop {
            let mut line = String::new();
            let bytes = self
                .stdout
                .read_line(&mut line)
                .await
                .map_err(|err| io_error(format!("failed to read app-server output: {err}")))?;

            if bytes == 0 {
                return Err(io_error("codex app-server closed stdout"));
            }

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

    async fn write_json<T>(&mut self, payload: &T) -> Result<(), Error>
    where
        T: Serialize,
    {
        let mut line = serde_json::to_string(payload)
            .map_err(|err| io_error(format!("failed to serialize JSON-RPC payload: {err}")))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|err| io_error(format!("failed to write app-server input: {err}")))?;
        self.stdin
            .flush()
            .await
            .map_err(|err| io_error(format!("failed to flush app-server input: {err}")))?;
        Ok(())
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
        assert_eq!(request_timeout("turn/start"), None);
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
