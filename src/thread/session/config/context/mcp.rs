use std::collections::HashMap;

use codex_config::{McpServerConfig, McpServerTransportConfig};

use super::ContextSelectorSummary;

pub(in crate::thread) fn build_mcp_summary(
    effective_mcp_servers: &HashMap<String, McpServerConfig>,
) -> ContextSelectorSummary {
    let mut entries = effective_mcp_servers
        .iter()
        .map(|(name, config)| {
            let (transport_label, target, extra) = match &config.transport {
                McpServerTransportConfig::Stdio { command, cwd, .. } => (
                    "stdio",
                    command.clone(),
                    cwd.as_ref()
                        .map(|path| format!("cwd {}", path.render_for_ui()))
                        .unwrap_or_default(),
                ),
                McpServerTransportConfig::StreamableHttp { url, .. } => {
                    ("http", url.clone(), String::new())
                }
            };
            (
                name.clone(),
                format!(
                    "{} · {}{}",
                    transport_label,
                    truncate_middle(&target, 48),
                    if extra.is_empty() {
                        String::new()
                    } else {
                        format!(" · {extra}")
                    }
                ),
                format_mcp_report_entry(name, config),
                matches!(config.transport, McpServerTransportConfig::Stdio { .. }),
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    if entries.is_empty() {
        return ContextSelectorSummary {
            label: "MCP · none".to_string(),
            description: "No MCP servers are configured for this session.".to_string(),
            report: "No MCP servers are configured for this session.".to_string(),
        };
    }

    let total = entries.len();
    let stdio_count = entries.iter().filter(|entry| entry.3).count();
    let http_count = total.saturating_sub(stdio_count);

    let mut description_lines = vec![format!(
        "{total} MCP server(s) configured for this session ({stdio_count} stdio, {http_count} http)."
    )];
    description_lines.extend(
        entries
            .iter()
            .take(3)
            .map(|(name, preview, _, _)| format!("{name} · {preview}")),
    );
    if total > 3 {
        description_lines.push(format!("+{} more", total - 3));
    }

    let mut report_lines = vec![format!(
        "MCP servers configured for this session: {total} ({stdio_count} stdio, {http_count} http)."
    )];
    for (_, _, report, _) in entries {
        report_lines.push(String::new());
        report_lines.push(report);
    }

    ContextSelectorSummary {
        label: format!("MCP · {total} srv"),
        description: description_lines.join("\n"),
        report: report_lines.join("\n"),
    }
}

fn format_mcp_report_entry(name: &str, config: &McpServerConfig) -> String {
    let mut lines = vec![format!(
        "- {name} [{}{}{}]",
        match &config.transport {
            McpServerTransportConfig::Stdio { .. } => "stdio",
            McpServerTransportConfig::StreamableHttp { .. } => "http",
        },
        if config.enabled { "" } else { ", disabled" },
        if config.required { ", required" } else { "" }
    )];

    match &config.transport {
        McpServerTransportConfig::Stdio {
            command, args, cwd, ..
        } => {
            lines.push(format!("  command: {}", truncate_middle(command, 120)));
            if !args.is_empty() {
                lines.push(format!("  args: {}", args.join(" ")));
            }
            if let Some(cwd) = cwd {
                lines.push(format!("  cwd: {}", cwd.render_for_ui()));
            }
        }
        McpServerTransportConfig::StreamableHttp { url, .. } => {
            lines.push(format!("  url: {}", truncate_middle(url, 120)));
        }
    }

    lines.push(format!(
        "  tools: {}",
        config
            .enabled_tools
            .as_ref()
            .map(|tools| tools.join(", "))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "all".to_string())
    ));
    if let Some(disabled_tools) = &config.disabled_tools
        && !disabled_tools.is_empty()
    {
        lines.push(format!("  disabled tools: {}", disabled_tools.join(", ")));
    }

    lines.join("\n")
}

fn truncate_middle(value: &str, max_chars: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return value.to_string();
    }
    if max_chars <= 3 {
        return chars.iter().take(max_chars).collect();
    }

    let head_len = max_chars / 2;
    let tail_len = max_chars.saturating_sub(head_len + 3);
    let head = chars.iter().take(head_len).collect::<String>();
    let tail = chars
        .iter()
        .rev()
        .take(tail_len)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{head}...{tail}")
}
