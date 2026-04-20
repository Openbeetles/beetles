#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT_PATH="$ROOT_DIR/scripts/docker/linux_aarch64_build_docker.sh"
BUILD_SH="$ROOT_DIR/build.sh"

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

if [[ ! -f "$SCRIPT_PATH" ]]; then
  echo "FAIL: expected Linux aarch64 Docker bootstrap script at $SCRIPT_PATH" >&2
  exit 1
fi

assert_contains "$SCRIPT_PATH" 'CONTAINER_NAME="beetle-linux-aarch64"' \
  "bootstrap script must pin the beginner-facing container name"
assert_contains "$SCRIPT_PATH" 'IMAGE="rust:1-bookworm"' \
  "bootstrap script must use the official Rust Bookworm image"
assert_contains "$SCRIPT_PATH" '--platform linux/arm64' \
  "bootstrap script must create a Linux arm64 container"
assert_contains "$SCRIPT_PATH" '-v "\$ROOT_DIR:/workspace"' \
  "bootstrap script must mount the Beetle repository into /workspace"
assert_contains "$SCRIPT_PATH" 'sleep infinity' \
  "bootstrap script must create a persistent container instead of a one-shot build"
assert_contains "$SCRIPT_PATH" 'apt-get install -y build-essential pkg-config libasound2-dev libudev-dev ca-certificates curl wget git' \
  "bootstrap script must install the Linux build prerequisites"
assert_contains "$SCRIPT_PATH" 'docker exec -it \$CONTAINER_NAME bash' \
  "bootstrap script must print the command for entering the container"
assert_contains "$SCRIPT_PATH" 'cargo build --release --target aarch64-unknown-linux-gnu' \
  "bootstrap script must guide users to the GNU aarch64 build command"

assert_contains "$BUILD_SH" 'scripts/docker/linux_aarch64_build_docker\.sh' \
  "build.sh help should point Linux aarch64 users at the one-shot Docker bootstrap script"

echo "linux_aarch64_build_docker_bootstrap_test: ok"
