//! Эвристики определения типа shell-команды и маркеров verification.

use std::path::Path;

use agent_client_protocol::schema::ToolKind;
use codex_app_server_protocol::CommandAction;

// Определяем ToolKind для shell-команды по command_actions и fallback-эвристикам текста команды.
pub(in crate::thread) fn command_tool_kind(
    command: &str,
    command_actions: &[CommandAction],
) -> ToolKind {
    let mut has_read = false;
    let mut has_list_files = false;
    let mut has_search = false;

    for action in command_actions {
        match action {
            CommandAction::Read { .. } => has_read = true,
            CommandAction::ListFiles { .. } => has_list_files = true,
            CommandAction::Search { .. } => has_search = true,
            CommandAction::Unknown { .. } => {}
        }
    }

    if has_search || has_list_files {
        return ToolKind::Search;
    }
    if has_read {
        return ToolKind::Read;
    }

    let inner = extract_inner_shell_command(command);
    let normalized = inner.to_ascii_lowercase();
    if looks_like_search_command(&normalized) || looks_like_listing_command(&normalized) {
        return ToolKind::Search;
    }
    if looks_like_read_command(&normalized) {
        return ToolKind::Read;
    }

    // Оставляем карточки команд в общем сворачиваемом tool UI (не terminal-card),
    // чтобы пользователь мог по запросу раскрыть и посмотреть сырой command input.
    ToolKind::Think
}

pub(in crate::thread) fn command_looks_like_verification(command: &str) -> bool {
    let normalized = command.to_ascii_lowercase();
    let verification_markers = [
        "cargo test",
        "cargo clippy",
        "cargo check",
        "go test",
        "pytest",
        "dotnet test",
        "mvn test",
        "gradle test",
        "jest",
        "vitest",
        "eslint",
        "ruff check",
        "tsc",
    ];
    verification_markers
        .iter()
        .any(|marker| normalized.contains(marker))
}

pub(in crate::thread) fn extract_inner_shell_command(command: &str) -> String {
    let trimmed = command.trim();
    let Some(parts) = shlex::split(trimmed) else {
        return trimmed.to_string();
    };

    if parts.len() >= 3
        && is_shell_executable(&parts[0])
        && shell_command_arg_index(&parts).is_some()
    {
        let command_index = shell_command_arg_index(&parts).unwrap_or(2);
        return parts[command_index].trim().to_string();
    }

    trimmed.to_string()
}

fn is_shell_executable(program: &str) -> bool {
    let binary = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    let binary = binary
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(binary)
        .to_ascii_lowercase();
    matches!(
        binary.as_str(),
        "bash"
            | "sh"
            | "zsh"
            | "fish"
            | "cmd"
            | "cmd.exe"
            | "powershell"
            | "powershell.exe"
            | "pwsh"
            | "pwsh.exe"
    ) || binary.ends_with("cmd.exe")
        || binary.ends_with("powershell.exe")
        || binary.ends_with("pwsh.exe")
}

fn shell_command_arg_index(parts: &[String]) -> Option<usize> {
    if parts.len() < 3 {
        return None;
    }

    let shell = Path::new(&parts[0])
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&parts[0])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(&parts[0])
        .to_ascii_lowercase();

    if matches!(shell.as_str(), "cmd" | "cmd.exe") || shell.ends_with("cmd.exe") {
        return parts
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, part)| matches!(part.to_ascii_lowercase().as_str(), "/c" | "/k"))
            .map(|(index, _)| index + 1)
            .filter(|index| *index < parts.len());
    }

    parts
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, part)| {
            matches!(
                part.to_ascii_lowercase().as_str(),
                "-c" | "-lc" | "-ic" | "-command" | "-commandwithargs"
            )
        })
        .map(|(index, _)| index + 1)
        .filter(|index| *index < parts.len())
}

fn shell_uses_command(command: &str, candidates: &[&str]) -> bool {
    command
        .split(['|', ';', '&'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .filter_map(|segment| segment.split_whitespace().next())
        .any(|token| candidates.contains(&token))
}

pub(super) fn looks_like_listing_command(command: &str) -> bool {
    command.contains("rg --files")
        || shell_uses_command(
            command,
            &[
                "ls",
                "tree",
                "eza",
                "exa",
                "fd",
                "find",
                "dir",
                "gci",
                "get-childitem",
            ],
        )
        || (shell_uses_command(command, &["pwd"]) && command.contains("&&"))
}

pub(super) fn looks_like_search_command(command: &str) -> bool {
    !command.contains("rg --files")
        && shell_uses_command(
            command,
            &[
                "rg",
                "ripgrep",
                "grep",
                "ack",
                "ag",
                "findstr",
                "select-string",
                "sls",
            ],
        )
}

pub(super) fn looks_like_read_command(command: &str) -> bool {
    shell_uses_command(
        command,
        &[
            "cat",
            "bat",
            "sed",
            "awk",
            "head",
            "tail",
            "less",
            "more",
            "nl",
            "type",
            "gc",
            "get-content",
        ],
    )
}

pub(super) fn looks_like_git_inspection_command(command: &str) -> bool {
    if !shell_uses_command(command, &["git"]) {
        return false;
    }
    command.contains("git status")
        || command.contains("git diff")
        || command.contains("git show")
        || command.contains("git log")
        || command.contains("git branch")
}
