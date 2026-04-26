# Codex ACP CAS

`codex-acp` is a practical fork of the Zed Codex ACP adapter. It connects Codex to ACP-compatible clients such as Zed through `codex app-server`.

This fork is focused on real daily use: better startup diagnostics, better session lifecycle behavior, more usable resume/archive/rename flows, and Linux-first stability improvements.

## Status

This project is usable, but still beta.

- Main real-world target today: Linux on `x86_64-unknown-linux-gnu`
- Fedora is the most tested environment today
- GitHub Releases are intended to ship:
  - Linux `x86_64-unknown-linux-gnu`
  - macOS Apple Silicon `aarch64-apple-darwin`
  - Windows `x86_64-pc-windows-msvc`
- Behavior may still change between releases

## Screenshots

### Resume and thread management

Resume picker with workspace-scoped thread selection:

![Resume picker](screenshot/resume.png)

Context selector and session controls:

![Selector context](screenshot/selector%20context.png)

Context limits and usage view:

![Selector limits](screenshot/selector%20limits.png)

### Plan mode

Plan mode and visible planning steps:

![Plan mode](screenshot/plan.png)

### Thread operations

Archive flow:

![Archive flow](screenshot/archive.png)

Rename flow:

![Rename flow](screenshot/rename.png)

Unarchive flow:

![Unarchive flow](screenshot/unarchive.png)

### Collaboration UI

Sub-agent and collaboration tool-call rendering:

![Subagents UI](screenshot/subagents.png)

## Supported Now

- ACP prompt capabilities: embedded context and image input
- Session lifecycle:
  - `new_session`
  - `load_session`
  - `fork_session`
  - `resume_session`
  - `list_sessions`
- Session-scoped MCP passthrough for `stdio` and `http`
- History replay after `load_session` and `resume_session`
- Session commands:
  - `/init`
  - `/status`
  - `/review`
  - `/threads`
  - `/resume`
  - `/fork`
  - `/archive`
  - `/unarchive`
  - `/rename`
  - `/compact`
  - `/undo`
  - `/plan`
  - hidden compatibility alias `/delete -> /archive`
- Better thread title handling for resume/archive/rename/fork flows
- ACP `session/fork` surfaced on top of native `thread/fork`
- Inline review flows for uncommitted changes, base branches, and specific commits, centered on one ACP picker behind `/review`
- In-place `/fork` and standard ACP `session/fork` support
- Tool call cards for command, MCP, web, image, file, and collab branches
- Clearer status surfacing through `/status` and the existing `Context` selector
- `Context` selector summaries for session status, context usage, MCP, skills, plugins, limits, and compaction
- Chat warnings when account limits cross 75%, 90%, 95%, and exhausted thresholds
- `Fast Mode` session config selector backed by Codex app-server `service_tier`, with `fast` as the primary surfaced value
- Practical plan mode support
- Better default-mode fallback plan progress for long step lists: visible checkpoints now advance across the list instead of only snapping at the very end of work
- Better startup and reconnect diagnostics
- Shorter first-open loading pulse: skills/account/limits metadata now hydrate right after the initial session response instead of blocking `new_session` / `load_session` / `resume_session`
- Safer turn-start timeout and stale turn-tail cleanup around reconnects
- Safer history replay fencing for `/undo` and auto-restored session history
- Less UI freeze risk during `/resume --history` by replaying restored history outside the main session mutex
- Less duplicate file-change I/O when one patch item touches the same path multiple times
- More reliable file navigation from completed grouped file-change diff cards
- Less mutex hold time while waiting for file-change approval prompts
- Less chat stall while command approval prompts are pending
- Faster file-change start cards with ACP snapshot priming moved out of the main session mutex
- Less mutex hold time while final file-change diff is published; ACP buffer writeback is opt-in via `CODEX_ACP_SYNC_EDIT_BUFFERS=1`
- Safer transport drain: stale server requests are rejected during post-turn and pre-prompt cleanup instead of triggering late approvals
- Less reconnect spam: reconnect warnings now collapse into one normalized status line while reconnect-assisted stalled turns still abort cleanly
- Less brittle transport cleanup: background drain and thread-switch flush now wait for the queue to go quiet instead of assuming `64` messages or one tiny timeout is enough
- Less turn-completion lock contention: turn diff rendering now runs outside the main session mutex and optional ACP buffer writeback skips paths already reserved by file-change lifecycle
- Less sparse completed tool cards: no-output commands, MCP results, and completed collab/sub-agent calls now keep a short visible summary while retaining raw details
- Less transport serialization during quiet backend periods: app-server stdout now has a dedicated reader/inbox, so cancel, interrupt, post-turn drain, and thread-switch cleanup do not sit behind one long `next_message()` mutex wait

