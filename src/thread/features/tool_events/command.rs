//! Live/replay обработка shell command tool-call веток.

use std::path::{Path, PathBuf};

use agent_client_protocol::schema::{
    Meta, Terminal, ToolCall, ToolCallContent, ToolCallId, ToolCallUpdate, ToolCallUpdateFields,
    ToolKind,
};
use codex_app_server_protocol::{CommandAction, CommandExecutionStatus};
use serde_json::Value;
use tracing::warn;

use crate::thread::{
    SessionClient, ThreadInner,
    features::{
        file::changes as file_changes,
        status_mapping,
        tool_call_ui::{content, kind, location, raw},
    },
};

#[derive(Debug)]
// Replay-пакет для shell command item, чтобы не передавать много аргументов.
pub(in crate::thread) struct ReplayCommandExecution {
    pub(in crate::thread) id: String,
    pub(in crate::thread) command: String,
    pub(in crate::thread) cwd: PathBuf,
    pub(in crate::thread) status: CommandExecutionStatus,
    pub(in crate::thread) command_actions: Vec<CommandAction>,
    pub(in crate::thread) aggregated_output: Option<String>,
    pub(in crate::thread) exit_code_raw_output: Option<Value>,
}

#[derive(Debug)]
// Completed-пакет для shell-команды, чтобы live/replay render имели одинаковый shape.
pub(in crate::thread) struct CompletedCommandExecution {
    pub(in crate::thread) id: String,
    pub(in crate::thread) command: String,
    pub(in crate::thread) cwd: PathBuf,
    pub(in crate::thread) status: CommandExecutionStatus,
    pub(in crate::thread) command_actions: Vec<CommandAction>,
    pub(in crate::thread) aggregated_output: Option<String>,
    pub(in crate::thread) exit_code_raw_output: Option<Value>,
}

// Публикуем старт shell-команды и подготавливаем read-snapshot, если есть read actions.
pub(in crate::thread) async fn emit_command_execution_started(
    inner: &mut ThreadInner,
    id: String,
    command: String,
    cwd: PathBuf,
    status: CommandExecutionStatus,
    command_actions: Vec<CommandAction>,
) {
    inner.started_tool_calls.insert(id.clone());
    if inner.client.supports_read_text_file() {
        for action in &command_actions {
            let CommandAction::Read { path, .. } = action else {
                continue;
            };
            let read_path = file_changes::resolve_workspace_path(&inner.workspace_cwd, path);
            if let Err(err) = inner.client.prime_file_snapshot(read_path.clone()).await {
                warn!(
                    "Failed to prime ACP snapshot for command read {}: {err:?}",
                    read_path.display()
                );
            }
        }
    }
    let tool_status = status_mapping::map_command_status(status, true);
    let title = content::command_tool_label(&command, &cwd, &command_actions);
    let raw_input = raw::command_tool_raw_input(&command, &command_actions);
    let use_native_terminal = inner.client.supports_terminal_output()
        && command_uses_native_terminal(&command, &command_actions);
    if use_native_terminal {
        inner.terminal_tool_call_ids.insert(id.clone());
    }
    let tool_kind = command_tool_kind(&command, &command_actions, use_native_terminal);
    let locations = location::command_tool_locations(&cwd, &command, &command_actions);
    let tool_content =
        command_started_content(&id, &command, &cwd, &command_actions, use_native_terminal);
    let mut tool_call = ToolCall::new(ToolCallId::new(id.clone()), title)
        .kind(tool_kind)
        .status(tool_status)
        .locations(locations)
        .content(tool_content)
        .raw_input(raw_input);
    if use_native_terminal {
        tool_call = tool_call.meta(terminal_info_meta(&id, &cwd));
    }
    inner.client.send_tool_call(tool_call).await;
}

