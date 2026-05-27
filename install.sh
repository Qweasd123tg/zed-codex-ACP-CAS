#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_REPO_URL="https://github.com/Qweasd123tg/zed-codex-ACP-CAS"

usage() {
  cat <<'EOF'
Usage:
  ./install.sh [options] [install_dir] [binary_name]

Options:
  --download <version>      Download a GitHub Release binary. Use "latest" or a version/tag.
  --from-binary <path>      Install an existing release/downloaded binary instead of building.
  --sha256 <path>           Verify the source binary against a sha256 file before installing.
  --repo-url <url>          GitHub repository URL for --download (default: project origin).
  --config-home <path>      Create this adapter config directory (default: CODEX_CAS_HOME or ~/.codex-cas).
  --no-init-config          Do not create the adapter config directory.
  --with-checks             Run source checks before building from source.
  --checks-mode <mode>      Checks mode when --with-checks is enabled: quick|full (default: quick).
  --no-smoke-test           Skip pre-activation smoke test.
  -h, --help                Show this help.

Defaults:
  install_dir  = $HOME/.local/bin
  binary_name  = codex-acp (codex-acp.exe on Windows)

Examples:
  ./install.sh
  ./install.sh --download latest
  ./install.sh --download 0.23.5
  ./install.sh --from-binary ./codex-acp-linux-x86_64-gnu --sha256 ./codex-acp-linux-x86_64-gnu.sha256
  ./install.sh "$HOME/bin" codex-acp
EOF
}

compute_sha256() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    echo "No sha256 tool found (expected sha256sum or shasum)." >&2
    return 1
  fi
}

download_file() {
  local url="$1"
  local output="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 --connect-timeout 15 -o "$output" "$url"
  elif command -v wget >/dev/null 2>&1; then
    wget -O "$output" "$url"
  else
    echo "No download tool found (expected curl or wget)." >&2
    return 1
  fi
}

verify_sha256() {
  local binary="$1"
  local sha_file="$2"
  if [[ ! -f "$sha_file" ]]; then
    echo "sha256 file not found: $sha_file" >&2
    exit 1
  fi

  local expected
  expected="$(awk 'NF {print $1; exit}' "$sha_file")"
  if [[ ! "$expected" =~ ^[0-9A-Fa-f]{64}$ ]]; then
    echo "Invalid sha256 file: $sha_file" >&2
    exit 1
  fi

  local actual
  actual="$(compute_sha256 "$binary")"
  local expected_lower
  expected_lower="$(printf '%s' "$expected" | tr '[:upper:]' '[:lower:]')"
  if [[ "$expected_lower" != "$actual" ]]; then
    echo "sha256 mismatch for $binary" >&2
    echo "expected: $expected_lower" >&2
    echo "actual:   $actual" >&2
    exit 1
  fi
}

smoke_test_binary() {
  local binary="$1"
  echo "[install] smoke test: $binary --help"
  local output
  local status
  set +e
  output="$("$binary" --help 2>&1)"
  status=$?
  set -e

  if [[ $status -ne 0 ]]; then
    echo "[install] --help failed with status $status" >&2
    echo "$output" >&2
    exit 1
  fi

  if ! grep -q "Usage:" <<<"$output" || ! grep -q -- "--config" <<<"$output"; then
    echo "[install] unexpected help output" >&2
    echo "$output" >&2
    exit 1
  fi
}

host_os() {
  local os
  os="$(uname -s)"
  case "$os" in
    Linux) echo "linux" ;;
    Darwin) echo "darwin" ;;
    MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
    *)
      echo "Unsupported OS: $os" >&2
      return 1
      ;;
  esac
}

host_arch() {
  local arch
  arch="$(uname -m)"
  case "$arch" in
    x86_64|amd64) echo "x64" ;;
    arm64|aarch64) echo "arm64" ;;
    *)
      echo "Unsupported CPU architecture: $arch" >&2
      return 1
      ;;
  esac
}

release_asset_name() {
  local os="$1"
  local arch="$2"
  case "$os-$arch" in
    linux-x64) echo "codex-acp-linux-x86_64-gnu" ;;
    darwin-arm64) echo "codex-acp-macos-aarch64-apple-darwin" ;;
    windows-x64) echo "codex-acp-windows-x86_64-pc-windows-msvc.exe" ;;
    *)
      echo "No release artifact is published for $os-$arch." >&2
      return 1
      ;;
  esac
}

default_binary_name() {
  if [[ "$(host_os)" == "windows" ]]; then
    echo "codex-acp.exe"
  else
    echo "codex-acp"
  fi
}

release_base_url() {
  local repo_url="$1"
  local version="$2"
  repo_url="${repo_url%/}"
  if [[ "$version" == "latest" ]]; then
    echo "$repo_url/releases/latest/download"
    return
  fi

  if [[ "$version" == v* ]]; then
    echo "$repo_url/releases/download/$version"
  else
    echo "$repo_url/releases/download/v$version"
  fi
}