## Why Use This Fork

Compared with upstream-oriented adapter work, this fork currently focuses more on:

- Better startup diagnostics when Zed or `codex app-server` fails early
- Better session resume and thread switching behavior
- Better archive, unarchive, and rename handling
- More usable ACP rendering for collaboration and sub-agent flows
- Linux-first practical fixes

## Differences From Upstream

This project does not claim full upstream parity.

Current strengths of this fork:

- More robust startup behavior and clearer logging
- Less startup latency before Zed gets a ready ACP thread
- Better session lifecycle handling in ACP clients
- Less UI freeze risk during `/undo` history rebuilds
- Less UI freeze risk during `/resume --history` thread switches
- Less repeated ACP snapshot and writeback churn on multi-hunk file edits
- Less chat stall while waiting for file edit approval
- Less chat stall while waiting for shell command approval
- Less lock contention while file-change start cards are published
- Less lock contention while file-change completion diff/writeback is published
- Less lock contention while final turn-diff cards and ACP buffer sync are published
- Less risk of ghost approvals from stale app-server requests during drain/flush cleanup
- Clearer reconnect UX with one normalized retry status and cleaner reconnect-assisted stall aborts
- More reliable pre-prompt and thread-switch cleanup under bursty app-server tails
- Better transport responsiveness while the backend is quiet: app-server message reads no longer monopolize the transport mutex for the full wait window
- Better thread titles in lists and resumed sessions
- Inline review flows backed by native `review/start`
- Practical thread switching with native `Zed` `New Thread`, `/fork`, `/resume`, and archive-triggered replacement
- Standard ACP `session/fork` surfaced separately from the in-place slash `/fork` flow
- Practical plan mode support
- Canonical session status surfacing through `/status` plus the `Context` selector
- `plugins` now sit alongside `status`, `MCP`, `skills`, and limits in the selector UX
- `Fast Mode` is surfaced as session config over Codex `service_tier` and carried through start, resume, fork, and future turns
- More complete collab and sub-agent UI mapping

Current gaps:

- No full structured elicitation parity yet
- Manual `Plan mode` is usable, but it is not an exact match for Codex CLI `update_plan` autoplan rendering; think of it as a CLI-like collaboration flow rather than the same UI contract
- Default-mode fallback checkpoint rendering is intentionally pragmatic ACP UI, not a pixel-for-pixel clone of Codex CLI autoplan visuals
- `DynamicToolCall` is intentionally unsupported in runtime code for now; the old partial implementation was removed and summarized in `docs/drafts/dynamic-tool-call-backup.md`
- Some upstream-style flows are still missing or incomplete, including `close_session` and `/logout`
- There is still no true delete path end-to-end: `codex app-server` does not give this fork a practical hard-delete flow, and the current ACP bridge in `Zed` still does not surface `session/delete`, so `/delete` stays only as a compatibility alias to `/archive`
- Slash `/new` is intentionally not surfaced anymore. Use native `Zed` `New Thread` for a real new ACP session; in-place backend switching remains only for `/fork` and archive-triggered replacement flows. Standard ACP `session/fork` is supported by the adapter, but current `Zed` still has no dedicated UI entrypoint for it, so slash `/fork` remains the practical path unless you patch the client. The old soft-new behavior is summarized in `docs/drafts/soft-new-backup.md`
- Some behavior still depends on Zed-side ACP support

