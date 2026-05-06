#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  scripts/linux_live_test_flow.sh --scenario NAME [options]

Scenarios:
  boot_idle       Collect systemd, journal, health, resource, and channel boot evidence.
  qq_text         Collect evidence while real QQ messages are sent to the bot.
  full_machine    QQ semantic flow plus health/resource/metrics/tools/config/memory probes.

Options:
  --host HOST                  Linux device host. Default: BEETLE_LINUX_HOST or 192.168.1.139.
  --user USER                  SSH user. Default: BEETLE_LINUX_USER or beetle.
  --ssh-pass PASS              SSH password. Prefer BEETLE_LINUX_SSH_PASS.
  --pairing-code CODE          Pairing code for protected HTTP routes. Prefer BEETLE_PAIRING_CODE.
  --duration SECONDS           Manual QQ exercise window. Default: 300 for QQ scenarios, 0 for boot_idle.
  --expected-messages COUNT    Required QQ inbound/reply count for QQ scenarios. Default: 6.
  --require-display            Require /api/health display.available=true.
  --qq-acceptance-file FILE    Semantic acceptance evidence file. Created if missing.
  --output-dir DIR             Evidence root. Default: target/linux-live.
  -h, --help                   Show this help.

Flow:
  1. verify SSH, systemd service, local HTTP, and journal access
  2. collect before snapshots into target/linux-live/<run>/
  3. for QQ scenarios, wait while the operator tests bot 1903462822 in the real QQ client
  4. collect after snapshots and journal since run start
  5. gate every pass/fail decision on saved logs, HTTP payloads, and the semantic acceptance file
EOF
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

scenario=""
host="${BEETLE_LINUX_HOST:-192.168.1.139}"
ssh_user="${BEETLE_LINUX_USER:-beetle}"
ssh_pass="${BEETLE_LINUX_SSH_PASS:-}"
pairing_code="${BEETLE_PAIRING_CODE:-}"
duration=""
expected_messages="6"
require_display=0
qq_acceptance_file=""
output_dir="$REPO_ROOT/target/linux-live"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --scenario)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --scenario requires a value." >&2; exit 2; }
      scenario="$1"
      ;;
    --host)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --host requires a value." >&2; exit 2; }
      host="$1"
      ;;
    --user)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --user requires a value." >&2; exit 2; }
      ssh_user="$1"
      ;;
    --ssh-pass)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --ssh-pass requires a value." >&2; exit 2; }
      ssh_pass="$1"
      ;;
    --pairing-code)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --pairing-code requires a value." >&2; exit 2; }
      pairing_code="$1"
      ;;
    --duration)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --duration requires a value." >&2; exit 2; }
      duration="$1"
      ;;
    --expected-messages)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --expected-messages requires a value." >&2; exit 2; }
      expected_messages="$1"
      ;;
    --require-display)
      require_display=1
      ;;
    --qq-acceptance-file)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --qq-acceptance-file requires a value." >&2; exit 2; }
      qq_acceptance_file="$1"
      ;;
    --output-dir)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --output-dir requires a value." >&2; exit 2; }
      output_dir="$1"
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
      echo "Error: unexpected positional argument: $1" >&2
      usage
      exit 2
      ;;
  esac
  shift
done

case "$scenario" in
  boot_idle|qq_text|full_machine) ;;
  "")
    echo "Error: --scenario is required." >&2
    usage
    exit 2
    ;;
  *)
    echo "Error: unsupported scenario: $scenario" >&2
    usage
    exit 2
    ;;
esac

if [[ -z "$duration" ]]; then
  case "$scenario" in
    boot_idle) duration="0" ;;
    qq_text|full_machine) duration="300" ;;
  esac
fi
[[ "$duration" =~ ^[0-9]+$ ]] || { echo "Error: --duration must be an integer." >&2; exit 2; }
[[ "$expected_messages" =~ ^[0-9]+$ ]] || {
  echo "Error: --expected-messages must be an integer." >&2
  exit 2
}
if [[ -n "$pairing_code" && ! "$pairing_code" =~ ^[0-9]{6}$ ]]; then
  echo "Error: --pairing-code must be exactly 6 digits when provided." >&2
  exit 2
fi
if [[ "$scenario" == "full_machine" && -z "$pairing_code" ]]; then
  echo "Error: full_machine requires --pairing-code or BEETLE_PAIRING_CODE." >&2
  exit 2
fi

remote="${ssh_user}@${host}"

ssh_run() {
  local cmd="$1"
  if [[ -n "$ssh_pass" ]]; then
    SSHPASS="$ssh_pass" sshpass -e ssh \
      -o StrictHostKeyChecking=no \
      -o PreferredAuthentications=password \
      -o PubkeyAuthentication=no \
      "$remote" "$cmd"
  else
    ssh -o StrictHostKeyChecking=no "$remote" "$cmd"
  fi
}

