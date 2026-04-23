//! Поток старта и остановки сессии: создание session, bootstrap capability и очистка.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::{
    EnvVariable, HttpHeader, McpServer, McpServerHttp, McpServerSse, McpServerStdio,
};
use codex_core::config::types::{McpServerConfig, McpServerTransportConfig};
use codex_core::plugins::PluginsManager;
use codex_core::skills::SkillsManager;
use serde_json::Value as JsonValue;

use super::session_config::{
    AccountStatus, ContextSelectorSummary, build_mcp_summary, build_skills_summary, policy_to_mode,
    resolve_reasoning_effort, service_tier_override_from_config,
    service_tier_override_from_session, to_app_approval, to_app_sandbox_mode,
};
use super::{
    AppServerProcess, ClientCapabilities, Config, ContextUsageSource, EditApprovalMode, Error,
    ListSessionsResponse, ModeKind, SessionClient, SessionId, Thread, ThreadInner,
    ThreadListParams, ThreadResumeParams, ThreadSortKey, ThreadStartParams,
};
use crate::thread::features::collab::remember_agent_label;
use crate::thread::features::resume::common::thread_display_title;
use crate::thread::session_usage_cache::{context_usage_cache_path, restore_cached_context_usage};
use codex_app_server_protocol::{ThreadForkParams, ThreadResumeResponse, ThreadStartResponse};
use tracing::{info, warn};

const RESUME_STARTUP_RETRY_ATTEMPTS: usize = 6;
const RESUME_STARTUP_RETRY_DELAY_MS: u64 = 300;

pub(crate) struct SessionMcpSetup {
    pub(crate) config_overrides: Option<HashMap<String, JsonValue>>,
    pub(crate) summary: ContextSelectorSummary,
}

struct StartedBackendThread {
    app: AppServerProcess,
    start: ThreadStartResponse,
}

fn startup_error(stage: &str, error: Error) -> Error {
    Error::internal_error().data(format!("{stage}: {error}"))
}

fn format_session_updated_at(updated_at: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(updated_at, 0)
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| updated_at.to_string())
}

fn pending_skills_summary() -> ContextSelectorSummary {
    ContextSelectorSummary {
        label: "Skills · loading".to_string(),
        description: "Discovering workspace skills for this session.".to_string(),
        report: "Workspace skills are still being discovered for this session.".to_string(),
    }
}

fn pending_plugins_summary() -> ContextSelectorSummary {
    ContextSelectorSummary {
        label: "Plugins · loading".to_string(),
        description: "Discovering plugin marketplaces for this session.".to_string(),
        report: "Plugin marketplaces are still being discovered for this session.".to_string(),
    }
}

pub(crate) fn build_session_mcp_setup(
    base_mcp_servers: &HashMap<String, McpServerConfig>,
    cwd: &Path,
    mcp_servers: Vec<McpServer>,
) -> Result<SessionMcpSetup, Error> {
    let mut merged_mcp_servers = base_mcp_servers.clone();
    let mut saw_supported_server = false;

    for mcp_server in mcp_servers {
        match mcp_server {
            McpServer::Http(server) => {
                saw_supported_server = true;
                insert_http_mcp_server(&mut merged_mcp_servers, server);
            }
            McpServer::Stdio(server) => {
                saw_supported_server = true;
                insert_stdio_mcp_server(&mut merged_mcp_servers, cwd, server);
            }
            McpServer::Sse(McpServerSse { name, .. }) => {
                warn!(
                    server_name = %name,
                    "ACP requested an SSE MCP server, but codex app-server currently supports only stdio/streamable HTTP passthrough; ignoring server"
                );
            }
            _ => {
                warn!("ACP requested an unknown MCP server transport; ignoring server");
            }
        }
    }

    let summary = build_mcp_summary(&merged_mcp_servers);

    let config_overrides = if saw_supported_server {
        let mut overrides = HashMap::new();
        overrides.insert(
            "mcp_servers".to_string(),
            serde_json::to_value(merged_mcp_servers)
                .map_err(|err| Error::internal_error().data(err.to_string()))?,
        );
        Some(overrides)
    } else {
        None
    };

    Ok(SessionMcpSetup {
        config_overrides,
        summary,
    })
}

