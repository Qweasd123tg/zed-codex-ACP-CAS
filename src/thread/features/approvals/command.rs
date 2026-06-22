//! Обработка подтверждений запуска shell-команд (command approval).

use std::{collections::HashMap, path::Path};

use agent_client_protocol::{
    Error,
    schema::v1::{
        PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
        SelectedPermissionOutcome, Terminal, ToolCall, ToolCallContent, ToolCallId, ToolCallStatus,
        ToolCallUpdate, ToolCallUpdateFields, ToolKind,
    },
};
use codex_app_server_protocol::{
    AdditionalPermissionProfile, CommandExecutionApprovalDecision,
    CommandExecutionRequestApprovalParams, CommandExecutionRequestApprovalResponse,
    NetworkPolicyRuleAction,
};

use crate::thread::features::tool_call_ui::kind::{
    command_looks_like_verification, command_tool_kind, extract_inner_shell_command,
};
use crate::thread::features::tool_call_ui::location::command_tool_locations;
use crate::thread::features::tool_call_ui::title::command_tool_title;
use crate::thread::features::{tool_call_ui::content, tool_events};
use crate::thread::{ALLOW_ONCE, CANCEL_TURN, REJECT_ONCE, SessionClient, ThreadInner};

pub(in crate::thread) struct CommandApprovalPending {
    pub(in crate::thread) client: SessionClient,
    pub(in crate::thread) request_id: codex_app_server_protocol::RequestId,
    pub(in crate::thread) preflight_tool_call: Option<ToolCall>,
    pub(in crate::thread) tool_call: ToolCallUpdate,
    pub(in crate::thread) options: Vec<PermissionOption>,
    pub(in crate::thread) decisions_by_option_id: HashMap<String, CommandExecutionApprovalDecision>,
}

impl CommandApprovalPending {
    pub(in crate::thread) async fn request_permission(
        &mut self,
    ) -> Result<RequestPermissionOutcome, Error> {
        if let Some(tool_call) = self.preflight_tool_call.take() {
            self.client.send_tool_call(tool_call).await;
        }
        self.client
            .request_permission(self.tool_call.clone(), self.options.clone())
            .await
    }
}

// Отправляем решения по подтверждению команд обратно в app-server и зеркалим результат в ACP UI.
pub(in crate::thread) async fn handle_command_approval(
    inner: &mut ThreadInner,
    request_id: codex_app_server_protocol::RequestId,
    params: CommandExecutionRequestApprovalParams,
) -> Result<(), Error> {
    let mut pending = prepare_command_approval(inner, request_id, params);
    let outcome = pending.request_permission().await?;
    let decision = command_approval_decision_from_outcome(outcome, &pending.decisions_by_option_id);

    inner
        .app
        .lock()
        .await
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

    let command = params.command.as_deref().unwrap_or_default();
    let command_actions = params.command_actions.as_deref().unwrap_or_default();
    let use_terminal_preflight = should_use_terminal_approval_preflight(
        inner.client.supports_terminal_output(),
        &params,
        command,
        command_actions,
    );

    let fields = command_approval_fields(
        &params,
        &inner.workspace_cwd,
        command,
        command_actions,
        use_terminal_preflight,
    );
    let preflight_tool_call = use_terminal_preflight.then(|| {
        command_approval_preflight_tool_call(
            &params,
            &inner.workspace_cwd,
            command,
            command_actions,
        )
    });

    let (options, decisions_by_option_id) = command_approval_options(&params);

    Box::new(CommandApprovalPending {
        client: inner.client.clone(),
        request_id,
        preflight_tool_call,
        tool_call: ToolCallUpdate::new(tool_call_id, fields),
        options,
        decisions_by_option_id,
    })
}

fn command_approval_title(
    params: &CommandExecutionRequestApprovalParams,
    command: &str,
    command_actions: &[codex_app_server_protocol::CommandAction],
) -> String {
    if let Some(network) = params.network_approval_context.as_ref() {
        return format!("Network access: {}", network.host);
    }

    if !command.trim().is_empty() || !command_actions.is_empty() {
        return command_tool_title(command, command_actions);
    }
    params
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .map(|reason| {
            let preview = reason.chars().take(80).collect::<String>();
            if preview.len() < reason.len() {
                format!("{preview}...")
            } else {
                preview
            }
        })
        .unwrap_or_else(|| "Command approval".to_string())
}

