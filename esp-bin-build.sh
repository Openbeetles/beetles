#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$SCRIPT_DIR"
PRESETS_FILE="$ROOT_DIR/board_presets.toml"
CARGO_TOML="$ROOT_DIR/Cargo.toml"
CONFIGURE_UI_FIRMWARE_DIR="$ROOT_DIR/configure-ui/public/firmware"

# shellcheck source=scripts/esp_bin_build_lib.sh
source "$ROOT_DIR/scripts/esp_bin_build_lib.sh"

usage() {
  cat <<'EOF'
Usage:
  ./esp-bin-build.sh [--version vX.Y.Z] [--output-dir /abs/path] [build.sh args...]

Default behavior:
  - Enumerate every supported ESP board preset from board_presets.toml
  - Build each board through build.sh --no-deploy
  - Merge bootloader + partition-table + app into one flashable bin
  - Generate per-board ESP Web Tools manifests plus release catalog/report/checksums
  - Publish a complete release bundle under dist/esp/{version}/

Examples:
  ./esp-bin-build.sh
  ./esp-bin-build.sh --package-profile voice
  ./esp-bin-build.sh --version v0.1.0-beta.1
EOF
}

require_file() {
  local path="$1"
  local label="$2"
  if [[ ! -f "$path" ]]; then
    echo "Error: missing ${label}: $path" >&2
    exit 1
  fi
}

require_esptool() {
  if ! python3 -m esptool version >/dev/null 2>&1; then
    echo "Error: python3 -m esptool is required for merged single-bin generation." >&2
    exit 1
  fi
}

write_board_manifest() {
  local manifest_file="$1"
  local display_name="$2"
  local version="$3"
  local chip_family="$4"
  local board="$5"
  cat >"$manifest_file" <<EOF
{
  "name": "$display_name",
  "version": "$version",
  "builds": [
    {
      "chipFamily": "$chip_family",
      "parts": [
        {
          "path": "${board}.bin",
          "offset": 0
        }
      ]
    }
  ]
}
EOF
}

copy_update_part() {
  local source_file="$1"
  local output_file="$2"
  cp -p "$source_file" "$output_file"
}

write_release_catalog() {
  local output_file="$1"
  local records_file="$2"
  local version="$3"
  local generated_at="$4"
  python3 - "$records_file" "$version" "$generated_at" >"$output_file" <<'PY'
import csv
import json
import sys

records_path, version, generated_at = sys.argv[1:4]
boards = []
with open(records_path, newline="", encoding="utf-8") as fh:
    reader = csv.DictReader(fh, delimiter="\t")
    for row in reader:
        update_parts = [
            {
                "kind": "bootloader",
                "file": row["update_bootloader_file"],
                "offset": int(row["update_bootloader_offset"], 0),
                "sha256": row["update_bootloader_sha256"],
                "size_bytes": int(row["update_bootloader_size_bytes"]),
            },
            {
                "kind": "partition-table",
                "file": row["update_partition_table_file"],
                "offset": int(row["update_partition_table_offset"], 0),
                "sha256": row["update_partition_table_sha256"],
                "size_bytes": int(row["update_partition_table_size_bytes"]),
            },
            {
                "kind": "app",
                "file": row["update_app_file"],
                "offset": int(row["update_app_offset"], 0),
                "sha256": row["update_app_sha256"],
                "size_bytes": int(row["update_app_size_bytes"]),
            },
        ]
        boards.append(
            {
                "id": row["board"],
                "title": row["title"],
                "chip_family": row["chip_family"],
                "target": row["target"],
                "flash_size": row["flash_size"],
                "partition_table": row["partition_table"],
                "bin": {
                    "file": row["bin_file"],
                    "sha256": row["bin_sha256"],
                    "size_bytes": int(row["bin_size_bytes"]),
                },
                "update_parts": update_parts,
                "manifest": {
                    "file": row["manifest_file"],
                    "sha256": row["manifest_sha256"],
                    "size_bytes": int(row["manifest_size_bytes"]),
                },
            }
        )

payload = {
    "schema_version": 1,
    "product": "beetle",
    "version": version,
    "generated_at_utc": generated_at,
    "boards": boards,
}
json.dump(payload, sys.stdout, indent=2, ensure_ascii=False)
sys.stdout.write("\n")
PY
}

