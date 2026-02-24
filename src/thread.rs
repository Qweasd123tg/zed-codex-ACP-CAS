use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use agent_client_protocol::{
    AvailableCommand, AvailableCommandInput, AvailableCommandsUpdate, Client, ClientCapabilities,
    ConfigOptionUpdate, Content, ContentBlock, ContentChunk, Diff, EmbeddedResource,
    CurrentModeUpdate, EmbeddedResourceResource, Error, ListSessionsResponse, LoadSessionResponse,
    Meta, ModelId, ModelInfo, PermissionOption, PermissionOptionKind, Plan, PlanEntry,
    PlanEntryPriority, PlanEntryStatus, RequestPermissionOutcome, RequestPermissionRequest,
    ReadTextFileRequest,
    ResourceLink, SelectedPermissionOutcome, SessionConfigId,
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption, SessionId,
    SessionMode, SessionModeId, SessionModeState, SessionModelState, SessionNotification,
    SessionUpdate, StopReason, TextResourceContents, ToolCall, ToolCallContent, ToolCallId,
    ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
    UnstructuredCommandInput,
    WriteTextFileRequest,
};
use codex_app_server_protocol::{
    AskForApproval as AppAskForApproval, CommandExecutionApprovalDecision,
    CommandAction,
    CommandExecutionOutputDeltaNotification, CommandExecutionRequestApprovalParams,
    CommandExecutionRequestApprovalResponse, CommandExecutionStatus,
    FileChangeRequestApprovalParams, FileChangeRequestApprovalResponse,
    FileChangeOutputDeltaNotification, FileChangeApprovalDecision, ItemCompletedNotification,
    ItemStartedNotification, McpToolCallProgressNotification, McpToolCallStatus,
    Model as AppModel, PatchApplyStatus, PatchChangeKind, ReasoningSummaryTextDeltaNotification,
    ReasoningTextDeltaNotification, SandboxMode as AppSandboxMode,
    SandboxPolicy as AppSandboxPolicy, ServerNotification, ServerRequest,
    ToolRequestUserInputAnswer, ToolRequestUserInputParams, ToolRequestUserInputQuestion,
    ToolRequestUserInputResponse,
    TerminalInteractionNotification, ThreadCompactStartParams, ThreadItem, ThreadListParams,
    ThreadResumeParams, ThreadRollbackParams, ThreadSortKey, ThreadStartParams,
    ThreadTokenUsageUpdatedNotification, Turn as AppTurn, TurnInterruptParams, TurnPlanStep,
    TurnPlanStepStatus, TurnStartParams, TurnStatus, TurnDiffUpdatedNotification, UserInput,
};
use codex_common::approval_presets::{ApprovalPreset, builtin_approval_presets};
use codex_core::config::Config;
use codex_protocol::config_types::{CollaborationMode, ModeKind, Settings as CollaborationSettings};
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::{AskForApproval, SandboxPolicy};
use tracing::{error, info, warn};

use crate::ACP_CLIENT;
use crate::app_server::AppServerProcess;

mod protocol_contract;
mod turn_state;

static APPROVAL_PRESETS: LazyLock<Vec<ApprovalPreset>> = LazyLock::new(builtin_approval_presets);

const ALLOW_ONCE: &str = "allow-once";
const REJECT_ONCE: &str = "reject-once";
const CANCEL_TURN: &str = "cancel-turn";
const NONE_OF_THE_ABOVE: &str = "None of the above";
const RESUME_CANCEL_OPTION_ID: &str = "resume-cancel";
const RESUME_PICK_LIMIT: usize = 8;
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

pub struct Thread {
    inner: tokio::sync::Mutex<ThreadInner>,
    cancel_tx: tokio::sync::watch::Sender<u64>,
}

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
enum SessionCommand {
    Threads,
    Resume { thread_id: Option<String> },
    Compact,
    Undo { num_turns: u32 },
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
}

#[derive(Clone)]
struct SessionClient {
    session_id: SessionId,
    client: Arc<dyn Client>,
    client_capabilities: Arc<Mutex<ClientCapabilities>>,
    suppress_text_output: bool,
}

impl SessionClient {
    fn new(session_id: SessionId, client_capabilities: Arc<Mutex<ClientCapabilities>>) -> Self {
        Self {
            session_id,
            client: ACP_CLIENT.get().expect("Client should be set").clone(),
            client_capabilities,
            suppress_text_output: env_flag("CODEX_ACP_DEV_LOGS_WITHOUT_TEXT_OUTPUT"),
        }
    }

