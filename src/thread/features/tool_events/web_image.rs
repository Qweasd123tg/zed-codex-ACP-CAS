//! Live/replay обработка web-search и image-view tool-call веток.

use agent_client_protocol::schema::{
    Content, ContentBlock, ResourceLink, ToolCall, ToolCallContent, ToolCallId, ToolCallLocation,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};

use crate::thread::{SessionClient, ThreadInner};

// Публикуем старт web-search tool-call.
pub(in crate::thread) async fn emit_web_search_started(
    inner: &mut ThreadInner,
    id: String,
    query: String,
) {
    inner.started_tool_calls.insert(id.clone());
    inner
        .client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id), format!("Search web: {query}"))
                .kind(ToolKind::Fetch)
                .status(ToolCallStatus::InProgress),
        )
        .await;
}

// Закрываем web-search tool-call.
pub(in crate::thread) async fn emit_web_search_completed(inner: &mut ThreadInner, id: String) {
    inner
        .client
        .send_tool_call_update(ToolCallUpdate::new(
            ToolCallId::new(id.clone()),
            ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
        ))
        .await;
    inner.started_tool_calls.remove(&id);
}

// Публикуем image-view как завершённый read-tool call.
pub(in crate::thread) async fn emit_image_view_started(
    inner: &mut ThreadInner,
    id: String,
    path: String,
) {
    inner
        .client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id), format!("View image {path}"))
                .kind(ToolKind::Read)
                .status(ToolCallStatus::Completed)
                .locations(vec![ToolCallLocation::new(path.clone())])
                .content(vec![ToolCallContent::Content(Content::new(
                    ContentBlock::ResourceLink(ResourceLink::new(path.clone(), path)),
                ))]),
        )
        .await;
}

// Replay-рендер web-search.
pub(in crate::thread) async fn replay_web_search(
    client: &SessionClient,
    id: String,
    query: String,
) {
    client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id), format!("Search web: {query}"))
                .kind(ToolKind::Fetch)
                .status(ToolCallStatus::Completed),
        )
        .await;
}

// Replay-рендер image-view.
pub(in crate::thread) async fn replay_image_view(client: &SessionClient, id: String, path: String) {
    client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id), format!("View image {path}"))
                .kind(ToolKind::Read)
                .status(ToolCallStatus::Completed)
                .locations(vec![ToolCallLocation::new(path.clone())])
                .content(vec![ToolCallContent::Content(Content::new(
                    ContentBlock::ResourceLink(ResourceLink::new(path.clone(), path)),
                ))]),
        )
        .await;
}
