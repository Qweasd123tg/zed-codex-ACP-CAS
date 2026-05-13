# Матрица Фич И Сравнение С Upstream

Актуально на `2026-04-24` после повторного прогона `bash script/update_references.sh` и глубокого аудита месячного отставания по `ACP`, официальному `zed-industries/codex-acp`, `openai/codex` app-server и `Zed` ACP-клиенту.

## Снимок References

| Reference | Состояние | Дата / commit | Примечание |
| --- | --- | --- | --- |
| `agent-client-protocol` | обновлен | `2026-04-24`, `7d7dac5` | Локальная ссылка теперь указывает на `v0.12.2-9-g7d7dac5`. |
| `codex-acp-upstream` | обновлен | `2026-04-24`, `ee9418a` | Локальная ссылка теперь указывает на `v0.12.0`. Это основной источник для сравнения с официальным `zed codex acp`. |
| `codex` | обновлен | `2026-04-24`, `f802f0a39` | Локальная ссылка теперь указывает на `rusty-v8-v146.4.0-1093-gf802f0a39`. |
| `zed` | обновлен | `2026-04-24`, `1c1b03c3d6` | Локальная ссылка теперь указывает на `nightly-2-g1c1b03c3d6`. |

Сравнение ниже опирается прежде всего на `references/codex-acp-upstream@v0.12.0` и `references/codex@rusty-v8-v146.4.0-1093-gf802f0a39`. `zed`-референс важен для оценки реального client-side поведения ACP history/debug/session UI.

## Легенда

- `[x]` реализовано полноценно.
- `[~]` реализовано частично или есть только каркас/частичный plumbing.
- `[ ]` отсутствует.
- `<= 2026-02-18` означает: фича уже была в `codex-acp-upstream@v0.9.4`, точную первую точку в этой заметке отдельно не трассировал.

## Короткий Вывод

- По выбранному набору parity-фич с официальным `zed codex acp` у форка сейчас `9/15` полных совпадений, `1/15` частичное совпадение и `5/15` явных пробелов.
- Основные пробелы относительно официального адаптера: `close_session`, `/logout` и отдельные client-native UX-ветки, которые в текущем Zed ACP пока не дают достаточной отдачи.
- Основные сильные стороны форка: отдельный `resume_session`, workspace-scoped `/resume`, `/threads`, `/plan`, app-server-ориентированный flow восстановления тредов, нижний `Context` control, `Speed` service-tier selector, более точные ACP `ToolCallLocation` для Zed Follow и отдельный режим ручного restore через `ACP_DISABLE_AUTO_RESTORE=1` + `/resume`.
- Дополнительно форк теперь быстрее отдает первый ready-thread в `Zed`: skills/account/rate-limit metadata догружаются сразу после session response отдельным config update, а не держат весь `new_session` / `load_session` / `resume_session` в startup-loading. После аудита свежего Zed history UI адаптер также перестал подменять failed resume пустой свежей сессией: при `no rollout found` он сначала пробует найти rollout через `thread/read` и повторить `thread/resume` по path, а если история реально недоступна, возвращает явную ошибку.

## Правило Для Новых UX-Фич

Перед вводом новой user-facing фичи сначала проверять ownership: закрывается ли она кодом этого адаптера, или зависит от `Zed`, `ACP`, `codex app-server` либо другого upstream-контракта. Если видимый UI рисует `Zed` и ACP не дает явного поля/метода для управления этим UI, фича считается Zed-side до доказательства обратного. В таком случае в runtime адаптера не добавляется "готовая" ветка ради теоретического parity; сначала документируется ограничение, нужный Zed-side patch/контракт и fallback, который реально доступен в форке.

## 0.1 Что Реально Поменялось За Месяц

