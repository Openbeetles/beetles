#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "$ROOT/target/test-artifacts"
TMP_DIR="$(mktemp -d "$ROOT/target/test-artifacts/esp-soak-analyze.XXXXXX")"
LOG="$TMP_DIR/serial.log"
OUT="$TMP_DIR/out"

cat >"$LOG" <<'LOGEOF'
I (1000) beetle::heartbeat: [heartbeat] HEARTBEAT version=0.1.0 uptime_secs=10 resource pressure=Normal tls_fragmentation=Healthy storage_contention=Healthy heap_internal_free=60000 heap_largest_internal=31744 active_http=0 active_wss=1 agent_tasks=0 inbound=0 outbound=0
I (1010) beetle::heartbeat: [heartbeat] write_back queued=4 worker_started=false deferred_total=100 dropped_total=0 coalesced_total=0 worker_starts_total=2
I (31000) beetle::heartbeat: [heartbeat] HEARTBEAT version=0.1.0 uptime_secs=40 resource pressure=Normal tls_fragmentation=Healthy storage_contention=Healthy heap_internal_free=60000 heap_largest_internal=31744 active_http=0 active_wss=1 agent_tasks=0 inbound=0 outbound=0
I (31010) beetle::heartbeat: [heartbeat] write_back queued=4 worker_started=false deferred_total=430 dropped_total=0 coalesced_total=0 worker_starts_total=2
I (61000) beetle::heartbeat: [heartbeat] HEARTBEAT version=0.1.0 uptime_secs=70 resource pressure=Normal tls_fragmentation=Healthy storage_contention=Healthy heap_internal_free=60000 heap_largest_internal=31744 active_http=0 active_wss=1 agent_tasks=0 inbound=0 outbound=0
I (61010) beetle::heartbeat: [heartbeat] write_back queued=4 worker_started=false deferred_total=760 dropped_total=0 coalesced_total=0 worker_starts_total=2
LOGEOF

"$ROOT/scripts/esp_soak_analyze.sh" --output-dir "$OUT" "$LOG" >/dev/null

SUMMARY="$(find "$OUT" -name summary.md -print -quit)"
REGRESSIONS="$(find "$OUT" -name regressions.csv -print -quit)"

grep -q 'pending_write_back_starvation' "$REGRESSIONS"
grep -q 'Write-back starvation lines: 1' "$SUMMARY"
