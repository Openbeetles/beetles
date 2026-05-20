#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_SCRIPT_PATH="$ROOT_DIR/build.sh"
LIB_PATH="$ROOT_DIR/scripts/esp_bin_build_lib.sh"
ENTRYPOINT_PATH="$ROOT_DIR/esp-bin-build.sh"
PACKAGE_SCRIPT_PATH="$ROOT_DIR/scripts/package_esp_release_bundle.sh"

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

assert_file_exists() {
  local path="$1"
  local message="$2"
  if [[ ! -f "$path" ]]; then
    echo "FAIL: $message" >&2
    echo "  missing file: $path" >&2
    exit 1
  fi
}

assert_file_not_exists() {
  local path="$1"
  local message="$2"
  if [[ -e "$path" ]]; then
    echo "FAIL: $message" >&2
    echo "  unexpected path: $path" >&2
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

assert_file_exists \
  "$BUILD_SCRIPT_PATH" \
  "build.sh should remain the canonical ESP flash entrypoint"
assert_file_exists \
  "$LIB_PATH" \
  "esp-bin-build should provide a shared helper library for version and board metadata"
assert_file_exists \
  "$ENTRYPOINT_PATH" \
  "esp-bin-build should exist at the repository root alongside build.sh"
assert_file_exists \
  "$PACKAGE_SCRIPT_PATH" \
  "ESP release packaging should live in a dedicated script for CI/release reuse"
assert_file_exists \
  "$ROOT_DIR/components/json/CMakeLists.txt" \
  "ESP-SR should keep a minimal json component-name shim for IDF6 cJSON without restoring a vendor tree"
assert_file_exists \
  "$ROOT_DIR/components/beetle_wakenet/idf_component.yml" \
  "WakeNet managed dependencies should live with the S3 WakeNet component instead of Cargo-global ESP components"
assert_file_contains \
  "$ROOT_DIR/components/beetle_wakenet/idf_component.yml" \
  'target in [esp32s3, esp32p4]' \
  "WakeNet managed dependencies should be target-gated to ESP32-S3/P4"
assert_file_contains \
  "$ROOT_DIR/build.rs" \
  "cargo:rustc-check-cfg=cfg(beetle_esp_sr_wakenet)" \
  "build.rs should declare the Beetle ESP-SR WakeNet cfg used by the Rust wake backend"
assert_file_contains \
  "$ROOT_DIR/build.rs" \
  "cargo:rustc-cfg=beetle_esp_sr_wakenet" \
  "build.rs should enable the Rust WakeNet backend for ESP32-S3/P4 builds"
assert_file_contains \
  "$BUILD_SCRIPT_PATH" \
  'default_idf_toolchain_for_target()' \
  "build.sh should keep the P4 ESP-SR clang toolchain contract in the board build path"
assert_file_contains \
  "$BUILD_SCRIPT_PATH" \
  'export IDF_TOOLCHAIN="$BUILD_IDF_TOOLCHAIN"' \
  "build.sh should export the required IDF toolchain before Cargo enters esp-idf-sys"
assert_file_contains \
  "$ROOT_DIR/build.ps1" \
  'Get-DefaultIdfToolchainForTarget' \
  "build.ps1 should keep the same P4 ESP-SR clang toolchain contract as build.sh"
assert_file_not_contains \
  "$ROOT_DIR/Cargo.toml" \
  'remote_component = { name = "espressif/esp-sr"' \
  "ESP-SR should not be a Cargo-global extra component that forces P4 to link WakeNet"
assert_file_contains \
  "$ROOT_DIR/components/json/CMakeLists.txt" \
  "REQUIRES espressif__cjson" \
  "json shim should map ESP-SR's legacy component name to the registry cJSON component"
assert_file_contains \
  "$ROOT_DIR/components/beetle_wakenet/CMakeLists.txt" \
  "idf_component_get_property(esp_dsp_lib espressif__esp-dsp COMPONENT_LIB)" \
  "WakeNet component should own the ESP-DSP component lookup used by the IDF6 compile shim"
assert_file_contains \
  "$ROOT_DIR/components/beetle_wakenet/CMakeLists.txt" \
  "target_compile_options(\${esp_dsp_lib} PRIVATE" \
  "WakeNet component should apply the ESP-DSP IDF6 C++ compile shim without patching managed sources"
assert_file_contains \
  "$ROOT_DIR/sdkconfig.defaults.esp32s3" \
  "CONFIG_SR_WN_WN9_HIESP=y" \
  "ESP32-S3 sdkconfig should select the official WakeNet9 Hi,ESP model"
assert_file_contains \
  "$ROOT_DIR/sdkconfig.defaults.esp32p4" \
  "CONFIG_SR_WN_WN9_HIESP=y" \
  "ESP32-P4 sdkconfig should select the same fixed WakeNet9 Hi,ESP model"
assert_file_not_contains \
  "$ROOT_DIR/sdkconfig.defaults.esp32s3" \
  "CONFIG_USE_WAKENET" \
  "ESP32-S3 sdkconfig should not keep stale ESP-SR Kconfig symbols"
assert_file_not_contains \
  "$ROOT_DIR/sdkconfig.defaults.esp32s3" \
  "CONFIG_WAKENET_MODEL_IN_PSRAM" \
  "ESP32-S3 sdkconfig should not keep stale WakeNet PSRAM Kconfig symbols"
assert_file_not_contains \
  "$ROOT_DIR/sdkconfig.defaults.esp32s3" \
  "CONFIG_SR_WN_DETECTION_MODE" \
  "ESP32-S3 sdkconfig should not keep stale WakeNet detection-mode Kconfig symbols"

# shellcheck source=../esp_bin_build_lib.sh
source "$LIB_PATH"

legacy_ota_meta_partition="ota"'data'
legacy_ota_slot_zero="ota"'_0'
legacy_ota_meta_offset="ota"'data_offset='
legacy_ota_init_bin="ota"'_data_initial.bin'
legacy_ota_data_var='OTA'"DATA_BIN"
legacy_ota_flash_segment='0x19000 "$'"$legacy_ota_data_var"'"'

assert_eq \
  "$(beetle_esp_release_version "$ROOT_DIR/Cargo.toml")" \
  "v0.1.0" \
  "esp-bin-build should derive the public release version from Cargo.toml"

supported_boards="$(beetle_supported_esp_boards "$ROOT_DIR/board_presets.toml")"
assert_contains_line \
  "$supported_boards" \
  "esp32-s3-8mb" \
  "esp-bin-build should package the 8MB S3 preset"
assert_contains_line \
  "$supported_boards" \
  "esp32-s3-16mb" \
  "esp-bin-build should package the 16MB S3 preset"
assert_contains_line \
  "$supported_boards" \
  "esp32-s3-32mb" \
  "esp-bin-build should package the 32MB S3 preset"
assert_contains_line \
  "$supported_boards" \
  "esp32-p4-nano-16mb" \
  "esp-bin-build should package the P4 preset"

assert_eq \
  "$(beetle_board_config_value "$ROOT_DIR/board_presets.toml" "esp32-p4-nano-16mb" "target")" \
  "riscv32imafc-esp-espidf" \
  "esp-bin-build should reuse board_presets.toml target metadata"
assert_eq \
  "$(beetle_board_config_value "$ROOT_DIR/board_presets.toml" "esp32-p4-nano-16mb" "idf_toolchain")" \
  "clang" \
  "P4 ESP-SR builds should use the board-declared clang IDF toolchain"
assert_eq \
  "$(beetle_board_config_value "$ROOT_DIR/board_presets.toml" "esp32-s3-32mb" "flash_size")" \
  "32MB" \
  "esp-bin-build should reuse board_presets.toml flash size metadata"
assert_file_contains \
  "$ROOT_DIR/partitions.csv" \
  'factory,   app,  factory, 0x20000,  0x600000' \
  "official ESP partition tables should use a single factory app slot at 0x20000 sized 0x600000"
assert_file_contains \
  "$ROOT_DIR/partitions.csv" \
  'storage,   data, littlefs,0x620000, 0x950000' \
  "default 16MB partition table should cut the WakeNet model space from the storage tail"
assert_file_contains \
  "$ROOT_DIR/partitions.csv" \
  'model,     data, spiffs,  0xF70000, 0x080000' \
  "default 16MB partition table should publish a 512KiB WakeNet model partition before coredump"
assert_file_contains \
  "$ROOT_DIR/partitions_8mb.csv" \
  'storage,   data, littlefs,0x620000, 0x150000' \
  "8MB partition table should keep the storage offset and cut the model partition from its tail"
assert_file_contains \
  "$ROOT_DIR/partitions_8mb.csv" \
  'model,     data, spiffs,  0x770000, 0x080000' \
  "8MB partition table should publish a 512KiB WakeNet model partition before coredump"
assert_file_contains \
  "$ROOT_DIR/partitions_32mb.csv" \
  'storage,   data, littlefs,0x620000,  0x1950000' \
  "32MB partition table should keep the storage offset and cut the model partition from its tail"
assert_file_contains \
  "$ROOT_DIR/partitions_32mb.csv" \
  'model,     data, spiffs,  0x1F70000, 0x080000' \
  "32MB partition table should publish a 512KiB WakeNet model partition before coredump"
assert_file_contains \
  "$ROOT_DIR/partitions_p4_16mb.csv" \
  'storage,   data, littlefs,0x620000, 0x950000' \
  "P4 16MB partition table should cut the WakeNet model space from the storage tail"
assert_file_contains \
  "$ROOT_DIR/partitions_p4_16mb.csv" \
  'model,     data, spiffs,  0xF70000, 0x080000' \
  "P4 16MB partition table should publish a 512KiB WakeNet model partition before coredump"
assert_file_contains \
  "$ROOT_DIR/partitions.csv" \
  'coredump,  data, coredump,0xFF0000, 0x10000' \
  "default 16MB partition table should keep the coredump partition unchanged"
assert_file_not_contains \
  "$ROOT_DIR/partitions.csv" \
  "$legacy_ota_meta_partition" \
  "official ESP partition tables should no longer publish a legacy OTA metadata partition"
assert_file_not_contains \
  "$ROOT_DIR/partitions_p4_16mb.csv" \
  "$legacy_ota_slot_zero" \
  "official ESP partition tables should no longer publish OTA app slots"

assert_eq \
  "$(beetle_esp_dist_dir "$ROOT_DIR" "v0.1.0")" \
  "$ROOT_DIR/dist/esp/v0.1.0" \
  "esp-bin-build should publish under dist/esp/{version}"
assert_eq \
  "$(beetle_manifest_chip_family_from_target "xtensa-esp32s3-espidf")" \
  "ESP32-S3" \
  "esp-bin-build manifests should use the ESP Web Tools chip family for S3 boards"
assert_eq \
  "$(beetle_manifest_chip_family_from_target "riscv32imafc-esp-espidf")" \
  "ESP32-P4" \
  "esp-bin-build manifests should use the ESP Web Tools chip family for P4 boards"
if beetle_target_mcu_from_triple "riscv32imc-esp-espidf" >/dev/null 2>&1; then
  echo "FAIL: ESP32-C3 target must not be accepted as an official Beetle ESP target" >&2
  exit 1
fi
if beetle_manifest_chip_family_from_target "xtensa-esp32s2-espidf" >/dev/null 2>&1; then
  echo "FAIL: ESP32-S2 target must not be published in official Beetle ESP manifests" >&2
  exit 1
fi

assert_file_contains \
  "$BUILD_SCRIPT_PATH" \
  'BOOTLOADER_FLASH_OFFSET="$(beetle_flasher_args_image_offset "$FLASHER_ARGS_JSON" bootloader)"' \
  "build.sh should derive the bootloader offset from ESP-IDF flasher_args.json"
assert_file_contains \
  "$BUILD_SCRIPT_PATH" \
  'PARTITION_TABLE_FLASH_OFFSET="$(beetle_flasher_args_image_offset "$FLASHER_ARGS_JSON" partition-table)"' \
  "build.sh should derive the partition table offset from ESP-IDF flasher_args.json"
assert_file_contains \
  "$BUILD_SCRIPT_PATH" \
  'APP_FLASH_OFFSET="$(beetle_flasher_args_image_offset "$FLASHER_ARGS_JSON" app)"' \
  "build.sh should derive the application offset from ESP-IDF flasher_args.json"
assert_file_contains \
  "$BUILD_SCRIPT_PATH" \
  'MODEL_PARTITION_OFFSET="$(beetle_partition_csv_offset "$PARTITION_CSV" model 2>/dev/null || true)"' \
  "build.sh should derive the optional WakeNet model offset from the active partition CSV"
assert_file_contains \
  "$BUILD_SCRIPT_PATH" \
  'MODEL_BIN="$(beetle_find_srmodels_bin "$RELEASE_DIR" || true)"' \
  "build.sh should locate srmodels.bin from the current ESP-IDF build directory"
assert_file_contains \
  "$BUILD_SCRIPT_PATH" \
  'if [[ "$model_offset" != "missing" ]]; then' \
  "build.sh artifact collection should only attach srmodels.bin when the active partition table has a model partition"
assert_file_not_contains \
  "$BUILD_SCRIPT_PATH" \
  'OTADATA_BIN=' \
  "build.sh should stop tracking OTA data artifacts"
assert_file_not_contains \
  "$BUILD_SCRIPT_PATH" \
  "$legacy_ota_init_bin" \
  "build.sh should stop copying ota_data_initial.bin into flash/release contracts"
assert_file_not_contains \
  "$BUILD_SCRIPT_PATH" \
  "$legacy_ota_flash_segment" \
  "build.sh should no longer flash a legacy OTA metadata segment"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'BOARD="$board" "$ROOT_DIR/build.sh" --no-deploy' \
  "esp-bin-build should drive the existing build.sh once per supported board"
assert_file_not_contains \
  "$ENTRYPOINT_PATH" \
  'mapfile -t boards' \
  "esp-bin-build should stay compatible with macOS default Bash and avoid mapfile"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'if [[ ${#build_args[@]} -gt 0 ]]; then' \
  "esp-bin-build should guard optional build args before expanding the array"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'python3 -m esptool --chip "$flash_chip" merge-bin' \
  "esp-bin-build should produce single-bin outputs through esptool merge-bin"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'bootloader_offset="$(beetle_flasher_args_image_offset "$flasher_args_json" bootloader)"' \
  "esp-bin-build should derive the bootloader merge offset from ESP-IDF flasher_args.json"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'partition_table_offset="$(beetle_flasher_args_image_offset "$flasher_args_json" partition-table)"' \
  "esp-bin-build should derive the partition table merge offset from ESP-IDF flasher_args.json"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'app_offset="$(beetle_flasher_args_image_offset "$flasher_args_json" app)"' \
  "esp-bin-build should derive the app merge offset from ESP-IDF flasher_args.json"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'model_offset="$(beetle_partition_csv_offset "$partition_csv" model 2>/dev/null || true)"' \
  "esp-bin-build should derive the optional WakeNet model offset from the board partition table"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'model_bin="$(beetle_find_srmodels_bin "$release_dir" || true)"' \
  "esp-bin-build should locate srmodels.bin from the current ESP-IDF build directory"
assert_file_not_contains \
  "$LIB_PATH" \
  'beetle_partition_offset()' \
  "esp-bin-build helper library should drop the OTA-only partition offset parser"
assert_file_contains \
  "$LIB_PATH" \
  'beetle_partition_csv_offset()' \
  "esp-bin-build helper library should expose a generic partition CSV offset parser for model partitions"
assert_file_not_contains \
  "$ENTRYPOINT_PATH" \
  "$legacy_ota_meta_offset" \
  "esp-bin-build should stop parsing a legacy OTA metadata offset from partition tables"
assert_file_not_contains \
  "$ENTRYPOINT_PATH" \
  "$legacy_ota_init_bin" \
  "esp-bin-build should stop depending on ota_data_initial.bin when generating merged images"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  '"$app_offset" "$app_bin"' \
  "esp-bin-build should place the merged application image at the ESP-IDF emitted app offset"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'output_file="$stage_dir/${board}.bin"' \
  "esp-bin-build should stage board bins before atomically publishing the final release bundle"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'copy_update_part "$bootloader_bin"' \
  "esp-bin-build should publish bootloader as an update-safe part"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'copy_update_part "$partition_table_bin"' \
  "esp-bin-build should publish partition table as an update-safe part"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'copy_update_part "$app_bin"' \
  "esp-bin-build should publish app as an update-safe part"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'copy_update_part "$model_bin"' \
  "esp-bin-build should publish srmodels.bin as an update-safe model part"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'write_release_catalog "$stage_dir/release-catalog.json"' \
  "esp-bin-build should emit a machine-readable release catalog"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'write_release_report "$stage_dir/release-report.json"' \
  "esp-bin-build should emit a machine-readable release report"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'write_sha256sums "$stage_dir/SHA256SUMS"' \
  "esp-bin-build should generate bundle-local SHA256SUMS"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'sync_configure_ui_firmware_assets "$output_dir" "$CONFIGURE_UI_FIRMWARE_DIR"' \
  "esp-bin-build should mirror the published release bundle into Configure UI firmware assets"
assert_file_contains \
  "$PACKAGE_SCRIPT_PATH" \
  'beetle-${version}-esp-release-bundle.tar.gz' \
  "ESP release packaging should archive the full versioned bundle for GitHub releases"
assert_file_contains \
  "$ROOT_DIR/.github/workflows/release.yml" \
  './esp-bin-build.sh' \
  "release workflow should build the published ESP bundle through the shared esp-bin-build entrypoint"
assert_file_contains \
  "$ROOT_DIR/.github/workflows/release.yml" \
  'scripts/package_esp_release_bundle.sh' \
  "release workflow should package ESP release assets through the dedicated helper script"

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/beetle-esp-bin-build-test.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT

mkdir -p "$tmp_dir/scripts" "$tmp_dir/bin"
cp "$ENTRYPOINT_PATH" "$tmp_dir/esp-bin-build.sh"
cp "$LIB_PATH" "$tmp_dir/scripts/esp_bin_build_lib.sh"
cp "$PACKAGE_SCRIPT_PATH" "$tmp_dir/scripts/package_esp_release_bundle.sh"

cat >"$tmp_dir/Cargo.toml" <<'EOF'
[package]
version = "9.9.9"
EOF

cat >"$tmp_dir/board_presets.toml" <<'EOF'
[boards.esp32-s3-8mb]
target = "xtensa-esp32s3-espidf"
partition_table = "partitions_8mb.csv"
flash_size = "8MB"

[boards.esp32-p4-nano-16mb]
target = "riscv32imafc-esp-espidf"
partition_table = "partitions_p4_16mb.csv"
flash_size = "16MB"
idf_toolchain = "clang"
EOF

cat >"$tmp_dir/partitions_8mb.csv" <<'EOF'
# Name, Type, SubType, Offset, Size
phy_init, data, phy, 0x19000, 0x1000
factory, app, factory, 0x20000, 0x600000
storage, data, littlefs, 0x620000, 0x150000
model, data, spiffs, 0x770000, 0x080000
coredump, data, coredump, 0x7F0000, 0x10000
EOF

cat >"$tmp_dir/partitions_p4_16mb.csv" <<'EOF'
# Name, Type, SubType, Offset, Size
phy_init, data, phy, 0x19000, 0x1000
factory, app, factory, 0x20000, 0x600000
storage, data, littlefs, 0x620000, 0x950000
model, data, spiffs, 0xF70000, 0x080000
coredump, data, coredump, 0xFF0000, 0x10000
EOF

cat >"$tmp_dir/build.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$#" > "$PWD/build_argc.txt"
printf '%s\n' "$*" > "$PWD/build_argv.txt"
case "${BOARD:-esp32-s3-8mb}" in
  esp32-s3-8mb) target="xtensa-esp32s3-espidf" ;;
  esp32-p4-nano-16mb) target="riscv32imafc-esp-espidf" ;;
  *) echo "unexpected board: ${BOARD:-}" >&2; exit 1 ;;
