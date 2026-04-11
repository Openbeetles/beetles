#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

assert_no_mapfile() {
  local file="$1"
  if grep -n '\<mapfile\>' "$file" >/dev/null 2>&1; then
    echo "FAIL: bash portability regression: mapfile is not allowed in $file" >&2
    grep -n '\<mapfile\>' "$file" >&2 || true
    exit 1
  fi
}

assert_no_mapfile "$ROOT_DIR/build.sh"
assert_no_mapfile "$ROOT_DIR/scripts/esp_hosted_c6.sh"

echo "bash_portability_test: ok"
