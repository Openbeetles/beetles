#!/usr/bin/env bash
# Observability API contract gate:
# - /api/health stays lightweight liveness
# - /api/resource stays resource/admission diagnostics
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
  src/platform/http_server/handlers/resource.rs \
  'HealthBody|get_current_error|display_available|audio_duplex_capabilities' \
  "resource handler must not read health/explanation fields"

check_prod_absent \
  src/platform/http_server/handlers/resource.rs \
  '^\s*(network|firmware_identity|workflow|last_error|display|audio)\s*:' \
  "resource handler must not serialize removed cross-contract top-level fields"

check_prod_present \
  src/platform/http_server/handlers/resource.rs \
  'network_gate_summary:\s*NetworkGateSummaryBody' \
  "resource handler must expose the network gate summary instead of full network"

check_prod_absent \
  src/platform/http_server/handlers/metrics.rs \
  'orchestrator::|handlers::(health|resource)|HealthBody|ResourceBody' \
  "metrics handler must not depend on health/resource snapshots"

check_prod_absent \
  src/platform/http_server/handlers/metrics.rs \
  '^\s*(status|network_status|display|audio|pressure|budget|admission|governance_metrics|network_gate_summary|planes|leases|threads)\s*:' \
  "metrics handler must not serialize health/resource objects"

if ! rg -nU 'HttpRouteSpec::immediate_operator\(\s*ROUTE_HEALTH,\s*RouteMethod::Get' \
  src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: /api/health must stay an immediate operator route" >&2
  exit 1
fi

if ! rg -nU 'HttpRouteSpec::snapshot_operator\(\s*ROUTE_RESOURCE,\s*RouteMethod::Get' \
  src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: /api/resource must stay a snapshot operator route" >&2
  exit 1
fi

if ! rg -n 'observability_route_classes_preserve_contract_boundaries' \
  src/platform/http_server/router/catalog.rs >/dev/null; then
  echo "FAIL: observability route class contract test is missing" >&2
  exit 1
fi

for doc in docs/zh-cn/config-api.md docs/en-us/config-api.md; do
  for key in health.wifi health.network health.workflow resource.network resource.firmware_identity; do
    if ! rg -n "$key" "$doc" >/dev/null; then
      echo "FAIL: $doc must explicitly document removed duplicate field $key" >&2
      exit 1
    fi
  done

  for key in crash cpu_usage_percent load_average process_memory_kb; do
    if ! rg -n -- "- \`$key\`" "$doc" >/dev/null; then
      echo "FAIL: $doc must list /api/resource field $key" >&2
      exit 1
    fi
  done
done