count_matches() {
  local pattern="$1"
  local file="$2"
  grep -E -c "$pattern" "$file" 2>/dev/null || true
}

fail_if_matches() {
  local pattern="$1"
  local file="$2"
  local message="$3"
  if grep -E "$pattern" "$file" >/dev/null 2>&1; then
    echo "Gate failed: $message" >&2
    grep -E "$pattern" "$file" >&2 || true
    exit 1
  fi
}

require_matches() {
  local pattern="$1"
  local file="$2"
  local message="$3"
  if ! grep -E "$pattern" "$file" >/dev/null 2>&1; then
    echo "Gate failed: $message" >&2
    echo "  file: $file" >&2
    exit 1
  fi
}

http_get_remote() {
  local path="$1"
  local output="$2"
  local header=""
  if [[ -n "$pairing_code" ]]; then
    header="-H 'X-Pairing-Code: $pairing_code'"
  fi
  if ! ssh_run "curl -sS --max-time 15 $header 'http://127.0.0.1${path}'" > "$output"; then
    echo "HTTP probe failed: GET $path" > "$output"
    return 1
  fi
}

collect_http_probe() {
  local label="$1"
  local path="$2"
  local output="$3"
  if http_get_remote "$path" "$output"; then
    printf 'ok %s %s\n' "$label" "$path" >> "$run_dir/http_probe_status.txt"
  else
    printf 'fail %s %s\n' "$label" "$path" >> "$run_dir/http_probe_status.txt"
  fi
}

write_qq_acceptance_template() {
  local file="$1"
  [[ -n "$file" ]] || return 0
  mkdir -p "$(dirname "$file")"
  if [[ -e "$file" ]]; then
    return 0
  fi
  {
    echo '# Set every field to pass only after checking saved logs and the real QQ client.'
    echo 'LINUX_SEMANTIC_ACCEPTANCE=pending'
    echo 'qq_client_bot_1903462822=pending'
    echo 'qq_basic_llm_answer=pending'
    echo 'qq_status_tool_read=pending'
    echo 'qq_memory_continuity=pending'
    echo 'qq_reminder_write_back=pending'
    echo 'qq_plain_text_rendering=pending'
    echo 'qq_capability_boundary=pending'
    echo 'ui_health_resource_metrics=pending'
    echo 'display_health=pending'
    echo 'notes='
  } > "$file"
}

require_qq_acceptance() {
  local file="$1"
  local key
  [[ -f "$file" ]] || {
    echo "Gate failed: QQ semantic acceptance file not found: $file" >&2
    exit 1
  }
  for key in \
    LINUX_SEMANTIC_ACCEPTANCE \
    qq_client_bot_1903462822 \
    qq_basic_llm_answer \
    qq_status_tool_read \
    qq_memory_continuity \
    qq_reminder_write_back \
    qq_plain_text_rendering \
    qq_capability_boundary \
    ui_health_resource_metrics \
    display_health
  do
    if ! grep -Eq "^${key}=pass([[:space:]]*(#.*)?)?$" "$file"; then
      echo "Gate failed: QQ semantic acceptance missing ${key}=pass in $file" >&2
      exit 1
    fi
  done
}

print_qq_test_plan() {
  cat >&2 <<'EOF'
QQ semantic test plan for bot 1903462822:
  A. Ask for device identity, version, Linux/Raspberry Pi status, and current time.
  B. Ask "查看系统状态" and verify the answer reflects live resource/network/channel state.
  C. Ask it to remember a concrete fact, then ask a follow-up that requires recall.
  D. Set a 45-second reminder and verify the bot sends the due reminder without prompting.
  E. Ask for unavailable hardware/audio/camera/display boundaries and verify it does not fabricate success.
  F. Ask for a compact status report with heading/list/table/code-block semantics; verify QQ shows readable plain text without pipe tables, code fences, or Markdown artifacts.
  G. Send at least the expected message count; do not use numeric echo-only messages as acceptance.
EOF
}

