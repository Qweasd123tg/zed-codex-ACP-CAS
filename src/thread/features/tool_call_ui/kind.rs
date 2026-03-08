//! Heuristics for shell command kind detection and verification markers.

use std::path::Path;

use agent_client_protocol::ToolKind;
use codex_app_server_protocol::CommandAction;

// Determine ToolKind from command actions first, then fall back to shell-text heuristics.
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

    // Keep command cards in the generic collapsible tool UI, not a terminal card,
    // so the user can expand them on demand and inspect raw command input.
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

pub(super) fn extract_inner_shell_command(command: &str) -> String {
    let trimmed = command.trim();
    let Some(parts) = shlex::split(trimmed) else {
        return trimmed.to_string();
    };

    if parts.len() >= 3
        && is_shell_executable(&parts[0])
        && matches!(parts[1].as_str(), "-c" | "-lc" | "-ic")
    {
        return parts[2].trim().to_string();
    }

    trimmed.to_string()
}

fn is_shell_executable(program: &str) -> bool {
    let binary = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    matches!(binary, "bash" | "sh" | "zsh" | "fish")
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
        || shell_uses_command(command, &["ls", "tree", "eza", "exa", "fd", "find"])
        || (shell_uses_command(command, &["pwd"]) && command.contains("&&"))
}

pub(super) fn looks_like_search_command(command: &str) -> bool {
    !command.contains("rg --files")
        && shell_uses_command(command, &["rg", "ripgrep", "grep", "ack", "ag"])
}

pub(super) fn looks_like_read_command(command: &str) -> bool {
    shell_uses_command(
        command,
        &[
            "cat", "bat", "sed", "awk", "head", "tail", "less", "more", "nl",
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
