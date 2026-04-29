//! Тесты модуля Thread для парсинга slash-команд, UI-форматирования и логики маппинга.

use super::features::approvals::user_input::build_request_user_input_permission_options;
use super::features::collab::CollabAgentLabel;
use super::features::collab::content::{
    collab_tool_content, collab_tool_raw_input, collab_tool_raw_output, format_collab_receivers,
};
use super::features::collab::render::{collab_tool_title, collab_tool_title_with_context};
use super::features::collab::status::{
    collab_agent_state_summary, collab_status_summary_line, map_collab_status,
};
use super::features::file::changes::{
    file_change_to_replay_diff, file_change_tool_location, should_prompt_file_change_approval,
};
use super::features::plan::{
    collaboration_mode_for_turn, fallback_plan_can_enter_summarizing,
    fallback_plan_entries_for_steps, fallback_plan_should_advance, limit_plan_entries,
    parse_collaboration_mode, plan_entries_all_pending, plan_from_plan_item_text, plan_from_text,
    promote_first_pending_step, should_clear_visible_plan_for_mode_change,
};
use super::features::tool_call_ui::kind::{command_looks_like_verification, command_tool_kind};
use super::features::tool_call_ui::title::command_tool_title;
use super::prompt_commands::{builtin_commands, parse_session_command};
use super::session_config::{
    current_permission_mode_id, mode_state, parse_reasoning_effort, permission_modes,
    policy_to_mode, to_app_approval, to_app_sandbox_mode,
};
use super::turn_diff::parse_turn_unified_diff_files;
use super::unified_diff::{apply_unified_diff_to_text, unified_diff_to_old_new};
use super::{
    APPROVAL_PRESETS, AUTO_ASK_EDITS_MODE_ID, AUTO_MODE_ID, DEFAULT_SESSION_MODE_ID, DiffScope,
    EditApprovalMode, FallbackPlanPhase, FallbackPlanState, MAX_VISIBLE_PLAN_ENTRIES,
    NONE_OF_THE_ABOVE, PLAN_SESSION_MODE_ID, SessionCommand,
};
use agent_client_protocol::schema::{
    Content, ContentBlock, PermissionOptionKind, Plan, PlanEntry, PlanEntryPriority,
    PlanEntryStatus, ResourceLink, ToolCallContent, ToolCallStatus, ToolKind,
};
use codex_app_server_protocol::{
    CollabAgentState, CollabAgentStatus, CollabAgentTool, CollabAgentToolCallStatus, CommandAction,
    PatchChangeKind, ReadOnlyAccess as AppReadOnlyAccess, SandboxMode as AppSandboxMode,
    SandboxPolicy as AppSandboxPolicy, ToolRequestUserInputQuestion,
};
use codex_protocol::config_types::ModeKind;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::{ReadOnlyAccess, SandboxPolicy};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
fn parses_delete_command_as_archive_alias() {
    let prompt: Vec<ContentBlock> = vec!["/delete 019d-test".into()];
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
fn parse_collaboration_mode_accepts_chat_alias() {
    assert_eq!(parse_collaboration_mode("chat"), Some(ModeKind::Default));
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
    let names = builtin_commands()
        .into_iter()
        .map(|command| command.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"init".to_string()));
    assert!(names.contains(&"status".to_string()));
    assert!(names.contains(&"review".to_string()));
    assert!(names.contains(&"fork".to_string()));
}

#[test]
fn parses_plan_entries_from_markdown_lines() {
    let plan = plan_from_text(
            "# Plan\n- [x] done\n- [ ] pending\n- [~] running\n- bullet\n1. numbered\n2) alternate\nplain text",
        )
        .expect("expected plan entries");

    assert_eq!(plan.entries.len(), 6);
    assert_eq!(plan.entries[0].content, "done");
    assert_eq!(plan.entries[0].status, PlanEntryStatus::Completed);
    assert_eq!(plan.entries[1].content, "pending");
    assert_eq!(plan.entries[1].status, PlanEntryStatus::Pending);
    assert_eq!(plan.entries[2].content, "running");
    assert_eq!(plan.entries[2].status, PlanEntryStatus::InProgress);
    assert_eq!(plan.entries[3].content, "bullet");
    assert_eq!(plan.entries[3].status, PlanEntryStatus::Pending);
    assert_eq!(plan.entries[4].content, "numbered");
    assert_eq!(plan.entries[4].status, PlanEntryStatus::Pending);
    assert_eq!(plan.entries[5].content, "alternate");
    assert_eq!(plan.entries[5].status, PlanEntryStatus::Pending);
}

#[test]
fn parses_plain_proposed_plan_block() {
    let plan = plan_from_text("# Final plan\n- first\n- second\n")
        .expect("expected proposed plan entries");

    assert_eq!(plan.entries.len(), 2);
    assert_eq!(plan.entries[0].content, "first");
    assert_eq!(plan.entries[0].status, PlanEntryStatus::Pending);
    assert_eq!(plan.entries[1].content, "second");
    assert_eq!(plan.entries[1].status, PlanEntryStatus::Pending);
}

