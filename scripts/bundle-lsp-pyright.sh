#!/bin/bash
# Backward-compatible wrapper — bundles all language servers.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/bundle-lsps-macos.sh" "$@"