write_release_report() {
  local output_file="$1"
  local records_file="$2"
  local build_args_file="$3"
  local version="$4"
  local generated_at="$5"
  local cargo_version="$6"
  local git_sha="$7"
  local git_ref="$8"
  local git_dirty="$9"
  python3 - "$records_file" "$build_args_file" "$version" "$generated_at" "$cargo_version" "$git_sha" "$git_ref" "$git_dirty" >"$output_file" <<'PY'
import csv
import json
import sys

(
    records_path,
    build_args_path,
    version,
    generated_at,
    cargo_version,
    git_sha,
    git_ref,
    git_dirty,
) = sys.argv[1:9]

boards = []
with open(records_path, newline="", encoding="utf-8") as fh:
    reader = csv.DictReader(fh, delimiter="\t")
    for row in reader:
        update_parts = [
            {
                "kind": "bootloader",
                "file": row["update_bootloader_file"],
                "offset": int(row["update_bootloader_offset"], 0),
                "sha256": row["update_bootloader_sha256"],
                "size_bytes": int(row["update_bootloader_size_bytes"]),
            },
            {
                "kind": "partition-table",
                "file": row["update_partition_table_file"],
                "offset": int(row["update_partition_table_offset"], 0),
                "sha256": row["update_partition_table_sha256"],
                "size_bytes": int(row["update_partition_table_size_bytes"]),
            },
            {
                "kind": "app",
                "file": row["update_app_file"],
                "offset": int(row["update_app_offset"], 0),
                "sha256": row["update_app_sha256"],
                "size_bytes": int(row["update_app_size_bytes"]),
            },
        ]
        boards.append(
            {
                "board": row["board"],
                "title": row["title"],
                "chip_family": row["chip_family"],
                "target": row["target"],
                "flash_size": row["flash_size"],
                "partition_table": row["partition_table"],
                "artifacts": {
                    "bin": {
                        "file": row["bin_file"],
                        "sha256": row["bin_sha256"],
                        "size_bytes": int(row["bin_size_bytes"]),
                    },
                    "update_parts": update_parts,
                    "manifest": {
                        "file": row["manifest_file"],
                        "sha256": row["manifest_sha256"],
                        "size_bytes": int(row["manifest_size_bytes"]),
                    },
                },
            }
        )

with open(build_args_path, encoding="utf-8") as fh:
    build_args = [line.rstrip("\n") for line in fh if line.rstrip("\n")]

dirty_value = None
if git_dirty == "0":
    dirty_value = False
elif git_dirty == "1":
    dirty_value = True

payload = {
    "schema_version": 1,
    "kind": "beetle-esp-release-bundle",
    "status": "ok",
    "version": version,
    "generated_at_utc": generated_at,
    "source": {
        "cargo_version": cargo_version,
        "git_commit": git_sha or None,
        "git_ref": git_ref or None,
        "git_dirty": dirty_value,
    },
    "inputs": {
        "board_presets": "board_presets.toml",
        "build_entrypoint": "build.sh",
        "forwarded_build_args": build_args,
    },
    "artifacts": {
        "catalog": "release-catalog.json",
        "report": "release-report.json",
        "checksums": "SHA256SUMS",
    },
    "boards": boards,
}
json.dump(payload, sys.stdout, indent=2, ensure_ascii=False)
sys.stdout.write("\n")
PY
}

write_sha256sums() {
  local output_file="$1"
  local bundle_dir="$2"
  (
    cd "$bundle_dir"
    find . -type f ! -name 'SHA256SUMS' ! -name '.board-records.tsv' ! -name '.build-args.txt' \
      | LC_ALL=C sort \
      | while IFS= read -r file; do
          [[ -n "$file" ]] || continue
          file="${file#./}"
          printf '%s  %s\n' "$(beetle_sha256_file "$file")" "$file"
        done
  ) >"$output_file"
}

