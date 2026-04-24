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
docker_release_arg_calls="$(rg -n 'run_linux_docker_build "\$BUILD_TARGET" "\$\{RELEASE_ARGS\[@\]\}"' "$BUILD_SH" | wc -l | tr -d '[:space:]')"
if [[ "$docker_release_arg_calls" != "2" ]]; then
  echo "FAIL: docker build must only run from the final release build path and fallback path" >&2
  echo "  found call count: $docker_release_arg_calls" >&2
  exit 1
fi
assert_contains '3\) BUILD_TARGET="x86_64-unknown-linux-gnu"' \
  "x86_64 Docker builds must use the native GNU Linux target instead of cross-building musl inside Linux"
assert_contains 'if \[\[ "\$target" == "x86_64-unknown-linux-gnu" \]\]; then' \
  "x86_64 docker helper must support the GNU target selected by the Docker path"
assert_contains 'docker run --rm --platform linux/amd64' \
  "x86_64 Docker build must force an amd64 container instead of inheriting the host Docker default platform"
assert_contains '-e DEBIAN_FRONTEND=noninteractive' \
  "x86_64 Docker build must run apt non-interactively without debconf frontend warnings"
assert_contains 'apt-get install -y --no-install-recommends pkg-config libasound2-dev libudev-dev' \
  "x86_64 Docker build must install native Linux dev headers required by linux-full dependencies"
assert_contains 'bash "\$SCRIPT_ROOT/scripts/docker/linux_armv7_build_docker\.sh"' \
  "armv7 docker build must bootstrap the shared GNU helper container"
assert_contains 'docker exec beetle-linux-armv7-gnu-cross /bin/bash -lc' \
  "armv7 docker build must compile inside the prepared helper container"
assert_contains 'bash "\$SCRIPT_ROOT/scripts/docker/linux_aarch64_build_docker\.sh"' \
  "aarch64 docker build must bootstrap the shared GNU helper container"
assert_contains 'docker exec beetle-linux-aarch64 /bin/bash -lc' \
  "aarch64 docker build must compile inside the prepared helper container"
assert_contains 'Docker build target \$target is deprecated' \
  "deprecated ARM musl docker targets must fail explicitly instead of silently using stale paths"
assert_contains 'if \[\[ -z "\$\{USE_DOCKER:-\}" \]\]; then' \
  "local rustup target installation must stay behind the non-Docker guard"

echo "linux_docker_build_contract_test: ok"
