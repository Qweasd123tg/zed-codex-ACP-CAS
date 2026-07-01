//! Реализация ACP `Agent`, которая сопоставляет жизненный цикл ACP-сессии с `Thread`
//! Каждый `Thread` работает поверх Codex app-server.

use acp::{
    Agent, Client, ConnectTo, ConnectionTo, Dispatch, Error, Handled, UntypedMessage,
    schema::ProtocolVersion,
    schema::v1::{
        AgentAuthCapabilities, AgentCapabilities, AuthMethod, AuthMethodAgent, AuthMethodId,
        AuthenticateRequest, AuthenticateResponse, CancelNotification, ClientCapabilities,
        CloseSessionRequest, CloseSessionResponse, DeleteSessionRequest, DeleteSessionResponse,
        ExtRequest, ExtResponse, ForkSessionRequest, ForkSessionResponse, Implementation,
        InitializeRequest, InitializeResponse, ListSessionsRequest, ListSessionsResponse,
        LoadSessionRequest, LoadSessionResponse, LogoutCapabilities, LogoutRequest, LogoutResponse,
        McpCapabilities, NewSessionRequest, NewSessionResponse, PromptCapabilities, PromptRequest,
        PromptResponse, ResumeSessionRequest, ResumeSessionResponse, SessionCapabilities,
        SessionCloseCapabilities, SessionDeleteCapabilities, SessionForkCapabilities, SessionId,
        SessionListCapabilities, SessionResumeCapabilities, SetSessionConfigOptionRequest,
        SetSessionConfigOptionResponse, SetSessionModeRequest, SetSessionModeResponse,
    },
};
use agent_client_protocol as acp;
use codex_core::{
    CodexAuth,
    auth::{AuthManager, read_codex_api_key_from_env, read_openai_api_key_from_env},
    config::Config,
};
use codex_login::{CODEX_API_KEY_ENV_VAR, OPENAI_API_KEY_ENV_VAR};
use serde::Deserialize;
use serde_json::value::to_raw_value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tracing::{debug, info, warn};

use crate::thread::{
    SessionMcpSetup, Thread, build_session_mcp_setup, is_missing_rollout_thread_error,
};

const EXT_THREAD_ROLLBACK_METHOD: &str = "zed.dev/codex/thread/rollback";

pub struct CodexAgent {
    auth_manager: Arc<AuthManager>,
    client_capabilities: Arc<RwLock<ClientCapabilities>>,
    config: Config,
    auto_restore_enabled: bool,
    startup_instant: Instant,
    startup_restore_bypassed: AtomicBool,
    // Реестр активных ACP-сессий в памяти на время жизни процесса.
    sessions: Arc<Mutex<HashMap<SessionId, Arc<Thread>>>>,
}

struct ExistingSessionBootstrap {
    thread: Arc<Thread>,
    restored_backend_history: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingSessionRestoreFailureAction {
    StartFreshBackend,
    ReturnError,
}

fn existing_session_restore_failure_action(error: &Error) -> ExistingSessionRestoreFailureAction {
    if is_missing_rollout_thread_error(error) {
        ExistingSessionRestoreFailureAction::StartFreshBackend
    } else {
        ExistingSessionRestoreFailureAction::ReturnError
    }
}

const STARTUP_RESTORE_GUARD_WINDOW: Duration = Duration::from_secs(5);
const STARTUP_COMMANDS_SYNC_DELAY: Duration = Duration::from_millis(200);
const SLOW_STARTUP_NOTICE_THRESHOLD: Duration = Duration::from_millis(1500);

impl CodexAgent {
    fn auto_restore_enabled_from_env() -> bool {
        std::env::var("ACP_DISABLE_AUTO_RESTORE")
            .map(|value| {
                let normalized = value.trim().to_ascii_lowercase();
                normalized.is_empty() || !matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
            })
            .unwrap_or(true)
    }

