# Codex ACP CAS (Rust)

ACP-адаптер, который подключает Codex к ACP-совместимым клиентам (например, Zed) через мост к `codex app-server`.

## Что это
Проект реализует ACP `Agent` и маппит lifecycle сессий/turn/tool-calls между ACP и Codex app-server.

Бинарь проекта: `codex-acp`.

## Статус
- Версия ветки: `0.1.x` (бета-стадия, возможны изменения поведения между релизами).
- Поддерживаемый релизный target: `x86_64-unknown-linux-gnu`.
- Основной сценарий эксплуатации: Fedora x86_64.

## Поддерживаемые возможности
Из фактической реализации:
- ACP prompt capabilities: `embedded_context`, `image`.
- Сессии: `new_session`, `load_session`, `resume_session`, `list_sessions`.
- Replay истории после `load_session`/`/resume`.
- Slash-команды:
  - `/threads`
  - `/resume [partial_id] [--no-history]`
  - `/compact`
  - `/undo [N]`
  - `/reasoning [none|minimal|low|medium|high|xhigh]`
  - `/plan [on|off|<request>]`
  - `/context`
- Tool-call карточки и статусы для command/mcp/web/image/file/collab веток.

## Ограничения
- MCP passthrough из ACP-клиента пока не маппится в app-server режим.
  В `new_session/load_session` MCP-конфигурация принимается, но фактически игнорируется.

## Авторизация
Поддерживаемые методы (через `authenticate`):
- ChatGPT login flow.
- `CODEX_API_KEY`.
- `OPENAI_API_KEY`.

## Быстрый старт
Локальный запуск:

```bash
cargo run -- --help
```

После сборки:

```bash
./target/release/codex-acp --help
```

## Локальный workflow
Полезные скрипты:

```bash
bash script/run_live_checks.sh quick
bash script/run_live_checks.sh full
bash script/build_install_cas.sh
bash script/smoke_test_cas.sh "$HOME/.local/bin/codex-acp-cas"
```

Обновление references:

```bash
bash script/update_references.sh
bash script/update_references.sh --daily
bash script/update_references.sh --repo zed
```

## Сборка и проверки
Базовый набор:

```bash
cargo build
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

Проверка под релизный target:

```bash
cargo test --release --target x86_64-unknown-linux-gnu
```

## Релизы
Подготовка релиза:

```bash
bash script/prepare_release.sh 0.1.0
git push origin main
git push origin v0.1.0
```

GitHub Actions release pipeline собирает Linux-артефакт для `x86_64-unknown-linux-gnu` и публикует `tar.gz` + `.sha256`.

## Архитектурная документация
- Карта связности thread-подсистемы: `docs/thread-feature-map.md`.
- Экспортируемая карта для визуализаторов:
  - `docs/thread-feature-map.graph.json`
  - `docs/thread-feature-map.graph.mmd` (Mermaid)
  - `docs/thread-feature-map.markmap.md` (Mind map)
- Генерация экспортов: `script/export_thread_feature_map.py`.
- Правила разработки и проверки: `AGENTS.md`.

## Лицензия
Apache-2.0 (`LICENSE`).
