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

assert_absent() {
  local pattern="$1"
  local message="$2"
  if rg -n "$pattern" "$BUILD_SH" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    rg -n "$pattern" "$BUILD_SH" >&2 || true
    exit 1
  fi
}

assert_contains '5\) BUILD_TARGET="aarch64-unknown-linux-musl"' \
  "Linux aarch64 cross-build default must remain musl before native Linux overrides are applied"

gnu_assignment_lines=$(rg -n 'BUILD_TARGET="aarch64-unknown-linux-gnu"' "$BUILD_SH" | cut -d: -f1 || true)
gnu_assignment_count=$(printf '%s\n' "$gnu_assignment_lines" | sed '/^$/d' | wc -l | tr -d ' ')
if [[ "$gnu_assignment_count" != "2" ]]; then
  echo "FAIL: Linux native aarch64 and remote aarch64 paths must both assign the GNU target" >&2
  rg -n 'BUILD_TARGET="aarch64-unknown-linux-gnu"' "$BUILD_SH" >&2 || true
  exit 1
fi

assert_contains 'elif \[\[ \$PLATFORM_CHOICE -eq 5 \]\] && \[\[ "\$CURRENT_ARCH" == "aarch64" \|\| "\$CURRENT_ARCH" == "arm64" \]\]' \
  "Linux native aarch64 path must explicitly switch option 5 to GNU"
assert_contains 'REMOTE_BUILD_TARGET_ENV="linux-aarch64"' \
  "remote aarch64 builds must still keep the dedicated GNU remote target contract"
assert_contains 'BUILD_TARGET="aarch64-unknown-linux-gnu"' \
  "Linux native and remote aarch64 paths must keep the GNU target contract"
assert_contains 'linux-full\)' \
  "linux-full package profile branch must exist"
assert_contains 'linux-full\)[[:space:]]*$' \
  "linux-full package profile must be split out from generic voice+vision+sensor builds"
assert_contains 'capability_sensor,capability_office' \
  "linux-full must include capability_office so account-config APIs are compiled into Linux builds"

echo "linux_aarch64_build_contract_test: ok"