fn command_approval_fields(
    params: &CommandExecutionRequestApprovalParams,
    workspace_cwd: &Path,
    command: &str,
    command_actions: &[codex_app_server_protocol::CommandAction],
    use_terminal_preflight: bool,
) -> ToolCallUpdateFields {
    let cwd = params.cwd.as_deref().unwrap_or(workspace_cwd);
    let title = if use_terminal_preflight {
        content::command_tool_label(command, cwd, command_actions)
    } else {
        command_approval_title(params, command, command_actions)
    };
    let mut fields = ToolCallUpdateFields::new()
        .title(title)
        .kind(command_approval_kind(
            command,
            command_actions,
            use_terminal_preflight,
        ))
        .status(ToolCallStatus::Pending);

    if !use_terminal_preflight {
        fields = fields.content(command_approval_content(params));
    }
    if params.cwd.is_some() || !command.trim().is_empty() || !command_actions.is_empty() {
        fields = fields.locations(command_tool_locations(cwd, command, command_actions));
    }
    fields.raw_input(serde_json::to_value(params).ok())
}

fn command_approval_kind(
    command: &str,
    command_actions: &[codex_app_server_protocol::CommandAction],
    use_terminal_preflight: bool,
) -> ToolKind {
    if use_terminal_preflight {
        return ToolKind::Execute;
    }

    match command_tool_kind(command, command_actions) {
        ToolKind::Think => ToolKind::Execute,
        kind => kind,
    }
}

fn should_use_terminal_approval_preflight(
    supports_terminal_output: bool,
    params: &CommandExecutionRequestApprovalParams,
    command: &str,
    command_actions: &[codex_app_server_protocol::CommandAction],
) -> bool {
    supports_terminal_output
        && !command.trim().is_empty()
        && tool_events::command::command_uses_native_terminal(command, command_actions)
        && params.network_approval_context.is_none()
        && params.additional_permissions.is_none()
        && params.skill_metadata.is_none()
}

fn command_approval_preflight_tool_call(
    params: &CommandExecutionRequestApprovalParams,
    workspace_cwd: &Path,
    command: &str,
    command_actions: &[codex_app_server_protocol::CommandAction],
) -> ToolCall {
    let cwd = params.cwd.as_deref().unwrap_or(workspace_cwd);
    ToolCall::new(
        ToolCallId::new(params.item_id.clone()),
        content::command_tool_label(command, cwd, command_actions),
    )
    .kind(ToolKind::Execute)
    .status(ToolCallStatus::Pending)
    .locations(command_tool_locations(cwd, command, command_actions))
    .content(vec![ToolCallContent::Terminal(Terminal::new(
        params.item_id.clone(),
    ))])
    .raw_input(serde_json::to_value(params).ok())
    .meta(tool_events::command::terminal_info_meta(
        &params.item_id,
        cwd,
    ))
}

pub(in crate::thread) fn command_approval_decision_from_outcome(
    outcome: RequestPermissionOutcome,
    decisions_by_option_id: &HashMap<String, CommandExecutionApprovalDecision>,
) -> CommandExecutionApprovalDecision {
    match outcome {
        RequestPermissionOutcome::Cancelled => CommandExecutionApprovalDecision::Cancel,
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            if let Some(decision) = decisions_by_option_id.get(option_id.0.as_ref()) {
                return decision.clone();
            }
            match option_id.0.as_ref() {
                ALLOW_ONCE => CommandExecutionApprovalDecision::Accept,
                CANCEL_TURN => CommandExecutionApprovalDecision::Cancel,
                _ => CommandExecutionApprovalDecision::Decline,
            }
        }
        _ => CommandExecutionApprovalDecision::Decline,
    }
}

