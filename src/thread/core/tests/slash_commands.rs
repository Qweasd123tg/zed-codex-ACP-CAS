use super::*;

#[test]
// Проверяем, что /threads парсится как управляющая команда, а не обычный текст промпта.
fn parses_threads_command() {
    let prompt: Vec<ContentBlock> = vec!["/threads".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Threads)
    );
}

#[test]
fn parses_resume_command_with_thread_id() {
    let prompt: Vec<ContentBlock> = vec!["/resume thread_123".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Resume {
            thread_id: Some("thread_123".to_string()),
            include_history: true,
        })
    );
}

#[test]
fn parses_resume_command_without_thread_id() {
    let prompt: Vec<ContentBlock> = vec!["/resume".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Resume {
            thread_id: None,
            include_history: true,
        })
    );
}

#[test]
fn parses_resume_command_with_partial_query() {
    let prompt: Vec<ContentBlock> = vec!["/resume 019c6455".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Resume {
            thread_id: Some("019c6455".to_string()),
            include_history: true,
        })
    );
}

#[test]
fn parses_resume_command_without_space_before_query() {
    let prompt: Vec<ContentBlock> = vec!["/resumeпривет".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Resume {
            thread_id: Some("привет".to_string()),
            include_history: true,
        })
    );
}

#[test]
fn parses_resume_command_with_additional_resource_blocks() {
    let prompt: Vec<ContentBlock> = vec![
        "/resume".into(),
        ContentBlock::ResourceLink(ResourceLink::new("ctx", "file:///tmp/ctx.md")),
    ];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Resume {
            thread_id: None,
            include_history: true,
        })
    );
}

#[test]
fn parses_command_when_first_text_block_is_command() {
    let prompt: Vec<ContentBlock> = vec!["/resume".into(), "thread-123".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Resume {
            thread_id: None,
            include_history: true,
        })
    );
}

#[test]
fn parses_resume_command_with_history_flag() {
    let prompt: Vec<ContentBlock> = vec!["/resume --history".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Resume {
            thread_id: None,
            include_history: true,
        })
    );
}

#[test]
fn parses_resume_command_with_query_and_history_flag() {
    let prompt: Vec<ContentBlock> = vec!["/resume 019c6455 --history".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Resume {
            thread_id: Some("019c6455".to_string()),
            include_history: true,
        })
    );
}

#[test]
fn parses_resume_command_with_no_history_flag() {
    let prompt: Vec<ContentBlock> = vec!["/resume --no-history".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Resume {
            thread_id: None,
            include_history: false,
        })
    );
}

#[test]
fn ignores_command_when_first_text_block_is_not_command() {
    let prompt: Vec<ContentBlock> = vec!["continue".into(), "/resume".into()];
    assert_eq!(parse_session_command(&prompt), None);
}

#[test]
fn ignores_regular_prompt_text() {
    let prompt: Vec<ContentBlock> = vec!["continue this task".into()];
    assert_eq!(parse_session_command(&prompt), None);
}

#[test]
fn parses_compact_command() {
    let prompt: Vec<ContentBlock> = vec!["/compact".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Compact)
    );
}

#[test]
fn parses_undo_command_with_optional_count() {
    let prompt: Vec<ContentBlock> = vec!["/undo 2".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Undo { num_turns: 2 })
    );
}

#[test]
fn parses_rename_command_with_name() {
    let prompt: Vec<ContentBlock> = vec!["/rename My current thread".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Rename {
            name: Some("My current thread".to_string()),
        })
    );
}

#[test]
fn parses_rename_command_without_name() {
    let prompt: Vec<ContentBlock> = vec!["/rename".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Rename { name: None })
    );
}

#[test]
fn parses_diff_command_without_args() {
    let prompt: Vec<ContentBlock> = vec!["/diff".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Diff {
            scope: DiffScope::LastTurn,
            paths: Vec::new(),
        })
    );
}

#[test]
fn parses_diff_command_with_session_flag() {
    let prompt: Vec<ContentBlock> = vec!["/diff --session".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Diff {
            scope: DiffScope::Session,
            paths: Vec::new(),
        })
    );
}

#[test]
fn parses_diff_command_with_last_n_flag() {
    let prompt: Vec<ContentBlock> = vec!["/diff --last 3".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Diff {
            scope: DiffScope::LastN(3),
            paths: Vec::new(),
        })
    );
}

#[test]
fn parses_diff_command_last_one_collapses_to_last_turn() {
    let prompt: Vec<ContentBlock> = vec!["/diff --last 1".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Diff {
            scope: DiffScope::LastTurn,
            paths: Vec::new(),
        })
    );
}

#[test]
fn parses_diff_command_with_path_filter() {
    let prompt: Vec<ContentBlock> = vec!["/diff src/lib.rs".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Diff {
            scope: DiffScope::LastTurn,
            paths: vec!["src/lib.rs".to_string()],
        })
    );
}

#[test]
fn parses_diff_command_with_session_flag_and_paths() {
    let prompt: Vec<ContentBlock> = vec!["/diff --session src/lib.rs src/main.rs".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Diff {
            scope: DiffScope::Session,
            paths: vec!["src/lib.rs".to_string(), "src/main.rs".to_string()],
        })
    );
}

