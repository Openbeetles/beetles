#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_SH="$ROOT_DIR/build.sh"

assert_contains() {
  local pattern="$1"
  local message="$2"
  if ! rg -n "$pattern" "$BUILD_SH" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  missing pattern: $pattern" >&2
    exit 1
  fi
}

assert_contains 'spiffs_data/skills' \
  "linux deploy should source official runtime skills from spiffs_data/skills"
assert_contains 'REMOTE_TMP_SKILLS_DIR' \
  "linux deploy should stage official skill artifacts on the remote device"
assert_contains 'mkdir -p "\$DEPLOY_STATE_DIR/skills"' \
  "linux deploy install path should prepare the remote skills directory"
assert_contains 'mv "\$f" "\$DEPLOY_STATE_DIR/skills/\$base"' \
  "linux deploy should install uploaded official skills into the remote state root"

echo "linux_deploy_official_skills_sync_test: ok"
