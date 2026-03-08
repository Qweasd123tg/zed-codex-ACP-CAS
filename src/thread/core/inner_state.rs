//! Internal `Thread` state transitions that reset per-turn transient bookkeeping.

use crate::thread::session_config::{policy_to_mode, to_app_approval, to_app_sandbox_mode};
use crate::thread::{AppSandboxPolicy, EditApprovalMode, ModeKind, ThreadInner};
use codex_common::approval_presets::ApprovalPreset;
use tracing::warn;

impl ThreadInner {
    // Clear only ephemeral turn state; leave long-lived session caches intact.
    pub(super) fn reset_turn_transient_state(&mut self) {
        self.active_turn_id = None;
        self.active_turn_mode_kind = None;
        self.active_turn_saw_plan_item = false;
        self.active_turn_saw_plan_delta = false;
        self.started_tool_calls.clear();
        self.completed_turn_ids.clear();
        self.turn_plan_updates_seen.clear();
        self.fallback_plan = None;
        self.file_change_locations.clear();
        self.file_change_started_changes.clear();
        self.file_change_before_contents.clear();
        self.latest_turn_diff = None;
        self.file_change_paths_this_turn.clear();
        self.synced_paths_this_turn.clear();
        self.last_plan_steps.clear();
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

    pub(super) fn apply_mode_preset(
        &mut self,
        preset: &ApprovalPreset,
        edit_approval_mode: EditApprovalMode,
        collaboration_mode_kind: ModeKind,
    ) {
        self.approval_policy = to_app_approval(preset.approval);
        self.sandbox_policy = AppSandboxPolicy::from(preset.sandbox.clone());
        self.sandbox_mode = to_app_sandbox_mode(&preset.sandbox);
        self.edit_approval_mode = edit_approval_mode;
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
