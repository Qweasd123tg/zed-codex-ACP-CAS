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
  - copies the fresh binary into `.build/codex-acp-current` for checkout-local dev use
  - writes `.build/codex-acp-current.build-info.txt`
  - use `./install.sh` for the canonical local install path
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

echo "[build] cargo build --release"
cargo build --release

ARTIFACTS_DIR="$ROOT_DIR/.build"
SOURCE_BIN="$ROOT_DIR/target/release/codex-acp"
CURRENT_BIN="$ARTIFACTS_DIR/codex-acp-current"
CURRENT_INFO="$ARTIFACTS_DIR/codex-acp-current.build-info.txt"

mkdir -p "$ARTIFACTS_DIR"

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
echo "[done] build info: $CURRENT_INFO"