run_gates() {
  local journal_file="$run_dir/journal.log"
  local health_file="$run_dir/http/health.json"
  local resource_file="$run_dir/http/resource.json"
  local metrics_file="$run_dir/http/metrics.json"

  if [[ "$scenario" == "full_machine" ]]; then
    if [[ -z "$pairing_code" ]]; then
      echo "Gate failed: full_machine requires --pairing-code for protected config/tool/memory probes" >&2
      exit 1
    fi
    if grep -E '^fail ' "$run_dir/http_probe_status.txt" >/dev/null 2>&1; then
      echo "Gate failed: full_machine HTTP probes failed" >&2
      grep -E '^fail ' "$run_dir/http_probe_status.txt" >&2 || true
      exit 1
    fi
  fi

  require_matches '^active$' "$run_dir/systemd.is-active.txt" "beetle service is not active"
  require_matches '"status":"ok"' "$health_file" "health status is not ok"
  require_matches '"network_status"' "$health_file" "health payload lacks network status"
  require_matches 'pressure|process_memory_kb|mem_available|resource' "$resource_file" \
    "resource payload lacks resource evidence"
  require_matches 'llm_calls|tool_calls|msg_in|msg_out|dispatch_ok' "$metrics_file" \
    "metrics payload lacks message/LLM/tool counters"
  require_matches '\[heartbeat\] HEARTBEAT version=' "$journal_file" "heartbeat logs not captured"
  require_matches '\[heartbeat\] metrics .*storage_ops=' "$journal_file" "metrics heartbeat not captured"

  fail_if_matches 'panic|abort|segmentation fault|stack overflow|Failed to create task|thread .* panicked' \
    "$journal_file" "fatal runtime pattern found"
  fail_if_matches 'dispatch_fail=[1-9][0-9]*|err_dispatch=[1-9][0-9]*|outbound_enq_fail=[1-9][0-9]*|inbound_drop=[1-9][0-9]*' \
    "$journal_file" "message failure metric increased"
  fail_if_matches 'llm_err=[1-9][0-9]*|err_llm_req=[1-9][0-9]*|err_llm_parse=[1-9][0-9]*' \
    "$journal_file" "LLM error metric increased"
  fail_if_matches 'tool_err=[1-9][0-9]*|tool_protocol_violation=[1-9][0-9]*' \
    "$journal_file" "tool execution/protocol metric increased"
  fail_if_matches 'storage_contention=Critical|http_route_reject=[1-9][0-9]*|spawn_fail=[1-9][0-9]*' \
    "$journal_file" "resource or route blocker found"

  if [[ "$require_display" -eq 1 ]]; then
    require_matches '"display":\{"available":true\}' "$health_file" \
      "display.available was not true"
  fi

  if [[ "$scenario" == "qq_text" || "$scenario" == "full_machine" ]]; then
    require_matches '\[qq_ws\] hello ok' "$journal_file" "QQ WSS hello was not captured"
    require_matches 'QQ Channel sender thread started|\[qq_sender\] sender loop started' \
      "$journal_file" "QQ sender startup was not captured"

    local inbound_count reply_count
    inbound_count="$(count_matches '\[qq_ws\] message enqueued' "$journal_file")"
    reply_count="$(count_matches '\[agent\] reply outbound enqueued' "$journal_file")"
    if (( inbound_count < expected_messages )); then
      echo "Gate failed: expected $expected_messages QQ inbound messages, got $inbound_count" >&2
      exit 1
    fi
    if (( reply_count < expected_messages )); then
      echo "Gate failed: expected $expected_messages QQ replies, got $reply_count" >&2
      exit 1
    fi
    require_qq_acceptance "$qq_acceptance_file"
  fi
}

run_id="$(date -u +%Y%m%dT%H%M%SZ)-$$-$scenario"
run_dir="$output_dir/$run_id"
mkdir -p "$run_dir/http"
: > "$run_dir/http_probe_status.txt"

if [[ "$scenario" == "qq_text" || "$scenario" == "full_machine" ]]; then
  if [[ -z "$qq_acceptance_file" ]]; then
    qq_acceptance_file="$run_dir/qq_acceptance.env"
  fi
  write_qq_acceptance_template "$qq_acceptance_file"
fi

{
  echo "run_id=$run_id"
  echo "scenario=$scenario"
  echo "host=$host"
  echo "user=$ssh_user"
  echo "duration_seconds=$duration"
  echo "expected_messages=$expected_messages"
  echo "pairing_code=$([[ -n "$pairing_code" ]] && echo present || echo absent)"
  echo "require_display=$require_display"
  echo "created_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  if [[ -n "$qq_acceptance_file" ]]; then
    echo "qq_acceptance_file=$qq_acceptance_file"
  fi
} > "$run_dir/metadata.env"

cat > "$run_dir/commands.md" <<EOF
# Reproduce Linux Raspberry Pi live test flow

