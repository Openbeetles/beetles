#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  scripts/esp_soak_analyze.sh [--output-dir DIR] [--heap-largest-floor BYTES] <log-file>

Outputs:
  - summary.md
  - metrics.csv
  - regressions.csv

This is an evidence parser only. It does not require network access and does not
decide release eligibility by itself.
EOF
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

warn_if_non_target_dir() {
  local dir="$1"
  case "$dir" in
    "$REPO_ROOT"/target|"$REPO_ROOT"/target/*|target|target/*)
      ;;
    *)
      echo "Warning: output dir is outside target/: $dir" >&2
      echo "         Generated analysis may be picked up by git status; do not commit run artifacts." >&2
      ;;
  esac
}

output_dir="$REPO_ROOT/target/esp-soak"
heap_largest_floor="${HEAP_LARGEST_FLOOR:-32768}"
log_file=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-dir)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --output-dir requires a value." >&2; exit 2; }
      output_dir="$1"
      ;;
    --heap-largest-floor)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --heap-largest-floor requires a value." >&2; exit 2; }
      heap_largest_floor="$1"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      echo "Error: unknown argument: $1" >&2
      usage
      exit 2
      ;;
    *)
      [[ -z "$log_file" ]] || { echo "Error: only one log file is supported per run." >&2; exit 2; }
      log_file="$1"
      ;;
  esac
  shift
done

[[ -n "$log_file" ]] || { usage; exit 2; }
[[ -f "$log_file" ]] || { echo "Error: log file not found: $log_file" >&2; exit 1; }
[[ "$heap_largest_floor" =~ ^[0-9]+$ ]] || {
  echo "Error: --heap-largest-floor must be an integer byte count." >&2
  exit 2
}
warn_if_non_target_dir "$output_dir"

run_id="$(date -u +%Y%m%dT%H%M%SZ)-$$"
run_dir="$output_dir/$run_id"
mkdir -p "$run_dir"

metrics_csv="$run_dir/metrics.csv"
regressions_csv="$run_dir/regressions.csv"
summary_md="$run_dir/summary.md"

awk -v floor="$heap_largest_floor" \
    -v metrics="$metrics_csv" \
    -v regressions="$regressions_csv" \
    -v summary="$summary_md" \
    -v log_file="$log_file" '
BEGIN {
  print "line,pressure,heap_largest,worker_starts_total,stack_low_margin,active_wss,camera_frame_active" > metrics;
  print "line,check,severity,detail" > regressions;
  first_largest = -1;
  min_largest = -1;
  max_largest = -1;
  last_largest = -1;
  last_worker_starts = -1;
  critical_open = 0;
  saw_critical = 0;
  panic_count = 0;
  issue_count = 0;
  stack_low_margin_count = 0;
  heap_floor_count = 0;
  voice_wss_count = 0;
  frame_lease_count = 0;
  idle_worker_count = 0;
  write_back_starvation_count = 0;
  write_back_defer_churn_count = 0;
  write_back_stalled_samples = 0;
  write_back_starvation_reported = 0;
  last_write_back_deferred = -1;
  metric_rows = 0;
}

function trim(value) {
  gsub(/^[[:space:]]+|[[:space:]]+$/, "", value);
  return value;
}

function value_after(line, key,    pattern, start, rest) {
  pattern = key "=";
  start = index(line, pattern);
  if (start == 0) {
    return "";
  }
  rest = substr(line, start + length(pattern));
  sub(/[ ,;|)}\]]+.*/, "", rest);
  gsub(/^["'\''"]|["'\''"]$/, "", rest);
  return rest;
}

function numeric_after(line, key,    value) {
  value = value_after(line, key);
  gsub(/[^0-9].*/, "", value);
  return value;
}

function record_issue(line_no, check, severity, detail) {
  gsub(/"/, "\"\"", detail);
  print line_no ",\"" check "\",\"" severity "\",\"" detail "\"" >> regressions;
  issue_count++;
}

