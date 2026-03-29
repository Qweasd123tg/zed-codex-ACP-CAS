//! Вывод списка доступных thread для `/threads`.

use agent_client_protocol::{Error, StopReason};
use codex_app_server_protocol::ThreadSortKey;

use super::common::{format_relative_timestamp, list_all_threads, thread_display_title};
use crate::thread::ThreadInner;

// Получаем историю thread и рендерим полный список для `/threads`.
pub(in crate::thread) async fn handle_threads_command(
    inner: &mut ThreadInner,
) -> Result<StopReason, Error> {
    let threads = list_all_threads(inner, ThreadSortKey::UpdatedAt, None, None).await?;

    if threads.is_empty() {
        inner
            .client
            .send_agent_text("No saved threads found. Create one prompt first.")
            .await;
        return Ok(StopReason::EndTurn);
    }

    let mut lines = vec![format!(
        "Saved threads (newest first, {} total):",
        threads.len()
    )];
    for thread in threads {
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
            thread_display_title(&thread)
        ));
    }
    lines.push(
        "Use `/resume` to choose a thread from this workspace, or `/resume <partial_id>` to search."
            .to_string(),
    );

    inner.client.send_agent_text(lines.join("\n")).await;
    Ok(StopReason::EndTurn)
}
