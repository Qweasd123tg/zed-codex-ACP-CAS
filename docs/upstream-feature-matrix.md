# Матрица Фич И Сравнение С Upstream

Актуально на `2026-03-31` после прогона `bash script/update_references.sh`, синхронизации локального кода по `RequestPermissions`, UX-обновлений session config и warning forwarding.

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

- По выбранному набору parity-фич с официальным `zed codex acp` у форка сейчас `9/15` полных совпадений, `0/15` частичных совпадений и `6/15` явных пробелов.
- Основные пробелы относительно официального адаптера: `close_session`, `init`/`logout` и отдельные client-native UX-ветки, которые в текущем Zed ACP пока не дают достаточной отдачи.
- Основные сильные стороны форка: отдельный `resume_session`, workspace-scoped `/resume`, `/threads`, `/plan`, app-server-ориентированный flow восстановления тредов, нижний `Context` control и отдельный режим ручного restore через `ACP_DISABLE_AUTO_RESTORE=1` + `/resume`.

## 1. Parity С Официальным `zed codex acp`

| Фича | Источник | Дата | Оригинальный `zed codex acp` | Наш форк | Где у нас / комментарий |
| --- | --- | --- | --- | --- | --- |
| `load_session` с replay истории | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У upstream replay идет через `src/codex_agent.rs`; у нас загрузка и replay разведены на `src/codex_agent.rs` и `src/thread/session/lifecycle.rs`. |
| `list_sessions` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У upstream список идет из rollout storage, у нас из `thread/list` app-server в `src/thread/session/lifecycle.rs`. |
| `close_session` | `codex-acp-upstream` | `2026-03-13`, `be20828` | `[x]` | `[ ]` | В нашем `src/codex_agent.rs` capability `close` и handler `close_session` не реализованы. |
| Usage update / контекстное окно | `codex-acp-upstream`, `codex` | `2026-02-27`, `34dc10c`; протокол виден в `codex` на `2026-03-03`, `8da7e4bda` | `[x]` | `[x]` | У нас есть `ThreadTokenUsageUpdated` в `src/thread/features/notification/mod.rs` и `send_usage_update` в `src/thread/session/client.rs`. |
| Session config: `mode`, `permissions`, `model`, `reasoning_effort`, `context_control` | `codex-acp-upstream` + форк | `<= 2026-02-18`, `c0b82cc`; `permissions` и `context_control` локально, selector enrich `2026-03-31` | `[x]` | `[x]` | У нас это разнесено по `src/thread/session/config/*` и `src/thread/session/settings.rs`; `mode` и `permissions` теперь отдельные selectors, а `context_control` показывает `status`, `ctx %`, `MCP`, `skills`, account limits (`5h`/`wk`) и умеет запускать compaction. `status` в short label держит суммарный `used`, а detail/report — workspace, account и `used / in / out`. |
| `/compact` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У нас команда реализована в `src/thread/features/session/controls.rs`. |
| `/undo` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У нас `undo` тоже вынесен в `src/thread/features/session/controls.rs`. Сам rollback-flow работает, но visual edit/rewind button в текущем `Zed` по-прежнему зависит от client-side ACP fix; для нативной кнопки нужен патч/пересборка `Zed`. |
| `/review` | `codex-acp-upstream` + `codex` (`review/start`) | `<= 2026-02-18`, `c0b82cc`; локально `2026-03-31` | `[x]` | `[x]` | У форка теперь есть user-facing inline review-flow через один основной entrypoint `/review`. Bare команда открывает ACP picker для uncommitted/base-branch/commit/custom сценариев, а кастомные инструкции задаются через `/review <text>`. |
| `/init` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[ ]` | Отдельной `/init`-ветки в `src/thread/prompt/commands.rs` нет. |
| `/logout` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[ ]` | У нас есть только `authenticate`, но нет slash/logout handler. |
| ACP approvals для command / file change / tool user input | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У форка это идет через `src/thread/core/server_requests.rs` и `src/thread/features/approvals/*`; для command approval popup теперь surfaced reason, очищенная inner shell-команда, `cwd`, network context и additional permissions. |
| `RequestPermissions` tool | `codex`, sync в `codex-acp-upstream` | `2026-03-08`, `e6b93841c`; в official adapter попало через `2026-03-13`, `be20828` | `[x]` | `[x]` | У нас есть отдельная typed-ветка `ServerRequest::PermissionsRequestApproval` и ACP popup в `src/thread/features/approvals/permissions.rs`. |
| `DynamicToolCall` (`item/tool/call`) | `codex`, sync в `codex-acp-upstream` | `2026-02-25`, `a0fd94bde`; в official adapter попало через `2026-03-13`, `be20828` | `[x]` | `[ ]` | В форке была только частичная экспериментальная ветка, но для текущего Zed она не дала достаточной практической отдачи. В runtime-пайплайне поддержку вырезали, а возвращаться к ней имеет смысл только если появится конкретный Zed-side client-native use case. Бэкап-конспект оставлен в `docs/drafts/dynamic-tool-call-backup.md`. |
| Forwarding warning-сообщений в клиент | `codex-acp-upstream` | `2026-03-05`, `a278432` | `[x]` | `[x]` | В `src/thread/features/notification/mod.rs` теперь поднимаются `ConfigWarning`, `DeprecationNotice` и `WindowsWorldWritableWarning`; текст уходит в ACP-чат через `src/thread/features/notification/events/warnings.rs`. |
| ACP MCP passthrough + sanitize имен серверов | `codex-acp-upstream` | `2026-03-05`, `678a99e` | `[x]` | `[~]` | В форке `mcp_servers` из ACP теперь маппятся в session-scoped `thread/start` / `thread/resume` `config` overrides и переживают replacement-thread внутри одной ACP-сессии. Поддержаны `stdio` и `http`; ACP `sse` пока явно игнорируется. |