{
  line = $0;
  lower_line = tolower(line);
  pressure = value_after(line, "pressure");
  if (pressure == "") {
    pressure = value_after(line, "resource_pressure");
  }

  heap_largest = numeric_after(line, "heap_largest");
  if (heap_largest == "") {
    heap_largest = numeric_after(line, "largest_block");
  }
  if (heap_largest == "") {
    heap_largest = numeric_after(line, "largest_internal_block");
  }

  worker_starts = numeric_after(line, "worker_starts_total");
  low_margin = numeric_after(line, "low_margin");
  if (low_margin == "") {
    low_margin = numeric_after(line, "stack_low_margin");
  }
  active_wss = numeric_after(line, "active_wss");
  if (active_wss == "") {
    active_wss = numeric_after(line, "external_wss_active");
  }
  camera_frame_active = "";
  if (lower_line ~ /cameraframe|frame[_ -]?lease|camera[_ -]?frame/) {
    camera_frame_active = numeric_after(line, "active");
    if (camera_frame_active == "" && (lower_line ~ /active=true|state=active|held=true/)) {
      camera_frame_active = 1;
    }
  }

  if (pressure == "Critical") {
    critical_open = 1;
    saw_critical = 1;
  } else if (pressure == "Normal") {
    critical_open = 0;
  }

  if (heap_largest != "") {
    heap_largest += 0;
    if (first_largest < 0) {
      first_largest = heap_largest;
    }
    last_largest = heap_largest;
    if (min_largest < 0 || heap_largest < min_largest) {
      min_largest = heap_largest;
    }
    if (max_largest < 0 || heap_largest > max_largest) {
      max_largest = heap_largest;
    }
    if (heap_largest < floor) {
      heap_floor_count++;
      record_issue(NR, "heap_largest_below_floor", "blocker", "heap_largest=" heap_largest " floor=" floor);
    }
  }

  if (worker_starts != "") {
    worker_starts += 0;
    if (last_worker_starts >= 0 && lower_line ~ /idle/) {
      if (worker_starts > last_worker_starts) {
        idle_worker_count++;
        record_issue(NR, "worker_starts_growth_during_idle", "risk", "worker_starts_total grew from " last_worker_starts " to " worker_starts " during idle line");
      }
    }
    last_worker_starts = worker_starts;
  }

  if (lower_line ~ /write_back[[:space:]]+queued=/) {
    write_back_queued = numeric_after(line, "queued");
    write_back_worker_started = value_after(line, "worker_started");
    write_back_deferred = numeric_after(line, "deferred_total");
    if (write_back_queued != "" &&
        write_back_queued + 0 > 0 &&
        write_back_worker_started == "false") {
      if (write_back_deferred != "" &&
          last_write_back_deferred >= 0 &&
          write_back_deferred + 0 >= last_write_back_deferred) {
        write_back_stalled_samples++;
      } else {
        write_back_stalled_samples = 1;
      }
      if (write_back_deferred != "" && last_write_back_deferred >= 0) {
        write_back_defer_delta = (write_back_deferred + 0) - last_write_back_deferred;
        if (write_back_defer_delta > 60) {
          write_back_defer_churn_count++;
          record_issue(NR, "write_back_defer_churn", "blocker", "queued=" write_back_queued " worker_started=false deferred_delta=" write_back_defer_delta);
        }
      }
      if (write_back_stalled_samples >= 3 && !write_back_starvation_reported) {
        write_back_starvation_count++;
        write_back_starvation_reported = 1;
        record_issue(NR, "pending_write_back_starvation", "blocker", "queued=" write_back_queued " worker_started=false stalled_samples=" write_back_stalled_samples " deferred_total=" write_back_deferred);
      }
    } else {
      write_back_stalled_samples = 0;
      write_back_starvation_reported = 0;
    }
    if (write_back_deferred != "") {
      last_write_back_deferred = write_back_deferred + 0;
    }
  }

  if (lower_line ~ /idle[-_ ]?stop|idle[-_ ]?stopped|idle timeout|idle_timeout/) {
    if (lower_line ~ /active=(1|true|active)|state=active|still active/) {
      idle_worker_count++;
      record_issue(NR, "lazy_worker_active_after_idle", "blocker", trim(line));
    }
  }

  if (line ~ /VoiceExclusive/) {
    if ((active_wss != "" && active_wss + 0 > 0) ||
        lower_line ~ /external[_ -]?wss[^[:space:]]*[=:](1|true|active|connected)/) {
      voice_wss_count++;
      record_issue(NR, "voice_exclusive_external_wss_active", "blocker", trim(line));
    }
  }

  if (camera_frame_active != "" && camera_frame_active + 0 > 0 &&
      lower_line ~ /leak|stale|expired|conflict|after[_ -]?(return|release|idle)|not[_ -]?released|held[_ -]?after|active[_ -]?after/) {
    frame_lease_count++;
    record_issue(NR, "camera_frame_lease_active", "blocker", trim(line));
  }

  if (low_margin != "" && low_margin + 0 > 0) {
    stack_low_margin_count++;
    record_issue(NR, "stack_low_margin", "blocker", "low_margin=" low_margin);
  }

  if (line ~ /Guru Meditation|Core[[:space:]]+[0-9]+ panic|panic.ed|PANIC|core dump|coredump.*(written|stored|checksum|panic)|stack overflow in task pthread|Failed to create task/) {
    panic_count++;
    record_issue(NR, "panic_or_task_failure", "blocker", trim(line));
  }

  if (pressure != "" || heap_largest != "" || worker_starts != "" || low_margin != "" || active_wss != "" || camera_frame_active != "") {
    print NR "," pressure "," heap_largest "," worker_starts "," low_margin "," active_wss "," camera_frame_active >> metrics;
    metric_rows++;
  }
}