    fn supports_terminal_output(&self) -> bool {
        self.client_capabilities
            .lock()
            .unwrap()
            .meta
            .as_ref()
            .and_then(|meta| meta.get("terminal_output"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    fn supports_write_text_file(&self) -> bool {
        self.client_capabilities.lock().unwrap().fs.write_text_file
    }

    fn supports_read_text_file(&self) -> bool {
        self.client_capabilities.lock().unwrap().fs.read_text_file
    }

    async fn send_notification(&self, update: SessionUpdate) {
        if let Err(err) = self
            .client
            .session_notification(SessionNotification::new(self.session_id.clone(), update))
            .await
        {
            error!("Failed to send session notification: {err:?}");
        }
    }

    async fn send_agent_text(&self, text: impl Into<String>) {
        if self.suppress_text_output {
            return;
        }
        self.send_notification(SessionUpdate::AgentMessageChunk(ContentChunk::new(
            text.into().into(),
        )))
        .await;
    }

    async fn send_user_text(&self, text: impl Into<String>) {
        if self.suppress_text_output {
            return;
        }
        self.send_notification(SessionUpdate::UserMessageChunk(ContentChunk::new(
            text.into().into(),
        )))
        .await;
    }

    async fn send_agent_thought(&self, text: impl Into<String>) {
        if self.suppress_text_output {
            return;
        }
        self.send_notification(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
            text.into().into(),
        )))
        .await;
    }

    async fn send_tool_call(&self, tool_call: ToolCall) {
        self.send_notification(SessionUpdate::ToolCall(tool_call)).await;
    }

    async fn send_tool_call_update(&self, update: ToolCallUpdate) {
        self.send_notification(SessionUpdate::ToolCallUpdate(update)).await;
    }

    async fn request_permission(
        &self,
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
    ) -> Result<RequestPermissionOutcome, Error> {
        let response = self
            .client
            .request_permission(RequestPermissionRequest::new(
                self.session_id.clone(),
                tool_call,
                options,
            ))
            .await?;
        Ok(response.outcome)
    }

    async fn write_text_file(&self, path: PathBuf, content: String) -> Result<(), Error> {
        self.client
            .write_text_file(WriteTextFileRequest::new(self.session_id.clone(), path, content))
            .await?;
        Ok(())
    }

    async fn prime_file_snapshot(&self, path: PathBuf) -> Result<(), Error> {
        self.client
            .read_text_file(ReadTextFileRequest::new(self.session_id.clone(), path))
            .await?;
        Ok(())
    }

    async fn send_usage_update(&self, used: u64, size: u64) {
        self.send_notification(SessionUpdate::UsageUpdate(
            agent_client_protocol::UsageUpdate::new(used, size),
        ))
        .await;
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
}

impl ThreadInner {
    fn reset_turn_transient_state(&mut self) {
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
        self.carryover_plan_steps = None;
    }

    fn prepare_for_new_turn(&mut self, turn_id: &str, collaboration_mode_kind: ModeKind) {
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

    fn finalize_active_turn(&mut self, turn_id: &str) {
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

    fn apply_mode_preset(
        &mut self,
        preset: &ApprovalPreset,
        edit_approval_mode: EditApprovalMode,
        collaboration_mode_kind: ModeKind,
    ) {
        self.approval_policy = to_app_approval(preset.approval);
        self.sandbox_policy = to_app_sandbox_policy(&preset.sandbox);
        self.sandbox_mode = to_app_sandbox_mode(&preset.sandbox);
        self.edit_approval_mode = edit_approval_mode;
        self.collaboration_mode_kind = collaboration_mode_kind;
        self.sync_sandbox_mode_from_policy("apply_mode_preset");
    }

    fn sync_sandbox_mode_from_policy(&mut self, context: &str) {
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

impl Thread {
    pub async fn start_session(
        config: &Config,
        cwd: PathBuf,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
    ) -> Result<(SessionId, Self), Error> {
        let mut app = AppServerProcess::spawn("codex").await?;
        app.initialize("codex-acp-cas", "Codex ACP CAS").await?;

        let start = app
            .thread_start(ThreadStartParams {
                model: config.model.clone(),
                model_provider: Some(config.model_provider_id.clone()),
                cwd: Some(cwd.to_string_lossy().to_string()),
                approval_policy: Some(to_app_approval(*config.approval_policy.get())),
                sandbox: Some(to_app_sandbox_mode(config.sandbox_policy.get())),
                ..Default::default()
            })
            .await?;

        let session_id = SessionId::new(start.thread.id.clone());
        let models = app.model_list().await.map(|response| response.data).unwrap_or_default();
        let reasoning_effort =
            resolve_reasoning_effort(&models, &start.model, start.reasoning_effort);

        let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(0_u64);
        let thread = Thread {
            inner: tokio::sync::Mutex::new(ThreadInner {
                session_id: session_id.clone(),
                app,
                thread_id: start.thread.id,
                workspace_cwd: cwd,
                client: SessionClient::new(session_id.clone(), client_capabilities),
                approval_policy: start.approval_policy,
                sandbox_policy: start.sandbox.clone(),
                sandbox_mode: policy_to_mode(&start.sandbox),
                edit_approval_mode: EditApprovalMode::AutoApprove,
                collaboration_mode_kind: ModeKind::Default,
                current_model: start.model,
                reasoning_effort,
                compaction_in_progress: false,
                last_used_tokens: None,
                context_window_size: None,
                models,
                active_turn_id: None,
                active_turn_mode_kind: None,
                active_turn_saw_plan_item: false,
                active_turn_saw_plan_delta: false,
                started_tool_calls: HashSet::new(),
                completed_turn_ids: HashSet::new(),
                turn_plan_updates_seen: HashSet::new(),
                fallback_plan: None,
                file_change_locations: HashMap::new(),
                file_change_started_changes: HashMap::new(),
                file_change_before_contents: HashMap::new(),
                latest_turn_diff: None,
                file_change_paths_this_turn: HashSet::new(),
                synced_paths_this_turn: HashSet::new(),
                last_plan_steps: Vec::new(),
                carryover_plan_steps: None,
                replay_turns: vec![],
            }),
            cancel_tx,
        };

        Ok((session_id, thread))
    }

    pub async fn resume_session(
        session_id: SessionId,
        config: &Config,
        cwd: PathBuf,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
    ) -> Result<Self, Error> {
        let mut app = AppServerProcess::spawn("codex").await?;
        app.initialize("codex-acp-cas", "Codex ACP CAS").await?;

        let resume = app
            .thread_resume(ThreadResumeParams {
                thread_id: session_id.0.to_string(),
                model: config.model.clone(),
                model_provider: Some(config.model_provider_id.clone()),
                cwd: Some(cwd.to_string_lossy().to_string()),
                approval_policy: Some(to_app_approval(*config.approval_policy.get())),
                sandbox: Some(to_app_sandbox_mode(config.sandbox_policy.get())),
                ..Default::default()
            })
            .await?;

        let models = app.model_list().await.map(|response| response.data).unwrap_or_default();
        let reasoning_effort =
            resolve_reasoning_effort(&models, &resume.model, resume.reasoning_effort);
        let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(0_u64);

        Ok(Thread {
            inner: tokio::sync::Mutex::new(ThreadInner {
                session_id: session_id.clone(),
                app,
                thread_id: resume.thread.id,
                workspace_cwd: cwd,
                client: SessionClient::new(session_id, client_capabilities),
                approval_policy: resume.approval_policy,
                sandbox_policy: resume.sandbox.clone(),
                sandbox_mode: policy_to_mode(&resume.sandbox),
                edit_approval_mode: EditApprovalMode::AutoApprove,
                collaboration_mode_kind: ModeKind::Default,
                current_model: resume.model,
                reasoning_effort,
                compaction_in_progress: false,
                last_used_tokens: None,
                context_window_size: None,
                models,
                active_turn_id: None,
                active_turn_mode_kind: None,
                active_turn_saw_plan_item: false,
                active_turn_saw_plan_delta: false,
                started_tool_calls: HashSet::new(),
                completed_turn_ids: HashSet::new(),
                turn_plan_updates_seen: HashSet::new(),
                fallback_plan: None,
                file_change_locations: HashMap::new(),
                file_change_started_changes: HashMap::new(),
                file_change_before_contents: HashMap::new(),
                latest_turn_diff: None,
                file_change_paths_this_turn: HashSet::new(),
                synced_paths_this_turn: HashSet::new(),
                last_plan_steps: Vec::new(),
                carryover_plan_steps: None,
                replay_turns: resume.thread.turns,
            }),
            cancel_tx,
        })
    }

    pub async fn load(&self) -> Result<LoadSessionResponse, Error> {
        let mut inner = self.inner.lock().await;
        if let Ok(models) = inner.app.model_list().await {
            inner.models = models.data;
        }

        Ok(LoadSessionResponse::new()
            .models(session_model_state(&inner.models, &inner.current_model))
            .modes(Some(mode_state(
                inner.approval_policy,
                inner.sandbox_mode,
                inner.edit_approval_mode,
                inner.collaboration_mode_kind,
            )))
            .config_options(config_options(
                &inner.models,
                &inner.current_model,
                inner.reasoning_effort,
                usage_percent(inner.last_used_tokens, inner.context_window_size),
                inner.approval_policy,
                inner.sandbox_mode,
                inner.edit_approval_mode,
                inner.collaboration_mode_kind,
            )))
    }

    pub async fn config_options(&self) -> Result<Vec<SessionConfigOption>, Error> {
        let inner = self.inner.lock().await;
        Ok(config_options(
            &inner.models,
            &inner.current_model,
            inner.reasoning_effort,
            usage_percent(inner.last_used_tokens, inner.context_window_size),
            inner.approval_policy,
            inner.sandbox_mode,
            inner.edit_approval_mode,
            inner.collaboration_mode_kind,
        ))
    }

    pub async fn notify_config_options_update(&self) {
        let (client, options) = {
            let inner = self.inner.lock().await;
            (
                inner.client.clone(),
                config_options(
                    &inner.models,
                    &inner.current_model,
                    inner.reasoning_effort,
                    usage_percent(inner.last_used_tokens, inner.context_window_size),
                    inner.approval_policy,
                    inner.sandbox_mode,
                    inner.edit_approval_mode,
                    inner.collaboration_mode_kind,
                ),
            )
        };
        client
            .send_notification(SessionUpdate::ConfigOptionUpdate(
                ConfigOptionUpdate::new(options),
            ))
            .await;
    }

    pub async fn notify_current_mode_update(&self) {
        let (client, current_mode_id) = {
            let inner = self.inner.lock().await;
            (
                inner.client.clone(),
                mode_state(
                    inner.approval_policy,
                    inner.sandbox_mode,
                    inner.edit_approval_mode,
                    inner.collaboration_mode_kind,
                )
                .current_mode_id,
            )
        };
        client
            .send_notification(SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(
                current_mode_id,
            )))
            .await;
    }

    pub async fn notify_available_commands(&self) {
        let client = {
            let inner = self.inner.lock().await;
            inner.client.clone()
        };
        client
            .send_notification(SessionUpdate::AvailableCommandsUpdate(
                AvailableCommandsUpdate::new(builtin_commands()),
            ))
            .await;
    }

    pub async fn replay_loaded_history(&self) {
        let (client, workspace_cwd, turns) = {
            let mut inner = self.inner.lock().await;
            let turns = std::mem::take(&mut inner.replay_turns);
            (inner.client.clone(), inner.workspace_cwd.clone(), turns)
        };
        replay_turns(&client, &workspace_cwd, turns).await;
    }

    pub async fn prompt(&self, request: agent_client_protocol::PromptRequest) -> Result<StopReason, Error> {
        let command = parse_session_command(&request.prompt);
        let mut plan_prompt: Option<String> = None;
        let mut inner = self.inner.lock().await;
        drain_background_notifications(&mut inner).await?;
        if let Some(command) = command {
            match command {
                SessionCommand::Threads => return handle_threads_command(&mut inner).await,
                SessionCommand::Resume { thread_id } => {
                    return handle_resume_selector_command(&mut inner, thread_id.as_deref()).await;
                }
                SessionCommand::Compact => return handle_compact_command(&mut inner).await,
                SessionCommand::Undo { num_turns } => {
                    return handle_undo_command(&mut inner, num_turns).await;
                }
                SessionCommand::Reasoning { raw_value, effort } => {
                    return handle_reasoning_command(&mut inner, raw_value, effort).await;
                }
                SessionCommand::PlanMode { raw_value, mode } => {
                    return handle_plan_mode_command(&mut inner, raw_value, mode).await;
                }
                SessionCommand::PlanPrompt { prompt } => {
                    plan_prompt = Some(prompt);
                }
                SessionCommand::Context => return handle_context_command(&mut inner).await,
            }
        }

        if inner.compaction_in_progress {
            inner
                .client
                .send_agent_text(
                    "Context compaction is still running. Wait for \"Context compacted.\" and send your prompt again.",
                )
                .await;
            return Ok(StopReason::EndTurn);
        }

        let input = if let Some(prompt) = plan_prompt.as_ref() {
            build_prompt_items(vec![ContentBlock::from(prompt.clone())])
        } else {
            build_prompt_items(request.prompt)
        };
        if input.is_empty() {
            return Err(Error::invalid_params().data("prompt is empty"));
        }

        let collaboration_mode_kind = if plan_prompt.is_some() {
            ModeKind::Plan
        } else {
            inner.collaboration_mode_kind
        };
        let stop_reason =
            run_single_turn(&mut inner, &self.cancel_tx, input, collaboration_mode_kind).await?;

        if stop_reason == StopReason::EndTurn
            && collaboration_mode_kind == ModeKind::Plan
            && inner.active_turn_saw_plan_item
        {
            let implement_now = prompt_plan_implementation(&mut inner).await?;
            if implement_now {
                if !inner.last_plan_steps.is_empty() {
                    inner.carryover_plan_steps = Some(inner.last_plan_steps.clone());
                }
                inner.collaboration_mode_kind = ModeKind::Default;
                notify_mode_and_config_update(&inner).await;
                let implementation_input =
                    build_prompt_items(vec![ContentBlock::from(PLAN_IMPLEMENTATION_PROMPT)]);
                if !implementation_input.is_empty() {
                    inner
                        .client
                        .send_agent_text("Switching to default mode and implementing the plan.")
                        .await;
                    return run_single_turn(
                        &mut inner,
                        &self.cancel_tx,
                        implementation_input,
                        ModeKind::Default,
                    )
                    .await;
                }
            }
        }

        Ok(stop_reason)
    }

    pub async fn set_mode(&self, mode: SessionModeId) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        if mode.0.as_ref() == PLAN_SESSION_MODE_ID {
            let default_preset = APPROVAL_PRESETS
                .iter()
                .find(|preset| preset.id == AUTO_MODE_ID)
                .ok_or_else(Error::invalid_params)?;
            inner.apply_mode_preset(
                default_preset,
                EditApprovalMode::AutoApprove,
                ModeKind::Plan,
            );
            return Ok(());
        }

        if mode.0.as_ref() == AUTO_ASK_EDITS_MODE_ID {
            let default_preset = APPROVAL_PRESETS
                .iter()
                .find(|preset| preset.id == AUTO_MODE_ID)
                .ok_or_else(Error::invalid_params)?;
            inner.apply_mode_preset(
                default_preset,
                EditApprovalMode::AskEveryEdit,
                ModeKind::Default,
            );
            return Ok(());
        }

        let preset = APPROVAL_PRESETS
            .iter()
            .find(|preset| preset.id == mode.0.as_ref())
            .ok_or_else(Error::invalid_params)?;
        let edit_approval_mode = if preset.id == AUTO_MODE_ID {
            EditApprovalMode::AutoApprove
        } else {
            EditApprovalMode::AskEveryEdit
        };
        inner.apply_mode_preset(preset, edit_approval_mode, ModeKind::Default);
        Ok(())
    }

    pub async fn set_model(&self, model: ModelId) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        inner.current_model = model.0.to_string();
        inner.reasoning_effort = normalize_reasoning_effort_for_model(
            &inner.models,
            &inner.current_model,
            inner.reasoning_effort,
        );
        inner.last_used_tokens = None;
        inner.context_window_size = None;
        Ok(())
    }

    pub async fn set_reasoning_effort(&self, effort: ReasoningEffort) -> Result<(), Error> {
        let mut inner = self.inner.lock().await;
        if let Some(model) = find_model_for_current(&inner.models, &inner.current_model)
            && !model
                .supported_reasoning_efforts
                .iter()
                .any(|option| option.reasoning_effort == effort)
        {
            return Err(Error::invalid_params().data(format!(
                "Reasoning effort `{}` is not supported by model `{}`",
                reasoning_effort_value(effort),
                model.display_name,
            )));
        }
        inner.reasoning_effort = effort;
        Ok(())
    }

    pub async fn set_config_option(
        &self,
        config_id: SessionConfigId,
        value: agent_client_protocol::SessionConfigValueId,
    ) -> Result<(), Error> {
        match config_id.0.as_ref() {
            "mode" => self.set_mode(SessionModeId::new(value.0)).await,
            "model" => self.set_model(ModelId::new(value.0)).await,
            "reasoning_effort" => {
                let effort = parse_reasoning_effort(&value.0)
                    .ok_or_else(|| Error::invalid_params().data("Unsupported reasoning effort"))?;
                self.set_reasoning_effort(effort).await
            }
            _ => Err(Error::invalid_params().data("Unsupported config option")),
        }
    }

    pub async fn cancel(&self) -> Result<(), Error> {
        let current = *self.cancel_tx.borrow();
        self.cancel_tx
            .send(current.saturating_add(1))
            .map_err(|err| Error::internal_error().data(err.to_string()))
    }

    pub async fn list_sessions(
        _config: &Config,
        cwd: Option<PathBuf>,
        cursor: Option<String>,
    ) -> Result<ListSessionsResponse, Error> {
        let mut app = AppServerProcess::spawn("codex").await?;
        app.initialize("codex-acp-cas", "Codex ACP CAS").await?;

        let response = app
            .thread_list(ThreadListParams {
                cursor,
                limit: Some(25),
                sort_key: Some(ThreadSortKey::UpdatedAt),
                model_providers: None,
                source_kinds: None,
                archived: Some(false),
            })
            .await?;

        let sessions = response
            .data
            .into_iter()
            .filter_map(|thread| {
                if let Some(expected_cwd) = cwd.as_ref()
                    && thread.cwd != *expected_cwd
                {
                    return None;
                }

                Some(
                    agent_client_protocol::SessionInfo::new(
                        SessionId::new(thread.id),
                        thread.cwd,
                    )
                    .title(Some(thread.preview))
                    .updated_at(Some(thread.updated_at.to_string())),
                )
            })
            .collect();

        Ok(ListSessionsResponse::new(sessions).next_cursor(response.next_cursor))
    }
}

async fn run_single_turn(
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
    let collaboration_mode = collaboration_mode_for_turn(collaboration_mode_kind, &model, effort);
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
    initialize_fallback_plan_for_turn(inner, &turn_id, collaboration_mode_kind).await;

    let mut interrupted = false;
    let mut cancel_rx = cancel_tx.subscribe();

    loop {
        tokio::select! {
            result = cancel_rx.changed() => {
                if result.is_ok() && !interrupted {
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
            message = inner.app.next_message() => {
                let message = message?;
                if let Some(stop_reason) = handle_message(inner, message, &turn_id).await? {
                    inner.finalize_active_turn(&turn_id);
                    drain_post_turn_notifications(
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

async fn prompt_plan_implementation(inner: &mut ThreadInner) -> Result<bool, Error> {
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

async fn notify_mode_and_config_update(inner: &ThreadInner) {
    let current_mode_id = mode_state(
        inner.approval_policy,
        inner.sandbox_mode,
        inner.edit_approval_mode,
        inner.collaboration_mode_kind,
    )
    .current_mode_id;
    inner
        .client
        .send_notification(SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(
            current_mode_id,
        )))
        .await;
    notify_config_update(inner).await;
}

async fn notify_config_update(inner: &ThreadInner) {
    inner
        .client
        .send_notification(SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(
            config_options(
                &inner.models,
                &inner.current_model,
                inner.reasoning_effort,
                usage_percent(inner.last_used_tokens, inner.context_window_size),
                inner.approval_policy,
                inner.sandbox_mode,
                inner.edit_approval_mode,
                inner.collaboration_mode_kind,
            ),
        )))
        .await;
}

async fn handle_threads_command(inner: &mut ThreadInner) -> Result<StopReason, Error> {
    let response = inner
        .app
        .thread_list(ThreadListParams {
            cursor: None,
            limit: Some(20),
            sort_key: Some(ThreadSortKey::UpdatedAt),
            model_providers: None,
            source_kinds: None,
            archived: Some(false),
        })
        .await?;

    if response.data.is_empty() {
        inner
            .client
            .send_agent_text("No saved threads found. Create one prompt first.")
            .await;
        return Ok(StopReason::EndTurn);
    }

    let mut lines = vec!["Saved threads (newest first):".to_string()];
    for thread in response.data {
        lines.push(format!(
            "- `{}` | {} | cwd: `{}` | updated_at: {}",
            thread.id,
            normalize_preview(&thread.preview),
            thread.cwd.display(),
            thread.updated_at
        ));
    }
    lines.push(
        "Use `/resume` to choose a thread from this workspace, or `/resume <partial_id>` to search."
            .to_string(),
    );

    inner.client.send_agent_text(lines.join("\n")).await;
    Ok(StopReason::EndTurn)
}

async fn handle_resume_selector_command(
    inner: &mut ThreadInner,
    query: Option<&str>,
) -> Result<StopReason, Error> {
    let all_threads = inner
        .app
        .thread_list(ThreadListParams {
            cursor: None,
            limit: Some(100),
            sort_key: Some(ThreadSortKey::UpdatedAt),
            model_providers: None,
            source_kinds: None,
            archived: Some(false),
        })
        .await?
        .data;

    if all_threads.is_empty() {
        inner
            .client
            .send_agent_text("No saved threads found. Create one prompt first.")
            .await;
        return Ok(StopReason::EndTurn);
    }

    let normalized_query = query
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .map(ToString::to_string);

    if let Some(query) = normalized_query.as_deref()
        && all_threads.iter().any(|thread| thread.id == query)
    {
        return handle_resume_command(inner, query).await;
    }

    let candidates = if let Some(query) = normalized_query.as_deref() {
        let mut in_workspace = all_threads
            .iter()
            .filter(|thread| thread.cwd == inner.workspace_cwd && thread_matches_query(thread, query))
            .cloned()
            .collect::<Vec<_>>();
        if in_workspace.is_empty() {
            in_workspace = all_threads
                .iter()
                .filter(|thread| thread_matches_query(thread, query))
                .cloned()
                .collect::<Vec<_>>();
        }
        in_workspace
    } else {
        all_threads
            .iter()
            .filter(|thread| thread.cwd == inner.workspace_cwd)
            .cloned()
            .collect::<Vec<_>>()
    };

    if candidates.is_empty() {
        let message = if let Some(query) = normalized_query {
            format!(
                "No threads found for `{query}`.\nTry `/resume` for current workspace threads or `/threads` to list all."
            )
        } else {
            format!(
                "No saved threads for current workspace `{}`.\nUse `/threads` to list all threads.",
                inner.workspace_cwd.display()
            )
        };
        inner.client.send_agent_text(message).await;
        return Ok(StopReason::EndTurn);
    }

    if candidates.len() == 1 {
        return handle_resume_command(inner, &candidates[0].id).await;
    }

    show_resume_picker(inner, candidates, normalized_query.as_deref()).await
}

async fn show_resume_picker(
    inner: &mut ThreadInner,
    mut candidates: Vec<codex_app_server_protocol::Thread>,
    query: Option<&str>,
) -> Result<StopReason, Error> {
    let total = candidates.len();
    candidates.truncate(RESUME_PICK_LIMIT);

    let title = match query {
        Some(query) => format!("Resume thread for `{query}`"),
        None => "Resume thread from current workspace".to_string(),
    };

    let mut lines = Vec::new();
    lines.push(format!("Select a thread to resume ({total} match(es)):"));
    if total > RESUME_PICK_LIMIT {
        lines.push(format!(
            "Showing the newest {RESUME_PICK_LIMIT} matches. Narrow with `/resume <partial_id>` if needed."
        ));
    }
    for thread in &candidates {
        lines.push(format!(
            "- `{}` | {} | cwd: `{}` | updated_at: {}",
            thread.id,
            normalize_preview(&thread.preview),
            thread.cwd.display(),
            thread.updated_at
        ));
    }

    let mut options = Vec::new();
    let mut id_by_option = HashMap::new();
    for (idx, thread) in candidates.into_iter().enumerate() {
        let option_id = format!("resume-thread-{}", idx + 1);
        let label = format!(
            "{} · {}",
            shorten_thread_id(&thread.id),
            normalize_preview(&thread.preview)
        );
        options.push(PermissionOption::new(
            option_id.clone(),
            label,
            PermissionOptionKind::AllowOnce,
        ));
        id_by_option.insert(option_id, thread.id);
    }
    options.push(PermissionOption::new(
        RESUME_CANCEL_OPTION_ID,
        "Cancel",
        PermissionOptionKind::RejectOnce,
    ));

    let outcome = inner
        .client
        .request_permission(
            ToolCallUpdate::new(
                ToolCallId::new("resume-selector"),
                ToolCallUpdateFields::new()
                    .title(title)
                    .kind(ToolKind::Think)
                    .status(ToolCallStatus::Pending)
                    .content(vec![lines.join("\n").into()]),
            ),
            options,
        )
        .await?;

    let selected_option_id = match outcome {
        RequestPermissionOutcome::Cancelled => {
            inner.client.send_agent_text("Resume cancelled.").await;
            return Ok(StopReason::EndTurn);
        }
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            option_id.0.to_string()
        }
        _ => {
            inner.client.send_agent_text("Resume cancelled.").await;
            return Ok(StopReason::EndTurn);
        }
    };

    if selected_option_id == RESUME_CANCEL_OPTION_ID {
        inner.client.send_agent_text("Resume cancelled.").await;
        return Ok(StopReason::EndTurn);
    }

    let Some(selected_thread_id) = id_by_option.get(&selected_option_id).cloned() else {
        warn!(selected_option_id, "resume selector returned unknown option id");
        inner
            .client
            .send_agent_text("Could not resolve selected thread. Run `/resume` again.")
            .await;
        return Ok(StopReason::EndTurn);
    };

    handle_resume_command(inner, &selected_thread_id).await
}

fn thread_matches_query(thread: &codex_app_server_protocol::Thread, query: &str) -> bool {
    if thread.id.contains(query) {
        return true;
    }
    let needle = query.to_lowercase();
    thread.preview.to_lowercase().contains(&needle)
}

fn shorten_thread_id(thread_id: &str) -> String {
    if thread_id.chars().count() <= 12 {
        thread_id.to_string()
    } else {
        format!("{}…", thread_id.chars().take(12).collect::<String>())
    }
}

async fn handle_resume_command(inner: &mut ThreadInner, thread_id: &str) -> Result<StopReason, Error> {
    let resume = inner
        .app
        .thread_resume(ThreadResumeParams {
            thread_id: thread_id.to_string(),
            ..Default::default()
        })
        .await?;

    inner.thread_id = resume.thread.id.clone();
    inner.workspace_cwd = resume.thread.cwd.clone();
    inner.approval_policy = resume.approval_policy;
    inner.sandbox_policy = resume.sandbox.clone();
    inner.sandbox_mode = policy_to_mode(&resume.sandbox);
    inner.sync_sandbox_mode_from_policy("handle_resume_command");
    inner.current_model = resume.model;
    inner.compaction_in_progress = false;
    inner.last_used_tokens = None;
    inner.context_window_size = None;
    inner.reset_turn_transient_state();

    if let Ok(models) = inner.app.model_list().await {
        inner.models = models.data;
    }
    inner.reasoning_effort =
        resolve_reasoning_effort(&inner.models, &inner.current_model, resume.reasoning_effort);

    let workspace_cwd = inner.workspace_cwd.clone();
    replay_turns(&inner.client, &workspace_cwd, resume.thread.turns).await;
    notify_config_update(inner).await;

    inner
        .client
        .send_agent_text(format!(
            "Resumed thread `{}`.\nPreview: {}\nNow continue chatting; context is loaded.",
            resume.thread.id,
            normalize_preview(&resume.thread.preview),
        ))
        .await;

    Ok(StopReason::EndTurn)
}

async fn handle_compact_command(inner: &mut ThreadInner) -> Result<StopReason, Error> {
    if inner.compaction_in_progress {
        inner
            .client
            .send_agent_text("Context compaction is already running.")
            .await;
        return Ok(StopReason::EndTurn);
    }

    inner
        .app
        .thread_compact_start(ThreadCompactStartParams {
            thread_id: inner.thread_id.clone(),
        })
        .await?;
    inner.compaction_in_progress = true;
    // Token usage can stay stale (often 100%) until the next completed model turn.
    // Clear cached usage right after /compact to avoid misleading context percentage.
    inner.last_used_tokens = None;
    notify_config_update(inner).await;
    inner
        .client
        .send_agent_text(
            "Context compaction started. Wait for \"Context compacted.\" before sending the next prompt.",
        )
        .await;
    Ok(StopReason::EndTurn)
}

async fn handle_undo_command(inner: &mut ThreadInner, num_turns: u32) -> Result<StopReason, Error> {
    let response = inner
        .app
        .thread_rollback(ThreadRollbackParams {
            thread_id: inner.thread_id.clone(),
            num_turns,
        })
        .await?;

    let workspace_cwd = inner.workspace_cwd.clone();
    replay_turns(&inner.client, &workspace_cwd, response.thread.turns).await;
    inner
        .client
        .send_agent_text(format!("Rolled back last {num_turns} turn(s)."))
        .await;
    Ok(StopReason::EndTurn)
}

async fn handle_reasoning_command(
    inner: &mut ThreadInner,
    raw_value: Option<String>,
    effort: Option<ReasoningEffort>,
) -> Result<StopReason, Error> {
    if let (Some(raw_value), None) = (&raw_value, effort) {
        inner
            .client
            .send_agent_text(format!(
                "Unsupported reasoning effort `{raw_value}`.\nUse one of: `none`, `minimal`, `low`, `medium`, `high`, `xhigh`."
            ))
            .await;
        return Ok(StopReason::EndTurn);
    }

    let model_name = find_model_for_current(&inner.models, &inner.current_model)
        .map(|model| model.display_name.clone())
        .unwrap_or_else(|| inner.current_model.clone());

    if let Some(effort) = effort {
        if let Some(model) = find_model_for_current(&inner.models, &inner.current_model)
            && !model
                .supported_reasoning_efforts
                .iter()
                .any(|option| option.reasoning_effort == effort)
        {
            let supported = model
                .supported_reasoning_efforts
                .iter()
                .map(|option| format!("`{}`", reasoning_effort_value(option.reasoning_effort)))
                .collect::<Vec<_>>()
                .join(", ");
            inner
                .client
                .send_agent_text(format!(
                    "Model `{}` does not support `{}`.\nSupported values: {}",
                    model.display_name,
                    reasoning_effort_value(effort),
                    supported,
                ))
                .await;
            return Ok(StopReason::EndTurn);
        }

        inner.reasoning_effort = effort;
        notify_config_update(inner).await;

        inner
            .client
            .send_agent_text(format!(
                "Reasoning effort set to `{}` for model `{}`.",
                reasoning_effort_value(effort),
                model_name,
            ))
            .await;
        return Ok(StopReason::EndTurn);
    }

    let supported = find_model_for_current(&inner.models, &inner.current_model)
        .map(|model| {
            model
                .supported_reasoning_efforts
                .iter()
                .map(|option| format!("`{}`", reasoning_effort_value(option.reasoning_effort)))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "`none`, `minimal`, `low`, `medium`, `high`, `xhigh`".to_string());

    inner
        .client
        .send_agent_text(format!(
            "Current reasoning effort: `{}`\nModel: `{}`\nSupported: {}\nSet with `/reasoning <value>`.",
            reasoning_effort_value(inner.reasoning_effort),
            model_name,
            supported,
        ))
        .await;
    Ok(StopReason::EndTurn)
}

async fn handle_plan_mode_command(
    inner: &mut ThreadInner,
    raw_value: Option<String>,
    mode: Option<ModeKind>,
) -> Result<StopReason, Error> {
    if let (Some(raw_value), None) = (&raw_value, mode) {
        inner
            .client
            .send_agent_text(format!(
                "Unsupported plan mode `{raw_value}`.\nUse one of: `on`, `off`, `plan`, `default`."
            ))
            .await;
        return Ok(StopReason::EndTurn);
    }

    if let Some(mode) = mode {
        if mode == ModeKind::Plan
            && let Some(default_preset) = APPROVAL_PRESETS.iter().find(|preset| preset.id == AUTO_MODE_ID)
        {
            inner.apply_mode_preset(
                default_preset,
                EditApprovalMode::AutoApprove,
                ModeKind::Plan,
            );
        } else {
            inner.collaboration_mode_kind = mode;
        }
        inner.sync_sandbox_mode_from_policy("handle_plan_mode_command");
        notify_mode_and_config_update(inner).await;
        inner
            .client
            .send_agent_text(format!(
                "Collaboration mode set to `{}`.",
                collaboration_mode_label(mode),
            ))
            .await;
        return Ok(StopReason::EndTurn);
    }

    inner
        .client
        .send_agent_text(format!(
            "Current collaboration mode: `{}`.\nSet with `/plan on` or `/plan off`, or run a one-shot planning turn with `/plan <your request>`.",
            collaboration_mode_label(inner.collaboration_mode_kind),
        ))
        .await;
    Ok(StopReason::EndTurn)
}

async fn handle_context_command(inner: &mut ThreadInner) -> Result<StopReason, Error> {
    match (inner.last_used_tokens, inner.context_window_size) {
        (Some(used), Some(size)) if size > 0 => {
            let percent = (used as f64 / size as f64) * 100.0;
            inner
                .client
                .send_agent_text(format!(
                    "Context usage: {used}/{size} tokens ({percent:.1}%)."
                ))
                .await;
        }
        (Some(used), None) => {
            inner
                .client
                .send_agent_text(format!(
                    "Context usage: {used} tokens (window size is not available yet)."
                ))
                .await;
        }
        _ => {
            inner
                .client
                .send_agent_text(
                    "Context usage is not available yet. App-server sends it only after the first completed model turn in this session.",
                )
                .await;
        }
    }
    Ok(StopReason::EndTurn)
}

async fn handle_message(
    inner: &mut ThreadInner,
    message: codex_app_server_protocol::JSONRPCMessage,
    expected_turn_id: &str,
) -> Result<Option<StopReason>, Error> {
    match message {
        codex_app_server_protocol::JSONRPCMessage::Notification(notification) => {
            handle_notification(inner, notification, expected_turn_id).await
        }
        codex_app_server_protocol::JSONRPCMessage::Request(request) => {
            handle_server_request(inner, request).await?;
            Ok(None)
        }
        codex_app_server_protocol::JSONRPCMessage::Response(response) => {
            warn!("Ignoring unexpected app-server response: {:?}", response.id);
            Ok(None)
        }
        codex_app_server_protocol::JSONRPCMessage::Error(error) => {
            warn!("Ignoring unexpected app-server error: {}", error.error.message);
            Ok(None)
        }
    }
}

async fn drain_post_turn_notifications(
    inner: &mut ThreadInner,
    expected_turn_id: &str,
    timeout: std::time::Duration,
) -> Result<(), Error> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline - now;
        let message = match tokio::time::timeout(remaining, inner.app.next_message()).await {
            Ok(message) => message?,
            Err(_) => break,
        };
        let _ = handle_message(inner, message, expected_turn_id).await?;
    }
    Ok(())
}

async fn drain_background_notifications(inner: &mut ThreadInner) -> Result<(), Error> {
    // Drain already-buffered app-server notifications before starting a new turn.
    // This keeps compaction state and usage indicators in sync between prompts.
    for _ in 0..64 {
        let message = match tokio::time::timeout(
            std::time::Duration::from_millis(5),
            inner.app.next_message(),
        )
        .await
        {
            Ok(message) => message?,
            Err(_) => break,
        };
        let _ = handle_message(inner, message, "").await?;
    }
    Ok(())
}

async fn handle_notification(
    inner: &mut ThreadInner,
    notification: codex_app_server_protocol::JSONRPCNotification,
    expected_turn_id: &str,
) -> Result<Option<StopReason>, Error> {
    let Ok(notification) = ServerNotification::try_from(notification) else {
        return Ok(None);
    };

    match notification {
        ServerNotification::AgentMessageDelta(delta) => {
            if delta.turn_id == expected_turn_id {
                if !delta.delta.trim().is_empty()
                    && fallback_plan_can_enter_summarizing(
                        inner.fallback_plan.as_ref(),
                        expected_turn_id,
                        !inner.started_tool_calls.is_empty(),
                    )
                {
                    maybe_advance_fallback_plan(inner, expected_turn_id, FallbackPlanPhase::Summarizing)
                        .await;
                }
                inner.client.send_agent_text(delta.delta).await;
            }
            Ok(None)
        }
        ServerNotification::ReasoningTextDelta(ReasoningTextDeltaNotification {
            turn_id, delta, ..
        }) => {
            if turn_id == expected_turn_id {
                inner.client.send_agent_thought(delta).await;
            }
            Ok(None)
        }
        ServerNotification::ReasoningSummaryTextDelta(ReasoningSummaryTextDeltaNotification {
            turn_id, delta, ..
        }) => {
            if turn_id == expected_turn_id {
                inner.client.send_agent_thought(delta).await;
            }
            Ok(None)
        }
        ServerNotification::ThreadTokenUsageUpdated(ThreadTokenUsageUpdatedNotification {
            thread_id,
            token_usage,
            ..
        }) => {
            if thread_id == inner.thread_id {
                // `total` is cumulative across turns. For context fullness we need
                // the latest turn's in-window total.
                let mut used = i64_to_u64_saturating(token_usage.last.total_tokens);
                inner.last_used_tokens = Some(used);
                let size = token_usage
                    .model_context_window
                    .map(i64_to_u64_saturating)
                    .filter(|size| *size > 0);
                if let Some(size) = size {
                    if used > size {
                        used = size;
                        inner.last_used_tokens = Some(used);
                    }
                    inner.context_window_size = Some(size);
                }
                if let Some(size) = inner.context_window_size {
                    inner.client.send_usage_update(used, size).await;
                }
                notify_config_update(inner).await;
            }
            Ok(None)
        }
        ServerNotification::TurnPlanUpdated(payload) => {
            if payload.turn_id == expected_turn_id {
                inner.turn_plan_updates_seen.insert(payload.turn_id.clone());
                let mut entries = payload
                    .plan
                    .into_iter()
                    .map(turn_plan_step_to_entry)
                    .collect::<Vec<_>>();
                let is_active_plan_turn =
                    inner.active_turn_mode_kind == Some(ModeKind::Plan) && payload.turn_id == expected_turn_id;
                if is_active_plan_turn && plan_entries_all_pending(&entries) {
                    let phase = inner
                        .fallback_plan
                        .as_ref()
                        .filter(|state| state.turn_id == payload.turn_id)
                        .map(|state| state.phase)
                        .unwrap_or_else(|| {
                            if inner.started_tool_calls.is_empty() {
                                FallbackPlanPhase::Planning
                            } else {
                                FallbackPlanPhase::Implementing
                            }
                        });
                    let saw_tool_activity = inner
                        .fallback_plan
                        .as_ref()
                        .filter(|state| state.turn_id == payload.turn_id)
                        .is_some_and(|state| state.saw_tool_activity)
                        || !inner.started_tool_calls.is_empty();
                    let steps = entries
                        .iter()
                        .map(|entry| entry.content.clone())
                        .collect::<Vec<_>>();
                    inner.fallback_plan = Some(FallbackPlanState {
                        turn_id: payload.turn_id.clone(),
                        phase,
                        saw_tool_activity,
                        steps: steps.clone(),
                    });
                    entries = fallback_plan_entries_for_steps(phase, &steps);
                } else if inner
                    .fallback_plan
                    .as_ref()
                    .is_some_and(|state| state.turn_id == payload.turn_id)
                {
                    inner.fallback_plan = None;
                }
                inner.last_plan_steps = entries.iter().map(|entry| entry.content.clone()).collect();
                inner
                    .client
                    .send_notification(SessionUpdate::Plan(Plan::new(limit_plan_entries(entries))))
                    .await;
            }
            Ok(None)
        }
        ServerNotification::PlanDelta(payload) => {
            if payload.turn_id == expected_turn_id {
                inner.active_turn_saw_plan_delta = true;
                inner.client.send_agent_text(payload.delta).await;
            }
            Ok(None)
        }
        ServerNotification::TurnDiffUpdated(payload) => {
            handle_turn_diff_updated(inner, payload, expected_turn_id).await;
            Ok(None)
        }
        ServerNotification::ItemStarted(payload) => {
            handle_item_started(inner, payload).await;
            Ok(None)
        }
        ServerNotification::ItemCompleted(payload) => {
            handle_item_completed(inner, payload, expected_turn_id).await;
            Ok(None)
        }
        ServerNotification::CommandExecutionOutputDelta(payload) => {
            handle_command_output_delta(inner, payload).await;
            Ok(None)
        }
        ServerNotification::TerminalInteraction(payload) => {
            handle_terminal_interaction(inner, payload).await;
            Ok(None)
        }
        ServerNotification::FileChangeOutputDelta(FileChangeOutputDeltaNotification {
            item_id,
            turn_id,
            delta,
            ..
        }) => {
            if turn_id == expected_turn_id {
                inner
                    .client
                    .send_tool_call_update(ToolCallUpdate::new(
                        ToolCallId::new(item_id),
                        ToolCallUpdateFields::new().content(vec![delta.into()]),
                    ))
                    .await;
            }
            Ok(None)
        }
        ServerNotification::McpToolCallProgress(McpToolCallProgressNotification {
            item_id,
            turn_id,
            message,
            ..
        }) => {
            if turn_id == expected_turn_id {
                inner
                    .client
                    .send_tool_call_update(ToolCallUpdate::new(
                        ToolCallId::new(item_id),
                        ToolCallUpdateFields::new().content(vec![message.into()]),
                    ))
                    .await;
            }
            Ok(None)
        }
        ServerNotification::TurnCompleted(payload) => {
            match turn_state::register_turn_completion(
                &mut inner.completed_turn_ids,
                expected_turn_id,
                &payload.turn.id,
            ) {
                turn_state::TurnCompletionDisposition::Accepted => {}
                turn_state::TurnCompletionDisposition::Duplicate => {
                    warn!(
                        turn_id = payload.turn.id.as_str(),
                        "Ignoring duplicate turn completion notification"
                    );
                    return Ok(None);
                }
                turn_state::TurnCompletionDisposition::UnexpectedTurnId => {
                    return Ok(None);
                }
            }
            maybe_advance_fallback_plan(inner, expected_turn_id, FallbackPlanPhase::Done).await;
            if inner
                .fallback_plan
                .as_ref()
                .is_some_and(|state| state.turn_id == expected_turn_id)
            {
                inner.fallback_plan = None;
            }
            inner.turn_plan_updates_seen.remove(expected_turn_id);
            finalize_turn_diff(inner, expected_turn_id).await;

            if payload.turn.status == TurnStatus::Failed
                && let Some(error) = payload.turn.error
            {
                inner
                    .client
                    .send_agent_text(format!("\n[turn error] {}", error.message))
                    .await;
            }

            let stop_reason = match payload.turn.status {
                TurnStatus::Interrupted => StopReason::Cancelled,
                TurnStatus::Completed | TurnStatus::Failed | TurnStatus::InProgress => {
                    StopReason::EndTurn
                }
            };
            Ok(Some(stop_reason))
        }
        ServerNotification::Error(error) => {
            if error.turn_id == expected_turn_id {
                inner
                    .client
                    .send_agent_text(format!("\n[error] {}", error.error.message))
                    .await;
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

async fn handle_server_request(
    inner: &mut ThreadInner,
    request: codex_app_server_protocol::JSONRPCRequest,
) -> Result<(), Error> {
    let request_id = request.id.clone();
    let request_method = request.method.clone();
    let server_request = match ServerRequest::try_from(request) {
        Ok(server_request) => server_request,
        Err(err) => {
            protocol_contract::reject_unparseable_server_request(
                &mut inner.app,
                request_id,
                &request_method,
                &err,
            )
            .await?;
            return Ok(());
        }
    };

    match server_request {
        ServerRequest::CommandExecutionRequestApproval { request_id, params } => {
            handle_command_approval(inner, request_id, params).await
        }
        ServerRequest::FileChangeRequestApproval { request_id, params } => {
            handle_file_change_approval(inner, request_id, params).await
        }
        ServerRequest::ToolRequestUserInput { request_id, params } => {
            handle_tool_request_user_input(inner, request_id, params).await
        }
        ServerRequest::DynamicToolCall { request_id, .. } => {
            protocol_contract::reject_unsupported_server_request(
                &mut inner.app,
                request_id,
                "item/tool/call",
            )
            .await
        }
        ServerRequest::ChatgptAuthTokensRefresh { request_id, .. } => {
            protocol_contract::reject_unsupported_server_request(
                &mut inner.app,
                request_id,
                "account/chatgptAuthTokens/refresh",
            )
            .await
        }
        ServerRequest::ApplyPatchApproval { request_id, .. } => {
            protocol_contract::reject_unsupported_server_request(
                &mut inner.app,
                request_id,
                "applyPatchApproval",
            )
            .await
        }
        ServerRequest::ExecCommandApproval { request_id, .. } => {
            protocol_contract::reject_unsupported_server_request(
                &mut inner.app,
                request_id,
                "execCommandApproval",
            )
            .await
        }
    }
}

async fn handle_command_approval(
    inner: &mut ThreadInner,
    request_id: codex_app_server_protocol::RequestId,
    params: CommandExecutionRequestApprovalParams,
) -> Result<(), Error> {
    let command_actions = params.command_actions.clone().unwrap_or_default();
    let title = params
        .command
        .as_deref()
        .map(|command| command_tool_title(command, &command_actions))
        .unwrap_or_else(|| "Run command".to_string());
    let tool_call_id = ToolCallId::new(params.item_id.clone());

    let mut fields = ToolCallUpdateFields::new()
        .title(title)
        .kind(ToolKind::Execute)
        .status(ToolCallStatus::Pending);
    if let Some(cwd) = params.cwd.clone() {
        fields = fields.locations(vec![ToolCallLocation::new(cwd)]);
    }
    fields = fields.raw_input(serde_json::to_value(&params).ok());

    let outcome = inner
        .client
        .request_permission(
            ToolCallUpdate::new(tool_call_id.clone(), fields),
            vec![
                PermissionOption::new(ALLOW_ONCE, "Allow once", PermissionOptionKind::AllowOnce),
                PermissionOption::new(REJECT_ONCE, "Reject", PermissionOptionKind::RejectOnce),
                PermissionOption::new(CANCEL_TURN, "Cancel turn", PermissionOptionKind::RejectOnce),
            ],
        )
        .await?;

    let decision = match outcome {
        RequestPermissionOutcome::Cancelled => CommandExecutionApprovalDecision::Cancel,
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            match option_id.0.as_ref() {
                ALLOW_ONCE => CommandExecutionApprovalDecision::Accept,
                CANCEL_TURN => CommandExecutionApprovalDecision::Cancel,
                _ => CommandExecutionApprovalDecision::Decline,
            }
        }
        _ => CommandExecutionApprovalDecision::Decline,
    };

    inner
        .app
        .send_command_approval_response(
            request_id,
            CommandExecutionRequestApprovalResponse { decision },
        )
        .await
}

async fn handle_file_change_approval(
    inner: &mut ThreadInner,
    request_id: codex_app_server_protocol::RequestId,
    params: FileChangeRequestApprovalParams,
) -> Result<(), Error> {
    if !should_prompt_file_change_approval(inner.collaboration_mode_kind, inner.edit_approval_mode)
    {
        return inner
            .app
            .send_file_change_approval_response(
                request_id,
                FileChangeRequestApprovalResponse {
                    decision: FileChangeApprovalDecision::Accept,
                },
            )
            .await;
    }

    let tool_call_id = ToolCallId::new(params.item_id.clone());
    let started_changes = inner
        .file_change_started_changes
        .get(&params.item_id)
        .cloned()
        .unwrap_or_default();
    let before_contents = inner
        .file_change_before_contents
        .get(&params.item_id)
        .cloned()
        .unwrap_or_default();
    let locations = inner
        .file_change_locations
        .get(&params.item_id)
        .cloned()
        .unwrap_or_default();
    let title = match locations.len() {
        0 => "Apply file changes".to_string(),
        1 => format!("Apply changes to {}", locations[0].display()),
        count => format!("Apply changes to {count} files"),
    };
    let mut details = Vec::new();
    if let Some(reason) = params.reason.clone()
        && !reason.trim().is_empty()
    {
        details.push(format!("Reason: {reason}"));
    }
    if let Some(root) = params.grant_root.clone() {
        details.push(format!(
            "Requested write access root: {}",
            root.display()
        ));
    }
    if !locations.is_empty() {
        let file_lines = locations
            .iter()
            .take(12)
            .map(|path| format!("- {}", path.display()))
            .collect::<Vec<_>>();
        details.push(format!("Proposed file changes:\n{}", file_lines.join("\n")));
    }
    let mut content = started_changes
        .iter()
        .map(|change| {
            ToolCallContent::Diff(file_change_to_preview_diff(
                &inner.workspace_cwd,
                &before_contents,
                change,
            ))
        })
        .collect::<Vec<_>>();
    let tool_locations = if started_changes.is_empty() {
        locations
            .iter()
            .cloned()
            .map(ToolCallLocation::new)
            .collect::<Vec<_>>()
    } else {
        started_changes
            .iter()
            .map(|change| file_change_tool_location(&inner.workspace_cwd, change))
            .collect::<Vec<_>>()
    };
    content.extend(details.into_iter().map(Into::into));

    let outcome = inner
        .client
        .request_permission(
            ToolCallUpdate::new(
                tool_call_id,
                ToolCallUpdateFields::new()
                    .title(title)
                    .kind(ToolKind::Edit)
                    .status(ToolCallStatus::Pending)
                    .locations(tool_locations)
                    .content(content)
                    .raw_input(serde_json::to_value(&params).ok()),
            ),
            vec![
                PermissionOption::new(ALLOW_ONCE, "Yes", PermissionOptionKind::AllowOnce),
                PermissionOption::new(REJECT_ONCE, "No", PermissionOptionKind::RejectOnce),
            ],
        )
        .await?;

    let decision = match outcome {
        RequestPermissionOutcome::Cancelled => FileChangeApprovalDecision::Decline,
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
            match option_id.0.as_ref() {
                ALLOW_ONCE => FileChangeApprovalDecision::Accept,
                _ => FileChangeApprovalDecision::Decline,
            }
        }
        _ => FileChangeApprovalDecision::Decline,
    };

    inner
        .app
        .send_file_change_approval_response(
            request_id,
            FileChangeRequestApprovalResponse { decision },
        )
        .await
}

fn should_prompt_file_change_approval(
    collaboration_mode_kind: ModeKind,
    edit_approval_mode: EditApprovalMode,
) -> bool {
    if collaboration_mode_kind == ModeKind::Plan {
        return true;
    }
    matches!(edit_approval_mode, EditApprovalMode::AskEveryEdit)
}

async fn handle_tool_request_user_input(
    inner: &mut ThreadInner,
    request_id: codex_app_server_protocol::RequestId,
    params: ToolRequestUserInputParams,
) -> Result<(), Error> {
    let raw_input = serde_json::to_value(&params).ok();
    let total_questions = params.questions.len();
    let mut answers = HashMap::new();
    let tool_call_id = ToolCallId::new(params.item_id.clone());
    inner
        .client
        .send_tool_call(
            ToolCall::new(tool_call_id.clone(), "Request user input")
                .kind(ToolKind::Think)
                .status(ToolCallStatus::Pending),
        )
        .await;

    for (question_index, question) in params.questions.iter().enumerate() {
        let (options, answer_labels_by_option_id, option_lines) =
            build_request_user_input_permission_options(question_index, question);
        if answer_labels_by_option_id.is_empty() {
            warn!(
                question_id = %question.id,
                "request_user_input question has no selectable options; skipping"
            );
            continue;
        }

        let mut content = Vec::new();
        if !question.question.trim().is_empty() {
            content.push(question.question.clone().into());
        }
        if !option_lines.is_empty() {
            content.push(format!("Options:\n{}", option_lines.join("\n")).into());
        }
        if question.is_secret {
            content.push("This answer is marked as secret.".to_string().into());
        }

        let title = if question.header.trim().is_empty() {
            format!("Plan input {}/{}", question_index + 1, total_questions)
        } else {
            format!(
                "{} ({}/{})",
                question.header.trim(),
                question_index + 1,
                total_questions
            )
        };

        let outcome = inner
            .client
            .request_permission(
                ToolCallUpdate::new(
                    tool_call_id.clone(),
                    ToolCallUpdateFields::new()
                        .title(title)
                        .kind(ToolKind::Think)
                        .status(ToolCallStatus::Pending)
                        .content(content)
                        .raw_input(raw_input.clone()),
                ),
                options,
            )
            .await?;

        let selected_option_id = match outcome {
            RequestPermissionOutcome::Cancelled => break,
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
                option_id.0.to_string()
            }
            _ => break,
        };

        if let Some(answer_label) = answer_labels_by_option_id.get(selected_option_id.as_str()) {
            answers.insert(
                question.id.clone(),
                ToolRequestUserInputAnswer {
                    answers: vec![answer_label.clone()],
                },
            );
        } else {
            warn!(
                question_id = %question.id,
                selected_option_id,
                "request_user_input selected unknown option id; skipping answer"
            );
        }
    }
    inner
        .client
        .send_tool_call_update(ToolCallUpdate::new(
            tool_call_id,
            ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
        ))
        .await;

    inner
        .app
        .send_tool_request_user_input_response(request_id, ToolRequestUserInputResponse { answers })
        .await
}

fn build_request_user_input_permission_options(
    _question_index: usize,
    question: &ToolRequestUserInputQuestion,
) -> (Vec<PermissionOption>, HashMap<String, String>, Vec<String>) {
    let mut answer_labels = Vec::new();
    let mut answer_labels_by_option_id = HashMap::new();
    let mut option_lines = Vec::new();

    if let Some(question_options) = &question.options {
        for option in question_options {
            answer_labels.push(option.label.clone());
            if option.description.trim().is_empty() {
                option_lines.push(format!("- {}", option.label));
            } else {
                option_lines.push(format!("- {}: {}", option.label, option.description.trim()));
            }
        }
    }

    if other_option_enabled_for_question(question) && answer_labels.len() < 3 {
        answer_labels.push(NONE_OF_THE_ABOVE.to_string());
        option_lines.push(format!("- {NONE_OF_THE_ABOVE}"));
    }

    if answer_labels.len() > 3 {
        warn!(
            question_id = %question.id,
            total_options = answer_labels.len(),
            "request_user_input has more than 3 options; truncating for ACP compatibility"
        );
        answer_labels.truncate(3);
    }

    let mut options = Vec::new();
    for (idx, answer_label) in answer_labels.into_iter().enumerate() {
        let option_id = format!("request-user-input-option-{}", idx + 1);
        answer_labels_by_option_id.insert(option_id.clone(), answer_label.clone());
        options.push(PermissionOption::new(
            option_id,
            answer_label,
            PermissionOptionKind::AllowOnce,
        ));
    }

    (options, answer_labels_by_option_id, option_lines)
}

fn other_option_enabled_for_question(question: &ToolRequestUserInputQuestion) -> bool {
    question.is_other
        && question
            .options
            .as_ref()
            .is_some_and(|options| !options.is_empty())
}

async fn handle_item_started(inner: &mut ThreadInner, payload: ItemStartedNotification) {
    let turn_id = payload.turn_id.clone();
    match payload.item {
        ThreadItem::ContextCompaction { .. } => {
            inner.compaction_in_progress = true;
        }
        ThreadItem::CommandExecution {
            id,
            command,
            cwd,
            status,
            command_actions,
            ..
        } => {
            maybe_advance_fallback_plan(inner, &turn_id, FallbackPlanPhase::Implementing).await;
            if command_looks_like_verification(&command) {
                maybe_advance_fallback_plan(inner, &turn_id, FallbackPlanPhase::Verifying).await;
            }
            inner.started_tool_calls.insert(id.clone());
            if inner.client.supports_read_text_file() {
                for action in &command_actions {
                    let CommandAction::Read { path, .. } = action else {
                        continue;
                    };
                    let read_path = resolve_workspace_path(&inner.workspace_cwd, path);
                    if let Err(err) = inner.client.prime_file_snapshot(read_path.clone()).await {
                        warn!(
                            "Failed to prime ACP snapshot for command read {}: {err:?}",
                            read_path.display()
                        );
                    }
                }
            }
            let tool_status = map_command_status(status, true);
            let title = command_tool_title(&command, &command_actions);
            let raw_input = command_tool_raw_input(&command, &command_actions);
            let tool_kind = command_tool_kind(&command, &command_actions);
            inner
                .client
                .send_tool_call(
                    ToolCall::new(ToolCallId::new(id), title)
                        .kind(tool_kind)
                        .status(tool_status)
                        .locations(vec![ToolCallLocation::new(cwd)])
                        .content(command_tool_placeholder_content())
                        .raw_input(raw_input),
                )
                .await;
        }
        ThreadItem::FileChange {
            id,
            changes,
            status,
        } => {
            maybe_advance_fallback_plan(inner, &turn_id, FallbackPlanPhase::Implementing).await;
            inner.started_tool_calls.insert(id.clone());
            inner
                .file_change_started_changes
                .insert(id.clone(), changes.clone());
            let locations = changes
                .iter()
                .map(|change| file_change_tool_location(&inner.workspace_cwd, change))
                .collect();
            let target_paths = changes
                .iter()
                .map(|change| file_change_target_path(&inner.workspace_cwd, change))
                .collect::<Vec<_>>();
            inner
                .file_change_paths_this_turn
                .extend(target_paths.iter().cloned());
            inner.file_change_locations.insert(id.clone(), target_paths);
            let before_contents = changes
                .iter()
                .map(|change| {
                    let path = resolve_workspace_path(&inner.workspace_cwd, Path::new(&change.path));
                    let content = read_file_text(&path);
                    (path, content)
                })
                .collect::<HashMap<_, _>>();

            if inner.client.supports_read_text_file() {
                for change in &changes {
                    // Prime Zed's shared buffer snapshot before the patch is approved/applied.
                    // This helps subsequent write_text_file produce real line edits for markers.
                    let source_path =
                        resolve_workspace_path(&inner.workspace_cwd, Path::new(&change.path));
                    if let Err(err) = inner.client.prime_file_snapshot(source_path.clone()).await {
                        warn!(
                            "Failed to prime ACP snapshot for {}: {err:?}",
                            source_path.display()
                        );
                    }

                    if let PatchChangeKind::Update {
                        move_path: Some(move_path),
                    } = &change.kind
                    {
                        let target_path = resolve_workspace_path(&inner.workspace_cwd, move_path);
                        if target_path != source_path
                            && let Err(err) =
                                inner.client.prime_file_snapshot(target_path.clone()).await
                        {
                            warn!(
                                "Failed to prime ACP snapshot for {}: {err:?}",
                                target_path.display()
                            );
                        }
                    }
                }
            }
            let preview_content = changes
                .iter()
                .map(|change| {
                    ToolCallContent::Diff(file_change_to_preview_diff(
                        &inner.workspace_cwd,
                        &before_contents,
                        change,
                    ))
                })
                .collect::<Vec<_>>();
            inner
                .file_change_before_contents
                .insert(id.clone(), before_contents);
            let title = if changes.is_empty() {
                "Apply edits".to_string()
            } else {
                format!("Edit {}", changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>().join(", "))
            };

            inner
                .client
                .send_tool_call(
                    ToolCall::new(ToolCallId::new(id), title)
                        .kind(ToolKind::Edit)
                        .status(map_patch_status(status, true))
                        .locations(locations)
                        .content(preview_content),
                )
                .await;
        }
        ThreadItem::McpToolCall {
            id,
            server,
            tool,
            status,
            arguments,
            ..
        } => {
            maybe_advance_fallback_plan(inner, &turn_id, FallbackPlanPhase::Implementing).await;
            inner.started_tool_calls.insert(id.clone());
            inner
                .client
                .send_tool_call(
                    ToolCall::new(ToolCallId::new(id), format!("{server}.{tool}"))
                        .kind(ToolKind::Execute)
                        .status(map_mcp_status(status, true))
                        .raw_input(arguments),
                )
                .await;
        }
        ThreadItem::WebSearch { id, query, .. } => {
            maybe_advance_fallback_plan(inner, &turn_id, FallbackPlanPhase::Implementing).await;
            inner.started_tool_calls.insert(id.clone());
            inner
                .client
                .send_tool_call(
                    ToolCall::new(ToolCallId::new(id), format!("Search web: {query}"))
                        .kind(ToolKind::Fetch)
                        .status(ToolCallStatus::InProgress),
                )
                .await;
        }
        ThreadItem::ImageView { id, path } => {
            maybe_advance_fallback_plan(inner, &turn_id, FallbackPlanPhase::Implementing).await;
            inner.started_tool_calls.insert(id.clone());
            inner
                .client
                .send_tool_call(
                    ToolCall::new(ToolCallId::new(id), format!("View image {path}"))
                        .kind(ToolKind::Read)
                        .status(ToolCallStatus::Completed)
                        .locations(vec![ToolCallLocation::new(path.clone())])
                        .content(vec![ToolCallContent::Content(Content::new(
                            ContentBlock::ResourceLink(ResourceLink::new(path.clone(), path)),
                        ))]),
                )
                .await;
        }
        _ => {}
    }
}

async fn handle_item_completed(
    inner: &mut ThreadInner,
    payload: ItemCompletedNotification,
    expected_turn_id: &str,
) {
    let turn_id = payload.turn_id.clone();
    match payload.item {
        ThreadItem::ContextCompaction { .. } => {
            inner.compaction_in_progress = false;
            inner.last_used_tokens = None;
            notify_config_update(inner).await;
            inner.client.send_agent_thought("Context compacted.").await;
        }
        ThreadItem::CommandExecution {
            id,
            command: _,
            status,
            aggregated_output,
            exit_code,
            ..
        } => {
            let mut fields = ToolCallUpdateFields::new().status(map_command_status(status, false));
            if let Some(output) = aggregated_output {
                fields = fields.content(vec![format!("```sh\n{}\n```", output.trim_end()).into()]);
            }
            if let Some(code) = exit_code {
                fields = fields.raw_output(serde_json::json!({ "exit_code": code }));
            }

            inner
                .client
                .send_tool_call_update(ToolCallUpdate::new(ToolCallId::new(id.clone()), fields))
                .await;
            inner.started_tool_calls.remove(&id);
        }
        ThreadItem::FileChange {
            id,
            changes,
            status,
        } => {
            let mut writeback_targets = Vec::new();
            if matches!(status, PatchApplyStatus::Completed) && inner.client.supports_write_text_file()
            {
                for change in &changes {
                    if matches!(change.kind, PatchChangeKind::Delete) {
                        continue;
                    }
                    let path = file_change_target_path(&inner.workspace_cwd, change);
                    if let Some(content) = read_file_text(&path) {
                        writeback_targets.push((path, content));
                    }
                }
            }

            let before_contents = inner
                .file_change_before_contents
                .remove(&id)
                .unwrap_or_default();
            let content = changes
                .into_iter()
                .filter_map(|change| {
                    file_change_to_tool_diff(&inner.workspace_cwd, &before_contents, change)
                })
                .map(ToolCallContent::Diff)
                .collect::<Vec<_>>();

            inner
                .client
                .send_tool_call_update(ToolCallUpdate::new(
                    ToolCallId::new(id.clone()),
                    ToolCallUpdateFields::new()
                        .status(map_patch_status(status, false))
                        .content(content),
                ))
                .await;

            for (path, content) in writeback_targets {
                match inner.client.write_text_file(path.clone(), content).await {
                    Ok(()) => {
                        inner.synced_paths_this_turn.insert(path);
                    }
                    Err(err) => {
                        warn!(
                            "Failed to sync file change into ACP buffer for {}: {err:?}",
                            path.display()
                        );
                    }
                }
            }

            inner.started_tool_calls.remove(&id);
            inner.file_change_locations.remove(&id);
            inner.file_change_started_changes.remove(&id);
            inner.file_change_before_contents.remove(&id);
        }
        ThreadItem::McpToolCall {
            id,
            status,
            result,
            error,
            ..
        } => {
            let mut fields = ToolCallUpdateFields::new().status(map_mcp_status(status, false));
            if let Some(result) = result {
                fields = fields.raw_output(serde_json::json!({ "result": result }));
            }
            if let Some(error) = error {
                fields = fields.raw_output(serde_json::json!({ "error": error }));
            }

            inner
                .client
                .send_tool_call_update(ToolCallUpdate::new(ToolCallId::new(id.clone()), fields))
                .await;
            inner.started_tool_calls.remove(&id);
        }
        ThreadItem::Plan { text, .. } => {
            if turn_id == expected_turn_id {
                inner.active_turn_saw_plan_item = true;
            }
            if inner.turn_plan_updates_seen.contains(&turn_id) {
                return;
            }
            if let Some(plan) = plan_from_text(&text) {
                let is_active_plan_turn =
                    inner.active_turn_mode_kind == Some(ModeKind::Plan) && turn_id == expected_turn_id;

                let plan = if is_active_plan_turn {
                    let phase = inner
                        .fallback_plan
                        .as_ref()
                        .filter(|state| state.turn_id == turn_id)
                        .map(|state| state.phase)
                        .unwrap_or_else(|| {
                            if inner.started_tool_calls.is_empty() {
                                FallbackPlanPhase::Planning
                            } else {
                                FallbackPlanPhase::Implementing
                            }
                        });
                    let saw_tool_activity = inner
                        .fallback_plan
                        .as_ref()
                        .filter(|state| state.turn_id == turn_id)
                        .is_some_and(|state| state.saw_tool_activity);
                    let steps = plan
                        .entries
                        .iter()
                        .map(|entry| entry.content.clone())
                        .collect::<Vec<_>>();

                    inner.fallback_plan = Some(FallbackPlanState {
                        turn_id: turn_id.clone(),
                        phase,
                        saw_tool_activity,
                        steps: steps.clone(),
                    });

                    Plan::new(fallback_plan_entries_for_steps(phase, &steps))
                } else {
                    inner.turn_plan_updates_seen.insert(turn_id.clone());
                    if inner
                        .fallback_plan
                        .as_ref()
                        .is_some_and(|state| state.turn_id == turn_id)
                    {
                        inner.fallback_plan = None;
                    }
                    promote_first_pending_step(plan)
                };
                inner.last_plan_steps = plan.entries.iter().map(|entry| entry.content.clone()).collect();
                inner
                    .client
                    .send_notification(SessionUpdate::Plan(normalize_plan_for_ui(plan)))
                    .await;
            } else if !text.is_empty() {
                if inner.active_turn_mode_kind == Some(ModeKind::Plan) && turn_id == expected_turn_id {
                    if !inner.active_turn_saw_plan_delta {
                        inner.client.send_agent_text(text).await;
                    }
                } else {
                    inner.client.send_agent_thought(text).await;
                }
            }
        }
        ThreadItem::WebSearch { id, .. } => {
            inner
                .client
                .send_tool_call_update(ToolCallUpdate::new(
                    ToolCallId::new(id.clone()),
                    ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
                ))
                .await;
            inner.started_tool_calls.remove(&id);
        }
        _ => {}
    }
}

async fn handle_command_output_delta(
    inner: &mut ThreadInner,
    payload: CommandExecutionOutputDeltaNotification,
) {
    if !inner.started_tool_calls.contains(&payload.item_id) {
        return;
    }

    let update = if inner.client.supports_terminal_output() {
        ToolCallUpdate::new(ToolCallId::new(payload.item_id.clone()), ToolCallUpdateFields::new())
            .meta(Meta::from_iter([(
                "terminal_output".to_owned(),
                serde_json::json!({
                    "terminal_id": payload.item_id,
                    "data": payload.delta,
                }),
            )]))
    } else {
        ToolCallUpdate::new(
            ToolCallId::new(payload.item_id),
            ToolCallUpdateFields::new().content(vec![payload.delta.into()]),
        )
    };

    inner.client.send_tool_call_update(update).await;
}

async fn handle_terminal_interaction(inner: &mut ThreadInner, payload: TerminalInteractionNotification) {
    if !inner.started_tool_calls.contains(&payload.item_id) {
        return;
    }

    if inner.client.supports_terminal_output() {
        inner
            .client
            .send_tool_call_update(
                ToolCallUpdate::new(ToolCallId::new(payload.item_id.clone()), ToolCallUpdateFields::new())
                    .meta(Meta::from_iter([(
                        "terminal_output".to_owned(),
                        serde_json::json!({
                            "terminal_id": payload.item_id,
                            "data": format!("\n{}\n", payload.stdin),
                        }),
                    )])),
            )
            .await;
    }
}

#[derive(Clone, Debug)]
struct TurnUnifiedDiffFile {
    path: PathBuf,
    old_text: String,
    new_text: String,
    is_delete: bool,
}

async fn handle_turn_diff_updated(
    inner: &mut ThreadInner,
    payload: TurnDiffUpdatedNotification,
    expected_turn_id: &str,
) {
    if payload.turn_id != expected_turn_id {
        return;
    }

    inner.latest_turn_diff = Some(payload.diff);
}

async fn finalize_turn_diff(inner: &mut ThreadInner, turn_id: &str) {
    let Some(diff) = inner.latest_turn_diff.take() else {
        return;
    };

    update_turn_diff_tool_call(inner, turn_id, &diff, false).await;
    sync_turn_diff_files_to_acp(inner, &diff).await;
}

async fn update_turn_diff_tool_call(
    inner: &mut ThreadInner,
    turn_id: &str,
    diff: &str,
    in_progress: bool,
) {
    let parsed_files = parse_turn_unified_diff_files(diff)
        .into_iter()
        .filter_map(|file| {
            let path = resolve_turn_diff_path(&inner.workspace_cwd, &file.path);
            if inner.file_change_paths_this_turn.contains(&path) {
                None
            } else {
                Some((file, path))
            }
        })
        .collect::<Vec<_>>();
    if parsed_files.is_empty() {
        return;
    }

    let tool_call_key = format!("{TURN_DIFF_TOOL_CALL_PREFIX}{turn_id}");
    let tool_call_id = ToolCallId::new(tool_call_key.clone());
    let status = if in_progress {
        ToolCallStatus::InProgress
    } else {
        ToolCallStatus::Completed
    };

    let mut content = Vec::new();
    let mut locations = Vec::new();
    for (file, path) in parsed_files {
        let old_text = if file.old_text.is_empty() {
            None
        } else {
            Some(file.old_text)
        };
        content.push(ToolCallContent::Diff(Diff::new(path.clone(), file.new_text).old_text(old_text)));
        locations.push(ToolCallLocation::new(path));
    }

    if inner.started_tool_calls.insert(tool_call_key.clone()) {
        inner
            .client
            .send_tool_call(
                ToolCall::new(tool_call_id, "Turn diff")
                    .kind(ToolKind::Edit)
                    .status(status)
                    .locations(locations)
                    .content(content),
            )
            .await;
    } else {
        inner
            .client
            .send_tool_call_update(ToolCallUpdate::new(
                tool_call_id,
                ToolCallUpdateFields::new()
                    .status(status)
                    .locations(locations)
                    .content(content),
            ))
            .await;
    }

    if !in_progress {
        inner.started_tool_calls.remove(&tool_call_key);
    }
}

async fn sync_turn_diff_files_to_acp(inner: &mut ThreadInner, diff: &str) {
    if !inner.client.supports_write_text_file() {
        return;
    }

    for file in parse_turn_unified_diff_files(diff) {
        if file.is_delete {
            continue;
        }

        let path = resolve_turn_diff_path(&inner.workspace_cwd, &file.path);
        if inner.synced_paths_this_turn.contains(&path) {
            continue;
        }

        let Some(content) = read_file_text(&path) else {
            continue;
        };

        match inner.client.write_text_file(path.clone(), content).await {
            Ok(()) => {
                inner.synced_paths_this_turn.insert(path);
            }
            Err(err) => {
                warn!(
                    "Failed to sync turn diff into ACP buffer for {}: {err:?}",
                    path.display()
                );
            }
        }
    }
}

fn parse_turn_unified_diff_files(unified_diff: &str) -> Vec<TurnUnifiedDiffFile> {
    fn finalize_section(
        section: &mut String,
        old_path: &mut Option<String>,
        new_path: &mut Option<String>,
        output: &mut Vec<TurnUnifiedDiffFile>,
    ) {
        if section.trim().is_empty() {
            section.clear();
            *old_path = None;
            *new_path = None;
            return;
        }

        let old = old_path.take();
        let new = new_path.take();
        let new_is_dev_null = new.as_deref().is_some_and(|path| path.trim() == DEV_NULL);
        let chosen_path = if new_is_dev_null {
            old
        } else {
            new.or(old)
        };
        let Some(path) = chosen_path else {
            section.clear();
            return;
        };

        let normalized = normalize_unified_diff_path(&path);
        if normalized.is_empty() {
            section.clear();
            return;
        }

        let Some((old_text, new_text)) = unified_diff_to_old_new(section) else {
            section.clear();
            return;
        };
        if old_text == new_text {
            section.clear();
            return;
        }

        output.push(TurnUnifiedDiffFile {
            path: PathBuf::from(normalized),
            old_text,
            new_text,
            is_delete: new_is_dev_null,
        });
        section.clear();
    }

    let mut files = Vec::new();
    let mut section = String::new();
    let mut old_path: Option<String> = None;
    let mut new_path: Option<String> = None;
    let mut saw_file_header = false;

    for raw_line in unified_diff.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);

        if line.starts_with("diff --git ") {
            finalize_section(&mut section, &mut old_path, &mut new_path, &mut files);
            saw_file_header = true;
        }

        if let Some(path) = line.strip_prefix("--- ") {
            old_path = Some(path.trim().to_string());
        } else if let Some(path) = line.strip_prefix("+++ ") {
            new_path = Some(path.trim().to_string());
        }

        if saw_file_header || !section.is_empty() || line.starts_with("--- ") || line.starts_with("+++ ") {
            section.push_str(raw_line);
        }
    }

    finalize_section(&mut section, &mut old_path, &mut new_path, &mut files);
    files
}

fn normalize_unified_diff_path(path: &str) -> String {
    let trimmed = path.trim().trim_matches('"');
    if trimmed == DEV_NULL {
        return String::new();
    }
    trimmed
        .strip_prefix("a/")
        .or_else(|| trimmed.strip_prefix("b/"))
        .unwrap_or(trimmed)
        .to_string()
}

fn resolve_turn_diff_path(workspace_cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }

    let direct = workspace_cwd.join(path);
    if direct.exists() {
        return direct;
    }

    for ancestor in workspace_cwd.ancestors() {
        if !ancestor.join(".git").exists() {
            continue;
        }
        let candidate = ancestor.join(path);
        if candidate.exists() {
            return candidate;
        }
    }

    direct
}

fn resolve_workspace_path(workspace_cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_cwd.join(path)
    }
}

fn read_file_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[derive(Clone, Debug)]
struct UnifiedDiffLine {
    kind: char,
    text: String,
}

#[derive(Clone, Debug)]
struct UnifiedDiffHunk {
    old_start: usize,
    new_start: usize,
    lines: Vec<UnifiedDiffLine>,
}

fn parse_unified_range(input: &str) -> Option<(usize, usize)> {
    let (start, len) = match input.split_once(',') {
        Some((start, len)) => (start, len),
        None => (input, "1"),
    };
    let start = start.parse::<usize>().ok()?;
    let len = len.parse::<usize>().ok()?;
    Some((start, len))
}

fn parse_unified_hunk_header(line: &str) -> Option<(usize, usize)> {
    let line = line.strip_prefix("@@ -")?;
    let (old_range, rest) = line.split_once(" +")?;
    let (new_range, _) = rest.split_once(" @@")?;
    let (old_start, _old_len) = parse_unified_range(old_range)?;
    let (new_start, _new_len) = parse_unified_range(new_range)?;
    Some((old_start, new_start))
}

fn parse_unified_diff_hunks(unified_diff: &str) -> Option<Vec<UnifiedDiffHunk>> {
    // Parse only the subset we need from unified diff: hunk headers and
    // line-prefixed bodies (' ', '+', '-'). File headers and move suffixes are ignored.
    let mut hunks = Vec::new();
    let mut current_hunk: Option<UnifiedDiffHunk> = None;

    for raw_line in unified_diff.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);

        if line.starts_with("@@") {
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk);
            }
            let (old_start, new_start) = parse_unified_hunk_header(line)?;
            current_hunk = Some(UnifiedDiffHunk {
                old_start,
                new_start,
                lines: Vec::new(),
            });
            continue;
        }

        let Some(hunk) = current_hunk.as_mut() else {
            continue;
        };

        let Some(kind) = line.chars().next() else {
            continue;
        };

        if !matches!(kind, ' ' | '+' | '-') {
            continue;
        }

        let mut text = line[1..].to_string();
        if raw_line.ends_with('\n') {
            text.push('\n');
        }
        hunk.lines.push(UnifiedDiffLine { kind, text });
    }

    if let Some(hunk) = current_hunk {
        hunks.push(hunk);
    }

    if hunks.is_empty() {
        None
    } else {
        Some(hunks)
    }
}

fn unified_diff_to_old_new(unified_diff: &str) -> Option<(String, String)> {
    let hunks = parse_unified_diff_hunks(unified_diff)?;
    let mut old_text = String::new();
    let mut new_text = String::new();
    for hunk in hunks {
        for line in hunk.lines {
            match line.kind {
                ' ' => {
                    old_text.push_str(&line.text);
                    new_text.push_str(&line.text);
                }
                '-' => old_text.push_str(&line.text),
                '+' => new_text.push_str(&line.text),
                _ => {}
            }
        }
    }

    if old_text.is_empty() && new_text.is_empty() {
        None
    } else {
        Some((old_text, new_text))
    }
}

fn split_text_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split_inclusive('\n').map(str::to_string).collect()
    }
}

