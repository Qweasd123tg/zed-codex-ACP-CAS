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
  - Linux packages: `.deb` and `.rpm`
- Behavior may still change between releases

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
  - `/archive`
  - `/unarchive`
  - `/rename`
  - `/compact`
  - `/undo`
  - `/plan`
- Better thread title handling for resume/archive/rename flows
- Tool call cards for command, MCP, web, image, file, collab, and dynamic tool branches
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
- Practical plan mode support
- More complete collab and sub-agent UI mapping

Current gaps:

- No full structured elicitation parity yet
- `DynamicToolCall` support is still partial
- Some upstream-style flows are still missing or incomplete, including `close_session`, `/init`, `/logout`, and review-oriented flows
- Some behavior still depends on Zed-side ACP support

## Limitations

- MCP passthrough supports `stdio` and `http` today
- MCP `sse` passthrough is not supported yet
- `DynamicToolCall` is only partially supported
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
- `.deb` for Debian and Ubuntu
- `.rpm` for Fedora and similar RPM-based systems

### Build From Source

Requirements:

- Rust toolchain
- `codex` available in your environment

Build:

```bash
cargo build --release
```

Run:

```bash
./target/release/codex-acp --help
```

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

- Finish startup request multiplexing cleanup
- Improve release automation and packaging
- Test release artifacts on Windows and macOS
- Keep reducing ACP and Zed session lifecycle edge cases
- Keep simplifying docs for non-maintainer users

## License

Apache-2.0. See [LICENSE](LICENSE).
