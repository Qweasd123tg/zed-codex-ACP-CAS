//! Terminal-output delta handlers for command and file-change streams in the ACP UI.

use super::{ThreadInner, ToolCallId, ToolCallUpdate, ToolCallUpdateFields};
use agent_client_protocol::Meta;
use codex_app_server_protocol::{
    CommandExecutionOutputDeltaNotification, TerminalInteractionNotification,
};

// Append command output incrementally to preserve live streaming behavior.
pub(super) async fn handle_command_output_delta(
    inner: &mut ThreadInner,
    payload: CommandExecutionOutputDeltaNotification,
) {
    if !inner.started_tool_calls.contains(&payload.item_id) {
        return;
    }

    let update = if inner.client.supports_terminal_output() {
        ToolCallUpdate::new(
            ToolCallId::new(payload.item_id.clone()),
            ToolCallUpdateFields::new(),
        )
        .meta(Meta::from_iter([(
            "terminal_output".to_owned(),
            serde_json::json!({
                "terminal_id": payload.item_id,
                "data": payload.delta,
            }),
        )]))
    } else {
        ToolCallUpdate::new(
            ToolCallId::new(payload.item_id),
            ToolCallUpdateFields::new().content(vec![payload.delta.into()]),
        )
    };

    inner.client.send_tool_call_update(update).await;
}

pub(super) async fn handle_terminal_interaction(
    inner: &mut ThreadInner,
    payload: TerminalInteractionNotification,
) {
    if !inner.started_tool_calls.contains(&payload.item_id) {
        return;
    }

    if inner.client.supports_terminal_output() {
        inner
            .client
            .send_tool_call_update(
                ToolCallUpdate::new(
                    ToolCallId::new(payload.item_id.clone()),
                    ToolCallUpdateFields::new(),
                )
                .meta(Meta::from_iter([(
                    "terminal_output".to_owned(),
                    serde_json::json!({
                        "terminal_id": payload.item_id,
                        "data": format!("\n{}\n", payload.stdin),
                    }),
                )])),
            )
            .await;
    }
}