// Публикуем завершение shell-команды с агрегированным stdout/stderr и exit code.
pub(in crate::thread) async fn emit_command_execution_completed(
    inner: &mut ThreadInner,
    data: CompletedCommandExecution,
) {
    let CompletedCommandExecution {
        id,
        command,
        cwd,
        status,
        command_actions,
        aggregated_output,
        exit_code_raw_output,
    } = data;

    let use_native_terminal = inner.terminal_tool_call_ids.remove(&id);
    let terminal_output_seen = inner.terminal_tool_call_output_seen.remove(&id);
    let mut fields = ToolCallUpdateFields::new()
        .status(status_mapping::map_command_status(status.clone(), false));
    if !use_native_terminal {
        fields = fields.content(content::command_tool_completed_content(
            &command,
            &cwd,
            &command_actions,
            status,
            aggregated_output.as_deref(),
        ));
    }
    let terminal_meta = use_native_terminal.then(|| {
        terminal_completion_meta(
            &id,
            (!terminal_output_seen)
                .then_some(aggregated_output.as_deref())
                .flatten(),
            exit_code_raw_output.as_ref(),
        )
    });
    if let Some(raw_output) = exit_code_raw_output.clone() {
        fields = fields.raw_output(raw_output);
    }

    let mut update = ToolCallUpdate::new(ToolCallId::new(id.clone()), fields);
    if let Some(meta) = terminal_meta {
        update = update.meta(meta);
    }
    inner.client.send_tool_call_update(update).await;
    inner.started_tool_calls.remove(&id);
}

// Replay-рендер shell-команды: сразу start + update.
pub(in crate::thread) async fn replay_command_execution(
    client: &SessionClient,
    data: ReplayCommandExecution,
) {
    let ReplayCommandExecution {
        id,
        command,
        cwd,
        status,
        command_actions,
        aggregated_output,
        exit_code_raw_output,
    } = data;

    let title = content::command_tool_label(&command, &cwd, &command_actions);
    let raw_input = raw::command_tool_raw_input(&command, &command_actions);
    let use_native_terminal = client.supports_terminal_output()
        && command_uses_native_terminal(&command, &command_actions);
    let tool_kind = command_tool_kind(&command, &command_actions, use_native_terminal);
    let locations = location::command_tool_locations(&cwd, &command, &command_actions);
    let initial_content = if use_native_terminal {
        vec![ToolCallContent::Terminal(Terminal::new(id.clone()))]
    } else {
        content::command_tool_completed_content(
            &command,
            &cwd,
            &command_actions,
            status.clone(),
            aggregated_output.as_deref(),
        )
    };
    let mut tool_call = ToolCall::new(ToolCallId::new(id.clone()), title)
        .kind(tool_kind)
        .status(status_mapping::map_command_status(status.clone(), false))
        .locations(locations)
        .content(initial_content)
        .raw_input(raw_input);
    if use_native_terminal {
        tool_call = tool_call.meta(terminal_info_meta(&id, &cwd));
    }
    client.send_tool_call(tool_call).await;

    let mut fields = ToolCallUpdateFields::new()
        .status(status_mapping::map_command_status(status.clone(), false));
    if !use_native_terminal {
        fields = fields.content(content::command_tool_completed_content(
            &command,
            &cwd,
            &command_actions,
            status,
            aggregated_output.as_deref(),
        ));
    }
    let terminal_meta = if use_native_terminal {
        Some(terminal_completion_meta(
            &id,
            aggregated_output.as_deref(),
            exit_code_raw_output.as_ref(),
        ))
    } else {
        None
    };
    if let Some(raw_output) = exit_code_raw_output.clone() {
        fields = fields.raw_output(raw_output);
    }
    let mut update = ToolCallUpdate::new(ToolCallId::new(id), fields);
    if let Some(meta) = terminal_meta {
        update = update.meta(meta);
    }
    client.send_tool_call_update(update).await;
}

fn command_started_content(
    id: &str,
    command: &str,
    cwd: &Path,
    command_actions: &[CommandAction],
    use_native_terminal: bool,
) -> Vec<ToolCallContent> {
    if use_native_terminal {
        vec![ToolCallContent::Terminal(Terminal::new(id.to_string()))]
    } else {
        content::command_tool_started_content(command, cwd, command_actions)
    }
}

pub(in crate::thread) fn command_uses_native_terminal(
    command: &str,
    command_actions: &[CommandAction],
) -> bool {
    (command_actions.is_empty()
        || command_actions
            .iter()
            .all(|action| matches!(action, CommandAction::Unknown { .. })))
        && kind::shell_operation(command).is_none()
}

fn command_tool_kind(
    command: &str,
    command_actions: &[CommandAction],
    use_native_terminal: bool,
) -> ToolKind {
    if use_native_terminal {
        ToolKind::Execute
    } else {
        kind::command_tool_kind(command, command_actions)
    }
}

pub(in crate::thread) fn terminal_info_meta(id: &str, cwd: &Path) -> Meta {
    Meta::from_iter([(
        "terminal_info".to_owned(),
        serde_json::json!({
            "terminal_id": id,
            "cwd": cwd.display().to_string(),
        }),
    )])
}