\`\`\`bash
BEETLE_LINUX_HOST=$host BEETLE_LINUX_USER=$ssh_user BEETLE_LINUX_SSH_PASS=<redacted> BEETLE_PAIRING_CODE=<redacted> \\
  scripts/linux_live_test_flow.sh --scenario $scenario --duration $duration --expected-messages $expected_messages
\`\`\`

Evidence is under:

\`\`\`
$run_dir
\`\`\`
EOF

echo "Linux live flow:"
echo "  scenario: $scenario"
echo "  remote: $remote"
echo "  run dir: $run_dir"

echo
echo "Step 1/5: verifying SSH, service, and remote clock."
ssh_run 'date -u +%Y-%m-%dT%H:%M:%SZ' > "$run_dir/remote_start_utc.txt"
remote_since="$(ssh_run 'date "+%Y-%m-%d %H:%M:%S"')"
ssh_run 'systemctl is-active beetle' > "$run_dir/systemd.is-active.txt"
ssh_run 'systemctl show beetle -p ActiveState -p SubState -p MainPID -p NRestarts -p ActiveEnterTimestamp --no-pager' \
  > "$run_dir/systemd.show.txt"
ssh_run 'uname -a; printf "\n"; /opt/beetle/current/beetle --version 2>/dev/null || beetle --version 2>/dev/null || true' \
  > "$run_dir/device_identity.txt"

echo
echo "Step 2/5: collecting before HTTP and process snapshots."
ssh_run 'ss -ltn | grep -E ":80 "' > "$run_dir/ports.before.txt" || true
ssh_run 'ps -L -o pid,tid,stat,pcpu,pmem,comm -p "$(pidof beetle)"' > "$run_dir/threads.before.txt" || true
collect_http_probe health /api/health "$run_dir/http/health.before.json"
collect_http_probe resource /api/resource "$run_dir/http/resource.before.json"
collect_http_probe metrics /api/metrics "$run_dir/http/metrics.before.json"

if [[ "$scenario" == "full_machine" ]]; then
  collect_http_probe config_system /api/config/system "$run_dir/http/config_system.json"
  collect_http_probe config_llm /api/config/llm "$run_dir/http/config_llm.json"
  collect_http_probe config_channels /api/config/channels "$run_dir/http/config_channels.json"
  collect_http_probe config_display /api/config/display "$run_dir/http/config_display.json"
  collect_http_probe tools /api/tools "$run_dir/http/tools.json"
  collect_http_probe memory_status /api/memory/status "$run_dir/http/memory_status.json"
  collect_http_probe diagnose /api/diagnose "$run_dir/http/diagnose.json"
  collect_http_probe channel_connectivity /api/channel_connectivity "$run_dir/http/channel_connectivity.json"
fi

echo
echo "Step 3/5: live exercise window."
if [[ "$scenario" == "qq_text" || "$scenario" == "full_machine" ]]; then
  print_qq_test_plan
  echo "QQ semantic acceptance file: $qq_acceptance_file"
  echo "Send real QQ messages to bot 1903462822 now. The script will wait $duration seconds."
fi
if (( duration > 0 )); then
  sleep "$duration"
fi

echo
echo "Step 4/5: collecting after snapshots and journal since run start."
collect_http_probe health /api/health "$run_dir/http/health.json"
collect_http_probe resource /api/resource "$run_dir/http/resource.json"
collect_http_probe metrics /api/metrics "$run_dir/http/metrics.json"
ssh_run 'systemctl is-active beetle' > "$run_dir/systemd.is-active.after.txt"
ssh_run 'systemctl show beetle -p ActiveState -p SubState -p MainPID -p NRestarts -p ActiveEnterTimestamp --no-pager' \
  > "$run_dir/systemd.show.after.txt"
ssh_run 'ss -ltn | grep -E ":80 "' > "$run_dir/ports.after.txt" || true
ssh_run 'ps -L -o pid,tid,stat,pcpu,pmem,comm -p "$(pidof beetle)"' > "$run_dir/threads.after.txt" || true
ssh_run "journalctl -u beetle --since '$remote_since' --no-pager" > "$run_dir/journal.log"

cp "$run_dir/http/health.json" "$run_dir/http/health.latest.json"
cp "$run_dir/http/resource.json" "$run_dir/http/resource.latest.json"
cp "$run_dir/http/metrics.json" "$run_dir/http/metrics.latest.json"

cat > "$run_dir/summary.md" <<EOF
# Linux live flow summary

- scenario: $scenario
- remote: $remote
- run dir: $run_dir
- started remote local time: $remote_since
- expected QQ messages: $expected_messages
- pairing code supplied: $([[ -n "$pairing_code" ]] && echo yes || echo no)
- require display: $require_display

Evidence files:

- \`systemd.show.txt\`, \`systemd.show.after.txt\`
- \`journal.log\`
- \`http/health.json\`
- \`http/resource.json\`
- \`http/metrics.json\`
- \`http_probe_status.txt\`
- \`qq_acceptance.env\` for QQ semantic acceptance when applicable
EOF

echo
echo "Step 5/5: gating saved evidence."
run_gates

echo "Linux live flow passed:"
echo "  run dir: $run_dir"
echo "  journal: $run_dir/journal.log"
