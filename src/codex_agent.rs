//! ACP `Agent` implementation that maps ACP session lifecycle onto `Thread`.
//! Each `Thread` runs on top of Codex app-server.

use agent_client_protocol::{
    Agent, AgentCapabilities, AuthMethod, AuthMethodId, AuthenticateRequest, AuthenticateResponse,
    CancelNotification, ClientCapabilities, Error, ExtRequest, ExtResponse, Implementation,
    InitializeRequest, InitializeResponse, ListSessionsRequest, ListSessionsResponse,
    LoadSessionRequest, LoadSessionResponse, McpCapabilities, NewSessionRequest,
    NewSessionResponse, PromptCapabilities, PromptRequest, PromptResponse, ProtocolVersion,
    ResumeSessionRequest, ResumeSessionResponse, SessionCapabilities, SessionId,
    SessionListCapabilities, SessionResumeCapabilities, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse, SetSessionModeRequest, SetSessionModeResponse,
    SetSessionModelRequest, SetSessionModelResponse,
};
use codex_core::{
    CodexAuth,
    auth::{AuthManager, read_codex_api_key_from_env, read_openai_api_key_from_env},
    config::Config,
};
use codex_login::{CODEX_API_KEY_ENV_VAR, OPENAI_API_KEY_ENV_VAR};
use serde::Deserialize;
use serde_json::value::to_raw_value;
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Mutex},
};
use tracing::{debug, info};

use crate::thread::Thread;

const EXT_THREAD_ROLLBACK_METHODS: [&str; 4] = [
    "zed.dev/codex/thread/rollback",
    "codex/thread/rollback",
    "zed.dev/session/rollback",
    "session/rollback",
];

pub struct CodexAgent {
    auth_manager: Arc<AuthManager>,
    client_capabilities: Arc<Mutex<ClientCapabilities>>,
    config: Config,
    // In-memory registry of active ACP sessions for the lifetime of this process.
    sessions: Rc<RefCell<HashMap<SessionId, Rc<Thread>>>>,
}

impl CodexAgent {
    // Keep a shared startup config reused by every ACP session.
    pub fn new(config: Config) -> Self {
        let auth_manager = AuthManager::shared(
            config.codex_home.clone(),
            false,
            config.cli_auth_credentials_store_mode,
        );

        Self {
            auth_manager,
            client_capabilities: Arc::default(),
            config,
            sessions: Rc::default(),
        }
    }

    fn get_thread(&self, session_id: &SessionId) -> Result<Rc<Thread>, Error> {
        self.sessions
            .borrow()
            .get(session_id)
            .cloned()
            .ok_or_else(|| Error::resource_not_found(None))
    }

