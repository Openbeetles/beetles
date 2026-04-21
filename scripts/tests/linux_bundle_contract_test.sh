#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
PACKAGE_SCRIPT="$ROOT_DIR/scripts/package_linux_release.sh"
PACKAGE_README="$ROOT_DIR/packaging/linux/README.txt"
EMBED_README="$ROOT_DIR/packaging/linux/embed-deps/README.md"
BUILD_DOC_ZH="$ROOT_DIR/docs/zh-cn/build-script.md"
BUILD_DOC_EN="$ROOT_DIR/docs/en-us/build-script.md"

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

assert_absent "$PACKAGE_SCRIPT" 'linux-\$\{triple\}-musl' \
  "package tarball names must not hardcode musl for every Linux architecture"
assert_contains "$PACKAGE_README" '^Beetle Linux bundle$' \
  "package README title must describe the generic Linux bundle, not a musl-only bundle"
assert_absent "$PACKAGE_README" 'one-click / SSH install is not the story yet' \
  "package README must not deny the public build.sh deploy flow"
assert_contains "$PACKAGE_README" '\./build\.sh --deploy-linux' \
  "package README must point users at the public build.sh deploy entry"
assert_absent "$EMBED_README" 'unknown-linux-musl|unknown-linux-musleabihf' \
  "embed-deps README must describe architecture directories without hardcoding musl Rust targets"
assert_absent "$BUILD_DOC_ZH" '如果你只是想先准备一个 Linux aarch64 GNU 构建容器' \
  "user-facing zh build docs must not expose internal Docker helper scripts as a public workflow"
assert_absent "$BUILD_DOC_EN" 'If you only want a ready Linux aarch64 GNU build container' \
  "user-facing en build docs must not expose internal Docker helper scripts as a public workflow"
assert_absent "$BUILD_DOC_ZH" '\./scripts/docker/linux_' \
  "user-facing zh build docs must not tell users to run internal Docker helper scripts directly"
assert_absent "$BUILD_DOC_EN" '\./scripts/docker/linux_' \
  "user-facing en build docs must not tell users to run internal Docker helper scripts directly"

echo "linux_bundle_contract_test: ok"