esac
release_dir="$PWD/target/$target/release-size"
idf_dir="$release_dir/build/mock/out/build"
mkdir -p "$release_dir" "$idf_dir"
printf 'boot' > "$release_dir/bootloader.bin"
printf 'part' > "$release_dir/partition-table.bin"
printf 'app' > "$release_dir/beetle.bin"
if [[ "${BOARD:-esp32-s3-8mb}" == "esp32-s3-8mb" || "${BOARD:-esp32-s3-8mb}" == "esp32-p4-nano-16mb" ]]; then
  mkdir -p "$idf_dir/srmodels"
  printf 'model' > "$idf_dir/srmodels/srmodels.bin"
fi
cat >"$idf_dir/flasher_args.json" <<'JSON'
{
  "flash_mode": "dio",
  "flash_size": "8MB",
  "flash_freq": "80m",
  "bootloader": { "offset": "0x1000", "file": "bootloader/bootloader.bin" },
  "partition-table": { "offset": "0x9000", "file": "partition_table/partition-table.bin" },
  "app": { "offset": "0x30000", "file": "beetle.bin" }
}
JSON
EOF
chmod +x "$tmp_dir/build.sh"

REAL_PYTHON3="$(command -v python3)"

cat >"$tmp_dir/bin/python3" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "-m" && "${2:-}" == "esptool" && "${3:-}" == "version" ]]; then
  exit 0
