#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REFERENCES_DIR="${REFERENCES_DIR:-$ROOT_DIR/references}"
STATE_FILE="${STATE_FILE:-$REFERENCES_DIR/.last-update}"

DAILY_MODE=0
DRY_RUN=0
ONLY_REPO=""

usage() {
  cat <<'EOF'
Usage:
  script/update_references.sh [options]

Options:
  --daily              Skip update when references were already updated today (UTC).
  --repo <name>        Update only one repo by base name (e.g. "zed", "codex-acp-upstream").
  --dry-run            Show planned actions without git fetch/pull/move.
  -h, --help           Show this help.

Behavior:
  - Finds git repos in references/ (top level).
  - Fetches/pulls latest changes from origin (ff-only).
  - Renames repo folder to include version: <base>@<version>.
  - Maintains stable symlink references/<base> -> <base>@<version>.

Examples:
  script/update_references.sh
  script/update_references.sh --daily
  script/update_references.sh --repo zed
EOF
}

log() {
  printf '%s\n' "$*"
}

run_cmd() {
  if [[ "$DRY_RUN" == "1" ]]; then
    log "[dry-run] $*"
    return 0
  fi
  "$@"
}

sanitize_version() {
  local raw="$1"
  local out
  out="$(printf '%s' "$raw" | tr '/ ' '__' | tr -cd '[:alnum:]._+-')"
  if [[ -z "$out" ]]; then
    out="unknown"
  fi
  printf '%s' "$out"
}

repo_base_name() {
  local name="$1"
  printf '%s' "$name" | sed -E 's/@[A-Za-z0-9._+-]+$//'
}

should_process_repo() {
  local current_name="$1"
  local base_name="$2"
  if [[ -z "$ONLY_REPO" ]]; then
    return 0
  fi
  [[ "$ONLY_REPO" == "$current_name" || "$ONLY_REPO" == "$base_name" ]]
}

update_one_repo() {
  local repo_path="$1"
  local current_name base_name
  current_name="$(basename "$repo_path")"
  base_name="$(repo_base_name "$current_name")"

  if ! should_process_repo "$current_name" "$base_name"; then
    return 10
  fi

  if [[ ! -d "$repo_path/.git" ]]; then
    log "[skip] $current_name: not a git repo"
    return 10
  fi

  if [[ -n "$(git -C "$repo_path" status --porcelain)" ]]; then
    log "[skip] $current_name: working tree is dirty"
    return 10
  fi

  if ! git -C "$repo_path" remote get-url origin >/dev/null 2>&1; then
    log "[skip] $current_name: no origin remote"
    return 10
  fi

  log "[update] $current_name"
  run_cmd git -C "$repo_path" fetch --prune --prune-tags --tags --force origin

  local branch origin_head target_branch
  branch="$(git -C "$repo_path" symbolic-ref --quiet --short HEAD || true)"
  if [[ -n "$branch" ]]; then
    if git -C "$repo_path" show-ref --verify --quiet "refs/remotes/origin/$branch"; then
      run_cmd git -C "$repo_path" merge --ff-only "origin/$branch"
    else
      origin_head="$(git -C "$repo_path" symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null || true)"
      if [[ -n "$origin_head" ]]; then
        target_branch="${origin_head#origin/}"
        if [[ "$branch" != "$target_branch" ]]; then
          run_cmd git -C "$repo_path" checkout "$target_branch"
        fi
        run_cmd git -C "$repo_path" merge --ff-only "$origin_head"
      else
        log "[warn] $current_name: cannot resolve origin/HEAD, skip pull"
      fi
    fi
  else
    log "[warn] $current_name: detached HEAD, fetch only"
  fi

  local version_raw version target_name target_path
  version_raw="$(git -C "$repo_path" describe --tags --always 2>/dev/null || git -C "$repo_path" rev-parse --short HEAD)"
  version="$(sanitize_version "$version_raw")"
  target_name="${base_name}@${version}"
  target_path="$REFERENCES_DIR/$target_name"

  if [[ "$current_name" != "$target_name" ]]; then
    if [[ -e "$target_path" ]]; then
      log "[warn] $current_name: target $target_name already exists, skip rename"
    else
      run_cmd mv "$repo_path" "$target_path"
      repo_path="$target_path"
      current_name="$target_name"
      log "[ok] renamed -> $target_name"
    fi
  else
    log "[ok] version folder already up to date: $target_name"
  fi

  local link_path
  link_path="$REFERENCES_DIR/$base_name"
  if [[ "$(basename "$repo_path")" == "$base_name" ]]; then
    return 0
  fi

  if [[ "$DRY_RUN" == "1" ]]; then
    log "[dry-run] ensure symlink $base_name -> $(basename "$repo_path")"
    return 0
  fi

  if [[ -L "$link_path" ]]; then
    run_cmd rm -f "$link_path"
  elif [[ -e "$link_path" ]]; then
    log "[warn] cannot create symlink $base_name -> $current_name (path exists and is not symlink)"
    return 0
  fi
  run_cmd ln -s "$current_name" "$link_path"
  log "[ok] symlink $base_name -> $current_name"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --daily)
      DAILY_MODE=1
      shift
      ;;
    --repo)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --repo" >&2
        exit 2
      fi
      ONLY_REPO="$2"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 2
      ;;
  esac
done

if [[ ! -d "$REFERENCES_DIR" ]]; then
  echo "References directory not found: $REFERENCES_DIR" >&2
  exit 1
fi

today_utc="$(date -u +%F)"
if [[ "$DAILY_MODE" == "1" && -f "$STATE_FILE" ]]; then
  last_day="$(head -n 1 "$STATE_FILE" | tr -d '\r' || true)"
  if [[ "$last_day" == "$today_utc" ]]; then
    log "[daily] references already updated today ($today_utc UTC)"
    exit 0
  fi
fi

updated_count=0
while IFS= read -r repo_path; do
  if update_one_repo "$repo_path"; then
    updated_count=$((updated_count + 1))
  fi
done < <(find "$REFERENCES_DIR" -mindepth 1 -maxdepth 1 -type d ! -name '.*' | sort)

if [[ "$DRY_RUN" != "1" ]]; then
  {
    printf '%s\n' "$today_utc"
    printf 'last_run_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf 'repo_filter=%s\n' "${ONLY_REPO:-all}"
  } > "$STATE_FILE"
fi

log "[done] processed repos: $updated_count"
