//! Парсинг команд промпта и преобразование slash-команд в действия.

use std::path::Path;

use super::{DiffScope, Error, SessionCommand, StopReason, ThreadInner};
use crate::thread::features::{plan::parse_collaboration_mode, session};
use crate::thread::session_selector_preferences::SlashCommandPreferences;
use agent_client_protocol::schema::v1::{
    AvailableCommand, AvailableCommandInput, ContentBlock, EmbeddedResource,
    EmbeddedResourceResource, ResourceLink, TextResourceContents, UnstructuredCommandInput,
};
use codex_app_server_protocol::{ReviewTarget, UserInput};
use codex_protocol::config_types::ModeKind;

const INIT_COMMAND_PROMPT: &str = include_str!("prompt_for_init_command.md");

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
                detail: None,
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
    let text = extract_command_text(prompt)?;

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

    if let Some(rest) = text.strip_prefix("/rename") {
        let name = rest.trim();
        return Some(SessionCommand::Rename {
            name: (!name.is_empty()).then(|| name.to_string()),
        });
    }

    if let Some(rest) = slash_command_rest(text, "/fork") {
        return Some(SessionCommand::Fork {
            args: (!rest.is_empty()).then(|| rest.to_string()),
        });
    }

    if let Some(rest) = slash_command_rest(text, "/review") {
        return Some(SessionCommand::Review {
            instructions: (!rest.is_empty()).then(|| rest.to_string()),
        });
    }

    if let Some(rest) = slash_command_rest(text, "/init") {
        return Some(SessionCommand::Init {
            args: (!rest.is_empty()).then(|| rest.to_string()),
        });
    }

    if let Some(rest) = slash_command_rest(text, "/status") {
        return Some(SessionCommand::Status {
            args: (!rest.is_empty()).then(|| rest.to_string()),
        });
    }

    if let Some(rest) = slash_command_rest(text, "/archive") {
        let query = rest.trim();
        return Some(SessionCommand::Archive {
            thread_id: (!query.is_empty()).then(|| query.to_string()),
        });
    }

    if let Some(rest) = slash_command_rest(text, "/unarchive") {
        let query = rest.trim();
        return Some(SessionCommand::Unarchive {
            thread_id: (!query.is_empty()).then(|| query.to_string()),
        });
    }

    if let Some(rest) = slash_command_rest(text, "/diff") {
        return Some(parse_diff_args(rest));
    }

    let mut parts = text.split_whitespace();
    match parts.next()? {
        "/compact" => Some(SessionCommand::Compact),
        "/undo" => {
            let num_turns = parts
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|value| *value > 0)
                .unwrap_or(1);
            Some(SessionCommand::Undo { num_turns })
        }
        _ => None,
    }
}

fn parse_diff_args(rest: &str) -> SessionCommand {
    let mut scope = DiffScope::LastTurn;
    let mut paths: Vec<String> = Vec::new();
    let mut tokens = rest.split_whitespace().peekable();

    while let Some(token) = tokens.next() {
        match token {
            "--session" | "--all" => {
                scope = DiffScope::Session;
            }
            "--last" => {
                // Следующий токен ожидается как положительное число; иначе трактуем как путь.
                match tokens.peek().and_then(|value| value.parse::<u32>().ok()) {
                    Some(count) if count > 0 => {
                        tokens.next();
                        scope = if count == 1 {
                            DiffScope::LastTurn
                        } else {
                            DiffScope::LastN(count)
                        };
                    }
                    _ => {
                        // `--last` без числа: просто оставим scope как есть и трактуем как ошибку парса
                        // (форматтер обработает сообщение) — сохраняем токен как путь-подсказку.
                        paths.push(token.to_string());
                    }
                }
            }
            other => {
                paths.push(other.to_string());
            }
        }
    }

    SessionCommand::Diff { scope, paths }
}

fn extract_command_text(prompt: &[ContentBlock]) -> Option<&str> {
    for block in prompt {
        let ContentBlock::Text(text_block) = block else {
            continue;
        };

        let trimmed = text_block.text.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Берём первый непустой текстовый блок: ACP-клиенты могут дописывать
        // дополнительные текстовые блоки с контекстом, и это не должно ломать slash-команды.
        return trimmed.starts_with('/').then_some(trimmed);
    }

    None
}

#[derive(Debug, PartialEq)]
pub(super) enum CommandDispatchOutcome {
    Stop(StopReason),
    PromptOverride { prompt: String, mode_kind: ModeKind },
    ReviewStart(ReviewTarget),
}