fi
if [[ "${1:-}" == "-m" && "${2:-}" == "esptool" ]]; then
  normalized_args=()
  for arg in "$@"; do
    normalized_args+=("${arg#"$PWD"/}")
  done
  printf '%s\n' "${normalized_args[*]}" >> "$PWD/esptool_args.txt"
  out=""
  prev=""
  for arg in "$@"; do
    if [[ "${prev:-}" == "-o" ]]; then
      out="$arg"
      break
    fi
    prev="$arg"
  done
  [[ -n "$out" ]] || exit 1
  printf 'merged' > "$out"
  exit 0
fi
exec "__REAL_PYTHON3__" "$@"
EOF
sed -i.bak "s|__REAL_PYTHON3__|$REAL_PYTHON3|g" "$tmp_dir/bin/python3"
rm -f "$tmp_dir/bin/python3.bak"
chmod +x "$tmp_dir/bin/python3"

mkdir -p "$tmp_dir/configure-ui/public/firmware"
printf 'stale' > "$tmp_dir/configure-ui/public/firmware/stale.bin"

(
  cd "$tmp_dir"
  PATH="$tmp_dir/bin:$PATH" ./esp-bin-build.sh >/dev/null
)

assert_file_exists \
  "$tmp_dir/dist/esp/v9.9.9/esp32-s3-8mb.bin" \
  "esp-bin-build should run successfully with no forwarded build args under macOS Bash 3"
