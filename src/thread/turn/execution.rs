//! Turn-execution helpers that bridge prompt input to the app-server turn lifecycle API.

use super::{
    Error, ModeKind, PLAN_IMPLEMENTATION_NO_OPTION_ID, PLAN_IMPLEMENTATION_TITLE,
    PLAN_IMPLEMENTATION_TOOL_CALL_ID, PLAN_IMPLEMENTATION_YES_OPTION_ID, PermissionOption,
    PermissionOptionKind, RequestPermissionOutcome, SelectedPermissionOutcome, StopReason,
    ThreadInner, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
    TurnInterruptParams, TurnStartParams, UserInput, features::plan, notification_dispatch,
};

use tracing::info;

// Send turn-start once, then stream item notifications until the final status arrives.
pub(super) async fn run_single_turn(
    inner: &mut ThreadInner,
    cancel_tx: &tokio::sync::watch::Sender<u64>,
    input: Vec<UserInput>,
    collaboration_mode_kind: ModeKind,
) -> Result<StopReason, Error> {
    inner.sync_sandbox_mode_from_policy("run_single_turn");
    let thread_id = inner.thread_id.clone();
    let model = inner.current_model.clone();
    let effort = inner.reasoning_effort;
    let approval_policy = inner.approval_policy;
    let sandbox_policy = inner.sandbox_policy.clone();
    let collaboration_mode =
        plan::collaboration_mode_for_turn(collaboration_mode_kind, &model, effort);
    let turn_response = inner
        .app
        .turn_start(TurnStartParams {
            thread_id,
            input,
            model: Some(model),
            effort: Some(effort),
            approval_policy: Some(approval_policy),
            sandbox_policy: Some(sandbox_policy),
            collaboration_mode,
            ..Default::default()
        })
        .await?;

    let turn_id = turn_response.turn.id;
    info!("Started turn {turn_id} for session {}", inner.session_id);
    inner.prepare_for_new_turn(&turn_id, collaboration_mode_kind);
    plan::initialize_fallback_plan_for_turn(inner, &turn_id, collaboration_mode_kind).await;

    let mut interrupted = false;
    let mut cancel_rx = cancel_tx.subscribe();

    loop {
        tokio::select! {
            result = cancel_rx.changed() => {
                if result.is_ok() && !interrupted
                    && let Some(active_turn_id) = inner.active_turn_id.clone()
                {
                    let thread_id = inner.thread_id.clone();
                    drop(inner.app.turn_interrupt(TurnInterruptParams {
                        thread_id,
                        turn_id: active_turn_id,
                    }).await);
                    interrupted = true;
                }
            }
            message = inner.app.next_message() => {
                let message = message?;
                if let Some(stop_reason) = notification_dispatch::handle_message(inner, message, &turn_id).await? {
                    inner.finalize_active_turn(&turn_id);
                    notification_dispatch::drain_post_turn_notifications(
                        inner,
                        &turn_id,
                        std::time::Duration::from_millis(200),
                    )
                    .await?;
                    return Ok(stop_reason);
                }
            }
        }
    }
}

pub(super) async fn prompt_plan_implementation(inner: &mut ThreadInner) -> Result<bool, Error> {
    let outcome = inner
        .client
        .request_permission(
            ToolCallUpdate::new(
                ToolCallId::new(PLAN_IMPLEMENTATION_TOOL_CALL_ID),
                ToolCallUpdateFields::new()
                    .title(PLAN_IMPLEMENTATION_TITLE)
                    .kind(ToolKind::Think)
                    .status(ToolCallStatus::Pending)
                    .content(vec![
                        "Switch to Default and start coding from the proposed plan?".into(),
                    ]),
            ),
            vec![
                PermissionOption::new(
                    PLAN_IMPLEMENTATION_YES_OPTION_ID,
                    "Yes, implement this plan",
                    PermissionOptionKind::AllowOnce,
                ),
                PermissionOption::new(
                    PLAN_IMPLEMENTATION_NO_OPTION_ID,
                    "No, stay in Plan mode",
                    PermissionOptionKind::RejectOnce,
                ),
            ],
        )
        .await?;

    Ok(matches!(
        outcome,
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. })
            if option_id.0.as_ref() == PLAN_IMPLEMENTATION_YES_OPTION_ID
    ))
}
