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
The heap largest floor is an observation floor: Normal+Healthy runtime samples
below it are recorded as risks, while non-Healthy, spawn, route, stack, panic,
or write-back regressions remain blockers.
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
  print "line,pressure,tls_fragmentation,storage_contention,storage_ops,storage_wait_last_us,storage_wait_total_us,storage_hold_last_us,storage_hold_total_us,storage_hold_last_stage,storage_last_age_ms,heap_largest,worker_starts_total,stack_low_margin,active_wss,camera_frame_active,scheduler_active_foreground,scheduler_recovery_active,scheduler_last_class,scheduler_last_decision,scheduler_defers,scheduler_degrades,scheduler_rejects" > metrics;
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
  blocker_count = 0;
  stack_low_margin_count = 0;
  storage_contention_risk_count = 0;
  storage_contention_blocker_count = 0;
  heap_floor_count = 0;
  heap_floor_risk_count = 0;
  voice_wss_count = 0;
  frame_lease_count = 0;
  idle_worker_count = 0;
  write_back_starvation_count = 0;
  write_back_defer_churn_count = 0;
  write_back_thread_start_count = 0;
  write_back_start_sample_index = 0;
  write_back_stalled_samples = 0;
  write_back_starvation_reported = 0;
  last_write_back_deferred = -1;
  chat_stream_final_count = 0;
  chat_stream_error_count = 0;
  scheduler_contract_seen = 0;
  scheduler_foreground_open = 0;
  scheduler_recovery_open = 0;
  scheduler_foreground_samples = 0;
  scheduler_recovery_samples = 0;
  scheduler_defer_count = 0;
  scheduler_degrade_count = 0;
  scheduler_resume_count = 0;
  scheduler_resume_missing_count = 0;
  foreground_ack_missing_count = 0;
  primary_delivery_missing_count = 0;
  deep_worker_not_deferred_count = 0;
  voice_auto_not_suppressed_count = 0;
  write_back_started_during_foreground_count = 0;
  post_foreground_recovery_violation_count = 0;
  display_status_missing_count = 0;
  startup_network_violation_count = 0;
  qq_msgseq_regression_count = 0;
  voice_session_stack_overflow_count = 0;
  wakenet_afe_empty_count = 0;
  wakenet_afe_empty_first_line = 0;
  wakenet_slow_feed_count = 0;
  wakenet_feed_profile_diag_count = 0;
  wakenet_feed_diag_contract_missing_count = 0;
  wakenet_afe_empty_blocker_threshold = 20;
  wakenet_feed_slow_threshold_us = 1000000;
  realtime_downlink_dropped_count = 0;
  realtime_direct_speaker_write_count = 0;
  audio_speaker_underrun_count = 0;
  response_overlap_count = 0;
  server_vad_half_duplex_suppression_count = 0;
  server_vad_half_duplex_churn_count = 0;
  server_vad_half_duplex_churn_threshold = 3;
  response_audio_done_without_summary_count = 0;
  realtime_local_speech_window_forced_close_count = 0;
  realtime_transport_exit_count = 0;
  realtime_turn_interrupted_count = 0;
  realtime_session_nonsteady_count = 0;
  realtime_voice_session_failed_count = 0;
  realtime_response_wait_recovered_count = 0;
  pending_audio_done_line = 0;
  pending_audio_done_event = "";
  realtime_server_event_count = 0;
  realtime_response_created_count = 0;
  realtime_response_done_count = 0;
  realtime_audio_done_count = 0;
  realtime_downlink_summary_count = 0;
  realtime_server_speech_started_count = 0;
  realtime_server_speech_stopped_count = 0;
  realtime_fragmented_turn_count = 0;
  realtime_fragmented_turn_churn_count = 0;
  realtime_fragmented_turn_threshold = 2;
  server_vad_last_start_ms = -1;
  realtime_heartbeat_count = 0;
  realtime_heartbeat_stale_samples = 0;
  realtime_heartbeat_stale_first_line = 0;
  realtime_heartbeat_counters_stale_count = 0;
  output_pending = 0;
  output_pending_line = 0;
  output_pending_until_ms = -1;
  local_interrupt_accepted_since_output = 0;
  last_audio_speaker_underrun = -1;
  barge_in_enabled_without_aec_count = 0;
  wakenet_path_seen = 0;
  wakenet_probe_seen = 0;
  wakenet_probe_attempts = 0;
  wakenet_probe_triggers = 0;
  wakenet_probe_false_wakes = 0;
  wakenet_low_sensitivity_probe_missing_count = 0;
  wakenet_low_recall_rate_count = 0;
  wakenet_false_wake_count = 0;
  wakenet_threshold_contract_seen = 0;
  wakenet_threshold_logged = 0;
  wakenet_threshold_contract_missing_count = 0;
  wakenet_threshold_not_logged_count = 0;
  wakenet_threshold_apply_failed_count = 0;
  storage_reset_format_count = 0;
  storage_reset_first_line = 0;
  nvs_read_failed_count = 0;
  config_unset_count = 0;
  enabled_channel_none_count = 0;
  network_ap_only_unconfigured_count = 0;
  runtime_pairing_count = 0;
  voice_validation_invalid_reset_count = 0;
  wifi_sta_ready_seen = 0;
  recent_voice_session_spawn_line = -1;
  display_heavy_degrade_seen = 0;
  display_status_retained_seen = 0;
  foreground_ack_seen = 0;
  foreground_llm_seen = 0;
  foreground_primary_final_seen = 0;
  foreground_primary_delivered_seen = 0;
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
  return trim(rest);
}

function rest_after(line, key,    pattern, start, rest) {
  pattern = key "=";
  start = index(line, pattern);
  if (start == 0) {
    return "";
  }
  rest = substr(line, start + length(pattern));
  return trim(rest);
}

function numeric_after(line, key,    value) {
  value = value_after(line, key);
  gsub(/[^0-9].*/, "", value);
  return value;
}

function log_timestamp_ms(line,    captured) {
  if (match(line, /\([0-9]+\)/) == 0) {
    return "";
  }
  captured = substr(line, RSTART + 1, RLENGTH - 2);
  return captured + 0;
}

function clear_output_pending() {
  output_pending = 0;
  output_pending_line = 0;
  output_pending_until_ms = -1;
  local_interrupt_accepted_since_output = 0;
}