## Limitations

- MCP passthrough supports `stdio` and `http` today
- MCP `sse` passthrough is not supported yet
- `item/tool/call` / `DynamicToolCall` requests are rejected as unsupported
- `Fast Mode` is available through the adapter `Fast Mode` session config selector. It primarily exposes `service_tier=fast`, with `flex` available as the alternate Codex service tier. Zed's native toolbar fast-mode button is currently native-thread/staff/model-gated and is not a generic custom ACP control, so custom ACP users should use the adapter selector instead.
- `/undo` itself works, and the adapter also exposes rollback via ACP ext methods, but the visual rewind/edit button and the pencil-style edit UX in current `Zed` still depend on a client-side ACP fix: the external-agent ACP bridge does not wire `truncate()` / rollback ext-methods for this flow yet. In practice that means patching or rebuilding `Zed` if you want the native button UX
- Zed Agent Panel message queueing is asymmetric today: native `Zed Agent` can send queued messages at turn/tool-call boundaries, but external ACP agents receive queued messages only after the current generation finishes. The pinned Codex app-server has `turn/steer`, so the backend can accept mid-turn steering, but this adapter cannot expose a real CLI-style "send before the next tool call" UX until `Zed` forwards queued external-agent prompts before generation completion.
- Recent Zed Agent UI can edit chat names through the session `title` when that client-side path is enabled for external agents. The adapter already publishes titles through `SessionInfoUpdate` and `/rename`; inline title editing itself is controlled by Zed.
- The selected-agent / `New Thread` trigger in current `Zed` can show a visibly odd pulsing state that appears only while the pointer is moving. In practice this looks like a client-side repaint/animation quirk, not an ACP startup stall in the adapter
- While history replay is restoring after `load_session` or replaying `/undo`, new prompts and session commands are intentionally fenced until replay finishes; this avoids overlapping turn/replay state in one ACP session
- Linux is the most tested platform right now
- Multi-platform release artifacts can exist before all platforms are equally tested in real use

## Install

### From GitHub Releases

Download the artifact for your platform from the releases page.

Current release artifacts are plain binaries plus `.sha256` files:

- `codex-acp-cas-<version>-x86_64-unknown-linux-gnu`
- `codex-acp-cas-<version>-<target>.sha256`

Place the binary somewhere on your `PATH`, mark it executable, and point Zed at that binary.

Example:

```bash
mkdir -p "$HOME/.local/bin"
mv codex-acp-cas-<version>-x86_64-unknown-linux-gnu "$HOME/.local/bin/codex-acp"
chmod +x "$HOME/.local/bin/codex-acp"
```

Then configure Zed to use the binary path:

```json
{
  "agent_servers": {
    "codex-acp-cas": {
      "type": "custom",
      "command": "/home/your-user/.local/bin/codex-acp"
    }
  }
}
```

### Add To Zed

1. Install or build `codex-acp` and make sure the binary path is stable.
2. Open your Zed settings JSON.
3. Add a custom agent server entry pointing to the `codex-acp` binary.
4. Restart Zed if the new agent does not appear immediately.

If you run the adapter directly from a repository checkout during local development, prefer
pointing Zed at `.build/codex-acp-current` and rebuilding with:

```bash
bash script/build_local_release.sh
```

That script rotates `.build/codex-acp-current` and `.build/codex-acp-previous`. Rebuilding only
`target/release/codex-acp` does not update the binary path if Zed is already configured to use
`.build/codex-acp-current`.

Example:

