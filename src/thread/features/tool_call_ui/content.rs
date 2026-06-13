//! Visible content builders for shell command tool-call cards.

use std::path::Path;

use agent_client_protocol::schema::ToolCallContent;
use codex_app_server_protocol::{CommandAction, CommandExecutionStatus};

const ACTION_PREVIEW_LIMIT: usize = 8;
const OUTPUT_HEAD_LINES: usize = 80;
const OUTPUT_TAIL_LINES: usize = 80;
const OUTPUT_MAX_CHARS: usize = 24_000;
const OUTPUT_HEAD_CHARS: usize = 11_500;
const OUTPUT_TAIL_CHARS: usize = 11_500;

pub(in crate::thread) fn command_tool_started_content(
    command: &str,
    cwd: &Path,
    command_actions: &[CommandAction],
) -> Vec<ToolCallContent> {
    vec![command_card_body(command, cwd, command_actions, Some("running"), None).into()]
}

pub(in crate::thread) fn command_tool_completed_content(
    command: &str,
    cwd: &Path,
    command_actions: &[CommandAction],
    status: CommandExecutionStatus,
    aggregated_output: Option<&str>,
) -> Vec<ToolCallContent> {
    vec![
        command_card_body(
            command,
            cwd,
            command_actions,
            Some(command_status_label(status.clone())),
            Some(command_output_section(status, aggregated_output)),
        )
        .into(),
    ]
}

fn command_card_body(
    command: &str,
    cwd: &Path,
    command_actions: &[CommandAction],
    status: Option<&str>,
    output_section: Option<String>,
) -> String {
    let mut body = String::new();
    if let Some(status) = status {
        body.push_str("Status: ");
        body.push_str(status);
        body.push_str("\n\n");
    }

    body.push_str("Command\n");
    body.push_str(&fenced_code("sh", command.trim()));
    body.push_str("\n\n");
    body.push_str("cwd: ");
    body.push_str(&cwd.display().to_string());

    let action_lines = command_action_lines(command_actions);
    if !action_lines.is_empty() {
        body.push_str("\n\nActions\n");
        body.push_str(&action_lines.join("\n"));
    }

    if let Some(output_section) = output_section {
        body.push_str("\n\n");
        body.push_str(&output_section);
    }

    body
}

fn command_output_section(
    status: CommandExecutionStatus,
    aggregated_output: Option<&str>,
) -> String {
    let Some(output) = aggregated_output
        .map(str::trim_end)
        .filter(|output| !output.is_empty())
    else {
        return command_completion_summary(status).to_string();
    };

    let preview = output_preview(output);
    let mut section = String::new();
    section.push_str("Output");
    if let Some(summary) = preview.omission_summary.as_ref() {
        section.push_str(" (");
        section.push_str(summary);
        section.push(')');
    }
    section.push('\n');
    section.push_str(&fenced_code("text", &preview.text));
    section
}

struct OutputPreview {
    text: String,
    omission_summary: Option<String>,
}

fn output_preview(output: &str) -> OutputPreview {
    let lines = output.lines().collect::<Vec<_>>();
    if lines.len() > OUTPUT_HEAD_LINES + OUTPUT_TAIL_LINES {
        let omitted = lines.len() - OUTPUT_HEAD_LINES - OUTPUT_TAIL_LINES;
        let mut text = String::new();
        text.push_str(&lines[..OUTPUT_HEAD_LINES].join("\n"));
        text.push_str("\n\n[... ");
        text.push_str(&omitted.to_string());
        text.push_str(" line(s) omitted ...]\n\n");
        text.push_str(&lines[lines.len() - OUTPUT_TAIL_LINES..].join("\n"));
        return OutputPreview {
            text,
            omission_summary: Some(format!(
                "showing first {OUTPUT_HEAD_LINES} and last {OUTPUT_TAIL_LINES} lines"
            )),
        };
    }

    if output.chars().count() > OUTPUT_MAX_CHARS {
        let head = output.chars().take(OUTPUT_HEAD_CHARS).collect::<String>();
        let tail = output
            .chars()
            .rev()
            .take(OUTPUT_TAIL_CHARS)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        let omitted = output
            .chars()
            .count()
            .saturating_sub(OUTPUT_HEAD_CHARS + OUTPUT_TAIL_CHARS);
        return OutputPreview {
            text: format!("{head}\n\n[... {omitted} character(s) omitted ...]\n\n{tail}"),
            omission_summary: Some(format!(
                "showing first {OUTPUT_HEAD_CHARS} and last {OUTPUT_TAIL_CHARS} characters"
            )),
        };
    }

    OutputPreview {
        text: output.to_string(),
        omission_summary: None,
    }
}