| Слой | Что изменилось | Дата / источник | Что это значит для форка |
| --- | --- | --- | --- |
| ACP protocol | `session/resume` стабилизирован, больше не просто unstable draft. | `2026-04-23`, `ac04ca2` / ACP `#1051`; changelog `v0.12.2`. | Наша идея с отдельным `resume_session` теперь совпадает с направлением протокола. Но dependency слой отстал: текущий `Cargo.toml` все еще держит `agent-client-protocol = 0.9.4`, тогда как официальный `codex-acp v0.12.0` уже на `0.11.1`. |
| ACP protocol | `session/close` стабилизирован. | `2026-04-23`, `efda480` / ACP `#1062`. | Функция стала официальной, но практическая ценность для нашего текущего Zed UX низкая. Не приоритет, пока нет задачи clean-close session lifecycle. |
| ACP protocol | Описан `additionalDirectories` контракт для `new/load/resume/list`. | RFD в `agent-client-protocol`, актуально в `v0.12.x`. | У нас сейчас фокус на single `cwd` + app-server thread state. Для полноценного multi-root continuity надо отдельно маппить `additional_directories` в app-server config/session list. |
| Official `codex-acp` | Релиз `v0.12.0`: переход на новый ACP Rust SDK shape и `codex rust-v0.124.0`. | `2026-04-24`, `74244b8`, `ee9418a`. | Главный технический долг: не отдельная команда, а API drift. Upstream `Agent` methods теперь получают `ConnectionTo<Client>`, thread state держится на `Arc`, а `local_spawner.rs` / `prompt_args.rs` удалены. |
| Official `codex-acp` | Добавлен ACP auth/logout capability, не только slash `/logout`. | `2026-03-31`, `a9e1075`, затем `v0.12.0` код. | У нас есть auth, но нет ACP `auth.logout` capability и нет handler `logout`. Это небольшой parity-gap, если нужен чистый account-switch UX. |
| Official `codex-acp` | MCP approval flow стал богаче: поддержан MCP elicitation как permission popup с persist modes. | `2026-03-31`, `c3e95ca`. | У нас есть ACP MCP passthrough и permission approvals, но именно upstream-style MCP elicitation approval стоит проверить отдельно, если используем MCP apps/connectors. |
| `codex app-server` | Permission model заметно усложнился: permission profiles, filesystem entries, strict auto-review, command permission profiles. | `2026-04-21` - `2026-04-23`, серия `#1827x`, `#19050`, `#19086`, `#19231`. | Это важнее `DynamicToolCall`: наши approvals должны следить за новой семантикой permission profiles, иначе UI может выглядеть работающим, но отвечать не тем профилем. |
| `codex app-server` | Появились sticky / turn-scoped environments и remote thread config endpoint. | `2026-04-21` - `2026-04-23`, `ddbe2536b`, `1d4cc494c`, `f11583b8f`. | Пока можно не трогать, но это будущий слой для managed environments. В ACP UI его сейчас лучше не поднимать без понятного UX. |
| `codex app-server` | `thread/resume` и `thread/fork` получили `excludeTurns`; thread state сильнее завязан на `ThreadStore`. | `2026-04-23`, `3d3028a5a`, `f1061d9d0`, `f1923a38b`. | Для нашего resume/replay это потенциально полезно: можно оптимизировать сценарии, где нам нужен context без полного history payload. Но надо сверить с текущим `ThreadResumeParams` pinned rev. |
| `codex app-server` | Укреплены device key / remote auth / Unix socket / remote plugin flows. | `2026-04-21` - `2026-04-23`, `69c3d1227`, `8a0ab3fc1`, `0d6a90cd6`. | Не приоритет для локального Zed workflow, но важно для будущего remote app-server сценария. |
| Zed ACP client | Новый ACP SDK, ACP debug view, session registration before load replay, usage UI fixes. | `2026-04-22` - `2026-04-24`, `58e2b7ecdd`, `2ca94a6032`, `1c1b03c3d6`. | Это прямой сигнал: Zed-side стал лучше для диагностики и load replay. Наш адаптер теперь надо тестировать против fresh Zed, а не только старого поведения history panel. |
| Zed Agent Panel / external agents | Очередь сообщений для external agents не отправляется на tool-call boundary. | Проверено по Zed docs и `zed-industries/zed#49601` от `2026-02-19`; актуально для `v0.224.6` и обсуждалось на nightly `v0.227.0`. | Для native `Zed Agent` queued messages могут уходить на следующей границе turn/tool-call, но для external ACP agents текущий клиент держит queued prompt до конца generation. У pinned `codex app-server` есть `turn/steer`, однако адаптер не сможет дать CLI-style steering до следующего tool call без Zed-side forwarding/extension method. |

## 1. Parity С Официальным `zed codex acp`

