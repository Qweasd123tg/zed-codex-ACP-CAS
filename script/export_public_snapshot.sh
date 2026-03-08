#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST_DIR=""
INIT_GIT=0
FORCE=0

usage() {
  cat <<'EOF'
Usage:
  script/export_public_snapshot.sh [options] <destination_dir>

Options:
  --init-git     Initialize a fresh git repository in the exported directory.
  --force        Allow exporting into an existing non-empty directory.
  -h, --help     Show this help.

Examples:
  script/export_public_snapshot.sh /tmp/codex-acp-public
  script/export_public_snapshot.sh --init-git /tmp/codex-acp-public
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --init-git)
      INIT_GIT=1
      shift
      ;;
    --force)
      FORCE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      echo "Unknown option: $1" >&2
      usage
      exit 2
      ;;
    *)
      if [[ -n "$DEST_DIR" ]]; then
        echo "Unexpected extra argument: $1" >&2
        usage
        exit 2
      fi
      DEST_DIR="$1"
      shift
      ;;
  esac
done

if [[ -z "$DEST_DIR" ]]; then
  usage
  exit 2
fi

ROOT_ABS="$(realpath "$ROOT_DIR")"
DEST_ABS="$(realpath -m "$DEST_DIR")"
if [[ "$DEST_ABS" == "$ROOT_ABS" || "$DEST_ABS" == "$ROOT_ABS/"* ]]; then
  echo "Destination must be outside the source repository: $DEST_DIR" >&2
  exit 1
fi

mkdir -p "$DEST_DIR"
if [[ "$FORCE" != "1" ]] && find "$DEST_DIR" -mindepth 1 -maxdepth 1 -print -quit | grep -q .; then
  echo "Destination directory is not empty: $DEST_DIR" >&2
  echo "Pass --force to allow overwriting it." >&2
  exit 1
fi

echo "[export] syncing working tree to $DEST_DIR"
rsync -a --delete \
  --exclude '.git/' \
  --exclude 'target/' \
  --exclude 'target-test/' \
  --exclude '.releases/' \
  --exclude 'references/' \
  --exclude 'dist/' \
  --exclude 'excalidraw.log' \
  "$ROOT_DIR/" "$DEST_DIR/"

if [[ "$INIT_GIT" == "1" ]]; then
  if [[ -d "$DEST_DIR/.git" ]]; then
    echo "Destination already contains a git repository: $DEST_DIR/.git" >&2
    exit 1
  fi
  echo "[export] initializing fresh git repository"
  git -C "$DEST_DIR" init -b main >/dev/null
  git -C "$DEST_DIR" add .
  git -C "$DEST_DIR" commit -m "Initial public snapshot" >/dev/null
fi

echo "[export] done"
echo "[export] source repo history was not modified: $ROOT_DIR/.git stays untouched"
echo "[export] next steps:"
echo "  cd \"$DEST_DIR\""
if [[ "$INIT_GIT" == "1" ]]; then
  echo "  git remote add origin <your-github-url>"
  echo "  git push -u origin main"
else
  echo "  git init -b main"
  echo "  git add ."
  echo "  git commit -m \"Initial public snapshot\""
  echo "  git remote add origin <your-github-url>"
  echo "  git push -u origin main"
fi
