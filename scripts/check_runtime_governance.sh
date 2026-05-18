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

prod_source() {
  sed '/^#\[cfg(test)\]/,$d' "$1"
}

existing_paths() {
  local path
  for path in "$@"; do
    [[ -e "$path" ]] && printf '%s\n' "$path"
  done
}

sanitize_storage_backend_wording() {
  sed -E \
    -e 's/[sS][pP][iI][fF][fF][sS]/storage-backend/g' \
    -e 's/[lL][iI][tT][tT][lL][eE][fF][sS]/storage-backend/g'
}

USER_STORAGE_BACKEND_LEAK_PATTERN='[sS][pP][iI][fF][fF][sS]|[lL][iI][tT][tT][lL][eE][fF][sS]|spiffs_|littlefs_|beetle_spiffs|name:[[:space:]]*"spiffs_|usage:[[:space:]]*"spiffs_'
USER_STORAGE_SPIFFS_LEAK_PATHS="$(
  existing_paths \
    src/cli \
    src/tools \
    src/heartbeat \
    src/metrics.rs \
    src/display.rs \
    src/platform/http_server/handlers \
    configure-ui/src \
    docs/en-us \
    docs/zh-cn \
    README.md \
    README.zh-CN.md \
    packaging/linux
)"
if [[ -n "$USER_STORAGE_SPIFFS_LEAK_PATHS" ]] &&
   rg -n "$USER_STORAGE_BACKEND_LEAK_PATTERN" $USER_STORAGE_SPIFFS_LEAK_PATHS \
     --glob '!docs/en-us/release-notes/**' \
     --glob '!docs/zh-cn/release-notes/**' >/tmp/beetle-storage-leaks.$$; then
  echo "FAIL: user/business-facing storage wording leaked platform backend naming:" >&2
  sanitize_storage_backend_wording </tmp/beetle-storage-leaks.$$ >&2
  rm -f /tmp/beetle-storage-leaks.$$
  exit 1
fi
rm -f /tmp/beetle-storage-leaks.$$

bash scripts/tests/esp_audio_codec_contract_test.sh >/dev/null

RESPONSE_BODY_INTO_VEC_HOT_PATH='ResponseBody::into_vec|\b(body|resp_body|response_body)\.into_vec\s*\('
if rg -n "$RESPONSE_BODY_INTO_VEC_HOT_PATH" src \
  --glob '!src/platform/response_body.rs' >/dev/null; then
  echo "FAIL: ResponseBody::into_vec escaped the owned-conversion implementation; consume ResponseBody with as_slice()/as_ref() on hot paths" >&2
  rg -n "$RESPONSE_BODY_INTO_VEC_HOT_PATH" src \
    --glob '!src/platform/response_body.rs' >&2
  exit 1
fi

if rg -n '\bstate_fs\.read\s*\(' src/tools >/dev/null; then
  echo "FAIL: tool state-file reads must use StateFs::read_bytes()/ByteBuffer instead of heap Vec reads" >&2
  rg -n '\bstate_fs\.read\s*\(' src/tools >&2
  exit 1
fi

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
    bg_timer|heartbeat)
      printf '%s\n' src/bg_timer.rs src/heartbeat src/main.rs
      ;;
    dispatch)
      printf '%s\n' src/channels/dispatch.rs
      ;;
    display)
      printf '%s\n' src/platform/display_driver.rs
      ;;
    http_snapshot_exec|http_config_exec|http_diag_exec)
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

check_no_prod_timed_wait_before_tests() {
  local file="$1"
  local pattern="$2"
  local label="$3"
  local matches
  matches="$(sed '/^#\[cfg(test)\]/,$d' "$file" | rg -n "$pattern" || true)"
  if [[ -n "$matches" ]]; then
    echo "FAIL: ESP lazy/scheduler timed wait escaped P0.3 cleanup in $label" >&2
    printf '%s\n' "$matches" >&2
    exit 1
  fi
}

check_no_prod_timed_wait_before_tests \
  src/runtime/write_back.rs \
  '\.wait_timeout\s*\(|recv_timeout\s*\(' \
  "write_back"
check_no_prod_timed_wait_before_tests \
  src/audio/voice_session.rs \
  'recv_timeout\s*\(' \
  "voice_session"

check_no_prod_pattern_before_tests() {
  local file="$1"
  local pattern="$2"
  local label="$3"
  local message="$4"
  local matches
  matches="$(sed '/^#\[cfg(test)\]/,$d' "$file" | rg -n "$pattern" || true)"
  if [[ -n "$matches" ]]; then
    echo "FAIL: $message in $label" >&2
    printf '%s\n' "$matches" >&2
    exit 1
  fi
}

BG_TIMER_STORAGE_HEAVY='gc_stale\s*\(|pop_due\s*\(|claim_due\s*\(|write_json_file|write_file(_unlocked)?|remove_file\s*\(|SessionRepairMode::Immediate|write_session_messages_unlocked|load_session_snapshot_unlocked|flush_session_store|serde_json::to_(vec|vec_pretty)|crate::memory::remind_tick|crate::task::task_due_tick|crate::memory::self_runtime_tick|crate::runtime::initiative_tick'
check_no_prod_pattern_before_tests \
  src/bg_timer.rs \
  "$BG_TIMER_STORAGE_HEAVY" \
  "bg_timer" \
  "bg_timer must schedule storage mutation through the write-back plane instead of running it inline"
check_no_prod_pattern_before_tests \
  src/heartbeat/mod.rs \
  'gc_stale\s*\(|pop_due\s*\(|claim_due\s*\(|write_json_file|write_file(_unlocked)?|remove_file\s*\(' \
  "heartbeat" \
  "heartbeat must remain a lightweight observability tick"
