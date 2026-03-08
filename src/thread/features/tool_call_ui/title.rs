//! Title and placeholder-content generation for shell tool-call cards.

use agent_client_protocol::ToolCallContent;
use codex_app_server_protocol::CommandAction;

// Build stable command titles so repeated updates map back to the same tool-call row.
pub(in crate::thread) fn command_tool_title(
    command: &str,
    command_actions: &[CommandAction],
) -> String {
    command_title_from_actions(command_actions).unwrap_or_else(|| command_title_from_shell(command))
}

pub(in crate::thread) fn command_tool_placeholder_content() -> Vec<ToolCallContent> {
    vec![
        "Command details are available in Raw Input."
            .to_string()
            .into(),
    ]
}

fn command_title_from_actions(command_actions: &[CommandAction]) -> Option<String> {
    let mut reads = Vec::new();
    let mut list_files_count = 0usize;
    let mut search_count = 0usize;
    let mut unknown_count = 0usize;

    for action in command_actions {
        match action {
            CommandAction::Read { path, .. } => reads.push(path),
            CommandAction::ListFiles { .. } => list_files_count += 1,
            CommandAction::Search { .. } => search_count += 1,
            CommandAction::Unknown { .. } => unknown_count += 1,
        }
    }

    if reads.is_empty() && list_files_count == 0 && search_count == 0 && unknown_count > 0 {
        return None;
    }

    if !reads.is_empty() && list_files_count == 0 && search_count == 0 {
        if reads.len() == 1 {
            if let Some(name) = reads[0]
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
            {
                return Some(format!("Read {name}"));
            }
            return Some("Read file".to_string());
        }
        return Some(format!("Read {} files", reads.len()));
    }

    if list_files_count > 0 && reads.is_empty() && search_count == 0 {
        return Some("Analyze folder contents".to_string());
    }

    if search_count > 0 && reads.is_empty() && list_files_count == 0 {
        return Some("Search in workspace".to_string());
    }

    if search_count > 0 && !reads.is_empty() && list_files_count == 0 {
        return Some("Search and inspect files".to_string());
    }

    if list_files_count > 0 || search_count > 0 || !reads.is_empty() {
        return Some("Inspect workspace files".to_string());
    }

    None
}

fn command_title_from_shell(command: &str) -> String {
    let inner_command = super::kind::extract_inner_shell_command(command);
    let normalized = inner_command.to_ascii_lowercase();

    if super::kind::command_looks_like_verification(&inner_command) {
        return "Run tests and checks".to_string();
    }
    if super::kind::looks_like_listing_command(&normalized) {
        return "Analyze folder contents".to_string();
    }
    if super::kind::looks_like_search_command(&normalized) {
        return "Search in workspace".to_string();
    }
    if super::kind::looks_like_read_command(&normalized) {
        return "Read file contents".to_string();
    }
    if super::kind::looks_like_git_inspection_command(&normalized) {
        return "Inspect git state".to_string();
    }

    "Run shell command".to_string()
}
