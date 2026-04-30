#!/usr/bin/env bash

beetle_esp_release_version() {
  local cargo_toml="${1:-}"
  [[ -f "$cargo_toml" ]] || return 1

  local detected=""
  detected="$(
    awk '
      /^\[package\]$/ { in_package=1; next }
      /^\[/ { if (in_package) exit }
      in_package && /^version[[:space:]]*=/ {
        match($0, /"[^"]+"/)
        if (RSTART > 0) {
          print substr($0, RSTART + 1, RLENGTH - 2)
          exit
        }
      }
    ' "$cargo_toml"
  )"
  [[ -n "$detected" ]] || return 1
  printf 'v%s\n' "$detected"
}

beetle_supported_esp_boards() {
  local presets_file="${1:-}"
  [[ -f "$presets_file" ]] || return 1

  awk '
    /^\[boards\./ {
      if (current != "" && is_esp) {
        print current
      }
      current = $0
      sub(/^\[boards\./, "", current)
      sub(/\]$/, "", current)
      is_esp = 0
      next
    }
    current != "" && /^target[[:space:]]*=/ {
      if ($0 ~ /"xtensa-esp32s3-espidf"/ || $0 ~ /"riscv32imafc-esp-espidf"/) {
        is_esp = 1
      }
    }
    END {
      if (current != "" && is_esp) {
        print current
      }
    }
  ' "$presets_file"
}

beetle_board_config_value() {
  local presets_file="${1:-}"
  local board="${2:-}"
  local key="${3:-}"
  [[ -f "$presets_file" && -n "$board" && -n "$key" ]] || return 1

  awk -v section="[boards.${board}]" -v key="$key" '
    $0 == section { found=1; next }
    found && /^\[/ { exit }
    found {
      line = $0
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", line)
      if (line ~ ("^" key "[[:space:]]*=")) {
        match(line, /"[^"]+"/)
        if (RSTART > 0) {
          print substr(line, RSTART + 1, RLENGTH - 2)
          exit
        }
      }
    }
  ' "$presets_file"
}

beetle_esp_dist_dir() {
  local root_dir="${1:-}"
  local version="${2:-}"
  [[ -n "$root_dir" && -n "$version" ]] || return 1
  printf '%s/dist/esp/%s\n' "$root_dir" "$version"
}

beetle_sha256_file() {
  local file="${1:-}"
  [[ -f "$file" ]] || return 1

  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  else
    echo "Error: need shasum or sha256sum to compute release checksums" >&2
    return 1
  fi
}

beetle_file_size_bytes() {
  local file="${1:-}"
  [[ -f "$file" ]] || return 1

  if stat -f '%z' "$file" >/dev/null 2>&1; then
    stat -f '%z' "$file"
  else
    stat -c '%s' "$file"
  fi
}

beetle_iso_utc_now() {
  date -u '+%Y-%m-%dT%H:%M:%SZ'
}

beetle_git_commit_sha() {
  local root_dir="${1:-}"
  if [[ -n "${GITHUB_SHA:-}" ]]; then
    printf '%s\n' "$GITHUB_SHA"
    return 0
  fi
  [[ -d "$root_dir/.git" || -f "$root_dir/.git" ]] || return 1
  git -C "$root_dir" rev-parse HEAD 2>/dev/null
}

beetle_git_ref_name() {
  local root_dir="${1:-}"
  if [[ -n "${GITHUB_REF_NAME:-}" ]]; then
    printf '%s\n' "$GITHUB_REF_NAME"
    return 0
  fi
  [[ -d "$root_dir/.git" || -f "$root_dir/.git" ]] || return 1
  git -C "$root_dir" symbolic-ref --quiet --short HEAD 2>/dev/null \
    || git -C "$root_dir" describe --tags --exact-match 2>/dev/null \
    || true
}

beetle_git_dirty() {
  local root_dir="${1:-}"
  [[ -d "$root_dir/.git" || -f "$root_dir/.git" ]] || return 1
  if git -C "$root_dir" diff --quiet --ignore-submodules -- \
    && git -C "$root_dir" diff --cached --quiet --ignore-submodules --; then
    printf '%s\n' '0'
  else
    printf '%s\n' '1'
  fi
}

beetle_target_mcu_from_triple() {
  local target="${1:-}"
  case "$target" in
    xtensa-esp32s3-espidf) printf '%s\n' 'esp32s3' ;;
    riscv32imafc-esp-espidf) printf '%s\n' 'esp32p4' ;;
    *) return 1 ;;
  esac
}

beetle_manifest_chip_family_from_target() {
  local target="${1:-}"
  case "$target" in
    xtensa-esp32s3-espidf) printf '%s\n' 'ESP32-S3' ;;
    riscv32imafc-esp-espidf) printf '%s\n' 'ESP32-P4' ;;
    *) return 1 ;;
  esac
}

beetle_release_board_title() {
  local board="${1:-}"
  local chip_family="${2:-}"
  local flash_size="${3:-}"
  [[ -n "$board" && -n "$chip_family" && -n "$flash_size" ]] || return 1
  printf 'Beetle %s (%s)\n' "$chip_family" "$flash_size"
}

beetle_find_esp_idf_build_dir() {
  local release_dir="${1:-}"
  [[ -d "$release_dir" ]] || return 1
  find "$release_dir/build" -path '*/out/build' -type d 2>/dev/null | sort | head -n 1
}

beetle_find_esp_release_dir() {
  local target_root="${1:-}"
  [[ -d "$target_root" ]] || return 1

  local candidate=""
  for candidate in "$target_root/release-size" "$target_root/release"; do
    [[ -d "$candidate" ]] || continue
    if [[ -f "$candidate/beetle.bin" && -f "$candidate/bootloader.bin" && -f "$candidate/partition-table.bin" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  while IFS= read -r candidate; do
    [[ -d "$candidate" ]] || continue
    if [[ -f "$candidate/beetle.bin" && -f "$candidate/bootloader.bin" && -f "$candidate/partition-table.bin" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done < <(find "$target_root" -mindepth 1 -maxdepth 1 -type d -name 'release*' 2>/dev/null | sort)

  return 1
}

beetle_flasher_args_value() {
  local flasher_args_json="${1:-}"
  local key="${2:-}"
  [[ -f "$flasher_args_json" && -n "$key" ]] || return 1
  sed -n "s/.*\"${key}\":[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p" "$flasher_args_json" | head -n 1
}

beetle_flasher_args_image_offset() {
  local flasher_args_json="${1:-}"
  local image_name="${2:-}"

  if [[ -z "$flasher_args_json" || -z "$image_name" || ! -f "$flasher_args_json" ]]; then
    return 1
  fi

  python3 - "$flasher_args_json" "$image_name" <<'PY'
import json
import re
import sys

path, image_name = sys.argv[1], sys.argv[2]
with open(path, "r", encoding="utf-8") as handle:
    data = json.load(handle)

entry = data.get(image_name)
if not isinstance(entry, dict):
    sys.exit(1)

offset = entry.get("offset")
if not isinstance(offset, str):
    sys.exit(1)

if not re.fullmatch(r"(0[xX][0-9a-fA-F]+|[0-9]+)", offset):
    sys.exit(1)

print(offset)
PY
}
