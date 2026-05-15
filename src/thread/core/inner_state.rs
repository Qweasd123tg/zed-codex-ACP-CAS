//! Внутренние переходы состояния Thread, которые сбрасывают транзиентный учёт на каждый turn.

use crate::thread::session_config::{policy_to_mode, to_app_approval, to_app_sandbox_mode};
use crate::thread::{AppSandboxPolicy, ModeKind, ThreadInner};
use codex_utils_approval_presets::ApprovalPreset;
use tracing::warn;

impl ThreadInner {
    // Очищаем только эфемерное состояние turn; долгоживущие кэши сессии не трогаем.
    pub(super) fn reset_turn_transient_state(&mut self) {
        self.active_turn_id = None;
        self.active_turn_mode_kind = None;
        self.active_turn_saw_plan_item = false;
        self.active_turn_saw_plan_delta = false;
        self.started_tool_calls.clear();
        self.last_completed_turn_id = None;
        self.last_turn_error_notice = None;
        self.turn_plan_updates_seen.clear();
        self.fallback_plan = None;
        self.file_change_locations.clear();
        self.file_change_started_changes.clear();
        self.file_change_before_contents.clear();
        self.latest_turn_diff = None;
        self.file_change_paths_this_turn.clear();
        self.synced_paths_this_turn.clear();
        self.last_plan_steps.clear();
        self.pending_thread_title_update = None;
        self.turn_last_progress_at = std::time::Instant::now();
        self.turn_reconnect_warning_count = 0;
        self.turn_reconnect_retry_limit_hit = false;
        self.turn_last_reconnect_progress = None;
        self.turn_reconnect_stall_notice_sent = false;
    }

    pub(super) fn prepare_for_new_turn(
        &mut self,
        turn_id: &str,
        collaboration_mode_kind: ModeKind,
    ) {
        if let Some(active_turn_id) = self.active_turn_id.as_deref()
            && active_turn_id != turn_id
        {
            warn!(
                previous_turn_id = active_turn_id,
                next_turn_id = turn_id,
                "Starting new turn while previous turn is still marked active"
            );
        }
        self.reset_turn_transient_state();
        self.active_turn_id = Some(turn_id.to_string());
        self.active_turn_mode_kind = Some(collaboration_mode_kind);
    }

    pub(super) fn finalize_active_turn(&mut self, turn_id: &str) {
        if self.active_turn_id.as_deref() != Some(turn_id) {
            warn!(
                active_turn_id = ?self.active_turn_id,
                finished_turn_id = turn_id,
                "Finalizing a turn that does not match the current active turn"
            );
        }
        self.active_turn_id = None;
        self.active_turn_mode_kind = None;
    }

    pub(super) fn mark_turn_progress(&mut self) {
        self.turn_last_progress_at = std::time::Instant::now();
        self.turn_reconnect_warning_count = 0;
        self.turn_reconnect_retry_limit_hit = false;
        self.turn_last_reconnect_progress = None;
        self.turn_reconnect_stall_notice_sent = false;
    }

    pub(super) fn note_reconnect_progress(&mut self, current: u32, total: u32) -> bool {
        self.turn_last_progress_at = std::time::Instant::now();
        let progress = (current, total);
        let is_new_progress = self.turn_last_reconnect_progress != Some(progress);
        if is_new_progress {
            self.turn_reconnect_warning_count = self.turn_reconnect_warning_count.saturating_add(1);
            self.turn_last_reconnect_progress = Some(progress);
        }
        if current >= total {
            self.turn_reconnect_retry_limit_hit = true;
        }
        is_new_progress
    }

    pub(super) fn mark_reconnect_stall_notice_sent(&mut self) -> bool {
        if self.turn_reconnect_stall_notice_sent {
            return false;
        }
        self.turn_reconnect_stall_notice_sent = true;
        true
    }

    pub(super) fn record_turn_error_notice(
        &mut self,
        turn_id: &str,
        message: impl AsRef<str>,
    ) -> Option<String> {
        record_turn_error_notice(&mut self.last_turn_error_notice, turn_id, message)
    }

    pub(super) fn apply_mode_preset(
        &mut self,
        preset: &ApprovalPreset,
        collaboration_mode_kind: ModeKind,
    ) {
        self.approval_policy = to_app_approval(preset.approval);
        self.sandbox_policy = AppSandboxPolicy::from(preset.sandbox.clone());
        self.sandbox_mode = to_app_sandbox_mode(&preset.sandbox);
        self.collaboration_mode_kind = collaboration_mode_kind;
        self.sync_sandbox_mode_from_policy("apply_mode_preset");
    }

    pub(super) fn sync_sandbox_mode_from_policy(&mut self, context: &str) {
        let expected_mode = policy_to_mode(&self.sandbox_policy);
        if self.sandbox_mode == expected_mode {
            return;
        }
        warn!(
            context,
            old_mode = ?self.sandbox_mode,
            new_mode = ?expected_mode,
            "Sandbox mode was inconsistent with stored sandbox policy; syncing mode"
        );
        self.sandbox_mode = expected_mode;
    }
}

fn record_turn_error_notice(
    last_notice: &mut Option<(String, String)>,
    turn_id: &str,
    message: impl AsRef<str>,
) -> Option<String> {
    let message = message.as_ref().trim();
    if message.is_empty() {
        return None;
    }

    if last_notice
        .as_ref()
        .is_some_and(|(seen_turn_id, seen_message)| {
            seen_turn_id == turn_id && seen_message == message
        })
    {
        return None;
    }

    *last_notice = Some((turn_id.to_string(), message.to_string()));
    Some(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::record_turn_error_notice;

    #[test]
    fn turn_error_notice_dedupes_same_message_for_same_turn() {
        let mut last_notice = None;
        assert_eq!(
            record_turn_error_notice(&mut last_notice, "turn-1", " limit reached "),
            Some("limit reached".to_string())
        );
        assert_eq!(
            record_turn_error_notice(&mut last_notice, "turn-1", "limit reached"),
            None
        );
        assert_eq!(
            record_turn_error_notice(&mut last_notice, "turn-2", "limit reached"),
            Some("limit reached".to_string())
        );
    }
}