fn command_approval_options(
    params: &CommandExecutionRequestApprovalParams,
) -> (
    Vec<PermissionOption>,
    HashMap<String, CommandExecutionApprovalDecision>,
) {
    let decisions = command_approval_decisions(params);
    let mut options = Vec::new();
    let mut decisions_by_option_id = HashMap::new();

    for (index, decision) in decisions.into_iter().enumerate() {
        let (id, label, kind) =
            command_approval_option_presentation(index, &decision, &decisions_by_option_id);
        options.push(PermissionOption::new(id.clone(), label, kind));
        decisions_by_option_id.insert(id, decision);
    }

    (options, decisions_by_option_id)
}

fn command_approval_decisions(
    params: &CommandExecutionRequestApprovalParams,
) -> Vec<CommandExecutionApprovalDecision> {
    if let Some(decisions) = params
        .available_decisions
        .as_ref()
        .filter(|decisions| !decisions.is_empty())
    {
        return decisions.clone();
    }

    let mut decisions = vec![CommandExecutionApprovalDecision::Accept];
    if let Some(execpolicy_amendment) = params.proposed_execpolicy_amendment.clone() {
        decisions.push(
            CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment {
                execpolicy_amendment,
            },
        );
    } else {
        decisions.push(CommandExecutionApprovalDecision::AcceptForSession);
    }
    decisions.push(CommandExecutionApprovalDecision::Decline);
    decisions
}

fn command_approval_option_presentation(
    index: usize,
    decision: &CommandExecutionApprovalDecision,
    existing: &HashMap<String, CommandExecutionApprovalDecision>,
) -> (String, &'static str, PermissionOptionKind) {
    match decision {
        CommandExecutionApprovalDecision::Accept => (
            unique_option_id(ALLOW_ONCE, index, existing),
            "Allow once",
            PermissionOptionKind::AllowOnce,
        ),
        CommandExecutionApprovalDecision::AcceptForSession => (
            unique_option_id("allow-for-session", index, existing),
            "Allow for session",
            PermissionOptionKind::AllowAlways,
        ),
        CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment { .. } => (
            unique_option_id("allow-matching-commands", index, existing),
            "Allow matching commands",
            PermissionOptionKind::AllowAlways,
        ),
        CommandExecutionApprovalDecision::ApplyNetworkPolicyAmendment {
            network_policy_amendment,
        } => match network_policy_amendment.action {
            NetworkPolicyRuleAction::Allow => (
                unique_option_id("allow-network-host", index, existing),
                "Allow this host",
                PermissionOptionKind::AllowAlways,
            ),
            NetworkPolicyRuleAction::Deny => (
                unique_option_id("deny-network-host", index, existing),
                "Deny this host",
                PermissionOptionKind::RejectAlways,
            ),
        },
        CommandExecutionApprovalDecision::Decline => (
            unique_option_id(REJECT_ONCE, index, existing),
            "Reject",
            PermissionOptionKind::RejectOnce,
        ),
        CommandExecutionApprovalDecision::Cancel => (
            unique_option_id(CANCEL_TURN, index, existing),
            "Cancel turn",
            PermissionOptionKind::RejectOnce,
        ),
    }
}

