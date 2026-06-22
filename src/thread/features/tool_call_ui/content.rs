//! Visible content builders for shell command tool-call cards.

use std::path::Path;

use agent_client_protocol::schema::v1::ToolCallContent;
use codex_app_server_protocol::{CommandAction, CommandExecutionStatus};

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
    vec![command_started_details(command, cwd, command_actions).into()]
}

pub(in crate::thread) fn command_tool_completed_content(
    command: &str,
    _cwd: &Path,
    command_actions: &[CommandAction],
    status: CommandExecutionStatus,
    aggregated_output: Option<&str>,
) -> Vec<ToolCallContent> {
    vec![command_output_section(command, status, command_actions, aggregated_output).into()]
}

fn command_started_details(command: &str, cwd: &Path, command_actions: &[CommandAction]) -> String {
    let mut body = String::new();
    body.push_str("Command\n");
    body.push_str(&fenced_code("sh", command.trim()));
    body.push_str("\n\ncwd: ");
    body.push_str(&cwd.display().to_string());

    let label = command_tool_label(command, cwd, command_actions);
    if !label.is_empty() {
        body.push_str("\n\nSummary: ");
        body.push_str(&label);
    }

    body
}

fn command_output_section(
    command: &str,
    status: CommandExecutionStatus,
    command_actions: &[CommandAction],
    aggregated_output: Option<&str>,
) -> String {
    let operation = super::kind::shell_operation(command);
    let Some(output) = aggregated_output
        .map(str::trim_end)
        .filter(|output| !output.is_empty())
    else {
        return operation
            .as_ref()
            .and_then(|operation| operation_completion_summary(operation, &status))
            .unwrap_or_else(|| command_completion_summary(&status).to_string());
    };

    let preview = output_preview(output);
    if operation
        .as_ref()
        .is_some_and(|operation| matches!(operation, super::kind::ShellOperation::Fetch { .. }))
        || should_render_plain_output(command_actions, &preview.text)
    {
        let mut section = String::new();
        if let Some(summary) = preview.omission_summary.as_ref() {
            section.push_str("Output (");
            section.push_str(summary);
            section.push_str(")\n");
        }
        section.push_str(&preview.text);
        return section;
    }

    let mut section = String::new();
    section.push_str("Terminal");
    if let Some(summary) = preview.omission_summary.as_ref() {
        section.push_str(" (");
        section.push_str(summary);
        section.push(')');
    }
    section.push_str(":\n");
    section.push_str(&fenced_code("", &preview.text));
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

fn should_render_plain_output(command_actions: &[CommandAction], output: &str) -> bool {
    command_actions.iter().any(|action| {
        matches!(
            action,
            CommandAction::ListFiles {
                path: _,
                command: _
            }
        )
    }) && output.lines().count() <= 4
        && output.chars().count() <= 500
}

pub(in crate::thread) fn command_tool_label(
    command: &str,
    cwd: &Path,
    command_actions: &[CommandAction],
) -> String {
    match command_actions {
        [CommandAction::Read { path, .. }] => {
            format!("Read {}", code_span(&display_path(cwd, path)))
        }
        [CommandAction::ListFiles { path, .. }] => {
            format!(
                "List the {} directory's contents",
                code_span(&display_optional_path(cwd, path.as_deref()))
            )
        }
        [CommandAction::Search { query, path, .. }] => {
            let query = query
                .as_deref()
                .map(str::trim)
                .filter(|query| !query.is_empty())
                .unwrap_or("*");
            format!(
                "Search for {} in {}",
                code_span(query),
                code_span(&display_optional_path(cwd, path.as_deref()))
            )
        }
        [CommandAction::Unknown { .. }] | [] => shell_tool_call_label(command),
        _ => super::title::command_tool_title(command, command_actions),
    }
}

fn shell_tool_call_label(command: &str) -> String {
    if let Some(operation) = super::kind::shell_operation(command) {
        return super::title::shell_operation_title(&operation);
    }
    let inner = super::kind::extract_inner_shell_command(command);
    let label = compact_inline(&inner);
    if label.is_empty() {
        "Run shell command".to_string()
    } else {
        label
    }
}

fn operation_completion_summary(
    operation: &super::kind::ShellOperation,
    status: &CommandExecutionStatus,
) -> Option<String> {
    if *status != CommandExecutionStatus::Completed {
        return None;
    }
    match operation {
        super::kind::ShellOperation::Fetch { url } => Some(format!("Fetched {url}")),
        super::kind::ShellOperation::Copy {
            source,
            destination,
        } => Some(format!("Copied {source} to {destination}")),
        super::kind::ShellOperation::Move {
            source,
            destination,
        } => Some(format!("Moved {source} to {destination}")),
        super::kind::ShellOperation::Delete { path } => Some(format!("Deleted {path}")),
        super::kind::ShellOperation::CreateDirectory { path } => {
            Some(format!("Created directory {path}"))
        }
        super::kind::ShellOperation::CreateFile { path } => Some(format!("Created file {path}")),
        super::kind::ShellOperation::Modify { path } => Some(format!("Modified {path}")),
    }
}

fn command_completion_summary(status: &CommandExecutionStatus) -> &'static str {
    match status {
        CommandExecutionStatus::Completed => "Command completed without output.",
        CommandExecutionStatus::Failed => "Command failed without output.",
        CommandExecutionStatus::Declined => "Command was declined.",
        CommandExecutionStatus::InProgress => "Command is still running.",
    }
}

