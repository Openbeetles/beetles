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
PROTECTED_WRITE_BACK_LOG="$TMP_DIR/protected-write-back.log"
WAKENET_BAD_LOG="$TMP_DIR/wakenet-bad.log"
OUT="$TMP_DIR/out"
HEALTHY_OUT="$TMP_DIR/healthy-out"
SPACED_OUT="$TMP_DIR/spaced-out"
SCHEDULER_HEALTHY_OUT="$TMP_DIR/scheduler-healthy-out"
SCHEDULER_BAD_OUT="$TMP_DIR/scheduler-bad-out"
PROTECTED_WRITE_BACK_OUT="$TMP_DIR/protected-write-back-out"
WAKENET_BAD_OUT="$TMP_DIR/wakenet-bad-out"

cat >"$LOG" <<'LOGEOF'
I (900) beetle::orchestrator: [orchestrator] startup memory checkpoint stage=agent_loop_deferred internal_free=45807 internal_min=45807 largest_block=31744 pressure=Cautious tls_fragmentation=Healthy
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
I (61500) beetle::heartbeat: [heartbeat] threads alive=7 historical=12 stack_total=96256 io=2 interactive=1 background=4 core0=2 core1=5 unpinned=0 std_compat=7 native=0 native_allowlist_hits=0 native_std_sync_forbidden=0 twdt_owner=3 twdt_feed_only=3 twdt_unmanaged=1 tls=1 http=2 wss=0 mode_sensitive=4 high_risk=2 critical=1 low_margin=1 hw_samples=7
I (61510) beetle::heartbeat: [heartbeat] thread_stack stack_hw_supported=true sampled=7 low_margin=1 low=voice_session:Medium:budget=8192 free=1112 top=agent_loop:Critical:budget=40960 free=9204,wifi_worker:High:budget=8192 free=4632,voice_session:Medium:budget=8192 free=1112
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
grep -q 'stack_low_margin.*low=voice_session:Medium:budget=8192 free=1112' "$REGRESSIONS"
grep -q 'heap_largest_below_floor' "$REGRESSIONS"
grep -q 'startup_heap_largest_below_floor.*stage=agent_loop_deferred' "$REGRESSIONS"
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
grep -q 'Stack low-margin lines: 1' "$SUMMARY"
grep -q 'Heap largest below floor risk lines: 3' "$SUMMARY"
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
I (1008) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=optional_maintenance source=background decision=defer reason=foreground_active retry_after_ms=29500 foreground_active=true foreground_source=external_user_message resume_after_ms=29500 profile=esp_compact pressure=Normal
I (1010) beetle::chat_stream: [chat_stream] event=final stream_id=chat_stream_1 session_appended=true message_id_present=true
I (1011) beetle::agent: [agent] primary_delivery event=outbound_enqueued delivered=true req_id=req-1 channel=qq_channel chat_id=chat-1
I (31001) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=deep_route_worker source=background decision=proceed reason=none retry_after_ms=none foreground_active=false foreground_source=none resume_after_ms=none profile=esp_compact pressure=Normal
I (31002) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=realtime_voice_session source=background decision=proceed reason=none retry_after_ms=none foreground_active=false foreground_source=none resume_after_ms=none profile=esp_compact pressure=Normal
I (31003) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=durable_write_back source=background decision=proceed reason=none retry_after_ms=none foreground_active=false foreground_source=none resume_after_ms=none profile=esp_compact pressure=Normal
I (31004) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=display_heavy_refresh source=background decision=proceed reason=none retry_after_ms=none foreground_active=false foreground_source=none resume_after_ms=none profile=esp_compact pressure=Normal
I (32000) beetle::heartbeat: [heartbeat] runtime_scheduler active_foreground=false source=none age_ms=none resume_after_ms=none foreground_recovery_active=true recovery_source=external_user_message recovery_age_ms=500 recovery_resume_after_ms=9500 active_work=0 profile=esp_compact permits=3 defers=4 degrades=1 suspends=0 drains=0 rejects=0 last_class=display_heavy_refresh last_source=background last_decision=degrade last_reason=foreground_active last_retry_after_ms=none
I (32001) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=durable_write_back source=background decision=defer reason=foreground_recovery retry_after_ms=9500 foreground_active=false foreground_source=none resume_after_ms=none foreground_recovery_active=true foreground_recovery_source=external_user_message recovery_resume_after_ms=9500 profile=esp_compact pressure=Normal
I (32002) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=display_heavy_refresh source=background decision=degrade reason=foreground_recovery retry_after_ms=none foreground_active=false foreground_source=none resume_after_ms=none foreground_recovery_active=true foreground_recovery_source=external_user_message recovery_resume_after_ms=9500 profile=esp_compact pressure=Normal
I (32003) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=channel_reconnect source=background decision=proceed reason=none retry_after_ms=none foreground_active=false foreground_source=none resume_after_ms=none foreground_recovery_active=true foreground_recovery_source=external_user_message recovery_resume_after_ms=9500 profile=esp_compact pressure=Normal
I (42001) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=durable_write_back source=background decision=proceed reason=none retry_after_ms=none foreground_active=false foreground_source=none resume_after_ms=none foreground_recovery_active=false foreground_recovery_source=none recovery_resume_after_ms=none profile=esp_compact pressure=Normal
LOGEOF