check_no_prod_pattern_before_tests \
  src/memory/hygiene.rs \
  'gc_stale\s*\(' \
  "memory hygiene" \
  "post-reply memory hygiene must not remove session files inline"
check_no_prod_pattern_before_tests \
  src/agent/loop/reply_finalize.rs \
  'write_json_file|write_file(_unlocked)?|remove_file\s*\(|SessionRepairMode::Immediate|write_session_messages_unlocked|load_session_snapshot_unlocked|flush_session_store|gc_stale\s*\(' \
  "reply_finalize" \
  "reply finalization must not perform direct storage repair, compaction, or file rewrite"

if ! rg -n 'service_write_back_tasks_runs_due_work_off_caller_thread' src/runtime/write_back.rs >/dev/null ||
   ! rg -n 'scheduler_storage_ticks_run_on_write_back_worker' src/runtime/write_back.rs >/dev/null ||
   ! rg -n 'periodic_storage_maintenance_defers_when_worker_stack_would_break_tls_floor' src/runtime/write_back.rs >/dev/null ||
   ! rg -n 'read_paths_do_not_touch_or_rewrite_long_term_memory_file' src/platform/storage/long_term_memory.rs >/dev/null ||
   ! rg -n 'append_batch_defers_cold_malformed_session_repair' src/platform/storage/session.rs >/dev/null ||
   ! rg -n 'load_recent_records_defers_repair_and_synthesizes_stable_ids' src/platform/storage/session.rs >/dev/null; then
  echo "FAIL: P4 storage/write-back/session deferred repair contract tests are missing" >&2
  exit 1
fi

if ! rg -n 'current_periodic_storage_maintenance_admission\(\)' src/runtime/write_back.rs >/dev/null ||
   ! rg -n 'requires_periodic_idle_headroom\(\)' src/runtime/write_back.rs >/dev/null ||
   ! rg -n 'PERIODIC_STORAGE_MAINTENANCE_MIN_LARGEST_BLOCK_BYTES' src/runtime/write_back.rs >/dev/null; then
  echo "FAIL: optional periodic storage maintenance must reserve write-back worker stack plus TLS headroom before scheduling" >&2
  exit 1
fi

if rg -n 'touch_long_term_memory_usage' src/platform/storage/long_term_memory.rs >/dev/null; then
  echo "FAIL: long-term memory read paths must not touch/persist usage metadata from hot routes or turn prepare" >&2
  rg -n 'touch_long_term_memory_usage' src/platform/storage/long_term_memory.rs >&2
  exit 1
fi

HTTP_RESPONSE_HEAVY='write_json_file|write_file(_unlocked)?|remove_file\s*\(|state_fs\(\)\.(write|remove)|create_http_client|connect_wss|std::thread::spawn|spawn_guarded'
http_response_body="$(
  sed -n '/^fn write_api_resp/,/^fn route_runtime_admission_response/p' src/platform/http_server/esp_transport.rs
  sed -n '/^fn write_outgoing/,/^fn esp_dispatch_route/p' src/platform/http_server/esp_transport.rs
)"
if printf '%s\n' "$http_response_body" | rg -n "$HTTP_RESPONSE_HEAVY" >/dev/null; then
  echo "FAIL: ESP HTTP response write path must not perform storage/network/spawn work" >&2
  printf '%s\n' "$http_response_body" | rg -n "$HTTP_RESPONSE_HEAVY" >&2
  exit 1
fi

if ! rg -n 'let route_worker_lease = match acquire_route_worker_lease\(spawn_contract\)' src/platform/http_server/esp_transport.rs >/dev/null ||
   ! rg -n 'let _route_worker_lease = route_worker_lease' src/platform/http_server/esp_transport.rs >/dev/null; then
  echo "FAIL: ESP route workers must acquire lane lease before spawning the worker stack and hold it for the worker lifetime" >&2
  exit 1
fi

if ! rg -n 'pub\(crate\) const fn lease_kind\s*\(' src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: route worker lane lease mapping escaped router catalog truth source" >&2
  exit 1
fi

if ! rg -n 'worker_route_lanes_map_to_runtime_lease_kinds' src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: route worker lane lease mapping no longer has a catalog contract test" >&2
  exit 1
fi

if ! rg -n 'runtime_mode_admission\s*\(' src/platform/http_server/router/catalog.rs >/dev/null ||
   ! rg -n 'route_runtime_mode_admission_blocks_unowned_diagnostics_during_config_active' src/platform/http_server/router/catalog.rs >/dev/null ||
   ! rg -n 'route_runtime_admission_response\s*\(' src/platform/http_server/esp_transport.rs >/dev/null; then
  echo "FAIL: config/recovery route runtime admission contract is missing" >&2
  exit 1
fi

if ! rg -n 'tracks_config_read_burst\s*\(' src/platform/http_server/router/catalog.rs >/dev/null ||
   ! rg -n 'ConfigReadBurstGuard::enter' src/platform/http_server/esp_transport.rs >/dev/null ||
   ! rg -n 'config_read_burst_guard_marks_config_recovery_lease_and_lifecycle' src/runtime/governance.rs >/dev/null ||
   ! rg -n 'ConfigReadBurst' dev-docs/esp-plane-budget-and-lease-register.md >/dev/null; then
  echo "FAIL: config read burst no longer has explicit config/recovery lease and lifecycle tracking" >&2
  exit 1
fi

