#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
P4_BOARD_DEFAULTS="$ROOT_DIR/sdkconfig.defaults.esp32p4.board"

assert_contains() {
  local needle="$1"
  local message="$2"
  if ! grep -Fx -- "$needle" "$P4_BOARD_DEFAULTS" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  missing line: $needle" >&2
    exit 1
  fi
}

assert_not_contains() {
  local needle="$1"
  local message="$2"
  if grep -Fx -- "$needle" "$P4_BOARD_DEFAULTS" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  unexpected line: $needle" >&2
    exit 1
  fi
}

assert_contains \
  "CONFIG_ESP32P4_SELECTS_REV_LESS_V3=y" \
  "P4 board defaults must explicitly opt into the rev<3 silicon path"
assert_contains \
  "CONFIG_ESP32P4_REV_MIN_100=y" \
  "P4 board defaults must remain compatible with rev1.x boards"
assert_not_contains \
  "CONFIG_ESP32P4_REV_MIN_301=y" \
  "P4 board defaults must not force a rev3.1 minimum on rev1.x hardware"

echo "p4_revision_contract_test: ok"
