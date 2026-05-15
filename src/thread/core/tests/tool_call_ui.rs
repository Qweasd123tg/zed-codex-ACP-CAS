use super::*;

#[test]
fn detects_verification_commands() {
    assert!(command_looks_like_verification("cargo test -q"));
    assert!(command_looks_like_verification("go test ./..."));
    assert!(command_looks_like_verification("ruff check ."));
    assert!(!command_looks_like_verification("rg --files"));
    assert!(!command_looks_like_verification("cat README.md"));
}

#[test]
fn command_title_uses_parsed_actions_when_available() {
    let actions = vec![CommandAction::ListFiles {
        command: "rg --files".to_string(),
        path: None,
    }];
    assert_eq!(
        command_tool_title("/bin/bash -lc 'echo hello'", &actions),
        "Analyze folder contents"
    );
}

#[test]
fn command_title_reads_single_file_name_from_action() {
    let actions = vec![CommandAction::Read {
        command: "cat src/thread.rs".to_string(),
        name: "cat".to_string(),
        path: PathBuf::from("src/thread.rs"),
    }];
    assert_eq!(
        command_tool_title("cat src/thread.rs", &actions),
        "Read thread.rs"
    );
}

#[test]
fn command_title_maps_common_shell_listing_commands() {
    assert_eq!(
        command_tool_title("/bin/bash -lc 'pwd && ls -la'", &[]),
        "Analyze folder contents"
    );
    assert_eq!(
        command_tool_title("/bin/bash -lc 'rg --files | head -n 200'", &[]),
        "Analyze folder contents"
    );
    assert_eq!(
        command_tool_title(r#"C:\Windows\System32\cmd.exe /d /s /c "dir""#, &[]),
        "Analyze folder contents"
    );
}

#[test]
fn command_title_maps_common_shell_search_and_check_commands() {
    assert_eq!(
        command_tool_title("/bin/bash -lc 'rg \"plan\" src/thread.rs'", &[]),
        "Search in workspace"
    );
    assert_eq!(
        command_tool_title("/bin/bash -lc 'cargo test -q'", &[]),
        "Run tests and checks"
    );
    assert_eq!(
        command_tool_title(r#"pwsh.exe -NoProfile -Command "cargo test -q""#, &[]),
        "Run tests and checks"
    );
}

#[test]
fn command_title_maps_common_network_commands() {
    assert_eq!(
        command_tool_title("/bin/bash -lc 'curl -I https://example.com'", &[]),
        "Network access: example.com"
    );
    assert_eq!(
        command_tool_title(
            r#"pwsh.exe -NoProfile -Command "iwr https://example.com/path""#,
            &[]
        ),
        "Network access: example.com"
    );
}

#[test]
fn command_title_maps_common_file_operation_commands() {
    assert_eq!(
        command_tool_title(
            "/bin/bash -lc 'touch /home/qweasd123tg/codex-acp-outside-workspace-test.txt'",
            &[]
        ),
        "Create file: codex-acp-outside-workspace-test.txt"
    );
    assert_eq!(
        command_tool_title("/bin/bash -lc 'mkdir -p /tmp/codex-acp-test-dir'", &[]),
        "Create folder: codex-acp-test-dir"
    );
    assert_eq!(
        command_tool_title(
            "/bin/bash -lc 'rm -f /tmp/codex-acp-test-dir/file.txt'",
            &[]
        ),
        "Delete path: file.txt"
    );
}

#[test]
fn command_title_falls_back_for_unknown_commands() {
    assert_eq!(
        command_tool_title("/bin/bash -lc 'echo done'", &[]),
        "Run shell command"
    );
}

#[test]
fn command_tool_kind_uses_search_for_listing_and_grep_commands() {
    assert_eq!(
        command_tool_kind("/bin/bash -lc 'pwd && ls -la'", &[]),
        ToolKind::Search
    );
    assert_eq!(
        command_tool_kind("/bin/bash -lc 'rg \"plan\" src/thread.rs'", &[]),
        ToolKind::Search
    );
    assert_eq!(
        command_tool_kind(r#"powershell.exe -NoProfile -Command "rg plan src""#, &[]),
        ToolKind::Search
    );
}

#[test]
fn command_tool_kind_uses_read_for_file_reads() {
    let actions = vec![CommandAction::Read {
        command: "cat src/thread.rs".to_string(),
        name: "cat".to_string(),
        path: PathBuf::from("src/thread.rs"),
    }];
    assert_eq!(
        command_tool_kind("cat src/thread.rs", &actions),
        ToolKind::Read
    );
}

#[test]
fn command_tool_kind_falls_back_to_think_for_other_shell_commands() {
    assert_eq!(
        command_tool_kind("/bin/bash -lc 'echo done'", &[]),
        ToolKind::Think
    );
}
