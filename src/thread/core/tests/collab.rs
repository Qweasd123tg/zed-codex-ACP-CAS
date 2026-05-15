use super::*;

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
