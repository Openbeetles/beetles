#!/usr/bin/env bash
# Check that no thread in the source uses a hard-coded small stack (8192 / 16384) where
# a STACK_* constant should be used instead (these threads call rustls on Linux).
#
# Allowlist: threads known to be safe (never call create_http_client / TLS on Linux).
# All other spawn_planned / spawn_guarded_with_profile_handle calls with a literal
# 8192 or 16384 are flagged for review.
#
# Usage: ./scripts/check_thread_stack.sh
#   Returns 0 if no violations found, 1 otherwise.

set -euo pipefail

if ! command -v rg >/dev/null 2>&1; then
  echo "check_thread_stack: ripgrep (rg) is required" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# Threads allowed to keep small stacks (no HTTPS/TLS on their Linux path).
ALLOWLIST="dispatch|http_server|bg_timer|heartbeat|cli_repl|restart_defer|buzzer_off|display|wifi_worker|audio_io_worker"

VIOLATIONS=0

# Patterns: spawn_planned("name", literal_small_stack, ...)
# We use rg to find matching lines, then check if the thread name is in the allowlist.
check_file() {
  local file="$1"
  # Match lines with spawn_planned or spawn_guarded*handle that have a literal 8192/16384
  rg --no-heading -n 'spawn_planned\(|spawn_guarded_with_profile_handle\(' "$file" \
    | rg '(8192|16384)' \
    | while IFS=: read -r lineno content; do
        # Extract the first quoted string (thread name) from the line
        name=$(echo "$content" | rg -o '"[^"]+"' | head -1 | tr -d '"' 2>/dev/null || true)
        if [[ -z "$name" ]]; then
          continue
        fi
        if echo "$name" | rg -q "^($ALLOWLIST)"; then
          continue
        fi
        echo "WARN [thread-stack] $file:$lineno — thread '$name' uses hard-coded small stack:" >&2
        echo "  $content" >&2
        echo "VIOLATION"
      done
}

FILES=(src/main.rs src/channels/dispatch.rs src/heartbeat/mod.rs src/bg_timer.rs)

for f in "${FILES[@]}"; do
  [[ -f "$f" ]] || continue
  count=$(check_file "$f" | grep -c "^VIOLATION" || true)
  VIOLATIONS=$((VIOLATIONS + count))
done

if [[ $VIOLATIONS -gt 0 ]]; then
  echo "" >&2
  echo "Found $VIOLATIONS hard-coded small stack(s) that may call rustls on Linux." >&2
  echo "Use STACK_CHANNEL_SENDER / STACK_AGENT_LOOP / STACK_CHANNEL_WS from src/util.rs." >&2
  exit 1
fi

echo "OK: no suspicious hard-coded small thread stacks found"
exit 0
