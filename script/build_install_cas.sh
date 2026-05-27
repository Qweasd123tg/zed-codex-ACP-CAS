#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "[compat] script/build_install_cas.sh is kept as a wrapper; use ./install.sh directly." >&2
exec "$ROOT_DIR/install.sh" "$@"