fn unique_option_id(
    base: &str,
    index: usize,
    existing: &HashMap<String, CommandExecutionApprovalDecision>,
) -> String {
    if existing.contains_key(base) {
        format!("{base}-{index}")
    } else {
        base.to_string()
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
    use std::path::Path;

    use super::{
        command_approval_decision_from_outcome, command_approval_fields, command_approval_lines,
        command_approval_options, command_approval_preflight_tool_call, command_approval_title,
        should_use_terminal_approval_preflight,
    };
    use agent_client_protocol::schema::v1::{
        PermissionOptionKind, RequestPermissionOutcome, SelectedPermissionOutcome, ToolCallContent,
        ToolKind,
    };
    use codex_app_server_protocol::{
        CommandExecutionApprovalDecision, CommandExecutionRequestApprovalParams,
        ExecPolicyAmendment,
    };

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
    fn command_approval_title_uses_command_actions_instead_of_generic_details() {
        let params: CommandExecutionRequestApprovalParams =
            serde_json::from_value(serde_json::json!({
                "threadId": "thread_1",
                "turnId": "turn_1",
                "itemId": "item_1",
                "reason": "Need to inspect the workspace",
                "command": "/bin/bash -lc 'git status --short'",
                "cwd": "/tmp/workspace",
                "commandActions": [
                    {
                        "type": "unknown",
                        "command": "git status --short"
                    }
                ]
            }))
            .expect("valid command approval params");

        assert_eq!(
            command_approval_title(
                &params,
                params.command.as_deref().unwrap_or_default(),
                params.command_actions.as_deref().unwrap_or_default(),
            ),
            "Inspect git state"
        );
    }

    #[test]
    fn command_approval_title_prefers_network_context() {
        let params: CommandExecutionRequestApprovalParams =
            serde_json::from_value(serde_json::json!({
                "threadId": "thread_1",
                "turnId": "turn_1",
                "itemId": "item_1",
                "command": "curl -I https://example.com",
                "networkApprovalContext": {
                    "host": "example.com",
                    "protocol": "https"
                }
            }))
            .expect("valid command approval params");

        assert_eq!(
            command_approval_title(
                &params,
                params.command.as_deref().unwrap_or_default(),
                params.command_actions.as_deref().unwrap_or_default(),
            ),
            "Network access: example.com"
        );
    }

    #[test]
    fn shell_command_approval_uses_terminal_preflight_when_supported() {
        let params: CommandExecutionRequestApprovalParams =
            serde_json::from_value(serde_json::json!({
                "threadId": "thread_1",
                "turnId": "turn_1",
                "itemId": "call_1",
                "reason": "Need to run a shell command",
                "command": "/bin/bash -lc 'echo hi && date'",
                "cwd": "/tmp/workspace",
                "commandActions": [
                    {
                        "type": "unknown",
                        "command": "echo hi && date"
                    }
                ]
            }))
            .expect("valid command approval params");
        let command = params.command.as_deref().unwrap_or_default();
        let actions = params.command_actions.as_deref().unwrap_or_default();

        assert!(should_use_terminal_approval_preflight(
            true, &params, command, actions
        ));

        let preflight =
            command_approval_preflight_tool_call(&params, Path::new("/repo"), command, actions);

        assert_eq!(preflight.tool_call_id.0.as_ref(), "call_1");
        assert_eq!(preflight.title, "echo hi && date");
        assert!(matches!(
            preflight.content.as_slice(),
            [ToolCallContent::Terminal(terminal)] if terminal.terminal_id.0.as_ref() == "call_1"
        ));
        let terminal_info = preflight
            .meta
            .as_ref()
            .and_then(|meta| meta.get("terminal_info"))
            .expect("terminal_info meta");
        assert_eq!(
            terminal_info.get("terminal_id").and_then(|id| id.as_str()),
            Some("call_1")
        );
        assert_eq!(
            terminal_info.get("cwd").and_then(|cwd| cwd.as_str()),
            Some("/tmp/workspace")
        );
    }

    #[test]
    fn terminal_preflight_request_fields_do_not_replace_terminal_content() {
        let params: CommandExecutionRequestApprovalParams =
            serde_json::from_value(serde_json::json!({
                "threadId": "thread_1",
                "turnId": "turn_1",
                "itemId": "call_1",
                "reason": "Need to run a shell command",
                "command": "/bin/bash -lc 'cargo test'",
                "cwd": "/tmp/workspace",
                "commandActions": [
                    {
                        "type": "unknown",
                        "command": "cargo test"
                    }
                ]
            }))
            .expect("valid command approval params");
        let command = params.command.as_deref().unwrap_or_default();
        let actions = params.command_actions.as_deref().unwrap_or_default();

        let fields = command_approval_fields(&params, Path::new("/repo"), command, actions, true);

        assert!(fields.content.is_none());
        assert_eq!(fields.title.as_deref(), Some("cargo test"));
        assert!(fields.locations.as_ref().is_some_and(|locations| {
            locations.len() == 1 && locations[0].path == Path::new("/tmp/workspace")
        }));
        assert!(fields.raw_input.is_some());
    }

    #[test]
    fn special_permission_command_approval_keeps_detailed_body() {
        let params: CommandExecutionRequestApprovalParams =
            serde_json::from_value(serde_json::json!({
                "threadId": "thread_1",
                "turnId": "turn_1",
                "itemId": "call_1",
                "command": "curl -I https://example.com",
                "networkApprovalContext": {
                    "host": "example.com",
                    "protocol": "https"
                }
            }))
            .expect("valid command approval params");
        let command = params.command.as_deref().unwrap_or_default();
        let actions = params.command_actions.as_deref().unwrap_or_default();

        assert!(!should_use_terminal_approval_preflight(
            true, &params, command, actions
        ));

        let fields = command_approval_fields(&params, Path::new("/repo"), command, actions, false);

        assert!(fields.content.is_some());
        assert_eq!(fields.kind, Some(ToolKind::Fetch));
    }

    #[test]
    fn delete_command_approval_uses_delete_kind_without_terminal_preflight() {
        let params: CommandExecutionRequestApprovalParams =
            serde_json::from_value(serde_json::json!({
                "threadId": "thread_1",
                "turnId": "turn_1",
                "itemId": "call_1",
                "reason": "Need to remove a generated file",
                "command": "rm -f /tmp/generated.md",
                "cwd": "/tmp/workspace"
            }))
            .expect("valid command approval params");
        let command = params.command.as_deref().unwrap_or_default();
        let actions = params.command_actions.as_deref().unwrap_or_default();

        assert!(!should_use_terminal_approval_preflight(
            true, &params, command, actions
        ));

        let fields = command_approval_fields(&params, Path::new("/repo"), command, actions, false);

        assert_eq!(fields.kind, Some(ToolKind::Delete));
        assert!(fields.content.is_some());
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

    #[test]
    fn command_approval_options_follow_available_decisions() {
        let mut params = base_command_approval_params();
        let persistent = CommandExecutionApprovalDecision::AcceptWithExecpolicyAmendment {
            execpolicy_amendment: ExecPolicyAmendment {
                command: vec!["cargo".to_string(), "test".to_string()],
            },
        };
        params.available_decisions = Some(vec![
            CommandExecutionApprovalDecision::Accept,
            persistent.clone(),
            CommandExecutionApprovalDecision::Decline,
        ]);

        let (options, decisions_by_option_id) = command_approval_options(&params);

        assert_eq!(options.len(), 3);
        assert_eq!(options[0].kind, PermissionOptionKind::AllowOnce);
        assert_eq!(options[1].kind, PermissionOptionKind::AllowAlways);
        assert_eq!(options[2].kind, PermissionOptionKind::RejectOnce);
        assert!(
            !options
                .iter()
                .any(|option| option.name.as_str() == "Cancel turn")
        );

        let selected = RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            options[1].option_id.clone(),
        ));
        assert_eq!(
            command_approval_decision_from_outcome(selected, &decisions_by_option_id),
            persistent
        );
    }

    #[test]
    fn command_approval_options_fallback_does_not_invent_cancel() {
        let params = base_command_approval_params();

        let (options, _) = command_approval_options(&params);

        assert_eq!(options.len(), 3);
        assert_eq!(options[0].kind, PermissionOptionKind::AllowOnce);
        assert_eq!(options[1].kind, PermissionOptionKind::AllowAlways);
        assert_eq!(options[2].kind, PermissionOptionKind::RejectOnce);
        assert_eq!(options[1].name.as_str(), "Allow for session");
        assert!(
            !options
                .iter()
                .any(|option| option.name.as_str() == "Cancel turn")
        );
    }

    fn base_command_approval_params() -> CommandExecutionRequestApprovalParams {
        serde_json::from_value(serde_json::json!({
            "threadId": "thread_1",
            "turnId": "turn_1",
            "itemId": "item_1",
            "command": "cargo test",
            "cwd": "/tmp/workspace"
        }))
        .expect("valid command approval params")
    }
}
