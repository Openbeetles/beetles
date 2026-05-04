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

assert_file_not_contains() {
  local file="$1"
  local pattern="$2"
  local message="$3"
  if grep -F -- "$pattern" "$file" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  unexpected pattern: $pattern" >&2
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
assert_eq \
  "$p4_write_profiles" \
  $'--before default-reset --after no-reset\n--before default-reset --after no-reset --no-stub\n--before no-reset --after no-reset --no-stub' \
  "P4 intermediate write-bin should keep the device in bootloader mode between images"

p4_app_write_profiles="$(beetle_espflash_connection_profiles esp32p4 write-bin-app)"
assert_eq \
  "$p4_app_write_profiles" \
  $'--before default-reset --after hard-reset\n--before default-reset --after hard-reset --no-stub\n--before no-reset --after hard-reset --no-stub' \
  "P4 app write-bin should hard-reset after the final image and keep no-stub fallbacks"

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
  'run_espflash_with_profile_key write-bin-app write-bin' \
  "build.sh should use the final-app write-bin flash profile"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'reset_before_final_app_flash_if_needed' \
  "build.sh should reset P4 out of the intermediate flash state before app write"
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
  'ESPFLASH_SKIP_UPDATE_CHECK=true espflash save-image --chip "$FLASH_CHIP"' \
  "build.sh should generate app images through espflash save-image so build-only runs do not depend on Python esptool modules"
assert_file_not_contains \
  "$ROOT_DIR/build.sh" \
  'third_party/espressif__esp'"-dsp" \
  "build.sh should not sync the removed ESP DSP vendor tree"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'PARTITION_FOR_FLASH="$PARTITION_TABLE_BIN"' \
  "build.sh should flash the compiled partition table artifact instead of the source CSV"
assert_file_not_contains \
  "$ROOT_DIR/build.sh" \
  'PARTITION_FOR_FLASH="$PARTITION_CSV"' \
  "build.sh update mode must not fall back to writing the source CSV as a partition image"

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/beetle-flash-strategy.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT
cat >"$tmp_dir/p4_flasher_args.json" <<'JSON'
{
  "bootloader": { "offset": "0x2000", "file": "bootloader/bootloader.bin" },
  "partition-table": { "offset": "0x8000", "file": "partition_table/partition-table.bin" },
  "app": { "offset": "0x20000", "file": "libespidf.bin" }
}
JSON
assert_eq \
  "$(beetle_flasher_args_image_offset "$tmp_dir/p4_flasher_args.json" bootloader)" \
  "0x2000" \
  "P4 flash should consume the bootloader offset emitted by ESP-IDF"
assert_eq \
  "$(beetle_flasher_args_image_offset "$tmp_dir/p4_flasher_args.json" partition-table)" \
  "0x8000" \
  "flash should consume the partition table offset emitted by ESP-IDF"
assert_eq \
  "$(beetle_flasher_args_image_offset "$tmp_dir/p4_flasher_args.json" app)" \
  "0x20000" \
  "flash should consume the app offset emitted by ESP-IDF"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'write-bin --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" "$PARTITION_TABLE_FLASH_OFFSET" "$PARTITION_TABLE_BIN"' \
  "build.sh should refresh the partition table during update flash"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'write-bin --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" "$BOOTLOADER_FLASH_OFFSET" "$BOOTLOADER_BIN"' \
  "build.sh should refresh the bootloader during update flash"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'write-bin --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" "$APP_FLASH_OFFSET" "$APP_BIN"' \
  "build.sh should refresh the app image during update flash"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'missing bootloader/partition-table bin required for update flash.' \
  "build.sh should fail fast when update flash lacks any compiled boot artifact"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'esp-artifacts/$artifact_id' \
  "build.sh should collect a stable ESP artifact directory keyed by git and ELF SHA"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'libespidf.elf' \
  "build.sh should preserve the ESP-IDF ELF for IDF-side address comparison"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'beetle.elf' \
  "build.sh should preserve the final cargo ELF used for Rust panic symbolization"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'symbol_elf=beetle.elf' \
  "build.sh should record beetle.elf as the default symbolization ELF"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'libespidf.map' \
  "build.sh should preserve the ESP-IDF map used for panic symbolization"
if [[ ! -x "$ROOT_DIR/scripts/esp_symbolize_panic.sh" ]]; then
  echo "FAIL: scripts/esp_symbolize_panic.sh should be executable" >&2
  exit 1
fi
assert_file_contains \
  "$ROOT_DIR/partitions.csv" \
  'storage,   data, littlefs,0x620000, 0x9D0000' \
  "default S3 partition table should move the storage partition behind the single 6MiB factory app slot"
assert_file_contains \
  "$ROOT_DIR/partitions.csv" \
  'factory,   app,  factory, 0x20000,  0x600000' \
  "default S3 partition table should publish a single 6MiB factory app slot"
assert_file_not_contains \
  "$ROOT_DIR/partitions.csv" \
  'model' \
  "default S3 partition table must not restore the removed wake resource partition"
assert_file_not_contains \
  "$ROOT_DIR/partitions.csv" \
  'ota_' \
  "default S3 partition table should no longer publish OTA app slots"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'filesystem format changes require full erase/factory reflash even when offset/size stay unchanged' \
  "build.sh update-flash prompt must not imply storage config survives format changes"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'Flash mode: full chip erase required for this storage format migration' \
  "build.sh flash menu must force full erase during destructive storage migration"
assert_file_not_contains \
  "$ROOT_DIR/build.sh" \
  'python3 -m esptool --chip "$FLASH_CHIP" elf2image' \
  "build.sh should no longer depend on python3 -m esptool elf2image for app image generation"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'beetle_full_erase_transport_for_chip "$FLASH_CHIP"' \
  "build.sh should choose the full-erase transport from the chip-specific flash strategy"
assert_file_contains \
  "$ROOT_DIR/build.sh" \
  'python3 -m esptool --chip "$FLASH_CHIP" --port "$CHOSEN_PORT" --before default-reset --after no-reset erase-flash' \
  "build.sh should use esptool for P4 full-chip erase"

echo "esp_flash_strategy_test: ok"
