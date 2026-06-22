//! Формирование текстового контента collaboration/sub-agent карточек.

use std::collections::HashMap;

use crate::thread::prompt_commands::normalize_preview;
use agent_client_protocol::schema::v1::ToolCallContent;
use codex_app_server_protocol::{CollabAgentState, CollabAgentTool};
use serde_json::Value;

use super::CollabAgentLabel;

// Собираем текстовый payload карточки: sender/receivers/prompt и статусы агентов.
pub(in crate::thread) fn collab_tool_content(
    tool: &CollabAgentTool,
    agent_labels: &HashMap<String, CollabAgentLabel>,
    sender_thread_id: &str,
    receiver_thread_ids: &[String],
    prompt: Option<&str>,
    agents_states: &HashMap<String, CollabAgentState>,
    include_prompt: bool,
) -> Vec<ToolCallContent> {
    let mut lines = vec![format!(
        "Sender: {}",
        format_thread_reference(sender_thread_id, agent_labels)
    )];
    append_receiver_lines(&mut lines, tool, receiver_thread_ids, agent_labels);

    if include_prompt
        && let Some(prompt) = prompt.map(str::trim).filter(|prompt| !prompt.is_empty())
    {
        lines.push(format!("Prompt: {}", normalize_preview(prompt)));
    }

    if !agents_states.is_empty() {
        let mut statuses = agents_states.iter().collect::<Vec<_>>();
        statuses.sort_by(|(left, _), (right, _)| left.cmp(right));

        if statuses.len() == 1 {
            let (thread_id, state) = statuses[0];
            if !receiver_thread_ids
                .iter()
                .any(|receiver| receiver == thread_id)
            {
                lines.push(format!(
                    "Agent: {}",
                    format_thread_reference(thread_id, agent_labels)
                ));
            }
            lines.push(format!(
                "Status: {}",
                super::status::collab_agent_state_summary(state)
            ));
        } else {
            lines.push(super::status::collab_status_summary_line(agents_states));
            for (thread_id, state) in statuses {
                lines.push(format!(
                    "- {}: {}",
                    format_thread_reference(thread_id, agent_labels),
                    super::status::collab_agent_state_summary(state)
                ));
            }
        }
    }

    vec![lines.join("\n").into()]
}

pub(in crate::thread) fn collab_tool_raw_input(
    tool: &CollabAgentTool,
    agent_labels: &HashMap<String, CollabAgentLabel>,
    receiver_thread_ids: &[String],
    prompt: Option<&str>,
) -> Option<Value> {
    let prompt = prompt.map(str::trim).filter(|prompt| !prompt.is_empty());
    let text = match tool {
        CollabAgentTool::SpawnAgent | CollabAgentTool::SendInput => {
            prompt.map(normalize_preview).or_else(|| {
                (!receiver_thread_ids.is_empty()).then(|| {
                    format!(
                        "Agent: {}",
                        format_collab_receivers_with_labels(receiver_thread_ids, agent_labels)
                    )
                })
            })
        }
        CollabAgentTool::Wait => (!receiver_thread_ids.is_empty()).then(|| {
            format!(
                "Waiting for: {}",
                format_collab_receivers_with_labels(receiver_thread_ids, agent_labels)
            )
        }),
        CollabAgentTool::ResumeAgent | CollabAgentTool::CloseAgent => {
            (!receiver_thread_ids.is_empty()).then(|| {
                format!(
                    "Agent: {}",
                    format_collab_receivers_with_labels(receiver_thread_ids, agent_labels)
                )
            })
        }
    }?;

    Some(Value::String(text))
}

pub(in crate::thread) fn collab_tool_raw_output(
    agent_labels: &HashMap<String, CollabAgentLabel>,
    receiver_thread_ids: &[String],
    agents_states: &HashMap<String, CollabAgentState>,
) -> Option<Value> {
    if agents_states.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    let mut statuses = agents_states.iter().collect::<Vec<_>>();
    statuses.sort_by(|(left, _), (right, _)| left.cmp(right));

    if statuses.len() == 1 {
        let (thread_id, state) = statuses[0];
        if !receiver_thread_ids
            .iter()
            .any(|receiver| receiver == thread_id)
        {
            lines.push(format!(
                "Agent: {}",
                format_thread_reference(thread_id, agent_labels)
            ));
        }
        lines.push(super::status::collab_agent_state_summary(state));
    } else {
        lines.push(super::status::collab_status_summary_line(agents_states));
        for (thread_id, state) in statuses {
            lines.push(format!(
                "{}: {}",
                format_thread_reference(thread_id, agent_labels),
                super::status::collab_agent_state_summary(state)
            ));
        }
    }

    Some(Value::String(lines.join("\n")))
}

