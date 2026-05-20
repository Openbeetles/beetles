#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WAKENET_C="$ROOT_DIR/components/beetle_wakenet/beetle_wakenet.c"
WAKENET_H="$ROOT_DIR/components/beetle_wakenet/include/beetle_wakenet.h"
ESP_SR_RS="$ROOT_DIR/src/wake/esp_sr.rs"

fail() {
  echo "esp_wakenet_afe_contract_test: $*" >&2
  exit 1
}

require_contains() {
  local file="$1"
  local needle="$2"
  local message="$3"
  grep -Fq "$needle" "$file" || fail "$message"
}

require_not_contains() {
  local file="$1"
  local needle="$2"
  local message="$3"
  if grep -Fq "$needle" "$file"; then
    fail "$message"
  fi
}

require_contains "$WAKENET_H" \
  "beetle_wakenet_take_event" \
  "WakeNet C ABI must expose a nonblocking take-event entry point"

require_contains "$ESP_SR_RS" \
  "beetle_wakenet_take_event" \
  "Rust WakeNet backend must consume C-side pending detection via nonblocking take-event"

require_contains "$WAKENET_C" \
  "fetch_with_delay(ctx->afe_data, portMAX_DELAY)" \
  "WakeNet detection must use a blocking AFE fetch task like xiaozhi"

require_not_contains "$WAKENET_C" \
  "fetch_with_delay(s_ctx->afe_data, 0)" \
  "WakeNet feed path must not poll AFE with zero-delay fetch"

require_not_contains "$WAKENET_C" \
  "return poll_detection()" \
  "WakeNet feed path must not call detection polling while filling feed chunks"

require_not_contains "$WAKENET_C" \
  "== BEETLE_WN_DETECTED" \
  "beetle_wakenet_feed must not synchronously return detection from feed hot path"

echo "esp_wakenet_afe_contract_test: ok"