#[test]
fn does_not_parse_list_without_plan_markers() {
    let plan = plan_from_text("1. first\n2. second\n");

    assert!(plan.is_none());
}

#[test]
fn parses_plan_with_intro_line() {
    let plan = plan_from_text("Implementation plan:\n- first\n- second\n")
        .expect("expected plan entries after intro line");

    assert_eq!(plan.entries.len(), 2);
    assert_eq!(plan.entries[0].content, "first");
    assert_eq!(plan.entries[1].content, "second");
}

#[test]
fn does_not_parse_request_user_input_options_as_plan() {
    let plan = plan_from_text("Question?\nOptions:\n1. small (Recommended)\n2. medium\n3. large\n");

    assert!(plan.is_none());
}

#[test]
fn does_not_parse_numbered_poem_as_plan() {
    let plan = plan_from_text(
        "1. На крышах звенят осторожные капли.\n2. Двор просыпается раньше фонарей.\n3. Ветер еще помнит февральскую строгость.\n",
    );

    assert!(plan.is_none());
}

#[test]
fn parses_list_only_plan_from_plan_item_text() {
    let plan =
        plan_from_plan_item_text("- Step 1\n- Step 2\n").expect("expected plan-item list parse");

    assert_eq!(plan.entries.len(), 2);
    assert_eq!(plan.entries[0].content, "Step 1");
    assert_eq!(plan.entries[1].content, "Step 2");
}

#[test]
fn does_not_parse_request_user_input_like_plan_item_text() {
    let plan = plan_from_plan_item_text("Question?\nOptions:\n1. First\n2. Second\n3. Third\n");

    assert!(plan.is_none());
}

#[test]
fn does_not_parse_numbered_list_only_plan_item_text() {
    let plan = plan_from_plan_item_text("1. first\n2. second\n3. third\n");

    assert!(plan.is_none());
}

#[test]
fn plan_item_prefers_steps_section_over_other_lists() {
    let plan = plan_from_plan_item_text(
        "## Цель и критерии готовности\n1. Готово ровно 3 стихотворения.\n2. В каждом стихотворении ровно 20 строк.\n\n## Пошаговая реализация\n1. Собрать словарь образов.\n2. Написать черновики.\n3. Сжать до финального объема.\n\n## Риски\n1. Ошибка подсчета строк.\n",
    )
    .expect("expected plan entries from steps section");

    assert_eq!(plan.entries.len(), 3);
    assert_eq!(plan.entries[0].content, "Собрать словарь образов.");
    assert_eq!(plan.entries[1].content, "Написать черновики.");
    assert_eq!(plan.entries[2].content, "Сжать до финального объема.");
}

#[test]
fn does_not_parse_sectioned_plan_item_without_steps_section() {
    let plan = plan_from_plan_item_text(
        "# План\n## Зафиксированные параметры\n1. Длина: 4 строки.\n2. Язык: русский.\n3. Рифма: обязательна.\n",
    );

    assert!(plan.is_none());
}

#[test]
fn parses_steps_section_with_etapy_heading() {
    let plan = plan_from_plan_item_text(
        "## Цель\n1. Подготовить результат.\n\n## Этапы реализации\n1. Собрать требования.\n2. Внести изменения.\n3. Проверить поведение.\n",
    )
    .expect("expected plan entries from stages heading");

    assert_eq!(plan.entries.len(), 3);
    assert_eq!(plan.entries[0].content, "Собрать требования.");
    assert_eq!(plan.entries[1].content, "Внести изменения.");
    assert_eq!(plan.entries[2].content, "Проверить поведение.");
}

#[test]
fn limits_large_plans_for_ui() {
    let entries = (1..=12)
        .map(|index| {
            PlanEntry::new(
                format!("step {index}"),
                PlanEntryPriority::Medium,
                PlanEntryStatus::Pending,
            )
        })
        .collect::<Vec<_>>();

    let limited = limit_plan_entries(entries);
    assert_eq!(limited.len(), MAX_VISIBLE_PLAN_ENTRIES);
    assert_eq!(limited[0].content, "step 1");
    assert_eq!(
        limited.last().map(|entry| entry.content.clone()),
        Some("step 6".to_string())
    );
    assert!(
        limited
            .iter()
            .all(|entry| entry.status == PlanEntryStatus::Pending)
    );
}

#[test]
fn fallback_plan_entries_track_phase_progression() {
    let planning = fallback_plan_entries_for_steps(FallbackPlanPhase::Planning, &[]);
    let implementing = fallback_plan_entries_for_steps(FallbackPlanPhase::Implementing, &[]);
    let done = fallback_plan_entries_for_steps(FallbackPlanPhase::Done, &[]);

    assert_eq!(planning.len(), 4);
    assert_eq!(planning[0].status, PlanEntryStatus::InProgress);
    assert_eq!(planning[1].status, PlanEntryStatus::Pending);

    assert_eq!(implementing[0].status, PlanEntryStatus::Completed);
    assert_eq!(implementing[1].status, PlanEntryStatus::InProgress);
    assert_eq!(implementing[2].status, PlanEntryStatus::Pending);

    assert!(
        done.iter()
            .all(|entry| entry.status == PlanEntryStatus::Completed)
    );
}

