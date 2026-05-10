//! Хелперы выполнения turn, связывающие prompt input с API жизненного цикла turn в app-server.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use super::{
    Error, ModeKind, PLAN_IMPLEMENTATION_NO_OPTION_ID, PLAN_IMPLEMENTATION_TITLE,
    PLAN_IMPLEMENTATION_TOOL_CALL_ID, PLAN_IMPLEMENTATION_YES_OPTION_ID, PermissionOption,
    PermissionOptionKind, RequestPermissionOutcome, SelectedPermissionOutcome, StopReason, Thread,
    ThreadInner, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
    TurnInterruptParams, TurnStartParams, UserInput, features::plan, notification_dispatch,
    session_config,
};
use codex_app_server_protocol::{
    CommandExecutionApprovalDecision, JSONRPCMessage, ReviewDelivery, ReviewStartParams,
    ReviewTarget, ServerNotification, ServerRequest, ThreadItem, TurnStatus,
};

use tracing::{info, warn};

const TURN_MESSAGE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
const RECONNECT_STALL_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(12);
const RECONNECT_SILENT_STALL_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(20);
const RECONNECT_STALL_WARNING_THRESHOLD: u32 = 5;
const RECONNECT_STALL_MESSAGE: &str = "Turn appears stuck after repeated reconnect failures. Ending this turn so the UI does not spin forever. Check network/auth and retry.";
const POST_TURN_NOTIFICATION_DRAIN_TIMEOUT: std::time::Duration =
    std::time::Duration::from_millis(200);

