//! Парсинг команд промпта и преобразование slash-команд в действия.

use std::path::Path;

use super::SessionCommand;
use crate::thread::features::plan::parse_collaboration_mode;
use crate::thread::session_config::parse_reasoning_effort;
use agent_client_protocol::{
    AvailableCommand, AvailableCommandInput, ContentBlock, EmbeddedResource,
    EmbeddedResourceResource, ResourceLink, TextResourceContents, UnstructuredCommandInput,
};
use codex_app_server_protocol::UserInput;

// Преобразуем ACP-блоки контента в обычные user input перед парсингом команд.
pub(super) fn build_prompt_items(prompt: Vec<ContentBlock>) -> Vec<UserInput> {
    prompt
        .into_iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text_block) => Some(UserInput::Text {
                text: text_block.text,
                text_elements: vec![],
            }),
            ContentBlock::Image(image_block) => Some(UserInput::Image {
                url: format!("data:{};base64,{}", image_block.mime_type, image_block.data),
            }),
            ContentBlock::ResourceLink(ResourceLink { name, uri, .. }) => Some(UserInput::Text {
                text: format_uri_as_link(Some(name), uri),
                text_elements: vec![],
            }),
            ContentBlock::Resource(EmbeddedResource {
                resource:
                    EmbeddedResourceResource::TextResourceContents(TextResourceContents {
                        text,
                        uri,
                        ..
                    }),
                ..
            }) => Some(UserInput::Text {
                text: format!(
                    "{}\n<context ref=\"{uri}\">\n{text}\n</context>",
                    format_uri_as_link(None, uri.clone())
                ),
                text_elements: vec![],
            }),
            ContentBlock::Audio(..) | ContentBlock::Resource(..) | _ => None,
        })
        .collect()
}

pub(super) fn parse_session_command(prompt: &[ContentBlock]) -> Option<SessionCommand> {
    let text = match prompt {
        [ContentBlock::Text(text)] => text.text.trim(),
        _ => return None,
    };

    if text == "/plan" || text.starts_with("/plan ") {
        let rest = text["/plan".len()..].trim();
        if rest.is_empty() {
            return Some(SessionCommand::PlanMode {
                raw_value: None,
                mode: None,
            });
        }

        let first = rest
            .split_whitespace()
            .next()
            .map(str::to_lowercase)
            .unwrap_or_default();
        let words = rest.split_whitespace().count();
        if words == 1
            && let Some(mode) = parse_collaboration_mode(&first)
        {
            return Some(SessionCommand::PlanMode {
                raw_value: Some(first),
                mode: Some(mode),
            });
        }

        return Some(SessionCommand::PlanPrompt {
            prompt: rest.to_string(),
        });
    }

    if let Some(rest) = text.strip_prefix("/resume") {
        let rest = rest.trim();
        return Some(SessionCommand::Resume {
            thread_id: if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
        });
    }

    let mut parts = text.split_whitespace();
    match parts.next()? {
        "/threads" => Some(SessionCommand::Threads),
        "/compact" => Some(SessionCommand::Compact),
        "/undo" => {
            let num_turns = parts
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|value| *value > 0)
                .unwrap_or(1);
            Some(SessionCommand::Undo { num_turns })
        }
        "/reasoning" | "/effort" => {
            let raw_value = parts.next().map(ToString::to_string);
            let effort = raw_value.as_deref().and_then(parse_reasoning_effort);
            Some(SessionCommand::Reasoning { raw_value, effort })
        }
        "/context" => Some(SessionCommand::Context),
        _ => None,
    }
}

pub(super) fn normalize_preview(preview: &str) -> String {
    let compact = preview.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        "(no preview)".to_string()
    } else if compact.chars().count() > 120 {
        let short = compact.chars().take(117).collect::<String>();
        format!("{short}...")
    } else {
        compact
    }
}

pub(super) fn builtin_commands() -> Vec<AvailableCommand> {
    vec![
        AvailableCommand::new("threads", "List saved Codex threads for this account"),
        AvailableCommand::new(
            "resume",
            "Resume a thread. Without args: pick from current workspace; with args: search by partial id",
        )
        .input(
            AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(
                "optional partial thread id",
            )),
        ),
        AvailableCommand::new(
            "compact",
            "Summarize the conversation to free context window",
        ),
        AvailableCommand::new("undo", "Rollback the most recent turn(s)").input(
            AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(
                "optional number of turns (default 1)",
            )),
        ),
        AvailableCommand::new(
            "reasoning",
            "Show or set reasoning effort (`none|minimal|low|medium|high|xhigh`)",
        )
        .input(AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(
            "optional effort value",
        ))),
        AvailableCommand::new(
            "plan",
            "Show/set plan mode (`on|off`) or run one-shot planning with `/plan <request>`",
        )
        .input(AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(
            "optional mode or request",
        ))),
        AvailableCommand::new("context", "Show current context window usage"),
    ]
}

pub(super) fn format_uri_as_link(name: Option<String>, uri: String) -> String {
    if let Some(name) = name
        && !name.is_empty()
    {
        format!("[@{name}]({uri})")
    } else if let Some(path) = uri.strip_prefix("file://") {
        let name = Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        format!("[@{name}]({uri})")
    } else if uri.starts_with("zed://") {
        let name = uri.split('/').next_back().unwrap_or(&uri);
        format!("[@{name}]({uri})")
    } else {
        uri
    }
}