#[test]
fn promote_first_pending_step_marks_only_first_step_in_progress() {
    let plan = Plan::new(vec![
        PlanEntry::new(
            "step 1",
            PlanEntryPriority::Medium,
            PlanEntryStatus::Pending,
        ),
        PlanEntry::new(
            "step 2",
            PlanEntryPriority::Medium,
            PlanEntryStatus::Pending,
        ),
        PlanEntry::new(
            "step 3",
            PlanEntryPriority::Medium,
            PlanEntryStatus::Pending,
        ),
    ]);

    let promoted = promote_first_pending_step(plan);
    assert_eq!(promoted.entries[0].status, PlanEntryStatus::InProgress);
    assert_eq!(promoted.entries[1].status, PlanEntryStatus::Pending);
    assert_eq!(promoted.entries[2].status, PlanEntryStatus::Pending);
}

#[test]
fn promote_first_pending_step_preserves_existing_statuses() {
    let plan = Plan::new(vec![
        PlanEntry::new(
            "step 1",
            PlanEntryPriority::Medium,
            PlanEntryStatus::Completed,
        ),
        PlanEntry::new(
            "step 2",
            PlanEntryPriority::Medium,
            PlanEntryStatus::InProgress,
        ),
        PlanEntry::new(
            "step 3",
            PlanEntryPriority::Medium,
            PlanEntryStatus::Pending,
        ),
    ]);

    let promoted = promote_first_pending_step(plan.clone());
    assert_eq!(promoted.entries, plan.entries);
}

#[test]
fn fallback_plan_can_enter_summarizing_only_after_tool_activity_and_no_active_calls() {
    let state = FallbackPlanState {
        turn_id: "turn_1".to_string(),
        phase: FallbackPlanPhase::Verifying,
        saw_tool_activity: true,
        steps: vec![],
    };
    assert!(fallback_plan_can_enter_summarizing(
        Some(&state),
        "turn_1",
        false
    ));
    assert!(!fallback_plan_can_enter_summarizing(
        Some(&state),
        "turn_1",
        true
    ));
}

#[test]
fn plan_entries_all_pending_detects_mixed_statuses() {
    let all_pending = vec![
        PlanEntry::new("a", PlanEntryPriority::Medium, PlanEntryStatus::Pending),
        PlanEntry::new("b", PlanEntryPriority::Medium, PlanEntryStatus::Pending),
    ];
    let mixed = vec![
        PlanEntry::new("a", PlanEntryPriority::Medium, PlanEntryStatus::InProgress),
        PlanEntry::new("b", PlanEntryPriority::Medium, PlanEntryStatus::Pending),
    ];
    assert!(plan_entries_all_pending(&all_pending));
    assert!(!plan_entries_all_pending(&mixed));
}

#[test]
fn fallback_plan_can_advance_to_done_without_tool_activity() {
    let state = FallbackPlanState {
        turn_id: "turn_1".to_string(),
        phase: FallbackPlanPhase::Planning,
        saw_tool_activity: false,
        steps: vec![],
    };
    assert!(fallback_plan_should_advance(
        &state,
        FallbackPlanPhase::Done
    ));
}

#[test]
fn fallback_plan_can_advance_to_done_after_tool_activity() {
    let state = FallbackPlanState {
        turn_id: "turn_1".to_string(),
        phase: FallbackPlanPhase::Summarizing,
        saw_tool_activity: true,
        steps: vec![],
    };
    assert!(fallback_plan_should_advance(
        &state,
        FallbackPlanPhase::Done
    ));
}

#[test]
fn fallback_plan_cannot_enter_summarizing_without_tool_activity() {
    let state = FallbackPlanState {
        turn_id: "turn_1".to_string(),
        phase: FallbackPlanPhase::Planning,
        saw_tool_activity: false,
        steps: vec![],
    };
    assert!(!fallback_plan_can_enter_summarizing(
        Some(&state),
        "turn_1",
        false
    ));
}

#[test]
fn fallback_plan_distributes_progress_for_longer_step_lists() {
    let steps = vec![
        "step 1".to_string(),
        "step 2".to_string(),
        "step 3".to_string(),
        "step 4".to_string(),
        "step 5".to_string(),
        "step 6".to_string(),
    ];

    let implementing = fallback_plan_entries_for_steps(FallbackPlanPhase::Implementing, &steps);
    let verifying = fallback_plan_entries_for_steps(FallbackPlanPhase::Verifying, &steps);
    let summarizing = fallback_plan_entries_for_steps(FallbackPlanPhase::Summarizing, &steps);

    assert_eq!(implementing[0].status, PlanEntryStatus::Completed);
    assert_eq!(implementing[1].status, PlanEntryStatus::InProgress);
    assert_eq!(implementing[2].status, PlanEntryStatus::Pending);

    assert_eq!(verifying[2].status, PlanEntryStatus::Completed);
    assert_eq!(verifying[3].status, PlanEntryStatus::InProgress);
    assert_eq!(verifying[4].status, PlanEntryStatus::Pending);

    assert_eq!(summarizing[4].status, PlanEntryStatus::Completed);
    assert_eq!(summarizing[5].status, PlanEntryStatus::InProgress);
}

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

