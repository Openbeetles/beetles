#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
FLOW="$ROOT_DIR/scripts/linux_live_test_flow.sh"
DOC="$ROOT_DIR/dev-docs/linux-raspberry-pi-live-test-flow.md"

assert_contains() {
  local file="$1"
  local pattern="$2"
  local message="$3"
  if ! rg -n -- "$pattern" "$file" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  file: $file" >&2
    echo "  missing pattern: $pattern" >&2
    exit 1
  fi
}

assert_absent() {
  local file="$1"
  local pattern="$2"
  local message="$3"
  if rg -n -- "$pattern" "$file" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  file: $file" >&2
    rg -n -- "$pattern" "$file" >&2 || true
    exit 1
  fi
}

assert_contains "$FLOW" 'scripts/linux_live_test_flow.sh --scenario NAME' \
  "Linux live flow must expose a fixed script entrypoint"
assert_contains "$FLOW" 'boot_idle       Collect systemd, journal, health, resource, and channel boot evidence' \
  "Linux flow must keep a boot/idle scenario"
assert_contains "$FLOW" 'qq_text         Collect evidence while real QQ messages are sent to the bot' \
  "Linux flow must keep a QQ text scenario"
assert_contains "$FLOW" 'full_machine    QQ semantic flow plus health/resource/metrics/tools/config/memory probes' \
  "Linux flow must keep a full-machine scenario"
assert_contains "$FLOW" 'output_dir="\$REPO_ROOT/target/linux-live"' \
  "Linux flow must write evidence under target/linux-live"
assert_contains "$FLOW" 'sshpass -e ssh' \
  "Linux flow must support password-based SSH without printing the password in commands"
assert_contains "$FLOW" 'systemctl is-active beetle' \
  "Linux flow must verify the managed beetle service state"
assert_contains "$FLOW" 'journalctl -u beetle --since' \
  "Linux flow must collect scoped real service logs"
assert_contains "$FLOW" 'curl -sS --max-time 15' \
  "Linux flow must collect HTTP evidence from the device"
assert_contains "$FLOW" "X-Pairing-Code" \
  "Linux flow must support protected config/tool/memory routes via pairing code"
assert_contains "$FLOW" 'QQ semantic test plan for bot 1903462822' \
  "Linux flow must drive real QQ semantic testing against the requested bot"
assert_contains "$FLOW" 'qq_basic_llm_answer=pending' \
  "QQ acceptance template must cover LLM answers"
assert_contains "$FLOW" 'qq_status_tool_read=pending' \
  "QQ acceptance template must cover live tool/status reads"
assert_contains "$FLOW" 'qq_memory_continuity=pending' \
  "QQ acceptance template must cover memory continuity"
assert_contains "$FLOW" 'qq_reminder_write_back=pending' \
  "QQ acceptance template must cover reminder write-back"
assert_contains "$FLOW" 'qq_plain_text_rendering=pending' \
  "QQ acceptance template must cover actual QQ-safe plain text rendering"
assert_contains "$FLOW" 'qq_capability_boundary=pending' \
  "QQ acceptance template must cover unavailable capability boundaries"
assert_contains "$FLOW" 'ui_health_resource_metrics=pending' \
  "QQ acceptance template must cover health/resource/metrics UI promises"
assert_contains "$FLOW" 'display_health=pending' \
  "QQ acceptance template must cover display health evidence"
assert_contains "$FLOW" 'require_qq_acceptance "\$qq_acceptance_file"' \
  "QQ scenarios must not pass without semantic acceptance evidence"
assert_contains "$FLOW" 'full_machine requires --pairing-code' \
  "full_machine must require pairing code for protected probes"
assert_contains "$FLOW" 'full_machine HTTP probes failed' \
  "full_machine must fail when protected HTTP evidence is missing"
assert_contains "$FLOW" 'expected \$expected_messages QQ inbound messages' \
  "QQ scenarios must gate on inbound message count from logs"
assert_contains "$FLOW" 'expected \$expected_messages QQ replies' \
  "QQ scenarios must gate on reply count from logs"
assert_contains "$FLOW" 'message enqueued' \
  "QQ inbound count must be based on real QQ WSS logs"
assert_contains "$FLOW" 'reply outbound enqueued' \
  "QQ reply count must be based on real agent/outbound logs"
assert_contains "$FLOW" 'panic\|abort\|segmentation fault\|stack overflow\|Failed to create task' \
  "Linux flow must fail on fatal runtime patterns"
assert_contains "$FLOW" 'dispatch_fail=\[1-9\]' \
  "Linux flow must fail when dispatch failure metrics increase"
assert_contains "$FLOW" 'llm_err=\[1-9\]' \
  "Linux flow must fail when LLM error metrics increase"
assert_contains "$FLOW" 'tool_err=\[1-9\]' \
  "Linux flow must fail when tool error metrics increase"
assert_contains "$FLOW" 'storage_contention=Critical' \
  "Linux flow must fail on critical resource contention"
assert_absent "$FLOW" 'espflash|ESPFLASH_PORT|/dev/cu\.|/dev/tty\.usb' \
  "Linux live flow must not reuse ESP serial/flash mechanics"

assert_contains "$DOC" 'scripts/linux_live_test_flow.sh --scenario full_machine' \
  "Linux flow document must name the full-machine command"
assert_contains "$DOC" 'target/linux-live/' \
  "Linux flow document must describe evidence directories"
assert_contains "$DOC" '机器人1903462822' \
  "Linux flow document must preserve the requested QQ bot identity"
assert_contains "$DOC" '没有证据的项不能写 pass' \
  "Linux flow document must require evidence-backed acceptance"

echo "linux_live_test_flow_contract_test: ok"
