//! Точка входа бинаря: парсит CLI-аргументы и запускает runtime ACP-сервера.

use anyhow::Result;
use clap::Parser;
use codex_arg0::arg0_dispatch_or_else;
use codex_common::CliConfigOverrides;

// Держим бинарь минимальным и делегируем инициализацию библиотечному коду.
fn main() -> Result<()> {
    arg0_dispatch_or_else(|codex_linux_sandbox_exe| async move {
        let cli_config_overrides = CliConfigOverrides::parse();
        codex_acp::run_main(codex_linux_sandbox_exe, cli_config_overrides).await?;
        Ok(())
    })
}