| Фича | Источник | Дата | Оригинальный `zed codex acp` | Наш форк | Где у нас / комментарий |
| --- | --- | --- | --- | --- | --- |
| `load_session` с replay истории | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У upstream replay идет через `src/codex_agent.rs`; у нас загрузка и replay разведены на `src/codex_agent.rs` и `src/thread/session/lifecycle.rs`. |
| `list_sessions` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У upstream список идет из rollout storage, у нас из `thread/list` app-server в `src/thread/session/lifecycle.rs`. С `2026-04-01` адаптер форматирует `updated_at` как RFC3339 и в `session/list`, и в live `SessionInfoUpdate`, чтобы Zed history не падал в `Unknown`. |
| `close_session` | `codex-acp-upstream` | `2026-03-13`, `be20828` | `[x]` | `[ ]` | В нашем `src/codex_agent.rs` capability `close` и handler `close_session` не реализованы. |
| Usage update / контекстное окно | `codex-acp-upstream`, `codex`, `zed` | `2026-02-27`, `34dc10c`; протокол виден в `codex` на `2026-03-03`, `8da7e4bda`; Zed ACP behavior проверен `2026-04-24`; adapter-side native indicator prep `2026-05-11` | `[x]` | `[~]` | У нас есть `ThreadTokenUsageUpdated` в `src/thread/features/notification/mod.rs` и `send_usage_update` в `src/thread/session/client.rs`. Adapter-side теперь пушит ACP `usage_update` не только после live token usage, но и после session load/resume, если есть cached context usage, чтобы новый Zed мог сразу отрисовать нативный context indicator. Если конкретная сборка Zed всё ещё не маппит external ACP `UsageUpdate` в круговой toolbar UI, fallback остается текстовым через `/status` и нижний `Context` selector. |
| Account rate-limit refresh after turn | форк + `account/rateLimits/read` | локально `2026-05-14` | `[ ]` | `[x]` | После обычного model turn адаптер делает best-effort refresh account limits с коротким timeout и применяет тот же notification path, что live `AccountRateLimitsUpdated`. Это держит нижний `Context` selector и `/status` свежее без ожидания следующего prompt; failure/timeout не шумит в чат. Reset-время для sub-day окон показывается с минутами, например `in 4h 23m`, чтобы selector не округлял лимиты только до часов. |
| Session config: `mode`, `permissions`, `model`, `reasoning_effort`, `context_control` | `codex-acp-upstream` + форк | `<= 2026-02-18`, `c0b82cc`; `permissions` и `context_control` локально, selector enrich `2026-03-31`, UX rename `2026-04-24`, grouped picker polish `2026-04-28`, model grouping `2026-05-05`, context display toggle `2026-05-10`, silent read-only selector picks `2026-05-11`, dynamic selector hover `2026-05-11`, Plan merged into Permissions `2026-05-11`, compact model label `2026-05-11`, split context usage display `2026-05-11`, model label style toggle `2026-05-12`, combined context/limits display `2026-05-12` | `[x]` | `[x]` | У нас это разнесено по `src/thread/session/config/*` и `src/thread/session/settings.rs`; отдельный нижний `Mode` selector больше не нужен: `Plan` surfaced внутри `Permissions` как workflow-пункт. Когда выбран `Plan`, эта же кнопка показывает `Plan`, но текущий read-only/workspace/full-access профиль сохраняется; выбор любого permission-профиля возвращает session в `Chat`/default и показывает выбранный профиль. `Model` теперь grouped control для `Models` / `Effort` / `Speed`, чтобы нижняя панель не раздувалась отдельными selectors. Так как ACP select передает только один `current_value`, активные nested пункты `Reasoning effort`/`Speed` помечаются `★` прямо в коротком option label. Model labels всегда приводятся к lowercase, чтобы backend-разнобой `GPT`/`gpt` не лез в UI; `Labels -> With gpt` показывает `gpt-5.5`, а `Labels -> Without gpt` режет префикс и короткие суффиксы вроде `5.5 ◕ ⚡`; `◔/◑/◕/●` показывают low/medium/high/extra-high reasoning, standard speed не показывает маркер, `⚡` означает fast, `~` — flex; полная сводка model/reasoning/speed остается в hover description. `context_control` в нижней панели теперь имеет три selected display mode: context-only, limits-only и combined context+limits. Context style, limits style, model label style и effort display style задаются через adapter config, поэтому style actions больше не шумят внутри selector UI. Hover у selected display mode сохраняет exact tokens, status и account limits. Вложенные selector style/display/layout preferences сохраняются adapter-side в `$CODEX_HOME/codex-acp/selector-preferences.json` с fallback-чтением старого `$CODEX_HOME/memories/codex-acp/selector-preferences.json`; layout часть умеет только known selectors/groups: `order`, `visible`, `name`, `groups`, без произвольных option values. Внутри selector остаются session status, `MCP`, `skills`, `plugins` и compaction. Read-only пункты selector не пишут summary в чат при клике: подробности доступны в Zed hover/description, а чат остается для `/status`, warning/status/error lifecycle notices и `Compact now`. Description самого selector тоже динамический: `Permissions` показывает Plan + сохраненный permission профиль либо текущий permission профиль, `Model` показывает model/reasoning/speed, а `Context` показывает percentage, exact tokens, status и account limits вместе прямо при hover на нижней кнопке. Чтение selector options теперь предварительно drain-ит фоновые app-server notifications, поэтому завершенная или failed compaction обновляет видимое состояние `Compacting...` без нового prompt. В Zed config selector descriptions сейчас plain text, не Markdown, поэтому визуальный polish идет через короткие option labels и ACP grouped select options: `Context` группируется в `Display` / `Integrations` / `Actions`, `Permissions` — в workflow/guarded/bypass. Account limits дополнительно дают компактный notice при 75/90/95/100% used; startup/resume snapshots только праймят dedupe-state, поэтому exhausted 5-hour notice не повторяется при следующем live update того же порога. |
| `/compact` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc`; local hang/UX fix `2026-05-11` | `[x]` | `[x]` | У нас команда реализована в `src/thread/features/session/controls.rs`. `/compact` теперь идет через отдельную prompt-flow ветку: адаптер сразу выставляет `compaction_in_progress`, показывает системный `Context compaction started`, очищает stale usage и только затем ждёт bounded `thread/compact/start`. Completion/failure notifications для context compaction обрабатываются как thread-scoped lifecycle events, а не только как active-turn items; после `/compact` или selector-triggered compaction адаптер запускает background drain watcher, чтобы Zed не оставался в `Context compaction is still running` / `Compacting...`. Selector action делает только короткий opportunistic drain, чтобы выбор `Compact now` не выглядел зависшим. |
| `/undo` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[x]` | У нас `undo` тоже вынесен в `src/thread/features/session/controls.rs`, а адаптер дополнительно понимает rollback через ACP ext methods (`zed.dev/codex/thread/rollback`, `session/rollback` и т.д.). Сам rollback-flow работает, но visual edit/rewind button и pencil-style edit UX в текущем `Zed` по-прежнему зависят от client-side ACP fix: внешний ACP bridge `Zed` пока не wire-ит `truncate()` / ext rollback path для этого UX. |
| Slash command visibility/disable config | форк | локально `2026-05-13` | `[ ]` | `[x]` | `$CODEX_HOME/codex-acp/selector-preferences.json` содержит `slash_commands` boolean map для surfaced commands. `false` убирает команду из ACP `available_commands` и блокирует ручной ввод до выполнения command flow; `/delete` остается hidden alias и следует за `archive`. |
| `/review` | `codex-acp-upstream` + `codex` (`review/start`) | `<= 2026-02-18`, `c0b82cc`; локально `2026-03-31` | `[x]` | `[x]` | У форка теперь есть user-facing inline review-flow через один основной entrypoint `/review`. Bare команда открывает ACP picker для uncommitted/base-branch/commit/custom сценариев, а кастомные инструкции задаются через `/review <text>`. |
| `/init` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc`; локально `2026-03-31` | `[x]` | `[x]` | `/init` теперь surfaced как builtin slash-команда в `src/thread/prompt/commands.rs` и идет как fixed prompt-turn с каноническим `AGENTS.md` bootstrap prompt. |
| `/logout` | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc` | `[x]` | `[ ]` | У нас есть только `authenticate`, но нет slash/logout handler. |
| ACP approvals для command / file change / tool user input | `codex-acp-upstream` | `<= 2026-02-18`, `c0b82cc`; backend-preset alignment `2026-05-14` | `[x]` | `[x]` | У форка это идет через `src/thread/core/server_requests.rs` и `src/thread/features/approvals/*`; для command approval popup теперь surfaced reason, очищенная inner shell-команда, `cwd`, network context и additional permissions. Command approval options теперь маппятся из Codex app-server `available_decisions` вместо жесткого набора allow/reject/cancel, поэтому Zed может показывать session/matching-command decisions, если backend их предлагает. File-change approvals теперь строго следуют `codex app-server`: если backend прислал `FileChangeRequestApproval`, адаптер всегда показывает confirmation popup; adapter-side `Ask edits` и auto-accept для таких backend requests убраны. |
| ACP `ToolCallLocation` / Zed Follow hints | ACP + Zed client Follow UI | local polish `2026-05-11` | `[x]` | `[x]` | File edits уже отдавали path + hunk line. Форк дополнительно отдает более точные locations для command cards и approval popups из structured `CommandAction` (`Read`, `ListFiles`, `Search`), с fallback на очевидный одиночный shell read/write path и `cwd` как последний вариант. Финальный `Turn diff` теперь тоже добавляет первый hunk line, когда line можно извлечь из unified diff. |
| `RequestPermissions` tool | `codex`, sync в `codex-acp-upstream` | `2026-03-08`, `e6b93841c`; в official adapter попало через `2026-03-13`, `be20828` | `[x]` | `[x]` | У нас есть отдельная typed-ветка `ServerRequest::PermissionsRequestApproval` и ACP popup в `src/thread/features/approvals/permissions.rs`. |
| `DynamicToolCall` (`item/tool/call`) | `codex`, sync в `codex-acp-upstream` | `2026-02-25`, `a0fd94bde`; в official adapter попало через `2026-03-13`, `be20828` | `[x]` | `[ ]` | В форке была только частичная экспериментальная ветка, но для текущего Zed она не дала достаточной практической отдачи. В runtime-пайплайне поддержку вырезали, а возвращаться к ней имеет смысл только если появится конкретный Zed-side client-native use case. Бэкап-конспект оставлен в `docs/drafts/dynamic-tool-call-backup.md`. |
| Forwarding warning-сообщений в клиент | `codex-acp-upstream` | `2026-03-05`, `a278432`; service notice framing `2026-05-11`, turn error dedupe `2026-05-11` | `[x]` | `[x]` | В `src/thread/features/notification/mod.rs` теперь поднимаются `ConfigWarning`, `DeprecationNotice` и `WindowsWorldWritableWarning`; служебный текст уходит в ACP-чат через общий Markdown quote notice formatter из `src/thread/session/client.rs`, чтобы warning/status/error сообщения рендерились как отдельный Zed-блок и не склеивались с обычным ответом агента. Одинаковый backend error text внутри одного turn дедуплицируется, потому что app-server может прислать сначала `Error`, а затем `TurnCompleted(Failed)` с тем же usage-limit сообщением. |
| ACP MCP passthrough + sanitize имен серверов | `codex-acp-upstream` | `2026-03-05`, `678a99e` | `[x]` | `[~]` | В форке `mcp_servers` из ACP теперь маппятся в session-scoped `thread/start` / `thread/resume` `config` overrides и переживают replacement-thread внутри одной ACP-сессии. Поддержаны `stdio` и `http`; ACP `sse` пока явно игнорируется. |