sync_configure_ui_firmware_assets() {
  local source_dir="$1"
  local target_dir="$2"
  local target_parent
  local source_real
  local target_real=""
  local mirror_stage=""

  [[ -d "$source_dir" ]] || {
    echo "Error: missing ESP release bundle for Configure UI sync: $source_dir" >&2
    exit 1
  }

  target_parent="$(dirname "$target_dir")"
  mkdir -p "$target_parent"
  source_real="$(cd "$source_dir" && pwd -P)"
  if [[ -d "$target_dir" ]]; then
    target_real="$(cd "$target_dir" && pwd -P)"
    if [[ "$source_real" == "$target_real" ||
      "$source_real" == "$target_real"/* ||
      "$target_real" == "$source_real"/* ]]; then
      echo "Error: Configure UI firmware directory and release output directory must not overlap: $target_dir" >&2
      exit 1
    fi
  fi

  mirror_stage="$(mktemp -d "$target_parent/.firmware-sync-stage.XXXXXX")"
  if ! cp -pR "$source_dir"/. "$mirror_stage"/; then
    rm -rf "$mirror_stage"
    echo "Error: failed to copy ESP release bundle into Configure UI firmware staging directory." >&2
    exit 1
  fi
  rm -rf "$target_dir"
  mv "$mirror_stage" "$target_dir"
  echo "Configure UI firmware assets synced to: $target_dir"
}

version=""
output_dir=""
build_args=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --version requires a value." >&2; exit 1; }
      version="$1"
      ;;
    --output-dir)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --output-dir requires a value." >&2; exit 1; }
      output_dir="$1"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      build_args+=("$1")
      ;;
  esac
  shift
done

if [[ -z "$version" ]]; then
  version="$(beetle_esp_release_version "$CARGO_TOML")"
fi
if [[ -z "$version" ]]; then
  echo "Error: failed to infer version from Cargo.toml." >&2
  exit 1
fi
if [[ -z "$output_dir" ]]; then
  output_dir="$(beetle_esp_dist_dir "$ROOT_DIR" "$version")"
fi

require_file "$PRESETS_FILE" "board presets"
require_file "$CARGO_TOML" "Cargo.toml"
require_esptool

boards=()
while IFS= read -r board; do
  [[ -n "$board" ]] || continue
  boards+=("$board")
done < <(beetle_supported_esp_boards "$PRESETS_FILE")
if [[ ${#boards[@]} -eq 0 ]]; then
  echo "Error: no supported ESP board presets found in $PRESETS_FILE" >&2
  exit 1
fi

parent_dir="$(dirname "$output_dir")"
mkdir -p "$parent_dir"
stage_dir="$(mktemp -d "$parent_dir/.esp-release-stage.XXXXXX")"
cleanup() {
  if [[ -n "${stage_dir:-}" && -d "$stage_dir" ]]; then
    rm -rf "$stage_dir"
  fi
}
trap cleanup EXIT

generated_at="$(beetle_iso_utc_now)"
cargo_version="${version#v}"
git_sha="$(beetle_git_commit_sha "$ROOT_DIR" || true)"
git_ref="$(beetle_git_ref_name "$ROOT_DIR" || true)"
git_dirty="$(beetle_git_dirty "$ROOT_DIR" || true)"
records_file="$stage_dir/.board-records.tsv"
build_args_file="$stage_dir/.build-args.txt"
printf '%s\n' 'board	title	chip_family	target	flash_size	partition_table	bin_file	bin_sha256	bin_size_bytes	update_bootloader_file	update_bootloader_offset	update_bootloader_sha256	update_bootloader_size_bytes	update_partition_table_file	update_partition_table_offset	update_partition_table_sha256	update_partition_table_size_bytes	update_app_file	update_app_offset	update_app_sha256	update_app_size_bytes	manifest_file	manifest_sha256	manifest_size_bytes' >"$records_file"
if [[ ${#build_args[@]} -gt 0 ]]; then
  printf '%s\n' "${build_args[@]}" >"$build_args_file"
else
  : >"$build_args_file"
fi

for board in "${boards[@]}"; do
  target="$(beetle_board_config_value "$PRESETS_FILE" "$board" "target")"
  partition_table="$(beetle_board_config_value "$PRESETS_FILE" "$board" "partition_table")"
  flash_size="$(beetle_board_config_value "$PRESETS_FILE" "$board" "flash_size")"
  flash_chip="$(beetle_target_mcu_from_triple "$target")"
  chip_family="$(beetle_manifest_chip_family_from_target "$target")"
  display_name="$(beetle_release_board_title "$board" "$chip_family" "$flash_size")"

  [[ -n "$target" && -n "$partition_table" && -n "$flash_size" && -n "$flash_chip" && -n "$chip_family" && -n "$display_name" ]] || {
    echo "Error: incomplete board preset metadata for $board" >&2
    exit 1
  }

  echo ""
  echo "========== Building merged ESP image =========="
  echo "  Board:       $board"
  echo "  Target:      $target"
  echo "  Chip:        $flash_chip"
  echo "  Partition:   $partition_table"
  echo "  Output:      $output_dir/${board}.bin"

  if [[ ${#build_args[@]} -gt 0 ]]; then
    BOARD="$board" "$ROOT_DIR/build.sh" --no-deploy "${build_args[@]}"
  else
    BOARD="$board" "$ROOT_DIR/build.sh" --no-deploy
  fi

  target_root="$ROOT_DIR/target/$target"
  release_dir="$(beetle_find_esp_release_dir "$target_root")"
  if [[ -z "$release_dir" ]]; then
    echo "Error: failed to locate ESP release directory for $board under $target_root" >&2
    exit 1
  fi
  bootloader_bin="$release_dir/bootloader.bin"
  partition_table_bin="$release_dir/partition-table.bin"
  app_bin="$release_dir/beetle.bin"
  esp_idf_build_dir="$(beetle_find_esp_idf_build_dir "$release_dir")"
  if [[ -z "$esp_idf_build_dir" ]]; then
    echo "Error: failed to locate ESP-IDF build directory for $board under $release_dir/build" >&2
    exit 1
  fi
  flasher_args_json="$esp_idf_build_dir/flasher_args.json"

  require_file "$bootloader_bin" "bootloader bin for $board"
  require_file "$partition_table_bin" "partition-table bin for $board"
  require_file "$app_bin" "app bin for $board"
  require_file "$flasher_args_json" "flasher args json for $board"

  flash_mode="$(beetle_flasher_args_value "$flasher_args_json" "flash_mode" || true)"
  flash_freq="$(beetle_flasher_args_value "$flasher_args_json" "flash_freq" || true)"
  flash_size_from_args="$(beetle_flasher_args_value "$flasher_args_json" "flash_size" || true)"
  flash_mode="${flash_mode:-dio}"
  flash_freq="${flash_freq:-80m}"
  flash_size="${flash_size_from_args:-$flash_size}"
  bootloader_offset="$(beetle_flasher_args_image_offset "$flasher_args_json" bootloader)" || {
    echo "Error: bootloader offset missing or invalid in $flasher_args_json" >&2
    exit 1
  }
  partition_table_offset="$(beetle_flasher_args_image_offset "$flasher_args_json" partition-table)" || {
    echo "Error: partition-table offset missing or invalid in $flasher_args_json" >&2
    exit 1
  }
  app_offset="$(beetle_flasher_args_image_offset "$flasher_args_json" app)" || {
    echo "Error: app offset missing or invalid in $flasher_args_json" >&2
    exit 1
  }

  output_file="$stage_dir/${board}.bin"
  python3 -m esptool --chip "$flash_chip" merge-bin \
    -o "$output_file" \
    --flash-mode "$flash_mode" \
    --flash-size "$flash_size" \
    --flash-freq "$flash_freq" \
    "$bootloader_offset" "$bootloader_bin" \
    "$partition_table_offset" "$partition_table_bin" \
    "$app_offset" "$app_bin"

  update_dir="$stage_dir/${board}/update"
  mkdir -p "$update_dir"
  update_bootloader_file="${board}/update/bootloader.bin"
  update_partition_table_file="${board}/update/partition-table.bin"
  update_app_file="${board}/update/app.bin"
  copy_update_part "$bootloader_bin" "$stage_dir/$update_bootloader_file"
  copy_update_part "$partition_table_bin" "$stage_dir/$update_partition_table_file"
  copy_update_part "$app_bin" "$stage_dir/$update_app_file"

  manifest_file="$stage_dir/${board}.manifest.json"
  write_board_manifest "$manifest_file" "$display_name" "$version" "$chip_family" "$board"

  bin_sha256="$(beetle_sha256_file "$output_file")"
  update_bootloader_sha256="$(beetle_sha256_file "$stage_dir/$update_bootloader_file")"
  update_partition_table_sha256="$(beetle_sha256_file "$stage_dir/$update_partition_table_file")"
  update_app_sha256="$(beetle_sha256_file "$stage_dir/$update_app_file")"
  manifest_sha256="$(beetle_sha256_file "$manifest_file")"
  bin_size_bytes="$(beetle_file_size_bytes "$output_file")"
  update_bootloader_size_bytes="$(beetle_file_size_bytes "$stage_dir/$update_bootloader_file")"
  update_partition_table_size_bytes="$(beetle_file_size_bytes "$stage_dir/$update_partition_table_file")"
  update_app_size_bytes="$(beetle_file_size_bytes "$stage_dir/$update_app_file")"
  manifest_size_bytes="$(beetle_file_size_bytes "$manifest_file")"
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$board" \
    "$display_name" \
    "$chip_family" \
    "$target" \
    "$flash_size" \
    "$partition_table" \
    "${board}.bin" \
    "$bin_sha256" \
    "$bin_size_bytes" \
    "$update_bootloader_file" \
    "$bootloader_offset" \
    "$update_bootloader_sha256" \
    "$update_bootloader_size_bytes" \
    "$update_partition_table_file" \
    "$partition_table_offset" \
    "$update_partition_table_sha256" \
    "$update_partition_table_size_bytes" \
    "$update_app_file" \
    "$app_offset" \
    "$update_app_sha256" \
    "$update_app_size_bytes" \
    "${board}.manifest.json" \
    "$manifest_sha256" \
    "$manifest_size_bytes" >>"$records_file"
done

write_release_catalog "$stage_dir/release-catalog.json" "$records_file" "$version" "$generated_at"
write_release_report "$stage_dir/release-report.json" "$records_file" "$build_args_file" "$version" "$generated_at" "$cargo_version" "$git_sha" "$git_ref" "$git_dirty"
write_sha256sums "$stage_dir/SHA256SUMS" "$stage_dir"
rm -f "$records_file" "$build_args_file"

rm -rf "$output_dir"
mv "$stage_dir" "$output_dir"
stage_dir=""
sync_configure_ui_firmware_assets "$output_dir" "$CONFIGURE_UI_FIRMWARE_DIR"

echo ""
echo "ESP release bundle written to: $output_dir"
