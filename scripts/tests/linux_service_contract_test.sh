#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
MAIN_RS="$ROOT_DIR/src/main.rs"
RELEASE_RS="$ROOT_DIR/src/runtime/linux_release.rs"
SERVICE_RS="$ROOT_DIR/src/runtime/linux_service.rs"

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

assert_contains "$ROOT_DIR/src/runtime/mod.rs" '^pub mod linux_service;$' \
  "runtime::linux_service must be exposed on Linux builds"
assert_contains "$MAIN_RS" 'linux_service::run_beetle_service_action' \
  "main.rs must delegate Linux restart/stop through runtime::linux_service"
assert_contains "$RELEASE_RS" 'linux_service::inspect_beetle_init_script_consistency' \
  "linux_release.rs must delegate init-script inspection through runtime::linux_service"
assert_contains "$SERVICE_RS" 'linux_systemd::run_beetle_systemd_action' \
  "linux_service.rs must prefer systemd when beetle is systemd-managed"
assert_contains "$SERVICE_RS" '/etc/init.d/beetle' \
  "linux_service.rs must support the shared SysV/init.d beetle path"
assert_contains "$SERVICE_RS" '-- run' \
  "linux_service.rs must validate the unified run entrypoint"
assert_contains "$MAIN_RS" 'systemd or /etc/init.d/beetle' \
  "CLI errors must describe the unified managed-service contract"

echo "linux_service_contract_test: ok"
