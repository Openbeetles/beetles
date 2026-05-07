#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  scripts/esp_live_test_flow.sh --scenario NAME [options]

Scenarios:
  boot_idle       Flash update, hard-reset, capture boot/idle serial log.
  qq_text        Flash update, hard-reset, capture while 10 QQ text/Markdown messages are sent.
  chat_stream    Flash update, hard-reset, run /api/sessions SSE chat smoke, capture serial log.

Options:
  --port DEVICE              Serial port. Defaults to ESPFLASH_PORT or auto-detect.
  --board BOARD              build.sh board preset. Defaults to BOARD or esp32-s3-16mb.
  --chip CHIP                espflash chip. Defaults from board.
  --baud BAUD                Monitor baud. Default: 115200.
  --duration SECONDS         Capture duration. Default: 180. Use 0 for manual Ctrl-C.
  --expected-messages COUNT  Required inbound count for qq_text. Default: 10.
  --flash-mode MODE          update or full-erase. Default: update.
  --qq-acceptance-file FILE  Required semantic acceptance evidence for qq_text.
  --device-url URL           Required for chat_stream. Defaults to BEETLE_DEVICE_URL.
  --pairing-code CODE        Required for chat_stream. Defaults to BEETLE_PAIRING_CODE.
  --chat-message TEXT        Message for chat_stream smoke.
  --chat-timeout SECONDS     HTTP stream timeout for chat_stream. Default: 90.
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

print_qq_acceptance_plan() {
  local board_name="$1"
  local baseline_clause
  case "$board_name" in
    esp32-s3-*) baseline_clause="confirm this S3 run is valid performance-baseline evidence." ;;
    *) baseline_clause="state this run is compatibility evidence, not the S3 performance baseline." ;;
  esac
  cat >&2 <<'EOF'
QQ acceptance plan:
EOF
  cat >&2 <<EOF
  A. Board/resource identity: ask for chip/board, WiFi state, pressure, and $baseline_clause
EOF
  cat >&2 <<'EOF'
  B. Resource/channel probe: ask for pressure, TLS fragmentation, QQ WSS online state, and queue depth.
  C. Wall-clock probe: ask for current date/time, timezone surface, and sync credibility.
  D. Reminder write-back: set a 45-second reminder and verify the bot sends the due reminder without manual prompting.
  E. Capability boundary: ask for an unavailable hardware/audio/display capability and verify it reports unavailable instead of fabricating success.
  F. Multi-turn tool mix: run at least two follow-up questions that require fresh status/tool reads, not fixed numeric echoes.
  G. Markdown document rendering: ask for a compact status report containing headings, bullets, a table, and a short code block; verify QQ shows readable headings/lists and does not expose raw table pipes or code fences.
EOF
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# shellcheck source=scripts/build_flash_strategy.sh
source "$SCRIPT_DIR/build_flash_strategy.sh"

