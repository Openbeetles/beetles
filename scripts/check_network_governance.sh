#!/usr/bin/env bash
# Network governance gate:
# - transport ownership must stay in src/network/mod.rs
# - no legacy runtime::stream_http / state external_wss bypasses
# - no public raw WSS connect re-exports outside approved low-level files
# - business domains must not call raw Platform HTTP-client primitives directly
# - transport admission/thread-role facades must stay inside low-level transport files

set -euo pipefail

if ! command -v rg >/dev/null 2>&1; then
  echo "check_network_governance: ripgrep (rg) is required" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

STATE_BYPASS='crate::state::(external_wss_|request_external_wss_|set_external_wss_)'
if rg -n "$STATE_BYPASS" src --glob '!src/network/mod.rs' >/dev/null; then
  echo "FAIL: external WSS state bypass detected outside src/network/mod.rs" >&2
  rg -n "$STATE_BYPASS" src --glob '!src/network/mod.rs' >&2
  exit 1
fi

RAW_HTTP_PRIMITIVE='create_(interactive_)?http_client\s*\('
if rg -n "$RAW_HTTP_PRIMITIVE" src \
  --glob '!src/network/mod.rs' \
  --glob '!src/platform/**' >/dev/null; then
  echo "FAIL: raw Platform HTTP client primitives must stay confined to src/network/mod.rs / src/platform/**" >&2
  rg -n "$RAW_HTTP_PRIMITIVE" src \
    --glob '!src/network/mod.rs' \
    --glob '!src/platform/**' >&2
  exit 1
fi

if rg -n 'runtime::stream_http|pub mod stream_http' src >/dev/null; then
  echo "FAIL: legacy runtime::stream_http surface must stay removed" >&2
  rg -n 'runtime::stream_http|pub mod stream_http' src >&2
  exit 1
fi

WSS_BYPASS='connect_wss(_with_headers_and_profile)?\s*\('
if rg -n "$WSS_BYPASS" src \
  --glob '!src/network/mod.rs' \
  --glob '!src/channels/wss_gateway/mod.rs' >/dev/null; then
  echo "FAIL: raw WSS connect bypass detected outside network governor / low-level gateway module" >&2
  rg -n "$WSS_BYPASS" src \
    --glob '!src/network/mod.rs' \
    --glob '!src/channels/wss_gateway/mod.rs' >&2
  exit 1
fi

if rg -n 'pub use .*connect_wss|pub use .*connect_wss_with_headers_and_profile' src/lib.rs src/channels/mod.rs >/dev/null; then
  echo "FAIL: raw WSS connect functions must not be publicly re-exported" >&2
  rg -n 'pub use .*connect_wss|pub use .*connect_wss_with_headers_and_profile' src/lib.rs src/channels/mod.rs >&2
  exit 1
fi

if rg -n 'orchestrator::permit::current_http_thread_role' src >/dev/null; then
  echo "FAIL: direct permit thread-role reads bypass unified network authority" >&2
  rg -n 'orchestrator::permit::current_http_thread_role' src >&2
  exit 1
fi

if ! rg -n 'config_persisting_suspend' src/channels/wss_gateway/loop.rs >/dev/null; then
  echo "FAIL: external WSS loop no longer distinguishes config-persisting suspend from generic mode gates" >&2
  exit 1
fi

if ! rg -n 'dingtalk_stream|wecom_aibot' src/channels/wss_gateway/loop.rs >/dev/null; then
  echo "FAIL: WSS lifecycle owners no longer cover all external WSS channel owners" >&2
  exit 1
fi

if ! rg -n 'record_queue_full|record_deferred_without_queue_full|record_disconnected_drop' src/channels >/dev/null; then
  echo "FAIL: channel inbound backpressure outcomes are no longer recorded" >&2
  exit 1
fi

TRANSPORT_FACADE='crate::orchestrator::(request_http_permit|current_http_thread_role|set_current_http_thread_role|begin_wss_session)\s*\('
if rg -n "$TRANSPORT_FACADE" src \
  --glob '!src/platform/http_client/**' \
  --glob '!src/channels/wss_gateway/*_conn.rs' \
  --glob '!src/util.rs' >/dev/null; then
  echo "FAIL: transport admission/thread-role facade escaped approved low-level owners" >&2
  rg -n "$TRANSPORT_FACADE" src \
    --glob '!src/platform/http_client/**' \
    --glob '!src/channels/wss_gateway/*_conn.rs' \
    --glob '!src/util.rs' >&2
  exit 1
fi

echo "OK: network governance checks passed"
exit 0
