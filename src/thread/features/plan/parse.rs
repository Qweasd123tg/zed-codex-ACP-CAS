//! Parsing and normalization for plan content plus collaboration-mode plumbing.

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
    // `turn/start.collaboration_mode` in app-server is sticky: once set, it applies to
    // this and subsequent turns. When plan mode is turned off, send an explicit `default`
    // mode so clients exit plan mode reliably without stale state.
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
    plan_from_text_with_mode(text, false)
}

pub(in crate::thread) fn plan_from_plan_item_text(text: &str) -> Option<Plan> {
    plan_from_text_with_mode(text, true)
}

fn plan_from_text_with_mode(text: &str, allow_list_only: bool) -> Option<Plan> {
    if allow_list_only {
        let steps_section_entries = parse_plan_entries_from_steps_section(text);
        if !steps_section_entries.is_empty() {
            return Some(Plan::new(steps_section_entries));
        }
    }

    let mut entries = Vec::new();
    let mut has_checkbox_entries = false;
    let mut has_plan_heading = false;
    let mut has_plan_intro = false;
    let mut has_options_marker = false;
    let mut has_non_numbered_list_entries = false;
    let mut has_any_heading = false;
    let mut non_entry_lines = 0usize;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if is_markdown_heading(trimmed) {
            has_any_heading = true;
            has_plan_heading |= heading_looks_like_plan(trimmed);
            if !heading_looks_like_plan(trimmed) {
                non_entry_lines += 1;
            }
            continue;
        }

        if line_looks_like_options_marker(trimmed) {
            has_options_marker = true;
            non_entry_lines += 1;
            continue;
        }

        if line_looks_like_plan_intro(trimmed) {
            has_plan_intro = true;
            non_entry_lines += 1;
            continue;
        }

        if let Some((entry, kind)) = parse_plan_entry_from_line(trimmed) {
            if kind == ParsedPlanLineKind::Checkbox {
                has_checkbox_entries = true;
            }
            if matches!(
                kind,
                ParsedPlanLineKind::Checkbox | ParsedPlanLineKind::Bullet
            ) {
                has_non_numbered_list_entries = true;
            }
            entries.push(entry);
            continue;
        }

        non_entry_lines += 1;
    }

    if entries.is_empty() {
        None
    } else if allow_list_only && has_any_heading && !has_checkbox_entries {
        // For plan items, sectioned markdown without an explicit steps section
        // is considered ambiguous: do not turn arbitrary lists
        // such as criteria or fixed parameters into a checklist.
        None
    } else if has_checkbox_entries || has_plan_heading || has_plan_intro {
        Some(Plan::new(entries))
    } else if has_options_marker {
        None
    } else if allow_list_only
        && non_entry_lines == 0
        && entries.len() >= 2
        && has_non_numbered_list_entries
    {
        Some(Plan::new(entries))
    } else {
        None
    }
}

fn parse_plan_entries_from_steps_section(text: &str) -> Vec<PlanEntry> {
    let mut in_steps_section = false;
    let mut entries = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if is_markdown_heading(trimmed) {
            if line_looks_like_steps_section_title(trimmed) {
                in_steps_section = true;
                continue;
            }
            if in_steps_section {
                break;
            }
            continue;
        }

        if line_looks_like_steps_section_title(trimmed) {
            in_steps_section = true;
            continue;
        }

        if !in_steps_section {
            continue;
        }

        if let Some((entry, _kind)) = parse_plan_entry_from_line(trimmed) {
            entries.push(entry);
            continue;
        }

        if !entries.is_empty() {
            break;
        }
    }

    entries
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

#[derive(Copy, Clone, Eq, PartialEq)]
enum ParsedPlanLineKind {
    Checkbox,
    Bullet,
    Numbered,
}

fn parse_plan_entry_from_line(line: &str) -> Option<(PlanEntry, ParsedPlanLineKind)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
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
        return Some((
            PlanEntry::new(
                content,
                PlanEntryPriority::Medium,
                PlanEntryStatus::Completed,
            ),
            ParsedPlanLineKind::Checkbox,
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
        return Some((
            PlanEntry::new(content, PlanEntryPriority::Medium, PlanEntryStatus::Pending),
            ParsedPlanLineKind::Checkbox,
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
        return Some((
            PlanEntry::new(
                content,
                PlanEntryPriority::Medium,
                PlanEntryStatus::InProgress,
            ),
            ParsedPlanLineKind::Checkbox,
        ));
    }

    // Plans from app-server often use plain bullets or numbering
    // such as "- first" or "1. second" without checkbox markers.
    if let Some(content) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        let content = content.trim();
        if content.is_empty() {
            return None;
        }
        return Some((
            PlanEntry::new(content, PlanEntryPriority::Medium, PlanEntryStatus::Pending),
            ParsedPlanLineKind::Bullet,
        ));
    }

    if let Some(content) = strip_numbered_prefix(trimmed) {
        let content = content.trim();
        if content.is_empty() {
            return None;
        }
        return Some((
            PlanEntry::new(content, PlanEntryPriority::Medium, PlanEntryStatus::Pending),
            ParsedPlanLineKind::Numbered,
        ));
    }

    None
}

fn is_markdown_heading(line: &str) -> bool {
    line.starts_with('#')
}

fn heading_looks_like_plan(line: &str) -> bool {
    let normalized = line
        .trim_start_matches('#')
        .trim()
        .to_lowercase()
        .replace('ё', "е");
    line_has_plan_keyword(&normalized)
}

fn line_looks_like_plan_intro(line: &str) -> bool {
    let normalized = line.to_lowercase().replace('ё', "е");
    (normalized.ends_with(':') || normalized.ends_with('.')) && line_has_plan_keyword(&normalized)
}

fn line_looks_like_steps_section_title(line: &str) -> bool {
    let normalized = line
        .trim_start_matches('#')
        .trim_end_matches(':')
        .trim()
        .to_lowercase()
        .replace('ё', "е");

    normalized.contains("пошаг")
        || normalized == "шаги"
        || normalized == "этапы"
        || (normalized.contains("этап") && normalized.contains("реализац"))
        || (normalized.contains("план") && normalized.contains("реализац"))
        || (normalized.contains("шаг") && normalized.contains("выполн"))
        || (normalized.contains("step") && normalized.contains("implementation"))
        || (normalized.contains("step") && normalized.contains("execution"))
        || normalized.contains("implementation steps")
        || normalized.contains("execution steps")
        || normalized.contains("step-by-step")
        || normalized == "steps"
        || (normalized.contains("steps") && normalized.contains("plan"))
        || (normalized.contains("шаг") && normalized.contains("реализац"))
        || (normalized.contains("шаг") && normalized.contains("план"))
}

fn line_has_plan_keyword(line: &str) -> bool {
    const PLAN_KEYWORDS: [&str; 11] = [
        "plan",
        "steps",
        "roadmap",
        "todo",
        "next steps",
        "implementation plan",
        "proposed plan",
        "план",
        "шаг",
        "этап",
        "дорожн",
    ];
    PLAN_KEYWORDS.iter().any(|keyword| line.contains(keyword))
}

fn line_looks_like_options_marker(line: &str) -> bool {
    let normalized = line
        .trim_end_matches(':')
        .trim()
        .to_lowercase()
        .replace('ё', "е");
    matches!(
        normalized.as_str(),
        "options" | "choices" | "variants" | "option" | "choice" | "варианты" | "опции"
    )
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
