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
- `soft /new` and in-place `/fork` support for switching backend threads inside one ACP session
- Tool call cards for command, MCP, web, image, file, and collab branches
- Practical plan mode support
- Better startup and reconnect diagnostics

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
- Better thread titles in lists and resumed sessions
- Practical in-place thread switching with `/new`, `/fork`, `/resume`, and archive-triggered replacement
- Practical plan mode support
- More complete collab and sub-agent UI mapping

Current gaps:

- No full structured elicitation parity yet
- Manual `Plan mode` is usable, but it is not an exact match for Codex CLI `update_plan` autoplan rendering; think of it as a CLI-like collaboration flow rather than the same UI contract
- `DynamicToolCall` is intentionally unsupported in runtime code for now; the old partial implementation was removed and summarized in `docs/drafts/dynamic-tool-call-backup.md`
- Some upstream-style flows are still missing or incomplete, including `close_session`, `/init`, `/logout`, and review-oriented flows
- `soft /new` and `/fork` switch only the backend thread; current Zed-side ACP behavior still does not clear sidebar chat history for in-place thread switches
- Some behavior still depends on Zed-side ACP support

## Limitations

- MCP passthrough supports `stdio` and `http` today
- MCP `sse` passthrough is not supported yet
- `item/tool/call` / `DynamicToolCall` requests are rejected as unsupported
- Zed rewind/edit support still depends on a client-side ACP fix for rollback wiring
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

What they usually mean:

- Timeout during `initialize` or `thread/start`: startup path is stuck
- `failed to start 'codex' app-server`: `codex` is missing or not available in `PATH`
- Panic backtrace: the adapter or child process crashed directly

## More Docs

User-facing documentation stays in this README. Deeper project notes are kept separately:

- [docs/upstream-feature-matrix.md](docs/upstream-feature-matrix.md)
- [docs/thread-feature-map.md](docs/thread-feature-map.md)
- [AGENTS.md](AGENTS.md)

## Roadmap

Near-term work:

- Add real review-oriented flows instead of replay-only review residue
- Surface a clearer `status` view, likely in a selector or lightweight slash command
- Improve command approval UX so the user can see the actual shell command or a clear preview before approving
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