fn apply_unified_diff_to_text(old_text: &str, unified_diff: &str) -> Option<String> {
    // Best-effort reconstruction of post-edit text from unified diff.
    // We validate context/deleted lines against old_text to avoid producing a wrong preview.
    let hunks = parse_unified_diff_hunks(unified_diff)?;
    let old_lines = split_text_lines(old_text);
    let mut old_index = 0usize;
    let mut new_lines = Vec::new();

    for hunk in hunks {
        let target_index = hunk.old_start.saturating_sub(1);
        if target_index > old_lines.len() || target_index < old_index {
            return None;
        }

        new_lines.extend(old_lines[old_index..target_index].iter().cloned());
        old_index = target_index;

        for line in hunk.lines {
            match line.kind {
                ' ' => {
                    let current_line = old_lines.get(old_index)?;
                    if current_line != &line.text {
                        return None;
                    }
                    new_lines.push(line.text);
                    old_index += 1;
                }
                '-' => {
                    let current_line = old_lines.get(old_index)?;
                    if current_line != &line.text {
                        return None;
                    }
                    old_index += 1;
                }
                '+' => {
                    new_lines.push(line.text);
                }
                _ => return None,
            }
        }
    }

    new_lines.extend(old_lines[old_index..].iter().cloned());
    Some(new_lines.concat())
}