assert_file_exists \
  "$tmp_dir/dist/esp/v9.9.9/esp32-s3-8mb.manifest.json" \
  "esp-bin-build should emit an ESP Web Tools manifest alongside each board bin"
assert_file_exists \
  "$tmp_dir/dist/esp/v9.9.9/esp32-p4-nano-16mb.bin" \
  "esp-bin-build should also build P4 WakeNet ESP boards in the same release bundle"
assert_file_exists \
  "$tmp_dir/dist/esp/v9.9.9/release-catalog.json" \
  "esp-bin-build should emit a release catalog in the published bundle directory"
assert_file_exists \
  "$tmp_dir/dist/esp/v9.9.9/release-report.json" \
  "esp-bin-build should emit a release report in the published bundle directory"
assert_file_exists \
  "$tmp_dir/dist/esp/v9.9.9/SHA256SUMS" \
  "esp-bin-build should emit SHA256SUMS for the full ESP release bundle"
assert_file_not_exists \
  "$tmp_dir/dist/esp/v9.9.9/.board-records.tsv" \
  "esp-bin-build should not leak internal board bookkeeping into the published release directory"
assert_file_not_exists \
  "$tmp_dir/dist/esp/v9.9.9/.build-args.txt" \
  "esp-bin-build should not leak internal build-arg bookkeeping into the published release directory"
