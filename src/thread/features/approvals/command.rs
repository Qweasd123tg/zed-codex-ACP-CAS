//! Shell-command approval handling.

use agent_client_protocol::{
    Error, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    SelectedPermissionOutcome, ToolCallId, ToolCallLocation, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};
use codex_app_server_protocol::{
    CommandExecutionApprovalDecision, CommandExecutionRequestApprovalParams,
    CommandExecutionRequestApprovalResponse,
};

use crate::thread::features::tool_call_ui::title::command_tool_title;
use crate::thread::{ALLOW_ONCE, CANCEL_TURN, REJECT_ONCE, ThreadInner};

// Send command approval decisions back to app-server and mirror the result in ACP UI.
pub(in crate::thread) async fn handle_command_approval(
    inner: &mut ThreadInner,
    request_id: codex_app_server_protocol::RequestId,
    params: CommandExecutionRequestApprovalParams,
) -> Result<(), Error> {
    let command_actions = params.command_actions.clone().unwrap_or_default();
    let title = params
        .command
        .as_deref()
        .map(|command| command_tool_title(command, &command_actions))
        .unwrap_or_else(|| "Run command".to_string());
    let tool_call_id = ToolCallId::new(params.item_id.clone());

    let mut fields = ToolCallUpdateFields::new()
        .title(title)
        .kind(ToolKind::Execute)
        .status(ToolCallStatus::Pending);
    if let Some(cwd) = params.cwd.clone() {
        fields = fields.locations(vec![ToolCallLocation::new(cwd)]);
    }
    fields = fields.raw_input(serde_json::to_value(&params).ok());

    let outcome = inner
        .client
        .request_permission(
            ToolCallUpdate::new(tool_call_id.clone(), fields),
            vec![
                PermissionOption::new(ALLOW_ONCE, "Allow once", PermissionOptionKind::AllowOnce),
                PermissionOption::new(REJECT_ONCE, "Reject", PermissionOptionKind::RejectOnce),
                PermissionOption::new(CANCEL_TURN, "Cancel turn", PermissionOptionKind::RejectOnce),
            ],
        )
        .await?;

    let decision = match outcome {
        RequestPermissionOutcome::Cancelled => CommandExecutionApprovalDecision::Cancel,
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            match option_id.0.as_ref() {
                ALLOW_ONCE => CommandExecutionApprovalDecision::Accept,
                CANCEL_TURN => CommandExecutionApprovalDecision::Cancel,
                _ => CommandExecutionApprovalDecision::Decline,
            }
        }
        _ => CommandExecutionApprovalDecision::Decline,
    };

    inner
        .app
        .send_command_approval_response(
            request_id,
            CommandExecutionRequestApprovalResponse { decision },
        )
        .await
}
