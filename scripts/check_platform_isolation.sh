#!/usr/bin/env bash
# §14.2：业务域不得直引 platform::spiffs / heap / hardware_drivers，不得 use crate::platform::*。
# §14.2: business modules must not import platform implementation modules or glob-import platform.
# See dev-docs/platform-isolation-plan.md §14.2.

set -euo pipefail

if ! command -v rg >/dev/null 2>&1; then
  echo "check_platform_isolation: ripgrep (rg) is required" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

DIRS=(
  src/tools
  src/orchestrator
  src/cli
  src/heartbeat
  src/channels
  src/agent
  src/bus
  src/llm
  src/memory
  src/skills
  src/cron
  src/state
)
EXISTING_DIRS=()
for dir in "${DIRS[@]}"; do
  if [[ -d "$dir" ]]; then
    EXISTING_DIRS+=("$dir")
  fi
done

PATTERN1='use\s+crate::platform::(spiffs|heap|hardware_drivers)'
PATTERN2='use\s+crate::platform::\*'
PATTERN3='esp_idf_svc::'
PATTERN4='crate::platform::is_wifi_sta_connected\s*\('
PATTERN5='crate::platform::wifi::wifi_sta_ip\s*\('

if rg -q "$PATTERN1" "${EXISTING_DIRS[@]}"; then
  echo "FAIL: forbidden direct use of platform implementation modules (spiffs|heap|hardware_drivers):" >&2
  rg "$PATTERN1" "${EXISTING_DIRS[@]}" >&2
  exit 1
fi

if rg -q "$PATTERN2" "${EXISTING_DIRS[@]}"; then
  echo "FAIL: forbidden use crate::platform::*" >&2
  rg "$PATTERN2" "${EXISTING_DIRS[@]}" >&2
  exit 1
fi

if rg -q "$PATTERN4" "${EXISTING_DIRS[@]}"; then
  echo "FAIL: forbidden direct WiFi status reads from platform helper in business domains" >&2
  rg "$PATTERN4" "${EXISTING_DIRS[@]}" >&2
  exit 1
fi

if rg -q "$PATTERN5" "${EXISTING_DIRS[@]}"; then
  echo "FAIL: forbidden direct WiFi IP reads from platform helper in business domains" >&2
  rg "$PATTERN5" "${EXISTING_DIRS[@]}" >&2
  exit 1
fi

ESP_IDF_SVC_HITS="$(rg -n "$PATTERN3" src \
  --glob '!src/platform/**' \
  --glob '!src/channels/wss_gateway/esp_conn.rs' || true)"
if [[ -n "$ESP_IDF_SVC_HITS" ]]; then
  echo "FAIL: forbidden direct esp_idf_svc usage outside platform/ (except fixed exceptions):" >&2
  echo "$ESP_IDF_SVC_HITS" >&2
  exit 1
fi

echo "OK: platform isolation §14.2 checks passed"
exit 0