assert_eq \
  "$(cat "$tmp_dir/build_argc.txt")" \
  "1" \
  "esp-bin-build should call build.sh with only --no-deploy when no extra args are provided"
assert_eq \
  "$(cat "$tmp_dir/build_argv.txt")" \
  "--no-deploy" \
  "esp-bin-build should not expand an empty build_args array into a Bash 3 nounset failure"
assert_file_contains \
  "$tmp_dir/esptool_args.txt" \
  '0x1000 target/xtensa-esp32s3-espidf/release-size/bootloader.bin' \
  "merged images should use the bootloader offset emitted by ESP-IDF"
assert_file_contains \
  "$tmp_dir/esptool_args.txt" \
  '0x9000 target/xtensa-esp32s3-espidf/release-size/partition-table.bin' \
  "merged images should use the partition table offset emitted by ESP-IDF"
assert_file_contains \
  "$tmp_dir/esptool_args.txt" \
  '0x30000 target/xtensa-esp32s3-espidf/release-size/beetle.bin' \
  "merged images should use the app offset emitted by ESP-IDF"
assert_file_contains \
  "$tmp_dir/esptool_args.txt" \
  '0x770000 target/xtensa-esp32s3-espidf/release-size/build/mock/out/build/srmodels/srmodels.bin' \
  "merged images should include srmodels.bin at the model partition offset from the CSV"
