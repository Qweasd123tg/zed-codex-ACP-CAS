//! Модуль оркестрации Thread: общее состояние, подключение подмодулей и сессионные константы.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};

use agent_client_protocol::{
    AvailableCommandsUpdate, Client, ClientCapabilities, ConfigOptionUpdate, ContentChunk,
    CurrentModeUpdate, Diff, Error, ListSessionsResponse, LoadSessionResponse, ModelId, ModelInfo,
    PermissionOption, PermissionOptionKind, ReadTextFileRequest, RequestPermissionOutcome,
    RequestPermissionRequest, SelectedPermissionOutcome, SessionConfigId, SessionConfigOption,
    SessionConfigOptionCategory, SessionConfigSelectOption, SessionId, SessionMode, SessionModeId,
    SessionModeState, SessionModelState, SessionNotification, SessionUpdate, StopReason, ToolCall,
    ToolCallContent, ToolCallId, ToolCallLocation, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind, WriteTextFileRequest,
};
use codex_app_server_protocol::{
    AskForApproval as AppAskForApproval, ItemCompletedNotification, ItemStartedNotification,
    Model as AppModel, SandboxMode as AppSandboxMode, SandboxPolicy as AppSandboxPolicy,
    ServerRequest, ThreadItem, ThreadListParams, ThreadReadParams, ThreadResumeParams,
    ThreadSortKey, ThreadStartParams, Turn as AppTurn, TurnDiffUpdatedNotification,
    TurnInterruptParams, TurnStartParams, UserInput,
};
use codex_core::config::Config;
use codex_protocol::config_types::ModeKind;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::{AskForApproval, SandboxPolicy};
use codex_utils_approval_presets::{ApprovalPreset, builtin_approval_presets};
pub(super) use tracing::warn;

use crate::ACP_CLIENT;
use crate::app_server::AppServerProcess;

// Делим обработчики по подмодулям, чтобы корневой модуль оставался читаемым.
mod features;
#[path = "thread/core/inner_state.rs"]
mod inner_state;
#[path = "thread/core/item_handlers.rs"]
mod item_handlers;
#[path = "thread/notification/dispatch.rs"]
mod notification_dispatch;
#[path = "thread/prompt/commands.rs"]
mod prompt_commands;
#[path = "thread/prompt/flow.rs"]
mod prompt_flow;
#[path = "thread/core/protocol_contract.rs"]
mod protocol_contract;
#[path = "thread/core/replay.rs"]
mod replay;
#[path = "thread/core/server_requests.rs"]
mod server_requests;
#[path = "thread/session/client.rs"]
mod session_client;
#[path = "thread/session/config/mod.rs"]
mod session_config;
#[path = "thread/session/lifecycle.rs"]
mod session_lifecycle;
#[path = "thread/session/settings.rs"]
mod session_settings;
#[path = "thread/session/view.rs"]
mod session_view;
#[path = "thread/core/terminal_updates.rs"]
mod terminal_updates;
#[path = "thread/turn/diff.rs"]
mod turn_diff;
#[path = "thread/turn/execution.rs"]
mod turn_execution;
#[path = "thread/turn/notify.rs"]
mod turn_notify;
#[path = "thread/turn/state.rs"]
mod turn_state;
#[path = "thread/core/unified_diff.rs"]
mod unified_diff;

use self::features::file::changes::{
    file_change_to_preview_diff, file_change_tool_location, read_file_text,
};
use self::features::plan::{
    fallback_plan_can_enter_summarizing, fallback_plan_entries_for_steps, limit_plan_entries,
    maybe_advance_fallback_plan, plan_entries_all_pending, turn_plan_step_to_entry,
};
use self::features::tool_call_ui::kind::command_looks_like_verification;
use self::item_handlers::{handle_item_completed, handle_item_started};
use self::terminal_updates::{handle_command_output_delta, handle_terminal_interaction};
use self::turn_diff::{finalize_turn_diff, handle_turn_diff_updated};
use self::unified_diff::{apply_unified_diff_to_text, first_hunk_line, unified_diff_to_old_new};

// Пресеты подтверждений статичны и переиспользуются между сессиями без лишних аллокаций.
static APPROVAL_PRESETS: LazyLock<Vec<ApprovalPreset>> = LazyLock::new(builtin_approval_presets);

