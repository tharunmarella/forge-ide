#!/bin/bash
# Forge IDE launcher — sets backend URL for lapce-proxy child processes.
DIR="$(cd "$(dirname "$0")" && pwd)"
export FORGE_SEARCH_URL="http://localhost:8080"
exec "$DIR/lapce" "$@"
