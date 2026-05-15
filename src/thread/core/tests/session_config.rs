use super::*;

#[test]
fn parses_reasoning_effort_values() {
    assert_eq!(
        parse_reasoning_effort("medium"),
        Some(ReasoningEffort::Medium)
    );
    assert_eq!(parse_reasoning_effort("high"), Some(ReasoningEffort::High));
    assert_eq!(
        parse_reasoning_effort("xhigh"),
        Some(ReasoningEffort::XHigh)
    );
    assert_eq!(parse_reasoning_effort("invalid"), None);
}

#[test]
fn collaboration_mode_for_turn_is_explicit_for_default_mode() {
    let mode =
        collaboration_mode_for_turn(ModeKind::Default, "gpt-5.3-codex", ReasoningEffort::High)
            .expect("mode should always be explicit");

    assert_eq!(mode.mode, ModeKind::Default);
    assert_eq!(mode.settings.model, "gpt-5.3-codex");
    assert_eq!(mode.settings.reasoning_effort, Some(ReasoningEffort::High));
}

#[test]
fn collaboration_mode_for_turn_is_explicit_for_plan_mode() {
    let mode = collaboration_mode_for_turn(ModeKind::Plan, "gpt-5.3-codex", ReasoningEffort::XHigh)
        .expect("mode should always be explicit");

    assert_eq!(mode.mode, ModeKind::Plan);
    assert_eq!(mode.settings.model, "gpt-5.3-codex");
    assert_eq!(mode.settings.reasoning_effort, Some(ReasoningEffort::XHigh));
}

#[test]
fn mode_state_uses_backend_auto_mode_id() {
    let auto_preset = APPROVAL_PRESETS
        .iter()
        .find(|preset| preset.id == AUTO_MODE_ID)
        .expect("auto preset should exist");
    let current_mode_id = current_permission_mode_id(
        to_app_approval(auto_preset.approval),
        to_app_sandbox_mode(&auto_preset.sandbox),
    );
    assert_eq!(current_mode_id.0.as_ref(), AUTO_MODE_ID);
}

#[test]
fn mode_state_shows_read_only_when_not_current() {
    let modes = permission_modes();

    assert!(modes.iter().any(|mode| mode.id.0.as_ref() == "read-only"));
}

#[test]
fn mode_state_keeps_read_only_when_current() {
    let read_only_preset = APPROVAL_PRESETS
        .iter()
        .find(|preset| preset.id == "read-only")
        .expect("read-only preset should exist");
    let current_mode_id = current_permission_mode_id(
        to_app_approval(read_only_preset.approval),
        to_app_sandbox_mode(&read_only_preset.sandbox),
    );
    let modes = permission_modes();

    assert_eq!(current_mode_id.0.as_ref(), "read-only");
    assert!(modes.iter().any(|mode| mode.id.0.as_ref() == "read-only"));
}

#[test]
fn permission_modes_explain_full_access_sandbox_behavior() {
    let modes = permission_modes();

    let full_access = modes
        .iter()
        .find(|mode| mode.id.0.as_ref() == "full-access")
        .expect("full access mode should exist");
    assert_eq!(full_access.name, "Full access");
    assert_eq!(
        full_access.description.as_deref(),
        Some(
            "No sandbox and no approval prompts for edits, commands, internet, or files outside this workspace."
        )
    );
}

#[test]
fn mode_state_contains_only_default_and_plan() {
    let state = mode_state(ModeKind::Default);
    assert_eq!(state.current_mode_id.0.as_ref(), DEFAULT_SESSION_MODE_ID);
    assert_eq!(state.available_modes.len(), 2);
    assert_eq!(
        state.available_modes[0].id.0.as_ref(),
        DEFAULT_SESSION_MODE_ID
    );
    assert_eq!(state.available_modes[1].id.0.as_ref(), PLAN_SESSION_MODE_ID);
}

#[test]
fn leaving_plan_mode_clears_visible_plan_state() {
    assert!(should_clear_visible_plan_for_mode_change(
        ModeKind::Plan,
        ModeKind::Default,
        false,
    ));
    assert!(should_clear_visible_plan_for_mode_change(
        ModeKind::Default,
        ModeKind::Default,
        true,
    ));
}

#[test]
fn staying_in_plan_mode_preserves_visible_plan_state() {
    assert!(!should_clear_visible_plan_for_mode_change(
        ModeKind::Plan,
        ModeKind::Plan,
        true,
    ));
    assert!(!should_clear_visible_plan_for_mode_change(
        ModeKind::Default,
        ModeKind::Plan,
        true,
    ));
}

#[test]
fn app_sandbox_policy_from_preserves_workspace_write_settings() {
    let policy = SandboxPolicy::WorkspaceWrite {
        writable_roots: vec![],
        read_only_access: ReadOnlyAccess::FullAccess,
        network_access: true,
        exclude_tmpdir_env_var: true,
        exclude_slash_tmp: true,
    };

    assert_eq!(
        AppSandboxPolicy::from(policy),
        AppSandboxPolicy::WorkspaceWrite {
            writable_roots: vec![],
            read_only_access: AppReadOnlyAccess::FullAccess,
            network_access: true,
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        }
    );
}

#[test]
fn app_sandbox_policy_from_preserves_external_sandbox() {
    let policy = SandboxPolicy::ExternalSandbox {
        network_access: codex_protocol::protocol::NetworkAccess::Enabled,
    };

    assert_eq!(
        AppSandboxPolicy::from(policy),
        AppSandboxPolicy::ExternalSandbox {
            network_access: codex_app_server_protocol::NetworkAccess::Enabled
        }
    );
}

#[test]
fn policy_to_mode_maps_external_sandbox_to_workspace_mode() {
    let policy = AppSandboxPolicy::ExternalSandbox {
        network_access: codex_app_server_protocol::NetworkAccess::Restricted,
    };
    assert_eq!(policy_to_mode(&policy), AppSandboxMode::WorkspaceWrite);
}
