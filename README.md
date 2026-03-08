# codex-acp

`codex-acp` is a Rust adapter that bridges `codex app-server` to ACP-compatible clients such as Zed.

It exposes Codex session lifecycle, turn streaming, approvals, replay, and tool-call UI through the Agent Client Protocol without reimplementing the Codex runtime itself.

## Status

- Beta-quality project under active development.
- Maintained and tested primarily on Fedora x86_64.
- Supported release target: `x86_64-unknown-linux-gnu`.
- The binary name is `codex-acp`.

## Implemented capabilities

- ACP prompt capabilities: `embedded_context`, `image`.
- Session lifecycle: `new_session`, `load_session`, `resume_session`, `list_sessions`.
- Authentication methods:
  - ChatGPT login flow
  - `CODEX_API_KEY`
  - `OPENAI_API_KEY`
- Streaming session updates:
  - user text
  - assistant text
  - assistant reasoning/thought chunks
  - plan updates
  - usage/context-window updates
- Tool-call cards and replay support for:
  - shell/command execution
  - file changes and diffs
  - MCP tool calls
  - web search
  - image view
  - collab/sub-agent events
- Runtime session controls:
  - approval preset / mode
  - model selection
  - reasoning effort
  - context compaction
  - rollback / undo
  - active turn cancellation

## Session semantics

- `load_session` resumes a saved thread and replays its history into the ACP client.
- `resume_session` resumes a saved thread without replaying prior history.
- `/resume` switches threads inside an active ACP session and replays history by default.
- `/resume --no-history` switches threads without replaying prior messages into the current chat UI.
- `session/list` defaults to the current workspace when the ACP client does not send `cwd`, which matches the intended CLI-style resume flow.

## Slash commands

- `/threads`
- `/resume [partial_id] [--no-history]`
- `/compact`
- `/undo [N]`
- `/reasoning [none|minimal|low|medium|high|xhigh]`
- `/effort ...`
- `/plan`
- `/plan on|off`
- `/plan <request>`
- `/context`

## Known limitations

- ACP `mcp_servers` configuration is currently accepted but not forwarded to `codex app-server` in app-server mode.
- Some app-server server requests are intentionally rejected because there is no supported ACP-side handling path yet, including:
  - `item/tool/call`
  - `account/chatgptAuthTokens/refresh`
  - `applyPatchApproval`
  - `execCommandApproval`
- Prompt conversion ignores audio and non-text embedded resources.
- `request_user_input` is option-based only and does not support free-form text entry.
- The adapter currently spawns `codex` from `PATH`.

## Zed rewind / edit support

Server-side rollback support exists in `codex-acp`, but the pencil/edit UX in Zed depends on client support for ACP truncate handling.

Required Zed-side behavior:

- ACP connection support for `truncate`
- an `ext_method("zed.dev/codex/thread/rollback", { sessionId, numTurns, replayHistory })` call

Without that client-side support, older or unpatched Zed builds may show the edit action as unavailable. The fallback path is `/undo N` followed by a new prompt.

## Getting started

Run locally from the repository root:

```bash
cargo run -- --help
```

After building:

```bash
./target/release/codex-acp --help
```

## Build, test, and local scripts

Core commands:

```bash
cargo build
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

Release-target validation:

```bash
cargo test --release --target x86_64-unknown-linux-gnu
```

Useful local scripts:

```bash
bash script/run_live_checks.sh quick
bash script/run_live_checks.sh full
bash script/build_install_cas.sh
bash script/smoke_test_cas.sh "$HOME/.local/bin/codex-acp-cas"
bash script/export_public_snapshot.sh --init-git /tmp/codex-acp-public
```

Reference refresh helpers:

```bash
bash script/update_references.sh
bash script/update_references.sh --daily
bash script/update_references.sh --repo zed
```

## Public snapshot export

If you want to publish a clean GitHub repository without rewriting or deleting the history of your local working repository, export a separate snapshot:

```bash
bash script/export_public_snapshot.sh --init-git /tmp/codex-acp-public
```

This copies the current working tree into a new directory, excludes local-only artifacts such as `.git`, `target/`, `target-test/`, `.releases/`, `references/`, `dist/`, and `excalidraw.log`, and optionally initializes a fresh `main` branch there.

Your current repository and its existing commit history remain untouched.

## Release workflow

Prepare a release:

```bash
bash script/prepare_release.sh 0.1.0
git push origin main
git push origin v0.1.0
```

GitHub Actions builds and publishes a Linux release archive for `x86_64-unknown-linux-gnu` together with a `.sha256` checksum.

## Repository layout

- `src/main.rs`: binary entrypoint
- `src/lib.rs`: runtime bootstrap and ACP connection startup
- `src/codex_agent.rs`: ACP `Agent` implementation
- `src/app_server.rs`: JSON-RPC bridge to `codex app-server`
- `src/thread.rs`: top-level thread orchestration and shared state
- `src/thread/core/*`: low-level routing, replay, protocol helpers, terminal updates
- `src/thread/features/*`: domain features such as approvals, plan, resume, session, tool events, collab, and file handling
- `src/thread/{prompt,notification,session,turn}/*`: runtime pipelines for prompt parsing, notification dispatch, session state, and turn execution
- `docs/thread-feature-map.md`: architecture map of the thread subsystem

## Additional documentation

- Architecture map: `docs/thread-feature-map.md`
- Contributor guide: `AGENTS.md`

## License

Apache-2.0. See `LICENSE`.