scenario=""
port="${ESPFLASH_PORT:-}"
board="${BOARD:-esp32-s3-16mb}"
chip=""
chip_explicit=0
baud="115200"
duration="180"
expected_messages="10"
flash_mode="update"
qq_acceptance_file=""
device_url="${BEETLE_DEVICE_URL:-}"
pairing_code="${BEETLE_PAIRING_CODE:-}"
chat_message="Beetle P1-2 stream smoke. Reply with OK."
chat_timeout="90"
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
    --board)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --board requires a value." >&2; exit 2; }
      board="$1"
      ;;
    --chip)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --chip requires a value." >&2; exit 2; }
      chip="$1"
      chip_explicit=1
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
    --flash-mode)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --flash-mode requires a value." >&2; exit 2; }
      flash_mode="$1"
      ;;
    --qq-acceptance-file)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --qq-acceptance-file requires a value." >&2; exit 2; }
      qq_acceptance_file="$1"
      ;;
    --device-url)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --device-url requires a value." >&2; exit 2; }
      device_url="$1"
      ;;
    --pairing-code)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --pairing-code requires a value." >&2; exit 2; }
      pairing_code="$1"
      ;;
    --chat-message)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --chat-message requires a value." >&2; exit 2; }
      chat_message="$1"
      ;;
    --chat-timeout)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --chat-timeout requires a value." >&2; exit 2; }
      chat_timeout="$1"
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
  boot_idle|qq_text|chat_stream) ;;
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
[[ "$chat_timeout" =~ ^[0-9]+$ ]] || {
  echo "Error: --chat-timeout must be an integer." >&2
  exit 2
}
if [[ "$scenario" == "chat_stream" ]]; then
  [[ "$duration" -gt 0 ]] || { echo "Error: chat_stream requires --duration > 0." >&2; exit 2; }
  [[ -n "$device_url" ]] || { echo "Error: chat_stream requires --device-url or BEETLE_DEVICE_URL." >&2; exit 2; }
  [[ -n "$pairing_code" ]] || { echo "Error: chat_stream requires --pairing-code or BEETLE_PAIRING_CODE." >&2; exit 2; }
fi
case "$flash_mode" in
  update|full-erase) ;;
  *)
    echo "Error: --flash-mode must be update or full-erase." >&2
    exit 2
    ;;
esac
chip_for_board() {
  local board_name="$1"
  case "$board_name" in
    esp32-p4-*) printf '%s\n' 'esp32p4' ;;
    esp32-s3-*) printf '%s\n' 'esp32s3' ;;
    *)
      echo "Error: unsupported --board: $board_name" >&2
      echo "Known live-flow boards: esp32-s3-8mb, esp32-s3-16mb, esp32-s3-32mb, esp32-p4-nano-16mb" >&2
      exit 2
      ;;
  esac
}

board_chip="$(chip_for_board "$board")"
if [[ -z "$chip" ]]; then
  chip="$board_chip"
elif [[ "$chip_explicit" -eq 1 && "$chip" != "$board_chip" ]]; then
  echo "Error: --board $board requires --chip $board_chip, got $chip." >&2
  exit 2
fi

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

write_qq_acceptance_template() {
  local file="$1"
  [[ -n "$file" ]] || return 0
  mkdir -p "$(dirname "$file")"
  if [[ -e "$file" ]]; then
    return 0
  fi
  {
    echo '# Set every field to pass after checking the real QQ client rendering and behavior.'
    echo 'QQ_SEMANTIC_ACCEPTANCE=pending'
    echo 'board_resource_identity=pending'
    echo 'resource_channel_probe=pending'
    echo 'wall_clock_probe=pending'
    echo 'reminder_write_back=pending'
    echo 'capability_boundary=pending'
    echo 'multi_turn_tool_mix=pending'
    echo 'markdown_document_rendering=pending'
    echo 'notes='
  } > "$file"
}

require_qq_acceptance() {
  local file="$1"
  local key
  [[ -f "$file" ]] || {
    echo "Gate failed: QQ semantic acceptance file not found: $file" >&2
    exit 1
  }
  for key in \
    QQ_SEMANTIC_ACCEPTANCE \
    board_resource_identity \
    resource_channel_probe \
    wall_clock_probe \
    reminder_write_back \
    capability_boundary \
    multi_turn_tool_mix \
    markdown_document_rendering
  do
    if ! grep -Eq "^${key}=pass([[:space:]]*(#.*)?)?$" "$file"; then
      echo "Gate failed: QQ semantic acceptance missing ${key}=pass in $file" >&2
      exit 1
    fi
  done
}

json_escape() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\n'/ }"
  printf '%s\n' "$value"
}

fetch_csrf_token() {
  local url="$1"
  local output="$2"
  curl -fsS --max-time 15 "$url/api/csrf_token" > "$output"
  sed -n 's/.*"csrf_token"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$output" | tail -n 1
}

