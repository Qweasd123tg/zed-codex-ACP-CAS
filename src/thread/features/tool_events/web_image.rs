//! Live/replay обработка web-search и image-view tool-call веток.

use std::{
    fs,
    path::{Path, PathBuf},
};

use agent_client_protocol::schema::v1::{
    Content, ContentBlock, ImageContent, ResourceLink, TextContent, ToolCall, ToolCallContent,
    ToolCallId, ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use tracing::warn;

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

// Публикуем старт генерации изображения как отдельный tool-call.
pub(in crate::thread) async fn emit_image_generation_started(
    inner: &mut ThreadInner,
    id: String,
    status: String,
) {
    inner.started_tool_calls.insert(id.clone());
    inner
        .client
        .send_tool_call(
            ToolCall::new(ToolCallId::new(id), "Image generation")
                .kind(ToolKind::Other)
                .status(image_generation_tool_status(&status))
                .raw_input(json!({ "status": status })),
        )
        .await;
}

// Закрываем генерацию изображения и отдаем ACP image block для Zed.
pub(in crate::thread) async fn emit_image_generation_completed(
    inner: &mut ThreadInner,
    id: String,
    status: String,
    revised_prompt: Option<String>,
    result: String,
) {
    let tool_status = image_generation_tool_status(&status);
    let saved_path = save_generated_image(&inner.cas_home, &id, result.as_str());
    emit_inline_generated_image(&inner.client, result.as_str()).await;
    let content = image_generation_content(
        revised_prompt.as_deref(),
        result.as_str(),
        saved_path.as_deref(),
    );
    let raw_output = image_generation_raw_output(
        &status,
        revised_prompt.as_deref(),
        &result,
        saved_path.as_deref(),
    );
    let title = image_generation_tool_title(saved_path.as_deref());

    if inner.started_tool_calls.remove(&id) {
        inner
            .client
            .send_tool_call_update(ToolCallUpdate::new(
                ToolCallId::new(id),
                ToolCallUpdateFields::new()
                    .title(title)
                    .status(tool_status)
                    .content(content)
                    .raw_output(raw_output),
            ))
            .await;
    } else {
        inner
            .client
            .send_tool_call(
                ToolCall::new(ToolCallId::new(id), title)
                    .kind(ToolKind::Other)
                    .status(tool_status)
                    .content(content)
                    .raw_output(raw_output),
            )
            .await;
    }
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

// Replay-рендер генерации изображения.
pub(in crate::thread) async fn replay_image_generation(
    client: &SessionClient,
    cas_home: &Path,
    id: String,
    status: String,
    revised_prompt: Option<String>,
    result: String,
) {
    let saved_path = save_generated_image(cas_home, &id, result.as_str());
    emit_inline_generated_image(client, result.as_str()).await;
    let raw_output = image_generation_raw_output(
        &status,
        revised_prompt.as_deref(),
        &result,
        saved_path.as_deref(),
    );
    client
        .send_tool_call(
            ToolCall::new(
                ToolCallId::new(id),
                image_generation_tool_title(saved_path.as_deref()),
            )
            .kind(ToolKind::Other)
            .status(image_generation_tool_status(&status))
            .content(image_generation_content(
                revised_prompt.as_deref(),
                result.as_str(),
                saved_path.as_deref(),
            ))
            .raw_output(raw_output),
        )
        .await;
}

fn image_generation_tool_status(status: &str) -> ToolCallStatus {
    match status {
        "" | "generating" | "in_progress" | "incomplete" => ToolCallStatus::InProgress,
        "failed" => ToolCallStatus::Failed,
        "completed" => ToolCallStatus::Completed,
        _ => ToolCallStatus::Completed,
    }
}

fn image_generation_content(
    revised_prompt: Option<&str>,
    result: &str,
    saved_path: Option<&Path>,
) -> Vec<ToolCallContent> {
    let mut content = Vec::new();

    if let Some(revised_prompt) = revised_prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        content.push(ToolCallContent::Content(Content::new(ContentBlock::Text(
            TextContent::new(format!("Revised prompt: {revised_prompt}")),
        ))));
    }

    if !result.is_empty() {
        content.push(ToolCallContent::Content(Content::new(ContentBlock::Image(
            image_content(result, saved_path),
        ))));
    }

    content
}

fn image_generation_raw_output(
    status: &str,
    revised_prompt: Option<&str>,
    result: &str,
    saved_path: Option<&Path>,
) -> serde_json::Value {
    json!({
        "status": status,
        "revised_prompt": revised_prompt,
        "image_base64_length": result.len(),
        "saved_path": saved_path.map(display_path),
    })
}

async fn emit_inline_generated_image(client: &SessionClient, result: &str) {
    if result.is_empty() {
        return;
    }
    let markdown = format!("\n\n![Generated image]({})\n", image_data_uri(result));
    client.send_agent_text(markdown).await;
}

fn image_content(result: &str, saved_path: Option<&Path>) -> ImageContent {
    let image = ImageContent::new(result, "image/png");
    match saved_path {
        Some(saved_path) => image.uri(file_uri(saved_path)),
        None => image,
    }
}

fn image_generation_tool_title(saved_path: Option<&Path>) -> String {
    match saved_path {
        Some(_) => "Generated image".to_string(),
        None => "Image generation".to_string(),
    }
}

fn save_generated_image(cas_home: &Path, id: &str, result: &str) -> Option<PathBuf> {
    if result.is_empty() || cas_home.as_os_str().is_empty() {
        return None;
    }
    let bytes = match decode_image_base64(result) {
        Ok(bytes) => bytes,
        Err(error) => {
            warn!("Failed to decode generated image {id}: {error}");
            return None;
        }
    };
    let path = generated_image_path(cas_home, id);
    if let Some(parent) = path.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        warn!(
            "Failed to create generated image directory {}: {error}",
            parent.display()
        );
        return None;
    }
    if let Err(error) = fs::write(&path, bytes) {
        warn!(
            "Failed to write generated image {}: {error}",
            path.display()
        );
        return None;
    }
    Some(path)
}