#[cfg(test)]
pub(crate) fn build_session_mcp_config_overrides(
    base_mcp_servers: &HashMap<String, McpServerConfig>,
    cwd: &Path,
    mcp_servers: Vec<McpServer>,
) -> Result<Option<HashMap<String, JsonValue>>, Error> {
    Ok(build_session_mcp_setup(base_mcp_servers, cwd, mcp_servers)?.config_overrides)
}

fn insert_http_mcp_server(target: &mut HashMap<String, McpServerConfig>, server: McpServerHttp) {
    let McpServerHttp {
        name, url, headers, ..
    } = server;
    target.insert(
        normalize_mcp_server_name(&name),
        McpServerConfig {
            transport: McpServerTransportConfig::StreamableHttp {
                url,
                bearer_token_env_var: None,
                http_headers: headers_to_map(headers),
                env_http_headers: None,
            },
            required: false,
            enabled: true,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            disabled_tools: None,
            enabled_tools: None,
            disabled_reason: None,
            scopes: None,
            oauth_resource: None,
        },
    );
}

fn insert_stdio_mcp_server(
    target: &mut HashMap<String, McpServerConfig>,
    cwd: &Path,
    server: McpServerStdio,
) {
    let McpServerStdio {
        name,
        command,
        args,
        env,
        ..
    } = server;
    target.insert(
        normalize_mcp_server_name(&name),
        McpServerConfig {
            transport: McpServerTransportConfig::Stdio {
                command: command.display().to_string(),
                args,
                env: env_to_map(env),
                env_vars: vec![],
                cwd: Some(cwd.to_path_buf()),
            },
            required: false,
            enabled: true,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            disabled_tools: None,
            enabled_tools: None,
            disabled_reason: None,
            scopes: None,
            oauth_resource: None,
        },
    );
}

fn normalize_mcp_server_name(name: &str) -> String {
    let normalized = name.replace(|c: char| c.is_whitespace(), "_");
    if normalized.is_empty() {
        "mcp_server".to_string()
    } else {
        normalized
    }
}

pub(in crate::thread) async fn load_session_skills_summary_for_cwd(
    codex_home: &Path,
    bundled_skills_enabled: bool,
    cwd: &Path,
) -> ContextSelectorSummary {
    let plugins_manager = Arc::new(PluginsManager::new(codex_home.to_path_buf()));
    let skills_manager = SkillsManager::new(
        codex_home.to_path_buf(),
        plugins_manager,
        bundled_skills_enabled,
    );
    let outcome = skills_manager.skills_for_cwd(cwd, false).await;
    build_skills_summary(&outcome)
}

fn headers_to_map(headers: Vec<HttpHeader>) -> Option<HashMap<String, String>> {
    if headers.is_empty() {
        None
    } else {
        Some(
            headers
                .into_iter()
                .map(|header| (header.name, header.value))
                .collect(),
        )
    }
}

fn env_to_map(env: Vec<EnvVariable>) -> Option<HashMap<String, String>> {
    if env.is_empty() {
        None
    } else {
        Some(
            env.into_iter()
                .map(|entry| (entry.name, entry.value))
                .collect(),
        )
    }
}

pub(in crate::thread) fn is_missing_rollout_thread_error(error: &Error) -> bool {
    let message = error.to_string();
    message.contains("no rollout found for thread id")
}

pub(in crate::thread) async fn thread_resume_with_startup_retry(
    app: &mut AppServerProcess,
    params: ThreadResumeParams,
) -> Result<codex_app_server_protocol::ThreadResumeResponse, Error> {
    for attempt in 0..=RESUME_STARTUP_RETRY_ATTEMPTS {
        match app.thread_resume(params.clone()).await {
            Ok(response) => return Ok(response),
            Err(error)
                if is_missing_rollout_thread_error(&error)
                    && attempt < RESUME_STARTUP_RETRY_ATTEMPTS =>
            {
                let retry_number = attempt + 1;
                warn!(
                    retry_number,
                    thread_id = params.thread_id,
                    "thread/resume reported missing rollout during startup; retrying"
                );
                tokio::time::sleep(Duration::from_millis(
                    RESUME_STARTUP_RETRY_DELAY_MS * retry_number as u64,
                ))
                .await;
            }
            Err(error) => return Err(error),
        }
    }

    unreachable!("retry loop must return on success or final error")
}

