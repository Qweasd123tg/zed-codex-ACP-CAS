//! Обработка request permissions: app-server просит временно выдать доп. права.

use agent_client_protocol::{
    Error, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    SelectedPermissionOutcome, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
    ToolKind,
};
use codex_app_server_protocol::{
    AdditionalMacOsPermissions, AdditionalPermissionProfile, GrantedMacOsPermissions,
    GrantedPermissionProfile, PermissionGrantScope, PermissionsRequestApprovalParams,
    PermissionsRequestApprovalResponse, RequestId,
};

use crate::thread::ThreadInner;

const APPROVE_FOR_SESSION: &str = "approved-for-session";
const APPROVE_FOR_TURN: &str = "approved";
const DENY_REQUEST: &str = "abort";

pub(in crate::thread) async fn handle_permissions_request_approval(
    inner: &mut ThreadInner,
    request_id: RequestId,
    params: PermissionsRequestApprovalParams,
) -> Result<(), Error> {
    let tool_call_id = ToolCallId::new(params.item_id.clone());
    let raw_input = serde_json::to_value(&params).ok();
    let title = params
        .reason
        .clone()
        .unwrap_or_else(|| "Permissions Request".to_string());
    let content_lines = permission_request_content(&params);
    let content = if content_lines.is_empty() {
        Vec::new()
    } else {
        vec![content_lines.join("\n").into()]
    };

    let outcome = inner
        .client
        .request_permission(
            ToolCallUpdate::new(
                tool_call_id,
                ToolCallUpdateFields::new()
                    .title(title)
                    .kind(ToolKind::Think)
                    .status(ToolCallStatus::Pending)
                    .content(content)
                    .raw_input(raw_input),
            ),
            vec![
                PermissionOption::new(
                    APPROVE_FOR_SESSION,
                    "Yes, for session",
                    PermissionOptionKind::AllowAlways,
                ),
                PermissionOption::new(APPROVE_FOR_TURN, "Yes", PermissionOptionKind::AllowOnce),
                PermissionOption::new(DENY_REQUEST, "No", PermissionOptionKind::RejectOnce),
            ],
        )
        .await?;

    inner
        .app
        .send_permissions_request_approval_response(
            request_id,
            approval_response_from_outcome(outcome, &params.permissions),
        )
        .await
}

fn approval_response_from_outcome(
    outcome: RequestPermissionOutcome,
    requested: &AdditionalPermissionProfile,
) -> PermissionsRequestApprovalResponse {
    match outcome {
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            match option_id.0.as_ref() {
                APPROVE_FOR_SESSION => PermissionsRequestApprovalResponse {
                    permissions: granted_permissions_from_request(requested),
                    scope: PermissionGrantScope::Session,
                },
                APPROVE_FOR_TURN => PermissionsRequestApprovalResponse {
                    permissions: granted_permissions_from_request(requested),
                    scope: PermissionGrantScope::Turn,
                },
                _ => rejected_permissions_response(),
            }
        }
        RequestPermissionOutcome::Cancelled => rejected_permissions_response(),
        _ => rejected_permissions_response(),
    }
}

fn rejected_permissions_response() -> PermissionsRequestApprovalResponse {
    PermissionsRequestApprovalResponse {
        permissions: GrantedPermissionProfile::default(),
        scope: PermissionGrantScope::Turn,
    }
}

fn granted_permissions_from_request(
    requested: &AdditionalPermissionProfile,
) -> GrantedPermissionProfile {
    GrantedPermissionProfile {
        network: requested.network.clone(),
        file_system: requested.file_system.clone(),
        macos: requested
            .macos
            .as_ref()
            .map(granted_macos_permissions_from_request),
    }
}

