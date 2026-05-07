#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FLOW="$ROOT_DIR/scripts/esp_live_test_flow.sh"

assert_contains() {
  local file="$1"
  local needle="$2"
  local message="$3"
  if ! grep -F -- "$needle" "$file" >/dev/null; then
    echo "FAIL: $message" >&2
    echo "missing: $needle" >&2
    exit 1
  fi
}

assert_not_contains() {
  local file="$1"
  local needle="$2"
  local message="$3"
  if grep -F -- "$needle" "$file" >/dev/null; then
    echo "FAIL: $message" >&2
    echo "unexpected legacy backend wording" >&2
    exit 1
  fi
}

assert_contains "$FLOW" 'beetle_preferred_flash_port_for_chip "$chip"' \
  "live flow must select ports through the shared flash strategy"
assert_contains "$FLOW" 'lsof "$selected" "$sibling"' \
  "live flow must reject busy cu/tty serial aliases before flashing"
assert_contains "$FLOW" 'board="${BOARD:-esp32-s3-16mb}"' \
  "live flow must default to an explicit S3 board preset"
assert_contains "$FLOW" '--board)' \
  "live flow must allow the board preset to be passed explicitly"
assert_contains "$FLOW" 'chip_for_board()' \
  "live flow must infer the espflash chip from the board preset"
assert_contains "$FLOW" 'esp32-p4-*) printf' \
  "live flow must map P4 board presets to esp32p4"
assert_contains "$FLOW" 'esp32-s3-*) printf' \
  "live flow must map S3 board presets to esp32s3"
assert_contains "$FLOW" 'requires --chip' \
  "live flow must reject board/chip mismatches instead of guessing"
assert_contains "$FLOW" 'flash_mode="update"' \
  "live flow must default to non-erasing update flashes"
assert_contains "$FLOW" 'update|full-erase)' \
  "live flow must allow full erase only as an explicit flash mode"
assert_contains "$FLOW" '--qq-acceptance-file FILE' \
  "qq_text flow must accept an explicit semantic acceptance evidence file"
assert_contains "$FLOW" '--device-url URL' \
  "chat_stream flow must accept a device URL for HTTP smoke"
assert_contains "$FLOW" '--pairing-code CODE' \
  "chat_stream flow must accept a pairing code for HTTP smoke"
assert_contains "$FLOW" 'TARGET=esp ESPFLASH_PORT="$selected_port" "$REPO_ROOT/build.sh" --flash-update --no-monitor' \
  "live flow must flash through build.sh update mode without opening build.sh monitor"
assert_contains "$FLOW" 'BOARD="$board" TARGET=esp ESPFLASH_PORT="$selected_port" "$REPO_ROOT/build.sh" --flash-update --no-monitor' \
  "live flow must pass explicit board presets through update flashes"
assert_contains "$FLOW" 'BEETLE_FLASH_MODE=full-erase TARGET=esp ESPFLASH_PORT="$selected_port" "$REPO_ROOT/build.sh" --flash --no-monitor' \
  "live flow must keep full erase behind an explicit flash mode"
assert_contains "$FLOW" 'wait_for_port "$selected_port"' \
  "live flow must rediscover the serial port after flashing"
assert_contains "$FLOW" 'espflash monitor --port "$selected_port" --chip "$chip" --monitor-baud "$baud" --non-interactive --after hard-reset' \
  "live flow must hard-reset and monitor from boot"
assert_contains "$FLOW" "For qq_text, send the QQ test messages only after '[qq_ws] hello ok' appears." \
  "live flow must gate manual QQ testing on WSS readiness"
assert_contains "$FLOW" 'print_qq_acceptance_plan' \
  "live flow must print the semantic QQ acceptance plan instead of encouraging numeric echo tests"
assert_contains "$FLOW" 'baseline_clause=' \
  "QQ acceptance plan must distinguish S3 baseline evidence from P4 compatibility evidence"
assert_contains "$FLOW" 'not the S3 performance baseline' \
  "QQ acceptance plan must not treat P4 compatibility as the S3 baseline"
assert_contains "$FLOW" 'Reminder write-back' \
  "QQ acceptance plan must cover due reminder write-back and outbound delivery"
assert_contains "$FLOW" 'Multi-turn tool mix' \
  "QQ acceptance plan must cover mixed follow-up/tool-read scenarios"
assert_contains "$FLOW" 'Markdown document rendering' \
  "QQ acceptance plan must cover Markdown document rendering instead of only plain text"
assert_contains "$FLOW" 'raw table pipes or code fences' \
  "QQ Markdown acceptance must check table/code-fence readability in the actual client"
assert_contains "$FLOW" 'qq_text_completion_reached "$log_file"' \
  "live flow must stop qq_text captures after message and reply metrics close"