fn usize_to_u32_saturating(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

fn first_hunk_line(unified_diff: &str, use_new_start: bool) -> Option<u32> {
    let first_hunk = parse_unified_diff_hunks(unified_diff)?
        .into_iter()
        .next()?;
    let start = if use_new_start {
        first_hunk.new_start
    } else {
        first_hunk.old_start
    };
    Some(usize_to_u32_saturating(start.saturating_sub(1)))
}

fn file_change_target_path(
    workspace_cwd: &Path,
    change: &codex_app_server_protocol::FileUpdateChange,
) -> PathBuf {
    let source_path = resolve_workspace_path(workspace_cwd, Path::new(&change.path));
    match &change.kind {
        PatchChangeKind::Update {
            move_path: Some(move_path),
        } => resolve_workspace_path(workspace_cwd, move_path),
        _ => source_path,
    }
}

fn file_change_location_line(change: &codex_app_server_protocol::FileUpdateChange) -> Option<u32> {
    match &change.kind {
        PatchChangeKind::Add => first_hunk_line(&change.diff, true).or(Some(0)),
        PatchChangeKind::Delete => first_hunk_line(&change.diff, false).or(Some(0)),
        PatchChangeKind::Update { .. } => {
            first_hunk_line(&change.diff, true).or_else(|| first_hunk_line(&change.diff, false))
        }
    }
}

fn file_change_tool_location(
    workspace_cwd: &Path,
    change: &codex_app_server_protocol::FileUpdateChange,
) -> ToolCallLocation {
    let path = file_change_target_path(workspace_cwd, change);
    let location = ToolCallLocation::new(path);
    if let Some(line) = file_change_location_line(change) {
        location.line(line)
    } else {
        location
    }
}

fn file_change_to_tool_diff(
    workspace_cwd: &Path,
    before_contents: &HashMap<PathBuf, Option<String>>,
    change: codex_app_server_protocol::FileUpdateChange,
) -> Option<Diff> {
    let source_path = resolve_workspace_path(workspace_cwd, Path::new(&change.path));
    match change.kind {
        PatchChangeKind::Add => {
            let new_text = read_file_text(&source_path).unwrap_or(change.diff);
            Some(Diff::new(source_path, new_text))
        }
        PatchChangeKind::Delete => {
            let old_text = before_contents
                .get(&source_path)
                .cloned()
                .flatten()
                .or_else(|| if change.diff.is_empty() { None } else { Some(change.diff) });
            Some(Diff::new(source_path, String::new()).old_text(old_text))
        }
        PatchChangeKind::Update { move_path } => {
            let target_path = move_path
                .as_ref()
                .map(|path| resolve_workspace_path(workspace_cwd, path))
                .unwrap_or_else(|| source_path.clone());
            let new_text = read_file_text(&target_path).unwrap_or(change.diff);
            let old_text = before_contents.get(&source_path).cloned().flatten();
            Some(Diff::new(target_path, new_text).old_text(old_text))
        }
    }
}

fn file_change_to_preview_diff(
    workspace_cwd: &Path,
    before_contents: &HashMap<PathBuf, Option<String>>,
    change: &codex_app_server_protocol::FileUpdateChange,
) -> Diff {
    let source_path = resolve_workspace_path(workspace_cwd, Path::new(&change.path));
    match &change.kind {
        PatchChangeKind::Add => Diff::new(source_path, change.diff.clone()),
        PatchChangeKind::Delete => {
            let old_text = before_contents
                .get(&source_path)
                .cloned()
                .flatten()
                .or_else(|| if change.diff.is_empty() { None } else { Some(change.diff.clone()) });
            Diff::new(source_path, String::new()).old_text(old_text)
        }
        PatchChangeKind::Update { move_path } => {
            let target_path = move_path
                .as_ref()
                .map(|path| resolve_workspace_path(workspace_cwd, path))
                .unwrap_or_else(|| source_path.clone());
            let old_text = before_contents.get(&source_path).cloned().flatten();
            if change.diff.is_empty() {
                return Diff::new(target_path, old_text.clone().unwrap_or_default()).old_text(old_text);
            }

            // Prefer exact reconstruction from captured pre-edit content.
            if let Some(existing_old_text) = old_text.clone()
                && let Some(new_text) = apply_unified_diff_to_text(&existing_old_text, &change.diff)
            {
                return Diff::new(target_path, new_text).old_text(Some(existing_old_text));
            }

            // Fallback when old snapshot is unavailable/incompatible (e.g. resumed history):
            // derive both sides directly from the unified diff hunks.
            if let Some((parsed_old_text, parsed_new_text)) = unified_diff_to_old_new(&change.diff) {
                return Diff::new(target_path, parsed_new_text).old_text(Some(parsed_old_text));
            }

            Diff::new(target_path, change.diff.clone()).old_text(old_text)
        }
    }
}

fn file_change_to_replay_diff(
    workspace_cwd: &Path,
    change: codex_app_server_protocol::FileUpdateChange,
) -> Diff {
    let source_path = resolve_workspace_path(workspace_cwd, Path::new(&change.path));
    match change.kind {
        PatchChangeKind::Add => {
            // Some historical events may carry add/delete as unified hunks instead of raw content.
            // Prefer parsed old/new when available to keep replay diffs richly highlighted.
            if let Some((old_text, new_text)) = unified_diff_to_old_new(&change.diff) {
                let old_text = if old_text.is_empty() {
                    None
                } else {
                    Some(old_text)
                };
                Diff::new(source_path, new_text).old_text(old_text)
            } else {
                Diff::new(source_path, change.diff)
            }
        }
        PatchChangeKind::Delete => {
            if let Some((old_text, new_text)) = unified_diff_to_old_new(&change.diff) {
                let old_text = if old_text.is_empty() {
                    None
                } else {
                    Some(old_text)
                };
                Diff::new(source_path, new_text).old_text(old_text)
            } else {
                let old_text = if change.diff.is_empty() {
                    None
                } else {
                    Some(change.diff)
                };
                Diff::new(source_path, String::new()).old_text(old_text)
            }
        }
        PatchChangeKind::Update { move_path } => {
            let target_path = move_path
                .as_ref()
                .map(|path| resolve_workspace_path(workspace_cwd, path))
                .unwrap_or_else(|| source_path.clone());

            // Replay events only carry patch text for updates, so we reconstruct old/new
            // from the unified diff to keep UI line markers (+/-) available after /resume.
            if let Some((old_text, new_text)) = unified_diff_to_old_new(&change.diff) {
                Diff::new(target_path, new_text).old_text(Some(old_text))
            } else {
                Diff::new(target_path, change.diff)
            }
        }
    }
}

async fn replay_turns(client: &SessionClient, workspace_cwd: &Path, turns: Vec<AppTurn>) {
    for turn in turns {
        for item in turn.items {
            replay_thread_item(client, workspace_cwd, item).await;
        }
    }
}

async fn replay_thread_item(client: &SessionClient, workspace_cwd: &Path, item: ThreadItem) {
    match item {
        ThreadItem::UserMessage { content, .. } => {
            let text = render_user_inputs(content);
            if !text.is_empty() {
                client.send_user_text(text).await;
            }
        }
        ThreadItem::AgentMessage { text, .. } => {
            client.send_agent_text(text).await;
        }
        ThreadItem::Reasoning {
            summary, content, ..
        } => {
            for part in summary {
                if !part.is_empty() {
                    client.send_agent_thought(part).await;
                }
            }
            for part in content {
                if !part.is_empty() {
                    client.send_agent_thought(part).await;
                }
            }
        }
        ThreadItem::CommandExecution {
            id,
            command,
            cwd,
            status,
            command_actions,
            aggregated_output,
            exit_code,
            ..
        } => {
            let title = command_tool_title(&command, &command_actions);
            let raw_input = command_tool_raw_input(&command, &command_actions);
            let tool_kind = command_tool_kind(&command, &command_actions);
            client
                .send_tool_call(
                    ToolCall::new(ToolCallId::new(id.clone()), title)
                        .kind(tool_kind)
                        .status(map_command_status(status.clone(), false))
                        .locations(vec![ToolCallLocation::new(cwd)])
                        .content(command_tool_placeholder_content())
                        .raw_input(raw_input),
                )
                .await;

            let mut fields = ToolCallUpdateFields::new().status(map_command_status(status, false));
            if let Some(output) = aggregated_output {
                fields = fields.content(vec![format!("```sh\n{}\n```", output.trim_end()).into()]);
            }
            if let Some(code) = exit_code {
                fields = fields.raw_output(serde_json::json!({ "exit_code": code }));
            }
            client
                .send_tool_call_update(ToolCallUpdate::new(ToolCallId::new(id), fields))
                .await;
        }
        ThreadItem::FileChange {
            id,
            changes,
            status,
        } => {
            let locations = changes
                .iter()
                .map(|change| file_change_tool_location(workspace_cwd, change))
                .collect::<Vec<_>>();
            let content = changes
                .into_iter()
                .map(|change| {
                    ToolCallContent::Diff(file_change_to_replay_diff(workspace_cwd, change))
                })
                .collect::<Vec<_>>();

            client
                .send_tool_call(
                    ToolCall::new(ToolCallId::new(id), "Apply edits")
                        .kind(ToolKind::Edit)
                        .status(map_patch_status(status, false))
                        .locations(locations)
                        .content(content),
                )
                .await;
        }
        ThreadItem::McpToolCall {
            id,
            server,
            tool,
            status,
            arguments,
            result,
            error,
            ..
        } => {
            client
                .send_tool_call(
                    ToolCall::new(ToolCallId::new(id.clone()), format!("{server}.{tool}"))
                        .kind(ToolKind::Execute)
                        .status(map_mcp_status(status.clone(), false))
                        .raw_input(arguments),
                )
                .await;

            let mut fields = ToolCallUpdateFields::new().status(map_mcp_status(status, false));
            if let Some(result) = result {
                fields = fields.raw_output(serde_json::json!({ "result": result }));
            }
            if let Some(error) = error {
                fields = fields.raw_output(serde_json::json!({ "error": error }));
            }

            client
                .send_tool_call_update(ToolCallUpdate::new(ToolCallId::new(id), fields))
                .await;
        }
        ThreadItem::WebSearch { id, query, .. } => {
            client
                .send_tool_call(
                    ToolCall::new(ToolCallId::new(id), format!("Search web: {query}"))
                        .kind(ToolKind::Fetch)
                        .status(ToolCallStatus::Completed),
                )
                .await;
        }
        ThreadItem::ImageView { id, path } => {
            client
                .send_tool_call(
                    ToolCall::new(ToolCallId::new(id), format!("View image {path}"))
                        .kind(ToolKind::Read)
                        .status(ToolCallStatus::Completed)
                        .locations(vec![ToolCallLocation::new(path.clone())])
                        .content(vec![ToolCallContent::Content(Content::new(
                            ContentBlock::ResourceLink(ResourceLink::new(path.clone(), path)),
                        ))]),
                )
                .await;
        }
        ThreadItem::Plan { text, .. } => {
            if !text.is_empty() {
                client.send_agent_text(text).await;
            }
        }
        ThreadItem::EnteredReviewMode { review, .. } => {
            client
                .send_agent_thought(format!("Entered review mode: {review}"))
                .await;
        }
        ThreadItem::ExitedReviewMode { review, .. } => {
            client
                .send_agent_thought(format!("Exited review mode: {review}"))
                .await;
        }
        ThreadItem::ContextCompaction { .. } => {
            client.send_agent_thought("Context compacted.").await;
        }
        _ => {}
    }
}

fn render_user_inputs(inputs: Vec<UserInput>) -> String {
    inputs
        .into_iter()
        .filter_map(|input| match input {
            UserInput::Text { text, .. } => Some(text),
            UserInput::Image { .. } => Some("[image]".to_string()),
            UserInput::LocalImage { path } => Some(format!("[image: {}]", path.display())),
            UserInput::Skill { name, .. } => Some(format!("[skill: {name}]")),
            UserInput::Mention { name, .. } => Some(format!("@{name}")),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn initialize_fallback_plan_for_turn(
    inner: &mut ThreadInner,
    turn_id: &str,
    collaboration_mode_kind: ModeKind,
) {
    if collaboration_mode_kind == ModeKind::Plan {
        inner.fallback_plan = None;
        return;
    }

    let Some(steps) = inner.carryover_plan_steps.take().filter(|steps| !steps.is_empty()) else {
        inner.fallback_plan = None;
        return;
    };

    inner.fallback_plan = Some(FallbackPlanState {
        turn_id: turn_id.to_string(),
        phase: FallbackPlanPhase::Planning,
        saw_tool_activity: false,
        steps: steps.clone(),
    });
    inner.last_plan_steps = steps.clone();
    let entries = fallback_plan_entries_for_steps(FallbackPlanPhase::Planning, &steps);
    inner
        .client
        .send_notification(SessionUpdate::Plan(Plan::new(limit_plan_entries(entries))))
        .await;
}

async fn maybe_advance_fallback_plan(
    inner: &mut ThreadInner,
    turn_id: &str,
    next_phase: FallbackPlanPhase,
) {
    let mut entries_to_emit = None;
    if let Some(state) = inner.fallback_plan.as_mut()
        && state.turn_id == turn_id
    {
        if matches!(
            next_phase,
            FallbackPlanPhase::Implementing
                | FallbackPlanPhase::Verifying
                | FallbackPlanPhase::Summarizing
        ) {
            state.saw_tool_activity = true;
        }
        if !fallback_plan_should_advance(state, next_phase) {
            return;
        }
        state.phase = next_phase;
        entries_to_emit = Some(fallback_plan_entries_for_steps(state.phase, &state.steps));
    }

    if let Some(entries) = entries_to_emit {
        inner.last_plan_steps = entries.iter().map(|entry| entry.content.clone()).collect();
        inner
            .client
            .send_notification(SessionUpdate::Plan(Plan::new(limit_plan_entries(entries))))
            .await;
    }
}

fn fallback_plan_should_advance(state: &FallbackPlanState, next_phase: FallbackPlanPhase) -> bool {
    if next_phase <= state.phase {
        return false;
    }
    if next_phase == FallbackPlanPhase::Done && !state.saw_tool_activity {
        return false;
    }
    true
}

#[cfg(test)]
fn fallback_plan_entries(phase: FallbackPlanPhase) -> Vec<PlanEntry> {
    fallback_plan_entries_for_steps(phase, &[])
}

fn fallback_plan_entries_for_steps(phase: FallbackPlanPhase, steps: &[String]) -> Vec<PlanEntry> {
    fn status_for_step(phase: FallbackPlanPhase, index: usize) -> PlanEntryStatus {
        let target = phase as usize;
        if phase == FallbackPlanPhase::Done || index < target {
            PlanEntryStatus::Completed
        } else if index == target {
            PlanEntryStatus::InProgress
        } else {
            PlanEntryStatus::Pending
        }
    }

    let default_steps = [
        "Decide implementation scope and approach",
        "Apply code changes",
        "Run checks and verification",
        "Review and summarize results",
    ];
    let labels = if steps.is_empty() {
        default_steps
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    } else {
        steps.to_vec()
    };

    labels
    .into_iter()
    .enumerate()
    .map(|(index, label)| {
        PlanEntry::new(
            label,
            PlanEntryPriority::Medium,
            status_for_step(phase, index),
        )
    })
    .collect()
}

fn fallback_plan_can_enter_summarizing(
    state: Option<&FallbackPlanState>,
    turn_id: &str,
    has_active_tool_calls: bool,
) -> bool {
    if has_active_tool_calls {
        return false;
    }
    let Some(state) = state else {
        return false;
    };
    state.turn_id == turn_id
        && state.saw_tool_activity
        && state.phase < FallbackPlanPhase::Summarizing
}

fn plan_entries_all_pending(entries: &[PlanEntry]) -> bool {
    !entries.is_empty() && entries.iter().all(|entry| entry.status == PlanEntryStatus::Pending)
}

fn command_tool_title(command: &str, command_actions: &[CommandAction]) -> String {
    command_title_from_actions(command_actions).unwrap_or_else(|| command_title_from_shell(command))
}

fn command_tool_kind(command: &str, command_actions: &[CommandAction]) -> ToolKind {
    let mut has_read = false;
    let mut has_list_files = false;
    let mut has_search = false;

    for action in command_actions {
        match action {
            CommandAction::Read { .. } => has_read = true,
            CommandAction::ListFiles { .. } => has_list_files = true,
            CommandAction::Search { .. } => has_search = true,
            CommandAction::Unknown { .. } => {}
        }
    }

    if has_search || has_list_files {
        return ToolKind::Search;
    }
    if has_read {
        return ToolKind::Read;
    }

    let inner = extract_inner_shell_command(command);
    let normalized = inner.to_ascii_lowercase();
    if looks_like_search_command(&normalized) || looks_like_listing_command(&normalized) {
        return ToolKind::Search;
    }
    if looks_like_read_command(&normalized) {
        return ToolKind::Read;
    }

    // Keep command cards in the generic collapsible tool UI (non-terminal card),
    // so users can expand and inspect raw command input on demand.
    ToolKind::Think
}

fn command_tool_placeholder_content() -> Vec<ToolCallContent> {
    vec!["Command details are available in Raw Input.".to_string().into()]
}

fn command_title_from_actions(command_actions: &[CommandAction]) -> Option<String> {
    let mut reads = Vec::new();
    let mut list_files_count = 0usize;
    let mut search_count = 0usize;
    let mut unknown_count = 0usize;

    for action in command_actions {
        match action {
            CommandAction::Read { path, .. } => reads.push(path),
            CommandAction::ListFiles { .. } => list_files_count += 1,
            CommandAction::Search { .. } => search_count += 1,
            CommandAction::Unknown { .. } => unknown_count += 1,
        }
    }

    if reads.is_empty() && list_files_count == 0 && search_count == 0 && unknown_count > 0 {
        return None;
    }

    if !reads.is_empty() && list_files_count == 0 && search_count == 0 {
        if reads.len() == 1 {
            if let Some(name) = reads[0]
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
            {
                return Some(format!("Read {name}"));
            }
            return Some("Read file".to_string());
        }
        return Some(format!("Read {} files", reads.len()));
    }

    if list_files_count > 0 && reads.is_empty() && search_count == 0 {
        return Some("Analyze folder contents".to_string());
    }

    if search_count > 0 && reads.is_empty() && list_files_count == 0 {
        return Some("Search in workspace".to_string());
    }

    if search_count > 0 && !reads.is_empty() && list_files_count == 0 {
        return Some("Search and inspect files".to_string());
    }

    if list_files_count > 0 || search_count > 0 || !reads.is_empty() {
        return Some("Inspect workspace files".to_string());
    }

    None
}

fn command_title_from_shell(command: &str) -> String {
    let inner_command = extract_inner_shell_command(command);
    let normalized = inner_command.to_ascii_lowercase();

    if command_looks_like_verification(&inner_command) {
        return "Run tests and checks".to_string();
    }
    if looks_like_listing_command(&normalized) {
        return "Analyze folder contents".to_string();
    }
    if looks_like_search_command(&normalized) {
        return "Search in workspace".to_string();
    }
    if looks_like_read_command(&normalized) {
        return "Read file contents".to_string();
    }
    if looks_like_git_inspection_command(&normalized) {
        return "Inspect git state".to_string();
    }

    "Run shell command".to_string()
}

fn command_tool_raw_input(command: &str, command_actions: &[CommandAction]) -> Option<serde_json::Value> {
    if command.trim().is_empty() && command_actions.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "command": command,
        "commandActions": command_actions,
    }))
}

fn extract_inner_shell_command(command: &str) -> String {
    let trimmed = command.trim();
    let Some(parts) = shlex::split(trimmed) else {
        return trimmed.to_string();
    };

    if parts.len() >= 3
        && is_shell_executable(&parts[0])
        && matches!(parts[1].as_str(), "-c" | "-lc" | "-ic")
    {
        return parts[2].trim().to_string();
    }

    trimmed.to_string()
}

fn is_shell_executable(program: &str) -> bool {
    let binary = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    matches!(binary, "bash" | "sh" | "zsh" | "fish")
}

fn shell_uses_command(command: &str, candidates: &[&str]) -> bool {
    command
        .split(|ch: char| matches!(ch, '|' | ';' | '&'))
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .filter_map(|segment| segment.split_whitespace().next())
        .any(|token| candidates.contains(&token))
}

fn looks_like_listing_command(command: &str) -> bool {
    command.contains("rg --files")
        || shell_uses_command(command, &["ls", "tree", "eza", "exa", "fd", "find"])
        || (shell_uses_command(command, &["pwd"]) && command.contains("&&"))
}

fn looks_like_search_command(command: &str) -> bool {
    !command.contains("rg --files")
        && shell_uses_command(command, &["rg", "ripgrep", "grep", "ack", "ag"])
}

fn looks_like_read_command(command: &str) -> bool {
    shell_uses_command(
        command,
        &["cat", "bat", "sed", "awk", "head", "tail", "less", "more", "nl"],
    )
}

fn looks_like_git_inspection_command(command: &str) -> bool {
    if !shell_uses_command(command, &["git"]) {
        return false;
    }
    command.contains("git status")
        || command.contains("git diff")
        || command.contains("git show")
        || command.contains("git log")
        || command.contains("git branch")
}

fn command_looks_like_verification(command: &str) -> bool {
    let normalized = command.to_ascii_lowercase();
    let verification_markers = [
        "cargo test",
        "cargo clippy",
        "cargo check",
        "go test",
        "pytest",
        "dotnet test",
        "mvn test",
        "gradle test",
        "jest",
        "vitest",
        "eslint",
        "ruff check",
        "tsc",
    ];
    verification_markers
        .iter()
        .any(|marker| normalized.contains(marker))
}

fn collaboration_mode_for_turn(
    mode: ModeKind,
    model: &str,
    reasoning_effort: ReasoningEffort,
) -> Option<CollaborationMode> {
    // `turn/start.collaboration_mode` is sticky in app-server: when set, it applies to
    // this and subsequent turns. Send an explicit `default` mode when plan mode is off
    // so clients can reliably exit plan mode without stale state.
    Some(CollaborationMode {
        mode,
        settings: CollaborationSettings {
            model: model.to_string(),
            reasoning_effort: Some(reasoning_effort),
            developer_instructions: None,
        },
    })
}

fn collaboration_mode_label(mode: ModeKind) -> &'static str {
    match mode {
        ModeKind::Plan => "plan",
        _ => "default",
    }
}