fn generated_image_path(cas_home: &Path, id: &str) -> PathBuf {
    cas_home
        .join("generated-images")
        .join(format!("{}.png", safe_image_id(id)))
}

fn safe_image_id(id: &str) -> String {
    let safe: String = id
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '_',
        })
        .collect();
    if safe.trim_matches('_').is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        safe
    }
}

fn decode_image_base64(result: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let payload = result
        .split_once(',')
        .filter(|(prefix, _)| prefix.starts_with("data:"))
        .map(|(_, payload)| payload)
        .unwrap_or(result);
    STANDARD.decode(payload)
}

fn image_data_uri(result: &str) -> String {
    if result.starts_with("data:image/") {
        result.to_string()
    } else {
        format!("data:image/png;base64,{result}")
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", percent_encode_path(&display_path(path)))
}

fn percent_encode_path(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::v1::{Content, ContentBlock, ImageContent, TextContent};

    use super::{
        decode_image_base64, file_uri, generated_image_path, image_data_uri,
        image_generation_content, image_generation_tool_status, percent_encode_path, safe_image_id,
    };
    use crate::thread::ToolCallStatus;

    #[test]
    fn image_generation_status_maps_backend_strings() {
        assert_eq!(image_generation_tool_status(""), ToolCallStatus::InProgress);
        assert_eq!(
            image_generation_tool_status("generating"),
            ToolCallStatus::InProgress
        );
        assert_eq!(
            image_generation_tool_status("completed"),
            ToolCallStatus::Completed
        );
        assert_eq!(
            image_generation_tool_status("failed"),
            ToolCallStatus::Failed
        );
    }

    #[test]
    fn image_generation_content_emits_revised_prompt_and_png() {
        let content = image_generation_content(Some(" A tiny blue square "), "Zm9v", None);

        assert_eq!(content.len(), 2);
        assert!(matches!(
            &content[0],
            crate::thread::ToolCallContent::Content(Content {
                content: ContentBlock::Text(TextContent { text, .. }),
                ..
            }) if text == "Revised prompt: A tiny blue square"
        ));
        assert!(matches!(
            &content[1],
            crate::thread::ToolCallContent::Content(Content {
                content: ContentBlock::Image(ImageContent {
                    data,
                    mime_type,
                    uri,
                    ..
                }),
                ..
            }) if data == "Zm9v" && mime_type == "image/png" && uri.is_none()
        ));
    }

    #[test]
    fn image_generation_content_includes_saved_image_uri_when_available() {
        let saved_path = std::path::Path::new("/tmp/codex acp/generated-images/ig-1.png");
        let content = image_generation_content(None, "Zm9v", Some(saved_path));

        assert_eq!(content.len(), 1);
        assert!(matches!(
            &content[0],
            crate::thread::ToolCallContent::Content(Content {
                content: ContentBlock::Image(ImageContent { uri, .. }),
                ..
            }) if uri.as_deref() == Some("file:///tmp/codex%20acp/generated-images/ig-1.png")
        ));
    }

    #[test]
    fn generated_image_helpers_decode_and_sanitize_paths() {
        assert_eq!(
            decode_image_base64("data:image/png;base64,Zm9v").unwrap(),
            b"foo"
        );
        assert_eq!(image_data_uri("Zm9v"), "data:image/png;base64,Zm9v");
        assert_eq!(
            image_data_uri("data:image/png;base64,Zm9v"),
            "data:image/png;base64,Zm9v"
        );
        assert_eq!(safe_image_id("../bad id"), "___bad_id");
        assert_eq!(
            generated_image_path(std::path::Path::new("/tmp/.codex-cas"), "ig:1"),
            std::path::Path::new("/tmp/.codex-cas")
                .join("generated-images")
                .join("ig_1.png")
        );
        assert_eq!(
            percent_encode_path("/tmp/zed codex app server/image 1.png"),
            "/tmp/zed%20codex%20app%20server/image%201.png"
        );
        assert_eq!(
            file_uri(std::path::Path::new("/tmp/zed codex/image.png")),
            "file:///tmp/zed%20codex/image.png"
        );
    }
}
