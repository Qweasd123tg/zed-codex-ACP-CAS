#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MODE="${1:-full}"

run_quick() {
  echo "[checks] cargo test replay_diff_for_"
  cargo test replay_diff_for_
  echo "[checks] cargo test parse_turn_unified_diff_files_handles_add_update_delete"
  cargo test parse_turn_unified_diff_files_handles_add_update_delete
}

run_full() {
  echo "[checks] cargo fmt --all -- --check"
  cargo fmt --all -- --check
  echo "[checks] cargo clippy --all-targets --all-features -- -D warnings"
  cargo clippy --all-targets --all-features -- -D warnings
  echo "[checks] cargo test"
  cargo test
}

case "$MODE" in
  quick)
    run_quick
    ;;
  full)
    run_full
    ;;
  *)
    echo "Usage: $0 [quick|full]" >&2
    exit 2
    ;;
esac