fn parse_collaboration_mode(value: &str) -> Option<ModeKind> {
    match value {
        "plan" | "on" => Some(ModeKind::Plan),
        "default" | "off" | "code" => Some(ModeKind::Default),
        _ => None,
    }
}

fn turn_plan_step_to_entry(step: TurnPlanStep) -> PlanEntry {
    PlanEntry::new(
        step.step,
        PlanEntryPriority::Medium,
        match step.status {
            TurnPlanStepStatus::Pending => PlanEntryStatus::Pending,
            TurnPlanStepStatus::InProgress => PlanEntryStatus::InProgress,
            TurnPlanStepStatus::Completed => PlanEntryStatus::Completed,
        },
    )
}

fn plan_from_text(text: &str) -> Option<Plan> {
    let entries = text
        .lines()
        .filter_map(parse_plan_entry_from_line)
        .collect::<Vec<_>>();
    if entries.is_empty() {
        None
    } else {
        Some(Plan::new(entries))
    }
}

fn normalize_plan_for_ui(plan: Plan) -> Plan {
    Plan::new(limit_plan_entries(plan.entries))
}

fn promote_first_pending_step(plan: Plan) -> Plan {
    let mut entries = plan.entries;
    if entries
        .iter()
        .all(|entry| entry.status == PlanEntryStatus::Pending)
        && let Some(first) = entries.first_mut()
    {
        first.status = PlanEntryStatus::InProgress;
    }
    Plan::new(entries)
}