"$ROOT/scripts/esp_soak_analyze.sh" --output-dir "$SCHEDULER_HEALTHY_OUT" "$SCHEDULER_HEALTHY_LOG" >/dev/null
SCHEDULER_HEALTHY_SUMMARY="$(find "$SCHEDULER_HEALTHY_OUT" -name summary.md -print -quit)"
SCHEDULER_HEALTHY_REGRESSIONS="$(find "$SCHEDULER_HEALTHY_OUT" -name regressions.csv -print -quit)"
SCHEDULER_HEALTHY_METRICS="$(find "$SCHEDULER_HEALTHY_OUT" -name metrics.csv -print -quit)"

grep -q 'scheduler_active_foreground,scheduler_recovery_active,scheduler_last_class,scheduler_last_decision,scheduler_defers,scheduler_degrades,scheduler_rejects' "$SCHEDULER_HEALTHY_METRICS"
grep -q 'Scheduler foreground samples: 1' "$SCHEDULER_HEALTHY_SUMMARY"
grep -q 'Scheduler post-foreground recovery samples: 4' "$SCHEDULER_HEALTHY_SUMMARY"
grep -q 'Scheduler defer decisions: 5' "$SCHEDULER_HEALTHY_SUMMARY"
grep -q 'Scheduler degrade decisions: 2' "$SCHEDULER_HEALTHY_SUMMARY"
grep -q 'Scheduler resume decisions: 5' "$SCHEDULER_HEALTHY_SUMMARY"
grep -q 'Post-foreground recovery violations: 0' "$SCHEDULER_HEALTHY_SUMMARY"
! grep -q 'foreground_ack_missing_before_llm' "$SCHEDULER_HEALTHY_REGRESSIONS"
! grep -q 'primary_generated_but_not_delivered' "$SCHEDULER_HEALTHY_REGRESSIONS"
! grep -q 'scheduler_resume_missing' "$SCHEDULER_HEALTHY_REGRESSIONS"
! grep -q 'deep_worker_not_deferred_during_foreground' "$SCHEDULER_HEALTHY_REGRESSIONS"
! grep -q 'voice_auto_connect_not_suppressed' "$SCHEDULER_HEALTHY_REGRESSIONS"
! grep -q 'write_back_started_during_foreground' "$SCHEDULER_HEALTHY_REGRESSIONS"
! grep -q 'display_status_missing_during_degrade' "$SCHEDULER_HEALTHY_REGRESSIONS"
! grep -q 'post_foreground_recovery_violation' "$SCHEDULER_HEALTHY_REGRESSIONS"

