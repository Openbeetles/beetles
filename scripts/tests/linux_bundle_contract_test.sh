#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
PACKAGE_SCRIPT="$ROOT_DIR/scripts/package_linux_release.sh"
PACKAGE_README="$ROOT_DIR/packaging/linux/README.txt"
EMBED_README="$ROOT_DIR/packaging/linux/embed-deps/README.md"
BUILD_DOC_ZH="$ROOT_DIR/docs/zh-cn/build-script.md"
BUILD_DOC_EN="$ROOT_DIR/docs/en-us/build-script.md"
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

assert_contains_fixed() {
  local file="$1"
  local text="$2"
  local message="$3"
  if ! rg -n -F -- "$text" "$file" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  file: $file" >&2
    echo "  missing text: $text" >&2
    exit 1
  fi
}

assert_absent() {
  local file="$1"
  local pattern="$2"
  local message="$3"
  if rg -n -- "$pattern" "$file" >/dev/null 2>&1; then
    echo "FAIL: $message" >&2
    echo "  file: $file" >&2
    rg -n "$pattern" "$file" >&2 || true
    exit 1
  fi
}

assert_absent "$PACKAGE_SCRIPT" 'linux-\$\{triple\}-musl' \
  "package tarball names must not hardcode musl for every Linux architecture"
assert_contains_fixed "$PACKAGE_SCRIPT" '--binary target/<triple>/release/beetle' \
  "package script must expose the single-artifact binary argument form used by build.sh"
assert_contains_fixed "$PACKAGE_SCRIPT" '--target <triple>' \
  "package script must expose the single-artifact target argument form used by build.sh"
assert_contains "$PACKAGE_README" '^Beetle Linux bundle$' \
  "package README title must describe the generic Linux bundle, not a musl-only bundle"
assert_absent "$PACKAGE_README" 'one-click / SSH install is not the story yet' \
  "package README must not deny the public build.sh deploy flow"
assert_contains "$PACKAGE_README" 'TARGET=linux \./build\.sh --package-linux' \
  "package README must point users at the public one-command bundle flow"
assert_contains "$PACKAGE_README" '\./build\.sh --deploy-linux' \
  "package README must point users at the public build.sh deploy entry"
assert_absent "$EMBED_README" 'unknown-linux-musl|unknown-linux-musleabihf' \
  "embed-deps README must describe architecture directories without hardcoding musl Rust targets"
assert_contains "$BUILD_SH" '--package-linux' \
  "build.sh must expose the Linux bundle flag on the public entrypoint"
assert_absent "$BUILD_DOC_ZH" '如果你只是想先准备一个 Linux aarch64 GNU 构建容器' \
  "user-facing zh build docs must not expose internal Docker helper scripts as a public workflow"
assert_absent "$BUILD_DOC_EN" 'If you only want a ready Linux aarch64 GNU build container' \
  "user-facing en build docs must not expose internal Docker helper scripts as a public workflow"
assert_absent "$BUILD_DOC_ZH" '\./scripts/docker/linux_' \
  "user-facing zh build docs must not tell users to run internal Docker helper scripts directly"
assert_absent "$BUILD_DOC_EN" '\./scripts/docker/linux_' \
  "user-facing en build docs must not tell users to run internal Docker helper scripts directly"
assert_contains "$BUILD_DOC_ZH" 'TARGET=linux \./build\.sh --package-linux' \
  "user-facing zh build docs must expose the public one-command Linux bundle flow"
assert_contains "$BUILD_DOC_EN" 'TARGET=linux \./build\.sh --package-linux' \
  "user-facing en build docs must expose the public one-command Linux bundle flow"
assert_contains "$BUILD_DOC_ZH" '优先复用已保存的远端 Linux 主机' \
  "zh build docs must describe the automatic remote fallback so users do not need to pick build backends manually"
assert_contains "$BUILD_DOC_EN" 'saved remote Linux host' \
  "en build docs must describe the automatic remote fallback so users do not need to pick build backends manually"

echo "linux_bundle_contract_test: ok"