// Канонические id опций и лимиты для ACP-промптов и элементов plan-режима.
const ALLOW_ONCE: &str = "allow-once";
const REJECT_ONCE: &str = "reject-once";
const CANCEL_TURN: &str = "cancel-turn";
const NONE_OF_THE_ABOVE: &str = "None of the above";
const RESUME_CANCEL_OPTION_ID: &str = "resume-cancel";
const MAX_VISIBLE_PLAN_ENTRIES: usize = 6;
const PLAN_SESSION_MODE_ID: &str = "plan";
const AUTO_MODE_ID: &str = "auto";
const AUTO_ASK_EDITS_MODE_ID: &str = "auto-ask-edits";
const PLAN_IMPLEMENTATION_TOOL_CALL_ID: &str = "plan-implementation";
const PLAN_IMPLEMENTATION_YES_OPTION_ID: &str = "plan-implement-yes";
const PLAN_IMPLEMENTATION_NO_OPTION_ID: &str = "plan-implement-no";
const PLAN_IMPLEMENTATION_TITLE: &str = "Implement this plan?";
const PLAN_IMPLEMENTATION_PROMPT: &str = "Implement the plan.";
const DEV_NULL: &str = "/dev/null";
const TURN_DIFF_TOOL_CALL_PREFIX: &str = "turn-diff-";

// Публичный handle потока: оборачивает изменяемое состояние сессии и сигнал отмены.
pub struct Thread {
    inner: tokio::sync::Mutex<ThreadInner>,
    cancel_tx: tokio::sync::watch::Sender<u64>,
}

// Внутреннее изменяемое состояние, которое ведётся для одной ACP-сессии.
struct ThreadInner {
    session_id: SessionId,
    app: AppServerProcess,
    thread_id: String,
    workspace_cwd: PathBuf,
    client: SessionClient,
    approval_policy: AppAskForApproval,
    sandbox_policy: AppSandboxPolicy,
    sandbox_mode: AppSandboxMode,
    edit_approval_mode: EditApprovalMode,
    collaboration_mode_kind: ModeKind,
    current_model: String,
    reasoning_effort: ReasoningEffort,
    agent_labels: HashMap<String, features::collab::CollabAgentLabel>,
    compaction_in_progress: bool,
    last_used_tokens: Option<u64>,
    context_window_size: Option<u64>,
    models: Vec<AppModel>,
    active_turn_id: Option<String>,
    active_turn_mode_kind: Option<ModeKind>,
    active_turn_saw_plan_item: bool,
    active_turn_saw_plan_delta: bool,
    started_tool_calls: HashSet<String>,
    completed_turn_ids: HashSet<String>,
    turn_plan_updates_seen: HashSet<String>,
    fallback_plan: Option<FallbackPlanState>,
    file_change_locations: HashMap<String, Vec<PathBuf>>,
    file_change_started_changes: HashMap<String, Vec<codex_app_server_protocol::FileUpdateChange>>,
    file_change_before_contents: HashMap<String, HashMap<PathBuf, Option<String>>>,
    latest_turn_diff: Option<String>,
    file_change_paths_this_turn: HashSet<PathBuf>,
    synced_paths_this_turn: HashSet<PathBuf>,
    last_plan_steps: Vec<String>,
    carryover_plan_steps: Option<Vec<String>>,
    replay_turns: Vec<AppTurn>,
    turn_last_progress_at: std::time::Instant,
    turn_reconnect_warning_count: u32,
    turn_reconnect_retry_limit_hit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// Определяет, подтверждаются ли правки файлов автоматически или вручную.
enum EditApprovalMode {
    AutoApprove,
    AskEveryEdit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
enum FallbackPlanPhase {
    Planning = 0,
    Implementing = 1,
    Verifying = 2,
    Summarizing = 3,
    Done = 4,
}

#[derive(Clone, Debug)]
struct FallbackPlanState {
    turn_id: String,
    phase: FallbackPlanPhase,
    saw_tool_activity: bool,
    steps: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
// Slash-команды, которые распознаются до обычного выполнения промпта.
enum SessionCommand {
    Threads,
    Resume {
        thread_id: Option<String>,
        include_history: bool,
    },
    Archive {
        thread_id: Option<String>,
    },
    Unarchive {
        thread_id: Option<String>,
    },
    Compact,
    Undo {
        num_turns: u32,
    },
    Reasoning {
        raw_value: Option<String>,
        effort: Option<ReasoningEffort>,
    },
    PlanMode {
        raw_value: Option<String>,
        mode: Option<ModeKind>,
    },
    PlanPrompt {
        prompt: String,
    },
    Context,
    Rename {
        name: Option<String>,
    },
}

#[derive(Clone)]
// Адаптер ACP-клиента, привязанный к одному session id для исходящих событий.
struct SessionClient {
    session_id: SessionId,
    client: Arc<dyn Client>,
    client_capabilities: Arc<Mutex<ClientCapabilities>>,
    suppress_text_output: bool,
}

#[cfg(test)]
#[path = "thread/core/tests.rs"]
mod tests;
