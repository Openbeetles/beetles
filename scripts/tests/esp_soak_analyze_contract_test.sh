#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "$ROOT/target/test-artifacts"
TMP_DIR="$(mktemp -d "$ROOT/target/test-artifacts/esp-soak-analyze.XXXXXX")"
LOG="$TMP_DIR/serial.log"
OUT="$TMP_DIR/out"

cat >"$LOG" <<'LOGEOF'
I (900) beetle::orchestrator: [orchestrator] startup memory checkpoint stage=agent_loop_spawn internal_free=45807 internal_min=45807 largest_block=31744 pressure=Cautious tls_fragmentation=Healthy
I (950) beetle::orchestrator: [orchestrator] startup memory checkpoint stage=http_snapshot_exec_spawn internal_free=45807 internal_min=45807 largest_block=31744 pressure=Cautious tls_fragmentation=Healthy
I (1000) beetle::heartbeat: [heartbeat] HEARTBEAT version=0.1.0 uptime_secs=10 resource pressure=Normal tls_fragmentation=Healthy storage_contention=Healthy heap_internal_free=60000 heap_largest_internal=31744 active_http=0 active_wss=1 agent_tasks=0 inbound=0 outbound=0
I (1010) beetle::heartbeat: [heartbeat] write_back queued=4 worker_started=false deferred_total=100 dropped_total=0 coalesced_total=0 worker_starts_total=2
I (31000) beetle::heartbeat: [heartbeat] HEARTBEAT version=0.1.0 uptime_secs=40 resource pressure=Normal tls_fragmentation=Healthy storage_contention=Healthy heap_internal_free=60000 heap_largest_internal=31744 active_http=0 active_wss=1 agent_tasks=0 inbound=0 outbound=0
I (31010) beetle::heartbeat: [heartbeat] write_back queued=4 worker_started=false deferred_total=430 dropped_total=0 coalesced_total=0 worker_starts_total=2
I (61000) beetle::heartbeat: [heartbeat] HEARTBEAT version=0.1.0 uptime_secs=70 resource pressure=Normal tls_fragmentation=Healthy storage_contention=Healthy heap_internal_free=60000 heap_largest_internal=31744 active_http=0 active_wss=1 agent_tasks=0 inbound=0 outbound=0
I (61010) beetle::heartbeat: [heartbeat] write_back queued=4 worker_started=false deferred_total=760 dropped_total=0 coalesced_total=0 worker_starts_total=2
I (62000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (63000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (64000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
I (65000) beetle::util: [thread] started name=write_back core_target=Some(Core1) role=Background surface=StdThreadCompat native_std_sync_forbidden=false
LOGEOF

"$ROOT/scripts/esp_soak_analyze.sh" --output-dir "$OUT" "$LOG" >/dev/null

SUMMARY="$(find "$OUT" -name summary.md -print -quit)"
REGRESSIONS="$(find "$OUT" -name regressions.csv -print -quit)"

grep -q 'pending_write_back_starvation' "$REGRESSIONS"
grep -q 'write_back_worker_churn' "$REGRESSIONS"
grep -q 'heap_largest_below_floor' "$REGRESSIONS"
grep -q 'startup_heap_largest_below_floor.*stage=agent_loop_spawn' "$REGRESSIONS"
grep -q 'heap_largest_below_floor.*stage=http_snapshot_exec_spawn' "$REGRESSIONS"
grep -q 'Write-back starvation lines: 1' "$SUMMARY"
grep -q 'Write-back worker thread starts: 4' "$SUMMARY"
grep -q 'Heap largest trend: first=31744 min=31744 max=31744 last=31744' "$SUMMARY"
