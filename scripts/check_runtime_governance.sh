#!/usr/bin/env bash
# Runtime governance gate:
# - runtime mode source must not be patched outside the dedicated governance/source files
# - spawned-thread TWDT registration must stay inside the unified spawn wrapper
# - display command ownership must stay in bootstrap/main/platform

set -euo pipefail

if ! command -v rg >/dev/null 2>&1; then
  echo "check_runtime_governance: ripgrep (rg) is required" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

RAW_RUNTIME_SETTERS='set_(voice_exclusive_active|background_maintenance_active|config_plane_active|pairing_state_known|pairing_required|recovery_safe_mode_active)\s*\('
if rg -n "$RAW_RUNTIME_SETTERS" src \
  --glob '!src/state.rs' \
  --glob '!src/runtime/governance.rs' \
  --glob '!src/runtime/thread_registry.rs' \
  --glob '!src/runtime/linux_supervisor.rs' \
  --glob '!src/network/mod.rs' \
  --glob '!src/platform/http_server/mod.rs' \
  --glob '!src/memory/self_runtime/scheduler.rs' >/dev/null; then
  echo "FAIL: raw runtime state setters escaped approved governance owners" >&2
  rg -n "$RAW_RUNTIME_SETTERS" src \
    --glob '!src/state.rs' \
    --glob '!src/runtime/governance.rs' \
    --glob '!src/runtime/thread_registry.rs' \
    --glob '!src/runtime/linux_supervisor.rs' \
    --glob '!src/network/mod.rs' \
    --glob '!src/platform/http_server/mod.rs' \
    --glob '!src/memory/self_runtime/scheduler.rs' >&2
  exit 1
fi

if rg -n 'register_current_task_to_task_wdt\s*\(' src \
  --glob '!src/util.rs' \
  --glob '!src/main.rs' \
  --glob '!src/platform/task_wdt.rs' \
  --glob '!src/platform/esp_runtime_policy.rs' \
  --glob '!src/platform/wifi/esp.rs' >/dev/null; then
  echo "FAIL: direct TWDT registration escaped unified spawn/main owners" >&2
  rg -n 'register_current_task_to_task_wdt\s*\(' src \
    --glob '!src/util.rs' \
    --glob '!src/main.rs' \
    --glob '!src/platform/task_wdt.rs' \
    --glob '!src/platform/esp_runtime_policy.rs' \
    --glob '!src/platform/wifi/esp.rs' >&2
  exit 1
fi

if rg -n '\.display_command\s*\(' src \
  --glob '!src/platform/**' \
  --glob '!src/bootstrap.rs' \
  --glob '!src/main.rs' >/dev/null; then
  echo "FAIL: display command escaped approved display-plane owners" >&2
  rg -n '\.display_command\s*\(' src \
    --glob '!src/platform/**' \
    --glob '!src/bootstrap.rs' \
    --glob '!src/main.rs' >&2
  exit 1
fi

echo "OK: runtime governance checks passed"
exit 0
