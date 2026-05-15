use super::*;

#[test]
fn request_user_input_options_include_none_of_the_above_when_supported() {
    let question = ToolRequestUserInputQuestion {
        id: "q1".to_string(),
        header: "Header".to_string(),
        question: "Question?".to_string(),
        is_other: true,
        is_secret: false,
        options: Some(vec![
            codex_app_server_protocol::ToolRequestUserInputOption {
                label: "Yes".to_string(),
                description: "Continue".to_string(),
            },
        ]),
    };

    let (options, answer_labels_by_option_id, _) =
        build_request_user_input_permission_options(0, &question);

    assert_eq!(options.len(), 2);
    assert_eq!(answer_labels_by_option_id.len(), 2);
    assert_eq!(options[0].kind, PermissionOptionKind::AllowOnce);
    assert_eq!(options[1].kind, PermissionOptionKind::AllowOnce);
    assert!(
        answer_labels_by_option_id
            .values()
            .any(|label| label == "Yes")
    );
    assert!(
        answer_labels_by_option_id
            .values()
            .any(|label| label == NONE_OF_THE_ABOVE)
    );
}

#[test]
fn request_user_input_options_do_not_add_none_of_the_above_without_base_options() {
    let question = ToolRequestUserInputQuestion {
        id: "q1".to_string(),
        header: "Header".to_string(),
        question: "Question?".to_string(),
        is_other: true,
        is_secret: false,
        options: None,
    };

    let (options, answer_labels_by_option_id, _) =
        build_request_user_input_permission_options(0, &question);

    assert!(options.is_empty());
    assert!(answer_labels_by_option_id.is_empty());
}