fn limit_plan_entries(mut entries: Vec<PlanEntry>) -> Vec<PlanEntry> {
    if entries.len() > MAX_VISIBLE_PLAN_ENTRIES {
        entries.truncate(MAX_VISIBLE_PLAN_ENTRIES);
    }
    entries
}

fn parse_plan_entry_from_line(line: &str) -> Option<PlanEntry> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    if let Some(content) = trimmed
        .strip_prefix("- [x] ")
        .or_else(|| trimmed.strip_prefix("* [x] "))
    {
        let content = content.trim();
        if content.is_empty() {
            return None;
        }
        return Some(PlanEntry::new(
            content,
            PlanEntryPriority::Medium,
            PlanEntryStatus::Completed,
        ));
    }

    if let Some(content) = trimmed
        .strip_prefix("- [ ] ")
        .or_else(|| trimmed.strip_prefix("* [ ] "))
    {
        let content = content.trim();
        if content.is_empty() {
            return None;
        }
        return Some(PlanEntry::new(
            content,
            PlanEntryPriority::Medium,
            PlanEntryStatus::Pending,
        ));
    }

    if let Some(content) = trimmed
        .strip_prefix("- [~] ")
        .or_else(|| trimmed.strip_prefix("* [~] "))
        .or_else(|| trimmed.strip_prefix("- [-] "))
        .or_else(|| trimmed.strip_prefix("* [-] "))
    {
        let content = content.trim();
        if content.is_empty() {
            return None;
        }
        return Some(PlanEntry::new(
            content,
            PlanEntryPriority::Medium,
            PlanEntryStatus::InProgress,
        ));
    }

    // Proposed plans from app-server commonly use plain bullets/numbering
    // (for example: "- first", "1. second") without checkbox markers.
    if let Some(content) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| strip_numbered_prefix(trimmed))
    {
        let content = content.trim();
        if content.is_empty() {
            return None;
        }
        return Some(PlanEntry::new(
            content,
            PlanEntryPriority::Medium,
            PlanEntryStatus::Pending,
        ));
    }

    None
}

fn strip_numbered_prefix(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == 0 || index >= bytes.len() {
        return None;
    }

    if bytes[index] != b'.' && bytes[index] != b')' {
        return None;
    }
    index += 1;
    if index >= bytes.len() || !bytes[index].is_ascii_whitespace() {
        return None;
    }
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }

    Some(&line[index..])
}

fn i64_to_u64_saturating(value: i64) -> u64 {
    if value <= 0 { 0 } else { value as u64 }
}

fn map_command_status(status: CommandExecutionStatus, assume_in_progress: bool) -> ToolCallStatus {
    if assume_in_progress {
        return ToolCallStatus::InProgress;
    }
    match status {
        CommandExecutionStatus::Completed => ToolCallStatus::Completed,
        CommandExecutionStatus::InProgress
        | CommandExecutionStatus::Failed
        | CommandExecutionStatus::Declined => ToolCallStatus::Failed,
    }
}

fn map_patch_status(status: PatchApplyStatus, assume_in_progress: bool) -> ToolCallStatus {
    if assume_in_progress {
        return ToolCallStatus::InProgress;
    }
    match status {
        PatchApplyStatus::Completed => ToolCallStatus::Completed,
        PatchApplyStatus::InProgress | PatchApplyStatus::Failed | PatchApplyStatus::Declined => {
            ToolCallStatus::Failed
        }
    }
}

fn map_mcp_status(status: McpToolCallStatus, assume_in_progress: bool) -> ToolCallStatus {
    if assume_in_progress {
        return ToolCallStatus::InProgress;
    }
    match status {
        McpToolCallStatus::Completed => ToolCallStatus::Completed,
        McpToolCallStatus::InProgress | McpToolCallStatus::Failed => ToolCallStatus::Failed,
    }
}

fn to_app_approval(policy: AskForApproval) -> AppAskForApproval {
    match policy {
        AskForApproval::UnlessTrusted => AppAskForApproval::UnlessTrusted,
        AskForApproval::OnFailure => AppAskForApproval::OnFailure,
        AskForApproval::OnRequest => AppAskForApproval::OnRequest,
        AskForApproval::Never => AppAskForApproval::Never,
    }
}