    async fn check_auth(&self) -> Result<(), Error> {
        // For the OpenAI provider, one supported auth method must be available
        // before accepting any session/prompt call.
        if self.config.model_provider_id == "openai" && self.auth_manager.auth().await.is_none() {
            return Err(Error::auth_required());
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadRollbackExtParams {
    #[serde(alias = "session_id")]
    session_id: SessionId,
    #[serde(alias = "num_turns")]
    num_turns: u32,
    #[serde(default, alias = "replay_history")]
    replay_history: bool,
}

fn parse_thread_rollback_ext_params(
    args: &ExtRequest,
) -> Result<Option<ThreadRollbackExtParams>, Error> {
    if !EXT_THREAD_ROLLBACK_METHODS.contains(&args.method.as_ref()) {
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

#[async_trait::async_trait(?Send)]
impl Agent for CodexAgent {
    async fn initialize(&self, request: InitializeRequest) -> Result<InitializeResponse, Error> {
        let InitializeRequest {
            protocol_version,
            client_capabilities,
            client_info: _,
            ..
        } = request;

        debug!("Received initialize request with protocol version {protocol_version:?}");
        *self.client_capabilities.lock().unwrap() = client_capabilities;

        let mut capabilities = AgentCapabilities::new()
            .prompt_capabilities(PromptCapabilities::new().embedded_context(true).image(true))
            .mcp_capabilities(McpCapabilities::new().http(true))
            .load_session(true);

        capabilities.session_capabilities = SessionCapabilities::new()
            .list(SessionListCapabilities::new())
            .resume(SessionResumeCapabilities::new());

        let mut auth_methods = vec![
            CodexAuthMethod::ChatGpt.into(),
            CodexAuthMethod::CodexApiKey.into(),
            CodexAuthMethod::OpenAiApiKey.into(),
        ];

        // The device-code/browser flow is unavailable in some remote environments.
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

    async fn new_session(&self, request: NewSessionRequest) -> Result<NewSessionResponse, Error> {
        self.check_auth().await?;

        let NewSessionRequest {
            cwd, mcp_servers, ..
        } = request;

        if !mcp_servers.is_empty() {
            info!(
                "MCP server passthrough from ACP is not yet mapped in app-server mode; ignoring {} MCP server(s)",
                mcp_servers.len()
            );
        }

        let (session_id, thread) =
            Thread::start_session(&self.config, cwd, self.client_capabilities.clone()).await?;
        let thread = Rc::new(thread);
        let load = thread.load().await?;
        let notify_thread = thread.clone();
        tokio::task::spawn_local(async move {
            // Return the load/new response first, then publish dynamic command metadata
            // to avoid UI races during startup.
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            notify_thread.notify_available_commands().await;
        });

        self.sessions
            .borrow_mut()
            .insert(session_id.clone(), thread);

        Ok(NewSessionResponse::new(session_id)
            .modes(load.modes)
            .models(load.models)
            .config_options(load.config_options))
    }

    async fn load_session(
        &self,
        request: LoadSessionRequest,
    ) -> Result<LoadSessionResponse, Error> {
        self.check_auth().await?;

        let LoadSessionRequest {
            session_id,
            cwd,
            mcp_servers,
            ..
        } = request;

        if !mcp_servers.is_empty() {
            info!(
                "MCP server passthrough from ACP is not yet mapped in app-server mode; ignoring {} MCP server(s)",
                mcp_servers.len()
            );
        }

        let thread = Rc::new(
            Thread::resume_session(
                session_id.clone(),
                &self.config,
                cwd,
                self.client_capabilities.clone(),
            )
            .await?,
        );

        let load = thread.load().await?;
        let notify_thread = thread.clone();
        tokio::task::spawn_local(async move {
            // Reuse the same startup order as new_session plus history replay so the user
            // immediately sees prior tool calls and diffs after loading.
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            notify_thread.notify_available_commands().await;
            notify_thread.replay_loaded_history().await;
        });
        self.sessions.borrow_mut().insert(session_id, thread);

        Ok(load)
    }

    async fn resume_session(
        &self,
        request: ResumeSessionRequest,
    ) -> Result<ResumeSessionResponse, Error> {
        self.check_auth().await?;

        let ResumeSessionRequest {
            session_id,
            cwd,
            mcp_servers,
            ..
        } = request;

        if !mcp_servers.is_empty() {
            info!(
                "MCP server passthrough from ACP is not yet mapped in app-server mode; ignoring {} MCP server(s)",
                mcp_servers.len()
            );
        }

        let thread = Rc::new(
            Thread::resume_session(
                session_id.clone(),
                &self.config,
                cwd,
                self.client_capabilities.clone(),
            )
            .await?,
        );

        let load = thread.load().await?;
        let notify_thread = thread.clone();
        tokio::task::spawn_local(async move {
            // For session/resume, skip history replay but refresh dynamic commands after startup.
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            notify_thread.notify_available_commands().await;
        });
        self.sessions.borrow_mut().insert(session_id, thread);

        Ok(ResumeSessionResponse::new()
            .modes(load.modes)
            .models(load.models)
            .config_options(load.config_options))
    }

    async fn list_sessions(
        &self,
        request: ListSessionsRequest,
    ) -> Result<ListSessionsResponse, Error> {
        self.check_auth().await?;

        Thread::list_sessions(&self.config, request.cwd, request.cursor).await
    }

    async fn prompt(&self, request: PromptRequest) -> Result<PromptResponse, Error> {
        self.check_auth().await?;

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

    async fn set_session_model(
        &self,
        args: SetSessionModelRequest,
    ) -> Result<SetSessionModelResponse, Error> {
        let thread = self.get_thread(&args.session_id)?;
        thread.set_model(args.model_id).await?;
        thread.notify_config_options_update().await;
        Ok(SetSessionModelResponse::default())
    }

    async fn set_session_config_option(
        &self,
        args: SetSessionConfigOptionRequest,
    ) -> Result<SetSessionConfigOptionResponse, Error> {
        let thread = self.get_thread(&args.session_id)?;
        let is_mode_option = args.config_id.0.as_ref() == "mode";
        thread.set_config_option(args.config_id, args.value).await?;
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
            CodexAuthMethod::ChatGpt => Self::new(method, "Login with ChatGPT").description(
                "Use your ChatGPT login with Codex CLI (requires a paid ChatGPT subscription)",
            ),
            CodexAuthMethod::CodexApiKey => {
                Self::new(method, format!("Use {CODEX_API_KEY_ENV_VAR}")).description(format!(
                    "Requires setting the `{CODEX_API_KEY_ENV_VAR}` environment variable."
                ))
            }
            CodexAuthMethod::OpenAiApiKey => {
                Self::new(method, format!("Use {OPENAI_API_KEY_ENV_VAR}")).description(format!(
                    "Requires setting the `{OPENAI_API_KEY_ENV_VAR}` environment variable."
                ))
            }
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

    fn build_ext_request(method: &str, value: serde_json::Value) -> ExtRequest {
        let raw = to_raw_value(&value).expect("raw value");
        ExtRequest::new(method, Arc::from(raw))
    }

    #[test]
    fn parses_thread_rollback_ext_params_camel_case() {
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
    fn parses_thread_rollback_ext_params_snake_case_aliases() {
        let request = build_ext_request(
            "session/rollback",
            serde_json::json!({
                "session_id": "thread_321",
                "num_turns": 1
            }),
        );

        let params = parse_thread_rollback_ext_params(&request)
            .expect("should parse")
            .expect("known method");
        assert_eq!(params.session_id, SessionId::new("thread_321"));
        assert_eq!(params.num_turns, 1);
        assert!(!params.replay_history);
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
            "codex/thread/rollback",
            serde_json::json!({
                "sessionId": "thread_x"
            }),
        );

        let error = parse_thread_rollback_ext_params(&request).expect_err("should fail");
        assert_eq!(error.code, agent_client_protocol::ErrorCode::InvalidParams);
    }
}
