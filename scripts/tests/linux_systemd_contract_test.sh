#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
MAIN_RS="$ROOT_DIR/src/main.rs"
RELEASE_RS="$ROOT_DIR/src/runtime/linux_release.rs"
SYSTEMD_RS="$ROOT_DIR/src/runtime/linux_systemd.rs"

assert_contains() {
  local file="$1"
  local pattern="$2"
  local message="$3"
  if ! rg -n "$pattern" "$file" >/dev/null 2>&1; then
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
  if rg -n "$pattern" "$file" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  file: $file" >&2
    rg -n "$pattern" "$file" >&2 || true
    exit 1
  fi
}

assert_contains "$ROOT_DIR/src/runtime/mod.rs" '^pub mod linux_systemd;$' \
  "runtime::linux_systemd must be exposed on Linux builds"
assert_contains "$RELEASE_RS" 'linux_systemd::inspect_beetle_systemd_unit_consistency' \
  "linux_release.rs must delegate systemd unit inspection through runtime::linux_systemd"

assert_absent "$MAIN_RS" '/etc/systemd/system/beetle.service' \
  "main.rs must not hardcode a single systemd unit path"
assert_absent "$RELEASE_RS" '/etc/systemd/system/beetle.service' \
  "linux_release.rs must not hardcode a single systemd unit path"

assert_contains "$SYSTEMD_RS" 'systemctl' \
  "linux_systemd.rs must resolve unit state through systemctl"
assert_contains "$SYSTEMD_RS" 'LoadState' \
  "linux_systemd.rs must query LoadState to detect managed beetle services"
assert_contains "$SYSTEMD_RS" 'FragmentPath' \
  "linux_systemd.rs must query FragmentPath for release consistency inspection"

echo "linux_systemd_contract_test: ok"