## 2. Расширения Форка Поверх Официального Адаптера

| Фича | Источник | Дата | Оригинальный `zed codex acp` | Наш форк | Где у нас / комментарий |
| --- | --- | --- | --- | --- | --- |
| Отдельный `resume_session` capability | `ACP` (`session/resume`, unstable) + `codex app-server` (`thread/resume`) + форк | ACP draft уже есть в `agent-client-protocol v0.11.4`; у форка базовая ветка есть с `2026-02-22`, `119b438f` | `[ ]` | `[x]` | У нас `SessionCapabilities::resume(...)` и отдельный handler в `src/codex_agent.rs`. |
| `/threads` | форк | `2026-02-25`, `e1ace61b` | `[ ]` | `[x]` | Реализовано в `src/thread/features/resume/listing.rs`. |
| `/resume` с picker-ом по текущему workspace | форк + `thread/list` / `thread/resume` | `2026-02-25`, `e1ace61b`; UX/transport стабилизация `2026-03-29`, локально | `[ ]` | `[x]` | Реализовано через `src/thread/features/resume/selector.rs` и `apply.rs`; picker теперь paginated, с полным raw input, уникальным `ToolCallId` и cleanup transport-хвоста при переключении. |
| `/resume --no-history` | форк | `2026-02-25`, `b5cc35c3` | `[ ]` | `[x]` | Позволяет переключить context без replay старой ленты ACP. |
| `/new` (`soft-new`) | форк + `thread/start` | локально, `2026-03-31` | `[ ]` | `[x]` | Стартует fresh backend-thread внутри той же ACP-сессии и сбрасывает runtime session state. Ограничение текущего `Zed`: sidebar history не очищается, потому что это не client-side `new_session`, а in-place thread switch. |
| `/fork` | `codex` (`thread/fork`) + форк | локально, `2026-03-31` | `[ ]` | `[x]` | Форкает текущий materialized backend-thread через `thread/fork` и переводит текущую ACP-сессию на forked thread. Sidebar history тоже остается видимой, потому что `Zed` не делает visual reset для in-place thread switch. |
| `/archive [partial_id]`, `/unarchive [partial_id]` | `codex` (`thread/archive`, `thread/unarchive`) + форк | нативные RPC есть в `codex`; ACP-ветка форка добавлена локально `2026-03-29` | `[ ]` | `[x]` | `/archive` скрывает тред из обычных списков без hard delete, `/unarchive` возвращает archived тред обратно. Если архивируется текущий активный тред, форк сразу поднимает fresh backend-thread под той же ACP-сессией. Для неоднозначных query archive/unarchive используют picker с полным `raw_input`, как `/resume`. |
| `/rename <name>` | `codex` (`set_thread_name`) + форк | нативный op есть в `codex`; ACP-ветка форка добавлена локально `2026-03-29` | `[ ]` | `[x]` | Использует `thread/name/set`, сразу обновляет `SessionInfoUpdate` в ACP и поднимает `thread.name` в `/threads` и `/resume`. |
| `ACP_DISABLE_AUTO_RESTORE=1` для ручного restore-flow | форк | `2026-03-29`, локально | `[ ]` | `[x]` | Capability `load_session/resume_session` остаются видимыми для Zed, но внутри `src/codex_agent.rs` automatic backend-restore заменяется на fresh backend-thread; старый диалог подтягивается вручную через `/resume`. |
| `/plan` mode и one-shot planning | форк | базовая ветка `2026-02-25`, `30e0d57a`; поведение стабилизировано `2026-02-26`, `f537f1d5` | `[ ]` | `[x]` | Логика в `src/thread/features/plan/*`, prompt-flow в `src/thread/prompt/flow.rs`. |
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

