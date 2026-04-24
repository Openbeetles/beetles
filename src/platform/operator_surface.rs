//! ESP operator surface budgeting and window policy.

use crate::memory::MemorySystemKind;
#[cfg(all(
    feature = "ota",
    any(test, target_arch = "xtensa", target_arch = "riscv32")
))]
use crate::platform::http_server::router::catalog::OTA_ROUTE_SPECS;
#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
use crate::platform::http_server::router::catalog::{
    HttpRouteSpec, OperatorRouteAccess, ACTION_ROUTE_SPECS, MEMORY_AND_SKILL_ROUTE_SPECS,
    OBSERVABILITY_ROUTE_SPECS, PAIRING_AND_CONFIG_ROUTE_SPECS, ROOT_ROUTE_SPECS,
};
use serde::Serialize;

const ESP_OPERATOR_WINDOW_TTL_SECS: u64 = 300;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct OperatorWindowSnapshot {
    pub active: bool,
    pub ttl_secs: u64,
    pub remaining_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct OperatorSurfaceBudget {
    pub compact_view: bool,
    pub window_required_for_deep_routes: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_window: Option<OperatorWindowSnapshot>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ControlPlaneInventory {
    pub endpoints: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windowed_endpoints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_window: Option<OperatorWindowSnapshot>,
}

pub fn is_embedded_surface(memory_system_kind: MemorySystemKind) -> bool {
    matches!(memory_system_kind, MemorySystemKind::EspCompact)
}

pub fn operator_surface_budget(
    memory_system_kind: MemorySystemKind,
    window_active: bool,
) -> OperatorSurfaceBudget {
    let embedded = is_embedded_surface(memory_system_kind);
    OperatorSurfaceBudget {
        compact_view: embedded && !window_active,
        window_required_for_deep_routes: embedded && !window_active,
        operator_window: None,
    }
}

pub fn current_operator_surface_budget(
    memory_system_kind: MemorySystemKind,
) -> OperatorSurfaceBudget {
    let window = operator_window_snapshot(memory_system_kind);
    let window_active = window.as_ref().is_some_and(|item| item.active);
    let mut budget = operator_surface_budget(memory_system_kind, window_active);
    budget.operator_window = window;
    budget
}

pub fn control_plane_inventory(
    memory_system_kind: MemorySystemKind,
    _window_active: bool,
    _ota_supported: bool,
    inbound_webhooks_enabled: bool,
) -> ControlPlaneInventory {
    if !is_embedded_surface(memory_system_kind) {
        return host_control_plane_inventory(_ota_supported, inbound_webhooks_enabled);
    }

    #[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
    {
        ControlPlaneInventory {
            endpoints: embedded_control_plane_endpoints(
                OperatorRouteAccess::AlwaysOn,
                _ota_supported,
            ),
            windowed_endpoints: embedded_control_plane_endpoints(
                OperatorRouteAccess::Windowed,
                _ota_supported,
            ),
            operator_window: Some(operator_window_snapshot_from_active(_window_active)),
        }
    }

    #[cfg(not(any(test, target_arch = "xtensa", target_arch = "riscv32")))]
    {
        unreachable!("embedded operator inventory is only built on esp/test targets");
    }
}

pub fn windowed_control_plane_endpoints(
    memory_system_kind: MemorySystemKind,
    _ota_supported: bool,
) -> Vec<String> {
    if !is_embedded_surface(memory_system_kind) {
        return Vec::new();
    }

    #[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
    {
        embedded_control_plane_endpoints(OperatorRouteAccess::Windowed, _ota_supported)
    }

    #[cfg(not(any(test, target_arch = "xtensa", target_arch = "riscv32")))]
    {
        unreachable!("embedded operator endpoints are only built on esp/test targets");
    }
}

pub fn route_requires_operator_window(memory_system_kind: MemorySystemKind, _path: &str) -> bool {
    if !is_embedded_surface(memory_system_kind) {
        return false;
    }

    #[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
    {
        embedded_route_requires_access(_path, OperatorRouteAccess::Windowed)
    }

    #[cfg(not(any(test, target_arch = "xtensa", target_arch = "riscv32")))]
    {
        unreachable!("embedded operator gate is only checked on esp/test targets");
    }
}

pub fn open_operator_window(
    memory_system_kind: MemorySystemKind,
) -> Option<OperatorWindowSnapshot> {
    if !is_embedded_surface(memory_system_kind) {
        return None;
    }
    let expires_at = crate::state::open_esp_operator_window(ESP_OPERATOR_WINDOW_TTL_SECS);
    let now_secs = crate::util::current_unix_secs();
    Some(OperatorWindowSnapshot {
        active: true,
        ttl_secs: ESP_OPERATOR_WINDOW_TTL_SECS,
        remaining_secs: expires_at.saturating_sub(now_secs),
        expires_at: Some(expires_at),
    })
}

pub fn operator_window_snapshot(
    memory_system_kind: MemorySystemKind,
) -> Option<OperatorWindowSnapshot> {
    if !is_embedded_surface(memory_system_kind) {
        return None;
    }
    let active = crate::state::esp_operator_window_active();
    Some(operator_window_snapshot_from_active(active))
}

fn operator_window_snapshot_from_active(active: bool) -> OperatorWindowSnapshot {
    let now_secs = crate::util::current_unix_secs();
    let expires_at = crate::state::esp_operator_window_until();
    OperatorWindowSnapshot {
        active,
        ttl_secs: ESP_OPERATOR_WINDOW_TTL_SECS,
        remaining_secs: expires_at.unwrap_or_default().saturating_sub(now_secs),
        expires_at,
    }
}

fn host_control_plane_inventory(
    ota_supported: bool,
    inbound_webhooks_enabled: bool,
) -> ControlPlaneInventory {
    let mut endpoints = vec![
        "GET /api/pairing_code".to_string(),
        "POST /api/pairing_code".to_string(),
        "GET /api/config/system".to_string(),
        "POST /api/config/llm".to_string(),
        "POST /api/config/channels".to_string(),
        "POST /api/config/system".to_string(),
        "GET /api/config/hardware".to_string(),
        "POST /api/config/hardware".to_string(),
        "GET /api/config/audio".to_string(),
        "POST /api/config/audio".to_string(),
        "GET /api/hardware/discovery".to_string(),
        "GET /api/wifi/scan".to_string(),
        "GET /api/health".to_string(),
        "GET /api/operator/status".to_string(),
        "GET /api/diagnose".to_string(),
        "GET /api/system_info".to_string(),
        "GET /api/channel_connectivity".to_string(),
        "GET /api/tools".to_string(),
        "GET /api/sessions".to_string(),
        "GET /api/memory/status".to_string(),
        "POST /api/memory/maintenance".to_string(),
        "GET /api/capability_packages".to_string(),
        "POST /api/capability_packages".to_string(),
        "GET /api/skills".to_string(),
        "POST /api/skills".to_string(),
        "DELETE /api/skills".to_string(),
        "POST /api/skills/import".to_string(),
        "POST /api/restart".to_string(),
        "POST /api/config_reset".to_string(),
    ];
    if inbound_webhooks_enabled {
        endpoints.push("POST /api/webhook".to_string());
    }
    if ota_supported {
        endpoints.push("GET /api/ota/check".to_string());
        endpoints.push("POST /api/ota".to_string());
    }
    ControlPlaneInventory {
        endpoints,
        windowed_endpoints: Vec::new(),
        operator_window: None,
    }
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
fn embedded_control_plane_endpoints(
    access: OperatorRouteAccess,
    ota_supported: bool,
) -> Vec<String> {
    #[cfg(not(feature = "ota"))]
    let _ = ota_supported;

    let mut endpoints = Vec::new();
    push_embedded_control_plane_endpoints(&mut endpoints, ROOT_ROUTE_SPECS, access);
    push_embedded_control_plane_endpoints(&mut endpoints, PAIRING_AND_CONFIG_ROUTE_SPECS, access);
    push_embedded_control_plane_endpoints(&mut endpoints, OBSERVABILITY_ROUTE_SPECS, access);
    push_embedded_control_plane_endpoints(&mut endpoints, MEMORY_AND_SKILL_ROUTE_SPECS, access);
    push_embedded_control_plane_endpoints(&mut endpoints, ACTION_ROUTE_SPECS, access);
    #[cfg(feature = "ota")]
    if ota_supported {
        push_embedded_control_plane_endpoints(&mut endpoints, OTA_ROUTE_SPECS, access);
    }
    endpoints
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
fn push_embedded_control_plane_endpoints(
    endpoints: &mut Vec<String>,
    specs: &[HttpRouteSpec],
    access: OperatorRouteAccess,
) {
    for spec in specs {
        if spec.operator_access != access {
            continue;
        }
        endpoints.push(format!("{} {}", spec.method.as_str(), spec.path));
    }
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
fn embedded_route_requires_access(path: &str, access: OperatorRouteAccess) -> bool {
    route_group_requires_access(ROOT_ROUTE_SPECS, path, access)
        || route_group_requires_access(PAIRING_AND_CONFIG_ROUTE_SPECS, path, access)
        || route_group_requires_access(OBSERVABILITY_ROUTE_SPECS, path, access)
        || route_group_requires_access(MEMORY_AND_SKILL_ROUTE_SPECS, path, access)
        || route_group_requires_access(ACTION_ROUTE_SPECS, path, access)
        || {
            #[cfg(feature = "ota")]
            {
                route_group_requires_access(OTA_ROUTE_SPECS, path, access)
            }
            #[cfg(not(feature = "ota"))]
            {
                false
            }
        }
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
fn route_group_requires_access(
    specs: &[HttpRouteSpec],
    path: &str,
    access: OperatorRouteAccess,
) -> bool {
    specs
        .iter()
        .any(|spec| spec.path == path && spec.operator_access == access)
}

#[cfg(test)]
mod tests {
    use super::{
        control_plane_inventory, route_requires_operator_window, windowed_control_plane_endpoints,
    };
    use crate::memory::MemorySystemKind;

    #[test]
    fn embedded_inventory_separates_always_on_and_windowed_routes_from_catalog() {
        let inventory = control_plane_inventory(MemorySystemKind::EspCompact, false, true, true);

        assert!(inventory
            .endpoints
            .iter()
            .any(|item| item == "GET /api/config/llm"));
        assert!(inventory
            .endpoints
            .iter()
            .any(|item| item == "GET /api/config/channels"));
        assert!(inventory
            .endpoints
            .iter()
            .any(|item| item == "POST /api/operator/window"));
        assert!(!inventory
            .windowed_endpoints
            .iter()
            .any(|item| item == "GET /api/ota/check"));
    }

    #[test]
    fn embedded_window_policy_matches_windowed_inventory_paths() {
        for endpoint in windowed_control_plane_endpoints(MemorySystemKind::EspCompact, true) {
            let path = endpoint
                .split_once(' ')
                .map(|(_, path)| path)
                .expect("operator endpoint format");
            assert!(route_requires_operator_window(
                MemorySystemKind::EspCompact,
                path
            ));
        }
        assert!(!route_requires_operator_window(
            MemorySystemKind::EspCompact,
            "/api/ota"
        ));
    }
}
