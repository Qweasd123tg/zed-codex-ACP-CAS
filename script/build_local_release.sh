#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

usage() {
  cat <<'EOF'
Usage:
  bash script/build_local_release.sh

Behavior:
  - builds `cargo build --release`
  - rotates `.build/codex-acp-current` -> `.build/codex-acp-previous`
  - copies the fresh binary into `.build/codex-acp-current`
  - writes matching build-info files for rollback/debugging
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

echo "[build] cargo build --release"
cargo build --release

ARTIFACTS_DIR="$ROOT_DIR/.build"
SOURCE_BIN="$ROOT_DIR/target/release/codex-acp"
CURRENT_BIN="$ARTIFACTS_DIR/codex-acp-current"
PREVIOUS_BIN="$ARTIFACTS_DIR/codex-acp-previous"
CURRENT_INFO="$ARTIFACTS_DIR/codex-acp-current.build-info.txt"
PREVIOUS_INFO="$ARTIFACTS_DIR/codex-acp-previous.build-info.txt"

mkdir -p "$ARTIFACTS_DIR"

if [[ -f "$CURRENT_BIN" ]]; then
  mv -f "$CURRENT_BIN" "$PREVIOUS_BIN"
fi

if [[ -f "$CURRENT_INFO" ]]; then
  mv -f "$CURRENT_INFO" "$PREVIOUS_INFO"
fi

install -m 0755 "$SOURCE_BIN" "$CURRENT_BIN"

VERSION="$(awk -F'"' '/^version = / {print $2; exit}' Cargo.toml)"
COMMIT="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
if [[ -n "$(git status --porcelain 2>/dev/null || true)" ]]; then
  GIT_DIRTY=1
else
  GIT_DIRTY=0
fi
SHA256="$(sha256sum "$CURRENT_BIN" | awk '{print $1}')"
RUSTC_VERSION="$(rustc --version 2>/dev/null || echo unknown)"
BUILT_AT_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

cat >"$CURRENT_INFO" <<EOF
binary=$CURRENT_BIN
version=$VERSION
commit=$COMMIT
git_dirty=$GIT_DIRTY
sha256=$SHA256
rustc=$RUSTC_VERSION
built_at_utc=$BUILT_AT_UTC
source=$ROOT_DIR
EOF

echo "[done] current:  $CURRENT_BIN"
if [[ -f "$PREVIOUS_BIN" ]]; then
  echo "[done] previous: $PREVIOUS_BIN"
fi
echo "[done] build info: $CURRENT_INFO"
