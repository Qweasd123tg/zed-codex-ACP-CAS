# Руководство по репозиторию

## Структура проекта
`src/` содержит Rust ACP-адаптер:
- `src/main.rs`: entrypoint бинаря.
- `src/lib.rs`: инициализация runtime, запуск ACP-соединения.
- `src/codex_agent.rs`: реализация ACP `Agent` (initialize/auth/session lifecycle).
- `src/app_server.rs`: мост JSON-RPC к `codex app-server`.
- `src/prompt_args.rs`: разбор prompt-входа и вспомогательные parser-тесты.

`src/thread.rs` хранит только оркестрацию и общее состояние `ThreadInner`; вся рабочая логика разнесена в подпакеты:
- `src/thread/core/*`: роутинг и glue (`item_handlers`, `replay`, `server_requests`, `inner_state`, `terminal_updates`).
- `src/thread/features/*`: доменные срезы (`approvals`, `collab`, `file`, `notification`, `plan`, `resume`, `session`, `tool_events`, `tool_call_ui`).
- `src/thread/prompt/*`: парсинг slash-команд и основной prompt-flow.
- `src/thread/notification/*`: транспортный dispatch входящих JSON-RPC сообщений.
- `src/thread/session/*`: загрузка/конфигурация/настройки view сессии.
- `src/thread/turn/*`: выполнение turn и обработка turn diff/state.

Дополнительно:
- `.github/workflows/`: CI (`ci.yml`) и релизный пайплайн (`release.yml`).
- `script/`: локальные утилиты сборки, smoke-тестов и release-подготовки.
- `docs/thread-feature-map.md`: карта связности thread-подсистемы.

Политика релизной поддержки в форке: Fedora-oriented, целевой target `x86_64-unknown-linux-gnu`.

## Команды сборки и проверки
Запускать из корня репозитория:

```bash
cargo build
cargo run -- --help
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

Проверка, совпадающая с поддерживаемым релизным target:

```bash
cargo test --release --target x86_64-unknown-linux-gnu
```

## Политика релизов
- Для обновлений мелких фич сразу собирать релиз в `target-test`:

```bash
cargo build --release --target-dir target-test
```

- Полный релиз (`target/release` и релизные процедуры проекта) делать только на финальном штрихе всего проекта.

## Стиль кода и соглашения
- Rust edition: `2024`.
- Форматирование: `rustfmt`.
- Линтинг: `clippy -D warnings` в CI.
- Имена:
  - `snake_case` для модулей/функций/тестов.
  - `PascalCase` для типов/трейтов.
  - `UPPER_SNAKE_CASE` для констант.
- Отступ: 4 пробела.
- Функции делать узкими по ответственности.

## Правила изменений в `thread`-слое
1. `notification/dispatch` и `core/server_requests` оставлять тонкими роутерами.
2. Доменную логику держать в `features/*`, не возвращать ее в корневой `thread.rs`.
3. Для новых lifecycle-веток соблюдать симметрию: `started -> completed -> replay`.
4. Для turn-зависимых веток сохранять guard по `expected_turn_id`.
5. После изменений mode/config отправлять `notify_config_update` или `notify_mode_and_config_update`.

## Правила изменений в `collab/subagents`
Текущий контракт в коде завязан на `ThreadItem::CollabAgentToolCall` и enum-ы из `codex-app-server-protocol`.

Поддерживаемые collab-инструменты:
- `SpawnAgent`
- `SendInput`
- `ResumeAgent`
- `Wait`
- `CloseAgent`

Поддерживаемые статусы агентов:
- `PendingInit`
- `Running`
- `Completed`
- `Errored`
- `Shutdown`
- `NotFound`

При изменениях collab/subagents соблюдать:
1. Не выносить subagents в отдельный pipeline: это обычная ветка общего `ThreadItem`-потока.
2. При изменении `CollabAgentTool` синхронно обновлять `src/thread/features/collab/render.rs` (`collab_tool_title`), `src/thread/core/tests.rs` и docs.
3. При изменении `CollabAgentToolCallStatus` или `CollabAgentStatus` синхронно обновлять `src/thread/features/collab/status.rs`, `src/thread/features/collab/content.rs`, `src/thread/core/tests.rs` и docs.
4. Сохранять симметрию `started -> completed -> replay`; live- и replay-рендер не должны расходиться по title/status/content.
5. Не терять поля `sender_thread_id`, `receiver_thread_ids`, `prompt`, `agents_states[*].status`, `agents_states[*].message`; если upstream добавляет новые поля, сначала документировать и аккуратно прокидывать их в raw/content.
6. `collab`-карточки оставлять в `ToolKind::Think`, если нет явной причины менять UX-контракт.
7. Если `agent_nickname` / `agent_role` доступны через thread metadata, поднимать их в title/content и не заставлять пользователя читать голые `thread_id`.
8. `Raw Input` и `Raw Output` для `collab` держать человекочитаемыми: prompt/target в input, краткая summary статусов в output; не сливать туда шумный JSON без необходимости.

## Тестирование
Предпочтительный формат: unit-тесты рядом с реализацией (`#[cfg(test)]`).
Ключевые тестовые точки:
- `src/thread/core/tests.rs` (основной набор для thread-функциональности).
- `src/prompt_args.rs` (тесты парсинга аргументов).
- локальные `#[cfg(test)]` в отдельных модулях (`turn/state`, `core/protocol_contract` и т.д.).

При изменениях парсинга/протокола добавлять:
- happy-path сценарии,
- edge/invalid сценарии.
- Для `collab/subagents` отдельно проверять title/status/content mapping и replay/live-симметрию.

Перед PR обязательно прогонять `fmt`, `clippy`, `test`.

## Коммиты и PR
- Тема коммита: императивно и коротко.
- Текущий паттерн истории: sentence case, опционально с `(#PR)`.

В PR указывать:
- что изменено и зачем;
- ссылку на issue (если есть);
- точные команды проверки и результат;
- релизные/platform notes, если затронуты target/release скрипты.

## Безопасность и конфигурация
- Не коммитить ключи/токены.
- Для локальных запусков использовать env-переменные (`OPENAI_API_KEY`, `CODEX_API_KEY`).
- Любые учетные данные хранить вне репозитория.
