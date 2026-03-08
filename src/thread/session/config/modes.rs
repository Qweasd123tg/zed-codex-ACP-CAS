//! Mode and sandbox mapping helpers for `session_config`.

use crate::thread::{
    APPROVAL_PRESETS, AUTO_ASK_EDITS_MODE_ID, AUTO_MODE_ID, AppAskForApproval, AppModel,
    AppSandboxMode, AppSandboxPolicy, AskForApproval, EditApprovalMode, ModeKind, ModelId,
    ModelInfo, PLAN_SESSION_MODE_ID, SandboxPolicy, SessionMode, SessionModeId, SessionModeState,
    SessionModelState,
};

// Saturate signed values to avoid underflow when converting protocol counters.
pub(in crate::thread) fn i64_to_u64_saturating(value: i64) -> u64 {
    if value <= 0 { 0 } else { value as u64 }
}

pub(in crate::thread) fn to_app_approval(policy: AskForApproval) -> AppAskForApproval {
    match policy {
        AskForApproval::UnlessTrusted => AppAskForApproval::UnlessTrusted,
        AskForApproval::OnFailure => AppAskForApproval::OnFailure,
        AskForApproval::OnRequest => AppAskForApproval::OnRequest,
        AskForApproval::Never => AppAskForApproval::Never,
    }
}

pub(in crate::thread) fn to_app_sandbox_mode(policy: &SandboxPolicy) -> AppSandboxMode {
    match policy {
        SandboxPolicy::ReadOnly => AppSandboxMode::ReadOnly,
        SandboxPolicy::WorkspaceWrite { .. } | SandboxPolicy::ExternalSandbox { .. } => {
            AppSandboxMode::WorkspaceWrite
        }
        SandboxPolicy::DangerFullAccess => AppSandboxMode::DangerFullAccess,
    }
}

pub(in crate::thread) fn policy_to_mode(policy: &AppSandboxPolicy) -> AppSandboxMode {
    match policy {
        AppSandboxPolicy::ReadOnly => AppSandboxMode::ReadOnly,
        AppSandboxPolicy::WorkspaceWrite { .. } | AppSandboxPolicy::ExternalSandbox { .. } => {
            AppSandboxMode::WorkspaceWrite
        }
        AppSandboxPolicy::DangerFullAccess => AppSandboxMode::DangerFullAccess,
    }
}

pub(in crate::thread) fn mode_state(
    approval: AppAskForApproval,
    sandbox: AppSandboxMode,
    edit_approval_mode: EditApprovalMode,
    collaboration_mode_kind: ModeKind,
) -> SessionModeState {
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
    let current_mode_id = if collaboration_mode_kind == ModeKind::Plan {
        SessionModeId::new(PLAN_SESSION_MODE_ID)
    } else if current.id == AUTO_MODE_ID && edit_approval_mode == EditApprovalMode::AskEveryEdit {
        SessionModeId::new(AUTO_ASK_EDITS_MODE_ID)
    } else {
        SessionModeId::new(current.id)
    };

    let mut available_modes = Vec::new();
    for preset in APPROVAL_PRESETS.iter() {
        if preset.id == AUTO_MODE_ID {
            available_modes.push(
                SessionMode::new(AUTO_MODE_ID, preset.label).description(
                    "Default mode: file edits are auto-approved (Plan mode still asks).",
                ),
            );
            available_modes.push(
                SessionMode::new(AUTO_ASK_EDITS_MODE_ID, "Default (Ask on edits)")
                    .description("Default mode with confirmation popup for every file edit."),
            );
        } else {
            available_modes
                .push(SessionMode::new(preset.id, preset.label).description(preset.description));
        }
    }
    available_modes.push(SessionMode::new(PLAN_SESSION_MODE_ID, "Plan").description(
        "Plan-first mode with visible step tracking (uses Default sandbox/approval).",
    ));

    SessionModeState::new(current_mode_id, available_modes)
}

pub(in crate::thread) fn session_model_state(
    models: &[AppModel],
    current_model: &str,
) -> SessionModelState {
    let mut available_models = models
        .iter()
        .map(|model| {
            ModelInfo::new(ModelId::new(model.id.clone()), model.display_name.clone())
                .description(model.description.clone())
        })
        .collect::<Vec<_>>();

    let current_model_id = super::reasoning::find_model_for_current(models, current_model)
        .map(|model| model.id.clone())
        .unwrap_or_else(|| current_model.to_string());

    if !available_models
        .iter()
        .any(|model| model.model_id.0.as_ref() == current_model_id)
    {
        available_models.push(ModelInfo::new(
            ModelId::new(current_model_id.clone()),
            current_model_id.clone(),
        ));
    }

    SessionModelState::new(ModelId::new(current_model_id), available_models)
}
