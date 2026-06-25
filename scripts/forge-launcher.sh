#!/bin/bash
# Forge IDE launcher — sets backend URL for lapce-proxy child processes.
DIR="$(cd "$(dirname "$0")" && pwd)"
export FORGE_SEARCH_URL="https://forge-search-production.up.railway.app"
exec "$DIR/lapce" "$@"
