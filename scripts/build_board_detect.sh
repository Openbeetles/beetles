#!/usr/bin/env bash

# Shared ESP board detection helpers for Beetle build scripts.

beetle_normalize_flash_size() {
  local raw="${1:-}"
  raw="${raw// /}"
  raw="$(printf '%s' "$raw" | tr '[:lower:]' '[:upper:]')"
  case "$raw" in
    8MB|16MB|32MB)
      printf '%s\n' "$raw"
      ;;
    *)
      return 1
      ;;
  esac
}

beetle_map_board_from_chip_flash() {
  local chip="${1:-}"
  local flash_raw="${2:-}"
  local flash_size

  flash_size="$(beetle_normalize_flash_size "$flash_raw")" || return 1

  case "$chip" in
    esp32p4)
      [[ "$flash_size" == "16MB" ]] || return 1
      printf '%s\n' 'esp32-p4-nano-16mb'
      ;;
    esp32s3)
      case "$flash_size" in
        8MB) printf '%s\n' 'esp32-s3-8mb' ;;
        16MB) printf '%s\n' 'esp32-s3-16mb' ;;
        32MB) printf '%s\n' 'esp32-s3-32mb' ;;
        *) return 1 ;;
      esac
      ;;
    *)
      return 1
      ;;
  esac
}

beetle_parse_board_info() {
  local input="${1:-}"
  local chip flash_size

  chip="$(printf '%s\n' "$input" | awk '/^Chip type:/ { print $3; exit }')"
  flash_size="$(printf '%s\n' "$input" | awk '/^Flash size:/ { print $3; exit }')"

  [[ -n "$chip" && -n "$flash_size" ]] || return 1
  printf '%s\t%s\n' "$chip" "$flash_size"
}
