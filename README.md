# Codex ACP CAS (Rust)

ACP-адаптер, который подключает Codex к ACP-совместимым клиентам (например, Zed) через мост к `codex app-server`.

## Что это
Проект реализует ACP `Agent` и маппит lifecycle сессий/turn/tool-calls между ACP и Codex app-server.

Бинарь проекта: `codex-acp`.

## Статус
- Версия ветки: `0.1.x` (бета-стадия, возможны изменения поведения между релизами).
- Поддерживаемый релизный target: `x86_64-unknown-linux-gnu`.
- Основной сценарий эксплуатации: Fedora x86_64.
- `plan mode`: функционально рабочий и активно дорабатывается по поведению/стабильности.
  Отдельный «красивый» UI для plan mode не планируется; фокус на корректном ACP-потоке и практичном отображении.

## Поддерживаемые возможности
Из фактической реализации:
- ACP prompt capabilities: `embedded_context`, `image`.
- Сессии: `new_session`, `load_session`, `resume_session`, `list_sessions`.
- Replay истории после `load_session`/`/resume`.
- Отдельный `RequestPermissions` flow через ACP permission popup.
- Slash-команды:
  - `/threads`
  - `/resume [partial_id] [--no-history]`
  - `/compact`
  - `/undo [N]`
  - `/reasoning [none|minimal|low|medium|high|xhigh]`
  - `/plan [on|off|<request>]`
  - `/context`
- Tool-call карточки и статусы для command/mcp/web/image/file/collab веток.
  Для `collab/subagents` сейчас поддерживаются `spawn_agent`, `send_input`, `wait`, `resume_agent`, `close_agent`
  и агрегированные agent-state статусы `pending_init`, `running`, `completed`, `errored`, `shutdown`, `not_found`.
  В ACP UI для `collab` дополнительно поднимаются `agent_nickname/agent_role` из thread metadata,
  task prompt уходит в `Raw Input`, а `Raw Output` содержит краткую человекочитаемую summary статусов вместо сырого JSON.

## Поведение При Reconnect-Сбоях
- Если `codex app-server` уходит в reconnect-loop и не завершает turn, адаптер не держит ACP UI в вечной загрузке.
- Для таких случаев есть stall guard: после серии reconnect-warning или после длительной тишины после reconnect turn принудительно завершается с понятным error-text.

## Поведение Resume И Восстановления Сессий
- По умолчанию `/resume` переключает backend-thread и реплеит его историю в текущую ACP-сессию.
- Для "тихого" переключения контекста без replay старой истории использовать `/resume --no-history`.
- В Zed уже показанные сообщения текущей ACP-сессии сервер очистить не может: у ACP нет штатного API для reset/replace transcript. Если нужен полностью чистый UI, практический путь сейчас — открыть новый чат и уже в нем вызвать `/resume`.
- Если в env задан `ACP_DISABLE_AUTO_RESTORE=1`, адаптер все равно рекламирует `load_session` / `resume_session` как capability для совместимости с launch-flow Zed, но внутри вместо автоматического восстановления старого backend-thread поднимает свежий backend-thread под тем же ACP session handle. Старый диалог в таком режиме нужно подтягивать вручную через `/resume`.
- Для повторного `/resume` в одной и той же ACP-сессии адаптер теперь очищает transport-хвост от предыдущего треда и создает новый picker с уникальным `ToolCallId`, чтобы не упираться в повторно использованную интерактивную карточку.

## Ограничения
- MCP passthrough из ACP-клиента пока не маппится в app-server режим.
  В `new_session/load_session` MCP-конфигурация принимается, но фактически игнорируется.
- `DynamicToolCall` (`item/tool/call`) пока не реализован end-to-end и сейчас отклоняется как unsupported server request.

## Важно Для Edit/Rewind В Zed
- Серверная часть rollback в `codex-acp` реализована, но для работы кнопки карандаша (edit старого prompt) нужен клиентский фикс в сборке `zed`.
- Требуемая часть в Zed: поддержка `truncate` в ACP connection (`crates/agent_servers/src/acp.rs`) с вызовом `ext_method("zed.dev/codex/thread/rollback", { sessionId, numTurns, replayHistory })`.
- Без этого фикса в старых/ванильных сборках Zed карандаш будет `Unavailable`, потому что ACP-сессия не выдает `message.id` для rewind/edit ветки.
- Fallback для старых сборок: использовать `/undo N` и отправлять отредактированный prompt вручную.

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
- Снимок upstream-референсов и матрица parity/lag: `docs/upstream-feature-matrix.md`.
- Экспорт для визуализаторов генерируется локально через `script/export_thread_feature_map.py` и сейчас не хранится в репозитории как tracked-артефакт.
- Правила разработки и проверки: `AGENTS.md`.

## Лицензия
Apache-2.0 (`LICENSE`).
