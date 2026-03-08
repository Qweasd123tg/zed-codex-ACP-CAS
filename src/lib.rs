//! Codex ACP: an Agent Client Protocol implementation for Codex.
#![deny(clippy::print_stdout, clippy::print_stderr)]

use agent_client_protocol::AgentSideConnection;
use codex_common::CliConfigOverrides;
use codex_core::config::{Config, ConfigOverrides};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::{io::Result as IoResult, rc::Rc};
use tokio::task::LocalSet;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing_subscriber::EnvFilter;

mod app_server;
mod codex_agent;
mod thread;

pub static ACP_CLIENT: OnceLock<Arc<AgentSideConnection>> = OnceLock::new();

/// Runs the Codex ACP agent.
///
/// Configures an ACP agent that communicates over stdio and bridges
/// ACP to the existing codex-rs infrastructure.
///
/// # Errors
///
/// Returns an error if config parsing or startup fails.
// Build runtime configuration once and pass it into ACP agent initialization.
pub async fn run_main(
    codex_linux_sandbox_exe: Option<PathBuf>,
    cli_config_overrides: CliConfigOverrides,
) -> IoResult<()> {
    // Attach a simple subscriber so `tracing` output is visible.
    // The user controls log verbosity through `RUST_LOG`.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // Parse CLI overrides and load the effective config.
    let cli_kv_overrides = cli_config_overrides.parse_overrides().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("error parsing -c overrides: {e}"),
        )
    })?;

    let config_overrides = ConfigOverrides {
        codex_linux_sandbox_exe,
        ..ConfigOverrides::default()
    };

    let config =
        Config::load_with_cli_overrides_and_harness_overrides(cli_kv_overrides, config_overrides)
            .await
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("error loading config: {e}"),
                )
            })?;

    // Create the Agent implementation with its notification channel.
    let agent = Rc::new(codex_agent::CodexAgent::new(config));

    let stdin = tokio::io::stdin().compat();
    let stdout = tokio::io::stdout().compat_write();

    // Start the I/O task that drives the actual ACP exchange.
    LocalSet::new()
        .run_until(async move {
            // Create the ACP connection.
            let (client, io_task) = AgentSideConnection::new(agent.clone(), stdout, stdin, |fut| {
                tokio::task::spawn_local(fut);
            });

            if ACP_CLIENT.set(Arc::new(client)).is_err() {
                return Err(std::io::Error::other("ACP client already set"));
            }

            io_task
                .await
                .map_err(|e| std::io::Error::other(format!("ACP I/O error: {e}")))
        })
        .await?;

    Ok(())
}

// Re-export MCP server types for compatibility.
pub use codex_mcp_server::{
    CodexToolCallParam, CodexToolCallReplyParam, ExecApprovalElicitRequestParams,
    ExecApprovalResponse, PatchApprovalElicitRequestParams, PatchApprovalResponse,
};
