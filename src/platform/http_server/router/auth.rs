//! 配对 / CSRF 检查，替代仅 ESP 宏可用的逻辑。
//! Pairing and CSRF checks (replaces macros that need Esp request types).

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
use crate::memory::MemorySystemKind;
use crate::platform::csrf;
use crate::platform::http_server::common::{self, ApiResponse};
#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
use crate::platform::http_server::router::catalog::{
    HttpRouteSpec, OperatorRouteAccess, RouteMethod, ROUTE_CAPABILITY_PACKAGES,
    ROUTE_CHANNEL_CONNECTIVITY, ROUTE_CHANNEL_CONNECTIVITY_REFRESH, ROUTE_CONFIG_AUDIO,
    ROUTE_CONFIG_CHANNELS, ROUTE_CONFIG_DISPLAY, ROUTE_CONFIG_HARDWARE, ROUTE_CONFIG_LLM,
    ROUTE_CONFIG_RESET, ROUTE_CONFIG_SYSTEM, ROUTE_DIAGNOSE, ROUTE_HARDWARE_DISCOVERY,
    ROUTE_MEMORY_MAINTENANCE, ROUTE_MEMORY_STATUS, ROUTE_METRICS, ROUTE_OPERATOR_STATUS,
    ROUTE_OPERATOR_WINDOW, ROUTE_RESOURCE, ROUTE_RESTART, ROUTE_SESSIONS, ROUTE_SKILLS,
    ROUTE_SKILLS_IMPORT, ROUTE_SYSTEM_INFO, ROUTE_TOOLS,
};
use crate::platform::pairing;
use crate::platform::ConfigStore;

/// Shared operator-window error body used before expensive route-worker admission.
pub(crate) fn operator_window_required_response(path: &str) -> ApiResponse {
    let mut extra = serde_json::Map::new();
    extra.insert("path".to_string(), serde_json::json!(path));
    extra.insert(
        "open_endpoint".to_string(),
        serde_json::json!("POST /api/operator/window"),
    );
    ApiResponse::err_key_with_meta(
        403,
        "Forbidden",
        "system.operator_window_required",
        None,
        None,
        None,
        None,
        extra,
    )
}