struct PendingTurnCommandApproval {
    turn_id: String,
    request_id: codex_app_server_protocol::RequestId,
    decisions_by_option_id: HashMap<String, CommandExecutionApprovalDecision>,
    outcome_fut: Pin<Box<dyn Future<Output = Result<RequestPermissionOutcome, Error>> + Send>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StallAbortKind {
    Reconnect,
}

fn classify_turn_stall_abort(
    warning_count: u32,
    retry_limit_hit: bool,
    since_last_progress: std::time::Duration,
) -> Option<StallAbortKind> {
    if (retry_limit_hit
        && warning_count >= RECONNECT_STALL_WARNING_THRESHOLD
        && since_last_progress >= RECONNECT_STALL_GRACE_PERIOD)
        || (warning_count >= 1 && since_last_progress >= RECONNECT_SILENT_STALL_GRACE_PERIOD)
    {
        return Some(StallAbortKind::Reconnect);
    }

    None
}

async fn maybe_abort_turn_stall(inner: &mut ThreadInner) -> Option<StopReason> {
    let abort_kind = classify_turn_stall_abort(
        inner.turn_reconnect_warning_count,
        inner.turn_reconnect_retry_limit_hit,
        inner.turn_last_progress_at.elapsed(),
    )?;

    let message = match abort_kind {
        StallAbortKind::Reconnect => RECONNECT_STALL_MESSAGE,
    };
    inner
        .client
        .send_system_message("error", "Turn stalled", message)
        .await;
    Some(StopReason::EndTurn)
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
    async fn finish_active_turn(
        &self,
        turn_id: &str,
        stop_reason: StopReason,
    ) -> Result<StopReason, Error> {
        {
            let mut inner = self.inner.lock().await;
            inner.finalize_active_turn(turn_id);
        }

        let drain_outcome = self
            .drain_post_turn_notifications_ext(turn_id, POST_TURN_NOTIFICATION_DRAIN_TIMEOUT)
            .await?;
        if drain_outcome.was_truncated() {
            warn!(
                turn_id,
                processed_messages = drain_outcome.processed(),
                outcome = ?drain_outcome,
                "post-turn transport drain stopped before the queue went quiet"
            );
        }

        let mut inner = self.inner.lock().await;
        crate::thread::features::session::events::flush_pending_thread_title_update(&mut inner)
            .await;
        Ok(stop_reason)
    }

    async fn maybe_finish_stalled_turn(
        &self,
        turn_id: &str,
        has_pending_command_approval: bool,
    ) -> Result<Option<StopReason>, Error> {
        if has_pending_command_approval {
            return Ok(None);
        }

        let stop_reason = {
            let mut inner = self.inner.lock().await;
            maybe_abort_turn_stall(&mut inner).await
        };
        let Some(stop_reason) = stop_reason else {
            return Ok(None);
        };

        Ok(Some(self.finish_active_turn(turn_id, stop_reason).await?))
    }

    async fn cancel_pending_command_approval(
        &self,
        pending: PendingTurnCommandApproval,
    ) -> Result<(), Error> {
        let app = {
            let inner = self.inner.lock().await;
            inner.app.clone()
        };
        app.lock()
            .await
            .send_command_approval_response(
                pending.request_id,
                codex_app_server_protocol::CommandExecutionRequestApprovalResponse {
                    decision: CommandExecutionApprovalDecision::Cancel,
                },
            )
            .await
    }

    async fn interrupt_active_turn_if_needed(&self) -> bool {
        let (app, thread_id, active_turn_id) = {
            let inner = self.inner.lock().await;
            let Some(active_turn_id) = inner.active_turn_id.clone() else {
                return false;
            };
            (inner.app.clone(), inner.thread_id.clone(), active_turn_id)
        };
        // Если backend отверг interrupt (unknown turn, уже завершён и т.д.), считать turn
        // "прерванным" нельзя: cancel UI иначе застрянет в Cancelling, а turn продолжит жить.
        match app
            .lock()
            .await
            .turn_interrupt(TurnInterruptParams {
                thread_id: thread_id.clone(),
                turn_id: active_turn_id.clone(),
            })
            .await
        {
            Ok(_) => true,
            Err(err) => {
                warn!(
                    thread_id = %thread_id,
                    turn_id = %active_turn_id,
                    error = %err,
                    "turn/interrupt rejected; reporting turn as not interrupted"
                );
                false
            }
        }
    }

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
            let service_tier = inner.service_tier;
            let effort = inner.reasoning_effort;
            let approval_policy = inner.approval_policy;
            let sandbox_policy = inner.sandbox_policy.clone();
            let collaboration_mode =
                plan::collaboration_mode_for_turn(collaboration_mode_kind, &model, effort);
            let turn_response = inner
                .app
                .lock()
                .await
                .turn_start(TurnStartParams {
                    thread_id,
                    input,
                    model: Some(model),
                    service_tier: session_config::service_tier_override_from_session(service_tier),
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
                .lock()
                .await
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
        let mut pending_command_approval: Option<PendingTurnCommandApproval> = None;
        let app = {
            let inner = self.inner.lock().await;
            inner.app.clone()
        };
        let message_inbox = {
            let app = app.lock().await;
            app.message_inbox()
        };

        loop {
            let watchdog = tokio::time::sleep(TURN_MESSAGE_POLL_INTERVAL);
            tokio::pin!(watchdog);
            tokio::select! {
                result = cancel_rx.changed() => {
                    if result.is_ok() && !interrupted {
                        if let Some(pending) = pending_command_approval.take() {
                            self.cancel_pending_command_approval(pending).await?;
                        }
                        interrupted = self.interrupt_active_turn_if_needed().await;
                    }
                }
                _ = &mut watchdog => {
                    if let Some(stop_reason) = self
                        .maybe_finish_stalled_turn(&turn_id, pending_command_approval.is_some())
                        .await?
                    {
                        return Ok(stop_reason);
                    }
                }
                approval_result = async {
                    let pending = pending_command_approval
                        .as_mut()
                        .expect("pending command approval should exist");
                    let outcome = pending.outcome_fut.as_mut().await;
                    (
                        pending.request_id.clone(),
                        pending.turn_id.clone(),
                        pending.decisions_by_option_id.clone(),
                        outcome,
                    )
                }, if pending_command_approval.is_some() => {
                    let (request_id, approval_turn_id, decisions_by_option_id, outcome) =
                        approval_result;
                    pending_command_approval = None;
                    let active_turn_matches = {
                        let inner = self.inner.lock().await;
                        inner.active_turn_id.as_deref() == Some(approval_turn_id.as_str())
                    };
                    let decision = if active_turn_matches {
                        crate::thread::features::approvals::command::command_approval_decision_from_outcome(
                            outcome?,
                            &decisions_by_option_id,
                        )
                    } else {
                        codex_app_server_protocol::CommandExecutionApprovalDecision::Cancel
                    };
                    app.lock().await.send_command_approval_response(
                        request_id,
                        codex_app_server_protocol::CommandExecutionRequestApprovalResponse {
                            decision,
                        },
                    ).await?;
                }
                message = async { crate::app_server::recv_message_from_inbox(&message_inbox).await }, if pending_command_approval.is_none() => {
                    let message = message?;
                    match self.handle_active_turn_message(message, &turn_id).await? {
                        ActiveTurnMessageOutcome::Continue => {}
                        ActiveTurnMessageOutcome::PendingCommandApproval(pending) => {
                            pending_command_approval = Some(pending);
                            continue;
                        }
                        ActiveTurnMessageOutcome::Stop(stop_reason) => {
                            return self.finish_active_turn(&turn_id, stop_reason).await;
                        }
                    }
                    if let Some(stop_reason) = self
                        .maybe_finish_stalled_turn(&turn_id, pending_command_approval.is_some())
                        .await?
                    {
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
    ) -> Result<ActiveTurnMessageOutcome, Error> {
        if let JSONRPCMessage::Notification(notification) = &message
            && let Ok(ServerNotification::ItemStarted(payload)) =
                ServerNotification::try_from(notification.clone())
            && payload.turn_id == turn_id
            && let ThreadItem::FileChange {
                id,
                changes,
                status,
            } = payload.item
        {
            let snapshot = {
                let mut inner = self.inner.lock().await;
                plan::maybe_advance_fallback_plan(
                    &mut inner,
                    turn_id,
                    crate::thread::FallbackPlanPhase::Implementing,
                )
                .await;
                crate::thread::features::file::events::prepare_file_change_started_snapshot(
                    &mut inner, id, changes, status,
                )
            };
            crate::thread::features::file::events::emit_file_change_started_snapshot(snapshot)
                .await;
            return Ok(ActiveTurnMessageOutcome::Continue);
        }

        if let JSONRPCMessage::Notification(notification) = &message
            && let Ok(ServerNotification::ItemCompleted(payload)) =
                ServerNotification::try_from(notification.clone())
            && payload.turn_id == turn_id
            && let ThreadItem::FileChange {
                id,
                changes,
                status,
            } = payload.item
        {
            let snapshot = {
                let mut inner = self.inner.lock().await;
                crate::thread::features::file::events::prepare_file_change_completed_snapshot(
                    &mut inner, id, changes, status,
                )
            };
            let tool_call_update =
                crate::thread::features::file::events::build_file_change_completed_update(
                    &snapshot,
                );
            let writeback_targets =
                crate::thread::features::file::events::collect_file_change_writeback_targets(
                    &snapshot,
                );

            snapshot
                .client
                .send_tool_call_update(tool_call_update)
                .await;

            for (path, content) in writeback_targets {
                let still_same_turn = {
                    let inner = self.inner.lock().await;
                    inner.active_turn_id.as_deref() == Some(turn_id)
                };
                if !still_same_turn {
                    break;
                }

                match snapshot.client.write_text_file(path.clone(), content).await {
                    Ok(()) => {
                        let mut inner = self.inner.lock().await;
                        if inner.active_turn_id.as_deref() == Some(turn_id) {
                            inner.synced_paths_this_turn.insert(path);
                        } else {
                            break;
                        }
                    }
                    Err(err) => {
                        warn!(
                            "Failed to sync file change into ACP buffer for {}: {err:?}",
                            path.display()
                        );
                    }
                }
            }

            return Ok(ActiveTurnMessageOutcome::Continue);
        }

        if let JSONRPCMessage::Notification(notification) = &message
            && let Ok(ServerNotification::TurnCompleted(payload)) =
                ServerNotification::try_from(notification.clone())
        {
            let turn = payload.turn;
            let status = turn.status.clone();
            let turn_error_message = if status == TurnStatus::Failed {
                turn.error.as_ref().map(|error| error.message.clone())
            } else {
                None
            };
            let (completion_disposition, diff_snapshot, client) = {
                let mut inner = self.inner.lock().await;
                let disposition = crate::thread::turn_state::register_turn_completion(
                    &mut inner.last_completed_turn_id,
                    turn_id,
                    &turn.id,
                );
                if disposition == crate::thread::turn_state::TurnCompletionDisposition::Accepted {
                    plan::maybe_advance_fallback_plan(
                        &mut inner,
                        turn_id,
                        crate::thread::FallbackPlanPhase::Done,
                    )
                    .await;
                    inner.mark_turn_progress();
                    if inner
                        .fallback_plan
                        .as_ref()
                        .is_some_and(|state| state.turn_id == turn_id)
                    {
                        inner.fallback_plan = None;
                    }
                    inner.turn_plan_updates_seen.remove(turn_id);
                }
                let diff_snapshot = if disposition
                    == crate::thread::turn_state::TurnCompletionDisposition::Accepted
                {
                    crate::thread::turn_diff::prepare_finalized_turn_diff_snapshot(
                        &mut inner, turn_id,
                    )
                } else {
                    None
                };
                (disposition, diff_snapshot, inner.client.clone())
            };

            match completion_disposition {
                crate::thread::turn_state::TurnCompletionDisposition::Accepted => {}
                crate::thread::turn_state::TurnCompletionDisposition::Duplicate => {
                    warn!(
                        turn_id = turn.id.as_str(),
                        "Ignoring duplicate turn completion notification"
                    );
                    return Ok(ActiveTurnMessageOutcome::Continue);
                }
                crate::thread::turn_state::TurnCompletionDisposition::UnexpectedTurnId => {
                    return Ok(ActiveTurnMessageOutcome::Continue);
                }
            }

            if let Some(snapshot) = diff_snapshot {
                let synced_paths =
                    crate::thread::turn_diff::emit_finalized_turn_diff_snapshot(snapshot).await;
                if !synced_paths.is_empty() {
                    let mut inner = self.inner.lock().await;
                    if inner.active_turn_id.as_deref() == Some(turn_id) {
                        inner.synced_paths_this_turn.extend(synced_paths);
                    }
                }
            }

            if let Some(turn_error_message) = turn_error_message {
                client
                    .send_system_message("error", "Turn failed", turn_error_message)
                    .await;
            }

            let stop_reason = match status {
                TurnStatus::Interrupted => StopReason::Cancelled,
                TurnStatus::Completed | TurnStatus::Failed | TurnStatus::InProgress => {
                    StopReason::EndTurn
                }
            };
            return Ok(ActiveTurnMessageOutcome::Stop(stop_reason));
        }

        if let JSONRPCMessage::Request(request) = &message
            && let Ok(ServerRequest::CommandExecutionRequestApproval { request_id, params }) =
                ServerRequest::try_from(request.clone())
        {
            let pending = {
                let inner = self.inner.lock().await;
                let pending = crate::thread::features::approvals::command::prepare_command_approval(
                    &inner, request_id, params,
                );
                let request_id = pending.request_id.clone();
                let turn_id = turn_id.to_string();
                let client = pending.client;
                let tool_call = pending.tool_call;
                let options = pending.options;
                let decisions_by_option_id = pending.decisions_by_option_id;
                PendingTurnCommandApproval {
                    turn_id,
                    request_id,
                    decisions_by_option_id,
                    outcome_fut: Box::pin(async move {
                        client.request_permission(tool_call, options).await
                    }),
                }
            };
            return Ok(ActiveTurnMessageOutcome::PendingCommandApproval(pending));
        }

        if let JSONRPCMessage::Request(request) = &message
            && let Ok(ServerRequest::FileChangeRequestApproval { request_id, params }) =
                ServerRequest::try_from(request.clone())
        {
            self.handle_file_change_approval_request_ext(request_id, params)
                .await?;
            return Ok(ActiveTurnMessageOutcome::Continue);
        }

        let mut inner = self.inner.lock().await;
        Ok(
            match notification_dispatch::handle_message(&mut inner, message, turn_id).await? {
                Some(stop_reason) => ActiveTurnMessageOutcome::Stop(stop_reason),
                None => ActiveTurnMessageOutcome::Continue,
            },
        )
    }
}

enum ActiveTurnMessageOutcome {
    Continue,
    PendingCommandApproval(PendingTurnCommandApproval),
    Stop(StopReason),
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
    use super::{StallAbortKind, classify_turn_stall_abort};

    #[test]
    fn aborts_after_reconnect_retry_limit_with_long_stall() {
        assert_eq!(
            classify_turn_stall_abort(5, true, std::time::Duration::from_secs(12)),
            Some(StallAbortKind::Reconnect)
        );
    }

    #[test]
    fn keeps_waiting_before_retry_limit_is_hit() {
        assert_eq!(
            classify_turn_stall_abort(4, false, std::time::Duration::from_secs(19)),
            None
        );
    }

    #[test]
    fn aborts_after_single_reconnect_warning_with_long_silence() {
        assert_eq!(
            classify_turn_stall_abort(1, false, std::time::Duration::from_secs(20)),
            Some(StallAbortKind::Reconnect)
        );
    }
}
