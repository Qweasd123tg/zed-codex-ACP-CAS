//! Thin wrapper around ACP client calls bound to one session id and a capability snapshot.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tracing::error;

use crate::thread::{
    ACP_CLIENT, ClientCapabilities, ContentChunk, Error, PermissionOption, ReadTextFileRequest,
    RequestPermissionOutcome, RequestPermissionRequest, SessionClient, SessionId,
    SessionNotification, SessionUpdate, ToolCall, ToolCallUpdate, WriteTextFileRequest,
};

impl SessionClient {
    // Capture the session id once so every outgoing event is correctly bound to that session.
    pub(super) fn new(
        session_id: SessionId,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
    ) -> Self {
        Self {
            session_id,
            client: ACP_CLIENT.get().expect("Client should be set").clone(),
            client_capabilities,
            suppress_text_output: env_flag("CODEX_ACP_DEV_LOGS_WITHOUT_TEXT_OUTPUT"),
        }
    }

    pub(super) fn supports_terminal_output(&self) -> bool {
        self.client_capabilities
            .lock()
            .unwrap()
            .meta
            .as_ref()
            .and_then(|meta| meta.get("terminal_output"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    pub(super) fn supports_write_text_file(&self) -> bool {
        self.client_capabilities.lock().unwrap().fs.write_text_file
    }

    pub(super) fn supports_read_text_file(&self) -> bool {
        self.client_capabilities.lock().unwrap().fs.read_text_file
    }

    pub(super) async fn send_notification(&self, update: SessionUpdate) {
        if let Err(err) = self
            .client
            .session_notification(SessionNotification::new(self.session_id.clone(), update))
            .await
        {
            error!("Failed to send session notification: {err:?}");
        }
    }

    pub(super) async fn send_agent_text(&self, text: impl Into<String>) {
        if self.suppress_text_output {
            return;
        }
        self.send_notification(SessionUpdate::AgentMessageChunk(ContentChunk::new(
            text.into().into(),
        )))
        .await;
    }

    pub(super) async fn send_user_text(&self, text: impl Into<String>) {
        if self.suppress_text_output {
            return;
        }
        self.send_notification(SessionUpdate::UserMessageChunk(ContentChunk::new(
            text.into().into(),
        )))
        .await;
    }

    pub(super) async fn send_agent_thought(&self, text: impl Into<String>) {
        if self.suppress_text_output {
            return;
        }
        self.send_notification(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
            text.into().into(),
        )))
        .await;
    }

    pub(super) async fn send_tool_call(&self, tool_call: ToolCall) {
        self.send_notification(SessionUpdate::ToolCall(tool_call))
            .await;
    }

    pub(super) async fn send_tool_call_update(&self, update: ToolCallUpdate) {
        self.send_notification(SessionUpdate::ToolCallUpdate(update))
            .await;
    }

    pub(super) async fn request_permission(
        &self,
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
    ) -> Result<RequestPermissionOutcome, Error> {
        let response = self
            .client
            .request_permission(RequestPermissionRequest::new(
                self.session_id.clone(),
                tool_call,
                options,
            ))
            .await?;
        Ok(response.outcome)
    }

    pub(super) async fn write_text_file(
        &self,
        path: PathBuf,
        content: String,
    ) -> Result<(), Error> {
        self.client
            .write_text_file(WriteTextFileRequest::new(
                self.session_id.clone(),
                path,
                content,
            ))
            .await?;
        Ok(())
    }

    pub(super) async fn prime_file_snapshot(&self, path: PathBuf) -> Result<(), Error> {
        self.client
            .read_text_file(ReadTextFileRequest::new(self.session_id.clone(), path))
            .await?;
        Ok(())
    }

    pub(super) async fn send_usage_update(&self, used: u64, size: u64) {
        self.send_notification(SessionUpdate::UsageUpdate(
            agent_client_protocol::UsageUpdate::new(used, size),
        ))
        .await;
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
}
