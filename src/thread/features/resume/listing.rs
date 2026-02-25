//! Вывод списка доступных thread для `/threads`.

use agent_client_protocol::{Error, StopReason};
use codex_app_server_protocol::{ThreadListParams, ThreadSortKey};

use crate::thread::ThreadInner;
use crate::thread::prompt_commands::normalize_preview;

// Получаем историю thread и рендерим компактный список для интерактивного /resume.
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
            "- `{}` | {} | cwd: `{}` | updated_at: {}",
            thread.id,
            normalize_preview(&thread.preview),
            thread.cwd.display(),
            thread.updated_at
        ));
    }
    lines.push(
        "Use `/resume` to choose a thread from this workspace, or `/resume <partial_id>` to search."
            .to_string(),
    );

    inner.client.send_agent_text(lines.join("\n")).await;
    Ok(StopReason::EndTurn)
}
