#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

usage() {
  cat <<'EOF'
Usage:
  script/build_install_cas.sh [options] [install_dir] [binary_name]

Options:
  --with-checks             Run pre-install checks before build.
  --checks-mode <mode>      Checks mode when --with-checks is enabled: quick|full (default: quick).
  --no-smoke-test           Skip post-install smoke test.
  -h, --help                Show this help.

Defaults:
  install_dir  = $HOME/.local/bin
  binary_name  = codex-acp-cas

Examples:
  script/build_install_cas.sh
  script/build_install_cas.sh --with-checks --checks-mode quick
  script/build_install_cas.sh --with-checks --checks-mode full
  script/build_install_cas.sh --no-smoke-test
  script/build_install_cas.sh "$HOME/bin" codex-acp-cas
EOF
}

RUN_CHECKS=0
CHECKS_MODE="quick"
RUN_SMOKE_TEST=1
POSITIONAL_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --with-checks)
      RUN_CHECKS=1
      shift
      ;;
    --checks-mode)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --checks-mode" >&2
        usage
        exit 2
      fi
      CHECKS_MODE="$2"
      shift 2
      ;;
    --no-smoke-test)
      RUN_SMOKE_TEST=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      while [[ $# -gt 0 ]]; do
        POSITIONAL_ARGS+=("$1")
        shift
      done
      ;;
    -*)
      echo "Unknown option: $1" >&2
      usage
      exit 2
      ;;
    *)
      POSITIONAL_ARGS+=("$1")
      shift
      ;;
  esac
done

if [[ ${#POSITIONAL_ARGS[@]} -gt 2 ]]; then
  usage
  exit 2
fi

if [[ "$CHECKS_MODE" != "quick" && "$CHECKS_MODE" != "full" ]]; then
  echo "Unsupported --checks-mode: $CHECKS_MODE (expected quick|full)" >&2
  exit 2
fi

INSTALL_DIR="${POSITIONAL_ARGS[0]:-$HOME/.local/bin}"
BINARY_NAME="${POSITIONAL_ARGS[1]:-codex-acp-cas}"

if [[ "$RUN_CHECKS" == "1" ]]; then
  echo "[build] running checks: $CHECKS_MODE"
  bash script/run_live_checks.sh "$CHECKS_MODE"
fi

echo "[build] cargo build --release"
cargo build --release

SOURCE_BIN="$ROOT_DIR/target/release/codex-acp"
TARGET_BIN="$INSTALL_DIR/$BINARY_NAME"

mkdir -p "$INSTALL_DIR"
install -m 0755 "$SOURCE_BIN" "$TARGET_BIN"

if [[ "$RUN_SMOKE_TEST" == "1" ]]; then
  echo "[build] running post-install smoke test"
  bash script/smoke_test_cas.sh "$TARGET_BIN"
fi

VERSION="$(awk -F'"' '/^version = / {print $2; exit}' Cargo.toml)"
COMMIT="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
if [[ -n "$(git status --porcelain 2>/dev/null || true)" ]]; then
  GIT_DIRTY=1
else
  GIT_DIRTY=0
fi
SHA256="$(sha256sum "$TARGET_BIN" | awk '{print $1}')"
RUSTC_VERSION="$(rustc --version 2>/dev/null || echo unknown)"
BUILT_AT_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
cat >"$INSTALL_DIR/${BINARY_NAME}.build-info.txt" <<EOF
binary=$TARGET_BIN
version=$VERSION
commit=$COMMIT
git_dirty=$GIT_DIRTY
sha256=$SHA256
runcfg_checks_mode=$CHECKS_MODE
runcfg_smoke_test=$RUN_SMOKE_TEST
rustc=$RUSTC_VERSION
built_at_utc=$BUILT_AT_UTC
source=$ROOT_DIR
EOF

echo "[done] installed $TARGET_BIN"
echo "[done] build info: $INSTALL_DIR/${BINARY_NAME}.build-info.txt"