run_chat_stream_smoke() {
  local url="${device_url%/}"
  local csrf_response="$run_dir/chat_csrf.json"
  local stream_response="$run_dir/chat_stream.sse"
  local history_response="$run_dir/chat_history.json"
  local csrf escaped_message body

  csrf="$(fetch_csrf_token "$url" "$csrf_response")"
  [[ -n "$csrf" ]] || {
    echo "Gate failed: chat_stream could not fetch CSRF token from $url" >&2
    exit 1
  }
  escaped_message="$(json_escape "$chat_message")"
  body="{\"chat_id\":\"configure-ui:default\",\"content\":\"$escaped_message\"}"

  curl -fsS -N --max-time "$chat_timeout" \
    -X POST "$url/api/sessions" \
    -H 'Accept: text/event-stream' \
    -H 'Content-Type: application/json' \
    -H "X-Pairing-Code: $pairing_code" \
    -H "X-CSRF-Token: $csrf" \
    --data "$body" \
    > "$stream_response"

  require_matches '^event: queued$' "$stream_response" \
    "chat_stream did not emit queued SSE event"
  require_matches '^event: final$' "$stream_response" \
    "chat_stream did not emit final SSE event"
  require_matches '^event: done$' "$stream_response" \
    "chat_stream did not emit done SSE event"
  fail_if_matches '^event: error$' "$stream_response" \
    "chat_stream emitted error SSE event"

  curl -fsS --max-time 15 \
    -H "X-Pairing-Code: $pairing_code" \
    "$url/api/sessions?chat_id=configure-ui:default&limit=4" \
    > "$history_response"
  require_matches '"items"[[:space:]]*:' "$history_response" \
    "chat_stream history response missing items"
  require_matches '"message_id"[[:space:]]*:' "$history_response" \
    "chat_stream history response missing stable message_id"
  require_matches '"content"[[:space:]]*:' "$history_response" \
    "chat_stream history response missing content"
}

require_chat_stream_smoke() {
  [[ -f "$run_dir/chat_stream.sse" ]] || {
    echo "Gate failed: chat_stream SSE evidence not found: $run_dir/chat_stream.sse" >&2
    exit 1
  }
  [[ -f "$run_dir/chat_history.json" ]] || {
    echo "Gate failed: chat_stream history evidence not found: $run_dir/chat_history.json" >&2
    exit 1
  }
  require_matches '^event: final$' "$run_dir/chat_stream.sse" \
    "chat_stream final event missing from saved evidence"
  require_matches '^event: done$' "$run_dir/chat_stream.sse" \
    "chat_stream done event missing from saved evidence"
}

fail_on_unexpected_monitor_stderr() {
  local stderr_file="$1"
  local filtered_file="$2"
  [[ -s "$stderr_file" ]] || return 0
  grep -Ev 'BrokenPipe|Broken pipe' "$stderr_file" > "$filtered_file" || true
  if [[ -s "$filtered_file" ]]; then
    echo "Gate failed: monitor stderr contained unexpected output" >&2
    cat "$filtered_file" >&2
    exit 1
  fi
}