if ! rg -n 'display_channel_runtime_status_from_lifecycle' src/main.rs >/dev/null ||
   ! rg -n 'WaitingWallClock|Suspended|Connecting|CoolingDown' src/display.rs src/platform/display_driver.rs >/dev/null; then
  echo "FAIL: channel runtime display status no longer distinguishes wall-clock, mode-suspend, connect, and cooldown states" >&2
  exit 1
fi

if ! rg -n 'runtime::lease::format_baseline_log_line' src/heartbeat/mod.rs >/dev/null; then
  echo "FAIL: heartbeat no longer emits compact lease baseline" >&2
  exit 1
fi

if ! rg -n 'pub mod execution_budget' src/runtime/mod.rs >/dev/null ||
   ! rg -n 'pub struct ExecutionBudgetSnapshot' src/runtime/execution_budget.rs >/dev/null ||
   ! rg -n 'runtime::execution_budget::format_baseline_log_line' src/heartbeat/mod.rs >/dev/null; then
  echo "FAIL: runtime execution budget projection is no longer exposed through heartbeat" >&2
  exit 1
fi

if ! rg -n 'worker_route_classes_have_complete_contracts' src/platform/http_server/router/catalog.rs >/dev/null ||
   ! rg -n 'route_worker_runtime_admission_blocks_front_plane_contention' src/platform/http_server/router/catalog.rs >/dev/null ||
   ! rg -n 'execution_budget_maps_every_plane_thread_to_stack_or_logical_owner' src/runtime/plane.rs >/dev/null; then
  echo "FAIL: route/runtime execution budget contracts are no longer tested against their truth sources" >&2
  exit 1
fi

if rg -n 'local_diagnostic:\s*EspRouteExecutor|EspRouteExecutor::new\(RouteExecutionClass::LocalDiagnosticRoute' src/platform/http_server/esp_transport.rs >/dev/null; then
  echo "FAIL: ESP route executors must be keyed by RouteWorkerLane; local and slow diagnostic routes must share one diagnostic executor" >&2
  rg -n 'local_diagnostic:\s*EspRouteExecutor|EspRouteExecutor::new\(RouteExecutionClass::LocalDiagnosticRoute' src/platform/http_server/esp_transport.rs >&2
  exit 1
fi

if ! rg -n 'esp_route_executors_are_one_per_worker_lane' src/platform/http_server/esp_transport.rs >/dev/null; then
  echo "FAIL: ESP route executor lane cardinality test is missing" >&2
  exit 1
fi

if ! rg -n 'route_worker_runtime_busy_detail' src/platform/http_server/router/catalog.rs >/dev/null ||
   ! rg -n 'active_agent_tasks|storage_contention' src/platform/http_server/router/catalog.rs >/dev/null ||
   ! rg -n 'http_route_worker_runtime_busy' src/platform/http_server/esp_transport.rs >/dev/null; then
  echo "FAIL: ESP route worker lazy-start admission must reject front-plane/storage contention before spawning worker stacks" >&2
  exit 1
fi

if prod_source src/platform/http_server/esp_transport.rs |
   rg -n 'memory_snapshot_live|log_startup_memory_checkpoint' >/dev/null; then
  echo "FAIL: ESP HTTP request/route-worker path must not live-sample heap" >&2
  exit 1
fi

if ! rg -n 'storage_touching_routes_never_run_on_httpd_callback' src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: storage-touching route callback confinement contract test is missing" >&2
  exit 1
fi

if rg -n 'ConfigStore|pairing::|orchestrator::snapshot|resource_diagnostic_snapshot|RouteExecutionClass|route_worker' src/platform/http_server/handlers/csrf_token.rs >/dev/null; then
  echo "FAIL: csrf token read path must stay lightweight and must not depend on config/resource/route-worker state" >&2
  rg -n 'ConfigStore|pairing::|orchestrator::snapshot|resource_diagnostic_snapshot|RouteExecutionClass|route_worker' src/platform/http_server/handlers/csrf_token.rs >&2
  exit 1
fi

if rg -n '"restart_defer"|STACK_RESTART_DEFER|spawn_restart_defer_worker' src >/dev/null; then
  echo "FAIL: restart_defer must stay retired; restart responses must use the runtime delayed-task coordinator" >&2
  rg -n '"restart_defer"|STACK_RESTART_DEFER|spawn_restart_defer_worker' src >&2
  exit 1
fi

if ! rg -n 'schedule_restart_with_continuity_flush' src/platform/http_server/mod.rs >/dev/null; then
  echo "FAIL: Linux HTTP restart responses must use schedule_restart_with_continuity_flush" >&2
  exit 1
fi

if ! rg -n 'schedule_restart_with_continuity_flush' src/platform/http_server/esp_transport.rs >/dev/null; then
  echo "FAIL: ESP HTTP restart responses must use schedule_restart_with_continuity_flush" >&2
  exit 1
fi

if rg -n '(spawn(_guarded|_planned|_required)?|std::thread::spawn|Builder::new).*restart|restart.*(spawn(_guarded|_planned|_required)?|std::thread::spawn|Builder::new)' src/platform/http_server >/dev/null; then
  echo "FAIL: HTTP restart response paths must not create ad-hoc restart threads" >&2
  rg -n '(spawn(_guarded|_planned|_required)?|std::thread::spawn|Builder::new).*restart|restart.*(spawn(_guarded|_planned|_required)?|std::thread::spawn|Builder::new)' src/platform/http_server >&2
  exit 1
fi

if ! rg -n 'resource_light_snapshot\s*\(' src/platform/http_server/handlers/resource.rs src/orchestrator/mod.rs >/dev/null ||
   ! rg -n 'pub struct ResourceLightSnapshot' src/orchestrator/state.rs >/dev/null; then
  echo "FAIL: /api/resource must use the orchestrator cached-light resource snapshot" >&2
  exit 1
