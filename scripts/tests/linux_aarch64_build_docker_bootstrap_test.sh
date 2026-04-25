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

assert_contains "$SCRIPT_PATH" '^CONTAINER_NAME="beetle-linux-aarch64"$' \
  "aarch64 helper must keep the public container name stable"
assert_contains "$SCRIPT_PATH" '^IMAGE="rust:1-bookworm"$' \
  "aarch64 helper must use the official Rust Bookworm image"
assert_contains "$SCRIPT_PATH" '^WORKDIR_IN_CONTAINER="/workspace/beetle"$' \
  "aarch64 helper must mount beetle at the shared workspace path"
assert_contains "$SCRIPT_PATH" '^BUILD_TARGET="aarch64-unknown-linux-gnu"$' \
  "aarch64 helper must target the GNU aarch64 build"
assert_contains "$SCRIPT_PATH" '^TARGET_LINKER="aarch64-linux-gnu-gcc"$' \
  "aarch64 helper must define the GNU cross linker"
assert_contains "$SCRIPT_PATH" '^CONTAINER_PATH="/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"$' \
  "aarch64 helper must preserve cargo and rustup on PATH inside the container"
assert_contains "$SCRIPT_PATH" '^DOCKER_PLATFORM="\$\(host_docker_platform\)"$' \
  "aarch64 helper must choose the Docker platform from the host architecture"
assert_contains "$SCRIPT_PATH" '^uses_cross_toolchain\(\) \{$' \
  "aarch64 helper must distinguish native arm64 hosts from cross-build hosts"
assert_contains "$SCRIPT_PATH" 'docker "\$\{docker_create_args\[@\]\}" "\$IMAGE" sleep infinity >/dev/null' \
  "aarch64 helper must create a reusable named container"
assert_contains "$SCRIPT_PATH" '--platform "\$DOCKER_PLATFORM"' \
  "aarch64 helper must use the host-architecture Docker platform instead of a fixed qemu path"
assert_contains "$SCRIPT_PATH" '-v "\$ROOT_DIR:/workspace/beetle"' \
  "aarch64 helper must bind-mount the Beetle repo into the container"
assert_contains "$SCRIPT_PATH" 'dpkg --print-foreign-architectures \| grep -qx arm64 \|\| dpkg --add-architecture arm64' \
  "aarch64 helper must enable arm64 multiarch when cross-building from non-arm64 hosts"
assert_contains "$SCRIPT_PATH" 'apt-get update -o Acquire::Retries=5' \
  "aarch64 helper must retry apt metadata refresh for transient mirror failures"
assert_contains "$SCRIPT_PATH" 'apt-get install -y -o Acquire::Retries=5 --fix-missing' \
  "aarch64 helper must retry package downloads for transient mirror failures"
assert_contains "$SCRIPT_PATH" '^retry_in_container\(\) \{$' \
  "aarch64 helper must centralize transient network retries in one helper"
assert_contains "$SCRIPT_PATH" 'retry_in_container "rustup target add \$BUILD_TARGET"' \
  "aarch64 helper must retry rustup target downloads for transient network failures"
assert_contains "$SCRIPT_PATH" 'rustup target add \$BUILD_TARGET' \
  "aarch64 helper must install the Rust aarch64 GNU target"
assert_contains "$SCRIPT_PATH" 'TARGET=linux-aarch64 ./build.sh --no-deploy' \
  "aarch64 helper must advertise the Cargo-metadata-rooted GNU container build command"

assert_contains "$BUILD_SH" 'bash "\$SCRIPT_ROOT/scripts/docker/linux_aarch64_build_docker\.sh"' \
  "build.sh must invoke the internal aarch64 helper from the public Docker build path"

echo "linux_aarch64_build_docker_bootstrap_test: ok"