run_gates() {
  local log_file="$1"

  require_matches 'firmware_identity' "$log_file" "firmware identity not found"
  require_matches 'partition_layout_mismatch=false' "$log_file" "partition layout was not verified"
  if [[ "$scenario" == "qq_text" ]]; then
    require_matches 'STA connected|sta ip:' "$log_file" "STA did not connect"
    require_matches '\[qq_ws\] hello ok' "$log_file" "QQ WSS did not reach hello ok"
  else
    require_matches 'WiFi ready \(SoftAP bootstrap active|STA connected|sta ip:' "$log_file" \
      "boot_idle did not reach SoftAP or STA WiFi readiness"
  fi
  require_matches 'HEARTBEAT version=' "$log_file" "heartbeat not captured"
  require_matches '\[heartbeat\] metrics .*storage_ops=' "$log_file" "storage metrics not captured"

  fail_if_matches 'Guru Meditation|Task watchdog|TWDT|panic|abort|stack canary|Failed to create task|RTC_SW_CPU_RST' \
    "$log_file" "fatal runtime/reset pattern found"
  fail_if_matches '\[heartbeat\] metrics .*spiffs_' \
    "$log_file" "legacy storage metric names found"
  fail_if_matches 'storage_contention=Critical' \
    "$log_file" "critical storage contention found"
  fail_if_matches 'wifi:state: run -> init|wifi_reconn=[1-9][0-9]*|wifi_ap_restart=[1-9][0-9]*' \
    "$log_file" "WiFi disconnect/restart pattern found"
  fail_if_matches 'missing msg_id for QQ v2 passive reply|message dropped after send attempts|defer limit reached.*dropping message' \
    "$log_file" "message delivery drop pattern found"
  fail_if_matches '40054005|消息被去重|msgseq' \
    "$log_file" "QQ msg_seq dedupe failure found"
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
  if [[ "$scenario" == "chat_stream" ]]; then
    require_chat_stream_smoke
    require_matches '\[chat_stream\] event=final' "$log_file" \
      "chat_stream final event was not captured in serial log"
    fail_if_matches '\[chat_stream\] event=error' "$log_file" \
      "chat_stream error event found in serial log"
  fi
}

selected_port="$(detect_port)"
assert_port_free "$selected_port"

run_id="$(date -u +%Y%m%dT%H%M%SZ)-$$-$scenario"
run_dir="$output_dir/$run_id"
log_file="$run_dir/serial.log"
monitor_stderr="$run_dir/monitor.stderr"
monitor_unexpected_stderr="$run_dir/monitor.unexpected.stderr"
if [[ "$scenario" == "qq_text" && -z "$qq_acceptance_file" ]]; then
  qq_acceptance_file="$run_dir/qq_acceptance.env"
fi
mkdir -p "$run_dir"

