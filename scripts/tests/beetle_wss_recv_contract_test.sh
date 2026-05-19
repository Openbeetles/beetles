#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

source_file="components/beetle_wss/beetle_wss.c"

require() {
  local pattern="$1"
  local message="$2"
  if ! rg -n "$pattern" "$source_file" >/dev/null; then
    echo "FAIL: $message" >&2
    exit 1
  fi
}

reject() {
  local pattern="$1"
  local message="$2"
  if rg -n "$pattern" "$source_file" >/dev/null; then
    echo "FAIL: $message" >&2
    rg -n "$pattern" "$source_file" >&2
    exit 1
  fi
}

require 'pending_frame_active' \
  "WSS recv must retain an in-flight frame across short recv_timeout windows"
require 'beetle_wss_read_pending_payload' \
  "WSS recv must read payload into the final owned target instead of staging a full frame"
require 'beetle_wss_clear_pending_frame' \
  "WSS recv must release partially owned payload state on destroy/error cleanup"
require 'beetle_wss_ensure_rx_bytes_limited' \
  "WSS recv must use bounded header reads instead of growing rx_buf toward payload size"

reject 'beetle_wss_ensure_rx_bytes\(client,[[:space:]]*header_len[[:space:]]*\+[[:space:]]*\(size_t\)[[:space:]]*payload_len' \
  "WSS recv must not wait for header+payload inside rx_buf"
reject 'out_event->data[[:space:]]*=[[:space:]]*\(uint8_t[[:space:]]*\*\)[[:space:]]*malloc\(\(size_t\)[[:space:]]*payload_len\)' \
  "WSS recv must not allocate a second event payload after staging the payload in rx_buf"
reject 'memcpy\(out_event->data,[[:space:]]*payload,' \
  "WSS recv must not copy a full staged payload into the event buffer"

echo "beetle_wss_recv_contract_test: ok"