default_config_home() {
  if [[ -n "${CODEX_CAS_HOME:-}" ]]; then
    echo "$CODEX_CAS_HOME"
    return
  fi
  if [[ -n "${HOME:-}" ]]; then
    echo "$HOME/.codex-cas"
    return
  fi
  echo "HOME is not set; pass --config-home or --no-init-config." >&2
  return 1
}

RUN_CHECKS=0
CHECKS_MODE="quick"
RUN_SMOKE_TEST=1
INIT_CONFIG=1
FROM_BINARY=""
SHA256_FILE=""
DOWNLOAD_VERSION=""
REPO_URL="$DEFAULT_REPO_URL"
CONFIG_HOME=""
POSITIONAL_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --download)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --download" >&2
        usage >&2
        exit 2
      fi
      DOWNLOAD_VERSION="$2"
      shift 2
      ;;
    --from-binary|--from)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for $1" >&2
        usage >&2
        exit 2
      fi
      FROM_BINARY="$2"
      shift 2
      ;;
    --sha256)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --sha256" >&2
        usage >&2
        exit 2
      fi
      SHA256_FILE="$2"
      shift 2
      ;;
    --repo-url)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --repo-url" >&2
        usage >&2
        exit 2
      fi
      REPO_URL="$2"
      shift 2
      ;;
    --config-home)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --config-home" >&2
        usage >&2
        exit 2
      fi
      CONFIG_HOME="$2"
      shift 2
      ;;
    --no-init-config)
      INIT_CONFIG=0
      shift
      ;;
    --with-checks)
      RUN_CHECKS=1
      shift
      ;;
    --checks-mode)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --checks-mode" >&2
        usage >&2
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
      usage >&2
      exit 2
      ;;
    *)
      POSITIONAL_ARGS+=("$1")
      shift
      ;;
  esac
done

