use codex_app_server_protocol::{SkillMetadata, SkillScope, SkillsListResponse};

use super::ContextSelectorSummary;

pub(in crate::thread) fn build_skills_summary(
    response: &SkillsListResponse,
) -> ContextSelectorSummary {
    let mut entries = response
        .data
        .iter()
        .flat_map(|entry| entry.skills.iter())
        .map(|skill| {
            let display_name = skill
                .interface
                .as_ref()
                .and_then(|interface| interface.display_name.clone())
                .unwrap_or_else(|| skill.name.clone());
            let summary = skill
                .interface
                .as_ref()
                .and_then(|interface| interface.short_description.clone())
                .or_else(|| skill.short_description.clone())
                .unwrap_or_else(|| skill.description.clone());

            (
                display_name,
                summary,
                format_skill_report_entry(skill),
                skill.enabled,
                skill.scope,
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.0.to_lowercase());

    let total = entries.len();
    let enabled_count = entries.iter().filter(|entry| entry.3).count();
    let disabled_count = total.saturating_sub(enabled_count);
    let error_count = response
        .data
        .iter()
        .map(|entry| entry.errors.len())
        .sum::<usize>();

    if total == 0 && error_count == 0 {
        return ContextSelectorSummary {
            label: "Skills · none".to_string(),
            description: "No skills were discovered for the current workspace.".to_string(),
            report: "No skills were discovered for the current workspace.".to_string(),
        };
    }

    let mut description_lines = vec![format!(
        "{enabled_count}/{total} skill(s) enabled for this workspace."
    )];
    description_lines.extend(
        entries
            .iter()
            .take(3)
            .map(|(name, summary, _, enabled, scope)| {
                format!(
                    "{} · {} · {}",
                    name,
                    skill_scope_label(*scope),
                    if *enabled { summary } else { "disabled" }
                )
            }),
    );
    if disabled_count > 0 {
        description_lines.push(format!("{disabled_count} disabled"));
    }
    if error_count > 0 {
        description_lines.push(format!("{error_count} load error(s)"));
    }

    let mut report_lines = vec![format!(
        "Skills for current workspace: {enabled_count}/{total} enabled."
    )];
    if disabled_count > 0 {
        report_lines.push(format!("Disabled: {disabled_count}."));
    }
    if error_count > 0 {
        report_lines.push(format!("Load errors: {error_count}."));
        for error in response.data.iter().flat_map(|entry| entry.errors.iter()) {
            report_lines.push(format!("- {}: {}", error.path.display(), error.message));
        }
    }
    for (_, _, report, _, _) in entries {
        report_lines.push(String::new());
        report_lines.push(report);
    }

    let mut label = format!("Skills · {enabled_count} on");
    if error_count > 0 {
        label.push_str(&format!(" · {error_count} err"));
    }

    ContextSelectorSummary {
        label,
        description: description_lines.join("\n"),
        report: report_lines.join("\n"),
    }
}

fn format_skill_report_entry(skill: &SkillMetadata) -> String {
    let display_name = skill
        .interface
        .as_ref()
        .and_then(|interface| interface.display_name.clone())
        .unwrap_or_else(|| skill.name.clone());
    let summary = skill
        .interface
        .as_ref()
        .and_then(|interface| interface.short_description.clone())
        .or_else(|| skill.short_description.clone())
        .unwrap_or_else(|| skill.description.clone());

    let mut lines = vec![format!(
        "- {} [{}{}]",
        display_name,
        skill_scope_label(skill.scope),
        if skill.enabled { "" } else { ", disabled" }
    )];
    if display_name != skill.name {
        lines.push(format!("  name: {}", skill.name));
    }
    lines.push(format!("  summary: {summary}"));
    lines.push(format!("  path: {}", skill.path.display()));
    if let Some(dependencies) = &skill.dependencies
        && !dependencies.tools.is_empty()
    {
        lines.push(format!("  tool deps: {}", dependencies.tools.len()));
    }
    lines.join("\n")
}

fn skill_scope_label(scope: SkillScope) -> &'static str {
    match scope {
        SkillScope::User => "user",
        SkillScope::Repo => "repo",
        SkillScope::System => "system",
        SkillScope::Admin => "admin",
    }
}
