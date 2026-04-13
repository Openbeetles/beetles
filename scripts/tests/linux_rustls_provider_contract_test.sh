#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
MAIN_RS="$ROOT_DIR/src/main.rs"

install_line="$(rg -n 'install_linux_rustls_crypto_provider\(\);' "$MAIN_RS" | head -n1 | cut -d: -f1 || true)"
platform_init_line="$(rg -n 'if let Err\(e\) = platform\.init\(\)' "$MAIN_RS" | head -n1 | cut -d: -f1 || true)"
fn_line="$(rg -n '^fn install_linux_rustls_crypto_provider\(\)' "$MAIN_RS" | head -n1 | cut -d: -f1 || true)"
provider_line="$(rg -n 'rustls::crypto::ring::default_provider\(\)\.install_default\(\)' "$MAIN_RS" | head -n1 | cut -d: -f1 || true)"

if [[ -z "$install_line" || -z "$platform_init_line" || -z "$fn_line" || -z "$provider_line" ]]; then
  echo "FAIL: missing Linux rustls provider bootstrap markers in src/main.rs" >&2
  exit 1
fi

if (( install_line >= platform_init_line )); then
  echo "FAIL: Linux rustls provider must be installed before platform initialization" >&2
  echo "  install line: $install_line" >&2
  echo "  platform init line: $platform_init_line" >&2
  exit 1
fi

echo "linux_rustls_provider_contract_test: ok"
