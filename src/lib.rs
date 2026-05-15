//! Codex ACP — реализация Agent Client Protocol для Codex.
#![deny(clippy::print_stdout, clippy::print_stderr)]

use agent_client_protocol::ByteStreams;
use codex_core::config::{Config, ConfigOverrides};
use codex_utils_cli::CliConfigOverrides;
use std::io::Result as IoResult;
use std::path::PathBuf;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing_subscriber::EnvFilter;

mod app_server;
mod codex_agent;
mod thread;

/// Запускает ACP-агент Codex.
///
/// Настраивает ACP-агент, который общается через stdio и связывает
/// протокол ACP с существующей инфраструктурой codex-rs.
///
/// # Ошибки
///
/// Если не удалось распарсить конфиг или запустить программу.
// Собираем runtime-конфигурацию один раз и передаём её в инициализацию ACP-агента.
pub async fn run_main(
    codex_linux_sandbox_exe: Option<PathBuf>,
    cli_config_overrides: CliConfigOverrides,
) -> IoResult<()> {
    // Подключаем простой subscriber, чтобы вывод `tracing` был виден.
    // Пользователь может управлять уровнем логов через `RUST_LOG`.
    let _subscriber_init = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    // Парсим CLI-override-параметры и загружаем конфигурацию.
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

    // Создаём реализацию Agent с каналом уведомлений.
    let agent = std::sync::Arc::new(codex_agent::CodexAgent::new(config));

    let stdin = tokio::io::stdin().compat();
    let stdout = tokio::io::stdout().compat_write();

    agent
        .serve(ByteStreams::new(stdout, stdin))
        .await
        .map_err(|e| std::io::Error::other(format!("ACP error: {e}")))?;

    Ok(())
}