fi

if ! rg -n 'admission:\s*ResourceAdmissionSnapshot' src/orchestrator/state.rs >/dev/null; then
  echo "FAIL: orchestrator resource diagnostic no longer retains admission summary for internal governance/logging" >&2
  exit 1
fi

if ! rg -n 'governance_metrics:\s*orchestrator::ResourceGovernanceMetricsSnapshot' src/platform/http_server/handlers/resource.rs >/dev/null; then
  echo "FAIL: /api/resource no longer exposes governance metrics summary" >&2
  exit 1
fi

if ! rg -n 'CRASH_METADATA_PROVIDER:\s*OnceLock' src/orchestrator/mod.rs >/dev/null; then
  echo "FAIL: crash metadata provider registration is no longer owned by orchestrator" >&2
  exit 1
fi

if ! rg -n 'RECORDED_CRASH_METADATA:\s*Mutex' src/orchestrator/mod.rs >/dev/null ||
   ! rg -n 'pub fn record_crash_metadata\s*\(' src/orchestrator/mod.rs >/dev/null; then
  echo "FAIL: recorded crash metadata store/record API is missing from orchestrator" >&2
  exit 1
fi

if ! rg -n 'crash:\s*CrashMetadataSnapshot' src/orchestrator/state.rs >/dev/null ||
   ! rg -n 'crash_metadata_snapshot\s*\(' src/orchestrator/mod.rs src/orchestrator/state.rs >/dev/null; then
  echo "FAIL: ResourceDiagnosticSnapshot no longer aggregates crash metadata" >&2
  exit 1
fi

if rg -n 'crash:\s*orchestrator::CrashMetadataSnapshot|crash:\s*diag\.crash|resource_diagnostic_snapshot\s*\(' src/platform/http_server/handlers/resource.rs >/dev/null; then
  echo "FAIL: default /api/resource must not expose crash diagnostics or call the deep diagnostic snapshot" >&2
  exit 1
fi

if ! rg -n 'runtime_capabilities:\s*Vec<crate::orchestrator::RuntimeCapabilityState>' src/orchestrator/state.rs >/dev/null ||
   ! rg -n 'runtime_capability_snapshot\s*\(' src/orchestrator/state.rs >/dev/null; then
  echo "FAIL: orchestrator deep diagnostic snapshot no longer retains runtime capability facts" >&2
  exit 1
fi

if prod_source src/platform/http_server/handlers/resource.rs |
   rg -n 'runtime_capabilities|execution_budget|plane_lifecycle|threads|write_back' >/dev/null; then
  echo "FAIL: default /api/resource must not expose deep diagnostic objects" >&2
  exit 1
fi
if ! rg -n 'resource_route_stays_cached_light_immediate' src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: /api/resource must stay an immediate cached-light route on ESP" >&2
  exit 1
fi

if ! rg -n 'pub struct RuntimeCapabilityCallGuard' src/orchestrator/runtime_capability.rs >/dev/null; then
  echo "FAIL: runtime capability active-call control no longer exposes RuntimeCapabilityCallGuard" >&2
  exit 1
fi

if ! rg -n 'pub fn try_begin_runtime_capability_call\s*\(' src/orchestrator/runtime_capability.rs >/dev/null; then
  echo "FAIL: runtime capability active-call control no longer exposes try_begin_runtime_capability_call" >&2
  exit 1
fi

for field in 'pub active_calls:\s*u32' 'pub draining:\s*bool' 'pub last_transition_uptime_ms:\s*u64' 'pub drain_denied_total:\s*u64'; do
  if ! rg -n "$field" src/orchestrator/runtime_capability.rs >/dev/null; then
    echo "FAIL: runtime capability snapshot missing active-call field: $field" >&2
    exit 1
  fi
done

if ! rg -n 'try_begin_runtime_capability_call_with_policy\s*\(' src/tools/registry.rs >/dev/null; then
  echo "FAIL: tool execution no longer acquires runtime capability call guard" >&2
  exit 1
fi

if rg -n 'delivery\.finalize\(&final_content\)' src/agent/loop/turn_execution.rs >/dev/null; then
  echo "FAIL: raw final content must not be streamed before ReplyFinalize canonicalizes it" >&2
  rg -n 'delivery\.finalize\(&final_content\)' src/agent/loop/turn_execution.rs >&2
  exit 1
fi

if rg -n 'delivery\.on_stream_delta|on_stream_delta\(accumulated\)' src/agent/loop/turn_execution.rs >/dev/null; then
  echo "FAIL: LLM progress callback must not stream raw accumulated text before ReplyFinalize" >&2
  rg -n 'delivery\.on_stream_delta|on_stream_delta\(accumulated\)' src/agent/loop/turn_execution.rs >&2
  exit 1
fi

if rg -n 'sync_user_turn_relationship_topology' src/agent/loop/reply_finalize.rs src/agent/loop/turn_finalize.rs >/dev/null; then
  echo "FAIL: relationship topology sync must stay out of synchronous reply finalization" >&2
  rg -n 'sync_user_turn_relationship_topology' src/agent/loop/reply_finalize.rs src/agent/loop/turn_finalize.rs >&2
  exit 1
fi

if ! rg -n 'esp_compact_user_prompt_assembly_uses_compact_carry_without_governed_recall' src/memory/profile.rs >/dev/null; then
  echo "FAIL: EspCompact user prompt hot path must have a contract test for governed recall being off" >&2
  exit 1
fi

if ! rg -n 'build_context_messages_does_not_mutate_important_marker' src/memory/context_window.rs >/dev/null; then
  echo "FAIL: prompt context window must prove build-time important marker reads are non-mutating" >&2
  exit 1
