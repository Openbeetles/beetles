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
  "$(beetle_board_config_value "$ROOT_DIR/board_presets.toml" "esp32-s3-32mb" "flash_size")" \
  "32MB" \
  "esp-bin-build should reuse board_presets.toml flash size metadata"
assert_file_contains \
  "$ROOT_DIR/partitions.csv" \
  'factory,   app,  factory, 0x20000,  0x600000' \
  "official ESP partition tables should use a single factory app slot at 0x20000 sized 0x600000"
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

assert_file_contains \
  "$BUILD_SCRIPT_PATH" \
  '0x0 "$BOOTLOADER_BIN"' \
  "build.sh should flash the bootloader at offset 0x0"
assert_file_contains \
  "$BUILD_SCRIPT_PATH" \
  '0x8000 "$PARTITION_TABLE_BIN"' \
  "build.sh should flash the partition table at offset 0x8000"
assert_file_contains \
  "$BUILD_SCRIPT_PATH" \
  '0x20000 "$APP_BIN"' \
  "build.sh should flash the application at the fixed factory offset"
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
assert_file_not_contains \
  "$LIB_PATH" \
  'beetle_partition_offset()' \
  "esp-bin-build helper library should drop the OTA-only partition offset parser"
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
  '0x20000 "$app_bin"' \
  "esp-bin-build should place the merged application image at the fixed factory offset"
assert_file_contains \
  "$ENTRYPOINT_PATH" \
  'output_file="$stage_dir/${board}.bin"' \
  "esp-bin-build should stage board bins before atomically publishing the final release bundle"
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
EOF

cat >"$tmp_dir/partitions_8mb.csv" <<'EOF'
# Name, Type, SubType, Offset, Size
phy_init, data, phy, 0x19000, 0x1000
factory, app, factory, 0x20000, 0x600000
EOF

cat >"$tmp_dir/build.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$#" > "$PWD/build_argc.txt"
printf '%s\n' "$*" > "$PWD/build_argv.txt"
release_dir="$PWD/target/xtensa-esp32s3-espidf/release-size"
idf_dir="$release_dir/build/mock/out/build"
mkdir -p "$release_dir" "$idf_dir"
printf 'boot' > "$release_dir/bootloader.bin"
printf 'part' > "$release_dir/partition-table.bin"
printf 'app' > "$release_dir/beetle.bin"
cat >"$idf_dir/flasher_args.json" <<'JSON'
{"flash_mode":"dio","flash_size":"8MB","flash_freq":"80m"}
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
  printf '%s\n' "${normalized_args[*]}" > "$PWD/esptool_args.txt"
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
  '0x0 target/xtensa-esp32s3-espidf/release-size/bootloader.bin' \
  "merged images should include the bootloader at offset 0x0"
assert_file_contains \
  "$tmp_dir/esptool_args.txt" \
  '0x8000 target/xtensa-esp32s3-espidf/release-size/partition-table.bin' \
  "merged images should include the partition table at offset 0x8000"
assert_file_contains \
  "$tmp_dir/esptool_args.txt" \
  '0x20000 target/xtensa-esp32s3-espidf/release-size/beetle.bin' \
  "merged images should include the application at the fixed factory offset"
assert_file_not_contains \
  "$tmp_dir/esptool_args.txt" \
  '0x19000' \
  "merged images should not write a legacy OTA metadata segment"
assert_file_not_contains \
  "$tmp_dir/esptool_args.txt" \
  "$legacy_ota_init_bin" \
  "merged images should not depend on ota_data_initial.bin"

assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/esp32-s3-8mb.manifest.json" \
  '"chipFamily": "ESP32-S3"' \
  "board manifest should declare the correct ESP Web Tools chip family"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/esp32-s3-8mb.manifest.json" \
  '"path": "esp32-s3-8mb.bin"' \
  "board manifest should point to the staged merged single-bin artifact"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-catalog.json" \
  '"id": "esp32-s3-8mb"' \
  "release catalog should list each supported board"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/release-report.json" \
  '"status": "ok"' \
  "release report should capture a successful bundle build status"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/SHA256SUMS" \
  'esp32-s3-8mb.bin' \
  "SHA256SUMS should include the merged firmware artifact"
assert_file_contains \
  "$tmp_dir/dist/esp/v9.9.9/SHA256SUMS" \
  'release-catalog.json' \
  "SHA256SUMS should include release metadata"

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