    // Сохраняем общий стартовый конфиг, который используется всеми ACP-сессиями.
    pub fn new(config: Config) -> Self {
        let auth_manager = AuthManager::shared(
            config.codex_home.clone(),
            false,
            config.cli_auth_credentials_store_mode,
        );
        let auto_restore_enabled = Self::auto_restore_enabled_from_env();

        Self {
            auth_manager,
            client_capabilities: Arc::default(),
            config,
            auto_restore_enabled,
            startup_instant: Instant::now(),
            startup_restore_bypassed: AtomicBool::new(false),
            sessions: Arc::default(),
        }
    }

    fn get_thread(&self, session_id: &SessionId) -> Result<Arc<Thread>, Error> {
        self.sessions
            .lock()
            .map_err(|_| Error::internal_error().data("session registry lock poisoned"))?
            .get(session_id)
            .cloned()
            .ok_or_else(|| Error::resource_not_found(None))
    }

    async fn check_auth(&self) -> Result<(), Error> {
        // Для провайдера OpenAI один из поддерживаемых способов авторизации должен быть готов
        // до принятия любого вызова session/prompt.
        if self.config.model_provider_id == "openai" && self.auth_manager.auth().await.is_none() {
            return Err(Error::auth_required());
        }
        Ok(())
    }

    fn spawn_post_load_startup_tasks(
        thread: Arc<Thread>,
        replay_loaded_history: bool,
        startup_elapsed: Duration,
    ) {
        let notify_thread = thread.clone();
        tokio::task::spawn(async move {
            // Сначала отдаём клиенту session response, затем публикуем динамические
            // метаданные команд, чтобы не ловить UI-гонки на старте новой ACP-сессии.
            tokio::task::yield_now().await;
            tokio::time::sleep(STARTUP_COMMANDS_SYNC_DELAY).await;
            notify_thread.notify_usage_update().await;
            notify_thread.notify_available_commands().await;
            if startup_elapsed >= SLOW_STARTUP_NOTICE_THRESHOLD {
                notify_thread
                    .notify_slow_startup_ready(startup_elapsed)
                    .await;
            }
            if replay_loaded_history {
                notify_thread.replay_loaded_history().await;
            }
        });

        tokio::task::spawn(async move {
            thread.refresh_startup_metadata().await;
        });
    }

    async fn load_and_register_session(
        &self,
        session_id: SessionId,
        thread: Arc<Thread>,
        replay_loaded_history: bool,
        started_at: Instant,
    ) -> Result<LoadSessionResponse, Error> {
        let load = thread.load().await?;
        if replay_loaded_history {
            thread.mark_history_replay_pending().await;
        }
        Self::spawn_post_load_startup_tasks(
            thread.clone(),
            replay_loaded_history,
            started_at.elapsed(),
        );
        self.sessions
            .lock()
            .map_err(|_| Error::internal_error().data("session registry lock poisoned"))?
            .insert(session_id, thread);
        Ok(load)
    }

    #[allow(clippy::too_many_arguments)]
    async fn bootstrap_existing_session(
        &self,
        session_id: SessionId,
        cwd: PathBuf,
        client: ConnectionTo<Client>,
        session_mcp_setup: SessionMcpSetup,
        bypass_startup_restore: bool,
    ) -> Result<ExistingSessionBootstrap, Error> {
        if bypass_startup_restore {
            let thread = Thread::start_session_for_existing_session_id(
                session_id,
                &self.config,
                cwd,
                client,
                self.client_capabilities.clone(),
                session_mcp_setup.config_overrides,
                session_mcp_setup.summary,
            )
            .await?;

            return Ok(ExistingSessionBootstrap {
                thread: Arc::new(thread),
                restored_backend_history: false,
            });
        }

        match Thread::resume_session(
            session_id.clone(),
            &self.config,
            cwd.clone(),
            client.clone(),
            self.client_capabilities.clone(),
            session_mcp_setup.config_overrides.clone(),
            session_mcp_setup.summary.clone(),
        )
        .await
        {
            Ok(thread) => Ok(ExistingSessionBootstrap {
                thread: Arc::new(thread),
                restored_backend_history: true,
            }),
            Err(error) => match existing_session_restore_failure_action(&error) {
                ExistingSessionRestoreFailureAction::StartFreshBackend => {
                    warn!(
                        session_id = %session_id,
                        cwd = %cwd.display(),
                        error = %error,
                        "ACP session history is unavailable; starting fresh backend thread for existing Zed session"
                    );
                    let thread = Thread::start_session_for_existing_session_id(
                        session_id,
                        &self.config,
                        cwd,
                        client,
                        self.client_capabilities.clone(),
                        session_mcp_setup.config_overrides,
                        session_mcp_setup.summary,
                    )
                    .await?;

                    Ok(ExistingSessionBootstrap {
                        thread: Arc::new(thread),
                        restored_backend_history: false,
                    })
                }
                ExistingSessionRestoreFailureAction::ReturnError => Err(error),
            },
        }
    }

