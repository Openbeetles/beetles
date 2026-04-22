#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
CARGO_TOML="$ROOT_DIR/Cargo.toml"
LIB_RS="$ROOT_DIR/src/lib.rs"
TOOLS_MOD_RS="$ROOT_DIR/src/tools/mod.rs"
REGISTRY_RS="$ROOT_DIR/src/tools/registry.rs"
OFFICE_MOD_RS="$ROOT_DIR/src/office/mod.rs"

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

assert_contains "$CARGO_TOML" '^capability_office = \[\]$' \
  "Cargo features must define an explicit capability_office gate"
assert_absent "$CARGO_TOML" '^\s*"capability_office",\s*$' \
  "capability_office must not be enabled by default"

assert_contains "$LIB_RS" 'feature = "capability_office"' \
  "lib.rs must contain capability_office gates"
for item in 'pub mod mail;' 'pub mod contacts_directory;'; do
  line="$(rg -n -F "$item" "$LIB_RS" | head -n1 | cut -d: -f1 || true)"
  if [[ -z "$line" ]]; then
    echo "FAIL: missing lib.rs item: $item" >&2
    exit 1
  fi
  if ! sed -n "$((line-4)),$((line-1))p" "$LIB_RS" | rg 'feature = "capability_office"' >/dev/null 2>&1; then
    echo "FAIL: $item must be guarded by capability_office" >&2
    exit 1
  fi
done

assert_contains "$ROOT_DIR/src/documents/mod.rs" 'feature = "capability_office"' \
  "documents/mod.rs must gate remote office-specific documents surfaces"

assert_contains "$TOOLS_MOD_RS" 'feature = "capability_office"' \
  "tools/mod.rs must contain capability_office gates"
for item in \
  'pub mod calendar;' \
  'pub mod mail;' \
  'pub mod documents;' \
  'pub mod contacts_directory;' \
  'pub mod office_config;' \
  'pub mod office_status;'; do
  line="$(rg -n -F "$item" "$TOOLS_MOD_RS" | head -n1 | cut -d: -f1 || true)"
  if [[ -z "$line" ]]; then
    echo "FAIL: missing tools/mod.rs item: $item" >&2
    exit 1
  fi
  if ! sed -n "$((line-4)),$((line-1))p" "$TOOLS_MOD_RS" | rg 'feature = "capability_office"' >/dev/null 2>&1; then
    echo "FAIL: $item must be guarded by capability_office" >&2
    exit 1
  fi
done

assert_contains "$REGISTRY_RS" 'fn register_office_tools\(' \
  "registry must split office tool wiring into a dedicated register_office_tools function"
core_start="$(rg -n '^fn register_core_tools\(' "$REGISTRY_RS" | head -n1 | cut -d: -f1 || true)"
office_start="$(rg -n '^fn register_office_tools\(' "$REGISTRY_RS" | head -n1 | cut -d: -f1 || true)"
if [[ -z "$core_start" || -z "$office_start" ]]; then
  echo "FAIL: expected register_core_tools and register_office_tools markers" >&2
  exit 1
fi
if sed -n "${core_start},$((office_start-1))p" "$REGISTRY_RS" | rg 'OfficeConfigTool|OfficeStatusTool|MailTool|DocumentsTool|ContactsDirectoryTool|CalendarTool' >/dev/null 2>&1; then
  echo "FAIL: register_core_tools must no longer directly wire office tools" >&2
  exit 1
fi

assert_contains "$OFFICE_MOD_RS" 'target_arch = "xtensa"' \
  "office/mod.rs must contain target gates for host-only office surfaces"
for item in \
  'mod assessment;' \
  'mod authority_source;' \
  'mod config_management;' \
  'mod provider_schema;' \
  'mod public_contract;' \
  'mod resolver;' \
  'mod service;' \
  'mod tool_doctrine;' \
  'pub use assessment::' \
  'pub use authority_source::' \
  'pub(crate) use config_management::infer_single_capability_for_provider;' \
  'pub use config_management::' \
  'pub use provider_schema::' \
  'pub(crate) use public_contract::parse_public_account_upsert_request_value;' \
  'pub use resolver::' \
  'pub use service::' \
  'pub use tool_doctrine::'; do
  line="$(rg -n -F "$item" "$OFFICE_MOD_RS" | head -n1 | cut -d: -f1 || true)"
  if [[ -z "$line" ]]; then
    echo "FAIL: missing office/mod.rs item: $item" >&2
    exit 1
  fi
  if ! sed -n "$((line-4)),$((line-1))p" "$OFFICE_MOD_RS" | rg 'feature = "capability_office"' >/dev/null 2>&1; then
    echo "FAIL: $item must be guarded by capability_office" >&2
    exit 1
  fi
  if ! sed -n "$((line-4)),$((line-1))p" "$OFFICE_MOD_RS" | rg 'target_arch = "xtensa"|target_arch = "riscv32"' >/dev/null 2>&1; then
    echo "FAIL: $item must be guarded as host-only for ESP targets" >&2
    exit 1
  fi
done

echo "esp_office_compile_gate_test: ok"