#[test]
fn collab_tool_titles_are_human_readable() {
    assert_eq!(
        collab_tool_title(&CollabAgentTool::SpawnAgent, false),
        "Spawn agent"
    );
    assert_eq!(
        collab_tool_title(&CollabAgentTool::SpawnAgent, true),
        "Agent spawned"
    );
    assert_eq!(
        collab_tool_title(&CollabAgentTool::Wait, false),
        "Waiting for agents"
    );
    assert_eq!(
        collab_tool_title(&CollabAgentTool::Wait, true),
        "Wait complete"
    );
    assert_eq!(
        collab_tool_title(&CollabAgentTool::SendInput, true),
        "Input sent"
    );
}

#[test]
fn collab_tool_titles_include_cached_agent_labels_when_available() {
    let mut agent_labels = HashMap::new();
    agent_labels.insert(
        "agent-1".to_string(),
        CollabAgentLabel {
            nickname: Some("Darwin".to_string()),
            role: Some("explorer".to_string()),
        },
    );

    assert_eq!(
        collab_tool_title_with_context(
            &CollabAgentTool::SpawnAgent,
            true,
            &["agent-1".to_string()],
            &agent_labels,
        ),
        "Spawned Darwin [explorer]"
    );
    assert_eq!(
        collab_tool_title_with_context(
            &CollabAgentTool::SendInput,
            false,
            &["agent-1".to_string()],
            &agent_labels,
        ),
        "Send input to Darwin [explorer]"
    );
    assert_eq!(
        collab_tool_title_with_context(
            &CollabAgentTool::Wait,
            true,
            &["agent-1".to_string()],
            &agent_labels,
        ),
        "Finished waiting for Darwin [explorer]"
    );
}

#[test]
fn collab_receivers_are_truncated_for_compact_cards() {
    let receivers = vec![
        "a".to_string(),
        "b".to_string(),
        "c".to_string(),
        "d".to_string(),
    ];
    assert_eq!(
        format_collab_receivers(&receivers),
        "a, b, c, ... (+1 more)"
    );
}

#[test]
fn collab_status_summary_is_compact_and_includes_non_zero_buckets() {
    let mut states = HashMap::new();
    states.insert(
        "thread-1".to_string(),
        CollabAgentState {
            message: None,
            status: CollabAgentStatus::Running,
        },
    );
    states.insert(
        "thread-2".to_string(),
        CollabAgentState {
            message: Some("done".to_string()),
            status: CollabAgentStatus::Completed,
        },
    );
    states.insert(
        "thread-3".to_string(),
        CollabAgentState {
            message: Some("failed".to_string()),
            status: CollabAgentStatus::Errored,
        },
    );

    assert_eq!(
        collab_status_summary_line(&states),
        "Agents: 3 total · 1 running · 1 completed · 1 errored"
    );
}

#[test]
fn collab_status_mapping_matches_tool_call_statuses() {
    assert_eq!(
        map_collab_status(CollabAgentToolCallStatus::InProgress, true),
        ToolCallStatus::InProgress
    );
    assert_eq!(
        map_collab_status(CollabAgentToolCallStatus::Completed, false),
        ToolCallStatus::Completed
    );
    assert_eq!(
        map_collab_status(CollabAgentToolCallStatus::Failed, false),
        ToolCallStatus::Failed
    );
}

#[test]
fn collab_agent_state_summary_includes_message_preview() {
    let completed = CollabAgentState {
        message: Some("Finished audit and found no blocking issues.".to_string()),
        status: CollabAgentStatus::Completed,
    };
    let errored = CollabAgentState {
        message: Some("sandbox denied network access while fetching fixtures".to_string()),
        status: CollabAgentStatus::Errored,
    };

    assert_eq!(
        collab_agent_state_summary(&completed),
        "Completed - Finished audit and found no blocking issues."
    );
    assert_eq!(
        collab_agent_state_summary(&errored),
        "Error - sandbox denied network access while fetching fixtures"
    );
}

#[test]
fn collab_tool_content_shows_agent_messages_for_single_target() {
    let mut states = HashMap::new();
    states.insert(
        "agent-1".to_string(),
        CollabAgentState {
            message: Some("Applied the patch and reran the focused tests.".to_string()),
            status: CollabAgentStatus::Completed,
        },
    );

    let content = collab_tool_content(
        &CollabAgentTool::SendInput,
        &HashMap::new(),
        "main-thread",
        &["agent-1".to_string()],
        Some("Continue with the fix and verify it."),
        &states,
        false,
    );

    assert_eq!(
        first_tool_call_text(&content),
        "Sender: main-thread\nAgent: agent-1\nStatus: Completed - Applied the patch and reran the focused tests."
    );
}