async fn spawn_initialized_app(session_id: Option<&SessionId>) -> Result<AppServerProcess, Error> {
    let mut app = AppServerProcess::spawn("codex")
        .await
        .map_err(|error| startup_error("failed to spawn `codex app-server`", error))?;
    if let Some(session_id) = session_id {
        info!(session_id = %session_id, "Initializing codex app-server");
    } else {
        info!("Initializing codex app-server");
    }
    app.initialize("codex-acp-cas", "Codex ACP CAS")
        .await
        .map_err(|error| startup_error("failed to initialize `codex app-server`", error))?;
    Ok(app)
}

async fn start_backend_thread(
    config: &Config,
    cwd: &Path,
    session_id: Option<&SessionId>,
    session_mcp_config_overrides: Option<HashMap<String, JsonValue>>,
) -> Result<StartedBackendThread, Error> {
    let mut app = spawn_initialized_app(session_id).await?;
    if let Some(session_id) = session_id {
        info!(session_id = %session_id, "Starting backend thread");
    } else {
        info!(cwd = %cwd.display(), "Starting backend thread for new ACP session");
    }
    let start = app
        .thread_start(ThreadStartParams {
            model: config.model.clone(),
            model_provider: Some(config.model_provider_id.clone()),
            service_tier: service_tier_override_from_config(config.service_tier),
            cwd: Some(cwd.to_string_lossy().to_string()),
            approval_policy: Some(to_app_approval(*config.permissions.approval_policy.get())),
            sandbox: Some(to_app_sandbox_mode(config.permissions.sandbox_policy.get())),
            config: session_mcp_config_overrides,
            ..Default::default()
        })
        .await
        .map_err(|error| startup_error("failed to start backend thread", error))?;
    Ok(StartedBackendThread { app, start })
}

impl Thread {
    async fn fork_as_new_session(
        &self,
        config: &Config,
        cwd: PathBuf,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
        requested_session_mcp_config_overrides: Option<HashMap<String, JsonValue>>,
        requested_session_mcp_summary: ContextSelectorSummary,
    ) -> Result<(SessionId, Self), Error> {
        let requested_mcp_override_present = requested_session_mcp_config_overrides.is_some();
        let (
            source_thread_id,
            session_id,
            resumed_cwd,
            session_mcp_config_overrides,
            session_mcp_summary,
        ) = {
            let inner = self.inner.lock().await;
            let source_thread_id = inner.thread_id.clone();
            let source_model = inner.current_model.clone();
            let source_model_provider = inner.current_model_provider.clone();
            let source_service_tier = inner.service_tier;
            let source_approval_policy = inner.approval_policy;
            let source_sandbox_mode = inner.sandbox_mode;
            let session_mcp_config_overrides = if requested_mcp_override_present {
                requested_session_mcp_config_overrides.clone()
            } else {
                inner.session_mcp_config_overrides.clone()
            };
            let session_mcp_summary = if requested_mcp_override_present {
                requested_session_mcp_summary
            } else {
                inner.session_mcp_summary.clone()
            };
            let fork = match inner
                .app
                .lock()
                .await
                .thread_fork(ThreadForkParams {
                    thread_id: source_thread_id.clone(),
                    model: Some(source_model),
                    model_provider: Some(source_model_provider),
                    service_tier: service_tier_override_from_session(source_service_tier),
                    cwd: Some(cwd.to_string_lossy().to_string()),
                    approval_policy: Some(source_approval_policy),
                    sandbox: Some(source_sandbox_mode),
                    config: session_mcp_config_overrides.clone(),
                    ..Default::default()
                })
                .await
            {
                Ok(fork) => fork,
                Err(error) if is_missing_rollout_thread_error(&error) => {
                    return Err(Error::invalid_params().data(
                        "Current session is not ready to fork yet. Send at least one prompt first, then try again.",
                    ));
                }
                Err(error) => return Err(error),
            };

            (
                source_thread_id,
                SessionId::new(fork.thread.id),
                fork.thread.cwd,
                session_mcp_config_overrides,
                session_mcp_summary,
            )
        };

        info!(
            session_id = %session_id,
            source_thread_id = %source_thread_id,
            cwd = %resumed_cwd.display(),
            "Bootstrapping forked ACP session"
        );

        let thread = Self::resume_session(
            session_id.clone(),
            config,
            resumed_cwd,
            client_capabilities,
            session_mcp_config_overrides,
            session_mcp_summary,
        )
        .await?;

        Ok((session_id, thread))
    }

