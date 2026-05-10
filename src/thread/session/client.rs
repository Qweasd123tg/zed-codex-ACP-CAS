//! Тонкая обёртка вокруг вызовов ACP-клиента, ограниченная одним session id и снимком capability.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tracing::error;

use crate::thread::{
    Client, ClientCapabilities, ConnectionTo, ContentChunk, Error, PermissionOption,
    ReadTextFileRequest, RequestPermissionOutcome, RequestPermissionRequest, SessionClient,
    SessionId, SessionNotification, SessionUpdate, ToolCall, ToolCallUpdate, UsageUpdate,
    WriteTextFileRequest,
};

impl SessionClient {
    // Фиксируем session id один раз, чтобы каждое исходящее событие было корректно привязано к сессии.
    pub(super) fn new(
        session_id: SessionId,
        client: ConnectionTo<Client>,
        client_capabilities: Arc<RwLock<ClientCapabilities>>,
    ) -> Self {
        Self {
            session_id,
            client,
            client_capabilities,
            suppress_text_output: env_flag("CODEX_ACP_DEV_LOGS_WITHOUT_TEXT_OUTPUT"),
        }
    }

    pub(super) fn supports_terminal_output(&self) -> bool {
        read_client_capabilities(&self.client_capabilities, |caps| {
            caps.meta
                .as_ref()
                .and_then(|meta| meta.get("terminal_output"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        })
    }

    pub(super) fn supports_buffer_writeback(&self) -> bool {
        !env_flag("CODEX_ACP_DISABLE_SYNC_EDIT_BUFFERS")
            && read_client_capabilities(&self.client_capabilities, |caps| caps.fs.write_text_file)
    }

    pub(super) fn supports_read_text_file(&self) -> bool {
        read_client_capabilities(&self.client_capabilities, |caps| caps.fs.read_text_file)
    }

    pub(super) async fn send_notification(&self, update: SessionUpdate) {
        if let Err(err) = self
            .client
            .send_notification(SessionNotification::new(self.session_id.clone(), update))
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

    pub(super) async fn send_system_message(
        &self,
        label: &str,
        title: impl AsRef<str>,
        body: impl AsRef<str>,
    ) {
        self.send_agent_text(format_system_message(label, title.as_ref(), body.as_ref()))
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
            .send_request(RequestPermissionRequest::new(
                self.session_id.clone(),
                tool_call,
                options,
            ))
            .block_task()
            .await?;
        Ok(response.outcome)
    }

    pub(super) async fn write_text_file(
        &self,
        path: PathBuf,
        content: String,
    ) -> Result<(), Error> {
        self.client
            .send_request(WriteTextFileRequest::new(
                self.session_id.clone(),
                path,
                content,
            ))
            .block_task()
            .await?;
        Ok(())
    }

    pub(super) async fn prime_file_snapshot(&self, path: PathBuf) -> Result<(), Error> {
        self.client
            .send_request(ReadTextFileRequest::new(self.session_id.clone(), path))
            .block_task()
            .await?;
        Ok(())
    }

    pub(super) async fn send_usage_update(&self, used: u64, size: u64) {
        self.send_notification(SessionUpdate::UsageUpdate(UsageUpdate::new(used, size)))
            .await;
    }
}

// RwLock-читатель, устойчивый к poisoning: если writer panic-нул, мы всё равно отдаём
// последний успешно записанный snapshot capabilities.
fn read_client_capabilities<R>(
    capabilities: &RwLock<ClientCapabilities>,
    reader: impl FnOnce(&ClientCapabilities) -> R,
) -> R {
    match capabilities.read() {
        Ok(guard) => reader(&guard),
        Err(poison) => reader(&poison.into_inner()),
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
}

pub(in crate::thread) fn format_system_message(label: &str, title: &str, body: &str) -> String {
    let label = label.trim();
    let title = title.trim();
    let body = body.trim();
    let heading = if title.is_empty() {
        "System".to_string()
    } else if label.is_empty() {
        format!("System: {title}")
    } else {
        format!("System / {label}: {title}")
    };

    let mut message = format!("\n\n> **{heading}**");
    if !body.is_empty() {
        message.push_str("\n>\n");
        for line in body.lines() {
            if line.trim().is_empty() {
                message.push_str(">\n");
            } else {
                message.push_str("> ");
                message.push_str(line);
                message.push('\n');
            }
        }
        if message.ends_with('\n') {
            message.pop();
        }
    }
    message.push_str("\n\n");
    message
}

#[cfg(test)]
mod tests {
    use super::format_system_message;

    #[test]
    fn system_messages_are_visually_separated() {
        assert_eq!(
            format_system_message("warning", "Config warning", "Unknown field"),
            "\n\n> **System / warning: Config warning**\n>\n> Unknown field\n\n"
        );
    }

    #[test]
    fn system_message_formatter_trims_empty_body() {
        assert_eq!(
            format_system_message("status", "Reconnecting", "  "),
            "\n\n> **System / status: Reconnecting**\n\n"
        );
    }

    #[test]
    fn system_message_formatter_quotes_multiline_body() {
        assert_eq!(
            format_system_message(
                "status",
                "Account limits",
                "5-hour: resets in 4h\nWeekly: resets in 1d"
            ),
            "\n\n> **System / status: Account limits**\n>\n> 5-hour: resets in 4h\n> Weekly: resets in 1d\n\n"
        );
    }
}
