#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  script/smoke_test_cas.sh <path-to-codex-acp-binary>
EOF
}

if [[ $# -ne 1 ]]; then
  usage
  exit 2
fi

BINARY="$1"

if [[ ! -f "$BINARY" ]]; then
  echo "[smoke] binary not found: $BINARY" >&2
  exit 1
fi

if [[ ! -x "$BINARY" ]]; then
  echo "[smoke] binary is not executable: $BINARY" >&2
  exit 1
fi

echo "[smoke] running: $BINARY --help"
set +e
HELP_OUTPUT="$("$BINARY" --help 2>&1)"
HELP_STATUS=$?
set -e

if [[ $HELP_STATUS -ne 0 ]]; then
  echo "[smoke] --help failed with status $HELP_STATUS" >&2
  echo "$HELP_OUTPUT" >&2
  exit 1
fi

if ! grep -q "Usage: codex-acp" <<<"$HELP_OUTPUT"; then
  echo "[smoke] unexpected help output (missing 'Usage: codex-acp')" >&2
  echo "$HELP_OUTPUT" >&2
  exit 1
fi

echo "[smoke] ok"
