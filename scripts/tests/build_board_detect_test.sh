#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

# shellcheck source=../build_board_detect.sh
source "$ROOT_DIR/scripts/build_board_detect.sh"

assert_eq() {
  local actual="$1"
  local expected="$2"
  local message="$3"
  if [[ "$actual" != "$expected" ]]; then
    echo "FAIL: $message" >&2
    echo "  expected: $expected" >&2
    echo "  actual:   $actual" >&2
    exit 1
  fi
}

assert_fail() {
  local message="$1"
  shift
  if "$@"; then
    echo "FAIL: $message" >&2
    exit 1
  fi
}

assert_eq "$(beetle_map_board_from_chip_flash esp32p4 16MB)" "esp32-p4-nano-16mb" "P4 should map to the only supported P4 preset"
assert_eq "$(beetle_map_board_from_chip_flash esp32s3 8MB)" "esp32-s3-8mb" "S3 8MB should map to 8MB preset"
assert_eq "$(beetle_map_board_from_chip_flash esp32s3 16MB)" "esp32-s3-16mb" "S3 16MB should map to 16MB preset"
assert_eq "$(beetle_map_board_from_chip_flash esp32s3 32MB)" "esp32-s3-32mb" "S3 32MB should map to 32MB preset"
assert_fail "Unsupported flash size should not map" beetle_map_board_from_chip_flash esp32p4 32MB
assert_fail "Unknown chip should not map" beetle_map_board_from_chip_flash esp32c6 8MB

sample_output=$'Chip type:         esp32s3 (revision v0.2)\nCrystal frequency: 40MHz\nFlash size:        32MB\nFeatures:          WiFi, BLE\nMAC address:       00:11:22:33:44:55'
parsed="$(beetle_parse_board_info "$sample_output")"
assert_eq "$parsed" $'esp32s3\t32MB' "board-info parser should extract chip and flash size"

echo "build_board_detect_test: ok"
