use super::*;

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
