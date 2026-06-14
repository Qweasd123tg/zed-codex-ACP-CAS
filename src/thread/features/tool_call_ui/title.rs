//! Формирование заголовков для shell tool-call карточек.

use codex_app_server_protocol::CommandAction;

use super::kind::ShellOperation;

// Строим стабильные заголовки команд, чтобы повторные обновления маппились в ту же строку tool call.
pub(in crate::thread) fn command_tool_title(
    command: &str,
    command_actions: &[CommandAction],
) -> String {
    command_title_from_actions(command_actions).unwrap_or_else(|| command_title_from_shell(command))
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

    if let Some(operation) = super::kind::shell_operation(&inner_command) {
        return shell_operation_title(&operation);
    }
    if let Some(host) = network_host_from_shell_command(&inner_command) {
        return format!("Network access: {host}");
    }
    if let Some(title) = file_operation_title_from_shell_command(&inner_command) {
        return title;
    }
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

pub(in crate::thread) fn shell_operation_title(operation: &ShellOperation) -> String {
    match operation {
        ShellOperation::Fetch { url } => format!("Fetch {}", code_span(url)),
        ShellOperation::Copy {
            source,
            destination,
        } => {
            format!("Copy {} to {}", code_span(source), code_span(destination))
        }
        ShellOperation::Move {
            source,
            destination,
        } => {
            format!("Move {} to {}", code_span(source), code_span(destination))
        }
        ShellOperation::Delete { path } => format!("Delete {}", code_span(path)),
        ShellOperation::CreateDirectory { path } => {
            format!("Create directory {}", code_span(path))
        }
        ShellOperation::CreateFile { path } => format!("Create file {}", code_span(path)),
        ShellOperation::Modify { path } => format!("Modify {}", code_span(path)),
    }
}

fn network_host_from_shell_command(command: &str) -> Option<String> {
    let parts = shlex::split(command)?;
    let program = parts
        .first()
        .map(|part| command_name(part).to_ascii_lowercase())?;
    if !matches!(
        program.as_str(),
        "curl" | "wget" | "http" | "https" | "xh" | "invoke-webrequest" | "iwr"
    ) {
        return None;
    }

    parts
        .iter()
        .skip(1)
        .filter(|part| !part.starts_with('-'))
        .find_map(|part| url_host(part))
}

fn command_name(program: &str) -> String {
    std::path::Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program)
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_string()
}

fn url_host(value: &str) -> Option<String> {
    let rest = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))?;
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim_matches(['[', ']']);
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn file_operation_title_from_shell_command(command: &str) -> Option<String> {
    let parts = shlex::split(command)?;
    let program = parts
        .first()
        .map(|part| command_name(part).to_ascii_lowercase())?;
    match program.as_str() {
        "touch" => last_path_arg(&parts[1..]).map(|path| format!("Create file: {path}")),
        "mkdir" => last_path_arg(&parts[1..]).map(|path| format!("Create folder: {path}")),
        "rm" | "unlink" => last_path_arg(&parts[1..]).map(|path| format!("Delete path: {path}")),
        "mv" => Some("Move path".to_string()),
        "cp" => Some("Copy path".to_string()),
        "chmod" | "chown" => {
            last_path_arg(&parts[1..]).map(|path| format!("Change file permissions: {path}"))
        }
        "truncate" => last_path_arg(&parts[1..]).map(|path| format!("Modify file: {path}")),
        _ => None,
    }
}

fn last_path_arg(args: &[String]) -> Option<String> {
    args.iter()
        .rev()
        .find(|arg| is_path_like_arg(arg))
        .map(|path| path_display_name(path))
}

fn is_path_like_arg(arg: &str) -> bool {
    let trimmed = arg.trim();
    !trimmed.is_empty()
        && trimmed != "-"
        && !trimmed.starts_with('-')
        && !trimmed.contains('*')
        && !trimmed.contains('?')
}

fn path_display_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
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