function output_pending_active_at(ms) {
  if (ms != "" && output_pending_until_ms >= 0) {
    if (ms + 0 <= output_pending_until_ms) {
      return 1;
    }
    clear_output_pending();
    return 0;
  }
  return output_pending;
}

function is_boot_startup_checkpoint_stage(stage) {
  return stage ~ /^(memory_provider_registered|config_loaded|wifi_stack_ready|csrf_initialized|display_initialized|display_boot_dashboard|boot_memory_reads|audio_init_phase_done|voice_event_channel_ready|startup_self_check_ok|config_api_spawned|bg_timer_spawn|bg_timer_started|communication_plane_ready|orchestrator_initialized|display_thread_spawned|qq_ws_spawn|os_outbound_spawn|agent_loop_deferred|agent_loop_spawn)$/;
}

function scheduler_class_requires_resume(class) {
  return class != "optional_maintenance" && class != "self_runtime_llm_work";
}

function scheduler_class_must_defer_during_recovery(class) {
  return class == "deep_route_worker" ||
    class == "config_ui_chat_history_route" ||
    class == "durable_write_back" ||
    class == "optional_maintenance" ||
    class == "self_runtime_llm_work" ||
    class == "supplemental_delivery";
}

function record_issue(line_no, check, severity, detail) {
  gsub(/"/, "\"\"", detail);
  print line_no ",\"" check "\",\"" severity "\",\"" detail "\"" >> regressions;
  issue_count++;
  if (severity == "blocker") {
    blocker_count++;
  }
}

function storage_contention_from_metrics(wait_last, hold_last, ops, hold_stage, last_age,    wait_value, hold_value) {
  if (ops == "" || ops + 0 == 0 ||
      hold_stage == "" ||
      last_age == "" || last_age + 0 > 10000) {
    return "Healthy";
  }
  wait_value = wait_last == "" ? 0 : wait_last + 0;
  hold_value = hold_last == "" ? 0 : hold_last + 0;
  if (hold_value >= 1000000 || wait_value >= 50000) {
    return "Critical";
  }
  if (hold_value >= 200000 || wait_value >= 5000) {
    return "Cautious";
  }
  return "Healthy";
}

function record_pending_audio_done_without_summary(reason) {
  if (pending_audio_done_line <= 0) {
    return;
  }
  response_audio_done_without_summary_count++;
  record_issue(pending_audio_done_line, "response_audio_done_without_downlink_summary", "blocker", "event=" pending_audio_done_event " reason=" reason);
  pending_audio_done_line = 0;
  pending_audio_done_event = "";
}