fi

for capability_id in \
  RUNTIME_CAPABILITY_DISPLAY_OUTPUT \
  RUNTIME_CAPABILITY_HARDWARE_GPIO \
  RUNTIME_CAPABILITY_HARDWARE_I2C \
  RUNTIME_CAPABILITY_SENSOR \
  RUNTIME_CAPABILITY_CAMERA_FRAME; do
  if ! rg -n "$capability_id" src/orchestrator/runtime_capability.rs src/orchestrator/mod.rs >/dev/null; then
    echo "FAIL: P7 runtime capability id missing: $capability_id" >&2
    exit 1
  fi
done

if ! rg -n 'RUNTIME_CAPABILITY_HARDWARE_GPIO' src/tools/hardware.rs >/dev/null ||
   ! rg -n 'RUNTIME_CAPABILITY_HARDWARE_I2C' src/tools/i2c_device.rs src/tools/i2c_sensor.rs >/dev/null ||
   ! rg -n 'RUNTIME_CAPABILITY_SENSOR' src/tools/i2c_sensor.rs src/tools/sensor_watch.rs >/dev/null; then
  echo "FAIL: hardware/I2C/sensor tools must declare runtime capability guards" >&2
  exit 1
fi

if ! rg -n 'ToolEffectClass::HardwareRead.*RUNTIME_CAPABILITY_SENSOR|RUNTIME_CAPABILITY_SENSOR.*ToolEffectClass::HardwareRead' src/tools/policy.rs >/dev/null; then
  echo "FAIL: hardware read tool execution must require the sensor runtime capability" >&2
  exit 1
fi

if ! rg -n 'RUNTIME_CAPABILITY_HARDWARE_GPIO' src/tools/sensor_watch.rs >/dev/null ||
   ! rg -n 'RUNTIME_CAPABILITY_HARDWARE_I2C' src/tools/sensor_watch.rs >/dev/null; then
  echo "FAIL: sensor watch must hold underlying hardware.gpio/i2c active-call guards while sampling" >&2
  exit 1
fi

if ! rg -n 'hardware_drivers::drive_gpio_out' src/platform/linux/mod.rs >/dev/null ||
   ! rg -n 'hardware_drivers::drive_i2c_sensor_stub' src/platform/linux/mod.rs >/dev/null ||
   ! rg -n 'hardware_backend_serviceable_for_capability' src/orchestrator/runtime_capability.rs >/dev/null ||
   ! rg -n 'MemorySystemKind::LinuxFull' src/orchestrator/runtime_capability.rs >/dev/null; then
  echo "FAIL: Linux/host hardware stub contract must stay serviceable; ESP runtime capability governance must not disable the existing Linux development/test backend" >&2
  exit 1
fi

for metric in \
  record_runtime_spawn_failure \
  record_http_route_reject; do
  if ! rg -n "$metric" src/metrics.rs >/dev/null; then
    echo "FAIL: runtime governance metric missing from metrics.rs: $metric" >&2
    exit 1
  fi
done

if rg -n 'record_lease_conflict|record_lease_expired_replacement|record_plane_drain_timeout' src >/dev/null; then
  echo "FAIL: lease/drain debug counters must not be exposed as runtime governance business metrics" >&2
  exit 1
fi

for metric in \
  record_event_ingress_enqueued \
  record_event_ingress_rejected \
  record_event_ingress_purged \
  record_event_ingress_cancelled \
  record_event_ingress_stale_drop; do
  if ! rg -n "$metric" src/metrics.rs >/dev/null; then
    echo "FAIL: event ingress metric missing from metrics.rs: $metric" >&2
    exit 1
  fi
done

if ! rg -n 'event_ingress_enqueued_total=.*event_ingress_rejected_total=.*event_ingress_purged_total=.*event_ingress_cancelled_total=.*event_ingress_stale_drop_total=' src/metrics.rs >/dev/null; then
  echo "FAIL: heartbeat metrics baseline no longer exposes bounded event ingress counters" >&2
  exit 1
fi

if ! rg -n 'pub\(crate\) fn event_ingress_contract\s*\(' src/channels/inbound_backpressure.rs >/dev/null ||
   ! rg -n 'EventIngressFullPolicy::Coalesce' src/channels/inbound_backpressure.rs >/dev/null ||
   ! rg -n 'EventIngressRetention::BestEffort' src/channels/inbound_backpressure.rs >/dev/null; then
  echo "FAIL: bounded event ingress contract truth source is missing" >&2
  exit 1
fi

if ! rg -n 'record_enqueued\(\s*EventIngressSource::WssGateway' src/channels/wss_gateway/loop.rs src/channels/dingtalk/inbound.rs src/channels/wecom/aibot.rs >/dev/null; then
  echo "FAIL: WSS/stream ingress enqueue path no longer records bounded event ingress acceptance" >&2
  exit 1
fi

if ! rg -n 'record_enqueued\(\s*EventIngressSource::TelegramPoll' src/channels/telegram/poll.rs >/dev/null; then
  echo "FAIL: Telegram poll ingress enqueue path no longer records bounded event ingress acceptance" >&2
  exit 1
fi

if ! rg -n 'record_initiative_ingress_result' src/runtime/initiative.rs >/dev/null ||
   ! rg -n 'EventIngressSource::RuntimeInitiative' src/runtime/initiative.rs >/dev/null; then
  echo "FAIL: runtime initiative no longer records bounded event ingress outcomes" >&2
  exit 1
fi