fn permission_request_content(params: &PermissionsRequestApprovalParams) -> Vec<String> {
    let mut lines = Vec::new();

    if let Some(reason) = params.reason.as_ref()
        && !reason.trim().is_empty()
    {
        lines.push(reason.trim().to_string());
    }

    if let Some(file_system) = params.permissions.file_system.as_ref() {
        if let Some(read) = file_system.read.as_ref()
            && !read.is_empty()
        {
            lines.push(format!(
                "File system read: {}",
                read.iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if let Some(write) = file_system.write.as_ref()
            && !write.is_empty()
        {
            lines.push(format!(
                "File system write: {}",
                write
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    if let Some(network) = params.permissions.network.as_ref()
        && let Some(enabled) = network.enabled
    {
        lines.push(format!("Network access: {enabled}"));
    }

    if let Some(macos) = params.permissions.macos.as_ref() {
        lines.extend(macos_permission_lines(macos));
    }

    lines
}

fn granted_macos_permissions_from_request(
    requested: &AdditionalMacOsPermissions,
) -> GrantedMacOsPermissions {
    GrantedMacOsPermissions {
        preferences: Some(requested.preferences.clone()),
        automations: Some(requested.automations.clone()),
        launch_services: Some(requested.launch_services),
        accessibility: Some(requested.accessibility),
        calendar: Some(requested.calendar),
        reminders: Some(requested.reminders),
        contacts: Some(requested.contacts.clone()),
    }
}

fn macos_permission_lines(macos: &AdditionalMacOsPermissions) -> Vec<String> {
    vec![
        format!("macOS preferences: {:?}", macos.preferences),
        format!("macOS automations: {:?}", macos.automations),
        format!("macOS launch services: {}", macos.launch_services),
        format!("macOS accessibility: {}", macos.accessibility),
        format!("macOS calendar: {}", macos.calendar),
        format!("macOS reminders: {}", macos.reminders),
        format!("macOS contacts: {:?}", macos.contacts),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        APPROVE_FOR_SESSION, APPROVE_FOR_TURN, PermissionGrantScope,
        PermissionsRequestApprovalParams, approval_response_from_outcome,
        granted_permissions_from_request, permission_request_content,
    };
    use agent_client_protocol::{
        PermissionOptionId, RequestPermissionOutcome, SelectedPermissionOutcome,
    };
    use codex_app_server_protocol::AdditionalPermissionProfile;
    use codex_protocol::models::{
        MacOsAutomationPermission, MacOsContactsPermission, MacOsPreferencesPermission,
    };
    fn requested_profile() -> AdditionalPermissionProfile {
        serde_json::from_value(serde_json::json!({
            "network": { "enabled": true },
            "fileSystem": {
                "read": ["/tmp/read"],
                "write": ["/tmp/write"]
            },
            "macos": {
                "preferences": MacOsPreferencesPermission::ReadWrite,
                "automations": MacOsAutomationPermission::All,
                "launchServices": true,
                "accessibility": true,
                "calendar": false,
                "reminders": true,
                "contacts": MacOsContactsPermission::ReadOnly,
            }
        }))
        .expect("valid requested permissions profile")
    }

    #[test]
    fn decodes_permissions_request_from_protocol_types() {
        let decoded =
            serde_json::from_value::<PermissionsRequestApprovalParams>(serde_json::json!({
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "itemId": "item-1",
                    "reason": "Need broader access",
                    "permissions": {
                        "network": { "enabled": true },
                        "fileSystem": {
                            "read": ["/tmp/read"],
                            "write": ["/tmp/write"]
                        },
                    "macos": {
                        "preferences": "read_write",
                        "automations": "all",
                        "launchServices": true,
                        "accessibility": true,
                        "calendar": false,
                        "reminders": true,
                        "contacts": "read_only"
                    }
                }
            }))
            .expect("must decode params");

        assert_eq!(decoded.thread_id, "thread-1");
        assert_eq!(decoded.turn_id, "turn-1");
        assert_eq!(decoded.item_id, "item-1");
        assert_eq!(decoded.reason.as_deref(), Some("Need broader access"));
        assert_eq!(decoded.permissions, requested_profile());
    }

    #[test]
    fn grants_requested_permissions_for_session_scope() {
        let requested = requested_profile();
        let response = approval_response_from_outcome(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                PermissionOptionId::new(APPROVE_FOR_SESSION),
            )),
            &requested,
        );

        assert_eq!(response.scope, PermissionGrantScope::Session);
        assert_eq!(
            response.permissions,
            granted_permissions_from_request(&requested)
        );
    }

    #[test]
    fn grants_requested_permissions_for_turn_scope() {
        let requested = requested_profile();
        let response = approval_response_from_outcome(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                PermissionOptionId::new(APPROVE_FOR_TURN),
            )),
            &requested,
        );

        assert_eq!(response.scope, PermissionGrantScope::Turn);
        assert_eq!(
            response.permissions,
            granted_permissions_from_request(&requested)
        );
    }

    #[test]
    fn cancelled_permissions_request_rejects_with_empty_profile() {
        let requested = requested_profile();
        let response =
            approval_response_from_outcome(RequestPermissionOutcome::Cancelled, &requested);

        assert_eq!(response.scope, PermissionGrantScope::Turn);
        assert!(response.permissions.network.is_none());
        assert!(response.permissions.file_system.is_none());
        assert!(response.permissions.macos.is_none());
    }

    #[test]
    fn permission_request_content_includes_reason_and_requested_access() {
        let params = PermissionsRequestApprovalParams {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "item-1".to_string(),
            reason: Some("Need broader access".to_string()),
            permissions: requested_profile(),
        };

        let lines = permission_request_content(&params);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Need broader access"))
        );
        assert!(lines.iter().any(|line| line.contains("/tmp/read")));
        assert!(lines.iter().any(|line| line.contains("/tmp/write")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Network access: true"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("macOS accessibility: true"))
        );
    }
}
