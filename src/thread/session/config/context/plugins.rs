use codex_app_server_protocol::{PluginListResponse, PluginSource, PluginSummary};

use super::ContextSelectorSummary;

pub(in crate::thread) fn build_plugins_summary(
    response: &PluginListResponse,
) -> ContextSelectorSummary {
    let mut installed_entries = Vec::new();
    let mut available_entries = Vec::new();
    let mut total_plugins = 0usize;

    for marketplace in &response.marketplaces {
        for plugin in &marketplace.plugins {
            total_plugins = total_plugins.saturating_add(1);
            let display_name = plugin
                .interface
                .as_ref()
                .and_then(|interface| interface.display_name.clone())
                .unwrap_or_else(|| plugin.name.clone());
            let summary = plugin
                .interface
                .as_ref()
                .and_then(|interface| interface.short_description.clone())
                .or_else(|| {
                    plugin
                        .interface
                        .as_ref()
                        .and_then(|interface| interface.long_description.clone())
                })
                .or_else(|| {
                    plugin
                        .interface
                        .as_ref()
                        .and_then(|interface| interface.category.clone())
                })
                .unwrap_or_else(|| "No description provided.".to_string());

            if plugin.installed {
                installed_entries.push((
                    display_name.clone(),
                    summary.clone(),
                    format_plugin_report_entry(&marketplace.name, plugin, &display_name, &summary),
                    plugin.enabled,
                ));
            } else {
                available_entries.push((display_name, summary));
            }
        }
    }

    installed_entries.sort_by(|left, right| left.0.to_lowercase().cmp(&right.0.to_lowercase()));
    available_entries.sort_by(|left, right| left.0.to_lowercase().cmp(&right.0.to_lowercase()));

    let marketplace_count = response.marketplaces.len();
    let installed_count = installed_entries.len();
    let enabled_count = installed_entries.iter().filter(|entry| entry.3).count();
    let disabled_count = installed_count.saturating_sub(enabled_count);

    if total_plugins == 0 && response.remote_sync_error.is_none() {
        return ContextSelectorSummary {
            label: "Plugins · none".to_string(),
            description: "No plugins were discovered for this session.".to_string(),
            report: "No plugins were discovered for this session.".to_string(),
        };
    }

    let mut description_lines = if installed_count > 0 {
        vec![format!(
            "{enabled_count}/{installed_count} installed plugin(s) enabled across {marketplace_count} marketplace(s)."
        )]
    } else {
        vec![format!(
            "No plugins are installed for this session yet ({total_plugins} visible across {marketplace_count} marketplace(s))."
        )]
    };

    if installed_count > 0 {
        description_lines.extend(installed_entries.iter().take(3).map(
            |(name, summary, _, enabled)| {
                format!("{name} · {}", if *enabled { summary } else { "disabled" })
            },
        ));
        if disabled_count > 0 {
            description_lines.push(format!("{disabled_count} disabled"));
        }
    } else {
        description_lines.extend(
            available_entries
                .iter()
                .take(3)
                .map(|(name, summary)| format!("{name} · {summary}")),
        );
        if total_plugins > 3 {
            description_lines.push(format!("+{} more", total_plugins - 3));
        }
    }

    if response.remote_sync_error.is_some() {
        description_lines.push("remote sync unavailable".to_string());
    }

    let mut report_lines = vec![format!(
        "Plugins visible to this session: {total_plugins} across {marketplace_count} marketplace(s)."
    )];
    if installed_count == 0 {
        report_lines.push("Installed plugins: none.".to_string());
    } else {
        report_lines.push(format!(
            "Installed: {installed_count} ({enabled_count} enabled, {disabled_count} disabled)."
        ));
        for (_, _, report, _) in installed_entries {
            report_lines.push(String::new());
            report_lines.push(report);
        }
    }

    if !available_entries.is_empty() {
        report_lines.push(String::new());
        report_lines.push("Available but not installed:".to_string());
        for (name, summary) in available_entries.iter().take(10) {
            report_lines.push(format!("- {name} · {summary}"));
        }
        if available_entries.len() > 10 {
            report_lines.push(format!("- +{} more", available_entries.len() - 10));
        }
    }

    if let Some(error) = &response.remote_sync_error {
        report_lines.push(String::new());
        report_lines.push(format!("Remote sync error: {error}"));
    }

    let mut label = if installed_count == 0 {
        "Plugins · none".to_string()
    } else {
        format!("Plugins · {enabled_count} on")
    };
    if disabled_count > 0 {
        label.push_str(&format!(" · {disabled_count} off"));
    }
    if response.remote_sync_error.is_some() {
        label.push_str(" · sync err");
    }

    ContextSelectorSummary {
        label,
        description: description_lines.join("\n"),
        report: report_lines.join("\n"),
    }
}

fn format_plugin_report_entry(
    marketplace_name: &str,
    plugin: &PluginSummary,
    display_name: &str,
    summary: &str,
) -> String {
    let mut lines = vec![format!(
        "- {} [{}{}]",
        display_name,
        marketplace_name,
        if plugin.enabled {
            ", enabled"
        } else {
            ", disabled"
        }
    )];
    if display_name != plugin.name {
        lines.push(format!("  name: {}", plugin.name));
    }
    lines.push(format!("  id: {}", plugin.id));
    lines.push(format!("  summary: {summary}"));
    if let Some(interface) = &plugin.interface {
        if let Some(developer_name) = &interface.developer_name {
            lines.push(format!("  developer: {developer_name}"));
        }
        if let Some(category) = &interface.category {
            lines.push(format!("  category: {category}"));
        }
        if !interface.capabilities.is_empty() {
            lines.push(format!(
                "  capabilities: {}",
                interface.capabilities.join(", ")
            ));
        }
    }
    match &plugin.source {
        PluginSource::Local { .. } => {
            lines.push("  source: local".to_string());
        }
    }
    lines.join("\n")
}
