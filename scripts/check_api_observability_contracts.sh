#!/usr/bin/env bash
# Observability API contract gate:
# - /api/health stays lightweight liveness
# - /api/resource stays lightweight default polling
# - /api/metrics stays counters/latency only

set -euo pipefail

if ! command -v rg >/dev/null 2>&1; then
  echo "check_api_observability_contracts: ripgrep (rg) is required" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

prod_source() {
  sed '/^#\[cfg(test)\]/,$d' "$1"
}

check_prod_absent() {
  local file="$1"
  local pattern="$2"
  local message="$3"
  local matches
  matches="$(prod_source "$file" | rg -n "$pattern" || true)"
  if [[ -n "$matches" ]]; then
    echo "FAIL: $message" >&2
    printf '%s\n' "$matches" >&2
    exit 1
  fi
}

check_prod_present() {
  local file="$1"
  local pattern="$2"
  local message="$3"
  if ! prod_source "$file" | rg -n "$pattern" >/dev/null; then
    echo "FAIL: $message" >&2
    exit 1
  fi
}

check_prod_absent \
  src/platform/http_server/handlers/health.rs \
  'resource_diagnostic_snapshot|orchestrator::snapshot|workflow_audit_snapshot|NetworkRuntimeSnapshot' \
  "health handler must not read resource/workflow diagnostics or serialize a full NetworkRuntimeSnapshot"

check_prod_absent \
  src/platform/http_server/handlers/health.rs \
  '^\s*(wifi|network|workflow)\s*:' \
  "health handler must not serialize legacy or deep top-level fields"

check_prod_present \
  src/platform/http_server/handlers/health.rs \
  'network_status:\s*NetworkHealthStatus' \
  "health handler must expose only the lightweight network_status summary"

check_prod_present \
  src/platform/http_server/handlers/health.rs \
  'network_runtime_snapshot\s*\(' \
  "health handler must derive network_status from the lightweight network runtime accessor"

check_prod_absent \
  src/platform/http_server/handlers/system_info.rs \
  'platform\.memory_snapshot\s*\(' \
  "system_info is a default Configure UI route and must not live-sample memory on the request path"

check_prod_absent \
  src/platform/http_server/handlers/resource.rs \
  'HealthBody|get_current_error|display_available|audio_duplex_capabilities' \
  "resource handler must not read health/explanation fields"

check_prod_absent \
  src/platform/http_server/handlers/resource.rs \
  'resource_diagnostic_snapshot|runtime::|route_execution_budget_snapshot|crash_metadata_snapshot' \
  "default resource handler must stay cached-light and must not aggregate deep diagnostics"

check_prod_absent \
  src/platform/http_server/handlers/resource.rs \
  '^\s*(network|firmware_identity|workflow|last_error|display|audio|admission|network_gate_summary|leases|display_lease_denied_total|runtime_capabilities|execution_budget|planes|plane_lifecycle|threads|write_back|crash)\s*:' \
  "resource handler must not serialize removed cross-contract or internal-only top-level fields"

check_prod_absent \
  src/platform/http_server/handlers/metrics.rs \
  'orchestrator::|handlers::(health|resource)|HealthBody|ResourceBody' \
  "metrics handler must not depend on health/resource snapshots"

check_prod_absent \
  src/platform/http_server/handlers/metrics.rs \
  '^\s*(status|network_status|display|audio|pressure|budget|admission|governance_metrics|network_gate_summary|planes|leases|threads)\s*:' \
  "metrics handler must not serialize health/resource objects"

require_rust_test_pattern() {
  local test_name="$1"
  local pattern="$2"
  local message="$3"
  if ! awk "/fn ${test_name}\\(\\)/,/^    }/" src/platform/http_server/router/catalog.rs | rg -n "$pattern" >/dev/null; then
    echo "FAIL: $message" >&2
    exit 1
  fi
}

require_file_pattern() {
  local file="$1"
  local pattern="$2"
  local message="$3"
  if ! rg -n "$pattern" "$file" >/dev/null; then
    echo "FAIL: $message" >&2
    exit 1
  fi
}

if ! rg -nU 'HttpRouteSpec::immediate_operator\(\s*ROUTE_HEALTH,\s*RouteMethod::Get' \
  src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: /api/health must stay an immediate operator route" >&2
  exit 1
fi

if ! rg -nU 'HttpRouteSpec::immediate_operator\(\s*ROUTE_RESOURCE,\s*RouteMethod::Get' \
  src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: /api/resource must stay an immediate cached-light operator route" >&2
  exit 1
fi

if ! rg -nU '#\[cfg\(any\(target_arch = "xtensa", target_arch = "riscv32"\)\)\]\s*HttpRouteSpec::immediate_operator\(\s*ROUTE_METRICS,\s*RouteMethod::Get' \
  src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: ESP /api/metrics must stay an immediate counter route" >&2
  exit 1
fi

if ! rg -n 'observability_route_classes_preserve_contract_boundaries' \
  src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: observability route class contract test is missing" >&2
  exit 1
fi

if ! rg -n 'default_config_ui_route_allowlist_stays_cached_lightweight' \
  src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: default Configure UI route allowlist contract test is missing" >&2
  exit 1
fi

if ! rg -n 'pairing_and_csrf_routes_stay_immediate_without_config_activity' \
  src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: pairing/csrf lightweight route contract test is missing" >&2
  exit 1
fi