## 2. Расширения Форка Поверх Официального Адаптера

| Фича | Источник | Дата | Оригинальный `zed codex acp` | Наш форк | Где у нас / комментарий |
| --- | --- | --- | --- | --- | --- |
| Отдельный `resume_session` capability | `ACP` (`session/resume`, unstable) + `codex app-server` (`thread/resume`) + форк | ACP draft уже есть в `agent-client-protocol v0.11.4`; у форка базовая ветка есть с `2026-02-22`, `119b438f` | `[ ]` | `[x]` | У нас `SessionCapabilities::resume(...)` и отдельный handler в `src/codex_agent.rs`. |
| ACP `session/fork` capability | `ACP` (`session/fork`, unstable) + `codex` (`thread/fork`) + форк | ACP draft уже есть в `agent-client-protocol v0.11.4`; локально `2026-04-01` | `[ ]` | `[x]` | В `src/codex_agent.rs` теперь surfaced `SessionCapabilities::fork(...)` и handler `fork_session`. В отличие от slash `/fork`, ACP `session/fork` создает отдельную новую ACP-сессию поверх forked backend-thread, а не делает in-place switch текущего окна. При этом текущий `Zed` пока не дает отдельного native UI entrypoint для `session/fork`, так что practically используется slash `/fork` или патченый клиент. |
| `/threads` | форк | `2026-02-25`, `e1ace61b` | `[ ]` | `[x]` | Реализовано в `src/thread/features/resume/listing.rs`. |
| `/resume` с picker-ом по текущему workspace | форк + `thread/list` / `thread/resume` | `2026-02-25`, `e1ace61b`; UX/transport стабилизация `2026-03-29`, локально | `[ ]` | `[x]` | Реализовано через `src/thread/features/resume/selector.rs` и `apply.rs`; picker теперь paginated, с полным raw input, уникальным `ToolCallId` и cleanup transport-хвоста при переключении. |
| `/resume --no-history` | форк | `2026-02-25`, `b5cc35c3` | `[ ]` | `[x]` | Позволяет переключить context без replay старой ленты ACP. |
| `/fork` | `codex` (`thread/fork`) + форк | локально, `2026-03-31` | `[ ]` | `[x]` | Форкает текущий materialized backend-thread через `thread/fork` и переводит текущую ACP-сессию на forked thread. Sidebar history тоже остается видимой, потому что `Zed` не делает visual reset для in-place thread switch. |
| `/archive [partial_id]`, `/unarchive [partial_id]` | `codex` (`thread/archive`, `thread/unarchive`) + форк | нативные RPC есть в `codex`; ACP-ветка форка добавлена локально `2026-03-29` | `[ ]` | `[x]` | `/archive` скрывает тред из обычных списков без hard delete, `/unarchive` возвращает archived тред обратно. Если архивируется текущий активный тред, форк сразу поднимает fresh backend-thread под той же ACP-сессией. Для неоднозначных query archive/unarchive используют picker с полным `raw_input`, как `/resume`. Дополнительно оставлен скрытый compatibility alias `/delete -> /archive`; это не настоящий delete и в ACP/Zed custom-agent delete UI пока не surfaced. Для нативной delete-кнопки в history нужен и `session/delete` в ACP, и патч `Zed` ACP bridge. |
| `/rename <name>` | `codex` (`set_thread_name`) + форк | нативный op есть в `codex`; ACP-ветка форка добавлена локально `2026-03-29` | `[ ]` | `[x]` | Использует `thread/name/set`, сразу обновляет `SessionInfoUpdate` в ACP и поднимает `thread.name` в `/threads` и `/resume`. |
| `ACP_DISABLE_AUTO_RESTORE=1` для чистого старта без потери ручного history-open | форк | `2026-03-29`, локально; startup-window guard `2026-04-01`, локально | `[ ]` | `[x]` | Capability `load_session/resume_session` остаются видимыми для Zed. Внутри `src/codex_agent.rs` подавляется только самый ранний startup-driven restore сразу после старта агента; поздние явные открытия из history снова идут через обычный restore-path. |
| `/plan` mode и one-shot planning | форк | базовая ветка `2026-02-25`, `30e0d57a`; поведение стабилизировано `2026-02-26`, `f537f1d5` | `[ ]` | `[x]` | Логика в `src/thread/features/plan/*`, prompt-flow в `src/thread/prompt/flow.rs`. |
| `Speed` model-group option | `codex` `service_tier` + форк | `service_tier` в app-server protocol есть в срезе `2026-04-23`; локально `2026-04-24`, grouped under `Model` `2026-05-05` | `[ ]` | `[x]` | У форка есть backend `fast_mode` parser/helper в `src/thread/session/config/fast_mode.rs` и handler в `src/thread/session/settings.rs`, но UX теперь surfaced внутри grouped `Model` selector как `Speed: Standard/Fast/Flex`. Значение хранится в `ThreadInner.service_tier`, синхронизируется через `thread/start`, `thread/resume`, `thread/fork`, in-place `/resume`/`/fork` и уходит в `turn/start` для новых turns. Это не новый `ModeKind`: `Plan`/`Default` остаются collaboration-mode state, просто `Plan` теперь выбирается через `Permissions` selector. |
| Collab tool-call UI | форк | `2026-02-24`, `45c084ee` | `[ ]` | `[x]` | Отдельный доменный срез в `src/thread/features/collab/*`. |

