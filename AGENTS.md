# Repository Guidelines

## Project Structure & Module Organization
`src/` contains the Rust ACP adapter:
- `src/main.rs`: binary entrypoint.
- `src/lib.rs`: startup wiring and ACP connection setup.
- `src/codex_agent.rs`: ACP `Agent` implementation (sessions/auth/capabilities).
- `src/app_server.rs`: JSON-RPC bridge to `codex app-server`.
- `src/thread.rs`, `src/prompt_args.rs`: thread/prompt logic and most unit tests.

`npm/` contains npm packaging and runtime wrapper:
- `npm/bin/codex-acp.js`: platform-aware launcher.
- `npm/testing/`: package validation and platform detection tests.
- `npm/publish/`, `npm/template/`: release packaging helpers/templates.

`.github/workflows/` defines CI (`ci.yml`, `quick-check.yml`) and release automation. `script/` contains signing scripts.

## Build, Test, and Development Commands
Use these from the repository root:

```bash
cargo build
cargo run -- --help
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
bash npm/testing/validate.sh
node npm/testing/test-platform-detection.js
```

`cargo test --release --target <triple>` mirrors CI target testing when needed.

## Coding Style & Naming Conventions
Rust uses edition 2024 with `rustfmt` and strict `clippy` (`-D warnings` in CI). Follow standard Rust naming:
- `snake_case` for modules/functions/tests.
- `PascalCase` for types/traits.
- `UPPER_SNAKE_CASE` for constants.

Use 4-space indentation and keep functions focused. For Node code in `npm/`, keep ESM style (`import ... from`) and clear `camelCase` helper names.

## Testing Guidelines
Prefer unit tests colocated with implementation via `#[cfg(test)]` (see `src/thread.rs` and `src/prompt_args.rs`). Name tests by behavior, e.g. `parses_resume_command_with_thread_id`.

When changing parsing/protocol logic, add both happy-path and invalid/edge-case tests. Run Rust checks and both npm validation scripts before opening a PR.

## Commit & Pull Request Guidelines
Commit subjects should be imperative and concise; current history follows sentence case with optional PR ref, e.g. `Consolidate event mapping into one place (#151)`.

PRs should include:
- What changed and why.
- Linked issue (if applicable).
- Exact verification commands run and results.
- Platform/release notes when touching `npm/` packaging, targets, or signing scripts.

## Security & Configuration Tips
Never commit credentials. Use environment variables like `OPENAI_API_KEY` or `CODEX_API_KEY` for local runs. Keep `Cargo.toml` and `npm/package.json` versions in sync for release-related changes.