fn append_receiver_lines(
    lines: &mut Vec<String>,
    tool: &CollabAgentTool,
    receiver_thread_ids: &[String],
    agent_labels: &HashMap<String, CollabAgentLabel>,
) {
    if receiver_thread_ids.is_empty() {
        return;
    }

    match tool {
        CollabAgentTool::SpawnAgent => {
            if receiver_thread_ids.len() == 1 {
                lines.push(format!(
                    "Spawned: {}",
                    format_thread_reference(&receiver_thread_ids[0], agent_labels)
                ));
            } else {
                lines.push(format!(
                    "Spawned: {}",
                    format_collab_receivers_with_labels(receiver_thread_ids, agent_labels)
                ));
            }
        }
        CollabAgentTool::Wait => {
            lines.push(format!(
                "Waiting for: {}",
                format_collab_receivers_with_labels(receiver_thread_ids, agent_labels)
            ));
        }
        CollabAgentTool::SendInput | CollabAgentTool::ResumeAgent | CollabAgentTool::CloseAgent => {
            if receiver_thread_ids.len() == 1 {
                lines.push(format!(
                    "Agent: {}",
                    format_thread_reference(&receiver_thread_ids[0], agent_labels)
                ));
            } else {
                lines.push(format!(
                    "Agents: {}",
                    format_collab_receivers_with_labels(receiver_thread_ids, agent_labels)
                ));
            }
        }
    }
}

fn format_collab_receivers_with_labels(
    receiver_thread_ids: &[String],
    agent_labels: &HashMap<String, CollabAgentLabel>,
) -> String {
    let has_any_labels = receiver_thread_ids
        .iter()
        .filter_map(|thread_id| agent_labels.get(thread_id))
        .any(|label| {
            label
                .nickname
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
                || label
                    .role
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|value| !value.is_empty())
        });
    if !has_any_labels {
        return format_collab_receivers(receiver_thread_ids);
    }

    const MAX_RECEIVER_IDS: usize = 3;
    let formatted = receiver_thread_ids
        .iter()
        .map(|thread_id| format_thread_reference(thread_id, agent_labels))
        .collect::<Vec<_>>();
    if formatted.len() <= MAX_RECEIVER_IDS {
        return formatted.join(", ");
    }

    let visible = formatted
        .iter()
        .take(MAX_RECEIVER_IDS)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let remaining = formatted.len().saturating_sub(MAX_RECEIVER_IDS);
    format!("{visible}, ... (+{remaining} more)")
}

fn format_thread_reference(
    thread_id: &str,
    agent_labels: &HashMap<String, CollabAgentLabel>,
) -> String {
    let Some(label) = agent_labels.get(thread_id) else {
        return thread_id.to_string();
    };

    match (
        label
            .nickname
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        label
            .role
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    ) {
        (Some(nickname), Some(role)) => format!("{nickname} [{role}] ({thread_id})"),
        (Some(nickname), None) => format!("{nickname} ({thread_id})"),
        (None, Some(role)) => format!("[{role}] ({thread_id})"),
        (None, None) => thread_id.to_string(),
    }
}

// Ограничиваем количество receiver id в карточке, чтобы она оставалась читаемой.
pub(in crate::thread) fn format_collab_receivers(receiver_thread_ids: &[String]) -> String {
    const MAX_RECEIVER_IDS: usize = 3;
    if receiver_thread_ids.len() <= MAX_RECEIVER_IDS {
        return receiver_thread_ids.join(", ");
    }

    let visible = receiver_thread_ids
        .iter()
        .take(MAX_RECEIVER_IDS)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let remaining = receiver_thread_ids.len().saturating_sub(MAX_RECEIVER_IDS);
    format!("{visible}, ... (+{remaining} more)")
}
