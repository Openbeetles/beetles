#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_SH="$ROOT_DIR/build.sh"
FEATURE_EXPANDER="$ROOT_DIR/scripts/expand_cargo_features.py"

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

assert_csv_contains() {
  local csv="$1"
  local feature_name="$2"
  local message="$3"
  if ! printf '%s\n' "$csv" | rg "(^|,)${feature_name}(,|$)" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  csv: $csv" >&2
    exit 1
  fi
}

assert_contains 'BUILD_TARGET="aarch64-unknown-linux-musl"' \
  "Linux aarch64 cross-build default must remain musl before native Linux overrides are applied"

gnu_assignment_lines=$(rg -n 'BUILD_TARGET="aarch64-unknown-linux-gnu"' "$BUILD_SH" | cut -d: -f1 || true)
gnu_assignment_count=$(printf '%s\n' "$gnu_assignment_lines" | sed '/^$/d' | wc -l | tr -d ' ')
if [[ "$gnu_assignment_count" != "3" ]]; then
  echo "FAIL: Linux native, remote, and Docker aarch64 paths must all assign the GNU target" >&2
  rg -n 'BUILD_TARGET="aarch64-unknown-linux-gnu"' "$BUILD_SH" >&2 || true
  exit 1
fi

assert_contains 'elif \[\[ \$PLATFORM_CHOICE -eq 5 \]\] && \[\[ "\$CURRENT_ARCH" == "aarch64" \|\| "\$CURRENT_ARCH" == "arm64" \]\]' \
  "Linux native aarch64 path must explicitly switch option 5 to GNU"
assert_contains 'REMOTE_BUILD_TARGET_ENV="linux-aarch64"' \
  "remote aarch64 builds must still keep the dedicated GNU remote target contract"
assert_contains 'linux_apply_docker_target_for_platform\(\)' \
  "Linux Docker path must keep a dedicated target-selection override hook"
assert_contains 'BUILD_TARGET="aarch64-unknown-linux-gnu"' \
  "Linux Docker aarch64 path must also switch option 5 to the GNU target"
assert_contains 'BUILD_TARGET="aarch64-unknown-linux-gnu"' \
  "Linux native, remote, and Docker aarch64 paths must keep the GNU target contract"
assert_contains 'linux-full\)' \
  "linux-full package profile branch must exist"
assert_contains 'linux-full\)[[:space:]]*$' \
  "linux-full package profile must be split out from generic voice+vision+sensor builds"
assert_contains "roots_csv='default,capability_office,dingtalk'" \
  "linux-full must resolve from Cargo default plus capability_office and dingtalk"

linux_full_features="$(python3 "$FEATURE_EXPANDER" --manifest "$ROOT_DIR/Cargo.toml" --roots 'default,capability_office,dingtalk' --format csv)"
assert_csv_contains "$linux_full_features" "capability_office" \
  "linux-full expansion must keep capability_office so account-config APIs compile into Linux builds"
assert_csv_contains "$linux_full_features" "default_runtime" \
  "linux-full expansion must still include the default runtime closure"
assert_csv_contains "$linux_full_features" "qq_channel" \
  "linux-full expansion must inherit QQ from Cargo default"
assert_csv_contains "$linux_full_features" "dingtalk" \
  "linux-full expansion must keep DingTalk explicitly enabled"

echo "linux_aarch64_build_contract_test: ok"