fn to_app_sandbox_mode(policy: &SandboxPolicy) -> AppSandboxMode {
    match policy {
        SandboxPolicy::ReadOnly => AppSandboxMode::ReadOnly,
        SandboxPolicy::WorkspaceWrite { .. } | SandboxPolicy::ExternalSandbox { .. } => {
            AppSandboxMode::WorkspaceWrite
        }
        SandboxPolicy::DangerFullAccess => AppSandboxMode::DangerFullAccess,
    }
}

fn to_app_sandbox_policy(policy: &SandboxPolicy) -> AppSandboxPolicy {
    AppSandboxPolicy::from(policy.clone())
}

fn policy_to_mode(policy: &AppSandboxPolicy) -> AppSandboxMode {
    match policy {
        AppSandboxPolicy::ReadOnly => AppSandboxMode::ReadOnly,
        AppSandboxPolicy::WorkspaceWrite { .. } | AppSandboxPolicy::ExternalSandbox { .. } => {
            AppSandboxMode::WorkspaceWrite
        }
        AppSandboxPolicy::DangerFullAccess => AppSandboxMode::DangerFullAccess,
    }
}

fn mode_state(
    approval: AppAskForApproval,
    sandbox: AppSandboxMode,
    edit_approval_mode: EditApprovalMode,
    collaboration_mode_kind: ModeKind,
) -> SessionModeState {
    let current = APPROVAL_PRESETS
        .iter()
        .find(|preset| {
            to_app_approval(preset.approval) == approval && to_app_sandbox_mode(&preset.sandbox) == sandbox
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
                SessionMode::new(AUTO_MODE_ID, preset.label)
                    .description("Default mode: file edits are auto-approved (Plan mode still asks)."),
            );
            available_modes.push(
                SessionMode::new(AUTO_ASK_EDITS_MODE_ID, "Default (Ask on edits)")
                    .description("Default mode with confirmation popup for every file edit."),
            );
        } else {
            available_modes.push(
                SessionMode::new(preset.id, preset.label).description(preset.description),
            );
        }
    }
    available_modes.push(
        SessionMode::new(PLAN_SESSION_MODE_ID, "Plan")
            .description("Plan-first mode with visible step tracking (uses Default sandbox/approval)."),
    );

    SessionModeState::new(
        current_mode_id,
        available_modes,
    )
}

fn session_model_state(models: &[AppModel], current_model: &str) -> SessionModelState {
    let mut available_models = models
        .iter()
        .map(|model| {
            ModelInfo::new(ModelId::new(model.id.clone()), model.display_name.clone())
                .description(model.description.clone())
        })
        .collect::<Vec<_>>();

    let current_model_id = find_model_for_current(models, current_model)
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

fn config_options(
    models: &[AppModel],
    current_model: &str,
    current_reasoning_effort: ReasoningEffort,
    current_usage_percent: Option<u64>,
    approval: AppAskForApproval,
    sandbox: AppSandboxMode,
    edit_approval_mode: EditApprovalMode,
    collaboration_mode_kind: ModeKind,
) -> Vec<SessionConfigOption> {
    let mode_state = mode_state(approval, sandbox, edit_approval_mode, collaboration_mode_kind);
    let current_model_id = find_model_for_current(models, current_model)
        .map(|model| model.id.clone())
        .unwrap_or_else(|| current_model.to_string());

    let mut options = vec![
        SessionConfigOption::select(
            "mode",
            "Approval Preset",
            mode_state.current_mode_id.0,
            mode_state
                .available_modes
                .into_iter()
                .map(|mode| {
                    SessionConfigSelectOption::new(mode.id.0, mode.name).description(mode.description)
                })
                .collect::<Vec<_>>(),
        )
        .category(SessionConfigOptionCategory::Mode)
        .description("Choose an approval and sandboxing preset for your session"),
    ];

    let mut model_options = models
        .iter()
        .map(|model| {
            SessionConfigSelectOption::new(model.id.clone(), model.display_name.clone())
                .description(model.description.clone())
        })
        .collect::<Vec<_>>();

    if !model_options
        .iter()
        .any(|option| option.value.0.as_ref() == current_model_id)
    {
        model_options.push(SessionConfigSelectOption::new(
            current_model_id.clone(),
            current_model_id.clone(),
        ));
    }

    options.push(
        SessionConfigOption::select("model", "Model", current_model_id.clone(), model_options)
            .category(SessionConfigOptionCategory::Model)
            .description("Choose which model Codex should use"),
    );

    let mut reasoning_options = find_model_for_current(models, current_model)
        .map(|model| {
            model
                .supported_reasoning_efforts
                .iter()
                .map(|option| {
                    SessionConfigSelectOption::new(
                        reasoning_effort_value(option.reasoning_effort),
                        reasoning_effort_option_label(
                            option.reasoning_effort,
                            current_reasoning_effort,
                            current_usage_percent,
                        ),
                    )
                    .description(option.description.clone())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if reasoning_options.is_empty() {
        reasoning_options.push(SessionConfigSelectOption::new(
            reasoning_effort_value(current_reasoning_effort),
            reasoning_effort_option_label(
                current_reasoning_effort,
                current_reasoning_effort,
                current_usage_percent,
            ),
        ));
    }

    if !reasoning_options
        .iter()
        .any(|option| option.value.0.as_ref() == reasoning_effort_value(current_reasoning_effort))
    {
        reasoning_options.push(SessionConfigSelectOption::new(
            reasoning_effort_value(current_reasoning_effort),
            reasoning_effort_option_label(
                current_reasoning_effort,
                current_reasoning_effort,
                current_usage_percent,
            ),
        ));
    }

    options.push(
        SessionConfigOption::select(
            "reasoning_effort",
            "Reasoning Effort",
            reasoning_effort_value(current_reasoning_effort),
            reasoning_options,
        )
        .category(SessionConfigOptionCategory::Model)
        .description("Choose how much reasoning effort Codex should use"),
    );

    options
}

fn find_model_for_current<'a>(models: &'a [AppModel], current_model: &str) -> Option<&'a AppModel> {
    models
        .iter()
        .find(|model| model.id == current_model || model.model == current_model)
}

fn resolve_reasoning_effort(
    models: &[AppModel],
    current_model: &str,
    current: Option<ReasoningEffort>,
) -> ReasoningEffort {
    let effort = current.unwrap_or_default();
    normalize_reasoning_effort_for_model(models, current_model, effort)
}

fn normalize_reasoning_effort_for_model(
    models: &[AppModel],
    current_model: &str,
    current_effort: ReasoningEffort,
) -> ReasoningEffort {
    let Some(model) = find_model_for_current(models, current_model) else {
        return current_effort;
    };
    if model
        .supported_reasoning_efforts
        .iter()
        .any(|option| option.reasoning_effort == current_effort)
    {
        return current_effort;
    }
    model.default_reasoning_effort
}

fn parse_reasoning_effort(value: &str) -> Option<ReasoningEffort> {
    match value {
        "none" => Some(ReasoningEffort::None),
        "minimal" => Some(ReasoningEffort::Minimal),
        "low" => Some(ReasoningEffort::Low),
        "medium" => Some(ReasoningEffort::Medium),
        "high" => Some(ReasoningEffort::High),
        "xhigh" => Some(ReasoningEffort::XHigh),
        _ => None,
    }
}

fn reasoning_effort_value(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::None => "none",
        ReasoningEffort::Minimal => "minimal",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::XHigh => "xhigh",
    }
}

fn reasoning_effort_label(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::None => "None",
        ReasoningEffort::Minimal => "Minimal",
        ReasoningEffort::Low => "Low",
        ReasoningEffort::Medium => "Medium",
        ReasoningEffort::High => "High",
        ReasoningEffort::XHigh => "Extra High",
    }
}

fn reasoning_effort_option_label(
    effort: ReasoningEffort,
    current_effort: ReasoningEffort,
    current_usage_percent: Option<u64>,
) -> String {
    if effort == current_effort
        && let Some(percent) = current_usage_percent
    {
        return format!("{} · {}% ctx", reasoning_effort_label(effort), percent);
    }
    reasoning_effort_label(effort).to_string()
}

fn usage_percent(used: Option<u64>, size: Option<u64>) -> Option<u64> {
    let used = used?;
    let size = size?;
    if size == 0 {
        return None;
    }
    let clamped = used.min(size);
    Some(((clamped as f64 / size as f64) * 100.0).round() as u64)
}

fn build_prompt_items(prompt: Vec<ContentBlock>) -> Vec<UserInput> {
    prompt
        .into_iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text_block) => Some(UserInput::Text {
                text: text_block.text,
                text_elements: vec![],
            }),
            ContentBlock::Image(image_block) => Some(UserInput::Image {
                url: format!("data:{};base64,{}", image_block.mime_type, image_block.data),
            }),
            ContentBlock::ResourceLink(ResourceLink { name, uri, .. }) => Some(UserInput::Text {
                text: format_uri_as_link(Some(name), uri),
                text_elements: vec![],
            }),
            ContentBlock::Resource(EmbeddedResource {
                resource:
                    EmbeddedResourceResource::TextResourceContents(TextResourceContents {
                        text,
                        uri,
                        ..
                    }),
                ..
            }) => Some(UserInput::Text {
                text: format!(
                    "{}\n<context ref=\"{uri}\">\n{text}\n</context>",
                    format_uri_as_link(None, uri.clone())
                ),
                text_elements: vec![],
            }),
            ContentBlock::Audio(..) | ContentBlock::Resource(..) | _ => None,
        })
        .collect()
}

fn parse_session_command(prompt: &[ContentBlock]) -> Option<SessionCommand> {
    let text = match prompt {
        [ContentBlock::Text(text)] => text.text.trim(),
        _ => return None,
    };

    if text == "/plan" || text.starts_with("/plan ") {
        let rest = text["/plan".len()..].trim();
        if rest.is_empty() {
            return Some(SessionCommand::PlanMode {
                raw_value: None,
                mode: None,
            });
        }

        let first = rest
            .split_whitespace()
            .next()
            .map(str::to_lowercase)
            .unwrap_or_default();
        let words = rest.split_whitespace().count();
        if words == 1
            && let Some(mode) = parse_collaboration_mode(&first)
        {
            return Some(SessionCommand::PlanMode {
                raw_value: Some(first),
                mode: Some(mode),
            });
        }

        return Some(SessionCommand::PlanPrompt {
            prompt: rest.to_string(),
        });
    }

    let mut parts = text.split_whitespace();
    match parts.next()? {
        "/threads" => Some(SessionCommand::Threads),
        "/resume" => {
            let rest = text["/resume".len()..].trim();
            Some(SessionCommand::Resume {
                thread_id: if rest.is_empty() {
                    None
                } else {
                    Some(rest.to_string())
                },
            })
        }
        "/compact" => Some(SessionCommand::Compact),
        "/undo" => {
            let num_turns = parts
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|value| *value > 0)
                .unwrap_or(1);
            Some(SessionCommand::Undo { num_turns })
        }
        "/reasoning" | "/effort" => {
            let raw_value = parts.next().map(ToString::to_string);
            let effort = raw_value.as_deref().and_then(parse_reasoning_effort);
            Some(SessionCommand::Reasoning { raw_value, effort })
        }
        "/context" => Some(SessionCommand::Context),
        _ => None,
    }
}

fn normalize_preview(preview: &str) -> String {
    let compact = preview.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        "(no preview)".to_string()
    } else if compact.chars().count() > 120 {
        let short = compact.chars().take(117).collect::<String>();
        format!("{short}...")
    } else {
        compact
    }
}

fn builtin_commands() -> Vec<AvailableCommand> {
    vec![
        AvailableCommand::new("threads", "List saved Codex threads for this account"),
        AvailableCommand::new(
            "resume",
            "Resume a thread. Without args: pick from current workspace; with args: search by partial id",
        )
        .input(
            AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(
                "optional partial thread id",
            )),
        ),
        AvailableCommand::new(
            "compact",
            "Summarize the conversation to free context window",
        ),
        AvailableCommand::new("undo", "Rollback the most recent turn(s)").input(
            AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(
                "optional number of turns (default 1)",
            )),
        ),
        AvailableCommand::new(
            "reasoning",
            "Show or set reasoning effort (`none|minimal|low|medium|high|xhigh`)",
        )
        .input(AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(
            "optional effort value",
        ))),
        AvailableCommand::new(
            "plan",
            "Show/set plan mode (`on|off`) or run one-shot planning with `/plan <request>`",
        )
        .input(AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(
            "optional mode or request",
        ))),
        AvailableCommand::new("context", "Show current context window usage"),
    ]
}

