//! Mode/sandbox mapping helper-ы для session_config.

use crate::thread::{
    APPROVAL_PRESETS, AUTO_MODE_ID, AppAskForApproval, AppSandboxMode, AppSandboxPolicy,
    AskForApproval, DEFAULT_SESSION_MODE_ID, ModeKind, PLAN_SESSION_MODE_ID, SandboxPolicy,
    SessionMode, SessionModeId, SessionModeState,
};

// Сатурируем signed-значения, чтобы избежать underflow при конвертации счётчиков протокола.
pub(in crate::thread) fn i64_to_u64_saturating(value: i64) -> u64 {
    if value <= 0 { 0 } else { value as u64 }
}

pub(in crate::thread) fn to_app_approval(policy: AskForApproval) -> AppAskForApproval {
    policy.into()
}

pub(in crate::thread) fn to_app_sandbox_mode(policy: &SandboxPolicy) -> AppSandboxMode {
    match policy {
        SandboxPolicy::ReadOnly { .. } => AppSandboxMode::ReadOnly,
        SandboxPolicy::WorkspaceWrite { .. } | SandboxPolicy::ExternalSandbox { .. } => {
            AppSandboxMode::WorkspaceWrite
        }
        SandboxPolicy::DangerFullAccess => AppSandboxMode::DangerFullAccess,
    }
}

pub(in crate::thread) fn policy_to_mode(policy: &AppSandboxPolicy) -> AppSandboxMode {
    match policy {
        AppSandboxPolicy::ReadOnly { .. } => AppSandboxMode::ReadOnly,
        AppSandboxPolicy::WorkspaceWrite { .. } | AppSandboxPolicy::ExternalSandbox { .. } => {
            AppSandboxMode::WorkspaceWrite
        }
        AppSandboxPolicy::DangerFullAccess => AppSandboxMode::DangerFullAccess,
    }
}

pub(in crate::thread) fn mode_state(collaboration_mode_kind: ModeKind) -> SessionModeState {
    let current_mode_id = SessionModeId::new(match collaboration_mode_kind {
        ModeKind::Plan => PLAN_SESSION_MODE_ID,
        _ => DEFAULT_SESSION_MODE_ID,
    });

    SessionModeState::new(
        current_mode_id,
        vec![
            SessionMode::new(DEFAULT_SESSION_MODE_ID, "Chat")
                .description("Standard back-and-forth coding mode."),
            SessionMode::new(PLAN_SESSION_MODE_ID, "Plan")
                .description("Plan-first mode with visible step tracking."),
        ],
    )
}

pub(in crate::thread) fn current_permission_mode_id(
    approval: AppAskForApproval,
    sandbox: AppSandboxMode,
) -> SessionModeId {
    let current = APPROVAL_PRESETS
        .iter()
        .find(|preset| {
            to_app_approval(preset.approval) == approval
                && to_app_sandbox_mode(&preset.sandbox) == sandbox
        })
        .unwrap_or_else(|| {
            APPROVAL_PRESETS
                .iter()
                .find(|preset| preset.id == "read-only")
                .expect("read-only preset should exist")
        });

    SessionModeId::new(current.id)
}

pub(in crate::thread) fn permission_modes() -> Vec<SessionMode> {
    let mut available_modes = Vec::new();
    if let Some(read_only_preset) = APPROVAL_PRESETS
        .iter()
        .find(|preset| preset.id == "read-only")
    {
        available_modes.push(
            SessionMode::new(read_only_preset.id, "Read only").description(
                "Read-only sandbox. Codex must ask before edits, writes, or network access.",
            ),
        );
    }
    if APPROVAL_PRESETS
        .iter()
        .any(|preset| preset.id == AUTO_MODE_ID)
    {
        available_modes.push(
            SessionMode::new(AUTO_MODE_ID, "Workspace")
                .description("Workspace-write sandbox. Codex can edit this workspace; network and outside-workspace writes still ask."),
        );
    }
    if let Some(full_access_preset) = APPROVAL_PRESETS
        .iter()
        .find(|preset| preset.id == "full-access")
    {
        available_modes.push(
            SessionMode::new(full_access_preset.id, "Full access")
                .description("No sandbox and no approval prompts for edits, commands, internet, or files outside this workspace."),
        );
    }
    available_modes
}
