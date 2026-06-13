//! Обработчики дельт терминального вывода для потоков команд/изменений файлов в ACP UI.

use super::{ThreadInner, ToolCallId, ToolCallUpdate, ToolCallUpdateFields};
use agent_client_protocol::schema::Meta;
use codex_app_server_protocol::{
    CommandExecutionOutputDeltaNotification, TerminalInteractionNotification,
};

// Добавляем вывод команды инкрементально, чтобы сохранить живой стриминг.
pub(super) async fn handle_command_output_delta(
    inner: &mut ThreadInner,
    payload: CommandExecutionOutputDeltaNotification,
) {
    if !inner.started_tool_calls.contains(&payload.item_id) {
        return;
    }
    if !payload.delta.trim().is_empty() {
        inner.mark_turn_progress();
    }

    let update = if inner.terminal_tool_call_ids.contains(&payload.item_id) {
        if !payload.delta.is_empty() {
            inner
                .terminal_tool_call_output_seen
                .insert(payload.item_id.clone());
        }
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
    if !payload.stdin.trim().is_empty() {
        inner.mark_turn_progress();
    }

    if inner.terminal_tool_call_ids.contains(&payload.item_id) {
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