## 3. Свежие Возможности `codex app-server`, Которые Пока Не Полностью Подняты В Форке

Эти пункты уже видны в обновленном `references/codex`. Большая часть пока не отражена в ACP-обвязке форка; отдельные элементы используются точечно как compatibility/fallback path.

| Возможность | Источник | Дата | Статус форка | Комментарий |
| --- | --- | --- | --- | --- |
| Remote app-server auth through client | `codex` | `2026-03-25`, `1ff39b6fa` | `[ ]` | Полезно для удаленных/проксируемых сценариев, сейчас форк это не пробрасывает. |
| `fs/watch` API | `codex` | `2026-03-24`, `301b17c2a` | `[ ]` | В ACP-слой форка никакого watch-моста пока нет. |
| Override feature flags method | `codex` | `2026-03-24`, `0b08d8930` | `[ ]` | Пока форк не умеет управлять фичефлагами app-server извне. |
| `initialize` возвращает `codex_home` | `codex` | `2026-03-24`, `24c4ecaaa` | `[ ]` | В нашем мосте это сейчас не surfaced наружу. |
| ChatGPT device-code login в app-server | `codex` | `2026-03-27`, `47a9e2e08` | `[ ]` | У форка авторизация пока завязана на существующий login flow, без нового server-side device-code пути. |
| `thread/turns/list` | `codex` app-server protocol | видно в срезе `2026-04-23` | `[ ]` | Можно использовать для read-only turn preview или более дешевого восстановления деталей треда, но отдельного ACP UX в форке пока нет. |
| `thread/read` как fallback для failed `thread/resume` | `codex` app-server protocol + форк | аудит `2026-04-24` | `[x]` | Свежий app-server лучше умеет читать persisted/archived thread metadata через `thread/read`, чем старый resume-by-id path. Форк использует это как восстановительный fallback: если `thread/resume` по id отвечает `no rollout found`, адаптер читает `thread.path` через `thread/read` и повторяет `thread/resume` по path. Если path тоже не помогает, Zed получает ошибку вместо пустого fake-thread без истории. |
| Marketplace / plugin management (`marketplace/add`, `remove`, `upgrade`) | `codex` app-server protocol + TUI | видно в срезе `2026-04-23` | `[ ]` | У форка сейчас есть read-only `plugins` summary в `Context`, но нет ACP flow для установки/обновления marketplaces/plugins. |
| Auto-review permissions mode / Guardian approval review и verification notifications | `codex` app-server protocol + TUI | видно в срезе `2026-04-23`; TUI `/permissions` показывает `Auto-review` как отдельный workspace-write режим | `[ ]` | Это не bypass/full-access: `Auto-review` сохраняет workspace-write permission profile, но меняет reviewer/routing для eligible `on-request` approvals на auto-reviewer subagent. В протоколе есть `ApprovalsReviewer::AutoReview`, `item/autoApprovalReview/*`, `guardianWarning`, `model/verification` и `thread/approveGuardianDeniedAction`. План для форка: после protocol/dependency bump добавить отдельный пункт `Auto-review` в `Permissions -> Guarded` рядом с `Workspace`, прокинуть reviewer policy в `thread/start`/resume/fork/turn, и отдельно отрендерить auto-review lifecycle notifications. До этого не смешивать с `Full access`/bypass. |
| Model speed-tier metadata (`additional_speed_tiers`) | `codex` app-server protocol + TUI | видно в срезе `2026-04-23` | `[~]` | `Speed` selector уже есть, но текущий pinned protocol/API форка не дает полноценного model-level gating как в свежем TUI. Поэтому selector намеренно не скрывается по модели. |
| `turn/steer` / mid-turn queued input | `codex` app-server protocol + Zed Agent Panel | `codex` protocol уже содержит `turn/steer`; Zed external-agent limitation задокументирован в `zed-industries/zed#49601` | `[ ]` | Backend может принять input в active turn через `expected_turn_id`, но текущий Zed external-agent UI не прокидывает queued prompt на boundary. Добавлять adapter-side runtime path имеет смысл только после проверки, что свежий Zed реально шлет второй `session/prompt` до завершения первого turn, или после появления явного ACP/Zed extension method для steering. |
| Thread memory mode и item injection (`thread/memoryMode/set`, `thread/inject_items`) | `codex` app-server protocol | видно в срезе `2026-04-23` | `[ ]` | В ACP-слой форка пока не поднято; не стоит добавлять полумертвый runtime path без ясного Zed UX. |
| External agent config migration / import | `codex` TUI | видно в срезе `2026-04-23` | `[ ]` | В TUI появился startup migration flow для внешних agent configs/plugins. Для ACP CAS это отдельный продуктовый сценарий, не простой transport passthrough. |
| Permission profile enforcement model | `codex` core/app-server protocol | `2026-04-21` - `2026-04-23` | `[~]` | У форка есть permissions approval UI, но свежий `codex` уже различает canonical active profiles, command overlays, filesystem entries и strict auto-review. Это нужно проверять при обновлении pinned `codex` rev, иначе можно потерять точность approval-response. |
| `excludeTurns` на `thread/resume` / `thread/fork` | `codex` app-server protocol | `2026-04-23`, `3d3028a5a` | `[ ]` | Потенциально полезно для чистого context-switch без тяжелого payload history. Сейчас форк решает это своим `include_history` / replay layer, но после dependency bump стоит сверить, можно ли заменить часть логики нативным параметром. |
| Sticky / turn-scoped environments | `codex` app-server protocol | `2026-04-21` - `2026-04-23` | `[ ]` | Не нужен для текущего локального Zed workflow, но это будущий слой managed execution environment. Без Zed UX лучше не поднимать. |
| Unix socket transport | `codex` app-server | `2026-04-23`, `8a0ab3fc1` | `[ ]` | Может быть полезно для надежного local app-server transport, но текущий adapter bridge уже работает через stdio process. |

