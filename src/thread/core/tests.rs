//! Тесты модуля Thread для парсинга slash-команд, UI-форматирования и логики маппинга.

use super::features::approvals::user_input::build_request_user_input_permission_options;
use super::features::collab::CollabAgentLabel;
use super::features::collab::content::{
    collab_tool_content, collab_tool_raw_input, collab_tool_raw_output, format_collab_receivers,
};
use super::features::collab::render::{collab_tool_title, collab_tool_title_with_context};
use super::features::collab::status::{
    collab_agent_state_summary, collab_status_summary_line, map_collab_status,
};
use super::features::file::changes::{file_change_to_replay_diff, file_change_tool_location};
use super::features::plan::{
    collaboration_mode_for_turn, fallback_plan_can_enter_summarizing,
    fallback_plan_entries_for_steps, fallback_plan_should_advance, limit_plan_entries,
    parse_collaboration_mode, plan_entries_all_pending, plan_from_plan_item_text, plan_from_text,
    promote_first_pending_step, should_clear_visible_plan_for_mode_change,
};
use super::features::tool_call_ui::kind::{command_looks_like_verification, command_tool_kind};
use super::features::tool_call_ui::title::command_tool_title;
use super::prompt_commands::{builtin_commands, parse_session_command};
use super::session_config::{
    current_permission_mode_id, mode_state, parse_reasoning_effort, permission_modes,
    policy_to_mode, to_app_approval, to_app_sandbox_mode,
};
use super::turn_diff::parse_turn_unified_diff_files;
use super::unified_diff::{apply_unified_diff_to_text, unified_diff_to_old_new};
use super::{
    APPROVAL_PRESETS, AUTO_MODE_ID, DEFAULT_SESSION_MODE_ID, DiffScope, FallbackPlanPhase,
    FallbackPlanState, MAX_VISIBLE_PLAN_ENTRIES, NONE_OF_THE_ABOVE, PLAN_SESSION_MODE_ID,
    SessionCommand,
};
use crate::thread::session_selector_preferences::SlashCommandPreferences;
use agent_client_protocol::schema::v1::{
    Content, ContentBlock, PermissionOptionKind, Plan, PlanEntry, PlanEntryPriority,
    PlanEntryStatus, ResourceLink, ToolCallContent, ToolCallStatus, ToolKind,
};
use codex_app_server_protocol::{
    CollabAgentState, CollabAgentStatus, CollabAgentTool, CollabAgentToolCallStatus, CommandAction,
    PatchChangeKind, ReadOnlyAccess as AppReadOnlyAccess, SandboxMode as AppSandboxMode,
    SandboxPolicy as AppSandboxPolicy, ToolRequestUserInputQuestion,
};
use codex_protocol::config_types::ModeKind;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::{ReadOnlyAccess, SandboxPolicy};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[path = "tests/approvals.rs"]
mod approvals;
#[path = "tests/collab.rs"]
mod collab;
#[path = "tests/diffs.rs"]
mod diffs;
#[path = "tests/plan.rs"]
mod plan;
#[path = "tests/session_config.rs"]
mod session_config;
#[path = "tests/slash_commands.rs"]
mod slash_commands;
#[path = "tests/tool_call_ui.rs"]
mod tool_call_ui;