#[test]
fn collab_tool_content_shows_wait_summary_and_per_agent_details() {
    let mut states = HashMap::new();
    states.insert(
        "agent-1".to_string(),
        CollabAgentState {
            message: Some("Patched worker queue retry logic.".to_string()),
            status: CollabAgentStatus::Completed,
        },
    );
    states.insert(
        "agent-2".to_string(),
        CollabAgentState {
            message: Some("cargo test failed because fixture path was missing".to_string()),
            status: CollabAgentStatus::Errored,
        },
    );

    let content = collab_tool_content(
        &CollabAgentTool::Wait,
        &HashMap::new(),
        "main-thread",
        &["agent-1".to_string(), "agent-2".to_string()],
        None,
        &states,
        false,
    );

    assert_eq!(
        first_tool_call_text(&content),
        "Sender: main-thread\nWaiting for: agent-1, agent-2\nAgents: 2 total · 1 completed · 1 errored\n- agent-1: Completed - Patched worker queue retry logic.\n- agent-2: Error - cargo test failed because fixture path was missing"
    );
}

#[test]
fn collab_tool_content_prefers_cached_agent_labels() {
    let mut agent_labels = HashMap::new();
    agent_labels.insert(
        "agent-1".to_string(),
        CollabAgentLabel {
            nickname: Some("atlas".to_string()),
            role: Some("explorer".to_string()),
        },
    );
    let content = collab_tool_content(
        &CollabAgentTool::SendInput,
        &agent_labels,
        "main-thread",
        &["agent-1".to_string()],
        None,
        &HashMap::new(),
        false,
    );

    assert_eq!(
        first_tool_call_text(&content),
        "Sender: main-thread\nAgent: atlas [explorer] (agent-1)"
    );
}

#[test]
fn collab_tool_raw_input_uses_prompt_preview_for_spawned_agents() {
    assert_eq!(
        collab_tool_raw_input(
            &CollabAgentTool::SpawnAgent,
            &HashMap::new(),
            &[],
            Some("Please inspect the workspace and return only the missing artifacts."),
        ),
        Some(serde_json::Value::String(
            "Please inspect the workspace and return only the missing artifacts.".to_string(),
        ))
    );
}

#[test]
fn collab_tool_raw_output_is_human_readable() {
    let mut agent_labels = HashMap::new();
    agent_labels.insert(
        "agent-1".to_string(),
        CollabAgentLabel {
            nickname: Some("Volta".to_string()),
            role: Some("explorer".to_string()),
        },
    );
    let mut states = HashMap::new();
    states.insert(
        "agent-1".to_string(),
        CollabAgentState {
            message: Some("Collected 3 concrete improvements for smoke_check.py.".to_string()),
            status: CollabAgentStatus::Completed,
        },
    );

    assert_eq!(
        collab_tool_raw_output(&agent_labels, &["agent-1".to_string()], &states),
        Some(serde_json::Value::String(
            "Completed - Collected 3 concrete improvements for smoke_check.py.".to_string(),
        ))
    );
}

fn first_tool_call_text(content: &[ToolCallContent]) -> String {
    match content.first() {
        Some(ToolCallContent::Content(Content {
            content: ContentBlock::Text(text),
            ..
        })) => text.text.clone(),
        other => panic!("unexpected tool call content: {other:?}"),
    }
}

#[test]
fn parses_reasoning_effort_values() {
    assert_eq!(
        parse_reasoning_effort("medium"),
        Some(ReasoningEffort::Medium)
    );
    assert_eq!(parse_reasoning_effort("high"), Some(ReasoningEffort::High));
    assert_eq!(
        parse_reasoning_effort("xhigh"),
        Some(ReasoningEffort::XHigh)
    );
    assert_eq!(parse_reasoning_effort("invalid"), None);
}

#[test]
fn collaboration_mode_for_turn_is_explicit_for_default_mode() {
    let mode =
        collaboration_mode_for_turn(ModeKind::Default, "gpt-5.3-codex", ReasoningEffort::High)
            .expect("mode should always be explicit");

    assert_eq!(mode.mode, ModeKind::Default);
    assert_eq!(mode.settings.model, "gpt-5.3-codex");
    assert_eq!(mode.settings.reasoning_effort, Some(ReasoningEffort::High));
}

#[test]
fn collaboration_mode_for_turn_is_explicit_for_plan_mode() {
    let mode = collaboration_mode_for_turn(ModeKind::Plan, "gpt-5.3-codex", ReasoningEffort::XHigh)
        .expect("mode should always be explicit");

    assert_eq!(mode.mode, ModeKind::Plan);
    assert_eq!(mode.settings.model, "gpt-5.3-codex");
    assert_eq!(mode.settings.reasoning_effort, Some(ReasoningEffort::XHigh));
}

