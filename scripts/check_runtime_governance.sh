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

if ! rg -n 'let _route_worker_lease = match acquire_route_worker_lease\s*\(' src/platform/http_server/esp_transport.rs >/dev/null; then
  echo "FAIL: ESP route workers no longer acquire runtime route-worker leases at the execution-window call site" >&2
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

if ! rg -n 'threads:\s*runtime::thread_registry::ThreadRegistrySnapshot' src/platform/http_server/handlers/resource.rs >/dev/null; then
  echo "FAIL: /api/resource no longer exposes thread registry summary" >&2
  exit 1
fi

if ! rg -n 'admission:\s*orchestrator::ResourceAdmissionSnapshot' src/platform/http_server/handlers/resource.rs >/dev/null; then
  echo "FAIL: /api/resource no longer exposes admission summary" >&2
  exit 1
fi

if ! rg -n 'governance_metrics:\s*orchestrator::ResourceGovernanceMetricsSnapshot' src/platform/http_server/handlers/resource.rs >/dev/null; then
  echo "FAIL: /api/resource no longer exposes governance metrics summary" >&2
  exit 1
fi

if ! rg -n 'runtime_capabilities:\s*Vec<orchestrator::RuntimeCapabilityState>' src/platform/http_server/handlers/resource.rs >/dev/null ||
   ! rg -n 'runtime_capabilities:\s*Vec<crate::orchestrator::RuntimeCapabilityState>' src/orchestrator/state.rs >/dev/null ||
   ! rg -n 'runtime_capability_snapshot\s*\(' src/orchestrator/state.rs >/dev/null; then
  echo "FAIL: /api/resource no longer exposes runtime capability diagnostic snapshot" >&2
  exit 1
fi

if ! rg -n 'resource_diagnostic_snapshot\s*\(' src/platform/http_server/handlers/resource.rs src/orchestrator/mod.rs >/dev/null; then
  echo "FAIL: /api/resource no longer uses orchestrator diagnostic resource aggregation" >&2
  exit 1
fi
if ! rg -n 'resource_route_uses_snapshot_worker_on_esp' src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: /api/resource must use the snapshot route worker on ESP instead of the HTTPD callback" >&2
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

for metric in \
  record_runtime_spawn_failure \
  record_http_route_reject \
  record_lease_conflict \
  record_lease_expired_replacement \
  record_plane_drain_timeout; do
  if ! rg -n "$metric" src/metrics.rs >/dev/null; then
    echo "FAIL: runtime governance metric missing from metrics.rs: $metric" >&2
    exit 1
  fi
done

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
   ! rg -n 'record_plane_drain_timeout\(\)' src/network/mod.rs >/dev/null; then
  echo "FAIL: external WSS drain timeout no longer records runtime governance failure metrics" >&2
  exit 1
fi

if ! rg -n 'VoiceExclusiveTransportGuard::enter\(cfg\.platform\.as_ref\(\),\s*TAG\)' src/audio/voice_session.rs >/dev/null ||
   ! rg -n 'realtime voice transport admission failed' src/audio/voice_session.rs >/dev/null; then
  echo "FAIL: realtime voice no longer treats external WSS drain failure as transport admission failure" >&2
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

if ! rg -n 'RuntimeMode::Upgrade' src/runtime/mode.rs src/platform/http_server/router/catalog.rs >/dev/null ||
   ! rg -n 'upgrade_active' src/runtime/mode.rs src/runtime/thread_registry.rs src/state.rs >/dev/null ||
   ! rg -n 'set_upgrade_active' src/runtime/governance.rs src/runtime/mod.rs src/state.rs >/dev/null; then
  echo "FAIL: upgrade runtime mode is no longer connected to the global runtime source" >&2
  exit 1
fi

if rg -n 'RouteExecutionClass::OtaRoute|ROUTE_OTA|RouteWorkerLane::Ota|OtaHttpWorker|PlaneId::Ota|http_ota_exec|/api/ota' src >/dev/null; then
  echo "FAIL: official OTA route/worker contract must stay removed until a real implementation is restored" >&2
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
   ! rg -n 'esp_reset_reason|ESP_RST_PANIC|ESP_RST_TASK_WDT|ESP_RST_CPU_LOCKUP' src/platform/crash_evidence.rs >/dev/null ||
   ! rg -n '"crash"|last_panic_pc|last_resource_baseline_before_panic' src/platform/http_server/handlers/resource.rs >/dev/null; then
  echo "FAIL: crash metadata evidence source/resource diagnostics contract is incomplete" >&2
  exit 1
fi

if ! rg -n 'FirmwareIdentitySnapshot|booted_artifact_id|last_attempted_artifact_id' src/platform/firmware_identity.rs >/dev/null ||
   ! rg -n 'firmware_identity' src/platform/http_server/handlers/resource.rs >/dev/null; then
  echo "FAIL: firmware artifact identity is no longer exposed through resource diagnostics" >&2
  exit 1
fi

# P0.3 timed-wait allowlist:
# - platform/audio_drivers.rs keeps std-compatible audio ring waits; this worker
#   must not be moved to ESP native tasks without a separate audio-ring rewrite.
# - bg_timer, channel send/dispatch are steady scheduler loops, not lazy
#   spawn/idle-stop workers; they remain classified for later P1/P2 cleanup.

echo "OK: runtime governance checks passed"
exit 0