## 4. Что Стоит Подумать Всерьез

На текущем этапе для форка под `Zed` разумно держать такой shortlist:

1. Обновить dependency/API слой: `agent-client-protocol 0.9.4 -> 0.11.x/0.12.x` и pinned `zed-industries/codex` rev -> актуальный `openai/codex rust-v0.124.0` или осознанный свежий commit. Это не косметика: upstream `Agent` API, auth capabilities, permission profiles и app-server protocol shape уже разошлись с нашим базовым контрактом.
2. После dependency bump отдельно прогнать audit approval-flow: command/file/user-input permissions, `RequestPermissions`, MCP elicitation approvals, strict auto-review. Это зона с самым высоким риском тихих семантических регрессий.
3. Поднять `Auto-review` как отдельный guarded permission mode, а не как bypass: selector item рядом с `Workspace`, reviewer policy `auto_review`, lifecycle UI для `item/autoApprovalReview/*`, явное отличие от `Full access`.
4. `thread/read` preview как read-only surfaced flow без немедленного `resume`: transport уже есть, а практическая ценность для ежедневной навигации выше, чем у ещё одной параллельной status-команды.

Отдельное UX-направление, которое стоит держать рядом с этим shortlist:

- Канонический status-pane теперь уже surfaced: есть отдельный `/status`, а нижний `Context` selector держит `status`, `ctx %`, `MCP`, `skills`, `plugins` и limits как read-only summary entries с короткой строкой в списке и расширенным `description`. Сам selector может показывать в нижней панели либо контекст, либо остаток `5h` лимита, а hover на кнопке `Context` всегда объединяет context usage и account limits; `Status` intentionally держит на кнопке только суммарный `used`; detail/report раскрывает workspace, account и `used / in / out`. Нативный context circle остается Zed-side UX и не должен считаться закрытым только из-за adapter-side `UsageUpdate`.
- Следующий вопрос теперь не “как ещё назвать status-команду”, а какие новые read-only preview flows реально полезно поднимать рядом с уже существующим `/status`, `/diff` и selector UX. Основной кандидат из этой зоны сейчас `thread/read`.
- Для нового чистого чата канонический путь теперь native `Zed` `New Thread`. In-place switch в рантайме сознательно оставлен только для `/fork` и archive-triggered replacement; пока сам `Zed` не научится reset'ить ACP session view, старые сообщения в sidebar останутся видимыми после таких сценариев.