fn terminal_exit_meta(id: &str, raw_output: Option<&Value>) -> Meta {
    Meta::from_iter([(
        "terminal_exit".to_owned(),
        serde_json::json!({
            "terminal_id": id,
            "exit_code": raw_output.and_then(command_exit_code),
            "signal": null,
        }),
    )])
}

fn terminal_completion_meta(
    id: &str,
    aggregated_output: Option<&str>,
    raw_output: Option<&Value>,
) -> Meta {
    let mut meta = terminal_exit_meta(id, raw_output);
    if let Some(output) = aggregated_output
        .map(str::trim_end)
        .filter(|output| !output.is_empty())
    {
        meta.insert(
            "terminal_output".to_owned(),
            serde_json::json!({
                "terminal_id": id,
                "data": output,
            }),
        );
    }
    meta
}

fn command_exit_code(raw_output: &Value) -> Option<u32> {
    raw_output
        .get("exit_code")
        .and_then(Value::as_u64)
        .and_then(|code| u32::try_from(code).ok())
}

#[cfg(test)]
mod tests {
    use super::{
        command_exit_code, command_started_content, command_tool_kind,
        command_uses_native_terminal, terminal_completion_meta, terminal_info_meta,
    };
    use agent_client_protocol::schema::{ToolCallContent, ToolKind};
    use codex_app_server_protocol::CommandAction;
    use std::path::{Path, PathBuf};

    #[test]
    fn native_terminal_is_reserved_for_shell_or_unknown_commands() {
        assert!(command_uses_native_terminal("echo hi", &[]));
        assert!(command_uses_native_terminal(
            "echo hi",
            &[CommandAction::Unknown {
                command: "echo hi".to_string(),
            }]
        ));
        assert!(!command_uses_native_terminal(
            "ls",
            &[CommandAction::ListFiles {
                path: None,
                command: "ls".to_string(),
            }]
        ));
        assert!(!command_uses_native_terminal(
            "cat src/lib.rs",
            &[CommandAction::Read {
                command: "cat src/lib.rs".to_string(),
                name: "cat".to_string(),
                path: PathBuf::from("src/lib.rs"),
            }]
        ));
        assert!(!command_uses_native_terminal(
            "curl -I https://example.com",
            &[]
        ));
        assert!(!command_uses_native_terminal(
            "cp README.md docs/README.md",
            &[]
        ));
    }

    #[test]
    fn terminal_info_meta_matches_zed_display_only_terminal_contract() {
        let meta = terminal_info_meta("call-1", Path::new("/repo"));

        assert_eq!(
            meta.get("terminal_info")
                .and_then(|value| value.get("terminal_id"))
                .and_then(|value| value.as_str()),
            Some("call-1")
        );
        assert_eq!(
            meta.get("terminal_info")
                .and_then(|value| value.get("cwd"))
                .and_then(|value| value.as_str()),
            Some("/repo")
        );
    }

    #[test]
    fn native_terminal_started_content_uses_acp_terminal_variant() {
        let content = command_started_content("call-1", "echo hi", Path::new("/repo"), &[], true);

        assert!(matches!(
            content.as_slice(),
            [ToolCallContent::Terminal(terminal)]
                if terminal.terminal_id.0.as_ref() == "call-1"
        ));
        assert_eq!(command_tool_kind("echo hi", &[], true), ToolKind::Execute);
    }

    #[test]
    fn terminal_completion_meta_includes_output_and_exit() {
        let meta = terminal_completion_meta(
            "call-1",
            Some("hello\n"),
            Some(&serde_json::json!({ "exit_code": 0 })),
        );

        assert_eq!(
            meta.get("terminal_output")
                .and_then(|value| value.get("data"))
                .and_then(|value| value.as_str()),
            Some("hello")
        );
        assert_eq!(
            meta.get("terminal_exit")
                .and_then(|value| value.get("exit_code"))
                .and_then(|value| value.as_u64()),
            Some(0)
        );
    }

    #[test]
    fn command_exit_code_rejects_out_of_range_values() {
        assert_eq!(
            command_exit_code(&serde_json::json!({ "exit_code": 7 })),
            Some(7)
        );
        assert_eq!(
            command_exit_code(&serde_json::json!({ "exit_code": u64::MAX })),
            None
        );
    }
}