if [[ ${#POSITIONAL_ARGS[@]} -gt 2 ]]; then
  usage >&2
  exit 2
fi

if [[ "$CHECKS_MODE" != "quick" && "$CHECKS_MODE" != "full" ]]; then
  echo "Unsupported --checks-mode: $CHECKS_MODE (expected quick|full)" >&2
  exit 2
fi

if [[ -n "$FROM_BINARY" && -n "$DOWNLOAD_VERSION" ]]; then
  echo "--from-binary and --download are mutually exclusive." >&2
  exit 2
fi

if [[ -n "$DOWNLOAD_VERSION" && -n "$SHA256_FILE" ]]; then
  echo "--sha256 is only valid with --from-binary; --download fetches the matching sha256 file." >&2
  exit 2
fi

if [[ -n "$FROM_BINARY" && "$RUN_CHECKS" == "1" ]] || [[ -n "$DOWNLOAD_VERSION" && "$RUN_CHECKS" == "1" ]]; then
  echo "--with-checks is only valid for source installs." >&2
  exit 2
fi

if [[ -n "$SHA256_FILE" && -z "$FROM_BINARY" ]]; then
  echo "--sha256 requires --from-binary." >&2
  exit 2
fi

if [[ -z "$FROM_BINARY" && -z "$DOWNLOAD_VERSION" && ! -f "$ROOT_DIR/Cargo.toml" ]]; then
  DOWNLOAD_VERSION="latest"
fi

DEFAULT_INSTALL_DIR="${HOME:-}/.local/bin"
if [[ "$DEFAULT_INSTALL_DIR" == "/.local/bin" && ${#POSITIONAL_ARGS[@]} -lt 1 ]]; then
  echo "HOME is not set; pass an explicit install_dir." >&2
  exit 2
fi

INSTALL_DIR="${POSITIONAL_ARGS[0]:-$DEFAULT_INSTALL_DIR}"
BINARY_NAME="${POSITIONAL_ARGS[1]:-$(default_binary_name)}"
TARGET_BIN="$INSTALL_DIR/$BINARY_NAME"
TARGET_INFO="$INSTALL_DIR/${BINARY_NAME}.build-info.txt"

INSTALL_MODE="source"
SOURCE_BIN=""
SOURCE_SHA_FILE=""
VERSION="unknown"
COMMIT="unknown"
GIT_DIRTY="unknown"
DOWNLOAD_URL=""
RUSTC_VERSION="$(rustc --version 2>/dev/null || echo unknown)"
DOWNLOAD_DIR=""

cleanup_download() {
  [[ -z "$DOWNLOAD_DIR" ]] || rm -rf "$DOWNLOAD_DIR"
}
trap cleanup_download EXIT

if [[ -n "$DOWNLOAD_VERSION" ]]; then
  INSTALL_MODE="download"
  os="$(host_os)"
  arch="$(host_arch)"
  asset="$(release_asset_name "$os" "$arch")"
  base_url="$(release_base_url "$REPO_URL" "$DOWNLOAD_VERSION")"
  DOWNLOAD_URL="$base_url/$asset"
  DOWNLOAD_DIR="$(mktemp -d)"
  SOURCE_BIN="$DOWNLOAD_DIR/$asset"
  SOURCE_SHA_FILE="$DOWNLOAD_DIR/$asset.sha256"

  echo "[install] downloading $DOWNLOAD_URL"
  download_file "$DOWNLOAD_URL" "$SOURCE_BIN"
  echo "[install] downloading $DOWNLOAD_URL.sha256"
  download_file "$DOWNLOAD_URL.sha256" "$SOURCE_SHA_FILE"
  echo "[install] verifying sha256"
  verify_sha256 "$SOURCE_BIN" "$SOURCE_SHA_FILE"

  if [[ "$DOWNLOAD_VERSION" != "latest" ]]; then
    VERSION="${DOWNLOAD_VERSION#v}"
  fi
elif [[ -n "$FROM_BINARY" ]]; then
  INSTALL_MODE="binary"
  SOURCE_BIN="$FROM_BINARY"
  if [[ ! -f "$SOURCE_BIN" ]]; then
    echo "Source binary not found: $SOURCE_BIN" >&2
    exit 1
  fi
  if [[ -n "$SHA256_FILE" ]]; then
    echo "[install] verifying sha256"
    verify_sha256 "$SOURCE_BIN" "$SHA256_FILE"
  fi
else
  cd "$ROOT_DIR"
  if [[ "$RUN_CHECKS" == "1" ]]; then
    echo "[install] running checks: $CHECKS_MODE"
    bash script/run_live_checks.sh "$CHECKS_MODE"
  fi
  echo "[install] cargo build --release"
  cargo build --release
  SOURCE_BIN="$ROOT_DIR/target/release/codex-acp"
  VERSION="$(awk -F'"' '/^version = / {print $2; exit}' Cargo.toml)"
  COMMIT="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
  if [[ -n "$(git status --porcelain 2>/dev/null || true)" ]]; then
    GIT_DIRTY=1
  else
    GIT_DIRTY=0
  fi
fi

if [[ ! -f "$SOURCE_BIN" ]]; then
  echo "Built/source binary not found: $SOURCE_BIN" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"

TMP_BIN=""
TMP_INFO=""
cleanup_install() {
  [[ -z "$TMP_BIN" ]] || rm -f "$TMP_BIN"
  [[ -z "$TMP_INFO" ]] || rm -f "$TMP_INFO"
  cleanup_download
}
trap cleanup_install EXIT

TMP_BIN="$(mktemp "$INSTALL_DIR/.${BINARY_NAME}.tmp.XXXXXX")"
install -m 0755 "$SOURCE_BIN" "$TMP_BIN"

if [[ "$RUN_SMOKE_TEST" == "1" ]]; then
  smoke_test_binary "$TMP_BIN"
fi

mv -f "$TMP_BIN" "$TARGET_BIN"
TMP_BIN=""

if [[ "$INIT_CONFIG" == "1" ]]; then
  if [[ -z "$CONFIG_HOME" ]]; then
    CONFIG_HOME="$(default_config_home)"
  fi
  mkdir -p "$CONFIG_HOME"
fi

SHA256="$(compute_sha256 "$TARGET_BIN")"
BUILT_AT_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
TMP_INFO="$(mktemp "$INSTALL_DIR/.${BINARY_NAME}.build-info.tmp.XXXXXX")"
cat >"$TMP_INFO" <<EOF
binary=$TARGET_BIN
install_mode=$INSTALL_MODE
source_binary=$SOURCE_BIN
download_url=$DOWNLOAD_URL
version=$VERSION
commit=$COMMIT
git_dirty=$GIT_DIRTY
sha256=$SHA256
checks_mode=$CHECKS_MODE
smoke_test=$RUN_SMOKE_TEST
config_home=$CONFIG_HOME
rustc=$RUSTC_VERSION
built_at_utc=$BUILT_AT_UTC
source=$ROOT_DIR
EOF
mv -f "$TMP_INFO" "$TARGET_INFO"
TMP_INFO=""

echo "[done] installed: $TARGET_BIN"
echo "[done] build info: $TARGET_INFO"
if [[ "$INIT_CONFIG" == "1" ]]; then
  echo "[done] config home: $CONFIG_HOME"
  echo "[note] selector-preferences.json and display-maps.json are created by the adapter on first session startup."
fi
echo
echo "Zed settings snippet:"
cat <<EOF
{
  "agent_servers": {
    "codex-acp-cas": {
      "type": "custom",
      "command": "$TARGET_BIN"
    }
  }
}
EOF
