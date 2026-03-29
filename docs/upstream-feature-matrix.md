# Матрица Фич И Сравнение С Upstream

Актуально на `2026-03-29` после прогона `bash script/update_references.sh` и синхронизации локального кода по `RequestPermissions`.

## Снимок References

| Reference | Состояние | Дата / commit | Примечание |
| --- | --- | --- | --- |
| `agent-client-protocol` | обновлен | `2026-03-28`, `5124dce` | Локальная ссылка теперь указывает на `v0.11.4-2-g5124dce`. |
| `codex-acp-upstream` | обновлен | `2026-03-23`, `09fb7b1` | Локальная ссылка теперь указывает на `v0.10.0-2-g09fb7b1`. Это основной источник для сравнения с официальным `zed codex acp`. |
| `codex` | обновлен | `2026-03-28`, `4e119a3b3` | Локальная ссылка теперь указывает на `rusty-v8-v146.4.0-261-g4e119a3b3`. |
| `zed` | не обновлен | `2026-02-25`, `046b173b87` | `update_references.sh` пропустил репозиторий, потому что там локально изменен `crates/agent_servers/src/acp.rs`. |

Сравнение ниже опирается прежде всего на `references/codex-acp-upstream@v0.10.0-2-g09fb7b1` и `references/codex@rusty-v8-v146.4.0-261-g4e119a3b3`. `zed`-референс здесь вторичен.

## Легенда

- `[x]` реализовано полноценно.
- `[~]` реализовано частично или есть только каркас/частичный plumbing.
- `[ ]` отсутствует.
- `<= 2026-02-18` означает: фича уже была в `codex-acp-upstream@v0.9.4`, точную первую точку в этой заметке отдельно не трассировал.

## Короткий Вывод

- По выбранному набору parity-фич с официальным `zed codex acp` у форка сейчас `7/15` полных совпадений, `1/15` частичное совпадение и `7/15` явных пробелов.
- Основные пробелы относительно официального адаптера: `close_session`, review/init/logout-команды, полноценное client-side выполнение `DynamicToolCall` и forwarding warning-сообщений.
- Основные сильные стороны форка: отдельный `resume_session`, workspace-scoped `/resume`, `/threads`, `/plan`, `/context`, app-server-ориентированный flow восстановления тредов и отдельный режим ручного restore через `ACP_DISABLE_AUTO_RESTORE=1` + `/resume`.

## 1. Parity С Официальным `zed codex acp`

