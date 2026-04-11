#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
PROJECT_DIR="$ROOT_DIR/third_party/esp-hosted-mcu/slave"
BUILD_DIR="$ROOT_DIR/target/esp-hosted-c6/build"
SDKCONFIG_PATH="$ROOT_DIR/target/esp-hosted-c6/sdkconfig"
SDKCONFIG_DEFAULTS_CHAIN=(
  "$PROJECT_DIR/sdkconfig.defaults"
  "$PROJECT_DIR/sdkconfig.defaults.esp32c6"
  "$ROOT_DIR/sdkconfig.defaults.esp32c6.hosted_p4_function_board"
)
SDKCONFIG_DEFAULTS="$(IFS=';'; printf '%s' "${SDKCONFIG_DEFAULTS_CHAIN[*]}")"

usage() {
  cat <<'EOF'
Usage:
  ./build.sh build-c6
  ./build.sh flash-c6
  ./build.sh flash-all [P4 build args...]

Environment:
  ESP_HOSTED_C6_PORT   Optional. Serial port for the on-board C6.
  ESPFLASH_PORT        Optional. Serial port for the P4 main firmware (used by build.sh --flash).

Notes:
  - C6 flashing follows Espressif's hosted slave flow and uses the vendored
    esp-hosted-mcu slave project.
  - Before flashing the on-board C6, put the P4 into bootloader mode so it does
    not interfere with the shared on-board wiring.
EOF
}

ensure_project_present() {
  [[ -f "$PROJECT_DIR/CMakeLists.txt" ]] || {
    echo "Error: vendored esp-hosted-mcu slave project not found at $PROJECT_DIR" >&2
    exit 1
  }
}

set_esp_path() {
  local candidates=()
  local f

  candidates+=(
    "$HOME/export-esp.sh"
    "$HOME/.espup/export-esp.sh"
    "$HOME/.local/share/esp-rs/export-esp.sh"
  )
  if [[ -n "${IDF_PATH:-}" ]]; then
    candidates+=("$IDF_PATH/export.sh")
  fi
  candidates+=(
    "$HOME/esp/esp-idf/export.sh"
    "$HOME/.espressif/esp-idf/export.sh"
    "$HOME/.espressif/esp-idf-v6.0/export.sh"
  )

  local old_nullglob
  old_nullglob="$(shopt -p nullglob || true)"
  shopt -s nullglob
  candidates+=(
    "$HOME/.espressif/esp-idf-v"*/export.sh
    "$HOME/esp/"*/esp-idf/export.sh
  )
  eval "$old_nullglob"

  for f in "${candidates[@]}"; do
    [[ -f "$f" ]] || continue
    # shellcheck source=/dev/null
    source "$f" >/dev/null 2>&1 || continue
    if command -v idf.py >/dev/null 2>&1; then
      return
    fi
  done
}

ensure_idf_py() {
  set_esp_path
  command -v idf.py >/dev/null 2>&1 || {
    echo "Error: idf.py not found after loading ESP environment." >&2
    echo "Install/source ESP-IDF first, then rerun." >&2
    exit 1
  }
}

run_idf() {
  ensure_project_present
  ensure_idf_py
  mkdir -p "$(dirname "$SDKCONFIG_PATH")"
  (
    cd "$PROJECT_DIR"
    IDF_TARGET=esp32c6 \
    SDKCONFIG_DEFAULTS="$SDKCONFIG_DEFAULTS" \
    idf.py -B "$BUILD_DIR" \
      -DIDF_TARGET=esp32c6 \
      -DSDKCONFIG="$SDKCONFIG_PATH" \
      -DSDKCONFIG_DEFAULTS="$SDKCONFIG_DEFAULTS" \
      "$@"
  )
}

print_artifacts() {
  echo "========== ESP-Hosted C6 artifacts =========="
  echo "  Project:          $PROJECT_DIR"
  echo "  Build dir:        $BUILD_DIR"
  echo "  SDKCONFIG:        $SDKCONFIG_PATH"
  echo "  Defaults chain:   $SDKCONFIG_DEFAULTS"
  echo "  App binary:       $BUILD_DIR/network_adapter.bin"
  echo "  Bootloader:       $BUILD_DIR/bootloader/bootloader.bin"
  echo "  Partition table:  $BUILD_DIR/partition_table/partition-table.bin"
  echo ""
}

list_serial_ports() {
  local ports=()
  local p
  for p in /dev/cu.* /dev/tty.usb* /dev/ttyUSB* /dev/ttyACM* /dev/serial/by-id/*; do
    [[ -e "$p" ]] && ports+=("$p")
  done
  printf '%s\n' "${ports[@]}" | awk '!seen[$0]++'
}

choose_c6_port() {
  local ports=()
  local port
  if [[ -n "${ESP_HOSTED_C6_PORT:-}" ]]; then
    echo "$ESP_HOSTED_C6_PORT"
    return 0
  fi

  while IFS= read -r port; do
    [[ -n "$port" ]] && ports+=("$port")
  done < <(list_serial_ports)
  if [[ ${#ports[@]} -eq 0 ]]; then
    echo "Error: no serial ports found. Set ESP_HOSTED_C6_PORT=/dev/tty..." >&2
    exit 1
  fi
  if [[ ${#ports[@]} -eq 1 ]]; then
    echo "${ports[0]}"
    return 0
  fi

  echo "Detected serial ports for C6 flashing:" >&2
  local i
  for ((i = 0; i < ${#ports[@]}; i++)); do
    printf '  %d) %s\n' "$((i + 1))" "${ports[$i]}" >&2
  done
  while true; do
    read -r -p "Select C6 port [1-${#ports[@]}]: " choice
    if [[ "$choice" =~ ^[0-9]+$ ]] && (( choice >= 1 && choice <= ${#ports[@]} )); then
      echo "${ports[$((choice - 1))]}"
      return 0
    fi
    echo "Invalid selection." >&2
  done
}

print_c6_flash_notice() {
  cat <<'EOF'
Before flashing the on-board ESP32-C6:
  1. Connect the programmer/UART adapter to the board's PROG_C6 header.
  2. Put the ESP32-P4 into bootloader mode so it does not interfere with the C6 bus.
EOF
  echo ""
}

cmd="${1:-}"
next_arg="${2:-}"
case "$next_arg" in
  -h|--help|help)
    usage
    exit 0
    ;;
esac
case "$cmd" in
  ""|-h|--help|help)
    usage
    exit 0
    ;;
  build-c6)
    shift
    run_idf build "$@"
    print_artifacts
    ;;
  flash-c6)
    shift
    print_c6_flash_notice
    run_idf build
    c6_port="$(choose_c6_port)"
    echo "Flashing ESP32-C6 on port: $c6_port"
    run_idf -p "$c6_port" flash
    print_artifacts
    ;;
  flash-all)
    shift
    "$0" flash-c6
    echo "========== Flashing Beetle P4 main firmware =========="
    BOARD=esp32-p4-nano-16mb "$ROOT_DIR/build.sh" --flash "$@"
    ;;
  *)
    echo "Error: unknown C6 hosted subcommand: $cmd" >&2
    usage >&2
    exit 1
    ;;
esac
