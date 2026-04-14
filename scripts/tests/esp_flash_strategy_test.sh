#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

# shellcheck source=../build_flash_strategy.sh
source "$ROOT_DIR/scripts/build_flash_strategy.sh"

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

assert_contains_line() {
  local haystack="$1"
  local needle="$2"
  local message="$3"
  if ! printf '%s\n' "$haystack" | grep -Fx -- "$needle" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  missing line: $needle" >&2
    echo "  actual output:" >&2
    printf '%s\n' "$haystack" >&2
    exit 1
  fi
}

assert_not_contains_line() {
  local haystack="$1"
  local needle="$2"
  local message="$3"
  if printf '%s\n' "$haystack" | grep -Fx -- "$needle" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  unexpected line: $needle" >&2
    echo "  actual output:" >&2
    printf '%s\n' "$haystack" >&2
    exit 1
  fi
}

assert_file_contains() {
  local file="$1"
  local pattern="$2"
  local message="$3"
  if ! grep -F -- "$pattern" "$file" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  missing pattern: $pattern" >&2
    echo "  file: $file" >&2
    exit 1
  fi
}

assert_eq \
  "$(beetle_preferred_flash_port_for_chip esp32p4 /dev/cu.usbmodemP4 /dev/cu.wchusbserialP4)" \
  "/dev/cu.wchusbserialP4" \
  "P4 should prefer the USB-UART WCH serial port over the duplicate usbmodem alias"

assert_eq \
  "$(beetle_preferred_flash_port_for_chip esp32s3 /dev/cu.usbmodemS3 /dev/cu.wchusbserialS3)" \
  "/dev/cu.usbmodemS3" \
  "S3 should keep preferring the native usbmodem port"

assert_eq \
  "$(beetle_full_erase_transport_for_chip esp32p4)" \
  "esptool" \
  "P4 full erase should use esptool because espflash erase is not reliable on this board"
assert_eq \
  "$(beetle_full_erase_transport_for_chip esp32s3)" \
  "espflash" \
  "S3 full erase should stay on espflash"

p4_write_profiles="$(beetle_espflash_connection_profiles esp32p4 write-bin)"
assert_contains_line \
  "$p4_write_profiles" \
  "--before default-reset --after no-reset --no-stub" \
  "P4 flashing should first try default-reset without the RAM stub"
assert_contains_line \
  "$p4_write_profiles" \
  "--before no-reset --after no-reset --no-stub" \
  "P4 flashing should retry without toggling reset when already in bootloader mode"

p4_erase_profiles="$(beetle_espflash_connection_profiles esp32p4 erase-flash)"
assert_contains_line \
  "$p4_erase_profiles" \
  "--before default-reset --after no-reset" \
  "P4 erase should use the stub-capable reset flow"
assert_not_contains_line \
  "$p4_erase_profiles" \
  "--before default-reset --after no-reset --no-stub" \
  "P4 erase must not use no-stub because espflash requires the RAM stub"

s3_profiles="$(beetle_espflash_connection_profiles esp32s3 write-bin)"
assert_contains_line \
  "$s3_profiles" \
  "--before default-reset --after no-reset" \
  "S3 flashing should keep the default reset flow"
assert_contains_line \
  "$s3_profiles" \
  "--before usb-reset --after no-reset" \
  "S3 flashing should retry native USB reset automatically for one-click flashing"
assert_contains_line \
  "$s3_profiles" \
  "--before default-reset --after no-reset --no-stub" \
  "S3 flashing should fall back to a no-stub path automatically"

assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'source "$SCRIPT_ROOT/scripts/build_flash_strategy.sh"' \
  "build.sh should source the shared flash strategy helper"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'beetle_preferred_flash_port_for_chip "$FLASH_CHIP"' \
  "build.sh should auto-select the preferred port for the active chip"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'beetle_espflash_connection_profiles "$FLASH_CHIP"' \
  "build.sh should drive espflash through chip-specific connection profiles"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'espflash reset --port "$CHOSEN_PORT" --chip "$FLASH_CHIP"' \
  "build.sh should reset the device back into normal boot before opening the monitor"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'python3 -m serial.tools.miniterm "$CHOSEN_PORT" "$monitor_baud"' \
  "build.sh should use a raw serial monitor after flashing instead of re-entering the bootloader through espflash monitor"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'beetle_full_erase_transport_for_chip "$FLASH_CHIP"' \
  "build.sh should choose the full-erase transport from the chip-specific flash strategy"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'python3 -m esptool --chip "$FLASH_CHIP" --port "$CHOSEN_PORT" --before default-reset --after no-reset erase-flash' \
  "build.sh should use esptool for P4 full-chip erase"

echo "esp_flash_strategy_test: ok"