if ! rg -n 'record_cancelled\(\s*EventIngressSource::WriteBack' src/runtime/write_back.rs >/dev/null ||
   ! rg -n 'record_rejected\(\s*EventIngressSource::WriteBack' src/runtime/write_back.rs >/dev/null; then
  echo "FAIL: write-back queue no longer records coalesced/rejected ingress outcomes" >&2
  exit 1
fi

if rg -n 'inbound_tx\.send\(msg\)' src/heartbeat/mod.rs >/dev/null ||
   ! rg -n 'inbound_tx\.try_send\(msg\)' src/heartbeat/mod.rs >/dev/null; then
  echo "FAIL: heartbeat injection must use bounded try_send rather than blocking send" >&2
  exit 1
fi

if ! rg -n 'record_event_ingress_rejected' src/app_runtime_support.rs >/dev/null ||
   ! rg -n 'clear_pending_retry\(\)' src/app_runtime_support.rs >/dev/null; then
  echo "FAIL: pending retry bootstrap no longer preserves retry ownership around bounded enqueue" >&2
  exit 1
fi

if ! rg -n 'external_wss_suspend_timeout|voice_exclusive_wss_drain_timeout' src/network/mod.rs >/dev/null ||
   rg -n 'record_plane_drain_timeout\(\)' src/network/mod.rs >/dev/null; then
  echo "FAIL: external WSS drain timeout must remain a structured transport admission failure without a business metric counter" >&2
  exit 1
fi

if ! rg -n 'VoiceExclusiveTransportGuard::enter\(cfg\.platform\.as_ref\(\),\s*TAG\)' src/audio/voice_session.rs >/dev/null ||
   ! rg -n 'realtime voice transport admission failed' src/audio/voice_session.rs >/dev/null; then
  echo "FAIL: realtime voice no longer treats external WSS drain failure as transport admission failure" >&2
  exit 1
fi

if ! rg -n 'begin_external_wss_worker_evict_request' src/network/mod.rs >/dev/null ||
   ! rg -n 'external_wss_worker_should_exit_for_evict' src/channels/wss_gateway/loop.rs >/dev/null ||
   ! rg -n 'service_channel_wss_supervisors' src/bg_timer.rs >/dev/null ||
   ! rg -n 'realtime_voice_pre_spawn_largest_floor' src/network/mod.rs >/dev/null ||
   ! rg -n 'active_os_outbound_worker_count' src/network/mod.rs >/dev/null ||
   ! rg -n 'run_os_outbound_supervisor' src/channels/dispatch.rs >/dev/null; then
  echo "FAIL: realtime voice admission must evict external WSS/outbound workers and reserve connect-stack largest-block before spawning" >&2
  exit 1
fi

if ! rg -n 'OutboundHttpRecovery' src/network/mod.rs >/dev/null ||
   ! rg -n 'ensure_outbound_http_recovery_wss_evict' src/channels/dispatch.rs >/dev/null ||
   ! rg -n 'critical_defers_outbound_even_without_queue_congestion' src/orchestrator/admission.rs >/dev/null; then
  echo "FAIL: critical outbound HTTP recovery must defer before TLS and evict subordinate external WSS workers when needed" >&2
  exit 1
fi

if ! rg -n 'PrepareRealtimeTransportThenSpawnConnect' src/audio/voice_session.rs >/dev/null ||
   ! rg -n 'spawn_prepared_realtime_session_worker' src/audio/voice_session.rs >/dev/null; then
  echo "FAIL: realtime voice startup must keep connect and session workers split" >&2
  exit 1
fi

if ! rg -n 'external_wss_worker_should_exit_for_evict' src/channels/wecom/aibot.rs >/dev/null; then
  echo "FAIL: direct external WSS channel loops must honor worker eviction, not only the gateway loop" >&2
  exit 1
fi

if ! rg -n 'mark_audio_io_lifecycle' src/platform/audio_drivers.rs >/dev/null ||
   ! rg -n 'PlaneId::PlatformAudio' src/platform/audio_drivers.rs >/dev/null ||
   ! rg -n 'PlaneLifecycleState::Draining' src/platform/audio_drivers.rs >/dev/null ||
   ! rg -n 'PlaneLifecycleState::Unloaded' src/platform/audio_drivers.rs >/dev/null; then
  echo "FAIL: ESP audio IO worker no longer records stop/join lifecycle" >&2
  exit 1
fi

if ! rg -n 'runtime_mode:\s*Option<crate::runtime::RuntimeMode>' src/tools/policy.rs >/dev/null; then
  echo "FAIL: tool policy context no longer carries runtime mode" >&2
  exit 1
fi

if ! rg -n 'DEFAULT_EMBEDDED_TOOL_PROFILE' src/tools/policy.rs >/dev/null ||
   ! rg -n 'target_os = "linux"' src/tools/policy.rs >/dev/null; then
  echo "FAIL: Linux embedded target family is no longer covered by default tool runtime policy" >&2
  exit 1
fi

if ! rg -n 'pub fn tool_effect_visible_in_mode' src/tools/policy.rs >/dev/null; then
  echo "FAIL: tool effect/runtime-mode visibility truth source is missing" >&2
  exit 1
fi

if ! rg -n 'tool_effect_visible_in_mode\(shape\.effect_class,\s*policy\)' src/tools/registry.rs >/dev/null; then
  echo "FAIL: LLM tool execution assessment no longer checks runtime-mode effect visibility" >&2
  exit 1
fi

if ! rg -n 'tool_effect_visible_in_mode\(entry\.catalog_shape\.effect_class,\s*policy\)' src/tools/registry.rs >/dev/null; then
  echo "FAIL: LLM tool specs no longer check runtime-mode visibility from conservative catalog effect" >&2
  exit 1
fi

