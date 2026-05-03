#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  scripts/esp_live_test_flow.sh --scenario NAME [options]

Scenarios:
  boot_idle       Flash, hard-reset, capture boot/idle serial log.
  qq_text        Flash, hard-reset, capture while 10 QQ text messages are sent.

Options:
  --port DEVICE              Serial port. Defaults to ESPFLASH_PORT or auto-detect.
  --chip CHIP                espflash chip. Default: esp32s3.
  --baud BAUD                Monitor baud. Default: 115200.
  --duration SECONDS         Capture duration. Default: 180. Use 0 for manual Ctrl-C.
  --expected-messages COUNT  Required inbound count for qq_text. Default: 10.
  --output-dir DIR           Evidence root. Default: target/esp-live.
  -h, --help                 Show this help.

Flow:
  1. discover and verify serial port
  2. flash with ./build.sh
  3. rediscover and verify serial port
  4. hard-reset and capture boot serial log
  5. run scenario log gates
EOF
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# shellcheck source=scripts/build_flash_strategy.sh
source "$SCRIPT_DIR/build_flash_strategy.sh"

scenario=""
port="${ESPFLASH_PORT:-}"
chip="esp32s3"
baud="115200"
duration="180"
expected_messages="10"
output_dir="$REPO_ROOT/target/esp-live"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --scenario)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --scenario requires a value." >&2; exit 2; }
      scenario="$1"
      ;;
    --port)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --port requires a value." >&2; exit 2; }
      port="$1"
      ;;
    --chip)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --chip requires a value." >&2; exit 2; }
      chip="$1"
      ;;
    --baud)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --baud requires a value." >&2; exit 2; }
      baud="$1"
      ;;
    --duration)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --duration requires a value." >&2; exit 2; }
      duration="$1"
      ;;
    --expected-messages)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --expected-messages requires a value." >&2; exit 2; }
      expected_messages="$1"
      ;;
    --output-dir)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --output-dir requires a value." >&2; exit 2; }
      output_dir="$1"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      echo "Error: unknown argument: $1" >&2
      usage
      exit 2
      ;;
    *)
      echo "Error: unexpected positional argument: $1" >&2
      usage
      exit 2
      ;;
  esac
  shift
done

case "$scenario" in
  boot_idle|qq_text) ;;
  "")
    echo "Error: --scenario is required." >&2
    usage
    exit 2
    ;;
  *)
    echo "Error: unsupported scenario: $scenario" >&2
    usage
    exit 2
    ;;
esac

[[ "$duration" =~ ^[0-9]+$ ]] || { echo "Error: --duration must be an integer." >&2; exit 2; }
[[ "$expected_messages" =~ ^[0-9]+$ ]] || {
  echo "Error: --expected-messages must be an integer." >&2
  exit 2
}

add_existing_ports() {
  local pattern port_path
  for pattern in "$@"; do
    for port_path in $pattern; do
      [[ -e "$port_path" ]] || continue
      printf '%s\n' "$port_path"
    done
  done
}

detect_port() {
  if [[ -n "$port" ]]; then
    [[ -e "$port" ]] || { echo "Error: serial port not found: $port" >&2; exit 1; }
    printf '%s\n' "$port"
    return
  fi

  local candidates selected
  candidates="$(
    add_existing_ports \
      /dev/cu.usbmodem* \
      /dev/tty.usbmodem* \
      /dev/cu.usbserial* \
      /dev/tty.usbserial* \
      /dev/cu.wchusbserial* \
      /dev/tty.wchusbserial* \
      /dev/ttyUSB* \
      /dev/ttyACM* \
      | awk '!seen[$0]++'
  )"
  [[ -n "$candidates" ]] || {
    echo "Error: no serial ports found. Set ESPFLASH_PORT or pass --port." >&2
    exit 1
  }

  # Word splitting is intentional here: serial paths do not contain spaces.
  # shellcheck disable=SC2086
  if selected="$(beetle_preferred_flash_port_for_chip "$chip" $candidates)"; then
    printf '%s\n' "$selected"
    return
  fi

  echo "Error: multiple possible serial ports; pass --port explicitly:" >&2
  printf '%s\n' "$candidates" >&2
  exit 1
}

tty_sibling_for_port() {
  local value="$1"
  case "$value" in
    /dev/cu.*) printf '/dev/tty.%s\n' "${value#/dev/cu.}" ;;
    /dev/tty.*) printf '/dev/cu.%s\n' "${value#/dev/tty.}" ;;
    *) return 1 ;;
  esac
}

