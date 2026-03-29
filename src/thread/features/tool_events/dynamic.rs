//! Live/replay обработка dynamic tool-call item веток.

use agent_client_protocol::{
    Content, ContentBlock, ResourceLink, ToolCall, ToolCallContent, ToolCallId, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};
use codex_app_server_protocol::{DynamicToolCallOutputContentItem, DynamicToolCallStatus};
use serde_json::Value;

use crate::thread::features::status_mapping;
use crate::thread::{SessionClient, ThreadInner};

#[derive(Debug)]
pub(in crate::thread) struct ReplayDynamicToolCall {
    pub(in crate::thread) id: String,
    pub(in crate::thread) tool: String,
    pub(in crate::thread) arguments: Value,
    pub(in crate::thread) status: DynamicToolCallStatus,
    pub(in crate::thread) content_items: Option<Vec<DynamicToolCallOutputContentItem>>,
    pub(in crate::thread) success: Option<bool>,
    pub(in crate::thread) duration_ms: Option<i64>,
}

pub(in crate::thread) async fn emit_dynamic_tool_call_started(
    inner: &mut ThreadInner,
    id: String,
    tool: String,
    status: DynamicToolCallStatus,
    arguments: Value,
) {
    inner.started_tool_calls.insert(id.clone());
    inner
        .client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id), dynamic_tool_title(&tool))
                .kind(ToolKind::Execute)
                .status(status_mapping::map_dynamic_tool_status(status, true))
                .raw_input(dynamic_tool_raw_input(&tool, &arguments)),
        )
        .await;
}

pub(in crate::thread) async fn emit_dynamic_tool_call_completed(
    inner: &mut ThreadInner,
    id: String,
    status: DynamicToolCallStatus,
    content_items: Option<Vec<DynamicToolCallOutputContentItem>>,
    success: Option<bool>,
    duration_ms: Option<i64>,
) {
    let fields = dynamic_tool_update_fields(status, content_items, success, duration_ms);
    inner
        .client
        .send_tool_call_update(ToolCallUpdate::new(ToolCallId::new(id.clone()), fields))
        .await;
    inner.started_tool_calls.remove(&id);
}

pub(in crate::thread) async fn replay_dynamic_tool_call(
    client: &SessionClient,
    data: ReplayDynamicToolCall,
) {
    let ReplayDynamicToolCall {
        id,
        tool,
        arguments,
        status,
        content_items,
        success,
        duration_ms,
    } = data;

    client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id.clone()), dynamic_tool_title(&tool))
                .kind(ToolKind::Execute)
                .status(status_mapping::map_dynamic_tool_status(
                    status.clone(),
                    false,
                ))
                .raw_input(dynamic_tool_raw_input(&tool, &arguments)),
        )
        .await;

    client
        .send_tool_call_update(ToolCallUpdate::new(
            ToolCallId::new(id),
            dynamic_tool_update_fields(status, content_items, success, duration_ms),
        ))
        .await;
}

fn dynamic_tool_update_fields(
    status: DynamicToolCallStatus,
    content_items: Option<Vec<DynamicToolCallOutputContentItem>>,
    success: Option<bool>,
    duration_ms: Option<i64>,
) -> ToolCallUpdateFields {
    let content = dynamic_tool_content(content_items.as_deref(), success);
    let mut fields = ToolCallUpdateFields::new().status(status_mapping::map_dynamic_tool_status(
        status.clone(),
        false,
    ));
    if !content.is_empty() {
        fields = fields.content(content);
    }
    fields.raw_output(serde_json::json!({
        "status": status,
        "contentItems": content_items,
        "success": success,
        "durationMs": duration_ms,
    }))
}

fn dynamic_tool_content(
    content_items: Option<&[DynamicToolCallOutputContentItem]>,
    success: Option<bool>,
) -> Vec<ToolCallContent> {
    let mut content = content_items
        .unwrap_or_default()
        .iter()
        .map(|item| match item {
            DynamicToolCallOutputContentItem::InputText { text } => {
                ToolCallContent::Content(Content::new(text.clone()))
            }
            DynamicToolCallOutputContentItem::InputImage { image_url } => {
                ToolCallContent::Content(Content::new(ContentBlock::ResourceLink(
                    ResourceLink::new(image_url.clone(), image_url.clone()),
                )))
            }
        })
        .collect::<Vec<_>>();

    if content.is_empty() && success == Some(false) {
        content.push("Dynamic tool call failed.".to_string().into());
    }

    content
}

fn dynamic_tool_title(tool: &str) -> String {
    format!("Tool: {tool}")
}

fn dynamic_tool_raw_input(tool: &str, arguments: &Value) -> Value {
    serde_json::json!({
        "tool": tool,
        "arguments": arguments,
    })
}

#[cfg(test)]
mod tests {
    use super::dynamic_tool_content;
    use agent_client_protocol::{Content, ContentBlock, ResourceLink, ToolCallContent};
    use codex_app_server_protocol::DynamicToolCallOutputContentItem;

    #[test]
    fn dynamic_tool_content_maps_text_and_images() {
        let content = dynamic_tool_content(
            Some(&[
                DynamicToolCallOutputContentItem::InputText {
                    text: "ok".to_string(),
                },
                DynamicToolCallOutputContentItem::InputImage {
                    image_url: "https://example.com/image.png".to_string(),
                },
            ]),
            Some(true),
        );

        assert_eq!(
            content,
            vec![
                ToolCallContent::Content(Content::new("ok")),
                ToolCallContent::Content(Content::new(ContentBlock::ResourceLink(
                    ResourceLink::new(
                        "https://example.com/image.png",
                        "https://example.com/image.png",
                    ),
                ))),
            ]
        );
    }

    #[test]
    fn dynamic_tool_failed_without_items_gets_fallback_text() {
        let content = dynamic_tool_content(None, Some(false));
        assert_eq!(
            content,
            vec!["Dynamic tool call failed.".to_string().into()]
        );
    }
}
