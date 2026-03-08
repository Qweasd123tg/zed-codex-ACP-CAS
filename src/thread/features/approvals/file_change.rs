//! File-change approval handling for patches and edits.

use agent_client_protocol::{
    Error, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    SelectedPermissionOutcome, ToolCallContent, ToolCallId, ToolCallLocation, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use codex_app_server_protocol::{
    FileChangeApprovalDecision, FileChangeRequestApprovalParams, FileChangeRequestApprovalResponse,
};

use crate::thread::features::file::changes::should_prompt_file_change_approval;
use crate::thread::{
    ALLOW_ONCE, REJECT_ONCE, ThreadInner, file_change_to_preview_diff, file_change_tool_location,
};

pub(in crate::thread) async fn handle_file_change_approval(
    inner: &mut ThreadInner,
    request_id: codex_app_server_protocol::RequestId,
    params: FileChangeRequestApprovalParams,
) -> Result<(), Error> {
    if !should_prompt_file_change_approval(inner.collaboration_mode_kind, inner.edit_approval_mode)
    {
        return inner
            .app
            .send_file_change_approval_response(
                request_id,
                FileChangeRequestApprovalResponse {
                    decision: FileChangeApprovalDecision::Accept,
                },
            )
            .await;
    }

    let tool_call_id = ToolCallId::new(params.item_id.clone());
    let started_changes = inner
        .file_change_started_changes
        .get(&params.item_id)
        .cloned()
        .unwrap_or_default();
    let before_contents = inner
        .file_change_before_contents
        .get(&params.item_id)
        .cloned()
        .unwrap_or_default();
    let locations = inner
        .file_change_locations
        .get(&params.item_id)
        .cloned()
        .unwrap_or_default();
    let title = match locations.len() {
        0 => "Apply file changes".to_string(),
        1 => format!("Apply changes to {}", locations[0].display()),
        count => format!("Apply changes to {count} files"),
    };
    let mut details = Vec::new();
    if let Some(reason) = params.reason.clone()
        && !reason.trim().is_empty()
    {
        details.push(format!("Reason: {reason}"));
    }
    if let Some(root) = params.grant_root.clone() {
        details.push(format!("Requested write access root: {}", root.display()));
    }
    if !locations.is_empty() {
        let file_lines = locations
            .iter()
            .take(12)
            .map(|path| format!("- {}", path.display()))
            .collect::<Vec<_>>();
        details.push(format!("Proposed file changes:\n{}", file_lines.join("\n")));
    }
    let mut content = started_changes
        .iter()
        .map(|change| {
            ToolCallContent::Diff(file_change_to_preview_diff(
                &inner.workspace_cwd,
                &before_contents,
                change,
            ))
        })
        .collect::<Vec<_>>();
    let tool_locations = if started_changes.is_empty() {
        locations
            .iter()
            .cloned()
            .map(ToolCallLocation::new)
            .collect::<Vec<_>>()
    } else {
        started_changes
            .iter()
            .map(|change| file_change_tool_location(&inner.workspace_cwd, change))
            .collect::<Vec<_>>()
    };
    content.extend(details.into_iter().map(Into::into));

    let outcome = inner
        .client
        .request_permission(
            ToolCallUpdate::new(
                tool_call_id,
                ToolCallUpdateFields::new()
                    .title(title)
                    .kind(ToolKind::Edit)
                    .status(ToolCallStatus::Pending)
                    .locations(tool_locations)
                    .content(content)
                    .raw_input(serde_json::to_value(&params).ok()),
            ),
            vec![
                PermissionOption::new(ALLOW_ONCE, "Yes", PermissionOptionKind::AllowOnce),
                PermissionOption::new(REJECT_ONCE, "No", PermissionOptionKind::RejectOnce),
            ],
        )
        .await?;

    let decision = match outcome {
        RequestPermissionOutcome::Cancelled => FileChangeApprovalDecision::Decline,
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            match option_id.0.as_ref() {
                ALLOW_ONCE => FileChangeApprovalDecision::Accept,
                _ => FileChangeApprovalDecision::Decline,
            }
        }
        _ => FileChangeApprovalDecision::Decline,
    };

    inner
        .app
        .send_file_change_approval_response(
            request_id,
            FileChangeRequestApprovalResponse { decision },
        )
        .await
}