    async fn build_resumed_thread(
        session_id: SessionId,
        config: &Config,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
        session_mcp_config_overrides: Option<HashMap<String, JsonValue>>,
        session_mcp_summary: ContextSelectorSummary,
        mut app: AppServerProcess,
        resume: ThreadResumeResponse,
    ) -> Self {
        let models = match app.model_list().await {
            Ok(response) => response.data,
            Err(error) => {
                warn!(
                    error = %error,
                    session_id = %session_id,
                "Failed to load model list during resumed session startup"
                );
                Vec::new()
            }
        };
        let reasoning_effort =
            resolve_reasoning_effort(&models, &resume.model, resume.reasoning_effort);
        let context_usage_cache_path = context_usage_cache_path(&config.codex_home);
        let cached_context_usage = restore_cached_context_usage(
            &context_usage_cache_path,
            &resume.thread.id,
            &resume.thread.turns,
        );
        let resumed_workspace_cwd = resume.thread.cwd.clone();
        let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(0_u64);
        let mut agent_labels = HashMap::new();
        remember_agent_label(
            &mut agent_labels,
            resume.thread.id.clone(),
            resume.thread.agent_nickname.clone(),
            resume.thread.agent_role.clone(),
        );

        Thread {
            inner: tokio::sync::Mutex::new(ThreadInner {
                session_id: session_id.clone(),
                app: Arc::new(tokio::sync::Mutex::new(app)),
                codex_home: config.codex_home.clone(),
                bundled_skills_enabled: config.bundled_skills_enabled(),
                thread_id: resume.thread.id,
                context_usage_cache_path,
                session_mcp_config_overrides,
                session_mcp_summary,
                session_skills_summary: pending_skills_summary(),
                session_plugins_summary: pending_plugins_summary(),
                account_status: AccountStatus::default(),
                workspace_cwd: resumed_workspace_cwd,
                client: SessionClient::new(session_id, client_capabilities),
                approval_policy: resume.approval_policy,
                sandbox_policy: resume.sandbox.clone(),
                sandbox_mode: policy_to_mode(&resume.sandbox),
                edit_approval_mode: EditApprovalMode::AutoApprove,
                collaboration_mode_kind: ModeKind::Default,
                current_model: resume.model,
                current_model_provider: resume.model_provider,
                service_tier: resume.service_tier,
                reasoning_effort,
                agent_labels,
                compaction_in_progress: false,
                last_used_tokens: cached_context_usage.map(|(used, _)| used),
                total_token_usage: None,
                context_window_size: cached_context_usage.map(|(_, size)| size),
                context_usage_source: cached_context_usage.map(|_| ContextUsageSource::Cached),
                account_rate_limits: None,
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
                pending_thread_title_update: None,
                replay_turns: resume.thread.turns,
                history_replay_in_progress: false,
                turn_last_progress_at: std::time::Instant::now(),
                turn_reconnect_warning_count: 0,
                turn_reconnect_retry_limit_hit: false,
            }),
            cancel_tx,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn build_started_thread(
        session_id: SessionId,
        codex_home: PathBuf,
        bundled_skills_enabled: bool,
        cwd: PathBuf,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
        session_mcp_config_overrides: Option<HashMap<String, JsonValue>>,
        session_mcp_summary: ContextSelectorSummary,
        session_skills_summary: ContextSelectorSummary,
        account_status: AccountStatus,
        mut app: AppServerProcess,
        start: ThreadStartResponse,
    ) -> Self {
        let models = match app.model_list().await {
            Ok(response) => response.data,
            Err(error) => {
                warn!(error = %error, "Failed to load model list during session startup");
                Vec::new()
            }
        };
        let reasoning_effort =
            resolve_reasoning_effort(&models, &start.model, start.reasoning_effort);

        let (cancel_tx, _cancel_rx) = tokio::sync::watch::channel(0_u64);
        let mut agent_labels = HashMap::new();
        remember_agent_label(
            &mut agent_labels,
            start.thread.id.clone(),
            start.thread.agent_nickname.clone(),
            start.thread.agent_role.clone(),
        );

        Thread {
            inner: tokio::sync::Mutex::new(ThreadInner {
                session_id: session_id.clone(),
                app: Arc::new(tokio::sync::Mutex::new(app)),
                codex_home: codex_home.clone(),
                bundled_skills_enabled,
                thread_id: start.thread.id,
                context_usage_cache_path: context_usage_cache_path(&codex_home),
                session_mcp_config_overrides,
                session_mcp_summary,
                session_skills_summary,
                session_plugins_summary: pending_plugins_summary(),
                account_status,
                workspace_cwd: cwd,
                client: SessionClient::new(session_id, client_capabilities),
                approval_policy: start.approval_policy,
                sandbox_policy: start.sandbox.clone(),
                sandbox_mode: policy_to_mode(&start.sandbox),
                edit_approval_mode: EditApprovalMode::AutoApprove,
                collaboration_mode_kind: ModeKind::Default,
                current_model: start.model,
                current_model_provider: start.model_provider,
                service_tier: start.service_tier,
                reasoning_effort,
                agent_labels,
                compaction_in_progress: false,
                last_used_tokens: None,
                total_token_usage: None,
                context_window_size: None,
                context_usage_source: None,
                account_rate_limits: None,
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
                pending_thread_title_update: None,
                replay_turns: vec![],
                history_replay_in_progress: false,
                turn_last_progress_at: std::time::Instant::now(),
                turn_reconnect_warning_count: 0,
                turn_reconnect_retry_limit_hit: false,
            }),
            cancel_tx,
        }
    }

    pub(crate) async fn start_session_for_existing_session_id(
        session_id: SessionId,
        config: &Config,
        cwd: PathBuf,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
        session_mcp_config_overrides: Option<HashMap<String, JsonValue>>,
        session_mcp_summary: ContextSelectorSummary,
    ) -> Result<Self, Error> {
        info!(
            session_id = %session_id,
            cwd = %cwd.display(),
            "Bootstrapping fresh backend thread for existing ACP session"
        );
        let StartedBackendThread { app, start } = start_backend_thread(
            config,
            &cwd,
            Some(&session_id),
            session_mcp_config_overrides.clone(),
        )
        .await?;

        Ok(Self::build_started_thread(
            session_id,
            config.codex_home.clone(),
            config.bundled_skills_enabled(),
            cwd,
            client_capabilities,
            session_mcp_config_overrides,
            session_mcp_summary,
            pending_skills_summary(),
            AccountStatus::default(),
            app,
            start,
        )
        .await)
    }

    // Сначала запускаем сессию app-server, чтобы последующие capability-вызовы имели валидный session id.
    pub async fn start_session(
        config: &Config,
        cwd: PathBuf,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
        session_mcp_config_overrides: Option<HashMap<String, JsonValue>>,
        session_mcp_summary: ContextSelectorSummary,
    ) -> Result<(SessionId, Self), Error> {
        info!(cwd = %cwd.display(), "Bootstrapping new ACP session");
        let StartedBackendThread { app, start } =
            start_backend_thread(config, &cwd, None, session_mcp_config_overrides.clone()).await?;

        let session_id = SessionId::new(start.thread.id.clone());
        let thread = Self::build_started_thread(
            session_id.clone(),
            config.codex_home.clone(),
            config.bundled_skills_enabled(),
            cwd,
            client_capabilities,
            session_mcp_config_overrides,
            session_mcp_summary,
            pending_skills_summary(),
            AccountStatus::default(),
            app,
            start,
        )
        .await;

        Ok((session_id, thread))
    }

    pub async fn resume_session(
        session_id: SessionId,
        config: &Config,
        cwd: PathBuf,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
        session_mcp_config_overrides: Option<HashMap<String, JsonValue>>,
        session_mcp_summary: ContextSelectorSummary,
    ) -> Result<Self, Error> {
        info!(
            session_id = %session_id,
            cwd = %cwd.display(),
            "Bootstrapping resumed ACP session"
        );
        let mut app = AppServerProcess::spawn("codex")
            .await
            .map_err(|error| startup_error("failed to spawn `codex app-server`", error))?;
        info!(session_id = %session_id, "Initializing codex app-server");
        app.initialize("codex-acp-cas", "Codex ACP CAS")
            .await
            .map_err(|error| startup_error("failed to initialize `codex app-server`", error))?;

        let resume_params = ThreadResumeParams {
            thread_id: session_id.0.to_string(),
            model: config.model.clone(),
            model_provider: Some(config.model_provider_id.clone()),
            service_tier: service_tier_override_from_config(config.service_tier),
            cwd: Some(cwd.to_string_lossy().to_string()),
            approval_policy: Some(to_app_approval(*config.permissions.approval_policy.get())),
            sandbox: Some(to_app_sandbox_mode(config.permissions.sandbox_policy.get())),
            config: session_mcp_config_overrides.clone(),
            ..Default::default()
        };

        let resume = match thread_resume_with_startup_retry(&mut app, resume_params.clone()).await {
            Ok(resume) => resume,
            Err(error) if is_missing_rollout_thread_error(&error) => {
                warn!(
                    requested_thread_id = resume_params.thread_id,
                    "resume source is unavailable or not materialized; starting a fresh backend thread for this ACP session"
                );
                return Self::start_session_for_existing_session_id(
                    session_id,
                    config,
                    cwd,
                    client_capabilities,
                    session_mcp_config_overrides,
                    session_mcp_summary,
                )
                .await;
            }
            Err(error) => return Err(error),
        };
        Ok(Self::build_resumed_thread(
            session_id,
            config,
            client_capabilities,
            session_mcp_config_overrides,
            session_mcp_summary,
            app,
            resume,
        )
        .await)
    }

    pub async fn fork_session(
        &self,
        config: &Config,
        cwd: PathBuf,
        client_capabilities: Arc<Mutex<ClientCapabilities>>,
        requested_session_mcp_config_overrides: Option<HashMap<String, JsonValue>>,
        requested_session_mcp_summary: ContextSelectorSummary,
    ) -> Result<(SessionId, Self), Error> {
        self.fork_as_new_session(
            config,
            cwd,
            client_capabilities,
            requested_session_mcp_config_overrides,
            requested_session_mcp_summary,
        )
        .await
    }

    pub async fn list_sessions(
        config: &Config,
        cwd: Option<PathBuf>,
        cursor: Option<String>,
    ) -> Result<ListSessionsResponse, Error> {
        // ACP-клиенты (в т.ч. Zed) часто вызывают session/list без cwd.
        // По умолчанию ведём себя как CLI resume: показываем сессии текущего workspace.
        let effective_cwd = cwd.or_else(|| Some(config.cwd.clone()));

        info!(
            cwd = %effective_cwd
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<none>".to_string()),
            "Listing ACP sessions via codex app-server"
        );
        let mut app = AppServerProcess::spawn("codex")
            .await
            .map_err(|error| startup_error("failed to spawn `codex app-server`", error))?;
        info!("Initializing codex app-server for session list");
        app.initialize("codex-acp-cas", "Codex ACP CAS")
            .await
            .map_err(|error| startup_error("failed to initialize `codex app-server`", error))?;

        let response = app
            .thread_list(ThreadListParams {
                cursor,
                limit: Some(25),
                sort_key: Some(ThreadSortKey::UpdatedAt),
                model_providers: None,
                source_kinds: None,
                archived: Some(false),
                cwd: effective_cwd
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
                search_term: None,
            })
            .await
            .map_err(|error| startup_error("failed to list backend threads", error))?;

        let sessions = response
            .data
            .into_iter()
            .map(|thread| {
                let title = thread_display_title(&thread);
                agent_client_protocol::SessionInfo::new(SessionId::new(thread.id), thread.cwd)
                    .title(Some(title))
                    .updated_at(Some(format_session_updated_at(thread.updated_at)))
            })
            .collect();

        Ok(ListSessionsResponse::new(sessions).next_cursor(response.next_cursor))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_session_mcp_config_overrides, format_session_updated_at,
        is_missing_rollout_thread_error,
    };
    use agent_client_protocol::{Error, McpServer, McpServerHttp, McpServerSse, McpServerStdio};
    use codex_core::config::types::{McpServerConfig, McpServerTransportConfig};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn detects_retryable_missing_rollout_resume_error() {
        let error = Error::internal_error()
            .data("thread/resume failed: no rollout found for thread id 019-test (code -32600)");
        assert!(is_missing_rollout_thread_error(&error));
    }

    #[test]
    fn ignores_other_resume_errors() {
        let error = Error::internal_error().data("thread/resume failed: auth required");
        assert!(!is_missing_rollout_thread_error(&error));
    }

    #[test]
    fn detects_retryable_missing_rollout_even_in_wrapped_error_text() {
        let error = Error::internal_error()
            .data("Internal error: \"no rollout found for thread id 019-test\"");
        assert!(is_missing_rollout_thread_error(&error));
    }

    #[test]
    fn formats_session_list_updated_at_as_rfc3339() {
        let formatted = format_session_updated_at(1_775_014_896);

        let parsed = chrono::DateTime::parse_from_rfc3339(&formatted)
            .expect("session list updated_at should be RFC3339");
        assert_eq!(parsed.timestamp(), 1_775_014_896);
    }

    #[test]
    fn builds_session_mcp_overrides_for_stdio_and_http_servers() {
        let cwd = PathBuf::from("/tmp/workspace");
        let overrides = build_session_mcp_config_overrides(
            &HashMap::new(),
            &cwd,
            vec![
                McpServer::Stdio(
                    McpServerStdio::new("local files", "/bin/mcp-server")
                        .args(vec!["--root".to_string(), "/tmp".to_string()]),
                ),
                McpServer::Http(McpServerHttp::new(
                    "remote tools",
                    "https://example.com/mcp",
                )),
            ],
        )
        .expect("mcp overrides")
        .expect("non-empty overrides");

        let mcp_servers = overrides
            .get("mcp_servers")
            .and_then(|value| value.as_object())
            .expect("mcp_servers object");

        let stdio = mcp_servers
            .get("local_files")
            .expect("normalized stdio server");
        assert_eq!(
            stdio.get("command").and_then(|value| value.as_str()),
            Some("/bin/mcp-server")
        );
        assert_eq!(
            stdio.get("cwd").and_then(|value| value.as_str()),
            Some("/tmp/workspace")
        );

        let http = mcp_servers
            .get("remote_tools")
            .expect("normalized http server");
        assert_eq!(
            http.get("url").and_then(|value| value.as_str()),
            Some("https://example.com/mcp")
        );
    }

    #[test]
    fn keeps_base_mcp_servers_and_ignores_sse_servers() {
        let cwd = PathBuf::from("/tmp/workspace");
        let mut base = HashMap::new();
        base.insert(
            "base".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::Stdio {
                    command: "/bin/base-mcp".to_string(),
                    args: vec![],
                    env: None,
                    env_vars: vec![],
                    cwd: None,
                },
                required: false,
                enabled: true,
                disabled_reason: None,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
                scopes: None,
                oauth_resource: None,
            },
        );

        let overrides = build_session_mcp_config_overrides(
            &base,
            &cwd,
            vec![McpServer::Sse(McpServerSse::new(
                "remote sse",
                "https://example.com/sse",
            ))],
        )
        .expect("sse passthrough should not error");

        assert!(
            overrides.is_none(),
            "unsupported-only inputs should not override base config"
        );
    }
}