| Фича | Источник | Дата | Оригинальный `zed codex acp` | Наш форк | Где у нас / комментарий |
| --- | --- | --- | --- | --- | --- |
| `load_session` с replay истории | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У upstream replay идет через `src/codex_agent.rs`; у нас загрузка и replay разведены на `src/codex_agent.rs` и `src/thread/session/lifecycle.rs`. |
| `list_sessions` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У upstream список идет из rollout storage, у нас из `thread/list` app-server в `src/thread/session/lifecycle.rs`. |
| `close_session` | `codex-acp-upstream` | `2026-03-13`, `be20828` | `[x]` | `[ ]` | В нашем `src/codex_agent.rs` capability `close` и handler `close_session` не реализованы. |
| Usage update / контекстное окно | `codex-acp-upstream`, `codex` | `2026-02-27`, `34dc10c`; протокол виден в `codex` на `2026-03-03`, `8da7e4bda` | `[x]` | `[x]` | У нас есть `ThreadTokenUsageUpdated` в `src/thread/features/notification/mod.rs` и `send_usage_update` в `src/thread/session/client.rs`. |
| Session config: `mode`, `model`, `reasoning_effort` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У нас это разнесено по `src/thread/session/config/*` и `src/thread/session/settings.rs`. |
| `/compact` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У нас команда реализована в `src/thread/features/session/controls.rs`. |
| `/undo` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У нас `undo` тоже вынесен в `src/thread/features/session/controls.rs`. |
| `/review`, `/review-branch`, `/review-commit` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[ ]` | У форка нет user-facing review-команд; есть только replay review-состояния в `src/thread/features/session/*`, если такие item уже пришли из истории. |
| `/init` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[ ]` | Отдельной `/init`-ветки в `src/thread/prompt/commands.rs` нет. |
| `/logout` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[ ]` | У нас есть только `authenticate`, но нет slash/logout handler. |
| ACP approvals для command / file change / tool user input | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У форка это идет через `src/thread/core/server_requests.rs` и `src/thread/features/approvals/*`. |
| `RequestPermissions` tool | `codex`, sync в `codex-acp-upstream` | `2026-03-08`, `e6b93841c`; в official adapter попало через `2026-03-13`, `be20828` | `[x]` | `[x]` | У нас есть отдельная typed-ветка `ServerRequest::PermissionsRequestApproval` и ACP popup в `src/thread/features/approvals/permissions.rs`. |
| `DynamicToolCall` (`item/tool/call`) | `codex`, sync в `codex-acp-upstream` | `2026-02-25`, `a0fd94bde`; в official adapter попало через `2026-03-13`, `be20828` | `[x]` | `[~]` | У форка теперь есть typed request/response plumbing, ACP popup и live/replay render для `ThreadItem::DynamicToolCall` (`src/thread/features/dynamic_tool_call.rs`, `src/thread/features/tool_events/dynamic.rs`). До полной parity не хватает реального client-side execution и structured elicitation: current ACP path возвращает только typed text fallback, а `thread/start.dynamicTools` мы пока не рекламируем. |
| Forwarding warning-сообщений в клиент | `codex-acp-upstream` | `2026-03-05`, `a278432` | `[x]` | `[ ]` | В `src/thread/features/notification/mod.rs` warning-ветка отдельно не поднимается, неизвестные server notifications просто игнорируются. |
| ACP MCP passthrough + sanitize имен серверов | `codex-acp-upstream` | `2026-03-05`, `678a99e` | `[x]` | `[~]` | В форке `mcp_servers` из ACP теперь маппятся в session-scoped `thread/start` / `thread/resume` `config` overrides и переживают replacement-thread внутри одной ACP-сессии. Поддержаны `stdio` и `http`; ACP `sse` пока явно игнорируется. |

## 2. Расширения Форка Поверх Официального Адаптера

| Фича | Источник | Дата | Оригинальный `zed codex acp` | Наш форк | Где у нас / комментарий |
| --- | --- | --- | --- | --- | --- |
| Отдельный `resume_session` capability | `ACP` (`session/resume`, unstable) + `codex app-server` (`thread/resume`) + форк | ACP draft уже есть в `agent-client-protocol v0.11.4`; у форка базовая ветка есть с `2026-02-22`, `119b438f` | `[ ]` | `[x]` | У нас `SessionCapabilities::resume(...)` и отдельный handler в `src/codex_agent.rs`. |
| `/threads` | форк | `2026-02-25`, `e1ace61b` | `[ ]` | `[x]` | Реализовано в `src/thread/features/resume/listing.rs`. |
| `/resume` с picker-ом по текущему workspace | форк + `thread/list` / `thread/resume` | `2026-02-25`, `e1ace61b`; UX/transport стабилизация `2026-03-29`, локально | `[ ]` | `[x]` | Реализовано через `src/thread/features/resume/selector.rs` и `apply.rs`; picker теперь paginated, с полным raw input, уникальным `ToolCallId` и cleanup transport-хвоста при переключении. |
| `/resume --no-history` | форк | `2026-02-25`, `b5cc35c3` | `[ ]` | `[x]` | Позволяет переключить context без replay старой ленты ACP. |
| `/archive [partial_id]`, `/unarchive [partial_id]` | `codex` (`thread/archive`, `thread/unarchive`) + форк | нативные RPC есть в `codex`; ACP-ветка форка добавлена локально `2026-03-29` | `[ ]` | `[x]` | `/archive` скрывает тред из обычных списков без hard delete, `/unarchive` возвращает archived тред обратно. Если архивируется текущий активный тред, форк сразу поднимает fresh backend-thread под той же ACP-сессией. Для неоднозначных query archive/unarchive используют picker с полным `raw_input`, как `/resume`. |
| `/rename <name>` | `codex` (`set_thread_name`) + форк | нативный op есть в `codex`; ACP-ветка форка добавлена локально `2026-03-29` | `[ ]` | `[x]` | Использует `thread/name/set`, сразу обновляет `SessionInfoUpdate` в ACP и поднимает `thread.name` в `/threads` и `/resume`. |
| `ACP_DISABLE_AUTO_RESTORE=1` для ручного restore-flow | форк | `2026-03-29`, локально | `[ ]` | `[x]` | Capability `load_session/resume_session` остаются видимыми для Zed, но внутри `src/codex_agent.rs` automatic backend-restore заменяется на fresh backend-thread; старый диалог подтягивается вручную через `/resume`. |
| `/plan` mode и one-shot planning | форк | базовая ветка `2026-02-25`, `30e0d57a`; поведение стабилизировано `2026-02-26`, `f537f1d5` | `[ ]` | `[x]` | Логика в `src/thread/features/plan/*`, prompt-flow в `src/thread/prompt/flow.rs`. |
| `/context` | форк | `2026-02-25`, `e1ace61b` | `[ ]` | `[x]` | Session-level команда для текущего usage/context состояния. |
| Collab tool-call UI | форк | `2026-02-24`, `45c084ee` | `[ ]` | `[x]` | Отдельный доменный срез в `src/thread/features/collab/*`. |

## 3. Свежие Возможности `codex app-server`, Которые Пока Не Подняты В Форке

Эти пункты уже видны в обновленном `references/codex`, но пока не отражены в ACP-обвязке форка.

| Возможность | Источник | Дата | Статус форка | Комментарий |
| --- | --- | --- | --- | --- |
| Remote app-server auth through client | `codex` | `2026-03-25`, `1ff39b6fa` | `[ ]` | Полезно для удаленных/проксируемых сценариев, сейчас форк это не пробрасывает. |
| `fs/watch` API | `codex` | `2026-03-24`, `301b17c2a` | `[ ]` | В ACP-слой форка никакого watch-моста пока нет. |
| Override feature flags method | `codex` | `2026-03-24`, `0b08d8930` | `[ ]` | Пока форк не умеет управлять фичефлагами app-server извне. |
| `initialize` возвращает `codex_home` | `codex` | `2026-03-24`, `24c4ecaaa` | `[ ]` | В нашем мосте это сейчас не surfaced наружу. |
| ChatGPT device-code login в app-server | `codex` | `2026-03-27`, `47a9e2e08` | `[ ]` | У форка авторизация пока завязана на существующий login flow, без нового server-side device-code пути. |

## 4. Что Имеет Смысл Брать Следующим

Приоритетно выглядит такой порядок:

1. `close_session`, потому что это чистый parity-gap с официальным адаптером и понятный ACP-level контракт.
2. Довести `DynamicToolCall` от typed fallback до реального client-side execution/structured response flow.
3. Warning forwarding, потому что это маленький diff по коду, но заметно улучшает UX после compaction и других advisory-событий.
4. Довести MCP passthrough до полной parity-ветки, если потребуется поддержка ACP `sse` или отдельный UX для MCP auth/status.
5. Документирование reconnect stall-guard и связанной turn-логики в архитектурной карте, чтобы docs не отставали от кода.
