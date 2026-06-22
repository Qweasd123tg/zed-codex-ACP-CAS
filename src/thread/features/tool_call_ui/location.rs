//! Location mapping for shell tool-call cards and approval popups.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::ToolCallLocation;
use codex_app_server_protocol::CommandAction;

use super::kind::extract_inner_shell_command;

pub(in crate::thread) fn command_tool_locations(
    cwd: &Path,
    command: &str,
    command_actions: &[CommandAction],
) -> Vec<ToolCallLocation> {
    let mut locations = concrete_action_locations(cwd, command_actions);
    if locations.is_empty()
        && let Some(path) = obvious_shell_command_path(cwd, command)
    {
        locations.push(ToolCallLocation::new(path));
    }
    if locations.is_empty() {
        locations.push(ToolCallLocation::new(cwd.to_path_buf()));
    }
    locations
}

fn concrete_action_locations(
    cwd: &Path,
    command_actions: &[CommandAction],
) -> Vec<ToolCallLocation> {
    let mut seen = HashSet::new();
    let mut locations = Vec::new();
    let mut unknown_commands = Vec::new();

    for action in command_actions {
        let path = match action {
            CommandAction::Read { path, .. } => Some(resolve_command_path(cwd, path)),
            CommandAction::ListFiles { path, command } => path
                .as_deref()
                .and_then(|path| action_path(cwd, path))
                .or_else(|| {
                    unknown_commands.push(command.as_str());
                    None
                }),
            CommandAction::Search { path, command, .. } => path
                .as_deref()
                .and_then(|path| action_path(cwd, path))
                .or_else(|| {
                    unknown_commands.push(command.as_str());
                    None
                }),
            CommandAction::Unknown { command } => {
                unknown_commands.push(command.as_str());
                None
            }
        };
        if let Some(path) = path
            && seen.insert(path.clone())
        {
            locations.push(ToolCallLocation::new(path));
        }
    }

    if locations.is_empty() {
        for command in unknown_commands {
            if let Some(path) = obvious_shell_command_path(cwd, command)
                && seen.insert(path.clone())
            {
                locations.push(ToolCallLocation::new(path));
            }
        }
    }

    locations
}

fn action_path(cwd: &Path, path: &str) -> Option<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(resolve_command_path(cwd, Path::new(trimmed)))
}

fn resolve_command_path(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else if path == Path::new(".") {
        cwd.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn obvious_shell_command_path(cwd: &Path, command: &str) -> Option<PathBuf> {
    let inner = extract_inner_shell_command(command);
    if has_shell_control(&inner) {
        return None;
    }
    let parts = shlex::split(&inner)?;
    let program = parts.first().map(|part| command_name(part))?;
    let candidate = if is_read_like_program(&program) || is_write_like_program(&program) {
        last_path_like_arg(&parts[1..])
    } else {
        None
    }?;
    Some(resolve_command_path(cwd, Path::new(candidate)))
}

fn has_shell_control(command: &str) -> bool {
    command.contains(['|', ';', '<', '>'])
        || command.split_whitespace().any(|part| {
            matches!(
                part,
                "&&" | "||" | "&" | "2>&1" | "1>&2" | "2>" | "1>" | ">>" | "<<"
            )
        })
}

fn command_name(program: &str) -> String {
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program)
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase()
}

fn is_read_like_program(program: &str) -> bool {
    matches!(
        program,
        "cat"
            | "bat"
            | "sed"
            | "awk"
            | "head"
            | "tail"
            | "less"
            | "more"
            | "nl"
            | "type"
            | "gc"
            | "get-content"
    )
}

fn is_write_like_program(program: &str) -> bool {
    matches!(
        program,
        "touch" | "rm" | "unlink" | "mv" | "cp" | "chmod" | "chown" | "truncate"
    )
}

fn last_path_like_arg(args: &[String]) -> Option<&str> {
    args.iter()
        .rev()
        .find(|arg| is_path_like_arg(arg))
        .map(String::as_str)
}

fn is_path_like_arg(arg: &str) -> bool {
    let trimmed = arg.trim();
    !trimmed.is_empty()
        && trimmed != "-"
        && !trimmed.starts_with('-')
        && !trimmed.starts_with("http://")
        && !trimmed.starts_with("https://")
        && !trimmed.contains('*')
        && !trimmed.contains('?')
}

#[cfg(test)]
mod tests {
    use super::command_tool_locations;
    use codex_app_server_protocol::CommandAction;
    use std::path::{Path, PathBuf};

    #[test]
    fn locations_use_read_action_path() {
        let actions = vec![CommandAction::Read {
            command: "cat src/lib.rs".to_string(),
            name: "cat".to_string(),
            path: PathBuf::from("src/lib.rs"),
        }];

        let locations = command_tool_locations(Path::new("/repo"), "echo ignored", &actions);

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, PathBuf::from("/repo/src/lib.rs"));
        assert_eq!(locations[0].line, None);
    }

    #[test]
    fn locations_use_search_path_when_available() {
        let actions = vec![CommandAction::Search {
            command: "rg plan src/thread".to_string(),
            query: Some("plan".to_string()),
            path: Some("src/thread".to_string()),
        }];

        let locations = command_tool_locations(Path::new("/repo"), "rg plan src/thread", &actions);

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, PathBuf::from("/repo/src/thread"));
    }

    #[test]
    fn locations_fall_back_to_obvious_shell_read_path() {
        let locations = command_tool_locations(
            Path::new("/repo"),
            "/bin/bash -lc 'sed -n 1,40p src/thread.rs'",
            &[],
        );

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, PathBuf::from("/repo/src/thread.rs"));
    }

    #[test]
    fn locations_keep_cwd_for_compound_shell_commands() {
        let locations = command_tool_locations(
            Path::new("/repo"),
            "/bin/bash -lc 'cat src/lib.rs && cargo test'",
            &[],
        );

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, PathBuf::from("/repo"));
    }
}