#[test]
fn file_change_approval_is_always_prompted_in_plan_mode() {
    assert!(should_prompt_file_change_approval(
        ModeKind::Plan,
        EditApprovalMode::AutoApprove
    ));
    assert!(should_prompt_file_change_approval(
        ModeKind::Plan,
        EditApprovalMode::AskEveryEdit
    ));
}

#[test]
fn file_change_approval_respects_edit_mode_in_default_mode() {
    assert!(!should_prompt_file_change_approval(
        ModeKind::Default,
        EditApprovalMode::AutoApprove
    ));
    assert!(should_prompt_file_change_approval(
        ModeKind::Default,
        EditApprovalMode::AskEveryEdit
    ));
}

#[test]
fn mode_state_uses_custom_auto_ask_edits_id() {
    let auto_preset = APPROVAL_PRESETS
        .iter()
        .find(|preset| preset.id == AUTO_MODE_ID)
        .expect("auto preset should exist");
    let current_mode_id = current_permission_mode_id(
        to_app_approval(auto_preset.approval),
        to_app_sandbox_mode(&auto_preset.sandbox),
        EditApprovalMode::AskEveryEdit,
    );
    assert_eq!(current_mode_id.0.as_ref(), AUTO_ASK_EDITS_MODE_ID);
}

#[test]
fn mode_state_shows_read_only_when_not_current() {
    let auto_preset = APPROVAL_PRESETS
        .iter()
        .find(|preset| preset.id == AUTO_MODE_ID)
        .expect("auto preset should exist");
    let modes = permission_modes(
        to_app_approval(auto_preset.approval),
        to_app_sandbox_mode(&auto_preset.sandbox),
        EditApprovalMode::AutoApprove,
    );

    assert!(modes.iter().any(|mode| mode.id.0.as_ref() == "read-only"));
}

#[test]
fn mode_state_keeps_read_only_when_current() {
    let read_only_preset = APPROVAL_PRESETS
        .iter()
        .find(|preset| preset.id == "read-only")
        .expect("read-only preset should exist");
    let current_mode_id = current_permission_mode_id(
        to_app_approval(read_only_preset.approval),
        to_app_sandbox_mode(&read_only_preset.sandbox),
        EditApprovalMode::AskEveryEdit,
    );
    let modes = permission_modes(
        to_app_approval(read_only_preset.approval),
        to_app_sandbox_mode(&read_only_preset.sandbox),
        EditApprovalMode::AskEveryEdit,
    );

    assert_eq!(current_mode_id.0.as_ref(), "read-only");
    assert!(modes.iter().any(|mode| mode.id.0.as_ref() == "read-only"));
}

#[test]
fn permission_modes_explain_full_access_sandbox_behavior() {
    let auto_preset = APPROVAL_PRESETS
        .iter()
        .find(|preset| preset.id == AUTO_MODE_ID)
        .expect("auto preset should exist");
    let modes = permission_modes(
        to_app_approval(auto_preset.approval),
        to_app_sandbox_mode(&auto_preset.sandbox),
        EditApprovalMode::AutoApprove,
    );

    let full_access = modes
        .iter()
        .find(|mode| mode.id.0.as_ref() == "full-access")
        .expect("full access mode should exist");
    assert_eq!(full_access.name, "Full access");
    assert_eq!(
        full_access.description.as_deref(),
        Some(
            "No sandbox and no approval prompts for edits, commands, internet, or files outside this workspace."
        )
    );
}

#[test]
fn mode_state_contains_only_default_and_plan() {
    let state = mode_state(ModeKind::Default);
    assert_eq!(state.current_mode_id.0.as_ref(), DEFAULT_SESSION_MODE_ID);
    assert_eq!(state.available_modes.len(), 2);
    assert_eq!(
        state.available_modes[0].id.0.as_ref(),
        DEFAULT_SESSION_MODE_ID
    );
    assert_eq!(state.available_modes[1].id.0.as_ref(), PLAN_SESSION_MODE_ID);
}

#[test]
fn leaving_plan_mode_clears_visible_plan_state() {
    assert!(should_clear_visible_plan_for_mode_change(
        ModeKind::Plan,
        ModeKind::Default,
        false,
    ));
    assert!(should_clear_visible_plan_for_mode_change(
        ModeKind::Default,
        ModeKind::Default,
        true,
    ));
}

#[test]
fn staying_in_plan_mode_preserves_visible_plan_state() {
    assert!(!should_clear_visible_plan_for_mode_change(
        ModeKind::Plan,
        ModeKind::Plan,
        true,
    ));
    assert!(!should_clear_visible_plan_for_mode_change(
        ModeKind::Default,
        ModeKind::Plan,
        true,
    ));
}