if ! rg -n 'ToolPolicyContext::new\(msg\.ingress,\s*msg\.channel\.as_ref\(\)\)\s*$' src/agent/request_plan.rs src/agent/loop/turn_execution.rs >/dev/null; then
  echo "FAIL: agent request path no longer builds request-scoped tool policy" >&2
  exit 1
fi

if ! rg -n 'runtime_mode_snapshot\(\)\.current_mode' src/agent/request_plan.rs src/agent/loop/turn_execution.rs >/dev/null; then
  echo "FAIL: agent request path no longer feeds runtime mode into tool policy" >&2
  exit 1
fi

if ! rg -n 'embedded_runtime_mode_filters_effect_classes_without_user_text' src/tools/policy.rs >/dev/null; then
  echo "FAIL: runtime-mode tool effect policy contract test is missing" >&2
  exit 1
fi

if ! rg -n 'llm_tool_specs_filter_embedded_voice_exclusive_by_effect_class|llm_tool_specs_filter_dynamic_catalog_effect_class|llm_execution_denies_runtime_mode_hidden_tool' src/tools/registry.rs >/dev/null; then
  echo "FAIL: runtime-mode LLM tool visibility/execution contract tests are missing" >&2
  exit 1
fi

if ! rg -n 'actual_shape != permit\.shape \|\| actual_requires_network != permit\.requires_network' src/tools/registry.rs >/dev/null; then
  echo "FAIL: execute_permitted no longer binds permits to the actual dynamic execution shape" >&2
  exit 1
fi

if ! rg -n 'execute_permitted_rejects_args_that_change_dynamic_execution_shape' src/tools/registry.rs >/dev/null; then
  echo "FAIL: dynamic execution-shape permit binding contract test is missing" >&2
  exit 1
fi

if ! rg -n 'record_llm_request_body_bytes' src/metrics.rs src/llm/openai_compatible.rs src/llm/anthropic.rs >/dev/null; then
  echo "FAIL: LLM request body size metric is no longer recorded at request-body construction" >&2
  exit 1
fi

if ! rg -n 'llm_req_body_last_b=.*llm_req_body_max_b=' src/metrics.rs >/dev/null; then
  echo "FAIL: heartbeat metrics baseline no longer exposes LLM request body byte budget" >&2
  exit 1
fi

if ! rg -n 'fn build_qq_send_body' src/channels/qq/send.rs >/dev/null ||
   ! rg -n 'crate::error::Result<ByteBuffer>' src/channels/qq/send.rs >/dev/null ||
   ! rg -n 'crate::error::Result<Vec<ByteBuffer>>' src/channels/qq/send.rs >/dev/null ||
   ! rg -n 'max_payload_b=' src/channels/qq/send.rs >/dev/null; then
  echo "FAIL: QQ outbound payloads no longer use ByteBuffer with payload-size observability" >&2
  exit 1
fi

if ! rg -n 'POST_REPLY_BACKGROUND_MAX_DEFER_MS' src/constants.rs src/agent/loop/background_jobs.rs src/runtime/delayed_task.rs src/memory/self_runtime/scheduler.rs >/dev/null ||
   ! rg -n 'schedule_bounded_keyed_system_inbound_msg' src/runtime/delayed_task.rs src/agent/loop/background_jobs.rs src/memory/self_runtime/scheduler.rs >/dev/null; then
  echo "FAIL: embedded post-reply/self-runtime bounded deferral and keyed system coalescing contract is missing" >&2
  exit 1
fi

if ! rg -n 'deferred_buffer_coalesces_same_req_primary_retry' src/channels/dispatch.rs >/dev/null ||
   ! rg -n 'coalesced duplicate deferred primary' src/channels/dispatch.rs >/dev/null; then
  echo "FAIL: outbound deferred primary replay no longer coalesces duplicate req_id/channel retries" >&2
  exit 1
fi

if rg -n 'dropping newest primary|deferred primary.*dropp' src/channels/dispatch.rs >/dev/null; then
  echo "FAIL: outbound deferred primary replies must be preserved or backpressured, never dropped" >&2
  rg -n 'dropping newest primary|deferred primary.*dropp' src/channels/dispatch.rs >&2
  exit 1
fi

if ! rg -n 'prompt_skill_budget_for_runtime_mode' src/skills/mod.rs src/main.rs >/dev/null; then
  echo "FAIL: active prompt skill docs no longer pass through runtime-mode budget policy" >&2
  exit 1
fi

if ! rg -n 'DEFAULT_PROMPT_SKILL_MAX_CHARS' src/skills/mod.rs src/main.rs >/dev/null; then
  echo "FAIL: prompt skill docs no longer have a centralized default budget" >&2
  exit 1
fi

if ! rg -n 'prompt_context_normalization_budget' src/memory/profile.rs src/memory/prompt_context.rs src/memory/mod.rs >/dev/null; then
  echo "FAIL: prompt memory context normalization budget no longer has a centralized memory-profile truth source" >&2
  exit 1
fi

if ! rg -n 'normalize_for_prompt\(' src/memory/prompt_context.rs src/agent/loop/worker_context_stages.rs >/dev/null; then
  echo "FAIL: worker prompt memory no longer passes through normalized projection assembly" >&2
  exit 1
fi

if ! rg -n 'constitutional_stack_text\s*=\s*prompt_memory\.constitutional_stack_text\.take\(\)' src/agent/loop/worker_context_stages.rs >/dev/null; then
  echo "FAIL: worker finalize no longer consumes normalized projection groups before releasing prompt memory caches" >&2
  exit 1
fi

if ! rg -n 'pub struct FrameLeaseAdmission' src/runtime/frame_lease.rs >/dev/null; then
  echo "FAIL: camera frame lease no longer exposes explicit mode/pressure admission" >&2
  exit 1
