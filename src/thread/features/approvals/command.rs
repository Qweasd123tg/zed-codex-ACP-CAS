//! Обработка подтверждений запуска shell-команд (command approval).

use agent_client_protocol::{
    Error, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    SelectedPermissionOutcome, ToolCallContent, ToolCallId, ToolCallLocation, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use codex_app_server_protocol::{
    AdditionalPermissionProfile, CommandExecutionApprovalDecision,
    CommandExecutionRequestApprovalParams, CommandExecutionRequestApprovalResponse,
};

use crate::thread::features::tool_call_ui::kind::{
    command_looks_like_verification, extract_inner_shell_command,
};
use crate::thread::{ALLOW_ONCE, CANCEL_TURN, REJECT_ONCE, SessionClient, ThreadInner};

pub(in crate::thread) struct CommandApprovalPending {
    pub(in crate::thread) client: SessionClient,
    pub(in crate::thread) request_id: codex_app_server_protocol::RequestId,
    pub(in crate::thread) tool_call: ToolCallUpdate,
    pub(in crate::thread) options: Vec<PermissionOption>,
}

// Отправляем решения по подтверждению команд обратно в app-server и зеркалим результат в ACP UI.
pub(in crate::thread) async fn handle_command_approval(
    inner: &mut ThreadInner,
    request_id: codex_app_server_protocol::RequestId,
    params: CommandExecutionRequestApprovalParams,
) -> Result<(), Error> {
    let pending = prepare_command_approval(inner, request_id, params);
    let outcome = pending
        .client
        .request_permission(pending.tool_call, pending.options)
        .await?;
    let decision = command_approval_decision_from_outcome(outcome);

    inner
        .app
        .send_command_approval_response(
            pending.request_id,
            CommandExecutionRequestApprovalResponse { decision },
        )
        .await
}

pub(in crate::thread) fn prepare_command_approval(
    inner: &ThreadInner,
    request_id: codex_app_server_protocol::RequestId,
    params: CommandExecutionRequestApprovalParams,
) -> Box<CommandApprovalPending> {
    let tool_call_id = ToolCallId::new(params.item_id.clone());

    let mut fields = ToolCallUpdateFields::new()
        .title("Details")
        .kind(ToolKind::Execute)
        .status(ToolCallStatus::Pending)
        .content(command_approval_content(&params));
    if let Some(cwd) = params.cwd.clone() {
        fields = fields.locations(vec![ToolCallLocation::new(cwd)]);
    }
    fields = fields.raw_input(serde_json::to_value(&params).ok());

    Box::new(CommandApprovalPending {
        client: inner.client.clone(),
        request_id,
        tool_call: ToolCallUpdate::new(tool_call_id, fields),
        options: vec![
            PermissionOption::new(ALLOW_ONCE, "Allow once", PermissionOptionKind::AllowOnce),
            PermissionOption::new(REJECT_ONCE, "Reject", PermissionOptionKind::RejectOnce),
            PermissionOption::new(CANCEL_TURN, "Cancel turn", PermissionOptionKind::RejectOnce),
        ],
    })
}

pub(in crate::thread) fn command_approval_decision_from_outcome(
    outcome: RequestPermissionOutcome,
) -> CommandExecutionApprovalDecision {
    match outcome {
        RequestPermissionOutcome::Cancelled => CommandExecutionApprovalDecision::Cancel,
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            match option_id.0.as_ref() {
                ALLOW_ONCE => CommandExecutionApprovalDecision::Accept,
                CANCEL_TURN => CommandExecutionApprovalDecision::Cancel,
                _ => CommandExecutionApprovalDecision::Decline,
            }
        }
        _ => CommandExecutionApprovalDecision::Decline,
    }
}

fn command_approval_content(
    params: &CommandExecutionRequestApprovalParams,
) -> Vec<ToolCallContent> {
    let body = command_approval_lines(params).join("\n");
    vec![body.into()]
}

