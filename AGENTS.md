# Repository Guidelines

## Project structure

`src/` contains the Rust ACP adapter:

- `src/main.rs`: binary entrypoint.
- `src/lib.rs`: runtime bootstrap and ACP connection startup.
- `src/codex_agent.rs`: ACP `Agent` implementation (`initialize`, auth, session lifecycle, prompt handling).
- `src/app_server.rs`: thin JSON-RPC bridge to `codex app-server`.
- `src/prompt_args.rs`: prompt argument parsing helpers and parser tests.

`src/thread.rs` keeps orchestration and shared `ThreadInner` state. The implementation is intentionally split into focused submodules:

- `src/thread/core/*`: routing and glue (`item_handlers`, `replay`, `server_requests`, `inner_state`, `terminal_updates`).
- `src/thread/features/*`: domain slices (`approvals`, `collab`, `file`, `notification`, `plan`, `resume`, `session`, `tool_events`, `tool_call_ui`).
- `src/thread/prompt/*`: slash-command parsing and prompt flow.
- `src/thread/notification/*`: transport-level dispatch for incoming JSON-RPC messages.
- `src/thread/session/*`: session loading, config mapping, and view updates.
- `src/thread/turn/*`: turn execution, diff handling, and completion state.

Additional repository areas:

- `.github/workflows/`: CI and release automation.
- `script/`: local build, smoke-test, release-prep, and public-snapshot export helpers.
- `docs/thread-feature-map.md`: architecture map for the thread subsystem.

This fork is maintained primarily for Fedora-oriented Linux use, with `x86_64-unknown-linux-gnu` as the release target.

## Build and verification commands

Run from the repository root:

```bash
cargo build
cargo run -- --help
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

Validate against the supported release target:

```bash
cargo test --release --target x86_64-unknown-linux-gnu
```

## Release policy

For quick feature testing, build a release-style binary into `target-test`:

```bash
cargo build --release --target-dir target-test
```

Reserve the full `target/release` path and actual release workflow for final release preparation.

## Coding style

- Rust edition: `2024`.
- Formatting: `rustfmt`.
- Linting: `clippy -D warnings` in CI.
- Naming:
  - `snake_case` for modules, functions, and tests
  - `PascalCase` for types and traits
  - `UPPER_SNAKE_CASE` for constants
- Indentation: 4 spaces.
- Keep functions narrow in responsibility.

## Rules for `thread`-layer changes

1. Keep `notification/dispatch` and `core/server_requests` as thin routers.
2. Put domain logic in `features/*`, not back into root `thread.rs`.
3. For new lifecycle branches, preserve the symmetry `started -> completed -> replay`.
4. Keep `expected_turn_id` guards for turn-bound event handling.
5. After changing mode or config state, send `notify_config_update` or `notify_mode_and_config_update`.

## Testing expectations

Preferred style: unit tests next to the implementation via `#[cfg(test)]`.

Primary test locations:

- `src/thread/core/tests.rs`: main coverage for thread behavior.
- `src/prompt_args.rs`: prompt parser tests.
- local `#[cfg(test)]` modules in focused files such as `turn/state` and `core/protocol_contract`.

When changing parsing or protocol behavior, add both:

- happy-path scenarios
- edge and invalid scenarios

Before opening a PR, run `fmt`, `clippy`, and `test`.

## Commits and pull requests

- Keep commit subjects short and imperative.
- Existing history generally follows sentence case, optionally with `(#PR)`.

For PRs, include:

- what changed and why
- linked issue, if any
- exact verification commands and outcomes
- release or platform notes when `target` or release scripts are touched

## Security and configuration

- Never commit secrets or tokens.
- Use environment variables such as `OPENAI_API_KEY` and `CODEX_API_KEY` for local runs.
- Keep credentials outside the repository.