#[test]
fn no_longer_parses_new_command() {
    let prompt: Vec<ContentBlock> = vec!["/new".into()];
    assert_eq!(parse_session_command(&prompt), None);
}

#[test]
fn no_longer_parses_delete_command() {
    let prompt: Vec<ContentBlock> = vec!["/delete 019d-test".into()];
    assert_eq!(parse_session_command(&prompt), None);
}

#[test]
fn parses_fork_command_without_args() {
    let prompt: Vec<ContentBlock> = vec!["/fork".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Fork { args: None })
    );
}

#[test]
fn parses_fork_command_with_args_for_usage_handling() {
    let prompt: Vec<ContentBlock> = vec!["/fork now".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Fork {
            args: Some("now".to_string()),
        })
    );
}

#[test]
fn parses_init_command_without_args() {
    let prompt: Vec<ContentBlock> = vec!["/init".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Init { args: None })
    );
}

#[test]
fn parses_init_command_with_args_for_usage_handling() {
    let prompt: Vec<ContentBlock> = vec!["/init please".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Init {
            args: Some("please".to_string()),
        })
    );
}

#[test]
fn does_not_parse_init_prefix_as_command() {
    let prompt: Vec<ContentBlock> = vec!["/initiate".into()];
    assert_eq!(parse_session_command(&prompt), None);
}

#[test]
fn parses_status_command_without_args() {
    let prompt: Vec<ContentBlock> = vec!["/status".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Status { args: None })
    );
}

#[test]
fn parses_status_command_with_args_for_usage_handling() {
    let prompt: Vec<ContentBlock> = vec!["/status verbose".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Status {
            args: Some("verbose".to_string()),
        })
    );
}

#[test]
fn parses_review_command_without_instructions() {
    let prompt: Vec<ContentBlock> = vec!["/review".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Review { instructions: None })
    );
}

#[test]
fn parses_review_command_with_custom_instructions() {
    let prompt: Vec<ContentBlock> = vec!["/review focus on migrations".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Review {
            instructions: Some("focus on migrations".to_string()),
        })
    );
}

#[test]
fn parses_archive_command_with_query() {
    let prompt: Vec<ContentBlock> = vec!["/archive 019d-test".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Archive {
            thread_id: Some("019d-test".to_string()),
        })
    );
}

#[test]
fn parses_unarchive_command_with_query() {
    let prompt: Vec<ContentBlock> = vec!["/unarchive old-thread".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::Unarchive {
            thread_id: Some("old-thread".to_string()),
        })
    );
}

#[test]
fn parses_plan_command_without_value() {
    let prompt: Vec<ContentBlock> = vec!["/plan".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::PlanMode {
            raw_value: None,
            mode: None,
        })
    );
}

#[test]
fn parses_plan_command_with_on_value() {
    let prompt: Vec<ContentBlock> = vec!["/plan on".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::PlanMode {
            raw_value: Some("on".to_string()),
            mode: Some(ModeKind::Plan),
        })
    );
}

#[test]
fn parse_collaboration_mode_accepts_only_current_mode_values() {
    assert_eq!(parse_collaboration_mode("on"), Some(ModeKind::Plan));
    assert_eq!(parse_collaboration_mode("plan"), Some(ModeKind::Plan));
    assert_eq!(parse_collaboration_mode("off"), Some(ModeKind::Default));
    assert_eq!(parse_collaboration_mode("chat"), None);
    assert_eq!(parse_collaboration_mode("default"), None);
    assert_eq!(parse_collaboration_mode("code"), None);
}

#[test]
fn parses_plan_command_with_prompt() {
    let prompt: Vec<ContentBlock> = vec!["/plan разбей задачу на шаги".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::PlanPrompt {
            prompt: "разбей задачу на шаги".to_string(),
        })
    );
}

#[test]
fn parses_plan_command_with_unknown_single_word_as_prompt() {
    let prompt: Vec<ContentBlock> = vec!["/plan maybe".into()];
    assert_eq!(
        parse_session_command(&prompt),
        Some(SessionCommand::PlanPrompt {
            prompt: "maybe".to_string(),
        })
    );
}

#[test]
fn builtin_commands_include_review_and_fork() {
    let names = builtin_commands(&SlashCommandPreferences::default())
        .into_iter()
        .map(|command| command.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"init".to_string()));
    assert!(names.contains(&"status".to_string()));
    assert!(names.contains(&"review".to_string()));
    assert!(names.contains(&"fork".to_string()));
}

#[test]
fn builtin_commands_honor_slash_command_preferences() {
    let slash_commands =
        SlashCommandPreferences::from_commands(vec!["unarchive".to_string(), "status".to_string()]);
    let names = builtin_commands(&slash_commands)
        .into_iter()
        .map(|command| command.name)
        .collect::<Vec<_>>();

    assert_eq!(names[0], "unarchive");
    assert_eq!(names[1], "status");
    assert!(!names.contains(&"review".to_string()));
    assert!(!names.contains(&"archive".to_string()));
}
