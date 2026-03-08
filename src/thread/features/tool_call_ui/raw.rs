//! Raw-input payload construction for shell tool-call cards.

use codex_app_server_protocol::CommandAction;

pub(in crate::thread) fn command_tool_raw_input(
    command: &str,
    command_actions: &[CommandAction],
) -> Option<serde_json::Value> {
    if command.trim().is_empty() && command_actions.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "command": command,
        "commandActions": command_actions,
    }))
}