assert_file_not_contains \
  "$tmp_dir/esptool_args.txt" \
  '0x19000' \
  "merged images should not write a legacy OTA metadata segment"
assert_file_not_contains \
  "$tmp_dir/esptool_args.txt" \
  "$legacy_ota_init_bin" \
  "merged images should not depend on ota_data_initial.bin"

assert_file_exists \
  "$tmp_dir/configure-ui/public/firmware/esp32-s3-8mb.bin" \
  "esp-bin-build should copy the published merged firmware into Configure UI firmware assets"
assert_file_exists \
  "$tmp_dir/configure-ui/public/firmware/esp32-s3-8mb/update/app.bin" \
  "esp-bin-build should copy update-mode firmware parts into Configure UI firmware assets"
assert_file_exists \
  "$tmp_dir/configure-ui/public/firmware/esp32-s3-8mb/update/srmodels.bin" \
  "esp-bin-build should copy the WakeNet model update part into Configure UI firmware assets"
assert_file_exists \
  "$tmp_dir/configure-ui/public/firmware/esp32-p4-nano-16mb/update/srmodels.bin" \
  "esp-bin-build should copy the P4 WakeNet model update part into Configure UI firmware assets"
assert_file_exists \
  "$tmp_dir/configure-ui/public/firmware/release-catalog.json" \
  "esp-bin-build should copy the release catalog into Configure UI firmware assets"