assert_contains "$FLOW" 'qq_text expected message/reply metrics reached; ending capture early.' \
  "live flow must report early qq_text completion before analysis"
assert_contains "$FLOW" 'partition_layout_mismatch=false' \
  "live flow must require partition identity in serial logs"
assert_contains "$FLOW" 'WiFi ready \(SoftAP bootstrap active|STA connected|sta ip:' \
  "boot_idle must accept SoftAP readiness without requiring STA credentials"
assert_contains "$FLOW" 'if [[ "$scenario" == "qq_text" ]]; then' \
  "qq_text-specific gates must remain isolated from fresh boot_idle"
assert_contains "$FLOW" 'run_chat_stream_smoke' \
  "chat_stream flow must run the /api/sessions SSE smoke"
assert_contains "$FLOW" 'curl -fsS -N --max-time "$chat_timeout"' \
  "chat_stream flow must use curl streaming mode with an HTTP timeout"
assert_contains "$FLOW" '-X POST "$url/api/sessions"' \
  "chat_stream flow must post to /api/sessions instead of inventing /api/chat"
assert_contains "$FLOW" "require_matches '^event: final$'" \
  "chat_stream flow must require the final SSE event"
assert_contains "$FLOW" "require_matches '^event: done$'" \
  "chat_stream flow must require the done SSE event"
assert_contains "$FLOW" 'chat_history_response=$run_dir/chat_history.json' \
  "chat_stream flow metadata must preserve history readback evidence"
assert_contains "$FLOW" '\[chat_stream\] event=final' \
  "chat_stream live gate must require serial final evidence for analyzer alignment"
assert_contains "$FLOW" "require_matches 'STA connected|sta ip:'" \
  "qq_text must require STA readiness before manual QQ testing"
assert_contains "$FLOW" "require_matches '\\[qq_ws\\] hello ok'" \
  "qq_text must require QQ WSS hello before manual QQ testing"
assert_contains "$FLOW" '\[heartbeat\] metrics .*storage_ops=' \
  "live flow must require storage metrics in serial logs"
assert_contains "$FLOW" 'storage_contention=Critical' \
  "live flow must fail when critical storage contention appears"
assert_contains "$FLOW" 'legacy storage metric names found' \
  "live flow must reject legacy backend metric names without user-facing backend wording"
assert_not_contains "$FLOW" 'SPIFFS' \
  "live flow must not emit user-facing backend wording"
assert_contains "$FLOW" 'missing msg_id for QQ v2 passive reply|message dropped after send attempts' \
  "live flow must fail on QQ passive-reply anchor drops"
assert_contains "$FLOW" '40054005|消息被去重|msgseq' \
  "live flow must fail if QQ v2 msg_seq dedupe rejects active outbound"
assert_contains "$FLOW" 'dispatch_fail=[1-9][0-9]*|err_dispatch=[1-9][0-9]*' \
  "live flow must fail when dispatch error metrics increase"
assert_contains "$FLOW" 'tool_err=[1-9][0-9]*|tool_protocol_violation=[1-9][0-9]*' \
  "live flow must fail when tool execution/protocol metrics increase"
assert_not_contains "$FLOW" 'dedicated write-back worker thread started on ESP' \
  "live flow must not fail a single governed write-back lazy worker start"
assert_contains "$FLOW" 'monitor_stderr="$run_dir/monitor.stderr"' \
  "live flow must capture monitor stderr separately from serial logs"
assert_contains "$FLOW" 'monitor_unexpected_stderr="$run_dir/monitor.unexpected.stderr"' \
  "live flow must preserve unexpected monitor stderr separately"
assert_contains "$FLOW" 'BrokenPipe|Broken pipe' \
  "live flow must suppress known espflash monitor broken-pipe noise after timed captures"
assert_contains "$FLOW" "grep -Ev 'BrokenPipe|Broken pipe'" \
  "live flow must filter only known broken-pipe monitor noise"
assert_not_contains "$FLOW" 'Main thread panicked|called `Result::unwrap`' \
  "live flow must not suppress monitor panics or unwrap failures"
assert_contains "$FLOW" 'require_qq_acceptance "$qq_acceptance_file"' \
  "qq_text flow must gate on semantic acceptance evidence"
assert_contains "$FLOW" 'markdown_document_rendering=pending' \
  "QQ acceptance evidence must include Markdown document rendering"
assert_contains "$FLOW" 'fail_if_analyzer_blockers "$latest_regressions"' \
  "live flow must fail when analyzer reports blocker regressions"
assert_contains "$FLOW" 'severity == "blocker"' \
  "live flow must fail every analyzer blocker instead of allowlisting selected checks"

echo "esp_live_test_flow_contract_test: ok"
