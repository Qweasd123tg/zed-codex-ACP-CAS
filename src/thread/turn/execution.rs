//! Хелперы выполнения turn, связывающие prompt input с API жизненного цикла turn в app-server.

use super::{
    Error, ModeKind, PLAN_IMPLEMENTATION_NO_OPTION_ID, PLAN_IMPLEMENTATION_TITLE,
    PLAN_IMPLEMENTATION_TOOL_CALL_ID, PLAN_IMPLEMENTATION_YES_OPTION_ID, PermissionOption,
    PermissionOptionKind, RequestPermissionOutcome, SelectedPermissionOutcome, StopReason, Thread,
    ThreadInner, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
    TurnInterruptParams, TurnStartParams, UserInput, features::plan, notification_dispatch,
};
use codex_app_server_protocol::{
    JSONRPCMessage, ReviewDelivery, ReviewStartParams, ReviewTarget, ServerRequest,
};

use tracing::info;

const TURN_MESSAGE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
const RECONNECT_STALL_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(12);
const RECONNECT_SILENT_STALL_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(20);
const RECONNECT_STALL_WARNING_THRESHOLD: u32 = 5;
const RECONNECT_STALL_MESSAGE: &str = "\n[error] Turn appears stuck after repeated reconnect failures. Ending this turn so the UI does not spin forever. Check network/auth and retry.";
const POST_TURN_NOTIFICATION_DRAIN_TIMEOUT: std::time::Duration =
    std::time::Duration::from_millis(200);

fn should_abort_reconnect_stall(
    warning_count: u32,
    retry_limit_hit: bool,
    since_last_progress: std::time::Duration,
) -> bool {
    (retry_limit_hit
        && warning_count >= RECONNECT_STALL_WARNING_THRESHOLD
        && since_last_progress >= RECONNECT_STALL_GRACE_PERIOD)
        || (warning_count >= 1 && since_last_progress >= RECONNECT_SILENT_STALL_GRACE_PERIOD)
}

async fn maybe_abort_reconnect_stall(inner: &mut ThreadInner) -> Option<StopReason> {
    if !should_abort_reconnect_stall(
        inner.turn_reconnect_warning_count,
        inner.turn_reconnect_retry_limit_hit,
        inner.turn_last_progress_at.elapsed(),
    ) {
        return None;
    }

    inner.client.send_agent_text(RECONNECT_STALL_MESSAGE).await;
    Some(StopReason::EndTurn)
}

async fn finalize_turn_and_drain(inner: &mut ThreadInner, turn_id: &str) -> Result<(), Error> {
    inner.finalize_active_turn(turn_id);
    notification_dispatch::drain_post_turn_notifications(
        inner,
        turn_id,
        POST_TURN_NOTIFICATION_DRAIN_TIMEOUT,
    )
    .await
}

async fn prepare_started_turn(
    inner: &mut ThreadInner,
    turn_id: &str,
    collaboration_mode_kind: ModeKind,
) {
    info!("Started turn {turn_id} for session {}", inner.session_id);
    inner.prepare_for_new_turn(turn_id, collaboration_mode_kind);
    plan::initialize_fallback_plan_for_turn(inner, turn_id, collaboration_mode_kind).await;
}

impl Thread {
    pub(super) async fn run_single_turn_ext(
        &self,
        input: Vec<UserInput>,
        collaboration_mode_kind: ModeKind,
    ) -> Result<StopReason, Error> {
        let turn_id = {
            let mut inner = self.inner.lock().await;
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
            prepare_started_turn(&mut inner, &turn_id, collaboration_mode_kind).await;
            turn_id
        };

        self.drive_active_turn_ext(turn_id).await
    }