fn format_uri_as_link(name: Option<String>, uri: String) -> String {
    if let Some(name) = name
        && !name.is_empty()
    {
        format!("[@{name}]({uri})")
    } else if let Some(path) = uri.strip_prefix("file://") {
        let name = Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        format!("[@{name}]({uri})")
    } else if uri.starts_with("zed://") {
        let name = uri.split('/').next_back().unwrap_or(&uri);
        format!("[@{name}]({uri})")
    } else {
        uri
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_threads_command() {
        let prompt: Vec<ContentBlock> = vec!["/threads".into()];
        assert_eq!(parse_session_command(&prompt), Some(SessionCommand::Threads));
    }

    #[test]
    fn parses_resume_command_with_thread_id() {
        let prompt: Vec<ContentBlock> = vec!["/resume thread_123".into()];
        assert_eq!(
            parse_session_command(&prompt),
            Some(SessionCommand::Resume {
                thread_id: Some("thread_123".to_string()),
            })
        );
    }

    #[test]
    fn parses_resume_command_without_thread_id() {
        let prompt: Vec<ContentBlock> = vec!["/resume".into()];
        assert_eq!(
            parse_session_command(&prompt),
            Some(SessionCommand::Resume { thread_id: None })
        );
    }

    #[test]
    fn parses_resume_command_with_partial_query() {
        let prompt: Vec<ContentBlock> = vec!["/resume 019c6455".into()];
        assert_eq!(
            parse_session_command(&prompt),
            Some(SessionCommand::Resume {
                thread_id: Some("019c6455".to_string()),
            })
        );
    }

    #[test]
    fn ignores_regular_prompt_text() {
        let prompt: Vec<ContentBlock> = vec!["continue this task".into()];
        assert_eq!(parse_session_command(&prompt), None);
    }

    #[test]
    fn parses_compact_command() {
        let prompt: Vec<ContentBlock> = vec!["/compact".into()];
        assert_eq!(parse_session_command(&prompt), Some(SessionCommand::Compact));
    }

    #[test]
    fn parses_undo_command_with_optional_count() {
        let prompt: Vec<ContentBlock> = vec!["/undo 2".into()];
        assert_eq!(
            parse_session_command(&prompt),
            Some(SessionCommand::Undo { num_turns: 2 })
        );
    }

    #[test]
    fn parses_reasoning_command_without_value() {
        let prompt: Vec<ContentBlock> = vec!["/reasoning".into()];
        assert_eq!(
            parse_session_command(&prompt),
            Some(SessionCommand::Reasoning {
                raw_value: None,
                effort: None,
            })
        );
    }

    #[test]
    fn parses_reasoning_command_with_value() {
        let prompt: Vec<ContentBlock> = vec!["/reasoning xhigh".into()];
        assert_eq!(
            parse_session_command(&prompt),
            Some(SessionCommand::Reasoning {
                raw_value: Some("xhigh".to_string()),
                effort: Some(ReasoningEffort::XHigh),
            })
        );
    }

    #[test]
    fn parses_context_command() {
        let prompt: Vec<ContentBlock> = vec!["/context".into()];
        assert_eq!(parse_session_command(&prompt), Some(SessionCommand::Context));
    }

    #[test]
    fn parses_plan_command_without_value() {
        let prompt: Vec<ContentBlock> = vec!["/plan".into()];
        assert_eq!(
            parse_session_command(&prompt),
            Some(SessionCommand::PlanMode {
                raw_value: None,
                mode: None,
            })
        );
    }

    #[test]
    fn parses_plan_command_with_on_value() {
        let prompt: Vec<ContentBlock> = vec!["/plan on".into()];
        assert_eq!(
            parse_session_command(&prompt),
            Some(SessionCommand::PlanMode {
                raw_value: Some("on".to_string()),
                mode: Some(ModeKind::Plan),
            })
        );
    }

    #[test]
    fn parses_plan_command_with_prompt() {
        let prompt: Vec<ContentBlock> = vec!["/plan разбей задачу на шаги".into()];
        assert_eq!(
            parse_session_command(&prompt),
            Some(SessionCommand::PlanPrompt {
                prompt: "разбей задачу на шаги".to_string(),
            })
        );
    }

    #[test]
    fn parses_plan_command_with_unknown_single_word_as_prompt() {
        let prompt: Vec<ContentBlock> = vec!["/plan maybe".into()];
        assert_eq!(
            parse_session_command(&prompt),
            Some(SessionCommand::PlanPrompt {
                prompt: "maybe".to_string(),
            })
        );
    }

    #[test]
    fn parses_plan_entries_from_markdown_lines() {
        let plan = plan_from_text(
            "# Plan\n- [x] done\n- [ ] pending\n- [~] running\n- bullet\n1. numbered\n2) alternate\nplain text",
        )
        .expect("expected plan entries");

        assert_eq!(plan.entries.len(), 6);
        assert_eq!(plan.entries[0].content, "done");
        assert_eq!(plan.entries[0].status, PlanEntryStatus::Completed);
        assert_eq!(plan.entries[1].content, "pending");
        assert_eq!(plan.entries[1].status, PlanEntryStatus::Pending);
        assert_eq!(plan.entries[2].content, "running");
        assert_eq!(plan.entries[2].status, PlanEntryStatus::InProgress);
        assert_eq!(plan.entries[3].content, "bullet");
        assert_eq!(plan.entries[3].status, PlanEntryStatus::Pending);
        assert_eq!(plan.entries[4].content, "numbered");
        assert_eq!(plan.entries[4].status, PlanEntryStatus::Pending);
        assert_eq!(plan.entries[5].content, "alternate");
        assert_eq!(plan.entries[5].status, PlanEntryStatus::Pending);
    }

    #[test]
    fn parses_plain_proposed_plan_block() {
        let plan = plan_from_text("# Final plan\n- first\n- second\n")
            .expect("expected proposed plan entries");

        assert_eq!(plan.entries.len(), 2);
        assert_eq!(plan.entries[0].content, "first");
        assert_eq!(plan.entries[0].status, PlanEntryStatus::Pending);
        assert_eq!(plan.entries[1].content, "second");
        assert_eq!(plan.entries[1].status, PlanEntryStatus::Pending);
    }

    #[test]
    fn limits_large_plans_for_ui() {
        let entries = (1..=12)
            .map(|index| {
                PlanEntry::new(
                    format!("step {index}"),
                    PlanEntryPriority::Medium,
                    PlanEntryStatus::Pending,
                )
            })
            .collect::<Vec<_>>();

        let limited = limit_plan_entries(entries);
        assert_eq!(limited.len(), MAX_VISIBLE_PLAN_ENTRIES);
        assert_eq!(limited[0].content, "step 1");
        assert_eq!(limited.last().map(|entry| entry.content.clone()), Some("step 6".to_string()));
        assert!(limited
            .iter()
            .all(|entry| entry.status == PlanEntryStatus::Pending));
    }

    #[test]
    fn fallback_plan_entries_track_phase_progression() {
        let planning = fallback_plan_entries(FallbackPlanPhase::Planning);
        let implementing = fallback_plan_entries(FallbackPlanPhase::Implementing);
        let done = fallback_plan_entries(FallbackPlanPhase::Done);

        assert_eq!(planning.len(), 4);
        assert_eq!(planning[0].status, PlanEntryStatus::InProgress);
        assert_eq!(planning[1].status, PlanEntryStatus::Pending);

        assert_eq!(implementing[0].status, PlanEntryStatus::Completed);
        assert_eq!(implementing[1].status, PlanEntryStatus::InProgress);
        assert_eq!(implementing[2].status, PlanEntryStatus::Pending);

        assert!(done
            .iter()
            .all(|entry| entry.status == PlanEntryStatus::Completed));
    }

    #[test]
    fn promote_first_pending_step_marks_only_first_step_in_progress() {
        let plan = Plan::new(vec![
            PlanEntry::new("step 1", PlanEntryPriority::Medium, PlanEntryStatus::Pending),
            PlanEntry::new("step 2", PlanEntryPriority::Medium, PlanEntryStatus::Pending),
            PlanEntry::new("step 3", PlanEntryPriority::Medium, PlanEntryStatus::Pending),
        ]);

        let promoted = promote_first_pending_step(plan);
        assert_eq!(promoted.entries[0].status, PlanEntryStatus::InProgress);
        assert_eq!(promoted.entries[1].status, PlanEntryStatus::Pending);
        assert_eq!(promoted.entries[2].status, PlanEntryStatus::Pending);
    }

    #[test]
    fn promote_first_pending_step_preserves_existing_statuses() {
        let plan = Plan::new(vec![
            PlanEntry::new("step 1", PlanEntryPriority::Medium, PlanEntryStatus::Completed),
            PlanEntry::new("step 2", PlanEntryPriority::Medium, PlanEntryStatus::InProgress),
            PlanEntry::new("step 3", PlanEntryPriority::Medium, PlanEntryStatus::Pending),
        ]);

        let promoted = promote_first_pending_step(plan.clone());
        assert_eq!(promoted.entries, plan.entries);
    }

    #[test]
    fn fallback_plan_can_enter_summarizing_only_after_tool_activity_and_no_active_calls() {
        let state = FallbackPlanState {
            turn_id: "turn_1".to_string(),
            phase: FallbackPlanPhase::Verifying,
            saw_tool_activity: true,
            steps: vec![],
        };
        assert!(fallback_plan_can_enter_summarizing(
            Some(&state),
            "turn_1",
            false
        ));
        assert!(!fallback_plan_can_enter_summarizing(
            Some(&state),
            "turn_1",
            true
        ));
    }

    #[test]
    fn plan_entries_all_pending_detects_mixed_statuses() {
        let all_pending = vec![
            PlanEntry::new("a", PlanEntryPriority::Medium, PlanEntryStatus::Pending),
            PlanEntry::new("b", PlanEntryPriority::Medium, PlanEntryStatus::Pending),
        ];
        let mixed = vec![
            PlanEntry::new("a", PlanEntryPriority::Medium, PlanEntryStatus::InProgress),
            PlanEntry::new("b", PlanEntryPriority::Medium, PlanEntryStatus::Pending),
        ];
        assert!(plan_entries_all_pending(&all_pending));
        assert!(!plan_entries_all_pending(&mixed));
    }

    #[test]
    fn fallback_plan_does_not_advance_to_done_without_tool_activity() {
        let state = FallbackPlanState {
            turn_id: "turn_1".to_string(),
            phase: FallbackPlanPhase::Planning,
            saw_tool_activity: false,
            steps: vec![],
        };
        assert!(!fallback_plan_should_advance(&state, FallbackPlanPhase::Done));
    }

    #[test]
    fn fallback_plan_can_advance_to_done_after_tool_activity() {
        let state = FallbackPlanState {
            turn_id: "turn_1".to_string(),
            phase: FallbackPlanPhase::Summarizing,
            saw_tool_activity: true,
            steps: vec![],
        };
        assert!(fallback_plan_should_advance(&state, FallbackPlanPhase::Done));
    }

    #[test]
    fn fallback_plan_cannot_enter_summarizing_without_tool_activity() {
        let state = FallbackPlanState {
            turn_id: "turn_1".to_string(),
            phase: FallbackPlanPhase::Planning,
            saw_tool_activity: false,
            steps: vec![],
        };
        assert!(!fallback_plan_can_enter_summarizing(
            Some(&state),
            "turn_1",
            false
        ));
    }

    #[test]
    fn detects_verification_commands() {
        assert!(command_looks_like_verification("cargo test -q"));
        assert!(command_looks_like_verification("go test ./..."));
        assert!(command_looks_like_verification("ruff check ."));
        assert!(!command_looks_like_verification("rg --files"));
        assert!(!command_looks_like_verification("cat README.md"));
    }

    #[test]
    fn command_title_uses_parsed_actions_when_available() {
        let actions = vec![CommandAction::ListFiles {
            command: "rg --files".to_string(),
            path: None,
        }];
        assert_eq!(
            command_tool_title("/bin/bash -lc 'echo hello'", &actions),
            "Analyze folder contents"
        );
    }

    #[test]
    fn command_title_reads_single_file_name_from_action() {
        let actions = vec![CommandAction::Read {
            command: "cat src/thread.rs".to_string(),
            name: "cat".to_string(),
            path: PathBuf::from("src/thread.rs"),
        }];
        assert_eq!(command_tool_title("cat src/thread.rs", &actions), "Read thread.rs");
    }

    #[test]
    fn command_title_maps_common_shell_listing_commands() {
        assert_eq!(
            command_tool_title("/bin/bash -lc 'pwd && ls -la'", &[]),
            "Analyze folder contents"
        );
        assert_eq!(
            command_tool_title("/bin/bash -lc 'rg --files | head -n 200'", &[]),
            "Analyze folder contents"
        );
    }

    #[test]
    fn command_title_maps_common_shell_search_and_check_commands() {
        assert_eq!(
            command_tool_title("/bin/bash -lc 'rg \"plan\" src/thread.rs'", &[]),
            "Search in workspace"
        );
        assert_eq!(
            command_tool_title("/bin/bash -lc 'cargo test -q'", &[]),
            "Run tests and checks"
        );
    }

    #[test]
    fn command_title_falls_back_for_unknown_commands() {
        assert_eq!(
            command_tool_title("/bin/bash -lc 'echo done'", &[]),
            "Run shell command"
        );
    }

    #[test]
    fn command_tool_kind_uses_search_for_listing_and_grep_commands() {
        assert_eq!(
            command_tool_kind("/bin/bash -lc 'pwd && ls -la'", &[]),
            ToolKind::Search
        );
        assert_eq!(
            command_tool_kind("/bin/bash -lc 'rg \"plan\" src/thread.rs'", &[]),
            ToolKind::Search
        );
    }

    #[test]
    fn command_tool_kind_uses_read_for_file_reads() {
        let actions = vec![CommandAction::Read {
            command: "cat src/thread.rs".to_string(),
            name: "cat".to_string(),
            path: PathBuf::from("src/thread.rs"),
        }];
        assert_eq!(command_tool_kind("cat src/thread.rs", &actions), ToolKind::Read);
    }

    #[test]
    fn command_tool_kind_falls_back_to_think_for_other_shell_commands() {
        assert_eq!(
            command_tool_kind("/bin/bash -lc 'echo done'", &[]),
            ToolKind::Think
        );
    }

    #[test]
    fn parses_reasoning_effort_values() {
        assert_eq!(parse_reasoning_effort("medium"), Some(ReasoningEffort::Medium));
        assert_eq!(parse_reasoning_effort("high"), Some(ReasoningEffort::High));
        assert_eq!(parse_reasoning_effort("xhigh"), Some(ReasoningEffort::XHigh));
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
        let mode =
            collaboration_mode_for_turn(ModeKind::Plan, "gpt-5.3-codex", ReasoningEffort::XHigh)
                .expect("mode should always be explicit");

        assert_eq!(mode.mode, ModeKind::Plan);
        assert_eq!(mode.settings.model, "gpt-5.3-codex");
        assert_eq!(mode.settings.reasoning_effort, Some(ReasoningEffort::XHigh));
    }

    #[test]
    fn file_change_approval_is_always_prompted_in_plan_mode() {
        assert!(should_prompt_file_change_approval(
            ModeKind::Plan,
            EditApprovalMode::AutoApprove
        ));
        assert!(should_prompt_file_change_approval(
            ModeKind::Plan,
            EditApprovalMode::AskEveryEdit
        ));
    }

    #[test]
    fn file_change_approval_respects_edit_mode_in_default_mode() {
        assert!(!should_prompt_file_change_approval(
            ModeKind::Default,
            EditApprovalMode::AutoApprove
        ));
        assert!(should_prompt_file_change_approval(
            ModeKind::Default,
            EditApprovalMode::AskEveryEdit
        ));
    }

    #[test]
    fn mode_state_uses_custom_auto_ask_edits_id() {
        let auto_preset = APPROVAL_PRESETS
            .iter()
            .find(|preset| preset.id == AUTO_MODE_ID)
            .expect("auto preset should exist");
        let state = mode_state(
            to_app_approval(auto_preset.approval),
            to_app_sandbox_mode(&auto_preset.sandbox),
            EditApprovalMode::AskEveryEdit,
            ModeKind::Default,
        );
        assert_eq!(state.current_mode_id.0.as_ref(), AUTO_ASK_EDITS_MODE_ID);
    }

    #[test]
    fn to_app_sandbox_policy_preserves_workspace_write_settings() {
        let policy = SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![],
            network_access: true,
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        };

        assert_eq!(
            to_app_sandbox_policy(&policy),
            AppSandboxPolicy::WorkspaceWrite {
                writable_roots: vec![],
                network_access: true,
                exclude_tmpdir_env_var: true,
                exclude_slash_tmp: true,
            }
        );
    }

    #[test]
    fn to_app_sandbox_policy_preserves_external_sandbox() {
        let policy = SandboxPolicy::ExternalSandbox {
            network_access: codex_protocol::protocol::NetworkAccess::Enabled,
        };

        assert_eq!(
            to_app_sandbox_policy(&policy),
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

    #[test]
    fn apply_unified_diff_to_text_reconstructs_content() {
        let old_text = "one\ntwo\nthree\n";
        let unified_diff = "\
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,4 @@
 one
-two
+TWO
 three
+four
";
        let new_text = apply_unified_diff_to_text(old_text, unified_diff)
            .expect("diff should be applicable to old content");
        assert_eq!(new_text, "one\nTWO\nthree\nfour\n");
    }

    #[test]
    fn unified_diff_to_old_new_ignores_move_suffix() {
        let diff = "\
--- a/src/old.txt
+++ b/src/new.txt
@@ -1 +1 @@
-before
+after

Moved to: src/new.txt
";
        let (old_text, new_text) =
            unified_diff_to_old_new(diff).expect("should extract old/new hunk text");
        assert_eq!(old_text, "before\n");
        assert_eq!(new_text, "after\n");
    }

    #[test]
    fn unified_diff_to_old_new_keeps_hunk_lines_starting_with_header_prefixes() {
        let diff = "\
--- a/src/example.txt
+++ b/src/example.txt
@@ -1 +1 @@
---- starts-with-triple-dash
++++ starts-with-triple-plus
";
        let (old_text, new_text) =
            unified_diff_to_old_new(diff).expect("should keep hunk body lines intact");
        assert_eq!(old_text, "--- starts-with-triple-dash\n");
        assert_eq!(new_text, "+++ starts-with-triple-plus\n");
    }

    #[test]
    fn parse_turn_unified_diff_files_handles_add_update_delete() {
        let diff = "\
diff --git a/src/update.txt b/src/update.txt
--- a/src/update.txt
+++ b/src/update.txt
@@ -1 +1 @@
-old
+new
diff --git a/src/add.txt b/src/add.txt
new file mode 100644
--- /dev/null
+++ b/src/add.txt
@@ -0,0 +1 @@
+added
diff --git a/src/delete.txt b/src/delete.txt
deleted file mode 100644
--- a/src/delete.txt
+++ /dev/null
@@ -1 +0,0 @@
-removed
";

        let files = parse_turn_unified_diff_files(diff);
        assert_eq!(files.len(), 3);

        assert_eq!(files[0].path, PathBuf::from("src/update.txt"));
        assert_eq!(files[0].old_text, "old\n");
        assert_eq!(files[0].new_text, "new\n");
        assert!(!files[0].is_delete);

        assert_eq!(files[1].path, PathBuf::from("src/add.txt"));
        assert_eq!(files[1].old_text, "");
        assert_eq!(files[1].new_text, "added\n");
        assert!(!files[1].is_delete);

        assert_eq!(files[2].path, PathBuf::from("src/delete.txt"));
        assert_eq!(files[2].old_text, "removed\n");
        assert_eq!(files[2].new_text, "");
        assert!(files[2].is_delete);
    }

    #[test]
    fn parse_turn_unified_diff_files_normalizes_quoted_paths() {
        let diff = "\
diff --git \"a/src/space file.txt\" \"b/src/space file.txt\"
--- \"a/src/space file.txt\"
+++ \"b/src/space file.txt\"
@@ -1 +1 @@
-before
+after
";

        let files = parse_turn_unified_diff_files(diff);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("src/space file.txt"));
        assert_eq!(files[0].old_text, "before\n");
        assert_eq!(files[0].new_text, "after\n");
        assert!(!files[0].is_delete);
    }

    #[test]
    fn parse_turn_unified_diff_files_ignores_sections_without_hunks() {
        let diff = "\
diff --git a/src/example.txt b/src/example.txt
--- a/src/example.txt
+++ b/src/example.txt
";

        let files = parse_turn_unified_diff_files(diff);
        assert!(files.is_empty());
    }

    #[test]
    fn replay_diff_for_update_uses_old_and_new_text() {
        let change = codex_app_server_protocol::FileUpdateChange {
            path: "README.md".to_string(),
            kind: PatchChangeKind::Update { move_path: None },
            diff: "\
--- a/README.md
+++ b/README.md
@@ -1 +1 @@
-hello
+world
"
            .to_string(),
        };

        let diff = file_change_to_replay_diff(Path::new("/tmp/workspace"), change);
        assert_eq!(diff.path, PathBuf::from("/tmp/workspace/README.md"));
        assert_eq!(diff.old_text.as_deref(), Some("hello\n"));
        assert_eq!(diff.new_text, "world\n");
    }

    #[test]
    fn replay_diff_for_add_uses_unified_hunk_when_available() {
        let change = codex_app_server_protocol::FileUpdateChange {
            path: "notes.md".to_string(),
            kind: PatchChangeKind::Add,
            diff: "\
--- /dev/null
+++ b/notes.md
@@ -0,0 +1,2 @@
+line one
+line two
"
            .to_string(),
        };

        let diff = file_change_to_replay_diff(Path::new("/tmp/workspace"), change);
        assert_eq!(diff.path, PathBuf::from("/tmp/workspace/notes.md"));
        assert_eq!(diff.old_text.as_deref(), None);
        assert_eq!(diff.new_text, "line one\nline two\n");
    }

    #[test]
    fn replay_diff_for_delete_uses_unified_hunk_when_available() {
        let change = codex_app_server_protocol::FileUpdateChange {
            path: "notes.md".to_string(),
            kind: PatchChangeKind::Delete,
            diff: "\
--- a/notes.md
+++ /dev/null
@@ -1,2 +0,0 @@
-line one
-line two
"
            .to_string(),
        };

        let diff = file_change_to_replay_diff(Path::new("/tmp/workspace"), change);
        assert_eq!(diff.path, PathBuf::from("/tmp/workspace/notes.md"));
        assert_eq!(diff.old_text.as_deref(), Some("line one\nline two\n"));
        assert_eq!(diff.new_text, "");
    }

    #[test]
    fn file_change_tool_location_uses_move_target_and_hunk_line() {
        let change = codex_app_server_protocol::FileUpdateChange {
            path: "src/old.rs".to_string(),
            kind: PatchChangeKind::Update {
                move_path: Some(PathBuf::from("src/new.rs")),
            },
            diff: "\
--- a/src/old.rs
+++ b/src/new.rs
@@ -3,2 +8,3 @@
-old
+new
 keep
"
            .to_string(),
        };

        let location = file_change_tool_location(Path::new("/tmp/workspace"), &change);
        assert_eq!(location.path, PathBuf::from("/tmp/workspace/src/new.rs"));
        assert_eq!(location.line, Some(7));
    }

    #[test]
    fn file_change_tool_location_defaults_to_first_line_for_non_unified_add() {
        let change = codex_app_server_protocol::FileUpdateChange {
            path: "notes.txt".to_string(),
            kind: PatchChangeKind::Add,
            diff: "hello\nworld\n".to_string(),
        };

        let location = file_change_tool_location(Path::new("/tmp/workspace"), &change);
        assert_eq!(location.path, PathBuf::from("/tmp/workspace/notes.txt"));
        assert_eq!(location.line, Some(0));
    }

    #[test]
    fn request_user_input_options_include_none_of_the_above_when_supported() {
        let question = ToolRequestUserInputQuestion {
            id: "q1".to_string(),
            header: "Header".to_string(),
            question: "Question?".to_string(),
            is_other: true,
            is_secret: false,
            options: Some(vec![codex_app_server_protocol::ToolRequestUserInputOption {
                label: "Yes".to_string(),
                description: "Continue".to_string(),
            }]),
        };

        let (options, answer_labels_by_option_id, _) =
            build_request_user_input_permission_options(0, &question);

        assert_eq!(options.len(), 2);
        assert_eq!(answer_labels_by_option_id.len(), 2);
        assert_eq!(options[0].kind, PermissionOptionKind::AllowOnce);
        assert_eq!(options[1].kind, PermissionOptionKind::AllowOnce);
        assert!(answer_labels_by_option_id.values().any(|label| label == "Yes"));
        assert!(
            answer_labels_by_option_id
                .values()
                .any(|label| label == NONE_OF_THE_ABOVE)
        );
    }

    #[test]
    fn request_user_input_options_do_not_add_none_of_the_above_without_base_options() {
        let question = ToolRequestUserInputQuestion {
            id: "q1".to_string(),
            header: "Header".to_string(),
            question: "Question?".to_string(),
            is_other: true,
            is_secret: false,
            options: None,
        };

        let (options, answer_labels_by_option_id, _) =
            build_request_user_input_permission_options(0, &question);

        assert!(options.is_empty());
        assert!(answer_labels_by_option_id.is_empty());
    }

}