    fn should_bypass_startup_restore(&self) -> bool {
        if self.auto_restore_enabled {
            return false;
        }

        if self.startup_restore_bypassed.load(Ordering::SeqCst) {
            return false;
        }

        if self.startup_instant.elapsed() > STARTUP_RESTORE_GUARD_WINDOW {
            return false;
        }

        self.startup_restore_bypassed
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadRollbackExtParams {
    session_id: SessionId,
    num_turns: u32,
    #[serde(default)]
    replay_history: bool,
}

fn parse_thread_rollback_ext_params(
    args: &ExtRequest,
) -> Result<Option<ThreadRollbackExtParams>, Error> {
    if args.method.as_ref() != EXT_THREAD_ROLLBACK_METHOD {
        return Ok(None);
    }

    serde_json::from_str::<ThreadRollbackExtParams>(args.params.get())
        .map(Some)
        .map_err(|err| Error::invalid_params().data(format!("invalid ext params: {err}")))
}

fn ext_json_response(value: serde_json::Value) -> Result<ExtResponse, Error> {
    let raw = to_raw_value(&value).map_err(|err| Error::internal_error().data(err.to_string()))?;
    Ok(ExtResponse::new(Arc::from(raw)))
}

impl CodexAgent {
    pub async fn serve(
        self: Arc<Self>,
        transport: impl ConnectTo<Agent> + 'static,
    ) -> acp::Result<()> {
        let agent = self;
        Agent
            .builder()
            .name("codex-acp-cas")
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: InitializeRequest, responder, _cx| {
                        responder.respond_with_result(agent.initialize(request).await)
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: AuthenticateRequest,
                                responder,
                                cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        cx.spawn(async move {
                            responder.respond_with_result(agent.authenticate(request).await)
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: LogoutRequest, responder, cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        cx.spawn(async move {
                            responder.respond_with_result(agent.logout(request).await)
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: NewSessionRequest, responder, cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        let session_cx = cx.clone();
                        cx.spawn(async move {
                            responder
                                .respond_with_result(agent.new_session(request, session_cx).await)
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: LoadSessionRequest, responder, cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        let session_cx = cx.clone();
                        cx.spawn(async move {
                            responder
                                .respond_with_result(agent.load_session(request, session_cx).await)
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: ResumeSessionRequest,
                                responder,
                                cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        let session_cx = cx.clone();
                        cx.spawn(async move {
                            responder.respond_with_result(
                                agent.resume_session(request, session_cx).await,
                            )
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: ForkSessionRequest, responder, cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        let session_cx = cx.clone();
                        cx.spawn(async move {
                            responder
                                .respond_with_result(agent.fork_session(request, session_cx).await)
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: ListSessionsRequest,
                                responder,
                                cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        cx.spawn(async move {
                            responder.respond_with_result(agent.list_sessions(request).await)
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: CloseSessionRequest,
                                responder,
                                cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        cx.spawn(async move {
                            responder.respond_with_result(agent.close_session(request).await)
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: DeleteSessionRequest,
                                responder,
                                cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        cx.spawn(async move {
                            responder.respond_with_result(agent.delete_session(request).await)
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: PromptRequest, responder, cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        cx.spawn(async move {
                            responder.respond_with_result(agent.prompt(request).await)
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_notification(
                {
                    let agent = agent.clone();
                    async move |notification: CancelNotification, cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        cx.spawn(async move {
                            if let Err(error) = agent.cancel(notification).await {
                                tracing::error!("Error handling cancel: {error:?}");
                            }
                            Ok(())
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_notification!(),
            )
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: SetSessionModeRequest,
                                responder,
                                cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        cx.spawn(async move {
                            responder.respond_with_result(agent.set_session_mode(request).await)
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = agent.clone();
                    async move |request: SetSessionConfigOptionRequest,
                                responder,
                                cx: ConnectionTo<Client>| {
                        let agent = agent.clone();
                        cx.spawn(async move {
                            responder
                                .respond_with_result(agent.set_session_config_option(request).await)
                        })?;
                        Ok(())
                    }
                },
                acp::on_receive_request!(),
            )
            .on_receive_dispatch(
                {
                    let agent = agent.clone();
                    async move |message: Dispatch<UntypedMessage, UntypedMessage>, _cx| {
                        let Dispatch::Request(request, responder) = message else {
                            return Ok(Handled::No {
                                message,
                                retry: false,
                            });
                        };

                        if request.method.as_str() != EXT_THREAD_ROLLBACK_METHOD {
                            return Ok(Handled::No {
                                message: Dispatch::Request(request, responder),
                                retry: false,
                            });
                        }

                        let raw = to_raw_value(&request.params)
                            .map_err(|err| Error::internal_error().data(err.to_string()))?;
                        let response = agent
                            .ext_method(ExtRequest::new(request.method, Arc::from(raw)))
                            .await?;
                        let value = serde_json::from_str(response.0.get())
                            .map_err(|err| Error::internal_error().data(err.to_string()))?;
                        responder.respond(value)?;
                        Ok(Handled::Yes)
                    }
                },
                acp::on_receive_dispatch!(),
            )
            .connect_to(transport)
            .await
    }

    async fn initialize(&self, request: InitializeRequest) -> Result<InitializeResponse, Error> {
        let InitializeRequest {
            protocol_version,
            client_capabilities,
            client_info: _,
            ..
        } = request;

        debug!("Received initialize request with protocol version {protocol_version:?}");
        // RwLock write-path: poisoning означало бы panic в этом же write, которого у нас нет.
        // Если читатель panic-нул с read guard — продолжить с текущим capabilities безопасно.
        match self.client_capabilities.write() {
            Ok(mut guard) => *guard = client_capabilities,
            Err(poison) => *poison.into_inner() = client_capabilities,
        }

        let mut capabilities = AgentCapabilities::new()
            .prompt_capabilities(PromptCapabilities::new().embedded_context(true).image(true))
            .mcp_capabilities(McpCapabilities::new().http(true))
            .load_session(true)
            .auth(AgentAuthCapabilities::new().logout(LogoutCapabilities::new()));
        capabilities.session_capabilities = SessionCapabilities::new()
            .list(SessionListCapabilities::new())
            .close(SessionCloseCapabilities::new())
            .delete(SessionDeleteCapabilities::new())
            .fork(SessionForkCapabilities::new())
            .resume(SessionResumeCapabilities::new());

        let mut auth_methods = vec![
            CodexAuthMethod::ChatGpt.into(),
            CodexAuthMethod::CodexApiKey.into(),
            CodexAuthMethod::OpenAiApiKey.into(),
        ];

        // Поток device-code/browser недоступен в некоторых удалённых окружениях.
        if std::env::var("NO_BROWSER").is_ok() {
            auth_methods.remove(0);
        }

        Ok(InitializeResponse::new(ProtocolVersion::V1)
            .agent_capabilities(capabilities)
            .agent_info(
                Implementation::new("codex-acp-cas", env!("CARGO_PKG_VERSION")).title("Codex CAS"),
            )
            .auth_methods(auth_methods))
    }

    async fn authenticate(
        &self,
        request: AuthenticateRequest,
    ) -> Result<AuthenticateResponse, Error> {
        let auth_method = CodexAuthMethod::try_from(request.method_id)?;

        if let Some(auth) = self.auth_manager.auth().await {
            match (auth, auth_method) {
                (
                    CodexAuth::ApiKey(..),
                    CodexAuthMethod::CodexApiKey | CodexAuthMethod::OpenAiApiKey,
                )
                | (CodexAuth::Chatgpt(..), CodexAuthMethod::ChatGpt) => {
                    return Ok(AuthenticateResponse::new());
                }
                _ => {}
            }
        }

        match auth_method {
            CodexAuthMethod::ChatGpt => {
                let opts = codex_login::ServerOptions::new(
                    self.config.codex_home.clone(),
                    codex_core::auth::CLIENT_ID.to_string(),
                    None,
                    self.config.cli_auth_credentials_store_mode,
                );

                let server =
                    codex_login::run_login_server(opts).map_err(Error::into_internal_error)?;
                server
                    .block_until_done()
                    .await
                    .map_err(Error::into_internal_error)?;
                self.auth_manager.reload();
            }
            CodexAuthMethod::CodexApiKey => {
                let api_key = read_codex_api_key_from_env().ok_or_else(|| {
                    Error::internal_error().data(format!("{CODEX_API_KEY_ENV_VAR} is not set"))
                })?;
                codex_login::login_with_api_key(
                    &self.config.codex_home,
                    &api_key,
                    self.config.cli_auth_credentials_store_mode,
                )
                .map_err(Error::into_internal_error)?;
            }
            CodexAuthMethod::OpenAiApiKey => {
                let api_key = read_openai_api_key_from_env().ok_or_else(|| {
                    Error::internal_error().data(format!("{OPENAI_API_KEY_ENV_VAR} is not set"))
                })?;
                codex_login::login_with_api_key(
                    &self.config.codex_home,
                    &api_key,
                    self.config.cli_auth_credentials_store_mode,
                )
                .map_err(Error::into_internal_error)?;
            }
        }

        self.auth_manager.reload();
        Ok(AuthenticateResponse::new())
    }

    async fn logout(&self, _request: LogoutRequest) -> Result<LogoutResponse, Error> {
        self.auth_manager
            .logout()
            .map_err(Error::into_internal_error)?;
        Ok(LogoutResponse::new())
    }

    async fn new_session(
        &self,
        request: NewSessionRequest,
        client: ConnectionTo<Client>,
    ) -> Result<NewSessionResponse, Error> {
        self.check_auth().await?;
        let started_at = Instant::now();

        let NewSessionRequest {
            cwd, mcp_servers, ..
        } = request;

        let session_mcp_setup =
            build_session_mcp_setup(self.config.mcp_servers.get(), &cwd, mcp_servers)?;
        if let Some(config) = &session_mcp_setup.config_overrides {
            info!(
                mcp_server_count = config
                    .get("mcp_servers")
                    .and_then(|value| value.as_object())
                    .map(|servers| servers.len())
                    .unwrap_or(0),
                "Applied ACP MCP servers as session-scoped app-server config overrides"
            );
        }

        let (session_id, thread) = Thread::start_session(
            &self.config,
            cwd,
            client,
            self.client_capabilities.clone(),
            session_mcp_setup.config_overrides,
            session_mcp_setup.summary,
        )
        .await?;
        let thread = Arc::new(thread);
        let load = self
            .load_and_register_session(session_id.clone(), thread, false, started_at)
            .await?;

        info!(
            session_id = %session_id,
            elapsed_ms = started_at.elapsed().as_millis() as u64,
            "Finished ACP new_session critical startup path"
        );

        Ok(NewSessionResponse::new(session_id)
            .modes(load.modes)
            .config_options(load.config_options))
    }

    async fn load_session(
        &self,
        request: LoadSessionRequest,
        client: ConnectionTo<Client>,
    ) -> Result<LoadSessionResponse, Error> {
        self.check_auth().await?;
        let started_at = Instant::now();

        let LoadSessionRequest {
            session_id,
            cwd,
            mcp_servers,
            ..
        } = request;

        let session_mcp_setup =
            build_session_mcp_setup(self.config.mcp_servers.get(), &cwd, mcp_servers)?;

        let bypass_startup_restore = self.should_bypass_startup_restore();

        let ExistingSessionBootstrap {
            thread,
            restored_backend_history,
        } = self
            .bootstrap_existing_session(
                session_id.clone(),
                cwd,
                client.clone(),
                session_mcp_setup,
                bypass_startup_restore,
            )
            .await?;

        // При обычном load-сценарии реплеим историю; если сработал startup guard,
        // или restore невозможен, открываем fresh backend-thread под тем же ACP session handle.
        let load = self
            .load_and_register_session(session_id, thread, restored_backend_history, started_at)
            .await?;

        info!(
            bypass_startup_restore,
            restored_backend_history,
            elapsed_ms = started_at.elapsed().as_millis() as u64,
            "Finished ACP load_session critical startup path"
        );

        Ok(load)
    }

    async fn resume_session(
        &self,
        request: ResumeSessionRequest,
        client: ConnectionTo<Client>,
    ) -> Result<ResumeSessionResponse, Error> {
        self.check_auth().await?;
        let started_at = Instant::now();

        let ResumeSessionRequest {
            session_id,
            cwd,
            mcp_servers,
            ..
        } = request;

        let session_mcp_setup =
            build_session_mcp_setup(self.config.mcp_servers.get(), &cwd, mcp_servers)?;

        let bypass_startup_restore = self.should_bypass_startup_restore();

        let ExistingSessionBootstrap {
            thread,
            restored_backend_history,
        } = self
            .bootstrap_existing_session(
                session_id.clone(),
                cwd,
                client.clone(),
                session_mcp_setup,
                bypass_startup_restore,
            )
            .await?;

        // Для session/resume не реплеим историю, но обновляем динамические команды после старта.
        let load = self
            .load_and_register_session(session_id, thread, false, started_at)
            .await?;

        info!(
            bypass_startup_restore,
            restored_backend_history,
            elapsed_ms = started_at.elapsed().as_millis() as u64,
            "Finished ACP resume_session critical startup path"
        );

        Ok(ResumeSessionResponse::new()
            .modes(load.modes)
            .config_options(load.config_options))
    }

    async fn list_sessions(
        &self,
        request: ListSessionsRequest,
    ) -> Result<ListSessionsResponse, Error> {
        self.check_auth().await?;

        Thread::list_sessions(&self.config, request.cwd, request.cursor).await
    }

    async fn close_session(
        &self,
        request: CloseSessionRequest,
    ) -> Result<CloseSessionResponse, Error> {
        self.sessions
            .lock()
            .map_err(|_| Error::internal_error().data("session registry lock poisoned"))?
            .remove(&request.session_id);
        Ok(CloseSessionResponse::default())
    }

    async fn delete_session(
        &self,
        request: DeleteSessionRequest,
    ) -> Result<DeleteSessionResponse, Error> {
        self.check_auth().await?;

        let loaded_thread = self
            .sessions
            .lock()
            .map_err(|_| Error::internal_error().data("session registry lock poisoned"))?
            .get(&request.session_id)
            .cloned();

        if let Some(thread) = loaded_thread {
            thread.delete_backend_thread().await?;
            self.sessions
                .lock()
                .map_err(|_| Error::internal_error().data("session registry lock poisoned"))?
                .remove(&request.session_id);
        } else {
            Thread::delete_session(request.session_id).await?;
        }

        Ok(DeleteSessionResponse::new())
    }

    async fn fork_session(
        &self,
        request: ForkSessionRequest,
        client: ConnectionTo<Client>,
    ) -> Result<ForkSessionResponse, Error> {
        self.check_auth().await?;
        let started_at = Instant::now();

        let ForkSessionRequest {
            session_id,
            cwd,
            mcp_servers,
            ..
        } = request;

        let session_mcp_setup =
            build_session_mcp_setup(self.config.mcp_servers.get(), &cwd, mcp_servers)?;
        let source_thread = self.get_thread(&session_id)?;
        let (forked_session_id, thread) = source_thread
            .fork_session(
                &self.config,
                cwd,
                client,
                self.client_capabilities.clone(),
                session_mcp_setup.config_overrides,
                session_mcp_setup.summary,
            )
            .await?;

        let thread = Arc::new(thread);
        let load = self
            .load_and_register_session(forked_session_id.clone(), thread, false, started_at)
            .await?;

        info!(
            session_id = %forked_session_id,
            elapsed_ms = started_at.elapsed().as_millis() as u64,
            "Finished ACP fork_session critical startup path"
        );

        Ok(ForkSessionResponse::new(forked_session_id)
            .modes(load.modes)
            .config_options(load.config_options))
    }

    async fn prompt(&self, request: PromptRequest) -> Result<PromptResponse, Error> {
        self.check_auth().await?;

        // Даем ACP-клиенту шанс отрисовать running turn/spinner до того, как
        // начнем pre-prompt drain и остальную подготовку внутри Thread::prompt.
        tokio::task::yield_now().await;

        let thread = self.get_thread(&request.session_id)?;
        let stop_reason = thread.prompt(request).await?;
        Ok(PromptResponse::new(stop_reason))
    }

    async fn cancel(&self, args: CancelNotification) -> Result<(), Error> {
        self.get_thread(&args.session_id)?.cancel().await
    }

    async fn set_session_mode(
        &self,
        args: SetSessionModeRequest,
    ) -> Result<SetSessionModeResponse, Error> {
        let thread = self.get_thread(&args.session_id)?;
        thread.set_mode(args.mode_id).await?;
        thread.notify_current_mode_update().await;
        thread.notify_config_options_update().await;
        Ok(SetSessionModeResponse::default())
    }

    async fn set_session_config_option(
        &self,
        args: SetSessionConfigOptionRequest,
    ) -> Result<SetSessionConfigOptionResponse, Error> {
        let thread = self.get_thread(&args.session_id)?;
        let is_mode_option = matches!(args.config_id.0.as_ref(), "mode" | "permissions");
        let value = args.value.as_value_id().cloned().ok_or_else(|| {
            Error::invalid_params().data("boolean config values are not supported")
        })?;
        thread.set_config_option(args.config_id, value).await?;
        if is_mode_option {
            thread.notify_current_mode_update().await;
        }
        let config_options = thread.config_options().await?;
        Ok(SetSessionConfigOptionResponse::new(config_options))
    }

    async fn ext_method(&self, args: ExtRequest) -> Result<ExtResponse, Error> {
        let Some(params) = parse_thread_rollback_ext_params(&args)? else {
            return Err(Error::method_not_found());
        };

        let thread = self.get_thread(&params.session_id)?;
        let remaining_turns = thread
            .rollback_turns_ext(params.num_turns, params.replay_history)
            .await?;

        ext_json_response(serde_json::json!({
            "ok": true,
            "remainingTurns": remaining_turns
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexAuthMethod {
    ChatGpt,
    CodexApiKey,
    OpenAiApiKey,
}

impl From<CodexAuthMethod> for AuthMethodId {
    fn from(method: CodexAuthMethod) -> Self {
        Self::new(match method {
            CodexAuthMethod::ChatGpt => "chatgpt",
            CodexAuthMethod::CodexApiKey => "codex-api-key",
            CodexAuthMethod::OpenAiApiKey => "openai-api-key",
        })
    }
}

impl From<CodexAuthMethod> for AuthMethod {
    fn from(method: CodexAuthMethod) -> Self {
        match method {
            CodexAuthMethod::ChatGpt => AuthMethod::Agent(
                AuthMethodAgent::new(method, "Login with ChatGPT").description(
                    "Use your ChatGPT login with Codex CLI (requires a paid ChatGPT subscription)",
                ),
            ),
            CodexAuthMethod::CodexApiKey => AuthMethod::Agent(
                AuthMethodAgent::new(method, format!("Use {CODEX_API_KEY_ENV_VAR}")).description(
                    format!("Requires setting the `{CODEX_API_KEY_ENV_VAR}` environment variable."),
                ),
            ),
            CodexAuthMethod::OpenAiApiKey => AuthMethod::Agent(
                AuthMethodAgent::new(method, format!("Use {OPENAI_API_KEY_ENV_VAR}")).description(
                    format!(
                        "Requires setting the `{OPENAI_API_KEY_ENV_VAR}` environment variable."
                    ),
                ),
            ),
        }
    }
}

impl TryFrom<AuthMethodId> for CodexAuthMethod {
    type Error = Error;

    fn try_from(value: AuthMethodId) -> Result<Self, Self::Error> {
        match value.0.as_ref() {
            "chatgpt" => Ok(CodexAuthMethod::ChatGpt),
            "codex-api-key" => Ok(CodexAuthMethod::CodexApiKey),
            "openai-api-key" => Ok(CodexAuthMethod::OpenAiApiKey),
            _ => Err(Error::invalid_params().data("unsupported authentication method")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_core::config::Config;
    use std::path::PathBuf;

    fn build_ext_request(method: &str, value: serde_json::Value) -> ExtRequest {
        let raw = to_raw_value(&value).expect("raw value");
        ExtRequest::new(method, Arc::from(raw))
    }

    fn build_test_config() -> Config {
        let mut config = Config::load_default_with_cli_overrides(vec![])
            .expect("default config should load for tests");
        config.cwd = PathBuf::from("/tmp/codex-acp-cas-tests");
        config
    }

    #[test]
    fn parses_thread_rollback_ext_params() {
        let request = build_ext_request(
            "zed.dev/codex/thread/rollback",
            serde_json::json!({
                "sessionId": "thread_123",
                "numTurns": 2,
                "replayHistory": true
            }),
        );

        let params = parse_thread_rollback_ext_params(&request)
            .expect("should parse")
            .expect("known method");
        assert_eq!(params.session_id, SessionId::new("thread_123"));
        assert_eq!(params.num_turns, 2);
        assert!(params.replay_history);
    }

    #[test]
    fn ignores_unknown_ext_method() {
        let request = build_ext_request(
            "example.com/ping",
            serde_json::json!({
                "sessionId": "thread_x",
                "numTurns": 1
            }),
        );

        let parsed = parse_thread_rollback_ext_params(&request).expect("parse result");
        assert!(parsed.is_none());
    }

    #[test]
    fn errors_on_invalid_ext_params() {
        let request = build_ext_request(
            "zed.dev/codex/thread/rollback",
            serde_json::json!({
                "sessionId": "thread_x"
            }),
        );

        let error = parse_thread_rollback_ext_params(&request).expect_err("should fail");
        assert_eq!(error.code, agent_client_protocol::ErrorCode::InvalidParams);
    }

    #[test]
    fn missing_rollout_restore_error_starts_fresh_backend() {
        let error = Error::internal_error()
            .data("thread/resume failed: no rollout found for thread id f5347cce");

        assert_eq!(
            existing_session_restore_failure_action(&error),
            ExistingSessionRestoreFailureAction::StartFreshBackend
        );
    }

    #[test]
    fn non_history_restore_error_is_not_hidden_by_fallback() {
        let error = Error::internal_error().data("thread/resume failed: auth required");

        assert_eq!(
            existing_session_restore_failure_action(&error),
            ExistingSessionRestoreFailureAction::ReturnError
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn initialize_advertises_session_lifecycle_capabilities() {
        let agent = CodexAgent::new(build_test_config());
        let response = agent
            .initialize(InitializeRequest::new(ProtocolVersion::V1))
            .await
            .expect("initialize should succeed");

        assert!(
            response
                .agent_capabilities
                .session_capabilities
                .fork
                .is_some(),
            "session/fork capability should be advertised",
        );
        assert!(
            response
                .agent_capabilities
                .session_capabilities
                .close
                .is_some(),
            "session/close capability should be advertised",
        );
        assert!(
            response
                .agent_capabilities
                .session_capabilities
                .delete
                .is_some(),
            "session/delete capability should be advertised",
        );
        assert!(
            response.agent_capabilities.auth.logout.is_some(),
            "logout capability should be advertised",
        );
    }
}