    pub(super) async fn run_review_turn_ext(
        &self,
        target: ReviewTarget,
    ) -> Result<StopReason, Error> {
        let turn_id = {
            let mut inner = self.inner.lock().await;
            inner.sync_sandbox_mode_from_policy("run_review_turn");
            let collaboration_mode_kind = inner.collaboration_mode_kind;
            let thread_id = inner.thread_id.clone();
            let review_response = inner
                .app
                .review_start(ReviewStartParams {
                    thread_id,
                    target,
                    delivery: Some(ReviewDelivery::Inline),
                })
                .await?;

            let turn_id = review_response.turn.id;
            prepare_started_turn(&mut inner, &turn_id, collaboration_mode_kind).await;
            turn_id
        };

        self.drive_active_turn_ext(turn_id).await
    }

    async fn drive_active_turn_ext(&self, turn_id: String) -> Result<StopReason, Error> {
        let mut interrupted = false;
        let mut cancel_rx = self.cancel_tx.subscribe();

        loop {
            let watchdog = tokio::time::sleep(TURN_MESSAGE_POLL_INTERVAL);
            tokio::pin!(watchdog);
            tokio::select! {
                result = cancel_rx.changed() => {
                    if result.is_ok() && !interrupted {
                        let mut inner = self.inner.lock().await;
                        if let Some(active_turn_id) = inner.active_turn_id.clone() {
                            let thread_id = inner.thread_id.clone();
                            drop(inner.app.turn_interrupt(TurnInterruptParams {
                                thread_id,
                                turn_id: active_turn_id,
                            }).await);
                            interrupted = true;
                        }
                    }
                }
                _ = &mut watchdog => {
                    let stop_reason = {
                        let mut inner = self.inner.lock().await;
                        maybe_abort_reconnect_stall(&mut inner).await
                    };
                    if let Some(stop_reason) = stop_reason {
                        let mut inner = self.inner.lock().await;
                        finalize_turn_and_drain(&mut inner, &turn_id).await?;
                        return Ok(stop_reason);
                    }
                }
                message = async {
                    let mut inner = self.inner.lock().await;
                    inner.app.next_message().await
                } => {
                    let message = message?;
                    if let Some(stop_reason) = self.handle_active_turn_message(message, &turn_id).await? {
                        let mut inner = self.inner.lock().await;
                        finalize_turn_and_drain(&mut inner, &turn_id).await?;
                        return Ok(stop_reason);
                    }
                    let stop_reason = {
                        let mut inner = self.inner.lock().await;
                        maybe_abort_reconnect_stall(&mut inner).await
                    };
                    if let Some(stop_reason) = stop_reason {
                        let mut inner = self.inner.lock().await;
                        finalize_turn_and_drain(&mut inner, &turn_id).await?;
                        return Ok(stop_reason);
                    }
                }
            }
        }
    }

    async fn handle_active_turn_message(
        &self,
        message: JSONRPCMessage,
        turn_id: &str,
    ) -> Result<Option<StopReason>, Error> {
        if let JSONRPCMessage::Request(request) = &message
            && let Ok(ServerRequest::FileChangeRequestApproval { request_id, params }) =
                ServerRequest::try_from(request.clone())
        {
            self.handle_file_change_approval_request_ext(request_id, params)
                .await?;
            return Ok(None);
        }

        let mut inner = self.inner.lock().await;
        notification_dispatch::handle_message(&mut inner, message, turn_id).await
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

#[cfg(test)]
mod tests {
    use super::should_abort_reconnect_stall;

    #[test]
    fn aborts_after_reconnect_retry_limit_with_long_stall() {
        assert!(should_abort_reconnect_stall(
            5,
            true,
            std::time::Duration::from_secs(12)
        ));
    }

    #[test]
    fn keeps_waiting_before_retry_limit_is_hit() {
        assert!(!should_abort_reconnect_stall(
            4,
            false,
            std::time::Duration::from_secs(19)
        ));
    }

    #[test]
    fn aborts_after_single_reconnect_warning_with_long_silence() {
        assert!(should_abort_reconnect_stall(
            1,
            false,
            std::time::Duration::from_secs(20)
        ));
    }
}
