#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

usage() {
  cat <<'EOF'
Usage:
  script/prepare_release.sh <version> [options]

Options:
  --checks-mode <mode>   quick|full|none (default: quick)
  --no-tag               Do not create git tag.
  --no-build             Do not build release bundle.
  --allow-dirty          Allow running with uncommitted changes (not recommended).
  --release-dir <dir>    Output directory for release artifacts (default: .releases)
  -h, --help             Show this help.

Examples:
  script/prepare_release.sh 0.1.0
  script/prepare_release.sh 0.1.1 --checks-mode full
  script/prepare_release.sh 0.2.0-rc.1 --no-build
EOF
}

VERSION=""
CHECKS_MODE="quick"
CREATE_TAG=1
RUN_BUILD=1
ALLOW_DIRTY=0
RELEASE_DIR="$ROOT_DIR/.releases"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --checks-mode)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --checks-mode" >&2
        exit 2
      fi
      CHECKS_MODE="$2"
      shift 2
      ;;
    --no-tag)
      CREATE_TAG=0
      shift
      ;;
    --no-build)
      RUN_BUILD=0
      shift
      ;;
    --allow-dirty)
      ALLOW_DIRTY=1
      shift
      ;;
    --release-dir)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --release-dir" >&2
        exit 2
      fi
      RELEASE_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      echo "Unknown argument: $1" >&2
      usage
      exit 2
      ;;
    *)
      if [[ -z "$VERSION" ]]; then
        VERSION="$1"
        shift
      else
        echo "Unexpected extra positional argument: $1" >&2
        usage
        exit 2
      fi
      ;;
  esac
done

if [[ -z "$VERSION" ]]; then
  usage
  exit 2
fi

if [[ ! "$CHECKS_MODE" =~ ^(quick|full|none)$ ]]; then
  echo "Unsupported checks mode: $CHECKS_MODE (expected quick|full|none)" >&2
  exit 2
fi

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
  echo "Invalid version '$VERSION' (expected semver-like, e.g. 1.2.3 or 1.2.3-rc.1)" >&2
  exit 2
fi

TAG="v$VERSION"

if [[ "$ALLOW_DIRTY" != "1" ]] && [[ -n "$(git status --porcelain 2>/dev/null || true)" ]]; then
  echo "Working tree is not clean. Commit/stash changes or pass --allow-dirty." >&2
  exit 1
fi

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  echo "Tag already exists: $TAG" >&2
  exit 1
fi

PREVIOUS_TAG="$(git tag --list 'v*' --sort=-creatordate | head -n 1 || true)"
CARGO_VERSION="$(awk -F'"' '/^version = / {print $2; exit}' Cargo.toml)"

if [[ "$CARGO_VERSION" != "$VERSION" ]]; then
  echo "[release] update Cargo.toml version: $CARGO_VERSION -> $VERSION"
  sed -i.bak -E "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml
  rm -f Cargo.toml.bak
fi

if [[ "$CHECKS_MODE" != "none" ]]; then
  echo "[release] running checks: $CHECKS_MODE"
  bash script/run_live_checks.sh "$CHECKS_MODE"
fi

git add Cargo.toml
if ! git diff --cached --quiet; then
  git commit -m "Release $TAG"
else
  echo "[release] version files unchanged, skipping release commit"
fi

if [[ "$CREATE_TAG" == "1" ]]; then
  git tag -a "$TAG" -m "Release $TAG"
  echo "[release] created tag: $TAG"
fi

if [[ "$RUN_BUILD" == "1" ]]; then
  echo "[release] cargo build --release"
  cargo build --release

  PLATFORM="$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)"
  OUT_DIR="$RELEASE_DIR/$TAG"
  mkdir -p "$OUT_DIR"

  OUT_BIN="$OUT_DIR/codex-acp-$VERSION-$PLATFORM"
  cp target/release/codex-acp "$OUT_BIN"
  chmod 0755 "$OUT_BIN"
  sha256sum "$OUT_BIN" > "$OUT_BIN.sha256"

  COMMIT="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
  BUILT_AT_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

  cat >"$OUT_DIR/release-manifest.txt" <<EOF
version=$VERSION
tag=$TAG
commit=$COMMIT
built_at_utc=$BUILT_AT_UTC
platform=$PLATFORM
binary=$OUT_BIN
source=$ROOT_DIR
previous_tag=$PREVIOUS_TAG
checks_mode=$CHECKS_MODE
EOF

  {
    echo "# Release $TAG"
    echo
    echo "Version: \`$VERSION\`"
    echo "Commit: \`$COMMIT\`"
    echo "Built at: \`$BUILT_AT_UTC\`"
    echo
    echo "## Changes"
    if [[ -n "$PREVIOUS_TAG" ]]; then
      git log --oneline --no-decorate "$PREVIOUS_TAG..HEAD" | sed 's/^/- /'
    else
      git log --oneline --no-decorate -n 30 | sed 's/^/- /'
    fi
  } >"$OUT_DIR/release-notes.md"

  echo "[release] artifacts:"
  echo "  - $OUT_BIN"
  echo "  - $OUT_BIN.sha256"
  echo "  - $OUT_DIR/release-manifest.txt"
  echo "  - $OUT_DIR/release-notes.md"
fi

echo
echo "[release] done"
echo "[release] next:"
echo "  git show --stat HEAD"
if [[ "$CREATE_TAG" == "1" ]]; then
  echo "  git show --no-patch $TAG"
  echo "  git push origin main"
  echo "  git push origin $TAG"
else
  echo "  git push origin main"
fi