## 4. Что Стоит Подумать Всерьез

На текущем этапе для форка под `Zed` разумно держать такой shortlist:

1. `/ps`, но только если удастся показать это не уродливо: либо как аккуратный ACP-card/listing flow, либо как понятный status-pane сценарий, а не как сырой шумный dump.

Отдельное UX-направление, которое стоит держать рядом с этим shortlist:

- Чуть богаче selector UX уже partially shipped: нижний `context_control` selector теперь surfacing `status`, `ctx %`, `MCP` и `skills` как read-only summary entries с короткой строкой в списке и расширенным `description`. `Status` intentionally держит на кнопке только суммарный `used`; detail/report раскрывает workspace, account и `used / in / out`. Отдельный hover-only канал здесь по-прежнему упирается в текущий ACP / `Zed` client contract.
- Следующий вопрос не “добавлять ли `status` / `MCP` / `skills` вообще”, а какие ещё данные реально стоит поднимать в selector'ы, а что лучше оставить slash-командам или отдельным flows. Кандидат из этой зоны сейчас в первую очередь `plugins`.
- Для `soft /new` и `/fork` UX уже реализован, но ограничение нужно считать постоянной оговоркой: пока сам `Zed` не научится reset'ить ACP session view, старые сообщения в sidebar останутся видимыми даже после in-place thread switch.

## 4.1 Текущие Ограничения Zed UI

Ниже то, что уже проверено на практике и не стоит пытаться "чинить" только силами адаптера:

- Для ACP tool call у command approval `title` обязателен. Полностью убрать вторую строку заголовка нельзя без client-side изменений в `Zed` или без сомнительных zero-width hacks.
- `Zed` агрессивно обрезает длинный `title`, поэтому полный `reason` лучше держать в body, а не в заголовке карточки.
- Внутренний padding и layout approval-card задает сам `Zed`; адаптер может только слегка подправлять текстовое содержимое, но не контролирует нативные отступы контейнера.
- Если разбивать approval body на несколько ACP content-item'ов, `Zed` рисует между ними разделители. Для code block это выглядит плохо, поэтому command approval body у нас теперь отдается одним markdown-блоком.
- Верхний label `Run Command` тоже рисует сам клиент по `ToolKind::Execute`; адаптер может менять только вторичный title и содержимое body.

## 5. Скорее Вторым Эшелоном

- `/diff`, если появится понятный UX-контракт: показывать git diff рабочего дерева, session-local diff, или оба режима.
- `/debug-config` как developer-facing dump текущего runtime/config состояния.
- `/init` как bootstrap-команда для project instructions / `AGENTS.md`-style setup.
- `thread/read` surfaced UX для preview старых тредов без немедленного `resume`.

## 6. Пока Можно Спокойно Не Трогать

- `close_session`: в текущем `Zed` практическая ценность низкая, пока клиент сам не умеет закрывать ACP-сессию и сразу открывать новую для clean sidebar.
- `/logout`
- `fs/watch`
- override feature flags
- `codex_home` из `initialize`
- remote auth through client
- `DynamicToolCall`: потенциал есть как у моста к client-side/native UX, но для текущего Zed нет достаточно сильной surfaced-поверхности, чтобы держать даже partial runtime-support в основном коде. Возвращаться к нему имеет смысл только если появится конкретный Zed-side use case: например client-native picker, structured editor context или другой интерактивный UI, который нельзя нормально закрыть текущими ACP primitives. Для такого возврата сохранен backup-конспект в `docs/drafts/dynamic-tool-call-backup.md`.
