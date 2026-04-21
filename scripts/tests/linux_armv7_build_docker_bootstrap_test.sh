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

assert_contains "$SCRIPT_PATH" '^CONTAINER_NAME="beetle-linux-armv7-gnu-cross"$' \
  "armv7 helper must use the dedicated GNU cross-build container name"
assert_contains "$SCRIPT_PATH" '^IMAGE="rust:1-bookworm"$' \
  "armv7 helper must use the multi-arch Rust image"
assert_contains "$SCRIPT_PATH" '^WORKDIR_IN_CONTAINER="/workspace/beetle"$' \
  "armv7 helper must mount beetle at the established workspace path"
assert_contains "$SCRIPT_PATH" '^BUILD_TARGET="armv7-unknown-linux-gnueabihf"$' \
  "armv7 helper must build the GNU target used by the cross-build path"
assert_contains "$SCRIPT_PATH" '^TARGET_LINKER="arm-linux-gnueabihf-gcc"$' \
  "armv7 helper must use the GNU cross linker"
assert_contains "$SCRIPT_PATH" '^CONTAINER_PATH="/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"$' \
  "armv7 helper must keep cargo and rustup on PATH inside the container"
assert_contains "$SCRIPT_PATH" '^DOCKER_PLATFORM="\$\(host_docker_platform\)"$' \
  "armv7 helper must choose the Docker platform from the host architecture"
assert_contains "$SCRIPT_PATH" 'docker create \\' \
  "armv7 helper must create the reusable container explicitly"
assert_contains "$SCRIPT_PATH" -- '--platform "\$DOCKER_PLATFORM" \\' \
  "armv7 helper must run a host-architecture container instead of qemu-arm"
assert_contains "$SCRIPT_PATH" -- '-v "\$ROOT_DIR:/workspace/beetle" \\' \
  "armv7 helper must bind-mount the beetle workspace into the container"
assert_contains "$SCRIPT_PATH" 'dpkg --print-foreign-architectures \| grep -qx armhf \|\| dpkg --add-architecture armhf' \
  "armv7 helper must enable armhf multiarch before installing target packages"
assert_contains "$SCRIPT_PATH" 'apt-get update -o Acquire::Retries=5' \
  "armv7 helper must retry apt metadata refresh for transient mirror failures"
assert_contains "$SCRIPT_PATH" 'apt-get install -y -o Acquire::Retries=5 --fix-missing' \
  "armv7 helper must retry package downloads for transient mirror failures"
assert_contains "$SCRIPT_PATH" '^retry_in_container\(\) \{$' \
  "armv7 helper must centralize transient network retries in one helper"
assert_contains "$SCRIPT_PATH" 'retry_in_container "rustup target add \$BUILD_TARGET"' \
  "armv7 helper must retry rustup target downloads for transient network failures"
assert_contains "$SCRIPT_PATH" 'rustup target add \$BUILD_TARGET' \
  "armv7 helper must install the Rust armv7 GNU target"
assert_contains "$SCRIPT_PATH" 'cargo build --release --target armv7-unknown-linux-gnueabihf' \
  "armv7 helper must advertise the exact GNU cross-build command"

echo "linux_armv7_build_docker_bootstrap_test: ok"