// Применяем slash-команду к текущему session state и возвращаем действие для prompt-flow.
pub(super) async fn dispatch_session_command(
    inner: &mut ThreadInner,
    command: SessionCommand,
) -> Result<CommandDispatchOutcome, Error> {
    match command {
        SessionCommand::Init { args } => {
            if args.is_some() {
                inner
                    .client
                    .send_system_message("usage", "Usage", "`/init`")
                    .await;
                return Ok(CommandDispatchOutcome::Stop(StopReason::EndTurn));
            }

            Ok(CommandDispatchOutcome::PromptOverride {
                prompt: INIT_COMMAND_PROMPT.to_string(),
                mode_kind: ModeKind::Default,
            })
        }
        SessionCommand::Status { args } => {
            if args.is_some() {
                inner
                    .client
                    .send_system_message("usage", "Usage", "`/status`")
                    .await;
                return Ok(CommandDispatchOutcome::Stop(StopReason::EndTurn));
            }

            inner
                .client
                .send_system_message(
                    "status",
                    "Session status",
                    crate::thread::session_config::full_status_report(
                        &inner.workspace_cwd,
                        &inner.backend_cli_version,
                        &inner.account_status,
                        inner.total_token_usage.as_ref(),
                        inner.account_rate_limits.as_ref(),
                        &inner.display_maps,
                        &inner.session_mcp_summary,
                        &inner.session_skills_summary,
                        &inner.session_plugins_summary,
                    ),
                )
                .await;
            Ok(CommandDispatchOutcome::Stop(StopReason::EndTurn))
        }
        SessionCommand::Review { instructions } => {
            match session::review::handle_review_command(inner, instructions).await? {
                session::review::ReviewDispatch::Start(target) => {
                    Ok(CommandDispatchOutcome::ReviewStart(target))
                }
                session::review::ReviewDispatch::Stop(stop_reason) => {
                    Ok(CommandDispatchOutcome::Stop(stop_reason))
                }
            }
        }
        SessionCommand::Archive { .. } => {
            Err(Error::internal_error().data("archive should be handled directly in prompt flow"))
        }
        SessionCommand::Unarchive { thread_id } => Ok(CommandDispatchOutcome::Stop(
            session::controls::handle_unarchive_command(inner, thread_id).await?,
        )),
        SessionCommand::Compact => {
            Err(Error::internal_error().data("compact should be handled directly in prompt flow"))
        }
        SessionCommand::Undo { .. } => {
            Err(Error::internal_error().data("undo should be handled directly in prompt flow"))
        }
        SessionCommand::PlanMode { raw_value, mode } => Ok(CommandDispatchOutcome::Stop(
            session::modes::handle_plan_mode_command(inner, raw_value, mode).await?,
        )),
        SessionCommand::PlanPrompt { prompt } => Ok(CommandDispatchOutcome::PromptOverride {
            prompt,
            mode_kind: ModeKind::Plan,
        }),
        SessionCommand::Fork { .. } => {
            Err(Error::internal_error().data("fork should be handled directly in prompt flow"))
        }
        SessionCommand::Rename { name } => Ok(CommandDispatchOutcome::Stop(
            session::controls::handle_rename_command(inner, name).await?,
        )),
        SessionCommand::Diff { scope, paths } => Ok(CommandDispatchOutcome::Stop(
            crate::thread::features::session::diff::handle_diff_command(inner, scope, paths)
                .await?,
        )),
    }
}

pub(super) fn session_command_name(command: &SessionCommand) -> &'static str {
    match command {
        SessionCommand::Init { .. } => "init",
        SessionCommand::Status { .. } => "status",
        SessionCommand::Review { .. } => "review",
        SessionCommand::Archive { .. } => "archive",
        SessionCommand::Unarchive { .. } => "unarchive",
        SessionCommand::Compact => "compact",
        SessionCommand::Undo { .. } => "undo",
        SessionCommand::PlanMode { .. } | SessionCommand::PlanPrompt { .. } => "plan",
        SessionCommand::Fork { .. } => "fork",
        SessionCommand::Rename { .. } => "rename",
        SessionCommand::Diff { .. } => "diff",
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

pub(super) fn builtin_commands(slash_commands: &SlashCommandPreferences) -> Vec<AvailableCommand> {
    let mut commands = [
        AvailableCommand::new(
            "init",
            "Create an AGENTS.md file with instructions for Codex",
        ),
        AvailableCommand::new(
            "status",
            "Show session status, context, MCP, skills, plugins, and limits",
        ),
        AvailableCommand::new(
            "review",
            "Open a review preset picker, or use `/review <instructions>` for a custom review",
        )
        .input(AvailableCommandInput::Unstructured(
            UnstructuredCommandInput::new("optional custom review instructions"),
        )),
        AvailableCommand::new(
            "fork",
            "Fork the current backend thread and keep working in the fork inside this ACP session",
        ),
        AvailableCommand::new(
            "archive",
            "Archive the current thread or a matched thread so it disappears from normal lists",
        )
        .input(AvailableCommandInput::Unstructured(
            UnstructuredCommandInput::new("optional partial thread id"),
        )),
        AvailableCommand::new(
            "unarchive",
            "Restore an archived thread back into normal lists",
        )
        .input(AvailableCommandInput::Unstructured(
            UnstructuredCommandInput::new("optional partial thread id"),
        )),
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
            "plan",
            "Show/set plan mode (`on|off`) or run one-shot planning with `/plan <request>`",
        )
        .input(AvailableCommandInput::Unstructured(
            UnstructuredCommandInput::new("optional mode or request"),
        )),
        AvailableCommand::new("rename", "Rename the current thread").input(
            AvailableCommandInput::Unstructured(UnstructuredCommandInput::new("new thread name")),
        ),
        AvailableCommand::new(
            "diff",
            "Show the diff of the last turn, or pass `--session`, `--last N`, or path filters",
        )
        .input(AvailableCommandInput::Unstructured(
            UnstructuredCommandInput::new("optional flags and/or path filters"),
        )),
    ]
    .into_iter()
    .filter(|command| slash_commands.is_enabled(command.name.as_str()))
    .collect::<Vec<_>>();
    commands.sort_by_key(|command| slash_commands.command_order(command.name.as_str()));
    commands
}

fn slash_command_rest<'a>(text: &'a str, command: &str) -> Option<&'a str> {
    let rest = text.strip_prefix(command)?;
    if rest.is_empty() {
        return Some(rest);
    }

    rest.chars()
        .next()
        .filter(|ch| ch.is_whitespace())
        .map(|_| rest.trim())
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
