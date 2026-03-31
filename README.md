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
  - `resume_session`
  - `list_sessions`
- Session-scoped MCP passthrough for `stdio` and `http`
- History replay after `load_session` and `resume_session`
- Session commands:
  - `/review`
  - `/threads`
  - `/resume`
  - `/new`
  - `/fork`
  - `/archive`
  - `/unarchive`
  - `/rename`
  - `/compact`
  - `/undo`
  - `/plan`
- Better thread title handling for resume/archive/rename/fork flows
- Inline review flows for uncommitted changes, base branches, and specific commits, centered on one ACP picker behind `/review`
- `soft /new` and in-place `/fork` support for switching backend threads inside one ACP session
- Tool call cards for command, MCP, web, image, file, and collab branches
- Practical plan mode support
- Better startup and reconnect diagnostics
- Safer turn-start timeout and stale turn-tail cleanup around reconnects
- Safer history replay fencing for `/undo` and auto-restored session history
- Less UI freeze risk during `/resume --history` by replaying restored history outside the main session mutex
- Less duplicate file-change I/O when one patch item touches the same path multiple times
- Less mutex hold time while waiting for file-change approval prompts
- Less chat stall while command approval prompts are pending
- Faster file-change start cards with ACP snapshot priming moved out of the main session mutex
- Less mutex hold time while final file-change diff and ACP writeback are published
- Safer transport drain: stale server requests are rejected during post-turn and pre-prompt cleanup instead of triggering late approvals
- Less reconnect spam: reconnect warnings now collapse into one normalized status line while reconnect-assisted stalled turns still abort cleanly
- Less brittle transport cleanup: background drain and thread-switch flush now wait for the queue to go quiet instead of assuming `64` messages or one tiny timeout is enough
- Less turn-completion lock contention: turn diff ACP writeback now runs outside the main session mutex and skips paths already reserved by file-change lifecycle

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
- Better thread titles in lists and resumed sessions
- Inline review flows backed by native `review/start`
- Practical in-place thread switching with `/new`, `/fork`, `/resume`, and archive-triggered replacement
- Practical plan mode support
- More complete collab and sub-agent UI mapping

Current gaps:

- No full structured elicitation parity yet
- Manual `Plan mode` is usable, but it is not an exact match for Codex CLI `update_plan` autoplan rendering; think of it as a CLI-like collaboration flow rather than the same UI contract
- `DynamicToolCall` is intentionally unsupported in runtime code for now; the old partial implementation was removed and summarized in `docs/drafts/dynamic-tool-call-backup.md`
- Some upstream-style flows are still missing or incomplete, including `close_session`, `/init`, and `/logout`
- `soft /new` and `/fork` switch only the backend thread; current Zed-side ACP behavior still does not clear sidebar chat history for in-place thread switches
- Some behavior still depends on Zed-side ACP support

## Limitations

- MCP passthrough supports `stdio` and `http` today
- MCP `sse` passthrough is not supported yet
- `item/tool/call` / `DynamicToolCall` requests are rejected as unsupported
- `/undo` itself works, but the visual rewind/edit button in current `Zed` still depends on a client-side ACP fix for rollback wiring; in practice that means patching or rebuilding `Zed` if you want the native button UX
- While history replay is restoring after `load_session` or replaying `/undo`, new prompts and session commands are intentionally fenced until replay finishes; this avoids overlapping turn/replay state in one ACP session
- Linux is the most tested platform right now
- Multi-platform release artifacts can exist before all platforms are equally tested in real use

## Install

### From GitHub Releases

Download the artifact for your platform from the releases page.

Planned release artifacts:

- `.tar.gz` for Linux
- `.tar.gz` for macOS Apple Silicon
- `.zip` for Windows

Extract the archive, place `codex-acp` somewhere on your `PATH`, and point Zed at that binary.

Example:

```bash
mkdir -p "$HOME/.local/bin"
tar -xzf codex-acp-cas-<version>-x86_64-unknown-linux-gnu.tar.gz
mv codex-acp "$HOME/.local/bin/codex-acp"
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
- `ACP_DISABLE_AUTO_RESTORE=1`
- `CODEX_ACP_STARTUP_TIMEOUT_MS=<milliseconds>`
- `CODEX_ACP_STARTUP_METADATA_TIMEOUT_MS=<milliseconds>`

`CODEX_ACP_STARTUP_TIMEOUT_MS` now also bounds the `turn/start` handshake, so an app-server that stops responding before it returns a `turn_id` does not leave the ACP UI spinning forever.

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

- Surface a clearer `status` view, likely in a selector or lightweight slash command
- Keep expanding selector UX carefully where it helps daily use, especially around `status`, `MCP`, `skills`, and `plugins`

Later candidates:

- `/diff`
- `/debug-config`
- `/init`
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