cat >"$PROTECTED_WRITE_BACK_LOG" <<'LOGEOF'
I (1000) beetle::heartbeat: [heartbeat] runtime_scheduler active_foreground=false source=none age_ms=none resume_after_ms=none foreground_recovery_active=true recovery_source=external_user_message recovery_age_ms=100 recovery_resume_after_ms=9900 active_work=0 profile=esp_compact permits=1 defers=1 degrades=0 suspends=0 drains=0 rejects=0 last_class=durable_write_back last_source=background last_decision=defer last_reason=foreground_recovery last_retry_after_ms=9900
I (1010) beetle::heartbeat: [heartbeat] write_back queued=4 worker_started=false deferred_total=10 dropped_total=0 coalesced_total=0 worker_starts_total=1
I (31000) beetle::heartbeat: [heartbeat] runtime_scheduler active_foreground=false source=none age_ms=none resume_after_ms=none foreground_recovery_active=true recovery_source=external_user_message recovery_age_ms=500 recovery_resume_after_ms=9500 active_work=0 profile=esp_compact permits=2 defers=2 degrades=0 suspends=0 drains=0 rejects=0 last_class=durable_write_back last_source=background last_decision=defer last_reason=foreground_recovery last_retry_after_ms=9500
I (31010) beetle::heartbeat: [heartbeat] write_back queued=4 worker_started=false deferred_total=220 dropped_total=0 coalesced_total=0 worker_starts_total=1
I (61000) beetle::heartbeat: [heartbeat] runtime_scheduler active_foreground=false source=none age_ms=none resume_after_ms=none foreground_recovery_active=true recovery_source=external_user_message recovery_age_ms=900 recovery_resume_after_ms=9100 active_work=0 profile=esp_compact permits=3 defers=3 degrades=0 suspends=0 drains=0 rejects=0 last_class=durable_write_back last_source=background last_decision=defer last_reason=foreground_recovery last_retry_after_ms=9100
I (61010) beetle::heartbeat: [heartbeat] write_back queued=4 worker_started=false deferred_total=430 dropped_total=0 coalesced_total=0 worker_starts_total=1
I (71000) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=durable_write_back source=background decision=proceed reason=none retry_after_ms=none foreground_active=false foreground_source=none resume_after_ms=none foreground_recovery_active=false foreground_recovery_source=none recovery_resume_after_ms=none profile=esp_compact pressure=Normal
I (71010) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (91000) beetle::heartbeat: [heartbeat] runtime_scheduler active_foreground=false source=none age_ms=none resume_after_ms=none foreground_recovery_active=false recovery_source=none recovery_age_ms=none recovery_resume_after_ms=none active_work=0 profile=esp_compact permits=4 defers=3 degrades=0 suspends=0 drains=0 rejects=0 last_class=durable_write_back last_source=background last_decision=proceed last_reason=none last_retry_after_ms=none
I (91010) beetle::heartbeat: [heartbeat] write_back queued=0 worker_started=true deferred_total=430 dropped_total=0 coalesced_total=0 worker_starts_total=2
LOGEOF

"$ROOT/scripts/esp_soak_analyze.sh" --output-dir "$PROTECTED_WRITE_BACK_OUT" "$PROTECTED_WRITE_BACK_LOG" >/dev/null
PROTECTED_WRITE_BACK_SUMMARY="$(find "$PROTECTED_WRITE_BACK_OUT" -name summary.md -print -quit)"
PROTECTED_WRITE_BACK_REGRESSIONS="$(find "$PROTECTED_WRITE_BACK_OUT" -name regressions.csv -print -quit)"

grep -q 'Write-back starvation lines: 0' "$PROTECTED_WRITE_BACK_SUMMARY"
grep -q 'Write-back defer churn lines: 0' "$PROTECTED_WRITE_BACK_SUMMARY"
! grep -q 'pending_write_back_starvation' "$PROTECTED_WRITE_BACK_REGRESSIONS"
! grep -q 'write_back_defer_churn' "$PROTECTED_WRITE_BACK_REGRESSIONS"

{
  for i in $(seq 1 25); do
    printf 'W (%d) AFE: Ringbuffer of AFE is empty, Please use feed() to write data\n' "$((3000 + i * 10))"
  done
  printf 'I (35398) beetle::heartbeat: [heartbeat] audio_wake worker_turns=26 worker_idle=3 mic_polls=23 mic_frames=23 mic_zero=0 mic_read_us=319 feed_calls=23 feed_busy=0 feed_cooldown=0 feed_detect=0 feed_us=1407298 mic_level_pm=0 zcr_pm=0 speech_ratio_pm=0 speech_coverage_pm=0 speech_dominance_pm=0 activation_pm=0 speech_like=false ref_ok=false\n'
} >"$WAKENET_BAD_LOG"

"$ROOT/scripts/esp_soak_analyze.sh" --output-dir "$WAKENET_BAD_OUT" "$WAKENET_BAD_LOG" >/dev/null
WAKENET_BAD_SUMMARY="$(find "$WAKENET_BAD_OUT" -name summary.md -print -quit)"
WAKENET_BAD_REGRESSIONS="$(find "$WAKENET_BAD_OUT" -name regressions.csv -print -quit)"

grep -q 'wakenet_afe_empty_spam' "$WAKENET_BAD_REGRESSIONS"
grep -q 'wakenet_feed_hot_path_slow' "$WAKENET_BAD_REGRESSIONS"
grep -q 'WakeNet AFE empty lines: 25' "$WAKENET_BAD_SUMMARY"
grep -q 'WakeNet slow feed lines: 1' "$WAKENET_BAD_SUMMARY"