assert_port_free() {
  local selected="$1"
  local sibling=""
  sibling="$(tty_sibling_for_port "$selected" || true)"
  if [[ -n "$sibling" && -e "$sibling" ]]; then
    if lsof "$selected" "$sibling" >/tmp/beetle-esp-live-lsof.$$ 2>&1; then
      cat /tmp/beetle-esp-live-lsof.$$ >&2
      rm -f /tmp/beetle-esp-live-lsof.$$
      echo "Error: serial port is busy: $selected" >&2
      exit 1
    fi
  elif lsof "$selected" >/tmp/beetle-esp-live-lsof.$$ 2>&1; then
    cat /tmp/beetle-esp-live-lsof.$$ >&2
    rm -f /tmp/beetle-esp-live-lsof.$$
    echo "Error: serial port is busy: $selected" >&2
    exit 1
  fi
  rm -f /tmp/beetle-esp-live-lsof.$$
}

wait_for_port() {
  local selected="$1"
  local i
  for i in 1 2 3 4 5 6 7 8 9 10; do
    [[ -e "$selected" ]] && return 0
    sleep 1
  done
  echo "Error: serial port did not reappear after flash: $selected" >&2
  exit 1
}

count_matches() {
  local pattern="$1"
  local file="$2"
  grep -E -c "$pattern" "$file" 2>/dev/null || true
}

latest_metric_value() {
  local key="$1"
  local file="$2"
  local line value
  line="$(grep -E '\[heartbeat\] metrics ' "$file" 2>/dev/null | tail -n 1 || true)"
  if [[ -z "$line" ]]; then
    printf '0\n'
    return
  fi
  value="$(printf '%s\n' "$line" | sed -E "s/.*(^| )$key=([0-9]+).*/\\2/")"
  if [[ "$value" =~ ^[0-9]+$ ]]; then
    printf '%s\n' "$value"
  else
    printf '0\n'
  fi
}

qq_text_completion_reached() {
  local file="$1"
  local inbound_count reply_count final_answer_count user_done_count
  inbound_count="$(count_matches '\[qq_ws\] message enqueued' "$file")"
  reply_count="$(count_matches '\[agent\] reply outbound enqueued' "$file")"
  final_answer_count="$(latest_metric_value final_answer_calls "$file")"
  user_done_count="$(latest_metric_value user_done "$file")"

  (( inbound_count >= expected_messages )) \
    && (( reply_count >= expected_messages )) \
    && (( final_answer_count >= expected_messages )) \
    && (( user_done_count >= expected_messages ))
}

fail_if_matches() {
  local pattern="$1"
  local file="$2"
  local message="$3"
  if grep -E "$pattern" "$file" >/dev/null; then
    echo "Gate failed: $message" >&2
    grep -E "$pattern" "$file" >&2 || true
    exit 1
  fi
}

require_matches() {
  local pattern="$1"
  local file="$2"
  local message="$3"
  if ! grep -E "$pattern" "$file" >/dev/null; then
    echo "Gate failed: $message" >&2
    exit 1
  fi
}

fail_if_analyzer_blockers() {
  local regressions_file="$1"
  [[ -f "$regressions_file" ]] || {
    echo "Gate failed: analyzer regressions file not found: $regressions_file" >&2
    exit 1
  }
  if awk -F, '
    NR > 1 {
      severity = $3
      gsub(/^"|"$/, "", severity)
      if (severity == "blocker") {
        print
        found = 1
      }
    }
    END { exit found ? 0 : 1 }
  ' "$regressions_file" >/tmp/beetle-esp-live-regressions.$$; then
    echo "Gate failed: analyzer reported blocker regressions" >&2
    cat /tmp/beetle-esp-live-regressions.$$ >&2
    rm -f /tmp/beetle-esp-live-regressions.$$
    exit 1
  fi
  rm -f /tmp/beetle-esp-live-regressions.$$
}

