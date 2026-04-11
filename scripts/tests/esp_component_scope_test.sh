#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
MAIN_CARGO_TOML="$ROOT_DIR/Cargo.toml"
HOSTED_COMPONENT_MANIFEST="$ROOT_DIR/components/beetle_hosted_deps/idf_component.yml"

if rg -n 'remote_component = \{ name = "espressif/(esp_wifi_remote|esp_hosted)"' "$MAIN_CARGO_TOML" >/dev/null 2>&1; then
  echo "FAIL: P4 hosted components leaked into root Cargo.toml global extra_components" >&2
  rg -n 'remote_component = \{ name = "espressif/(esp_wifi_remote|esp_hosted)"' "$MAIN_CARGO_TOML" >&2 || true
  exit 1
fi

if [[ ! -f "$HOSTED_COMPONENT_MANIFEST" ]]; then
  echo "FAIL: missing P4-only hosted dependency manifest: $HOSTED_COMPONENT_MANIFEST" >&2
  exit 1
fi

if ! rg -n 'espressif/(esp_wifi_remote|esp_hosted):' "$HOSTED_COMPONENT_MANIFEST" >/dev/null 2>&1; then
  echo "FAIL: P4-only hosted component manifest does not declare esp_hosted/esp_wifi_remote" >&2
  cat "$HOSTED_COMPONENT_MANIFEST" >&2
  exit 1
fi

if ! rg -n 'target in \[esp32p4\]' "$HOSTED_COMPONENT_MANIFEST" >/dev/null 2>&1; then
  echo "FAIL: P4-only hosted component manifest lost the esp32p4 target rule" >&2
  cat "$HOSTED_COMPONENT_MANIFEST" >&2
  exit 1
fi

echo "esp_component_scope_test: ok"