#[test]
fn app_sandbox_policy_from_preserves_workspace_write_settings() {
    let policy = SandboxPolicy::WorkspaceWrite {
        writable_roots: vec![],
        read_only_access: ReadOnlyAccess::FullAccess,
        network_access: true,
        exclude_tmpdir_env_var: true,
        exclude_slash_tmp: true,
    };

    assert_eq!(
        AppSandboxPolicy::from(policy),
        AppSandboxPolicy::WorkspaceWrite {
            writable_roots: vec![],
            read_only_access: AppReadOnlyAccess::FullAccess,
            network_access: true,
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        }
    );
}

#[test]
fn app_sandbox_policy_from_preserves_external_sandbox() {
    let policy = SandboxPolicy::ExternalSandbox {
        network_access: codex_protocol::protocol::NetworkAccess::Enabled,
    };

    assert_eq!(
        AppSandboxPolicy::from(policy),
        AppSandboxPolicy::ExternalSandbox {
            network_access: codex_app_server_protocol::NetworkAccess::Enabled
        }
    );
}

#[test]
fn policy_to_mode_maps_external_sandbox_to_workspace_mode() {
    let policy = AppSandboxPolicy::ExternalSandbox {
        network_access: codex_app_server_protocol::NetworkAccess::Restricted,
    };
    assert_eq!(policy_to_mode(&policy), AppSandboxMode::WorkspaceWrite);
}

#[test]
fn apply_unified_diff_to_text_reconstructs_content() {
    let old_text = "one\ntwo\nthree\n";
    let unified_diff = "\
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,4 @@
 one
-two
+TWO
 three
+four
";
    let new_text = apply_unified_diff_to_text(old_text, unified_diff)
        .expect("diff should be applicable to old content");
    assert_eq!(new_text, "one\nTWO\nthree\nfour\n");
}

#[test]
fn unified_diff_to_old_new_ignores_move_suffix() {
    let diff = "\
--- a/src/old.txt
+++ b/src/new.txt
@@ -1 +1 @@
-before
+after

Moved to: src/new.txt
";
    let (old_text, new_text) =
        unified_diff_to_old_new(diff).expect("should extract old/new hunk text");
    assert_eq!(old_text, "before\n");
    assert_eq!(new_text, "after\n");
}

#[test]
fn unified_diff_to_old_new_keeps_hunk_lines_starting_with_header_prefixes() {
    let diff = "\
--- a/src/example.txt
+++ b/src/example.txt
@@ -1 +1 @@
---- starts-with-triple-dash
++++ starts-with-triple-plus
";
    let (old_text, new_text) =
        unified_diff_to_old_new(diff).expect("should keep hunk body lines intact");
    assert_eq!(old_text, "--- starts-with-triple-dash\n");
    assert_eq!(new_text, "+++ starts-with-triple-plus\n");
}

#[test]
fn parse_turn_unified_diff_files_handles_add_update_delete() {
    let diff = "\
diff --git a/src/update.txt b/src/update.txt
--- a/src/update.txt
+++ b/src/update.txt
@@ -1 +1 @@
-old
+new
diff --git a/src/add.txt b/src/add.txt
new file mode 100644
--- /dev/null
+++ b/src/add.txt
@@ -0,0 +1 @@
+added
diff --git a/src/delete.txt b/src/delete.txt
deleted file mode 100644
--- a/src/delete.txt
+++ /dev/null
@@ -1 +0,0 @@
-removed
";

    let files = parse_turn_unified_diff_files(diff);
    assert_eq!(files.len(), 3);

    assert_eq!(files[0].path, PathBuf::from("src/update.txt"));
    assert_eq!(files[0].old_text, "old\n");
    assert_eq!(files[0].new_text, "new\n");
    assert!(!files[0].is_delete);

    assert_eq!(files[1].path, PathBuf::from("src/add.txt"));
    assert_eq!(files[1].old_text, "");
    assert_eq!(files[1].new_text, "added\n");
    assert!(!files[1].is_delete);

    assert_eq!(files[2].path, PathBuf::from("src/delete.txt"));
    assert_eq!(files[2].old_text, "removed\n");
    assert_eq!(files[2].new_text, "");
    assert!(files[2].is_delete);
}

#[test]
fn parse_turn_unified_diff_files_normalizes_quoted_paths() {
    let diff = "\
diff --git \"a/src/space file.txt\" \"b/src/space file.txt\"
--- \"a/src/space file.txt\"
+++ \"b/src/space file.txt\"
@@ -1 +1 @@
-before
+after
";

    let files = parse_turn_unified_diff_files(diff);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, PathBuf::from("src/space file.txt"));
    assert_eq!(files[0].old_text, "before\n");
    assert_eq!(files[0].new_text, "after\n");
    assert!(!files[0].is_delete);
}

#[test]
fn parse_turn_unified_diff_files_ignores_sections_without_hunks() {
    let diff = "\
diff --git a/src/example.txt b/src/example.txt
--- a/src/example.txt
+++ b/src/example.txt
";

    let files = parse_turn_unified_diff_files(diff);
    assert!(files.is_empty());
}

