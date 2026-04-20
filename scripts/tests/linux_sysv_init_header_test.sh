#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
INIT_SCRIPT="$ROOT_DIR/packaging/linux/beetle.init"

assert_contains() {
  local pattern="$1"
  if ! grep -Eq "$pattern" "$INIT_SCRIPT"; then
    echo "FAIL: missing pattern '$pattern' in $INIT_SCRIPT" >&2
    exit 1
  fi
}

assert_contains '^### BEGIN INIT INFO$'
assert_contains '^# Provides:[[:space:]]+beetle$'
assert_contains '^# Required-Start:[[:space:]]+\$remote_fs \$syslog$'
assert_contains '^# Required-Stop:[[:space:]]+\$remote_fs \$syslog$'
assert_contains '^# Default-Start:[[:space:]]+2 3 4 5$'
assert_contains '^# Default-Stop:[[:space:]]+0 1 6$'
assert_contains '^# Short-Description:[[:space:]]+Beetle runtime$'
assert_contains '^### END INIT INFO$'

echo "linux_sysv_init_header_test: ok"
