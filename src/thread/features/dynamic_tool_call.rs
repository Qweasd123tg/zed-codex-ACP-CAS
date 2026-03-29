//! Typed `item/tool/call` handling for app-server dynamic client tools.

use agent_client_protocol::{
    Error, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    SelectedPermissionOutcome, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
    ToolKind,
};
use codex_app_server_protocol::{
    DynamicToolCallOutputContentItem, DynamicToolCallParams, DynamicToolCallResponse, RequestId,
};
use tracing::warn;

use crate::thread::{REJECT_ONCE, ThreadInner};

const ACKNOWLEDGE_ONCE: &str = "dynamic-tool-call-acknowledge";

pub(in crate::thread) async fn handle_dynamic_tool_call(
    inner: &mut ThreadInner,
    request_id: RequestId,
    params: DynamicToolCallParams,
) -> Result<(), Error> {
    if let Err(response) = validate_dynamic_tool_turn(
        inner.active_turn_id.as_deref(),
        &params.turn_id,
        &params.tool,
    ) {
        warn!(
            request_turn_id = %params.turn_id,
            active_turn_id = ?inner.active_turn_id,
            tool = %params.tool,
            "Rejecting stale dynamic tool call request"
        );
        return inner
            .app
            .send_dynamic_tool_call_response(request_id, response)
            .await;
    }

    let outcome = inner
        .client
        .request_permission(
            ToolCallUpdate::new(
                ToolCallId::new(params.call_id.clone()),
                ToolCallUpdateFields::new()
                    .title(dynamic_tool_title(&params.tool))
                    .kind(ToolKind::Execute)
                    .status(ToolCallStatus::Pending)
                    .content(dynamic_tool_request_content(&params.tool))
                    .raw_input(dynamic_tool_raw_input(&params.tool, &params.arguments)),
            ),
            vec![
                PermissionOption::new(
                    ACKNOWLEDGE_ONCE,
                    "Acknowledge",
                    PermissionOptionKind::AllowOnce,
                ),
                PermissionOption::new(REJECT_ONCE, "Reject", PermissionOptionKind::RejectOnce),
            ],
        )
        .await?;

    inner
        .app
        .send_dynamic_tool_call_response(
            request_id,
            response_from_permission_outcome(outcome, &params.tool),
        )
        .await
}

fn validate_dynamic_tool_turn(
    active_turn_id: Option<&str>,
    request_turn_id: &str,
    tool: &str,
) -> Result<(), DynamicToolCallResponse> {
    match active_turn_id {
        Some(active_turn_id) if active_turn_id == request_turn_id => Ok(()),
        Some(active_turn_id) => Err(stale_turn_response(tool, Some(active_turn_id))),
        None => Err(stale_turn_response(tool, None)),
    }
}

fn response_from_permission_outcome(
    outcome: RequestPermissionOutcome,
    tool: &str,
) -> DynamicToolCallResponse {
    match outcome {
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. })
            if option_id.0.as_ref() == ACKNOWLEDGE_ONCE =>
        {
            acknowledged_without_execution_response(tool)
        }
        RequestPermissionOutcome::Cancelled
        | RequestPermissionOutcome::Selected(SelectedPermissionOutcome { .. }) => {
            declined_response(tool)
        }
        _ => declined_response(tool),
    }
}

fn acknowledged_without_execution_response(tool: &str) -> DynamicToolCallResponse {
    text_response(
        format!(
            "Dynamic tool `{tool}` was acknowledged, but this ACP adapter does not yet execute client-side dynamic tools."
        ),
        false,
    )
}

fn declined_response(tool: &str) -> DynamicToolCallResponse {
    text_response(
        format!("Dynamic tool `{tool}` was declined by the user."),
        false,
    )
}

fn stale_turn_response(tool: &str, active_turn_id: Option<&str>) -> DynamicToolCallResponse {
    let reason = match active_turn_id {
        Some(active_turn_id) => format!("Active turn: `{active_turn_id}`."),
        None => "There is no active turn.".to_string(),
    };
    text_response(
        format!(
            "Dynamic tool `{tool}` was ignored because its request targeted a stale turn. {reason}"
        ),
        false,
    )
}

fn text_response(text: String, success: bool) -> DynamicToolCallResponse {
    DynamicToolCallResponse {
        content_items: vec![DynamicToolCallOutputContentItem::InputText { text }],
        success,
    }
}

fn dynamic_tool_title(tool: &str) -> String {
    format!("Tool: {tool}")
}

fn dynamic_tool_request_content(tool: &str) -> Vec<agent_client_protocol::ToolCallContent> {
    vec![
        format!("Dynamic client tool requested: `{tool}`.").into(),
        "Arguments are available in Raw Input.".to_string().into(),
        "ACP 0.9.4 can confirm or reject this request, but it cannot collect arbitrary structured output from the client yet."
            .to_string()
            .into(),
    ]
}

fn dynamic_tool_raw_input(tool: &str, arguments: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "tool": tool,
        "arguments": arguments,
    })
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::{
        PermissionOptionId, RequestPermissionOutcome, SelectedPermissionOutcome,
    };

    use super::{
        ACKNOWLEDGE_ONCE, dynamic_tool_raw_input, response_from_permission_outcome,
        validate_dynamic_tool_turn,
    };

    #[test]
    fn acknowledged_response_is_typed_failure_until_execution_exists() {
        let response = response_from_permission_outcome(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                PermissionOptionId::new(ACKNOWLEDGE_ONCE),
            )),
            "lookup_ticket",
        );

        assert!(!response.success);
        assert_eq!(response.content_items.len(), 1);
    }

    #[test]
    fn cancelled_dynamic_tool_call_returns_declined_message() {
        let response =
            response_from_permission_outcome(RequestPermissionOutcome::Cancelled, "lookup_ticket");

        assert!(!response.success);
        assert_eq!(
            response.content_items,
            vec![
                codex_app_server_protocol::DynamicToolCallOutputContentItem::InputText {
                    text: "Dynamic tool `lookup_ticket` was declined by the user.".to_string(),
                }
            ]
        );
    }

    #[test]
    fn dynamic_tool_raw_input_keeps_tool_and_arguments() {
        let raw_input = dynamic_tool_raw_input("lookup_ticket", &serde_json::json!({ "id": 1 }));
        assert_eq!(
            raw_input,
            serde_json::json!({
                "tool": "lookup_ticket",
                "arguments": { "id": 1 },
            })
        );
    }

    #[test]
    fn rejects_dynamic_tool_call_without_active_turn() {
        let response = validate_dynamic_tool_turn(None, "turn-1", "lookup_ticket")
            .expect_err("missing active turn must be rejected");

        assert!(!response.success);
        assert_eq!(
            response.content_items,
            vec![
                codex_app_server_protocol::DynamicToolCallOutputContentItem::InputText {
                    text: "Dynamic tool `lookup_ticket` was ignored because its request targeted a stale turn. There is no active turn.".to_string(),
                }
            ]
        );
    }

    #[test]
    fn rejects_dynamic_tool_call_for_mismatched_turn() {
        let response = validate_dynamic_tool_turn(Some("turn-2"), "turn-1", "lookup_ticket")
            .expect_err("mismatched turn must be rejected");

        assert!(!response.success);
        assert_eq!(
            response.content_items,
            vec![
                codex_app_server_protocol::DynamicToolCallOutputContentItem::InputText {
                    text: "Dynamic tool `lookup_ticket` was ignored because its request targeted a stale turn. Active turn: `turn-2`.".to_string(),
                }
            ]
        );
    }
}
