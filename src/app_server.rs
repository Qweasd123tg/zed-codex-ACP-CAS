//! Thin async JSON-RPC client around `codex app-server --listen stdio://`.
//! It handles only transport and multiplexing between ACP thread logic
//! and the Codex app-server protocol.

use std::collections::VecDeque;
use std::process::Stdio;

use agent_client_protocol::Error;
use anyhow::Context;
use codex_app_server_protocol::{
    ClientInfo, ClientNotification, ClientRequest, FileChangeRequestApprovalResponse,
    InitializeCapabilities, InitializeParams, InitializeResponse, JSONRPCError, JSONRPCErrorError,
    JSONRPCMessage, JSONRPCResponse, ModelListParams, ModelListResponse, RequestId,
    ThreadCompactStartParams, ThreadCompactStartResponse, ThreadListParams, ThreadListResponse,
    ThreadResumeParams, ThreadResumeResponse, ThreadRollbackParams, ThreadRollbackResponse,
    ThreadStartParams, ThreadStartResponse, ToolRequestUserInputResponse, TurnInterruptParams,
    TurnInterruptResponse, TurnStartParams, TurnStartResponse,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tracing::warn;

// Normalize child-process I/O failures into protocol-level errors.
fn io_error(message: impl Into<String>) -> Error {
    Error::internal_error().data(message.into())
}

pub struct AppServerProcess {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    // While waiting for a specific response id, app-server may emit unrelated
    // notifications and server requests. Buffer them and replay them later.
    pending_messages: VecDeque<JSONRPCMessage>,
    next_request_id: i64,
}

impl AppServerProcess {
    pub async fn spawn(codex_bin: &str) -> Result<Self, Error> {
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
            .with_context(|| format!("failed to start `{codex_bin}` app-server"))
            .map_err(|err| io_error(err.to_string()))?;

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
        // Drain queued out-of-band messages first, then read from stdout.
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
                other => {
                    // Preserve stable protocol ordering: the thread event loop will
                    // process these messages later as ordinary stream events.
                    self.pending_messages.push_back(other);
                }
            }
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