{
  line = $0;
  lower_line = tolower(line);
  if (lower_line ~ /(corrupted dir pair|mount failed.*formatting)/) {
    storage_reset_format_count++;
    if (storage_reset_first_line == 0) {
      storage_reset_first_line = NR;
    }
  }
  if (lower_line ~ /\[config\] nvs read_strings failed/) {
    nvs_read_failed_count++;
  }
  if (lower_line ~ /config loaded/ && lower_line ~ /wifi_ssid set: false/) {
    config_unset_count++;
  }
  if (lower_line ~ /enabled_channel=.*[(]none[)]/) {
    enabled_channel_none_count++;
  }
  if (lower_line ~ /network stage=aponly/ && lower_line ~ /sta_configured=false/) {
    network_ap_only_unconfigured_count++;
  }
  if (lower_line ~ /runtime_mode/ && lower_line ~ /current_mode=pairing/) {
    runtime_pairing_count++;
  }
  event_type = "";
  if (lower_line ~ /realtime server event type=/) {
    event_type = value_after(line, "type");
    realtime_server_event_count++;
    if (pending_audio_done_line > 0 &&
        event_type != "" &&
        event_type != "response.audio.done" &&
        event_type != "response.output_audio.done") {
      record_pending_audio_done_without_summary("next_event=" event_type);
    }
    if (event_type == "response.created") {
      realtime_response_created_count++;
      current_event_ms = log_timestamp_ms(line);
      if (output_pending_active_at(current_event_ms) && !local_interrupt_accepted_since_output) {
        response_overlap_count++;
        record_issue(NR, "response_overlap_without_accepted_interrupt", "blocker", "event=response.created pending_output_line=" output_pending_line);
      }
    } else if (event_type == "response.done") {
      realtime_response_done_count++;
    } else if (event_type == "response.audio.done" || event_type == "response.output_audio.done") {
      realtime_audio_done_count++;
      pending_audio_done_line = NR;
      pending_audio_done_event = event_type;
    } else if (event_type == "input_audio_buffer.speech_started") {
      realtime_server_speech_started_count++;
      current_event_ms = log_timestamp_ms(line);
      server_vad_last_start_ms = current_event_ms == "" ? -1 : current_event_ms + 0;
      if (output_pending_active_at(current_event_ms) && !local_interrupt_accepted_since_output) {
        response_overlap_count++;
        record_issue(NR, "response_overlap_without_accepted_interrupt", "blocker", "event=input_audio_buffer.speech_started pending_output_line=" output_pending_line);
      }
    } else if (event_type == "input_audio_buffer.speech_stopped") {
      realtime_server_speech_stopped_count++;
      current_event_ms = log_timestamp_ms(line);
      if (server_vad_last_start_ms >= 0 && current_event_ms != "" &&
          current_event_ms + 0 >= server_vad_last_start_ms &&
          current_event_ms + 0 - server_vad_last_start_ms <= 250) {
        realtime_fragmented_turn_count++;
      }
    }
  }
  if (lower_line ~ /local interrupt accepted; playback aborted/) {
    local_interrupt_accepted_since_output = 1;
    clear_output_pending();
  }
  if (lower_line ~ /suppressing server vad turn during half-duplex playback/) {
    server_vad_half_duplex_suppression_count++;
    if (server_vad_half_duplex_suppression_count == server_vad_half_duplex_churn_threshold) {
      server_vad_half_duplex_churn_count++;
      record_issue(NR, "server_vad_during_half_duplex_output_churn", "blocker", "suppression_count=" server_vad_half_duplex_suppression_count " threshold=" server_vad_half_duplex_churn_threshold);
    }
  }
  if (lower_line ~ /force closing long local speech window/) {
    realtime_local_speech_window_forced_close_count++;
    record_issue(NR, "realtime_local_speech_window_forced_close", "blocker", trim(line));
  }
  if (lower_line ~ /realtime session transport exit/) {
    realtime_transport_exit_count++;
    if (lower_line ~ /interrupted_active_turn=true/ ||
        lower_line ~ /partial_output_pending_at_exit=true/) {
      realtime_turn_interrupted_count++;
      record_issue(NR, "realtime_transport_interrupted_active_turn", "blocker", trim(line));
    } else {
      record_issue(NR, "realtime_transport_exit", "blocker", trim(line));
    }
  }
  if (lower_line ~ /realtime session ended non-steady/) {
    realtime_session_nonsteady_count++;
    record_issue(NR, "realtime_session_nonsteady_exit", "blocker", trim(line));
  }
  if (lower_line ~ /realtime response wait timeout recovered/) {
    realtime_response_wait_recovered_count++;
    record_issue(NR, "realtime_response_wait_recovered", "risk", trim(line));
  }
  if (lower_line ~ /realtime voice session failed/) {
    realtime_voice_session_failed_count++;
    record_issue(NR, "realtime_voice_session_failed", "blocker", trim(line));
  }
  if (lower_line ~ /realtime audio downlink summary/) {
    realtime_downlink_summary_count++;
    summary_event = value_after(line, "event");
    if ((summary_event == "response.audio.done" || summary_event == "response.output_audio.done") &&
        pending_audio_done_line > 0) {
      pending_audio_done_line = 0;
      pending_audio_done_event = "";
    }
    downlink_dropped = numeric_after(line, "dropped");
    downlink_accepted = numeric_after(line, "accepted");
    downlink_direct_written = numeric_after(line, "direct_written");
    downlink_peak_staging = numeric_after(line, "peak_staging");
    downlink_peak_speaker = numeric_after(line, "peak_speaker");
    downlink_audio_ms = numeric_after(line, "audio_ms");
    downlink_elapsed_ms = numeric_after(line, "elapsed_ms");
    if (downlink_dropped != "" && downlink_dropped + 0 > 0) {
      realtime_downlink_dropped_count++;
      record_issue(NR, "realtime_downlink_dropped_nonzero", "blocker", "dropped=" downlink_dropped " event=" summary_event);
    }
    if (downlink_direct_written != "" && downlink_direct_written + 0 > 0) {
      realtime_direct_speaker_write_count++;
      record_issue(NR, "realtime_direct_speaker_write_nonzero", "blocker", "direct_written=" downlink_direct_written " event=" summary_event);
    }
    if ((downlink_accepted != "" && downlink_accepted + 0 > 0) ||
        (downlink_peak_staging != "" && downlink_peak_staging + 0 > 0) ||
        (downlink_peak_speaker != "" && downlink_peak_speaker + 0 > 0) ||
        (downlink_audio_ms != "" && downlink_audio_ms + 0 > 0)) {
      output_pending = 1;
      output_pending_line = NR;
      local_interrupt_accepted_since_output = 0;
      current_event_ms = log_timestamp_ms(line);
      if (current_event_ms != "" && downlink_audio_ms != "") {
        pending_tail_ms = downlink_audio_ms + 0;
        if (downlink_elapsed_ms != "") {
          pending_tail_ms -= downlink_elapsed_ms + 0;
        }
        if (pending_tail_ms < 0) {
          pending_tail_ms = 0;
        }
        output_pending_until_ms = current_event_ms + pending_tail_ms;
      }
    }
  }
  if (lower_line ~ /audio_speaker/) {
    speaker_underrun = numeric_after(line, "underrun");
    speaker_queue_last = numeric_after(line, "queue_last");
    speaker_queue_min = numeric_after(line, "queue_min");
    if (speaker_underrun != "") {
      if (speaker_underrun + 0 > 0 &&
          (last_audio_speaker_underrun < 0 || speaker_underrun + 0 > last_audio_speaker_underrun)) {
        audio_speaker_underrun_count++;
        record_issue(NR, "audio_speaker_underrun_nonzero", "blocker", "underrun=" speaker_underrun);
      }
      last_audio_speaker_underrun = speaker_underrun + 0;
    }
    if (speaker_queue_last != "" && speaker_queue_min != "" &&
        speaker_queue_last + 0 == 0 && speaker_queue_min + 0 == 0) {
      current_event_ms = log_timestamp_ms(line);
      if (output_pending_until_ms < 0 || current_event_ms == "" || current_event_ms + 0 > output_pending_until_ms) {
        clear_output_pending();
      }
    }
  }
  if (lower_line ~ /voice_realtime handoff_ms=/) {
    realtime_heartbeat_count++;
    voice_rt_local_commit = numeric_after(line, "local_commit_total");
    voice_rt_server_speech = numeric_after(line, "server_speech_total");
    voice_rt_turn_completed = numeric_after(line, "turn_completed_total");
    if (((realtime_server_speech_started_count > 0 && voice_rt_server_speech == "0") ||
         (realtime_response_done_count > 0 && voice_rt_turn_completed == "0")) &&
        (voice_rt_local_commit == "0" || voice_rt_local_commit == "")) {
      realtime_heartbeat_stale_samples++;
      if (realtime_heartbeat_stale_first_line == 0) {
        realtime_heartbeat_stale_first_line = NR;
      }
    }
  }
  if (lower_line ~ /audio contract/ && lower_line ~ /barge_in=true/ &&
      (lower_line ~ /aec=none/ || lower_line ~ /reference=inputreference/)) {
    barge_in_enabled_without_aec_count++;
    record_issue(NR, "barge_in_enabled_without_aec", "blocker", trim(line));
  }
  if (lower_line ~ /esp-sr afe wakenet init|wakenet triggered|beetle_wakenet.*feed(16k)? window|set wakenet model/) {
    wakenet_path_seen = 1;
  }
  if (lower_line ~ /beetle_wakenet.*feed16k window/) {
    if (lower_line ~ /input_profile=/ &&
        lower_line ~ /input_format=/ &&
        lower_line ~ /ch0_role=/ &&
        lower_line ~ /ch1_role=/ &&
        lower_line ~ /raw_frames=/ &&
        lower_line ~ /mic0_feed_to_raw_pm=/) {
      wakenet_feed_profile_diag_count++;
    } else {
      wakenet_feed_diag_contract_missing_count++;
      record_issue(NR, "wakenet_feed_profile_diag_missing", "risk", "feed16k window line lacks input_profile/channel_role/raw_frames/feed_to_raw diagnostics");
    }
  }
  if (lower_line ~ /audio_wake/ &&
      numeric_after(line, "feed_calls") != "" &&
      numeric_after(line, "feed_calls") + 0 > 0) {
    wakenet_path_seen = 1;
  }
  if (lower_line ~ /wakenet.*threshold|set_wakenet_threshold/) {
    wakenet_threshold_contract_seen = 1;
    if (lower_line ~ /threshold=[0-9.]+/) {
      wakenet_threshold_logged = 1;
    }
    if (lower_line ~ /(fail|failed|error|rc=-)/) {
      wakenet_threshold_apply_failed_count++;
      record_issue(NR, "wakenet_threshold_apply_failed", "blocker", trim(line));
    }
  }
  if (lower_line ~ /((wakenet|wake).*(probe|attempt)|manual_wake_attempt|wake_test_attempt)/) {
    wakenet_path_seen = 1;
    wakenet_probe_seen = 1;
    marker_attempts = numeric_after(line, "attempts");
    marker_triggers = numeric_after(line, "triggers");
    marker_false_wakes = numeric_after(line, "false_wakes");
    if (marker_false_wakes == "") {
      marker_false_wakes = numeric_after(line, "false_wake");
    }
    if (marker_attempts != "") {
      if (marker_attempts + 0 > wakenet_probe_attempts) {
        wakenet_probe_attempts = marker_attempts + 0;
      }
    } else if (value_after(line, "attempt") != "") {
      wakenet_probe_attempts++;
    }
    if (marker_triggers != "") {
      if (marker_triggers + 0 > wakenet_probe_triggers) {
        wakenet_probe_triggers = marker_triggers + 0;
      }
    } else if (lower_line ~ /(triggered=true|detected=true|success=true|result=triggered)/) {
      wakenet_probe_triggers++;
    }
    if (marker_false_wakes != "") {
      if (marker_false_wakes + 0 > wakenet_probe_false_wakes) {
        wakenet_probe_false_wakes = marker_false_wakes + 0;
      }
    } else if (lower_line ~ /(false_wake=true|result=false_wake)/) {
      wakenet_probe_false_wakes++;
    }
  }
  if (line ~ /Ringbuffer of AFE is empty, Please use feed\(\) to write data/) {
    wakenet_afe_empty_count++;
    if (wakenet_afe_empty_first_line == 0) {
      wakenet_afe_empty_first_line = NR;
    }
  }
  if (lower_line ~ /audio_wake/) {
    wake_feed_us = numeric_after(line, "feed_us");
    if (wake_feed_us != "" && wake_feed_us + 0 >= wakenet_feed_slow_threshold_us) {
      wakenet_slow_feed_count++;
      record_issue(NR, "wakenet_feed_hot_path_slow", "blocker", "feed_us=" wake_feed_us " threshold_us=" wakenet_feed_slow_threshold_us);
    }
  }
  if (lower_line ~ /\[heartbeat\] metrics .*spiffs_/) {
    record_issue(NR, "legacy_storage_metric_names", "blocker", "legacy backend metric names found");
  }
  pressure = value_after(line, "pressure");
  if (pressure == "") {
    pressure = value_after(line, "resource_pressure");
  }
  tls_fragmentation = value_after(line, "tls_fragmentation");
  storage_contention = value_after(line, "storage_contention");
  storage_contention_state = storage_contention;
  if (storage_contention_state ~ /^[0-9]+$/) {
    storage_contention_state = "";
  }
  storage_ops = numeric_after(line, "storage_ops");
  storage_wait_last_us = numeric_after(line, "storage_wait_last_us");
  storage_wait_total_us = numeric_after(line, "storage_wait_total_us");
  storage_hold_last_us = numeric_after(line, "storage_hold_last_us");
  storage_hold_total_us = numeric_after(line, "storage_hold_total_us");
  storage_hold_last_stage = value_after(line, "storage_hold_last_stage");
  storage_last_age_ms = numeric_after(line, "storage_last_age_ms");
  if (storage_contention_state == "" &&
      (storage_ops != "" || storage_wait_last_us != "" || storage_hold_last_us != "" ||
       storage_hold_last_stage != "" || storage_last_age_ms != "")) {
    storage_contention_state = storage_contention_from_metrics(storage_wait_last_us, storage_hold_last_us, storage_ops, storage_hold_last_stage, storage_last_age_ms);
  }

  heap_largest = numeric_after(line, "heap_largest");
  if (heap_largest == "") {
    heap_largest = numeric_after(line, "heap_largest_internal");
  }
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

  scheduler_active_foreground = value_after(line, "active_foreground");
  scheduler_recovery_active = value_after(line, "foreground_recovery_active");
  scheduler_last_class = value_after(line, "last_class");
  scheduler_last_decision = value_after(line, "last_decision");
  scheduler_defers = numeric_after(line, "defers");
  scheduler_degrades = numeric_after(line, "degrades");
  scheduler_rejects = numeric_after(line, "rejects");
  scheduler_event_class = value_after(line, "class");
  scheduler_event_source = value_after(line, "source");
  scheduler_event_decision = value_after(line, "decision");
  scheduler_event_foreground = value_after(line, "foreground_active");
  scheduler_event_recovery = value_after(line, "foreground_recovery_active");
  scheduler_decision_event_line = lower_line ~ /runtime_scheduler_decision/;
  scheduler_metric_class = scheduler_event_class;
  scheduler_metric_decision = scheduler_event_decision;
  if (scheduler_metric_class == "" || scheduler_metric_class == "none") {
    scheduler_metric_class = scheduler_last_class;
  }
  if (scheduler_metric_decision == "" || scheduler_metric_decision == "none") {
    scheduler_metric_decision = scheduler_last_decision;
  }
  scheduler_line = lower_line ~ /runtime_scheduler/ ||
    scheduler_active_foreground != "" ||
    scheduler_last_decision != "" ||
    scheduler_defers != "" ||
    lower_line ~ /runtime_policy/;
  if (scheduler_line) {
    scheduler_contract_seen = 1;
  }
  if (scheduler_active_foreground == "true") {
    scheduler_foreground_open = 1;
    scheduler_foreground_samples++;
  } else if (scheduler_active_foreground == "false") {
    scheduler_foreground_open = 0;
    foreground_ack_seen = 0;
    foreground_llm_seen = 0;
  } else if (scheduler_event_foreground == "true") {
    scheduler_foreground_open = 1;
  } else if (scheduler_event_foreground == "false") {
    scheduler_foreground_open = 0;
    foreground_ack_seen = 0;
    foreground_llm_seen = 0;
  }
  if (scheduler_recovery_active == "true") {
    scheduler_recovery_open = 1;
    scheduler_recovery_samples++;
  } else if (scheduler_recovery_active == "false") {
    scheduler_recovery_open = 0;
  } else if (scheduler_event_recovery == "true") {
    scheduler_recovery_open = 1;
  } else if (scheduler_event_recovery == "false") {
    scheduler_recovery_open = 0;
  }
  if ((lower_line ~ /foreground_ack/ && value_after(line, "before_llm") == "true") ||
      lower_line ~ /\[chat_stream\].*event=queued/) {
    foreground_ack_seen = 1;
  }
  if (lower_line ~ /llm_turn/ && value_after(line, "event") == "start") {
    foreground_llm_seen = 1;
    if (scheduler_contract_seen && !foreground_ack_seen) {
      foreground_ack_missing_count++;
      record_issue(NR, "foreground_ack_missing_before_llm", "blocker", trim(line));
    }
  }
  if (scheduler_contract_seen && lower_line ~ /\[chat_stream\].*event=final/) {
    foreground_primary_final_seen = 1;
    if (value_after(line, "message_id_present") == "true" ||
        value_after(line, "session_appended") == "true") {
      foreground_primary_delivered_seen = 1;
    }
  }
  if (scheduler_contract_seen && lower_line ~ /primary_delivery/ && value_after(line, "delivered") == "true") {
    foreground_primary_delivered_seen = 1;
  }
  if (scheduler_decision_event_line && scheduler_event_decision == "defer") {
    scheduler_defer_count++;
    if (scheduler_event_class != "" &&
        scheduler_event_class != "none" &&
        scheduler_class_requires_resume(scheduler_event_class)) {
      scheduler_pending_resume[scheduler_event_class] = 1;
    }
  } else if (scheduler_decision_event_line && scheduler_event_decision == "degrade") {
    scheduler_degrade_count++;
    if (scheduler_event_class != "" &&
        scheduler_event_class != "none" &&
        scheduler_class_requires_resume(scheduler_event_class)) {
      scheduler_pending_resume[scheduler_event_class] = 1;
    }
  } else if (scheduler_decision_event_line &&
             scheduler_event_decision == "proceed" &&
             scheduler_event_class != "" &&
             scheduler_event_class != "none") {
    if (scheduler_pending_resume[scheduler_event_class]) {
      scheduler_resume_count++;
      scheduler_pending_resume[scheduler_event_class] = 0;
    }
  }
  if (scheduler_decision_event_line &&
      scheduler_foreground_open &&
      scheduler_event_class != "" &&
      scheduler_event_decision != "") {
    if ((scheduler_event_class == "deep_route_worker" ||
         scheduler_event_class == "config_ui_chat_history_route") &&
        scheduler_event_decision != "defer") {
      deep_worker_not_deferred_count++;
      record_issue(NR, "deep_worker_not_deferred_during_foreground", "blocker", trim(line));
    }
    if ((scheduler_event_class == "realtime_voice_session" ||
         scheduler_event_class == "voice_fallback_interaction") &&
        scheduler_event_source != "user_facing" &&
        scheduler_event_decision != "defer") {
      voice_auto_not_suppressed_count++;
      record_issue(NR, "voice_auto_connect_not_suppressed", "blocker", trim(line));
    }
    if ((scheduler_event_class == "durable_write_back" ||
         scheduler_event_class == "optional_maintenance" ||
         scheduler_event_class == "self_runtime_llm_work") &&
        scheduler_event_decision != "defer") {
      write_back_started_during_foreground_count++;
      record_issue(NR, "write_back_started_during_foreground", "blocker", trim(line));
    }
  }
  if (scheduler_decision_event_line &&
      !scheduler_foreground_open &&
      scheduler_recovery_open &&
      scheduler_event_class != "" &&
      scheduler_event_decision != "") {
    if (scheduler_class_must_defer_during_recovery(scheduler_event_class) &&
        scheduler_event_decision != "defer") {
      post_foreground_recovery_violation_count++;
      record_issue(NR, "post_foreground_recovery_violation", "blocker", trim(line));
    }
    if ((scheduler_event_class == "realtime_voice_session" ||
         scheduler_event_class == "voice_fallback_interaction") &&
        scheduler_event_source != "user_facing" &&
        scheduler_event_decision != "defer") {
      post_foreground_recovery_violation_count++;
      record_issue(NR, "post_foreground_recovery_violation", "blocker", trim(line));
    }
    if (scheduler_event_class == "display_heavy_refresh" &&
        scheduler_event_decision != "degrade") {
      post_foreground_recovery_violation_count++;
      record_issue(NR, "post_foreground_recovery_violation", "blocker", trim(line));
    }
  }
  if (lower_line ~ /display_heavy_refresh/ && scheduler_event_decision == "degrade") {
    display_heavy_degrade_seen = 1;
  }
  if (lower_line ~ /display_status_surface/ && value_after(line, "retained") == "true") {
    display_status_retained_seen = 1;
  }

  runtime_wifi_sta = value_after(line, "wifi_sta");
  if (runtime_wifi_sta == "true" || lower_line ~ /sta.*connected|wifi.*got ip/) {
    wifi_sta_ready_seen = 1;
  }
  runtime_mode_current = value_after(line, "current_mode");
  runtime_booting = value_after(line, "booting");
  runtime_non_voice_outbound = value_after(line, "non_voice_outbound");
  runtime_realtime_voice = value_after(line, "realtime_voice");
  runtime_ext_wss_connect = value_after(line, "ext_wss_connect");
  if (lower_line ~ /\[thread\] started name=voice_realtime_connect/ && !wifi_sta_ready_seen) {
    startup_network_violation_count++;
    record_issue(NR, "voice_realtime_connect_before_network_ready", "blocker", trim(line));
  }
  if (lower_line ~ /\[thread\] started name=(qq_ws|feishu_ws|wecom_aibot(_ws)?|dingtalk_stream)/ &&
      !wifi_sta_ready_seen) {
    startup_network_violation_count++;
    record_issue(NR, "external_wss_worker_before_network_ready", "blocker", trim(line));
  }
  if (lower_line ~ /wifi sta not ready, waiting up to/ &&
      lower_line ~ /\[(qq_ws|feishu_ws|wecom_aibot(_ws)?|dingtalk_stream)\]/) {
    startup_network_violation_count++;
    record_issue(NR, "external_wss_worker_before_network_ready", "blocker", trim(line));
  }
  if (runtime_wifi_sta == "false" && runtime_booting == "false" &&
      (runtime_non_voice_outbound == "true" ||
       runtime_realtime_voice == "true" ||
       runtime_ext_wss_connect == "true")) {
    startup_network_violation_count++;
    record_issue(NR, "boot_normal_before_network_ready", "blocker", trim(line));
  }
  if (runtime_wifi_sta == "false" &&
      (runtime_mode_current == "voice_exclusive" || value_after(line, "voice_exclusive") == "true")) {
    startup_network_violation_count++;
    record_issue(NR, "voice_exclusive_before_network_ready", "blocker", trim(line));
  }

  if (storage_contention_state == "Critical") {
    storage_contention_blocker_count++;
    storage_detail = "storage_contention=Critical storage_contention_count=" storage_contention " storage_ops=" storage_ops " storage_wait_last_us=" storage_wait_last_us " storage_hold_last_us=" storage_hold_last_us " storage_hold_last_stage=" storage_hold_last_stage " storage_last_age_ms=" storage_last_age_ms;
    record_issue(NR, "storage_contention_critical", "blocker", storage_detail);
  } else if (storage_contention_state == "Cautious") {
    storage_contention_risk_count++;
    storage_detail = "storage_contention=Cautious storage_contention_count=" storage_contention " storage_ops=" storage_ops " storage_wait_last_us=" storage_wait_last_us " storage_hold_last_us=" storage_hold_last_us " storage_hold_last_stage=" storage_hold_last_stage " storage_last_age_ms=" storage_last_age_ms;
    record_issue(NR, "storage_contention_cautious", "risk", storage_detail);
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
      startup_stage = value_after(line, "stage");
      heap_detail = "stage=" startup_stage " heap_largest=" heap_largest " floor=" floor " pressure=" pressure " tls_fragmentation=" tls_fragmentation;
      if (lower_line ~ /startup memory checkpoint/ && is_boot_startup_checkpoint_stage(startup_stage)) {
        record_issue(NR, "startup_heap_largest_below_floor", "blocker", heap_detail);
      } else if (pressure == "Normal" && tls_fragmentation == "Healthy") {
        record_issue(NR, "heap_largest_below_observation_floor", "risk", heap_detail);
        heap_floor_risk_count++;
      } else {
        record_issue(NR, "heap_largest_below_floor", "blocker", heap_detail);
      }
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
    write_back_protected_defer_window = scheduler_foreground_open || scheduler_recovery_open;
    if (write_back_queued != "" &&
        write_back_queued + 0 > 0 &&
        write_back_worker_started == "false") {
      if (write_back_protected_defer_window) {
        write_back_stalled_samples = 0;
        write_back_starvation_reported = 0;
      } else {
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
      }
    } else {
      write_back_stalled_samples = 0;
      write_back_starvation_reported = 0;
    }
    if (write_back_deferred != "") {
      last_write_back_deferred = write_back_deferred + 0;
    }
    if (scheduler_foreground_open && write_back_worker_started == "true") {
      write_back_started_during_foreground_count++;
      record_issue(NR, "write_back_started_during_foreground", "blocker", trim(line));
    }
  }

  if (lower_line ~ /\[thread\] started name=write_back/) {
    write_back_thread_start_count++;
    write_back_start_ms = log_timestamp_ms(line);
    if (write_back_start_ms == "") {
      if (write_back_thread_start_count > 3) {
        record_issue(NR, "write_back_worker_churn", "blocker", "write_back worker thread starts=" write_back_thread_start_count " without timestamps");
      }
    } else {
      write_back_start_sample_index++;
      write_back_start_samples[write_back_start_sample_index] = write_back_start_ms;
      write_back_start_window_count = 0;
      write_back_start_window_first_ms = write_back_start_ms;
      for (write_back_start_sample in write_back_start_samples) {
        if (write_back_start_ms - write_back_start_samples[write_back_start_sample] <= 10000) {
          write_back_start_window_count++;
          if (write_back_start_samples[write_back_start_sample] < write_back_start_window_first_ms) {
            write_back_start_window_first_ms = write_back_start_samples[write_back_start_sample];
          }
        } else {
          delete write_back_start_samples[write_back_start_sample];
        }
      }
      if (write_back_start_window_count > 3) {
        record_issue(NR, "write_back_worker_churn", "blocker", "write_back worker dense starts=" write_back_start_window_count " total=" write_back_thread_start_count " window_ms=" (write_back_start_ms - write_back_start_window_first_ms));
      }
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

  if (low_margin != "" && low_margin + 0 > 0 && lower_line ~ /thread_stack/) {
    stack_detail = "low_margin=" low_margin;
    low_stack_detail = rest_after(line, "low");
    if (low_stack_detail != "") {
      sub(/[[:space:]]+top=.*/, "", low_stack_detail);
      stack_detail = stack_detail " low=" low_stack_detail;
    }
    stack_low_margin_count++;
    record_issue(NR, "stack_low_margin", "blocker", stack_detail);
  }

  if (line ~ /Guru Meditation|Core[[:space:]]+[0-9]+ panic|panic.ed|PANIC|core dump|coredump.*(written|stored|checksum|panic)|stack overflow in task pthread|Failed to create task/) {
    panic_count++;
    record_issue(NR, "panic_or_task_failure", "blocker", trim(line));
  }
  if (lower_line ~ /voice_session_spawn/) {
    recent_voice_session_spawn_line = NR;
  }
  if (line ~ /40054005/ || lower_line ~ /消息被去重.*msgseq/) {
    qq_msgseq_regression_count++;
    record_issue(NR, "qq_msgseq_regression", "blocker", trim(line));
  }
  if (line ~ /stack overflow in task pthread/ &&
      recent_voice_session_spawn_line > 0 &&
      NR - recent_voice_session_spawn_line <= 8) {
    voice_session_stack_overflow_count++;
    record_issue(NR, "voice_session_stack_overflow", "blocker", trim(line));
  }

  if (lower_line ~ /spawn failed name=http_.*_exec|http_route_worker_start:.*dispatch worker start failed/) {
    record_issue(NR, "route_worker_spawn_failed", "blocker", trim(line));
  }

  if (line ~ /ESP_ERR_HTTPD_RESP_SEND|httpd_sock_err: error in send/) {
    record_issue(NR, "http_response_send_error", "blocker", trim(line));
  }

  if (lower_line ~ /display refresh suppressed/) {
    record_issue(NR, "display_refresh_suppressed_under_pressure", "blocker", trim(line));
  }

  if (lower_line ~ /\[chat_stream\].*event=final/) {
    chat_stream_final_count++;
  }

  if (lower_line ~ /\[chat_stream\].*event=error/) {
    chat_stream_error_count++;
    record_issue(NR, "chat_stream_error", "blocker", trim(line));
  }

  if (pressure != "" || tls_fragmentation != "" || storage_contention != "" ||
      storage_ops != "" || storage_wait_last_us != "" || storage_wait_total_us != "" ||
      storage_hold_last_us != "" || storage_hold_total_us != "" || storage_hold_last_stage != "" ||
      storage_last_age_ms != "" || heap_largest != "" || worker_starts != "" ||
      low_margin != "" || active_wss != "" || camera_frame_active != "" ||
      scheduler_line) {
    print NR "," pressure "," tls_fragmentation "," storage_contention "," storage_ops "," storage_wait_last_us "," storage_wait_total_us "," storage_hold_last_us "," storage_hold_total_us "," storage_hold_last_stage "," storage_last_age_ms "," heap_largest "," worker_starts "," low_margin "," active_wss "," camera_frame_active "," scheduler_active_foreground "," scheduler_recovery_active "," scheduler_metric_class "," scheduler_metric_decision "," scheduler_defers "," scheduler_degrades "," scheduler_rejects >> metrics;
    metric_rows++;
  }
}

END {
  record_pending_audio_done_without_summary("end_of_log");
  if (realtime_heartbeat_stale_samples >= 2) {
    realtime_heartbeat_counters_stale_count++;
    record_issue(realtime_heartbeat_stale_first_line, "realtime_heartbeat_counters_stale", "blocker", "stale_samples=" realtime_heartbeat_stale_samples " server_speech_events=" realtime_server_speech_started_count " response_created=" realtime_response_created_count " response_done=" realtime_response_done_count);
  }
  if (realtime_fragmented_turn_count >= realtime_fragmented_turn_threshold) {
    realtime_fragmented_turn_churn_count++;
    record_issue(NR, "server_vad_fragmented_turn_churn", "blocker", "speech_started=" realtime_server_speech_started_count " speech_stopped=" realtime_server_speech_stopped_count " response_created=" realtime_response_created_count " tiny_turns=" realtime_fragmented_turn_count);
  }
  if (wakenet_path_seen) {
    if (!wakenet_probe_seen || wakenet_probe_attempts < 20) {
      wakenet_low_sensitivity_probe_missing_count++;
      record_issue(NR, "wakenet_low_sensitivity_probe_missing", "risk", "attempts=" wakenet_probe_attempts " required=20");
    } else if (wakenet_probe_attempts >= 20 &&
               wakenet_probe_triggers * 100 < wakenet_probe_attempts * 90) {
      wakenet_low_recall_rate_count++;
      record_issue(NR, "wakenet_low_recall_rate", "blocker", "attempts=" wakenet_probe_attempts " triggers=" wakenet_probe_triggers " required_rate=0.90");
    }
    if (wakenet_probe_false_wakes > 0) {
      wakenet_false_wake_count++;
      record_issue(NR, "wakenet_false_wake", "blocker", "false_wakes=" wakenet_probe_false_wakes);
    }
    if (!wakenet_threshold_contract_seen) {
      wakenet_threshold_contract_missing_count++;
      record_issue(NR, "wakenet_threshold_contract_missing", "blocker", "WakeNet path observed without threshold contract evidence");
    } else if (!wakenet_threshold_logged) {
      wakenet_threshold_not_logged_count++;
      record_issue(NR, "wakenet_threshold_not_logged", "blocker", "WakeNet threshold contract observed without threshold=<value>");
    }
  }
  if (scheduler_contract_seen && foreground_primary_final_seen && !foreground_primary_delivered_seen) {
    primary_delivery_missing_count++;
    record_issue(NR, "primary_generated_but_not_delivered", "blocker", "scheduler foreground log has chat_stream final without primary_delivery delivered=true");
  }
  if (display_heavy_degrade_seen && !display_status_retained_seen) {
    display_status_missing_count++;
    record_issue(NR, "display_status_missing_during_degrade", "blocker", "display heavy refresh degraded without display_status_surface retained=true evidence");
  }
  for (scheduler_resume_class in scheduler_pending_resume) {
    if (scheduler_pending_resume[scheduler_resume_class] && !scheduler_foreground_open) {
      scheduler_resume_missing_count++;
      record_issue(NR, "scheduler_resume_missing", "blocker", "class=" scheduler_resume_class " had defer/degrade without later decision=proceed");
    }
  }
  if (critical_open) {
    record_issue(NR, "unrecovered_critical_pressure", "blocker", "Critical pressure appeared without a later pressure=Normal line");
  }
  if (wakenet_afe_empty_count >= wakenet_afe_empty_blocker_threshold) {
    record_issue(wakenet_afe_empty_first_line, "wakenet_afe_empty_spam", "blocker", "lines=" wakenet_afe_empty_count " threshold=" wakenet_afe_empty_blocker_threshold);
  }
  if ((storage_reset_format_count > 0 || nvs_read_failed_count > 0) &&
      config_unset_count > 0 &&
      enabled_channel_none_count > 0 &&
      network_ap_only_unconfigured_count > 0 &&
      runtime_pairing_count > 0) {
    voice_validation_invalid_reset_count++;
    invalid_line = storage_reset_first_line > 0 ? storage_reset_first_line : NR;
    record_issue(invalid_line, "voice_validation_invalid_after_flash_reset", "blocker", "storage_reset_format=" storage_reset_format_count " nvs_read_failed=" nvs_read_failed_count " config_unset=" config_unset_count " enabled_channel_none=" enabled_channel_none_count " ap_only_unconfigured=" network_ap_only_unconfigured_count " pairing=" runtime_pairing_count);
  }

  print "# ESP soak analysis summary" > summary;
  print "" >> summary;
  print "- Log: `" log_file "`" >> summary;
  print "- Parsed metric rows: " metric_rows >> summary;
  print "- Heap largest floor: " floor " bytes" >> summary;
  print "- Issues detected: " issue_count >> summary;
  print "- Blocker issues detected: " blocker_count >> summary;
  print "- Panic/task failure lines: " panic_count >> summary;
  print "- Stack low-margin lines: " stack_low_margin_count >> summary;
  print "- Storage contention risk lines: " storage_contention_risk_count >> summary;
  print "- Storage contention blocker lines: " storage_contention_blocker_count >> summary;
  print "- Heap largest below floor lines: " heap_floor_count >> summary;
  print "- Heap largest below floor risk lines: " heap_floor_risk_count >> summary;
  print "- VoiceExclusive + external WSS violations: " voice_wss_count >> summary;
  print "- Camera frame lease active lines: " frame_lease_count >> summary;
  print "- Lazy worker idle violations: " idle_worker_count >> summary;
  print "- Write-back starvation lines: " write_back_starvation_count >> summary;
  print "- Write-back defer churn lines: " write_back_defer_churn_count >> summary;
  print "- Write-back worker thread starts: " write_back_thread_start_count >> summary;
  print "- Chat stream final events: " chat_stream_final_count >> summary;
  print "- Chat stream error events: " chat_stream_error_count >> summary;
  print "- Scheduler foreground samples: " scheduler_foreground_samples >> summary;
  print "- Scheduler post-foreground recovery samples: " scheduler_recovery_samples >> summary;
  print "- Scheduler defer decisions: " scheduler_defer_count >> summary;
  print "- Scheduler degrade decisions: " scheduler_degrade_count >> summary;
  print "- Scheduler resume decisions: " scheduler_resume_count >> summary;
  print "- Scheduler resume missing lines: " scheduler_resume_missing_count >> summary;
  print "- Foreground ack-before-LLM missing lines: " foreground_ack_missing_count >> summary;
  print "- Primary delivery missing lines: " primary_delivery_missing_count >> summary;
  print "- Deep worker foreground violations: " deep_worker_not_deferred_count >> summary;
  print "- Auto voice foreground violations: " voice_auto_not_suppressed_count >> summary;
  print "- Write-back foreground violations: " write_back_started_during_foreground_count >> summary;
  print "- Post-foreground recovery violations: " post_foreground_recovery_violation_count >> summary;
  print "- Display status missing lines: " display_status_missing_count >> summary;
  print "- Startup network readiness violations: " startup_network_violation_count >> summary;
  print "- QQ msgseq regression lines: " qq_msgseq_regression_count >> summary;
  print "- Voice session stack overflow lines: " voice_session_stack_overflow_count >> summary;
  print "- WakeNet AFE empty lines: " wakenet_afe_empty_count >> summary;
  print "- WakeNet slow feed lines: " wakenet_slow_feed_count >> summary;
  print "- WakeNet feed profile diagnostic lines: " wakenet_feed_profile_diag_count >> summary;
  print "- WakeNet feed diagnostic contract missing lines: " wakenet_feed_diag_contract_missing_count >> summary;
  print "- Realtime downlink dropped lines: " realtime_downlink_dropped_count >> summary;
  print "- Realtime direct speaker write lines: " realtime_direct_speaker_write_count >> summary;
  print "- Audio speaker underrun lines: " audio_speaker_underrun_count >> summary;
  print "- Realtime response overlap lines: " response_overlap_count >> summary;
  print "- Half-duplex server-VAD suppression lines: " server_vad_half_duplex_suppression_count >> summary;
  print "- Half-duplex server-VAD churn lines: " server_vad_half_duplex_churn_count >> summary;
  print "- Response audio.done without downlink summary lines: " response_audio_done_without_summary_count >> summary;
  print "- Realtime local speech window forced-close lines: " realtime_local_speech_window_forced_close_count >> summary;
  print "- Realtime transport exit lines: " realtime_transport_exit_count >> summary;
  print "- Realtime interrupted turn lines: " realtime_turn_interrupted_count >> summary;
  print "- Realtime non-steady exit lines: " realtime_session_nonsteady_count >> summary;
  print "- Realtime voice session failed lines: " realtime_voice_session_failed_count >> summary;
  print "- Realtime response wait recovered lines: " realtime_response_wait_recovered_count >> summary;
  print "- Realtime heartbeat stale counter lines: " realtime_heartbeat_counters_stale_count >> summary;
  print "- Server-VAD fragmented turn churn lines: " realtime_fragmented_turn_churn_count >> summary;
  print "- Barge-in without AEC lines: " barge_in_enabled_without_aec_count >> summary;
  print "- WakeNet low-sensitivity probe missing lines: " wakenet_low_sensitivity_probe_missing_count >> summary;
  print "- WakeNet low-recall lines: " wakenet_low_recall_rate_count >> summary;
  print "- WakeNet false wake lines: " wakenet_false_wake_count >> summary;
  print "- WakeNet threshold contract missing lines: " wakenet_threshold_contract_missing_count >> summary;
  print "- WakeNet threshold not logged lines: " wakenet_threshold_not_logged_count >> summary;
  print "- WakeNet threshold apply failed lines: " wakenet_threshold_apply_failed_count >> summary;
  print "- Voice validation invalid after flash reset lines: " voice_validation_invalid_reset_count >> summary;
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