END {
  if (critical_open) {
    record_issue(NR, "unrecovered_critical_pressure", "blocker", "Critical pressure appeared without a later pressure=Normal line");
  }

  print "# ESP soak analysis summary" > summary;
  print "" >> summary;
  print "- Log: `" log_file "`" >> summary;
  print "- Parsed metric rows: " metric_rows >> summary;
  print "- Heap largest floor: " floor " bytes" >> summary;
  print "- Issues detected: " issue_count >> summary;
  print "- Panic/task failure lines: " panic_count >> summary;
  print "- Stack low-margin lines: " stack_low_margin_count >> summary;
  print "- Heap largest below floor lines: " heap_floor_count >> summary;
  print "- VoiceExclusive + external WSS violations: " voice_wss_count >> summary;
  print "- Camera frame lease active lines: " frame_lease_count >> summary;
  print "- Lazy worker idle violations: " idle_worker_count >> summary;
  print "- Write-back starvation lines: " write_back_starvation_count >> summary;
  print "- Write-back defer churn lines: " write_back_defer_churn_count >> summary;
  if (saw_critical) {
    print "- Critical pressure observed: yes" >> summary;
  } else {
    print "- Critical pressure observed: no" >> summary;
  }
  if (min_largest >= 0) {
    print "- Heap largest trend: first=" first_largest " min=" min_largest " max=" max_largest " last=" last_largest >> summary;
  } else {
    print "- Heap largest trend: unavailable" >> summary;
  }
  print "- Resource regression detail: `regressions.csv`" >> summary;
  if (panic_count > 0) {
    print "" >> summary;
    print "Panic symbolization hint:" >> summary;
    print "" >> summary;
    print "```bash" >> summary;
    print "scripts/esp_symbolize_panic.sh target/esp-artifacts/<artifact-id> <addresses...>" >> summary;
    print "```" >> summary;
  }
}
' "$log_file"

echo "ESP soak analysis written:"
echo "  summary: $summary_md"
echo "  metrics: $metrics_csv"
echo "  regressions: $regressions_csv"