assert_file_exists \
  "$tmp_dir/configure-ui/public/firmware/SHA256SUMS" \
  "esp-bin-build should copy bundle checksums into Configure UI firmware assets"
assert_file_not_exists \
  "$tmp_dir/configure-ui/public/firmware/stale.bin" \
  "esp-bin-build should replace stale Configure UI firmware assets atomically"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/esp32-s3-8mb.manifest.json" \
  '"chipFamily": "ESP32-S3"' \
  "board manifest should declare the correct ESP Web Tools chip family"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/esp32-s3-8mb.manifest.json" \
  '"path": "esp32-s3-8mb.bin"' \
  "board manifest should point to the staged merged single-bin artifact"
assert_file_exists \
  "$tmp_dir/dist/esp/v9.9.9/esp32-s3-8mb/update/bootloader.bin" \
  "esp-bin-build should publish bootloader for update mode"
assert_file_exists \
  "$tmp_dir/dist/esp/v9.9.9/esp32-s3-8mb/update/partition-table.bin" \
  "esp-bin-build should publish partition table for update mode"
assert_file_exists \
  "$tmp_dir/dist/esp/v9.9.9/esp32-s3-8mb/update/app.bin" \
  "esp-bin-build should publish app for update mode"
assert_file_exists \
  "$tmp_dir/dist/esp/v9.9.9/esp32-s3-8mb/update/srmodels.bin" \
  "esp-bin-build should publish srmodels.bin for update mode"