{
  echo "run_id=$run_id"
  echo "scenario=$scenario"
  echo "board=$board"
  echo "chip=$chip"
  echo "baud=$baud"
  echo "duration_seconds=$duration"
  echo "expected_messages=$expected_messages"
  echo "flash_mode=$flash_mode"
  echo "selected_port=$selected_port"
  echo "monitor_stderr=$monitor_stderr"
  echo "monitor_unexpected_stderr=$monitor_unexpected_stderr"
  if [[ "$scenario" == "chat_stream" ]]; then
    echo "device_url=$device_url"
    echo "chat_timeout=$chat_timeout"
    echo "chat_stream_sse=$run_dir/chat_stream.sse"
    echo "chat_history_response=$run_dir/chat_history.json"
  fi
  if [[ -n "$qq_acceptance_file" ]]; then
    echo "qq_acceptance_file=$qq_acceptance_file"
  fi
  echo "created_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$run_dir/metadata.env"

cat > "$run_dir/commands.md" <<EOF
# Reproduce ESP live test flow

EOF
board_prefix=""
if [[ -n "$board" ]]; then
  board_prefix="BOARD=$board "
fi
if [[ "$flash_mode" == "full-erase" ]]; then
  cat >> "$run_dir/commands.md" <<EOF
${board_prefix}BEETLE_FLASH_MODE=full-erase TARGET=esp ESPFLASH_PORT=$selected_port ./build.sh --flash --no-monitor
EOF
else
  cat >> "$run_dir/commands.md" <<EOF
${board_prefix}TARGET=esp ESPFLASH_PORT=$selected_port ./build.sh --flash-update --no-monitor
EOF
fi
cat >> "$run_dir/commands.md" <<EOF
espflash monitor --port $selected_port --chip $chip --monitor-baud $baud --non-interactive --after hard-reset
scripts/esp_soak_analyze.sh --output-dir $run_dir/analysis $log_file
EOF
if [[ "$scenario" == "chat_stream" ]]; then
  cat >> "$run_dir/commands.md" <<EOF
curl -fsS -N --max-time $chat_timeout -X POST $device_url/api/sessions \\
  -H 'Accept: text/event-stream' \\
  -H 'Content-Type: application/json' \\
  -H 'X-Pairing-Code: <redacted>' \\
  -H 'X-CSRF-Token: <from /api/csrf_token>' \\
  --data '{"chat_id":"configure-ui:default","content":"<chat smoke message>"}'
EOF
fi

echo "ESP live flow:"
echo "  scenario: $scenario"
echo "  port: $selected_port"
echo "  log: $log_file"

echo
echo "Step 1/5: serial port selected and free."

echo
echo "Step 2/5: flashing with ./build.sh ($flash_mode)."
if [[ "$flash_mode" == "full-erase" ]]; then
  if [[ -n "$board" ]]; then
    BOARD="$board" BEETLE_FLASH_MODE=full-erase TARGET=esp ESPFLASH_PORT="$selected_port" "$REPO_ROOT/build.sh" --flash --no-monitor
  else
    BEETLE_FLASH_MODE=full-erase TARGET=esp ESPFLASH_PORT="$selected_port" "$REPO_ROOT/build.sh" --flash --no-monitor
  fi
else
  if [[ -n "$board" ]]; then
    BOARD="$board" TARGET=esp ESPFLASH_PORT="$selected_port" "$REPO_ROOT/build.sh" --flash-update --no-monitor
  else
    TARGET=esp ESPFLASH_PORT="$selected_port" "$REPO_ROOT/build.sh" --flash-update --no-monitor
  fi
fi

echo
echo "Step 3/5: rechecking serial port after flash."
wait_for_port "$selected_port"
assert_port_free "$selected_port"

echo
echo "Step 4/5: hard-reset monitor from boot."
echo "For qq_text, send the QQ test messages only after '[qq_ws] hello ok' appears."
if [[ "$scenario" == "chat_stream" ]]; then
  echo "For chat_stream, the flow waits for WiFi readiness, then posts /api/sessions SSE via curl."
fi
if [[ "$scenario" == "qq_text" ]]; then
  print_qq_acceptance_plan "$board"
  write_qq_acceptance_template "$qq_acceptance_file"
  echo "QQ semantic acceptance file: $qq_acceptance_file"
fi
if [[ "$duration" -eq 0 ]]; then
  espflash monitor --port "$selected_port" --chip "$chip" --monitor-baud "$baud" --non-interactive --after hard-reset 2>"$monitor_stderr" | tee "$log_file"
else
  espflash monitor --port "$selected_port" --chip "$chip" --monitor-baud "$baud" --non-interactive --after hard-reset 2>"$monitor_stderr" | tee "$log_file" &
  monitor_pid="$!"
  chat_stream_done=0
  for (( elapsed = 0; elapsed < duration; elapsed++ )); do
    sleep 1
    if [[ "$scenario" == "qq_text" ]] && qq_text_completion_reached "$log_file"; then
      echo "qq_text expected message/reply metrics reached; ending capture early."
      break
    fi
    if [[ "$scenario" == "chat_stream" && "$chat_stream_done" -eq 0 ]] \
      && grep -E 'WiFi ready \(SoftAP bootstrap active|STA connected|sta ip:' "$log_file" >/dev/null 2>&1; then
      echo "chat_stream WiFi readiness reached; running /api/sessions SSE smoke."
      run_chat_stream_smoke
      chat_stream_done=1
      echo "chat_stream SSE smoke reached final/done and history readback; ending capture early."
      break
    fi
  done
  kill "$monitor_pid" 2>/dev/null || true
  wait "$monitor_pid" 2>/dev/null || true
fi
fail_on_unexpected_monitor_stderr "$monitor_stderr" "$monitor_unexpected_stderr"
if [[ "$scenario" == "qq_text" ]]; then
  require_qq_acceptance "$qq_acceptance_file"
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