cat >"$SCHEDULER_BAD_LOG" <<'LOGEOF'
I (1000) beetle::heartbeat: [heartbeat] runtime_scheduler active_foreground=true source=external_user_message age_ms=500 resume_after_ms=29500 active_work=1 profile=esp_compact permits=1 defers=0 degrades=0 suspends=0 drains=0 rejects=0 last_class=external_user_message last_source=user_facing last_decision=proceed last_reason=none last_retry_after_ms=none
I (1001) beetle::agent: llm_turn event=start req_id=req-2
I (1002) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=deep_route_worker source=background decision=proceed reason=none retry_after_ms=none foreground_active=true foreground_source=external_user_message resume_after_ms=29500 profile=esp_compact pressure=Normal
I (1003) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=realtime_voice_session source=background decision=proceed reason=none retry_after_ms=none foreground_active=true foreground_source=external_user_message resume_after_ms=29500 profile=esp_compact pressure=Normal
I (1004) beetle::heartbeat: [heartbeat] write_back queued=1 worker_started=true deferred_total=0 dropped_total=0 coalesced_total=0 worker_starts_total=1
I (1005) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=display_heavy_refresh source=background decision=degrade reason=foreground_active retry_after_ms=none foreground_active=true foreground_source=external_user_message resume_after_ms=29500 profile=esp_compact pressure=Normal
I (1005) beetle::runtime: [runtime_scheduler] runtime_scheduler_decision class=durable_write_back source=background decision=proceed reason=none retry_after_ms=none foreground_active=false foreground_source=none resume_after_ms=none foreground_recovery_active=true foreground_recovery_source=external_user_message recovery_resume_after_ms=9500 profile=esp_compact pressure=Normal
I (1005) beetle::util: [thread] started name=qq_ws core_target=Some(Core0) role=Io surface=StdThreadCompat native_std_sync_forbidden=false
I (1005) beetle::channels::wss_gateway::r#loop: [qq_ws] WiFi STA not ready, waiting up to 60s
I (1005) beetle::util: [thread] started name=voice_realtime_connect core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (1005) beetle::heartbeat: [heartbeat] runtime_mode current_mode=normal wifi_sta=false booting=false pairing_known=true pairing_required=false voice_exclusive=false bg_maintenance=false recovery_safe_mode=false config_plane=true config_active=false config_phase=idle channel_plane=true voice_plane=true agent_plane=true foreground_active=false foreground_source=none foreground_age_ms=none foreground_resume_after_ms=none foreground_recovery_active=false foreground_recovery_source=none foreground_recovery_age_ms=none foreground_recovery_resume_after_ms=none ext_wss_connecting=0 timers=true periodic_maintenance=true non_voice_outbound=true realtime_voice=true ext_wss_connect=true ext_wss_suspend=false
I (1005) beetle::heartbeat: [heartbeat] runtime_mode current_mode=voice_exclusive wifi_sta=false booting=false pairing_known=true pairing_required=false voice_exclusive=true bg_maintenance=false recovery_safe_mode=false config_plane=true config_active=false config_phase=idle channel_plane=true voice_plane=true agent_plane=true foreground_active=true foreground_source=realtime_voice_session foreground_age_ms=10 foreground_resume_after_ms=29990 foreground_recovery_active=false foreground_recovery_source=none foreground_recovery_age_ms=none foreground_recovery_resume_after_ms=none ext_wss_connecting=0 timers=true periodic_maintenance=false non_voice_outbound=false realtime_voice=true ext_wss_connect=false ext_wss_suspend=true
I (1006) beetle::channels::qq::send: [qq_send] send status=400 body={"code":40054005,"message":"消息被去重，请检查请求msgseq"} chat_id=c2c:chat-1 chunk=1/1 http_ms=110 total_ms=110
I (1007) beetle::orchestrator: [orchestrator] startup memory checkpoint stage=voice_session_spawn internal_free=67347 internal_min=67347 largest_block=30720 pressure=Cautious tls_fragmentation=Healthy
***ERROR*** A stack overflow in task pthread has been detected.
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
grep -q 'post_foreground_recovery_violation' "$SCHEDULER_BAD_REGRESSIONS"
grep -q 'scheduler_resume_missing' "$SCHEDULER_BAD_REGRESSIONS"
grep -q 'qq_msgseq_regression' "$SCHEDULER_BAD_REGRESSIONS"
grep -q 'voice_session_stack_overflow' "$SCHEDULER_BAD_REGRESSIONS"
grep -q 'external_wss_worker_before_network_ready' "$SCHEDULER_BAD_REGRESSIONS"
grep -q 'voice_realtime_connect_before_network_ready' "$SCHEDULER_BAD_REGRESSIONS"
grep -q 'boot_normal_before_network_ready' "$SCHEDULER_BAD_REGRESSIONS"
grep -q 'voice_exclusive_before_network_ready' "$SCHEDULER_BAD_REGRESSIONS"