fi

if ! rg -n 'admit_current_camera_frame_capture' src/runtime/frame_lease.rs src/runtime/mod.rs >/dev/null ||
   ! rg -n 'FrameLeaseAdmission::current\(\)' src/runtime/frame_lease.rs >/dev/null ||
   ! rg -n 'admission\.ensure_allowed\(\)\?' src/runtime/frame_lease.rs >/dev/null; then
  echo "FAIL: camera frame borrow no longer passes through mode/pressure admission before lease acquisition" >&2
  exit 1
fi

if ! rg -n 'frame_lease_admission_denies_critical_pressure_before_borrow|frame_lease_admission_denies_voice_exclusive_before_borrow' src/runtime/frame_lease.rs >/dev/null; then
  echo "FAIL: camera frame admission no longer has critical-pressure and voice-exclusive contract tests" >&2
  exit 1
fi

if ! rg -n 'try_acquire_frame_capture_permit' src/tools/analyze_image.rs >/dev/null ||
   ! rg -n 'capture_frame\(max_bytes\)' src/tools/analyze_image.rs >/dev/null ||
   ! rg -n 'vision_request_body_too_large' src/tools/analyze_image.rs >/dev/null; then
  echo "FAIL: analyze_image local camera path no longer proves frame admission and request-body budget checks" >&2
  exit 1
fi

if ! rg -n 'fn metadata\(&self\) -> ToolMetadata' src/tools/analyze_image.rs >/dev/null ||
   ! rg -n 'ToolEffectClass::NetworkSearch' src/tools/analyze_image.rs >/dev/null; then
  echo "FAIL: analyze_image URL vision tool no longer declares network-search metadata" >&2
  exit 1
fi

if ! rg -n '^coredump,[[:space:]]+data,[[:space:]]+coredump,' partitions.csv >/dev/null; then
  echo "FAIL: ESP partition table no longer declares a coredump partition for panic evidence" >&2
  exit 1
fi

if rg -n 'RuntimeMode::Upgrade|upgrade_active|set_upgrade_active|UPGRADE_ACTIVE|runtime\.route_blocked_by_upgrade|allowed_in_upgrade_mode' src >/dev/null; then
  echo "FAIL: upgrade runtime mode is a removed OTA-era contract and must stay absent" >&2
  exit 1
fi

if rg -n 'RouteExecutionClass::OtaRoute|ROUTE_OTA|RouteWorkerLane::Ota|OtaHttpWorker|PlaneId::Ota|http_ota_exec|/api/ota' src >/dev/null; then
  echo "FAIL: official OTA route/worker contract must stay removed until a real implementation is restored" >&2
  exit 1
fi

if rg -n 'StorageDetachedWorkStore|REL_PATH_DETACHED_WORKS|memory/detached_works\.json' src/platform/esp32.rs >/dev/null; then
  echo "FAIL: ESP must not inject persisted detached_work storage; use the volatile system queue/store boundary instead" >&2
  exit 1
fi

if ! rg -n 'official_ota_route_is_not_exposed_without_an_implementation' src/platform/http_server/router/catalog.rs >/dev/null ||
   ! rg -n 'official_ota_worker_plane_is_not_registered_without_an_implementation' src/runtime/plane.rs >/dev/null ||
   ! rg -n 'http_worker_profiles_do_not_claim_precise_tls_handshake_lease_yet' src/runtime/plane.rs >/dev/null; then
  echo "FAIL: OTA removal tests no longer prove the dead route and worker stay absent" >&2
  exit 1
fi

if [[ ! -x scripts/parse_esp_panic_log.sh ]]; then
  echo "FAIL: panic log parser script is missing or not executable" >&2
  exit 1
fi
if ! rg -n 'suggested_symbolization_command' scripts/parse_esp_panic_log.sh >/dev/null ||
   ! rg -n -- '--artifact-dir' scripts/parse_esp_panic_log.sh scripts/esp_symbolize_panic.sh >/dev/null; then
  echo "FAIL: panic parser/symbolizer no longer preserves artifact-directory symbolization hints" >&2
  exit 1
fi

if ! rg -n 'pub struct CrashMetadataSnapshot' src/orchestrator/state.rs >/dev/null ||
   ! rg -n 'record_crash_metadata|register_crash_metadata_provider|crash_metadata_snapshot' src/orchestrator/mod.rs src/orchestrator/state.rs >/dev/null ||
   ! rg -n 'pub mod crash_evidence' src/platform/mod.rs >/dev/null ||
   ! rg -n 'esp_reset_reason|ESP_RST_PANIC|ESP_RST_TASK_WDT|ESP_RST_CPU_LOCKUP' src/platform/crash_evidence.rs >/dev/null; then
  echo "FAIL: crash metadata evidence source contract is incomplete" >&2
  exit 1
fi

if ! rg -n 'FirmwareIdentitySnapshot|booted_artifact_id|last_attempted_artifact_id' src/platform/firmware_identity.rs >/dev/null; then
  echo "FAIL: firmware artifact identity snapshot source is incomplete" >&2
  exit 1
fi

"$SCRIPT_DIR/check_api_observability_contracts.sh"

# P0.3 timed-wait allowlist:
# - platform/audio_drivers.rs keeps std-compatible audio ring waits; this worker
#   must not be moved to ESP native tasks without a separate audio-ring rewrite.
# - bg_timer, channel send/dispatch are steady scheduler loops, not lazy
#   spawn/idle-stop workers; they remain classified for later P1/P2 cleanup.

echo "OK: runtime governance checks passed"
exit 0
