//! Парсинг/нормализация plan-содержимого и collaboration-mode plumbing.

use agent_client_protocol::{Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus};
use codex_app_server_protocol::{TurnPlanStep, TurnPlanStepStatus};
use codex_protocol::config_types::{
    CollaborationMode, ModeKind, Settings as CollaborationSettings,
};
use codex_protocol::openai_models::ReasoningEffort;

use crate::thread::MAX_VISIBLE_PLAN_ENTRIES;

pub(in crate::thread) fn plan_entries_all_pending(entries: &[PlanEntry]) -> bool {
    !entries.is_empty()
        && entries
            .iter()
            .all(|entry| entry.status == PlanEntryStatus::Pending)
}

pub(in crate::thread) fn collaboration_mode_for_turn(
    mode: ModeKind,
    model: &str,
    reasoning_effort: ReasoningEffort,
) -> Option<CollaborationMode> {
    // `turn/start.collaboration_mode` в app-server «липкий»: после установки он применяется к
    // этому и следующим turn. Когда plan-mode выключен, отправляем явный режим `default`,
    // чтобы клиенты надёжно выходили из plan-mode без устаревшего состояния.
    Some(CollaborationMode {
        mode,
        settings: CollaborationSettings {
            model: model.to_string(),
            reasoning_effort: Some(reasoning_effort),
            developer_instructions: None,
        },
    })
}

pub(in crate::thread) fn collaboration_mode_label(mode: ModeKind) -> &'static str {
    match mode {
        ModeKind::Plan => "plan",
        _ => "default",
    }
}

pub(in crate::thread) fn parse_collaboration_mode(value: &str) -> Option<ModeKind> {
    match value {
        "plan" | "on" => Some(ModeKind::Plan),
        "default" | "off" | "code" => Some(ModeKind::Default),
        _ => None,
    }
}

pub(in crate::thread) fn turn_plan_step_to_entry(step: TurnPlanStep) -> PlanEntry {
    PlanEntry::new(
        step.step,
        PlanEntryPriority::Medium,
        match step.status {
            TurnPlanStepStatus::Pending => PlanEntryStatus::Pending,
            TurnPlanStepStatus::InProgress => PlanEntryStatus::InProgress,
            TurnPlanStepStatus::Completed => PlanEntryStatus::Completed,
        },
    )
}

pub(in crate::thread) fn plan_from_text(text: &str) -> Option<Plan> {
    let entries = text
        .lines()
        .filter_map(parse_plan_entry_from_line)
        .collect::<Vec<_>>();
    if entries.is_empty() {
        None
    } else {
        Some(Plan::new(entries))
    }
}

pub(in crate::thread) fn promote_first_pending_step(plan: Plan) -> Plan {
    let mut entries = plan.entries;
    if entries
        .iter()
        .all(|entry| entry.status == PlanEntryStatus::Pending)
        && let Some(first) = entries.first_mut()
    {
        first.status = PlanEntryStatus::InProgress;
    }
    Plan::new(entries)
}

pub(in crate::thread) fn limit_plan_entries(mut entries: Vec<PlanEntry>) -> Vec<PlanEntry> {
    if entries.len() > MAX_VISIBLE_PLAN_ENTRIES {
        entries.truncate(MAX_VISIBLE_PLAN_ENTRIES);
    }
    entries
}

fn parse_plan_entry_from_line(line: &str) -> Option<PlanEntry> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    if let Some(content) = trimmed
        .strip_prefix("- [x] ")
        .or_else(|| trimmed.strip_prefix("* [x] "))
    {
        let content = content.trim();
        if content.is_empty() {
            return None;
        }
        return Some(PlanEntry::new(
            content,
            PlanEntryPriority::Medium,
            PlanEntryStatus::Completed,
        ));
    }

    if let Some(content) = trimmed
        .strip_prefix("- [ ] ")
        .or_else(|| trimmed.strip_prefix("* [ ] "))
    {
        let content = content.trim();
        if content.is_empty() {
            return None;
        }
        return Some(PlanEntry::new(
            content,
            PlanEntryPriority::Medium,
            PlanEntryStatus::Pending,
        ));
    }

    if let Some(content) = trimmed
        .strip_prefix("- [~] ")
        .or_else(|| trimmed.strip_prefix("* [~] "))
        .or_else(|| trimmed.strip_prefix("- [-] "))
        .or_else(|| trimmed.strip_prefix("* [-] "))
    {
        let content = content.trim();
        if content.is_empty() {
            return None;
        }
        return Some(PlanEntry::new(
            content,
            PlanEntryPriority::Medium,
            PlanEntryStatus::InProgress,
        ));
    }

    // Планы из app-server часто используют обычные буллеты/нумерацию
    // (например: "- first", "1. second") без checkbox-маркеров.
    if let Some(content) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| strip_numbered_prefix(trimmed))
    {
        let content = content.trim();
        if content.is_empty() {
            return None;
        }
        return Some(PlanEntry::new(
            content,
            PlanEntryPriority::Medium,
            PlanEntryStatus::Pending,
        ));
    }

    None
}

fn strip_numbered_prefix(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == 0 || index >= bytes.len() {
        return None;
    }

    if bytes[index] != b'.' && bytes[index] != b')' {
        return None;
    }
    index += 1;
    if index >= bytes.len() || !bytes[index].is_ascii_whitespace() {
        return None;
    }
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }

    Some(&line[index..])
}
