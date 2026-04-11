#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

assert_cache_guard() {
  local file="$1"
  if ! rg -n '\.beetle-esp-component-graph\.sha256|Refresh-EspComponentGraphCache|refresh_esp_component_graph_cache' "$file" >/dev/null 2>&1; then
    echo "FAIL: missing ESP component graph cache invalidation in $file" >&2
    exit 1
  fi
}

assert_cache_guard "$ROOT_DIR/build.sh"
assert_cache_guard "$ROOT_DIR/build.ps1"

echo "esp_component_cache_test: ok"
