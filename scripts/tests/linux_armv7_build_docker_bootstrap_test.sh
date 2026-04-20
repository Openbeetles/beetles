#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT_PATH="$ROOT_DIR/scripts/docker/linux_armv7_build_docker.sh"

assert_contains() {
  local path="$1"
  local pattern="$2"
  local message="$3"
  if ! rg -n -- "$pattern" "$path" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  missing pattern: $pattern" >&2
    exit 1
  fi
}

if [[ ! -f "$SCRIPT_PATH" ]]; then
  echo "FAIL: expected $SCRIPT_PATH to exist" >&2
  exit 1
fi

assert_contains "$SCRIPT_PATH" '^CONTAINER_NAME="orangepi-zero-lts-bionic"$' \
  "armv7 helper must reuse the long-lived orangepi build container"
assert_contains "$SCRIPT_PATH" '^IMAGE="orangepi-zero-lts:bionic-2.0.8"$' \
  "armv7 helper must target the dedicated orangepi build image"
assert_contains "$SCRIPT_PATH" '^WORKDIR_IN_CONTAINER="/workspace/beetle"$' \
  "armv7 helper must mount beetle at the established workspace path"
assert_contains "$SCRIPT_PATH" '^BUILD_TARGET="armv7-unknown-linux-gnueabihf"$' \
  "armv7 helper must build the GNU target used by the native armv7 container path"
assert_contains "$SCRIPT_PATH" 'docker create \\' \
  "armv7 helper must create the reusable container explicitly"
assert_contains "$SCRIPT_PATH" -- '--platform linux/arm/v7 \\' \
  "armv7 helper must pin the container architecture explicitly"
assert_contains "$SCRIPT_PATH" -- '-v "\$ROOT_DIR:/workspace/beetle" \\' \
  "armv7 helper must bind-mount the beetle workspace into the container"
assert_contains "$SCRIPT_PATH" "cargo \\+stable build --release --target armv7-unknown-linux-gnueabihf" \
  "armv7 helper must advertise the exact stable GNU build command"

echo "linux_armv7_build_docker_bootstrap_test: ok"
