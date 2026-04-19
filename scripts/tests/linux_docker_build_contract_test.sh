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

assert_contains '^run_linux_docker_build\(\) \{$' \
  "docker build helper must exist"
assert_contains 'local cargo_args=\("\$@"\)' \
  "docker build helper must accept forwarded cargo args"
assert_contains 'cargo_cmd\+="\$cargo_args_quoted"' \
  "docker build helper must append forwarded cargo args into the cargo command"
assert_contains 'run_linux_docker_build "\$BUILD_TARGET" "\$\{RELEASE_ARGS\[@\]\}"' \
  "docker build helper call sites must forward RELEASE_ARGS so features and build args are preserved"
assert_contains 'rustup target add x86_64-unknown-linux-musl && \$cargo_cmd' \
  "x86_64 docker helper must run the shared cargo command"
assert_contains 'bash -c "\$cargo_cmd"' \
  "armv7/aarch64 docker helpers must run the shared cargo command"

echo "linux_docker_build_contract_test: ok"
