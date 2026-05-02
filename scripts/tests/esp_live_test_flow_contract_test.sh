#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FLOW="$ROOT_DIR/scripts/esp_live_test_flow.sh"

assert_contains() {
  local file="$1"
  local needle="$2"
  local message="$3"
  if ! grep -F "$needle" "$file" >/dev/null; then
    echo "FAIL: $message" >&2
    echo "missing: $needle" >&2
    exit 1
  fi
}

assert_contains "$FLOW" 'beetle_preferred_flash_port_for_chip "$chip"' \
  "live flow must select ports through the shared flash strategy"
assert_contains "$FLOW" 'lsof "$selected" "$sibling"' \
  "live flow must reject busy cu/tty serial aliases before flashing"
assert_contains "$FLOW" '"$REPO_ROOT/build.sh" --flash-update --no-monitor' \
  "live flow must flash through build.sh without opening build.sh monitor"
assert_contains "$FLOW" 'wait_for_port "$selected_port"' \
  "live flow must rediscover the serial port after flashing"
assert_contains "$FLOW" 'espflash monitor --port "$selected_port" --chip "$chip" --monitor-baud "$baud" --non-interactive --after hard-reset' \
  "live flow must hard-reset and monitor from boot"
assert_contains "$FLOW" "For qq_text, send the QQ test messages only after '[qq_ws] hello ok' appears." \
  "live flow must gate manual QQ testing on WSS readiness"
assert_contains "$FLOW" 'qq_text_completion_reached "$log_file"' \
  "live flow must stop qq_text captures after message and reply metrics close"
assert_contains "$FLOW" 'qq_text expected message/reply metrics reached; ending capture early.' \
  "live flow must report early qq_text completion before analysis"
assert_contains "$FLOW" 'partition_layout_mismatch=false' \
  "live flow must require partition identity in serial logs"
assert_contains "$FLOW" 'missing msg_id for QQ v2 passive reply|message dropped after send attempts' \
  "live flow must fail on QQ passive-reply anchor drops"
assert_contains "$FLOW" 'dispatch_fail=[1-9][0-9]*|err_dispatch=[1-9][0-9]*' \
  "live flow must fail when dispatch error metrics increase"
assert_contains "$FLOW" 'tool_err=[1-9][0-9]*|tool_protocol_violation=[1-9][0-9]*' \
  "live flow must fail when tool execution/protocol metrics increase"

echo "esp_live_test_flow_contract_test: ok"