/// Guard protected worker routes before heap admission can disclose worker state.
#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) fn worker_route_pre_admission_response(
    store: &dyn ConfigStore,
    memory_system_kind: MemorySystemKind,
    spec: HttpRouteSpec,
    uri: &str,
    headers: &[(String, String)],
) -> Option<ApiResponse> {
    if spec.operator_access == OperatorRouteAccess::Windowed
        && crate::platform::operator_surface::current_operator_surface_budget(memory_system_kind)
            .window_required_for_deep_routes
    {
        return Some(operator_window_required_response(spec.path));
    }
    route_auth_response(store, spec.method, spec.path, uri, headers)
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
fn route_auth_response(
    store: &dyn ConfigStore,
    method: RouteMethod,
    path: &str,
    uri: &str,
    headers: &[(String, String)],
) -> Option<ApiResponse> {
    match (method, path) {
        (RouteMethod::Post, ROUTE_CONFIG_LLM)
        | (RouteMethod::Post, ROUTE_CONFIG_CHANNELS)
        | (RouteMethod::Post, ROUTE_CONFIG_SYSTEM)
        | (RouteMethod::Post, ROUTE_CONFIG_HARDWARE)
        | (RouteMethod::Post, ROUTE_CONFIG_AUDIO)
        | (RouteMethod::Post, ROUTE_CONFIG_DISPLAY)
        | (RouteMethod::Post, ROUTE_CHANNEL_CONNECTIVITY_REFRESH)
        | (RouteMethod::Delete, ROUTE_SESSIONS)
        | (RouteMethod::Post, ROUTE_MEMORY_MAINTENANCE)
        | (RouteMethod::Post, ROUTE_CAPABILITY_PACKAGES)
        | (RouteMethod::Post, ROUTE_SKILLS)
        | (RouteMethod::Delete, ROUTE_SKILLS)
        | (RouteMethod::Post, ROUTE_SKILLS_IMPORT)
        | (RouteMethod::Post, ROUTE_RESTART)
        | (RouteMethod::Post, ROUTE_CONFIG_RESET) => require_pairing_csrf(store, uri, headers),
        (RouteMethod::Get, ROUTE_HARDWARE_DISCOVERY) => require_pairing_code(store, uri, headers),
        (RouteMethod::Get, ROUTE_OPERATOR_STATUS)
        | (RouteMethod::Get, ROUTE_METRICS)
        | (RouteMethod::Get, ROUTE_RESOURCE)
        | (RouteMethod::Get, ROUTE_TOOLS)
        | (RouteMethod::Get, ROUTE_DIAGNOSE)
        | (RouteMethod::Get, ROUTE_SYSTEM_INFO)
        | (RouteMethod::Get, ROUTE_CHANNEL_CONNECTIVITY)
        | (RouteMethod::Get, ROUTE_SESSIONS)
        | (RouteMethod::Get, ROUTE_MEMORY_STATUS)
        | (RouteMethod::Get, ROUTE_CAPABILITY_PACKAGES)
        | (RouteMethod::Get, ROUTE_SKILLS) => require_activated(store),
        (RouteMethod::Post, ROUTE_OPERATOR_WINDOW) => {
            require_activated(store).or_else(|| require_pairing_csrf(store, uri, headers))
        }
        _ => None,
    }
}

/// 未激活则返回 401 JSON（与 `require_activated!` 一致）。
pub fn require_activated(store: &dyn ConfigStore) -> Option<ApiResponse> {
    if !pairing::code_set(store) {
        return Some(ApiResponse::err_401_key("auth.pairing_required"));
    }
    None
}

/// 配对码鉴权：已激活且本次请求带配对码（header / query，与 `guard_pairing_csrf` 一致）。
/// 用于所有敏感配置读写接口；写操作额外叠加 CSRF。
pub fn require_pairing_code(
    store: &dyn ConfigStore,
    uri: &str,
    headers: &[(String, String)],
) -> Option<ApiResponse> {
    let stored_code = match pairing::get_valid_code(store) {
        Some(code) => code,
        None => return Some(ApiResponse::err_401_key("auth.pairing_required")),
    };
    let code = common::code_from_uri(uri)
        .map(String::from)
        .or_else(|| header_ci(headers, "X-Pairing-Code").map(String::from));
    match code.as_deref() {
        Some(candidate) if !candidate.is_empty() => {
            if !pairing::verify_code_value(&stored_code, candidate) {
                return Some(ApiResponse::err_401_key("auth.pairing_invalid"));
            }
        }
        _ => {
            return Some(ApiResponse::err_401_key("auth.pairing_invalid"));
        }
    }
    None
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
fn require_pairing_csrf(
    store: &dyn ConfigStore,
    uri: &str,
    headers: &[(String, String)],
) -> Option<ApiResponse> {
    if let Some(r) = require_pairing_code(store, uri, headers) {
        return Some(r);
    }
    require_csrf(store, headers)
}

fn header_ci<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// CSRF（与 `require_csrf!` 一致）。
pub fn require_csrf(_store: &dyn ConfigStore, headers: &[(String, String)]) -> Option<ApiResponse> {
    let token = header_ci(headers, "X-CSRF-Token").or_else(|| header_ci(headers, "x-csrf-token"));
    match token {
        Some(t) if csrf::verify_token(t) => None,
        Some(_) => Some(ApiResponse::err_403_key("auth.csrf_invalid")),
        None => Some(ApiResponse::err_403_key("auth.csrf_required")),
    }
}

#[cfg(test)]
mod tests {
    use super::{require_pairing_code, worker_route_pre_admission_response};
    use crate::error::Result;
    use crate::memory::MemorySystemKind;
    use crate::platform::http_server::router::catalog;
    use crate::platform::ConfigStore;
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingStore {
        reads: AtomicUsize,
    }

    impl ConfigStore for CountingStore {
        fn read_string(&self, key: &str) -> Result<Option<String>> {
            assert_eq!(key, "pairing_code");
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(Some("123456".to_string()))
        }

        fn write_string(&self, _key: &str, _value: &str) -> Result<()> {
            Ok(())
        }

        fn erase_keys(&self, _keys: &[&str]) -> Result<()> {
            Ok(())
        }
    }

    struct MissingPairingStore;

    impl ConfigStore for MissingPairingStore {
        fn read_string(&self, key: &str) -> Result<Option<String>> {
            assert_eq!(key, "pairing_code");
            Ok(None)
        }

        fn write_string(&self, _key: &str, _value: &str) -> Result<()> {
            Ok(())
        }

        fn erase_keys(&self, _keys: &[&str]) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn require_pairing_code_reads_pairing_code_once_on_valid_request() {
        let store = CountingStore {
            reads: AtomicUsize::new(0),
        };
        let headers = [("X-Pairing-Code".to_string(), "123456".to_string())];

        assert!(require_pairing_code(&store, "/api/config/system", &headers).is_none());
        assert_eq!(store.reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn worker_pre_admission_authenticates_protected_routes_before_executor_admission() {
        let store = MissingPairingStore;
        let spec =
            catalog::route_spec_for("POST", catalog::ROUTE_CONFIG_SYSTEM).expect("route spec");

        let response = worker_route_pre_admission_response(
            &store,
            MemorySystemKind::LinuxFull,
            spec,
            catalog::ROUTE_CONFIG_SYSTEM,
            &[],
        )
        .expect("auth response");

        assert_eq!(response.status, 401);
        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["error_key"], "auth.pairing_required");
        assert!(
            !String::from_utf8_lossy(&response.body).contains("internal_free"),
            "auth response must not leak route worker heap detail"
        );
    }
}