for route in ROUTE_PAIRING_CODE ROUTE_CSRF_TOKEN; do
  require_rust_test_pattern \
    pairing_and_csrf_routes_stay_immediate_without_config_activity \
    "\\(\"GET\", ${route}\\)" \
    "pairing/csrf lightweight test must cover GET ${route}"
  require_rust_test_pattern \
    pairing_and_csrf_routes_stay_immediate_without_config_activity \
    "\\(\"OPTIONS\", ${route}\\)" \
    "pairing/csrf lightweight test must cover OPTIONS ${route}"
done
require_rust_test_pattern \
  pairing_and_csrf_routes_stay_immediate_without_config_activity \
  'RouteExecutionClass::ImmediateRoute' \
  "pairing/csrf lightweight test must assert ImmediateRoute"
require_rust_test_pattern \
  pairing_and_csrf_routes_stay_immediate_without_config_activity \
  'config_activity_phase\(\)' \
  "pairing/csrf lightweight test must assert no config activity"

for route in \
  ROUTE_PAIRING_CODE \
  ROUTE_CSRF_TOKEN \
  ROUTE_CONFIG_SYSTEM \
  ROUTE_CONFIG_LLM \
  ROUTE_CONFIG_CHANNELS \
  ROUTE_CONFIG_HARDWARE \
  ROUTE_CONFIG_AUDIO \
  ROUTE_CONFIG_DISPLAY \
  ROUTE_HEALTH \
  ROUTE_RESOURCE \
  ROUTE_CHANNEL_CONNECTIVITY; do
  require_rust_test_pattern \
    default_config_ui_route_allowlist_stays_cached_lightweight \
    "\\(\"GET\", ${route}\\)" \
    "default Configure UI allowlist test must cover GET ${route}"
done

for route in \
  ROUTE_WIFI_SCAN \
  ROUTE_HARDWARE_DISCOVERY \
  ROUTE_DIAGNOSE \
  ROUTE_MEMORY_STATUS; do
  require_rust_test_pattern \
    default_config_ui_route_allowlist_stays_cached_lightweight \
    "\\(\"GET\", ${route}\\)" \
    "default Configure UI denylist test must cover GET ${route}"
done

for route in \
  ROUTE_CHANNEL_CONNECTIVITY_REFRESH \
  ROUTE_MEMORY_MAINTENANCE \
  ROUTE_SKILLS_IMPORT; do
  require_rust_test_pattern \
    default_config_ui_route_allowlist_stays_cached_lightweight \
    "\\(\"POST\", ${route}\\)" \
    "default Configure UI denylist test must cover POST ${route}"
done

require_rust_test_pattern \
  default_config_ui_route_allowlist_stays_cached_lightweight \
  'RouteExecutionClass::ImmediateRoute' \
  "default Configure UI test must permit ImmediateRoute"
require_rust_test_pattern \
  default_config_ui_route_allowlist_stays_cached_lightweight \
  'RouteExecutionClass::LocalDiagnosticRoute' \
  "default Configure UI test must keep local diagnostic routes in the denylist"
require_rust_test_pattern \
  default_config_ui_route_allowlist_stays_cached_lightweight \
  'RouteExecutionClass::SlowDiagnosticRoute' \
  "default Configure UI test must keep slow diagnostic routes in the denylist"

require_file_pattern \
  configure-ui/package.json \
  '"test:desktop-base":.*src/api/endpoints/system\.test\.ts' \
  "configure-ui test:desktop-base must run the default endpoint allowlist test"

for endpoint in /api/health /api/resource /api/metrics /api/system_info /api/channel_connectivity; do
  require_file_pattern \
    configure-ui/src/api/endpoints/system.test.ts \
    "\"${endpoint}\"" \
    "default endpoint allowlist test must assert ${endpoint}"
done

for endpoint in /api/wifi/scan /api/diagnose /api/hardware/discovery /api/channel_connectivity/refresh /api/config; do
  require_file_pattern \
    configure-ui/src/api/endpoints/system.test.ts \
    "\"${endpoint}\"" \
    "default endpoint allowlist test must deny ${endpoint}"
done

for doc in docs/zh-cn/config-api.md docs/en-us/config-api.md; do
  if rg -n 'route class|ImmediateRoute|SnapshotRoute|SlowDiagnosticRoute|route-admission|network-gate|lease-detail|thread/plane|write-back detail|serial heartbeat|路由准入|网络门控|租约明细|线程/plane|write-back 明细|串口 heartbeat|deep-diagnostic|deep diagnostic|cached-light' "$doc" >/dev/null; then
    echo "FAIL: $doc must stay external-facing and must not expose internal route/governance terminology" >&2
    rg -n 'route class|ImmediateRoute|SnapshotRoute|SlowDiagnosticRoute|route-admission|network-gate|lease-detail|thread/plane|write-back detail|serial heartbeat|路由准入|网络门控|租约明细|线程/plane|write-back 明细|串口 heartbeat|deep-diagnostic|deep diagnostic|cached-light' "$doc" >&2
    exit 1
  fi

  for key in health.wifi health.network health.workflow resource.network resource.firmware_identity; do
    if ! rg -n "$key" "$doc" >/dev/null; then
      echo "FAIL: $doc must explicitly document removed duplicate field $key" >&2
      exit 1
    fi
  done

  for key in governance_metrics cpu_usage_percent load_average process_memory_kb; do
    if ! rg -n -- "- \`$key\`" "$doc" >/dev/null; then
      echo "FAIL: $doc must list /api/resource field $key" >&2
      exit 1
    fi
  done
done
