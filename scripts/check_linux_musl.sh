#!/usr/bin/env bash
# Linux musl cross-check (aligned with .github/workflows/rust.yml linux-musl-check).
# Uses stable toolchain so repo rust-toolchain.toml (esp) does not apply.
# If plain `cargo check` fails (linker), retries with cargo-zigbuild (single fallback).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

export RUSTUP_TOOLCHAIN=stable

rustup target add armv7-unknown-linux-musleabihf
rustup target add riscv64gc-unknown-linux-musl

check_target() {
  local t=$1
  if cargo check --target "$t"; then
    return 0
  fi
  echo "check_linux_musl: cargo check failed for $t; retrying with cargo-zigbuild" >&2
  if ! command -v cargo-zigbuild >/dev/null 2>&1; then
    cargo install cargo-zigbuild --locked
  fi
  cargo zigbuild --target "$t"
}

check_target armv7-unknown-linux-musleabihf
check_target riscv64gc-unknown-linux-musl

# Linux 依赖图不得包含 ESP 栈（与 dev-docs/linux-platform-plan.md §2.1 验收一致）。
assert_no_esp_stack_in_tree() {
  local t=$1
  if cargo tree -p beetle --target "$t" 2>/dev/null | grep -E 'esp-idf-svc|embedded-svc' | grep -q .; then
    echo "FAIL: Linux dependency tree must not include esp-idf-svc or embedded-svc (target $t)" >&2
    cargo tree -p beetle --target "$t" 2>/dev/null | grep -E 'esp-idf-svc|embedded-svc' >&2
    exit 1
  fi
}

assert_no_esp_stack_in_tree armv7-unknown-linux-musleabihf
assert_no_esp_stack_in_tree riscv64gc-unknown-linux-musl

echo "OK: linux musl checks passed"