#[test]
fn replay_diff_for_update_uses_old_and_new_text() {
    let change = codex_app_server_protocol::FileUpdateChange {
        path: "README.md".to_string(),
        kind: PatchChangeKind::Update { move_path: None },
        diff: "\
--- a/README.md
+++ b/README.md
@@ -1 +1 @@
-hello
+world
"
        .to_string(),
    };

    let diff = file_change_to_replay_diff(Path::new("/tmp/workspace"), change);
    assert_eq!(diff.path, PathBuf::from("/tmp/workspace/README.md"));
    assert_eq!(diff.old_text.as_deref(), Some("hello\n"));
    assert_eq!(diff.new_text, "world\n");
}

#[test]
fn replay_diff_for_add_uses_unified_hunk_when_available() {
    let change = codex_app_server_protocol::FileUpdateChange {
        path: "notes.md".to_string(),
        kind: PatchChangeKind::Add,
        diff: "\
--- /dev/null
+++ b/notes.md
@@ -0,0 +1,2 @@
+line one
+line two
"
        .to_string(),
    };

    let diff = file_change_to_replay_diff(Path::new("/tmp/workspace"), change);
    assert_eq!(diff.path, PathBuf::from("/tmp/workspace/notes.md"));
    assert_eq!(diff.old_text.as_deref(), None);
    assert_eq!(diff.new_text, "line one\nline two\n");
}

#[test]
fn replay_diff_for_delete_uses_unified_hunk_when_available() {
    let change = codex_app_server_protocol::FileUpdateChange {
        path: "notes.md".to_string(),
        kind: PatchChangeKind::Delete,
        diff: "\
--- a/notes.md
+++ /dev/null
@@ -1,2 +0,0 @@
-line one
-line two
"
        .to_string(),
    };

    let diff = file_change_to_replay_diff(Path::new("/tmp/workspace"), change);
    assert_eq!(diff.path, PathBuf::from("/tmp/workspace/notes.md"));
    assert_eq!(diff.old_text.as_deref(), Some("line one\nline two\n"));
    assert_eq!(diff.new_text, "");
}

#[test]
fn file_change_tool_location_uses_move_target_and_hunk_line() {
    let change = codex_app_server_protocol::FileUpdateChange {
        path: "src/old.rs".to_string(),
        kind: PatchChangeKind::Update {
            move_path: Some(PathBuf::from("src/new.rs")),
        },
        diff: "\
--- a/src/old.rs
+++ b/src/new.rs
@@ -3,2 +8,3 @@
-old
+new
 keep
"
        .to_string(),
    };

    let location = file_change_tool_location(Path::new("/tmp/workspace"), &change);
    assert_eq!(location.path, PathBuf::from("/tmp/workspace/src/new.rs"));
    assert_eq!(location.line, Some(7));
}

#[test]
fn file_change_tool_location_defaults_to_first_line_for_non_unified_add() {
    let change = codex_app_server_protocol::FileUpdateChange {
        path: "notes.txt".to_string(),
        kind: PatchChangeKind::Add,
        diff: "hello\nworld\n".to_string(),
    };

    let location = file_change_tool_location(Path::new("/tmp/workspace"), &change);
    assert_eq!(location.path, PathBuf::from("/tmp/workspace/notes.txt"));
    assert_eq!(location.line, Some(0));
}

#[test]
fn request_user_input_options_include_none_of_the_above_when_supported() {
    let question = ToolRequestUserInputQuestion {
        id: "q1".to_string(),
        header: "Header".to_string(),
        question: "Question?".to_string(),
        is_other: true,
        is_secret: false,
        options: Some(vec![
            codex_app_server_protocol::ToolRequestUserInputOption {
                label: "Yes".to_string(),
                description: "Continue".to_string(),
            },
        ]),
    };

    let (options, answer_labels_by_option_id, _) =
        build_request_user_input_permission_options(0, &question);

    assert_eq!(options.len(), 2);
    assert_eq!(answer_labels_by_option_id.len(), 2);
    assert_eq!(options[0].kind, PermissionOptionKind::AllowOnce);
    assert_eq!(options[1].kind, PermissionOptionKind::AllowOnce);
    assert!(
        answer_labels_by_option_id
            .values()
            .any(|label| label == "Yes")
    );
    assert!(
        answer_labels_by_option_id
            .values()
            .any(|label| label == NONE_OF_THE_ABOVE)
    );
}

#[test]
fn request_user_input_options_do_not_add_none_of_the_above_without_base_options() {
    let question = ToolRequestUserInputQuestion {
        id: "q1".to_string(),
        header: "Header".to_string(),
        question: "Question?".to_string(),
        is_other: true,
        is_secret: false,
        options: None,
    };

    let (options, answer_labels_by_option_id, _) =
        build_request_user_input_permission_options(0, &question);

    assert!(options.is_empty());
    assert!(answer_labels_by_option_id.is_empty());
}