```json
{
  "agent_servers": {
    "codex-acp-cas": {
      "type": "custom",
      "command": "/home/your-user/.local/bin/codex-acp"
    }
  }
}
```

If `codex` is not already available in your environment, make sure it is installed and visible in `PATH`, because this adapter starts `codex app-server` under the hood.

### Build From Source

Requirements:

- Rust toolchain
- `codex` available in your environment

Build:

```bash
bash script/build_local_release.sh
```

Run:

```bash
./target/release/codex-acp --help
```

The local release script also keeps two rollback-friendly copies in the repository:

- `.build/codex-acp-current`
- `.build/codex-acp-previous`

## Development Checks

Basic local checks:

```bash
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

Release-target check for Linux:

```bash
cargo test --release --target x86_64-unknown-linux-gnu
```

## Configuration

Useful environment variables:

- `RUST_LOG=codex_acp=debug`
- `RUST_BACKTRACE=1`
- `CODEX_ACP_STARTUP_TIMEOUT_MS=<milliseconds>`
- `CODEX_ACP_STARTUP_METADATA_TIMEOUT_MS=<milliseconds>`
- `ACP_DISABLE_AUTO_RESTORE=1` for emergency startup debugging only

`CODEX_ACP_STARTUP_TIMEOUT_MS` now also bounds the `turn/start` handshake, so an app-server that stops responding before it returns a `turn_id` does not leave the ACP UI spinning forever.

Do not keep `ACP_DISABLE_AUTO_RESTORE=1` in your normal Zed configuration. It suppresses the earliest startup-driven backend restore right after the agent boots, which can make history entries appear in Zed while their chat content does not load. Use it only as a temporary diagnostic option if startup restore itself is hanging.

## Troubleshooting

If Zed seems to hang or the adapter looks like it crashed, run Zed from a terminal:

```bash
RUST_LOG=codex_acp=debug RUST_BACKTRACE=1 zed
```

Important log lines:

- `Starting codex app-server process`
- `Initializing codex app-server`
- `Sending startup-sensitive app-server request`
- `Queued app-server request while waiting for a response`
- `Timed out waiting for app-server startup response`
- `codex app-server closed stdout`
- `Turn appears stuck after repeated reconnect failures`

What they usually mean:

- Timeout during `initialize`, `thread/start`, or `turn/start`: app-server is stuck before the adapter can safely continue
- `failed to start 'codex' app-server`: `codex` is missing or not available in `PATH`
- `Turn appears stuck after repeated reconnect failures`: the adapter aborted a stalled turn and drained queued tail notifications so the next prompt starts from a clean state
- Panic backtrace: the adapter or child process crashed directly

Recent hardening in this fork:

- `ItemStarted` and `ItemCompleted` from the wrong `turn_id` are ignored instead of creating stale tool cards after reconnect or thread switch
- reconnect-stall watchdog abort now runs the same post-turn drain path as normal turn completion

## More Docs

User-facing documentation stays in this README. Deeper project notes are kept separately:

- [docs/upstream-feature-matrix.md](docs/upstream-feature-matrix.md)
- [docs/thread-feature-map.md](docs/thread-feature-map.md)
- [AGENTS.md](AGENTS.md)

Current Zed-specific UI caveats are tracked in [docs/upstream-feature-matrix.md](docs/upstream-feature-matrix.md), especially around approval-card layout and command/review/session UX that the adapter alone cannot fully control.

## Roadmap

Near-term work:

- Keep refining the `Context` selector and `/status` report where it helps daily use
- Decide the next surfaced preview flow after status, most likely `thread/read` or `/diff`

Later candidates:

- `/diff`
- `/debug-config`
- `thread/read`

Not a priority for this fork right now:

- `close_session` as a user-visible focus area in current Zed
- `/logout`
- `fs/watch`
- app-server feature flags plumbing
- `codex_home` surfacing
- remote auth through client

## License

Apache-2.0. See [LICENSE](LICENSE).
