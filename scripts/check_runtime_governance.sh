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

TASK_WDT_SUBSCRIPTION_WRAPPERS='(register_current_task_to_task_wdt|unregister_current_task_from_task_wdt)\s*\('
if rg -n "$TASK_WDT_SUBSCRIPTION_WRAPPERS" src \
  --glob '!src/util.rs' \
  --glob '!src/platform/task_wdt.rs' >/dev/null; then
  echo "FAIL: direct TWDT subscription mutation escaped unified spawn/task-wdt owners" >&2
  rg -n "$TASK_WDT_SUBSCRIPTION_WRAPPERS" src \
    --glob '!src/util.rs' \
    --glob '!src/platform/task_wdt.rs' >&2
  exit 1
fi

RAW_TASK_WDT_SUBSCRIPTION_MUTATORS='esp_task_wdt_(add|delete)\s*\('
if rg -n "$RAW_TASK_WDT_SUBSCRIPTION_MUTATORS" src \
  --glob '!src/platform/task_wdt.rs' >/dev/null; then
  echo "FAIL: raw ESP-IDF TWDT subscription mutation escaped platform/task_wdt" >&2
  rg -n "$RAW_TASK_WDT_SUBSCRIPTION_MUTATORS" src \
    --glob '!src/platform/task_wdt.rs' >&2
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

native_allowlist_names() {
  sed -n 's/.*NativeTaskPolicy[[:space:]]*{[[:space:]]*name:[[:space:]]*"\([^"]*\)".*/\1/p' \
    src/platform/task_affinity.rs
}

native_thread_source_files() {
  case "$1" in
    agent_loop)
      printf '%s\n' src/main.rs src/agent
      ;;
    audio_io_worker)
      printf '%s\n' src/platform/audio_drivers.rs
      ;;
    bg_timer|heartbeat|restart_defer)
      printf '%s\n' src/bg_timer.rs src/heartbeat src/main.rs
      ;;
    dispatch)
      printf '%s\n' src/channels/dispatch.rs
      ;;
    display)
      printf '%s\n' src/platform/display_driver.rs
      ;;
    http_snapshot_exec|http_config_exec|http_diag_exec|http_ota_exec)
      printf '%s\n' src/platform/http_server/esp_transport.rs src/platform/http_server/router src/platform/http_server/handlers
      ;;
    qq_ws|feishu_ws)
      printf '%s\n' src/channels/wss_gateway src/channels/qq src/channels/feishu
      ;;
    tg_poll|tg_sender|fs_sender|dt_sender|wc_sender|qq_sender)
      printf '%s\n' src/channels
      ;;
    voice_realtime|voice_realtime_connect|voice_session|voice_session_worker)
      printf '%s\n' src/audio src/main.rs
      ;;
    wifi_worker)
      printf '%s\n' src/platform/wifi/esp.rs
      ;;
    *)
      return 1
      ;;
  esac
}

NATIVE_STD_BLOCKING_PATTERN='std::sync::Condvar|Condvar::new|\.wait_timeout\s*\(|recv_timeout\s*\(|std::sync::mpsc|mpsc::Receiver|Receiver<'
native_allowlist="$(native_allowlist_names || true)"
if [[ -n "$native_allowlist" ]]; then
  while IFS= read -r native_name; do
    [[ -n "$native_name" ]] || continue
    if ! native_files="$(native_thread_source_files "$native_name")"; then
      echo "FAIL: native task allowlist entry has no governance source mapping: $native_name" >&2
      exit 1
    fi
    if rg -n "$NATIVE_STD_BLOCKING_PATTERN" $native_files >/dev/null; then
      echo "FAIL: native task '$native_name' maps to Rust std blocking primitives" >&2
      rg -n "$NATIVE_STD_BLOCKING_PATTERN" $native_files >&2
      exit 1
    fi
  done <<< "$native_allowlist"
fi

echo "OK: runtime governance checks passed"
exit 0
