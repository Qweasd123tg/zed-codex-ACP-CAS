//! Render the list of available threads for `/threads`.

use std::time::{SystemTime, UNIX_EPOCH};

use agent_client_protocol::{Error, StopReason};
use codex_app_server_protocol::{ThreadListParams, ThreadSortKey};

use crate::thread::ThreadInner;
use crate::thread::prompt_commands::normalize_preview;

// Fetch thread history and render a compact list for interactive `/resume`.
pub(in crate::thread) async fn handle_threads_command(
    inner: &mut ThreadInner,
) -> Result<StopReason, Error> {
    let response = inner
        .app
        .thread_list(ThreadListParams {
            cursor: None,
            limit: Some(20),
            sort_key: Some(ThreadSortKey::UpdatedAt),
            model_providers: None,
            source_kinds: None,
            archived: Some(false),
        })
        .await?;

    if response.data.is_empty() {
        inner
            .client
            .send_agent_text("No saved threads found. Create one prompt first.")
            .await;
        return Ok(StopReason::EndTurn);
    }

    let mut lines = vec!["Saved threads (newest first):".to_string()];
    for thread in response.data {
        lines.push(format!(
            "- `{}` | created: {} | updated: {} | branch: {} | {}",
            thread.id,
            format_relative_timestamp(thread.created_at),
            format_relative_timestamp(thread.updated_at),
            thread
                .git_info
                .as_ref()
                .and_then(|git| git.branch.as_deref())
                .filter(|value| !value.is_empty())
                .unwrap_or("-"),
            normalize_preview(&thread.preview)
        ));
    }
    lines.push(
        "Use `/resume` to choose a thread from this workspace, or `/resume <partial_id>` to search."
            .to_string(),
    );

    inner.client.send_agent_text(lines.join("\n")).await;
    Ok(StopReason::EndTurn)
}

fn format_relative_timestamp(unix_seconds: i64) -> String {
    if unix_seconds <= 0 {
        return "-".to_string();
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();

    if unix_seconds > now {
        return format_future_duration((unix_seconds - now) as u64);
    }

    format_past_duration((now - unix_seconds) as u64)
}

fn format_past_duration(delta: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    const MONTH: u64 = 30 * DAY;
    const YEAR: u64 = 365 * DAY;

    if delta < MINUTE {
        "just now".to_string()
    } else if delta < HOUR {
        format!("{}m ago", delta / MINUTE)
    } else if delta < DAY {
        format!("{}h ago", delta / HOUR)
    } else if delta < MONTH {
        format!("{}d ago", delta / DAY)
    } else if delta < YEAR {
        format!("{}mo ago", delta / MONTH)
    } else {
        format!("{}y ago", delta / YEAR)
    }
}

fn format_future_duration(delta: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;

    if delta < MINUTE {
        "soon".to_string()
    } else if delta < HOUR {
        format!("in {}m", delta / MINUTE)
    } else if delta < DAY {
        format!("in {}h", delta / HOUR)
    } else {
        format!("in {}d", delta / DAY)
    }
}