fn command_approval_lines(params: &CommandExecutionRequestApprovalParams) -> Vec<String> {
    let mut lines = Vec::new();

    if let Some(reason) = params.reason.as_ref()
        && !reason.trim().is_empty()
    {
        lines.push(indented(reason.trim()));
    }

    if let Some(command) = params.command.as_ref()
        && !command.trim().is_empty()
    {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        let inner_command = extract_inner_shell_command(command);
        lines.push(indented("Command:"));
        lines.push(format!("```sh\n{inner_command}\n```"));

        if command_looks_like_verification(&inner_command) {
            lines.push(indented("This looks like a verification or test command."));
        }
    }

    if let Some(cwd) = params.cwd.as_ref() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(indented(&format!("Working directory: `{}`", cwd.display())));
    }

    if let Some(network) = params.network_approval_context.as_ref() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(indented(&format!(
            "Requested network access: {}://{}",
            format!("{:?}", network.protocol).to_ascii_lowercase(),
            network.host
        )));
    }

    if let Some(additional_permissions) = params.additional_permissions.as_ref() {
        lines.extend(additional_permission_lines(additional_permissions));
    }

    if let Some(skill_metadata) = params.skill_metadata.as_ref() {
        lines.push(format!(
            "Requested by skill: `{}`",
            skill_metadata.path_to_skills_md.display()
        ));
    }

    lines
}

fn additional_permission_lines(profile: &AdditionalPermissionProfile) -> Vec<String> {
    let mut lines = Vec::new();

    if let Some(network) = profile.network.as_ref()
        && let Some(enabled) = network.enabled
    {
        lines.push(indented(&format!(
            "Additional network permission requested: {enabled}"
        )));
    }

    if let Some(file_system) = profile.file_system.as_ref() {
        if let Some(read) = file_system.read.as_ref()
            && !read.is_empty()
        {
            lines.push(indented(&format!(
                "Additional file system read: {}",
                read.iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        if let Some(write) = file_system.write.as_ref()
            && !write.is_empty()
        {
            lines.push(indented(&format!(
                "Additional file system write: {}",
                write
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    }

    lines
}

fn indented(text: &str) -> String {
    format!("\u{2002}{text}")
}

#[cfg(test)]
mod tests {
    use super::command_approval_lines;
    use codex_app_server_protocol::CommandExecutionRequestApprovalParams;

    #[test]
    fn command_approval_lines_include_reason_command_and_cwd() {
        let params: CommandExecutionRequestApprovalParams =
            serde_json::from_value(serde_json::json!({
                "threadId": "thread_1",
                "turnId": "turn_1",
                "itemId": "item_1",
                "reason": "Need to inspect the workspace",
                "command": "/bin/bash -lc 'pwd && ls -la'",
                "cwd": "/tmp/workspace",
                "commandActions": [
                    {
                        "type": "listFiles",
                        "command": "ls -la",
                        "path": null
                    }
                ]
            }))
            .expect("valid command approval params");

        let lines = command_approval_lines(&params);
        let joined = lines.join("\n");
        assert!(joined.contains("Need to inspect the workspace"));
        assert!(joined.contains("```sh"));
        assert!(joined.contains("pwd && ls -la"));
        assert!(!joined.contains("/bin/bash -lc"));
        assert!(joined.contains("Working directory: `/tmp/workspace`"));
    }

    #[test]
    fn command_approval_lines_include_network_and_additional_permissions() {
        let params: CommandExecutionRequestApprovalParams =
            serde_json::from_value(serde_json::json!({
                "threadId": "thread_1",
                "turnId": "turn_1",
                "itemId": "item_1",
                "command": "curl -I https://example.com",
                "networkApprovalContext": {
                    "host": "example.com",
                    "protocol": "https"
                },
                "additionalPermissions": {
                    "network": { "enabled": true },
                    "fileSystem": {
                        "read": ["/tmp/read"],
                        "write": ["/tmp/write"]
                    }
                }
            }))
            .expect("valid command approval params");

        let lines = command_approval_lines(&params);
        let joined = lines.join("\n");
        assert!(joined.contains("Requested network access: https://example.com"));
        assert!(joined.contains("Additional network permission requested: true"));
        assert!(joined.contains("Additional file system read: /tmp/read"));
        assert!(joined.contains("Additional file system write: /tmp/write"));
    }
}
