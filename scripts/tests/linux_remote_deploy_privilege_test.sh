#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_SH="$ROOT_DIR/build.sh"

assert_contains() {
  local pattern="$1"
  local message="$2"
  if ! rg -n "$pattern" "$BUILD_SH" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  missing pattern: $pattern" >&2
    exit 1
  fi
}

assert_absent() {
  local pattern="$1"
  local message="$2"
  if rg -n "$pattern" "$BUILD_SH" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    rg -n "$pattern" "$BUILD_SH" >&2 || true
    exit 1
  fi
}

assert_contains 'linux_remote_default_build_dir\(\)' \
  "remote build directory should be computed from the remote user instead of hard-coding /root"
assert_absent 'DEFAULT_REMOTE_BUILD_DIR="/root/beetle-build"' \
  "non-root remote builds must not default to /root/beetle-build"
assert_contains 'linux_remote_prepare_privileged_prefix\(\)' \
  "remote deploy should detect whether sudo is needed before writing system paths"
assert_contains 'PRIVILEGED_PREFIX' \
  "remote deploy/install commands should run under a sudo-aware privileged prefix"
assert_contains 'linux_deploy_upload_embed_deps \|\| return 1' \
  "embed-deps upload failures must stop deployment"
assert_contains 'linux_deploy_install_payloads \|\| return 1' \
  "payload install failures must stop deployment before success messaging"
assert_contains 'linux_deploy_manage_service \|\| return 1' \
  "service-management failures must stop deployment before success messaging"
assert_contains 'linux_deploy_verify_remote_install \|\| return 1' \
  "remote verification failures must stop deployment before showing success"
assert_contains 'linux_deploy_verify_remote_install' \
  "remote deployment should still verify the installed layout"

verify_line=$(rg -n 'linux_deploy_verify_remote_install' "$BUILD_SH" | head -n1 | cut -d: -f1)
show_line=$(rg -n 'linux_deploy_show_next_steps' "$BUILD_SH" | head -n1 | cut -d: -f1)
if [[ -z "$verify_line" || -z "$show_line" ]]; then
  echo "FAIL: expected both verification and success-summary calls in linux_deploy_main" >&2
  exit 1
fi
if (( verify_line >= show_line )); then
  echo "FAIL: deployment summary must be shown only after remote verification succeeds" >&2
  echo "  verify line: $verify_line" >&2
  echo "  summary line: $show_line" >&2
  exit 1
fi

echo "linux_remote_deploy_privilege_test: ok"
