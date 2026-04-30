#!/usr/bin/env bash

beetle_flash_port_rank_for_chip() {
  local chip="${1:-}"
  local port="${2:-}"

  case "$chip" in
    esp32p4)
      case "$port" in
        */cu.wchusbserial*) printf '%s\n' 0 ;;
        */cu.usbserial*|*/cu.SLAB*|*/cu.UART*) printf '%s\n' 1 ;;
        */cu.usbmodem*) printf '%s\n' 3 ;;
        *) printf '%s\n' 100 ;;
      esac
      ;;
    *)
      case "$port" in
        */cu.usbmodem*) printf '%s\n' 0 ;;
        */cu.usbserial*|*/cu.SLAB*|*/cu.UART*) printf '%s\n' 1 ;;
        */cu.wchusbserial*) printf '%s\n' 3 ;;
        *) printf '%s\n' 100 ;;
      esac
      ;;
  esac
}

beetle_preferred_flash_port_for_chip() {
  local chip="${1:-}"
  shift || true

  local port rank best_rank=999 best_port="" tie=0
  for port in "$@"; do
    [[ -n "$port" ]] || continue
    rank="$(beetle_flash_port_rank_for_chip "$chip" "$port")"
    if (( rank < best_rank )); then
      best_rank="$rank"
      best_port="$port"
      tie=0
    elif (( rank == best_rank )); then
      tie=1
    fi
  done

  if [[ -z "$best_port" || "$best_rank" -ge 100 || "$tie" -eq 1 ]]; then
    return 1
  fi
  printf '%s\n' "$best_port"
}

beetle_full_erase_transport_for_chip() {
  local chip="${1:-}"
  case "$chip" in
    esp32p4) printf '%s\n' 'esptool' ;;
    *) printf '%s\n' 'espflash' ;;
  esac
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

beetle_espflash_connection_profiles() {
  local chip="${1:-}"
  local subcommand="${2:-}"
  case "$chip" in
    esp32p4)
      case "$subcommand" in
        write-bin-app)
          printf '%s\n' '--before default-reset --after hard-reset'
          printf '%s\n' '--before default-reset --after hard-reset --no-stub'
          printf '%s\n' '--before no-reset --after hard-reset --no-stub'
          ;;
        erase-flash|erase-parts|erase-region)
          printf '%s\n' '--before default-reset --after no-reset'
          printf '%s\n' '--before no-reset --after no-reset'
          ;;
        *)
          printf '%s\n' '--before default-reset --after no-reset'
          printf '%s\n' '--before default-reset --after no-reset --no-stub'
          printf '%s\n' '--before no-reset --after no-reset --no-stub'
          ;;
      esac
      ;;
    esp32s3)
      case "$subcommand" in
        monitor)
          printf '%s\n' '--before no-reset --after no-reset --non-interactive --no-reset'
          ;;
        *)
          printf '%s\n' '--before default-reset --after no-reset'
          printf '%s\n' '--before usb-reset --after no-reset'
          printf '%s\n' '--before default-reset --after no-reset --no-stub'
          printf '%s\n' '--before no-reset --after no-reset --no-stub'
          ;;
      esac
      ;;
    *)
      case "$subcommand" in
        monitor)
          printf '%s\n' '--before no-reset --after no-reset --non-interactive --no-reset'
          ;;
        *)
          printf '%s\n' '--before default-reset --after no-reset'
          ;;
      esac
      ;;
  esac
}