### 4.1 UX Backlog

Идеи ниже зафиксированы как practical follow-ups после сжатия нижней панели selectors и не требуют Zed-side патча, если не указано обратное:

- `Context -> Display`: добавить четвертый display mode для remaining/free контекста. Важно явно отличать его от used %, например `24% free`, чтобы пользователь не путал остаток с заполнением.
- `Context` style actions: оставить короткие display styles без лишних слов: braille-only (`⣶`), used percentage-only (`76%`), future free percentage (`24% free`). Подробности должны оставаться в hover/description.
- `Model` selector: в hover для `Speed: Fast/Flex` добавить пояснение, что это request для новых turns when available, а не жесткая гарантия service tier.
- `Permissions` selector: сделать hover еще плотнее и предсказуемее: `Workflow: Plan` + `Permissions: Workspace/Read only/...`, чтобы было ясно, что Plan не стирает сохраненный sandbox/approval profile.
- `Compact now`: при запуске compaction держать в selector короткое, заметное состояние, например `Compacting...`; если позже появится дешевый progress/stale usage hint, можно заменить на более информативный compact marker.
- `Follow`: продолжать улучшать `ToolCallLocation` там, где app-server дает точные пути. Для search/list command actions использовать директорию поиска, включая `"." -> cwd`, а для shell fallback оставлять только очевидные одиночные read/write paths.

Already applied from this backlog:

- Command approval cards no longer use generic `Details` when the app-server asks for command permission. The adapter now reuses command-title heuristics for approval cards, including `Network access: host` for `curl`/`wget`/`iwr` style commands and `Create file: name` / `Create folder: name` / `Delete path: name` for obvious file operations. This matters because Zed collapses the approval body after completion and leaves the title as the durable visible text.
- `Reasoning effort` now lives inside the grouped `Model` selector as the `Effort` section. Effort/model label display styles are adapter-config preferences, so the selector keeps only operational model/effort/speed choices.
- `/status` now uses the same compact section model as selector hover: `Context`, `Limits`, `Session`, `Integrations`, while preserving MCP/skills/plugins reports.

## 4.2 Текущие Ограничения Zed UI

Ниже то, что уже проверено на практике и не стоит пытаться "чинить" только силами адаптера:

- Для ACP tool call у command approval `title` обязателен. Полностью убрать вторую строку заголовка нельзя без client-side изменений в `Zed` или без сомнительных zero-width hacks.
- `Zed` агрессивно обрезает длинный `title`, поэтому полный `reason` лучше держать в body, а не в заголовке карточки.
- Внутренний padding и layout approval-card задает сам `Zed`; адаптер может только слегка подправлять текстовое содержимое, но не контролирует нативные отступы контейнера.
- Если разбивать approval body на несколько ACP content-item'ов, `Zed` рисует между ними разделители. Для code block это выглядит плохо, поэтому command approval body у нас теперь отдается одним markdown-блоком.
- Верхний label `Run Command` тоже рисует сам клиент по `ToolKind::Execute`; адаптер может менять только вторичный title и содержимое body.
- Набор кнопок в command approval popup задается adapter-side ACP options. Форк теперь маппит их из `available_decisions` app-server, но точные подписи и группировка все еще ограничены тем, как `Zed` рисует permission options.
- Правый красный `x` у failed/cancelled tool/permission cards — client-side status indicator в `Zed`. Адаптер передает ACP status, но не управляет самой иконкой и не может adapter-side добавить симметричную зеленую галочку для completed state. Практичный adapter-side fallback — не маппить успешные/обычные исходы в failed/cancelled без причины и держать title/content достаточно информативными после сворачивания карточки.
- Selected-agent / `New Thread` trigger в текущем `Zed` может визуально пульсировать только пока движется указатель мыши. По фактическому поведению это больше похоже на client-side repaint/animation quirk, чем на отдельную задержку ACP startup path.
- Нативный retry/error banner над composer, как у встроенного Zed Agent (`error sending request... Retrying...`), сейчас не является ACP surface для external agents. Адаптер может показать status/error в чате, но не может отрисовать этот жёлтый transient banner без Zed-side патча или нового ACP/Zed контракта.
- Нативный left-toolbar `Thinking Mode` / effort split-button с иконкой, как у встроенного Zed Agent, сейчас рендерится только для native thread. ACP `SessionConfigOptionCategory::ThoughtLevel` может служить подсказкой для keyboard/icons/placement, но текущий external `ConfigOptionsView` рисует такие options обычными text selectors справа. Adapter-side можно дать `Reasoning`/`Thinking` selector и hover, но иконка, split-button и placement слева требуют Zed-side поддержки.
- Нативный context circle для custom ACP CAS должен идти через ACP `usage_update` (`used` + `size`). Adapter-side теперь отправляет этот update на live usage и на load/resume из cached usage, но если конкретная сборка `Zed` не маппит external ACP `UsageUpdate` в toolbar token-usage UI, fallback остается текстовым через `/status` и `Context`.
- Нативная toolbar-кнопка Fast Mode в свежем `Zed` завязана на native-thread/staff/model gating и `supports_fast_mode()`. Для custom ACP CAS она не является универсальным внешним контролом, поэтому форк поднимает отдельный `Speed` selector через обычный ACP session config path без патча клиента.
- Для ACP session history `updated_at` реально показывается только в полном history-view и во встроенном блоке `Recent` внутри чата. Toolbar/dropdown `Recently Updated` в `Zed` рендерит только `title`, без времени, `cwd` и `meta`.
- В свежем Agent UI `Zed` имя чата можно редактировать через session `title`, если этот client-side path включен для external agents. Adapter-side контракт уже держится на `SessionInfoUpdate.title` и `/rename`, но сама inline-редакция заголовка остается Zed-side UX, а не отдельной runtime-фичей адаптера.
- `cwd` и `meta` в `AgentSessionInfo` до клиента доезжают, но текущие history/render пути `Zed` их не рисуют. Это уже client-side ограничение, а не баг адаптера.

## 5. Скорее Вторым Эшелоном

- `/debug-config` как developer-facing dump текущего runtime/config состояния.
- `thread/read` surfaced UX для preview старых тредов без немедленного `resume`.

## 5.1 Рабочий Процесс `mini` Ветки

`main` остается стабильной веткой для проверенных изменений, release-flow и тегов
`v*`. Ветка `mini` используется как экспериментальный буфер для работы на более
дешевой/слабой модели, чтобы не тратить лишний лимит `GPT-5.5 High` и не рисковать
релизной веткой при проверке гипотез.

Правила работы:

- Новые небольшие UX-гипотезы, docs-заметки и безопасные эксперименты можно вести
  в `mini`.
- `mini` не тегируется `v*` и не считается релизной веткой.
- Перед переносом изменений из `mini` в `main` нужно просмотреть diff и прогнать
  обязательные проверки: `cargo fmt --all -- --check`, `cargo test`,
  `cargo clippy --all-targets --all-features -- -D warnings`.
- Для user-facing изменений перед релизным переносом дополнительно запускать
  `bash script/build_local_release.sh`.
- Если слабая модель сделала плохой diff, чинить или переписывать нужно `mini`, не
  трогая стабильный `main`.
- Переносить удачные изменения в `main` через merge или cherry-pick после review.
  Если change-set user-facing, вместе с переносом обновлять версию по политике
  релизов.
- Релизный порядок: `main` -> approved changes from `mini` -> version/checks/local
  release build -> commit/push -> tag `v*` только из `main`.

## 6. Пока Можно Спокойно Не Трогать

- `close_session`: в текущем `Zed` практическая ценность низкая, пока клиент сам не умеет закрывать ACP-сессию и сразу открывать новую для clean sidebar.
- `/logout`
- `fs/watch`
- override feature flags
- `codex_home` из `initialize`
- remote auth through client
- Нативный Zed toolbar Fast Mode button для custom ACP CAS: текущий практичный путь уже закрыт adapter-side `Speed` selector, а полное toolbar parity требует client-side UX контракта.
- Нативный Zed `Thinking Mode` / effort split-button для custom ACP CAS: без client-side mapping `ThoughtLevel` config options в left-toolbar контрол это останется обычным external selector, поэтому не добавлять adapter-side "готовую" иконку/placement-фичу.
- `DynamicToolCall`: потенциал есть как у моста к client-side/native UX, но для текущего Zed нет достаточно сильной surfaced-поверхности, чтобы держать даже partial runtime-support в основном коде. Возвращаться к нему имеет смысл только если появится конкретный Zed-side use case: например client-native picker, structured editor context или другой интерактивный UI, который нельзя нормально закрыть текущими ACP primitives. Для такого возврата сохранен backup-конспект в `docs/drafts/dynamic-tool-call-backup.md`.
