//! Общие helper-ы для `/threads` и `/resume`: чтение paginated thread list и общие formatters.

use std::time::{SystemTime, UNIX_EPOCH};

use agent_client_protocol::Error;
use codex_app_server_protocol::{Thread, ThreadListParams, ThreadSortKey};

use crate::thread::ThreadInner;

pub(in crate::thread) async fn list_all_threads(
    inner: &mut ThreadInner,
    sort_key: ThreadSortKey,
    cwd: Option<String>,
    search_term: Option<String>,
) -> Result<Vec<Thread>, Error> {
    let mut threads = Vec::new();
    let mut cursor = None;

    loop {
        let response = inner
            .app
            .thread_list(ThreadListParams {
                cursor: cursor.take(),
                limit: Some(100),
                sort_key: Some(sort_key),
                model_providers: None,
                source_kinds: None,
                archived: Some(false),
                cwd: cwd.clone(),
                search_term: search_term.clone(),
            })
            .await?;

        threads.extend(response.data);

        match response.next_cursor {
            Some(next_cursor) => cursor = Some(next_cursor),
            None => break,
        }
    }

    Ok(threads)
}

pub(in crate::thread) fn format_relative_timestamp(unix_seconds: i64) -> String {
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

pub(in crate::thread) fn thread_display_title(thread: &Thread) -> String {
    let base = thread
        .name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&thread.preview);
    crate::thread::prompt_commands::normalize_preview(base)
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