fn command_action_lines(command_actions: &[CommandAction]) -> Vec<String> {
    let mut lines = command_actions
        .iter()
        .take(ACTION_PREVIEW_LIMIT)
        .map(command_action_line)
        .collect::<Vec<_>>();
    let remaining = command_actions.len().saturating_sub(ACTION_PREVIEW_LIMIT);
    if remaining > 0 {
        lines.push(format!("- ... {remaining} more action(s)"));
    }
    lines
}

fn command_action_line(action: &CommandAction) -> String {
    match action {
        CommandAction::Read { name, path, .. } => {
            let mut line = format!("- read {}", path.display());
            let name = compact_inline(name);
            if !name.is_empty() {
                line.push_str(" via ");
                line.push_str(&name);
            }
            line
        }
        CommandAction::ListFiles { path, command } => {
            let target = path.as_deref().unwrap_or(".");
            format!(
                "- list files in {} ({})",
                compact_inline(target),
                compact_inline(command)
            )
        }
        CommandAction::Search {
            query,
            path,
            command,
        } => {
            let query = query.as_deref().unwrap_or("*");
            let target = path.as_deref().unwrap_or(".");
            format!(
                "- search {} in {} ({})",
                compact_inline(query),
                compact_inline(target),
                compact_inline(command)
            )
        }
        CommandAction::Unknown { command } => {
            format!("- run {}", compact_inline(command))
        }
    }
}

fn command_status_label(status: CommandExecutionStatus) -> &'static str {
    match status {
        CommandExecutionStatus::Completed => "completed",
        CommandExecutionStatus::Failed => "failed",
        CommandExecutionStatus::Declined => "declined",
        CommandExecutionStatus::InProgress => "running",
    }
}

fn command_completion_summary(status: CommandExecutionStatus) -> &'static str {
    match status {
        CommandExecutionStatus::Completed => "Command completed without output.",
        CommandExecutionStatus::Failed => "Command failed without output.",
        CommandExecutionStatus::Declined => "Command was declined.",
        CommandExecutionStatus::InProgress => "Command is still running.",
    }
}

fn fenced_code(language: &str, text: &str) -> String {
    let mut longest_run = 0usize;
    let mut current_run = 0usize;
    for ch in text.chars() {
        if ch == '`' {
            current_run += 1;
            longest_run = longest_run.max(current_run);
        } else {
            current_run = 0;
        }
    }
    let longest_backtick_run = longest_run.saturating_add(1).max(3);
    let fence = "`".repeat(longest_backtick_run);
    format!("{fence}{language}\n{text}\n{fence}")
}

fn compact_inline(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{command_tool_completed_content, command_tool_started_content};
    use crate::thread::ToolCallContent;
    use codex_app_server_protocol::{CommandAction, CommandExecutionStatus};
    use std::path::{Path, PathBuf};

    fn first_text(content: Vec<ToolCallContent>) -> String {
        match content.into_iter().next().expect("content") {
            ToolCallContent::Content(content) => match content.content {
                agent_client_protocol::schema::ContentBlock::Text(text) => text.text.to_string(),
                _ => panic!("expected text content"),
            },
            _ => panic!("expected content block"),
        }
    }

    #[test]
    fn command_started_content_shows_command_cwd_and_actions() {
        let actions = vec![CommandAction::Read {
            command: "cat src/lib.rs".to_string(),
            name: "cat".to_string(),
            path: PathBuf::from("src/lib.rs"),
        }];

        let text = first_text(command_tool_started_content(
            "cat src/lib.rs",
            Path::new("/repo"),
            &actions,
        ));

        assert!(text.contains("Status: running"));
        assert!(text.contains("```sh\ncat src/lib.rs\n```"));
        assert!(text.contains("cwd: /repo"));
        assert!(text.contains("Actions\n- read src/lib.rs via cat"));
    }

    #[test]
    fn command_completed_content_keeps_context_and_caps_long_output() {
        let output = (0..200)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");

        let text = first_text(command_tool_completed_content(
            "cargo test",
            Path::new("/repo"),
            &[],
            CommandExecutionStatus::Completed,
            Some(&output),
        ));

        assert!(text.contains("Status: completed"));
        assert!(text.contains("Command\n```sh\ncargo test\n```"));
        assert!(text.contains("Output (showing first 80 and last 80 lines)"));
        assert!(text.contains("[... 40 line(s) omitted ...]"));
        assert!(text.contains("line 199"));
    }
}
