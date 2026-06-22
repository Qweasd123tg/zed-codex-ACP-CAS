//! Тонкий асинхронный JSON-RPC-клиент вокруг `codex app-server --listen stdio://`.
//! Отвечает только за транспорт/мультиплексирование между логикой ACP-thread
//! и протоколом Codex app-server.

use std::process::Stdio;
use std::sync::{Arc, Mutex};

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
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
use tracing::{debug, info, warn};

#[path = "app_server/reader.rs"]
mod reader;
#[path = "app_server/request_policy.rs"]
mod request_policy;

use reader::{
    ActiveRequestGuard, ActiveRequestSlot, SharedAppStdin, drive_app_server_stdout,
    write_line_to_stdin,
};
pub(crate) use reader::{SharedAppMessageInbox, recv_message_from_inbox};
use request_policy::request_timeout;

// Нормализуем I/O-сбои дочернего процесса в ошибки уровня протокола.
fn io_error(message: impl Into<String>) -> Error {
    Error::internal_error().data(message.into())
}

fn format_timeout_duration(timeout: std::time::Duration) -> String {
    let millis = timeout.as_millis();
    if millis.is_multiple_of(1000) {
        format!("{}s", timeout.as_secs())
    } else {
        format!("{millis}ms")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDeleteParams {
    pub thread_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThreadDeleteResponse {}

#[derive(Debug, Serialize)]
struct RawClientRequest<T> {
    id: RequestId,
    method: &'static str,
    params: T,
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
                    "failed to start `{codex_bin}` app-server; ensure the `codex` binary is installed and available in PATH, or set CODEX_ACP_CODEX_BIN to an absolute backend path"
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

    pub async fn thread_delete(
        &mut self,
        params: ThreadDeleteParams,
    ) -> Result<ThreadDeleteResponse, Error> {
        let request_id = self.next_request_id();
        let request = RawClientRequest {
            id: request_id.clone(),
            method: "thread/delete",
            params,
        };
        self.raw_request(request, request_id, "thread/delete").await
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
        if let Some(timeout) = request_timeout(method_name).map_err(io_error)? {
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
                    "timed out waiting for `{method_name}` response after {}; codex app-server may be stuck during startup, auth, or early handshake",
                    format_timeout_duration(timeout)
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

    async fn raw_request<T, P>(
        &mut self,
        request: RawClientRequest<P>,
        request_id: RequestId,
        method_name: &str,
    ) -> Result<T, Error>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let (response_tx, response_rx) = oneshot::channel();
        let _active_request = ActiveRequestGuard::install(
            &self.active_request,
            request_id.clone(),
            method_name,
            false,
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