fn display_optional_path(cwd: &Path, path: Option<&str>) -> String {
    let path = path
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .unwrap_or(".");
    display_path(cwd, Path::new(path))
}

fn display_path(cwd: &Path, path: &Path) -> String {
    if path.as_os_str().is_empty() || path == Path::new(".") {
        return cwd.display().to_string();
    }
    if path.is_absolute() {
        return path.display().to_string();
    }
    cwd.join(path).display().to_string()
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

fn code_span(text: &str) -> String {
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
    let fence = "`".repeat(longest_run.saturating_add(1).max(1));
    format!("{fence}{text}{fence}")
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
                agent_client_protocol::schema::v1::ContentBlock::Text(text) => {
                    text.text.to_string()
                }
                _ => panic!("expected text content"),
            },
            _ => panic!("expected content block"),
        }
    }

    #[test]
    fn command_tool_label_uses_zed_transcript_label() {
        let actions = vec![CommandAction::Read {
            command: "cat src/lib.rs".to_string(),
            name: "cat".to_string(),
            path: PathBuf::from("src/lib.rs"),
        }];

        let label = super::command_tool_label("cat src/lib.rs", Path::new("/repo"), &actions);

        assert_eq!(label, "Read `/repo/src/lib.rs`");
    }

    #[test]
    fn command_started_content_keeps_expandable_details_without_transcript_chrome() {
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

        assert!(text.contains("Command\n```sh\ncat src/lib.rs\n```"));
        assert!(text.contains("cwd: /repo"));
        assert!(text.contains("Summary: Read `/repo/src/lib.rs`"));
        assert!(!text.contains("**Tool Call:"));
        assert!(!text.contains("Status:"));
    }

    #[test]
    fn command_completed_content_uses_terminal_block_for_shell_output() {
        let text = first_text(command_tool_completed_content(
            "bash -lc 'echo \"Hello from terminal!\" && date'",
            Path::new("/repo"),
            &[],
            CommandExecutionStatus::Completed,
            Some("Hello from terminal!\nSun Jun 14 00:06:51 MSK 2026"),
        ));

        assert!(
            text.contains(
                "Terminal:\n```\nHello from terminal!\nSun Jun 14 00:06:51 MSK 2026\n```"
            )
        );
        assert!(!text.contains("**Tool Call:"));
        assert!(!text.contains("Status:"));
    }

    #[test]
    fn command_completed_content_renders_short_list_output_as_plain_text() {
        let actions = vec![CommandAction::ListFiles {
            path: None,
            command: "ls".to_string(),
        }];

        let text = first_text(command_tool_completed_content(
            "ls",
            Path::new("/home/qweasd123tg/Code/1"),
            &actions,
            CommandExecutionStatus::Completed,
            Some("/home/qweasd123tg/Code/1 is empty."),
        ));

        assert_eq!(text, "/home/qweasd123tg/Code/1 is empty.");
        assert!(!text.contains("Terminal:"));
    }

    #[test]
    fn command_completed_content_renders_fetch_output_as_plain_text() {
        let text = first_text(command_tool_completed_content(
            "curl https://example.com",
            Path::new("/repo"),
            &[],
            CommandExecutionStatus::Completed,
            Some("# Example Domain\n\nLearn more"),
        ));

        assert_eq!(text, "# Example Domain\n\nLearn more");
        assert!(!text.contains("Terminal:"));
    }

    #[test]
    fn command_completed_content_summarizes_file_operations_without_output() {
        let text = first_text(command_tool_completed_content(
            "cp README.md docs/README.md",
            Path::new("/repo"),
            &[],
            CommandExecutionStatus::Completed,
            None,
        ));

        assert_eq!(text, "Copied README.md to docs/README.md");
    }

    #[test]
    fn command_completed_content_caps_long_output() {
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

        assert!(text.contains("Terminal (showing first 80 and last 80 lines):"));
        assert!(text.contains("[... 40 line(s) omitted ...]"));
        assert!(text.contains("line 199"));
    }
}
