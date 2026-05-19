#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "$ROOT/target/test-artifacts"
TMP_DIR="$(mktemp -d "$ROOT/target/test-artifacts/esp-soak-analyze.XXXXXX")"
LOG="$TMP_DIR/serial.log"
HEALTHY_LOG="$TMP_DIR/healthy-write-back.log"
SPACED_LOG="$TMP_DIR/spaced-write-back.log"
SCHEDULER_HEALTHY_LOG="$TMP_DIR/healthy-scheduler.log"
SCHEDULER_BAD_LOG="$TMP_DIR/bad-scheduler.log"
OUT="$TMP_DIR/out"
HEALTHY_OUT="$TMP_DIR/healthy-out"
SPACED_OUT="$TMP_DIR/spaced-out"
SCHEDULER_HEALTHY_OUT="$TMP_DIR/scheduler-healthy-out"
SCHEDULER_BAD_OUT="$TMP_DIR/scheduler-bad-out"

cat >"$LOG" <<'LOGEOF'
I (900) beetle::orchestrator: [orchestrator] startup memory checkpoint stage=agent_loop_spawn internal_free=45807 internal_min=45807 largest_block=31744 pressure=Cautious tls_fragmentation=Healthy
I (950) beetle::orchestrator: [orchestrator] startup memory checkpoint stage=http_snapshot_exec_spawn internal_free=45807 internal_min=45807 largest_block=31744 pressure=Cautious tls_fragmentation=Healthy
I (1000) beetle::heartbeat: [heartbeat] HEARTBEAT version=0.1.0 uptime_secs=10 resource pressure=Normal tls_fragmentation=Healthy storage_contention=Healthy heap_internal_free=60000 heap_largest_internal=31744 active_http=0 active_wss=1 agent_tasks=0 inbound=0 outbound=0
I (1005) beetle::heartbeat: [heartbeat] metrics storage_ops=2 storage_contention=0 storage_wait_last_us=0 storage_wait_total_us=0 storage_hold_last_us=100 storage_hold_total_us=100 storage_hold_last_stage=storage_read storage_last_age_ms=10
I (1010) beetle::heartbeat: [heartbeat] write_back queued=4 worker_started=false deferred_total=100 dropped_total=0 coalesced_total=0 worker_starts_total=2
I (31000) beetle::heartbeat: [heartbeat] HEARTBEAT version=0.1.0 uptime_secs=40 resource pressure=Normal tls_fragmentation=Healthy storage_contention=Healthy heap_internal_free=60000 heap_largest_internal=31744 active_http=0 active_wss=1 agent_tasks=0 inbound=0 outbound=0
I (31005) beetle::heartbeat: [heartbeat] metrics storage_ops=3 storage_contention=1 storage_wait_last_us=5000 storage_wait_total_us=5000 storage_hold_last_us=100 storage_hold_total_us=200 storage_hold_last_stage=storage_write storage_last_age_ms=10
I (31010) beetle::heartbeat: [heartbeat] write_back queued=4 worker_started=false deferred_total=430 dropped_total=0 coalesced_total=0 worker_starts_total=2
I (61000) beetle::heartbeat: [heartbeat] HEARTBEAT version=0.1.0 uptime_secs=70 resource pressure=Normal tls_fragmentation=Healthy storage_contention=Healthy heap_internal_free=60000 heap_largest_internal=31744 active_http=0 active_wss=1 agent_tasks=0 inbound=0 outbound=0
I (61005) beetle::heartbeat: [heartbeat] metrics storage_ops=4 storage_contention=2 storage_wait_last_us=50000 storage_wait_total_us=55000 storage_hold_last_us=100 storage_hold_total_us=300 storage_hold_last_stage=storage_write storage_last_age_ms=10
I (61010) beetle::heartbeat: [heartbeat] write_back queued=4 worker_started=false deferred_total=760 dropped_total=0 coalesced_total=0 worker_starts_total=2
I (62000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (63000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (64000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (65000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (66000) beetle::chat_stream: [chat_stream] event=final stream_id=chat_stream_1 session_appended=true message_id_present=true
I (67000) beetle::chat_stream: [chat_stream] event=error stream_id=chat_stream_2 error_key=chat.stream_timeout error_stage=
LOGEOF

"$ROOT/scripts/esp_soak_analyze.sh" --output-dir "$OUT" "$LOG" >/dev/null

SUMMARY="$(find "$OUT" -name summary.md -print -quit)"
REGRESSIONS="$(find "$OUT" -name regressions.csv -print -quit)"
METRICS="$(find "$OUT" -name metrics.csv -print -quit)"

grep -q 'pending_write_back_starvation' "$REGRESSIONS"
grep -q 'write_back_worker_churn' "$REGRESSIONS"
grep -q 'chat_stream_error' "$REGRESSIONS"
grep -q 'storage_contention_cautious' "$REGRESSIONS"
grep -q 'storage_contention_critical' "$REGRESSIONS"
grep -q 'heap_largest_below_floor' "$REGRESSIONS"
grep -q 'startup_heap_largest_below_floor.*stage=agent_loop_spawn' "$REGRESSIONS"
grep -q 'heap_largest_below_floor.*stage=http_snapshot_exec_spawn' "$REGRESSIONS"
grep -q 'heap_largest_below_observation_floor.*pressure=Normal.*tls_fragmentation=Healthy' "$REGRESSIONS"
grep -q 'storage_ops,storage_wait_last_us,storage_wait_total_us,storage_hold_last_us,storage_hold_total_us,storage_hold_last_stage,storage_last_age_ms' "$METRICS"
! grep -q 'spiffs_' "$METRICS"
grep -q 'Write-back starvation lines: 1' "$SUMMARY"
grep -q 'Write-back worker thread starts: 4' "$SUMMARY"
grep -q 'Chat stream final events: 1' "$SUMMARY"
grep -q 'Chat stream error events: 1' "$SUMMARY"
grep -q 'Storage contention risk lines: 1' "$SUMMARY"
grep -q 'Storage contention blocker lines: 1' "$SUMMARY"
grep -q 'Heap largest below floor risk lines: 4' "$SUMMARY"
grep -q 'Heap largest trend: first=31744 min=31744 max=31744 last=31744' "$SUMMARY"

cat >"$HEALTHY_LOG" <<'LOGEOF'
I (1000) beetle::heartbeat: [heartbeat] HEARTBEAT version=0.1.0 uptime_secs=10 resource pressure=Normal tls_fragmentation=Healthy storage_contention=Healthy heap_internal_free=60000 heap_largest_internal=32768 active_http=0 active_wss=1 agent_tasks=0 inbound=0 outbound=0
I (1005) beetle::heartbeat: [heartbeat] metrics storage_ops=2 storage_contention=0 storage_wait_last_us=0 storage_wait_total_us=0 storage_hold_last_us=100 storage_hold_total_us=100 storage_hold_last_stage=storage_read storage_last_age_ms=10
I (1010) beetle::heartbeat: [heartbeat] write_back queued=0 worker_started=false deferred_total=0 dropped_total=0 coalesced_total=0 worker_starts_total=0
I (2000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (31000) beetle::heartbeat: [heartbeat] HEARTBEAT version=0.1.0 uptime_secs=40 resource pressure=Normal tls_fragmentation=Healthy storage_contention=Healthy heap_internal_free=60000 heap_largest_internal=32768 active_http=0 active_wss=1 agent_tasks=0 inbound=0 outbound=0
I (31005) beetle::heartbeat: [heartbeat] metrics storage_ops=3 storage_contention=0 storage_wait_last_us=0 storage_wait_total_us=0 storage_hold_last_us=100 storage_hold_total_us=200 storage_hold_last_stage=storage_write storage_last_age_ms=10
I (31010) beetle::heartbeat: [heartbeat] write_back queued=0 worker_started=false deferred_total=0 dropped_total=0 coalesced_total=0 worker_starts_total=1
I (62000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (92000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (122000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (152000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
LOGEOF

"$ROOT/scripts/esp_soak_analyze.sh" --output-dir "$HEALTHY_OUT" "$HEALTHY_LOG" >/dev/null

HEALTHY_SUMMARY="$(find "$HEALTHY_OUT" -name summary.md -print -quit)"
HEALTHY_REGRESSIONS="$(find "$HEALTHY_OUT" -name regressions.csv -print -quit)"

grep -q 'Write-back worker thread starts: 5' "$HEALTHY_SUMMARY"
! grep -q 'write_back_worker_churn' "$HEALTHY_REGRESSIONS"
! awk -F',' 'NR > 1 && $3 == "blocker" { found=1 } END { exit found ? 0 : 1 }' "$HEALTHY_REGRESSIONS"

cat >"$SPACED_LOG" <<'LOGEOF'
I (0) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (9000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (18000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (27000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
LOGEOF

"$ROOT/scripts/esp_soak_analyze.sh" --output-dir "$SPACED_OUT" "$SPACED_LOG" >/dev/null
SPACED_REGRESSIONS="$(find "$SPACED_OUT" -name regressions.csv -print -quit)"
! grep -q 'write_back_worker_churn' "$SPACED_REGRESSIONS"

cat >"$SCHEDULER_HEALTHY_LOG" <<'LOGEOF'
I (1000) beetle::heartbeat: [heartbeat] runtime_scheduler active_foreground=true source=external_user_message age_ms=500 resume_after_ms=29500 active_work=1 profile=esp_compact permits=1 defers=0 degrades=0 suspends=0 drains=0 rejects=0 last_class=external_user_message last_source=user_facing last_decision=proceed last_reason=none last_retry_after_ms=none
I (1001) beetle::agent_delivery: [agent_delivery] foreground_ack event=visibility_enqueued before_llm=true owner=current_chat_visibility req_id=req-1 channel=qq_channel chat_id=chat-1
I (1002) beetle::agent: llm_turn event=start req_id=req-1
I (1003) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=deep_route_worker source=background decision=defer reason=foreground_active retry_after_ms=29500 foreground_active=true foreground_source=external_user_message resume_after_ms=29500 profile=esp_compact pressure=Normal
I (1004) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=realtime_voice_session source=background decision=defer reason=foreground_active retry_after_ms=29500 foreground_active=true foreground_source=external_user_message resume_after_ms=29500 profile=esp_compact pressure=Normal
I (1005) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=durable_write_back source=background decision=defer reason=foreground_active retry_after_ms=29500 foreground_active=true foreground_source=external_user_message resume_after_ms=29500 profile=esp_compact pressure=Normal
I (1006) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=display_heavy_refresh source=background decision=degrade reason=foreground_active retry_after_ms=none foreground_active=true foreground_source=external_user_message resume_after_ms=29500 profile=esp_compact pressure=Normal
I (1007) beetle::main: [main] display_status_surface retained=true heavy_refresh_degraded=true header=true ip=false footer=true
I (1010) beetle::chat_stream: [chat_stream] event=final stream_id=chat_stream_1 session_appended=true message_id_present=true
I (1011) beetle::agent: [agent] primary_delivery event=outbound_enqueued delivered=true req_id=req-1 channel=qq_channel chat_id=chat-1
I (31000) beetle::heartbeat: [heartbeat] runtime_scheduler active_foreground=false source=none age_ms=none resume_after_ms=none active_work=0 profile=esp_compact permits=2 defers=3 degrades=1 suspends=0 drains=0 rejects=0 last_class=display_heavy_refresh last_source=background last_decision=degrade last_reason=foreground_active last_retry_after_ms=none
I (31001) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=deep_route_worker source=background decision=proceed reason=none retry_after_ms=none foreground_active=false foreground_source=none resume_after_ms=none profile=esp_compact pressure=Normal
I (31002) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=realtime_voice_session source=background decision=proceed reason=none retry_after_ms=none foreground_active=false foreground_source=none resume_after_ms=none profile=esp_compact pressure=Normal
I (31003) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=durable_write_back source=background decision=proceed reason=none retry_after_ms=none foreground_active=false foreground_source=none resume_after_ms=none profile=esp_compact pressure=Normal
I (31004) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=display_heavy_refresh source=background decision=proceed reason=none retry_after_ms=none foreground_active=false foreground_source=none resume_after_ms=none profile=esp_compact pressure=Normal
LOGEOF

"$ROOT/scripts/esp_soak_analyze.sh" --output-dir "$SCHEDULER_HEALTHY_OUT" "$SCHEDULER_HEALTHY_LOG" >/dev/null
SCHEDULER_HEALTHY_SUMMARY="$(find "$SCHEDULER_HEALTHY_OUT" -name summary.md -print -quit)"
SCHEDULER_HEALTHY_REGRESSIONS="$(find "$SCHEDULER_HEALTHY_OUT" -name regressions.csv -print -quit)"
SCHEDULER_HEALTHY_METRICS="$(find "$SCHEDULER_HEALTHY_OUT" -name metrics.csv -print -quit)"

grep -q 'scheduler_active_foreground,scheduler_last_class,scheduler_last_decision,scheduler_defers,scheduler_degrades,scheduler_rejects' "$SCHEDULER_HEALTHY_METRICS"
grep -q 'Scheduler foreground samples: 1' "$SCHEDULER_HEALTHY_SUMMARY"
grep -q 'Scheduler defer decisions: 3' "$SCHEDULER_HEALTHY_SUMMARY"
grep -q 'Scheduler degrade decisions: 2' "$SCHEDULER_HEALTHY_SUMMARY"
grep -q 'Scheduler resume decisions: 4' "$SCHEDULER_HEALTHY_SUMMARY"
! grep -q 'foreground_ack_missing_before_llm' "$SCHEDULER_HEALTHY_REGRESSIONS"
! grep -q 'primary_generated_but_not_delivered' "$SCHEDULER_HEALTHY_REGRESSIONS"
! grep -q 'scheduler_resume_missing' "$SCHEDULER_HEALTHY_REGRESSIONS"
! grep -q 'deep_worker_not_deferred_during_foreground' "$SCHEDULER_HEALTHY_REGRESSIONS"
! grep -q 'voice_auto_connect_not_suppressed' "$SCHEDULER_HEALTHY_REGRESSIONS"
! grep -q 'write_back_started_during_foreground' "$SCHEDULER_HEALTHY_REGRESSIONS"
! grep -q 'display_status_missing_during_degrade' "$SCHEDULER_HEALTHY_REGRESSIONS"

cat >"$SCHEDULER_BAD_LOG" <<'LOGEOF'
I (1000) beetle::heartbeat: [heartbeat] runtime_scheduler active_foreground=true source=external_user_message age_ms=500 resume_after_ms=29500 active_work=1 profile=esp_compact permits=1 defers=0 degrades=0 suspends=0 drains=0 rejects=0 last_class=external_user_message last_source=user_facing last_decision=proceed last_reason=none last_retry_after_ms=none
I (1001) beetle::agent: llm_turn event=start req_id=req-2
I (1002) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=deep_route_worker source=background decision=proceed reason=none retry_after_ms=none foreground_active=true foreground_source=external_user_message resume_after_ms=29500 profile=esp_compact pressure=Normal
I (1003) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=realtime_voice_session source=background decision=proceed reason=none retry_after_ms=none foreground_active=true foreground_source=external_user_message resume_after_ms=29500 profile=esp_compact pressure=Normal
I (1004) beetle::heartbeat: [heartbeat] write_back queued=1 worker_started=true deferred_total=0 dropped_total=0 coalesced_total=0 worker_starts_total=1
I (1005) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=display_heavy_refresh source=background decision=degrade reason=foreground_active retry_after_ms=none foreground_active=true foreground_source=external_user_message resume_after_ms=29500 profile=esp_compact pressure=Normal
I (1010) beetle::chat_stream: [chat_stream] event=final stream_id=chat_stream_2 session_appended=false message_id_present=false
I (1011) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=durable_write_back source=background decision=defer reason=foreground_active retry_after_ms=29500 foreground_active=true foreground_source=external_user_message resume_after_ms=29500 profile=esp_compact pressure=Normal
LOGEOF

"$ROOT/scripts/esp_soak_analyze.sh" --output-dir "$SCHEDULER_BAD_OUT" "$SCHEDULER_BAD_LOG" >/dev/null
SCHEDULER_BAD_REGRESSIONS="$(find "$SCHEDULER_BAD_OUT" -name regressions.csv -print -quit)"

grep -q 'foreground_ack_missing_before_llm' "$SCHEDULER_BAD_REGRESSIONS"
grep -q 'primary_generated_but_not_delivered' "$SCHEDULER_BAD_REGRESSIONS"
grep -q 'deep_worker_not_deferred_during_foreground' "$SCHEDULER_BAD_REGRESSIONS"
grep -q 'voice_auto_connect_not_suppressed' "$SCHEDULER_BAD_REGRESSIONS"
grep -q 'write_back_started_during_foreground' "$SCHEDULER_BAD_REGRESSIONS"
grep -q 'display_status_missing_during_degrade' "$SCHEDULER_BAD_REGRESSIONS"
grep -q 'scheduler_resume_missing' "$SCHEDULER_BAD_REGRESSIONS"