run_gates() {
  local log_file="$1"

  require_matches 'firmware_identity' "$log_file" "firmware identity not found"
  require_matches 'partition_layout_mismatch=false' "$log_file" "partition layout was not verified"
  require_matches 'STA connected|sta ip:' "$log_file" "STA did not connect"
  require_matches '\[qq_ws\] hello ok' "$log_file" "QQ WSS did not reach hello ok"
  require_matches 'HEARTBEAT version=' "$log_file" "heartbeat not captured"

  fail_if_matches 'Guru Meditation|Task watchdog|TWDT|panic|abort|stack canary|Failed to create task|RTC_SW_CPU_RST' \
    "$log_file" "fatal runtime/reset pattern found"
  fail_if_matches '\[thread\] started name=write_back' \
    "$log_file" "dedicated write-back worker thread started on ESP"
  fail_if_matches 'wifi:state: run -> init|wifi_reconn=[1-9][0-9]*|wifi_ap_restart=[1-9][0-9]*' \
    "$log_file" "WiFi disconnect/restart pattern found"
  fail_if_matches 'missing msg_id for QQ v2 passive reply|message dropped after send attempts|defer limit reached.*dropping message' \
    "$log_file" "message delivery drop pattern found"
  fail_if_matches 'dispatch_fail=[1-9][0-9]*|err_dispatch=[1-9][0-9]*|outbound_enq_fail=[1-9][0-9]*|inbound_drop=[1-9][0-9]*' \
    "$log_file" "message failure metric increased"
  fail_if_matches 'tool_err=[1-9][0-9]*|tool_protocol_violation=[1-9][0-9]*' \
    "$log_file" "tool execution/protocol metric increased"

  if [[ "$scenario" == "qq_text" ]]; then
    local inbound_count reply_count
    inbound_count="$(count_matches '\[qq_ws\] message enqueued' "$log_file")"
    reply_count="$(count_matches '\[agent\] reply outbound enqueued' "$log_file")"
    if (( inbound_count < expected_messages )); then
      echo "Gate failed: qq_text expected $expected_messages inbound messages, got $inbound_count" >&2
      exit 1
    fi
    if (( reply_count < expected_messages )); then
      echo "Gate failed: qq_text expected $expected_messages primary replies, got $reply_count" >&2
      exit 1
    fi
  fi
}

selected_port="$(detect_port)"
assert_port_free "$selected_port"

run_id="$(date -u +%Y%m%dT%H%M%SZ)-$$-$scenario"
run_dir="$output_dir/$run_id"
log_file="$run_dir/serial.log"
mkdir -p "$run_dir"

{
  echo "run_id=$run_id"
  echo "scenario=$scenario"
  echo "chip=$chip"
  echo "baud=$baud"
  echo "duration_seconds=$duration"
  echo "expected_messages=$expected_messages"
  echo "selected_port=$selected_port"
  echo "created_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$run_dir/metadata.env"

cat > "$run_dir/commands.md" <<EOF
# Reproduce ESP live test flow

TARGET=esp ESPFLASH_PORT=$selected_port ./build.sh --flash-update --no-monitor
espflash monitor --port $selected_port --chip $chip --monitor-baud $baud --non-interactive --after hard-reset
scripts/esp_soak_analyze.sh --output-dir $run_dir/analysis $log_file
EOF

echo "ESP live flow:"
echo "  scenario: $scenario"
echo "  port: $selected_port"
echo "  log: $log_file"

echo
echo "Step 1/5: serial port selected and free."

echo
echo "Step 2/5: flashing with ./build.sh."
TARGET=esp ESPFLASH_PORT="$selected_port" "$REPO_ROOT/build.sh" --flash-update --no-monitor

echo
echo "Step 3/5: rechecking serial port after flash."
wait_for_port "$selected_port"
assert_port_free "$selected_port"

echo
echo "Step 4/5: hard-reset monitor from boot."
echo "For qq_text, send the QQ test messages only after '[qq_ws] hello ok' appears."
if [[ "$duration" -eq 0 ]]; then
  espflash monitor --port "$selected_port" --chip "$chip" --monitor-baud "$baud" --non-interactive --after hard-reset | tee "$log_file"
else
  espflash monitor --port "$selected_port" --chip "$chip" --monitor-baud "$baud" --non-interactive --after hard-reset | tee "$log_file" &
  monitor_pid="$!"
  for (( elapsed = 0; elapsed < duration; elapsed++ )); do
    sleep 1
    if [[ "$scenario" == "qq_text" ]] && qq_text_completion_reached "$log_file"; then
      echo "qq_text expected message/reply metrics reached; ending capture early."
      break
    fi
  done
  kill "$monitor_pid" 2>/dev/null || true
  wait "$monitor_pid" 2>/dev/null || true
fi

echo
echo "Step 5/5: analyzing and gating serial log."
bash "$SCRIPT_DIR/esp_soak_analyze.sh" --output-dir "$run_dir/analysis" "$log_file"
latest_regressions="$(find "$run_dir/analysis" -name regressions.csv -type f -print | sort | tail -n 1)"
fail_if_analyzer_blockers "$latest_regressions"
run_gates "$log_file"

echo "ESP live flow passed:"
echo "  run dir: $run_dir"
echo "  log: $log_file"