assert_file_exists \
  "$tmp_dir/dist/esp/v9.9.9/esp32-p4-nano-16mb/update/srmodels.bin" \
  "esp-bin-build should publish srmodels.bin for P4 update mode"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-catalog.json" \
  '"id": "esp32-s3-8mb"' \
  "release catalog should list each supported board"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-catalog.json" \
  '"update_parts": [' \
  "release catalog should expose segmented update parts"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-catalog.json" \
  '"offset": 36864' \
  "release catalog should expose the partition-table update offset"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-catalog.json" \
  '"offset": 196608' \
  "release catalog should expose the app update offset"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-catalog.json" \
  '"kind": "model"' \
  "release catalog should expose the WakeNet model update part"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-catalog.json" \
  '"offset": 7798784' \
  "release catalog should expose the model partition update offset"
assert_file_not_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-catalog.json" \
  'requires_full_erase' \
  "release catalog must not carry release-specific full-erase policy"
assert_file_not_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-catalog.json" \
  'storage_migration' \
  "release catalog must not carry release-specific storage migration policy"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-report.json" \
  '"status": "ok"' \
  "release report should capture a successful bundle build status"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-report.json" \
  '"kind": "model"' \
  "release report should expose the WakeNet model update part"
assert_file_not_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-report.json" \
  'requires_full_erase' \
  "release report must not carry release-specific full-erase policy"
assert_file_not_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-report.json" \
  'storage_migration' \
  "release report must not carry release-specific storage migration policy"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/SHA256SUMS" \
  'esp32-s3-8mb.bin' \
  "SHA256SUMS should include the merged firmware artifact"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/SHA256SUMS" \
  'esp32-s3-8mb/update/app.bin' \
  "SHA256SUMS should include update-mode app firmware"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/SHA256SUMS" \
  'esp32-s3-8mb/update/srmodels.bin' \
  "SHA256SUMS should include update-mode WakeNet model firmware"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/SHA256SUMS" \
  'esp32-p4-nano-16mb/update/srmodels.bin' \
  "SHA256SUMS should include P4 WakeNet model firmware"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/SHA256SUMS" \
  'release-catalog.json' \
  "SHA256SUMS should include release metadata"

if (
  cd "$tmp_dir"
  PATH="$tmp_dir/bin:$PATH" ./esp-bin-build.sh --output-dir "$tmp_dir/configure-ui/public/firmware/custom" >/dev/null 2>"$tmp_dir/overlap.stderr"
); then
  echo "FAIL: esp-bin-build should reject output directories inside Configure UI firmware assets" >&2
  exit 1
fi
assert_file_contains \
  "$tmp_dir/overlap.stderr" \
  "must not overlap" \
  "esp-bin-build should keep the release output directory separate from the Configure UI firmware mirror"

mkdir -p "$tmp_dir/release-assets"
(
  cd "$tmp_dir"
  scripts/package_esp_release_bundle.sh \
    --input-dir "$tmp_dir/dist/esp/v9.9.9" \
    --version v9.9.9 \
    --output-dir "$tmp_dir/release-assets"
)

assert_file_exists \
  "$tmp_dir/release-assets/beetle-v9.9.9-esp-release-bundle.tar.gz" \
  "release packaging should archive the full versioned ESP bundle"
assert_file_exists \
  "$tmp_dir/release-assets/beetle-v9.9.9-esp-release-catalog.json" \
  "release packaging should promote the catalog as a top-level release asset"
assert_file_exists \
  "$tmp_dir/release-assets/beetle-v9.9.9-esp-release-report.json" \
  "release packaging should promote the report as a top-level release asset"
assert_file_exists \
  "$tmp_dir/release-assets/beetle-v9.9.9-esp-SHA256SUMS.txt" \
  "release packaging should promote ESP bundle checksums as a top-level release asset"
assert_file_exists \
  "$tmp_dir/release-assets/beetle-v9.9.9-esp32-s3-8mb.bin" \
  "release packaging should expose each merged board firmware as a direct release asset"

echo "esp_bin_build_test: ok"
