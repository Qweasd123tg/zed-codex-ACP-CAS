//! `request_user_input` handling: question cards and answer mapping into ACP permissions.

use std::collections::HashMap;

use agent_client_protocol::{
    Error, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    SelectedPermissionOutcome, ToolCall, ToolCallId, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};
use codex_app_server_protocol::{
    ToolRequestUserInputAnswer, ToolRequestUserInputParams, ToolRequestUserInputQuestion,
    ToolRequestUserInputResponse,
};
use tracing::warn;

use crate::thread::{NONE_OF_THE_ABOVE, ThreadInner};

pub(in crate::thread) async fn handle_tool_request_user_input(
    inner: &mut ThreadInner,
    request_id: codex_app_server_protocol::RequestId,
    params: ToolRequestUserInputParams,
) -> Result<(), Error> {
    let raw_input = serde_json::to_value(&params).ok();
    let total_questions = params.questions.len();
    let mut answers = HashMap::new();
    let tool_call_id = ToolCallId::new(params.item_id.clone());
    inner
        .client
        .send_tool_call(
            ToolCall::new(tool_call_id.clone(), "Request user input")
                .kind(ToolKind::Think)
                .status(ToolCallStatus::Pending),
        )
        .await;

    for (question_index, question) in params.questions.iter().enumerate() {
        let (options, answer_labels_by_option_id, option_lines) =
            build_request_user_input_permission_options(question_index, question);
        if answer_labels_by_option_id.is_empty() {
            warn!(
                question_id = %question.id,
                "request_user_input question has no selectable options; skipping"
            );
            continue;
        }

        let mut content = Vec::new();
        if !question.question.trim().is_empty() {
            content.push(question.question.clone().into());
        }
        if !option_lines.is_empty() {
            content.push(format!("Options:\n{}", option_lines.join("\n")).into());
        }
        if question.is_secret {
            content.push("This answer is marked as secret.".to_string().into());
        }

        let title = if question.header.trim().is_empty() {
            format!("Plan input {}/{}", question_index + 1, total_questions)
        } else {
            format!(
                "{} ({}/{})",
                question.header.trim(),
                question_index + 1,
                total_questions
            )
        };

        let outcome = inner
            .client
            .request_permission(
                ToolCallUpdate::new(
                    tool_call_id.clone(),
                    ToolCallUpdateFields::new()
                        .title(title)
                        .kind(ToolKind::Think)
                        .status(ToolCallStatus::Pending)
                        .content(content)
                        .raw_input(raw_input.clone()),
                ),
                options,
            )
            .await?;

        let selected_option_id = match outcome {
            RequestPermissionOutcome::Cancelled => break,
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
                option_id.0.to_string()
            }
            _ => break,
        };

        if let Some(answer_label) = answer_labels_by_option_id.get(selected_option_id.as_str()) {
            answers.insert(
                question.id.clone(),
                ToolRequestUserInputAnswer {
                    answers: vec![answer_label.clone()],
                },
            );
        } else {
            warn!(
                question_id = %question.id,
                selected_option_id,
                "request_user_input selected unknown option id; skipping answer"
            );
        }
    }
    inner
        .client
        .send_tool_call_update(ToolCallUpdate::new(
            tool_call_id,
            ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
        ))
        .await;

    inner
        .app
        .send_tool_request_user_input_response(request_id, ToolRequestUserInputResponse { answers })
        .await
}

pub(in crate::thread) fn build_request_user_input_permission_options(
    _question_index: usize,
    question: &ToolRequestUserInputQuestion,
) -> (Vec<PermissionOption>, HashMap<String, String>, Vec<String>) {
    let mut answer_labels = Vec::new();
    let mut answer_labels_by_option_id = HashMap::new();
    let mut option_lines = Vec::new();

    if let Some(question_options) = &question.options {
        for option in question_options {
            answer_labels.push(option.label.clone());
            if option.description.trim().is_empty() {
                option_lines.push(format!("- {}", option.label));
            } else {
                option_lines.push(format!("- {}: {}", option.label, option.description.trim()));
            }
        }
    }

    if other_option_enabled_for_question(question) && answer_labels.len() < 3 {
        answer_labels.push(NONE_OF_THE_ABOVE.to_string());
        option_lines.push(format!("- {NONE_OF_THE_ABOVE}"));
    }

    if answer_labels.len() > 3 {
        warn!(
            question_id = %question.id,
            total_options = answer_labels.len(),
            "request_user_input has more than 3 options; truncating for ACP compatibility"
        );
        answer_labels.truncate(3);
    }

    let mut options = Vec::new();
    for (idx, answer_label) in answer_labels.into_iter().enumerate() {
        let option_id = format!("request-user-input-option-{}", idx + 1);
        answer_labels_by_option_id.insert(option_id.clone(), answer_label.clone());
        options.push(PermissionOption::new(
            option_id,
            answer_label,
            PermissionOptionKind::AllowOnce,
        ));
    }

    (options, answer_labels_by_option_id, option_lines)
}

fn other_option_enabled_for_question(question: &ToolRequestUserInputQuestion) -> bool {
    question.is_other
        && question
            .options
            .as_ref()
            .is_some_and(|options| !options.is_empty())
}
