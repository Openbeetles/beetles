//! ESP operator surface budgeting and window policy.

use crate::memory::MemorySystemKind;
use serde::Serialize;

const ESP_OPERATOR_WINDOW_TTL_SECS: u64 = 300;

const ESP_ALWAYS_ON_ENDPOINTS: &[&str] = &[
    "GET /pairing",
    "GET /wifi",
    "GET /api/pairing_code",
    "POST /api/pairing_code",
    "GET /api/csrf_token",
    "GET /api/config",
    "POST /api/config/wifi",
    "POST /api/config/llm",
    "POST /api/config/channels",
    "POST /api/config/system",
    "GET /api/config/hardware",
    "POST /api/config/hardware",
    "GET /api/config/audio",
    "POST /api/config/audio",
    "GET /api/config/display",
    "POST /api/config/display",
    "GET /api/wifi/scan",
    "GET /api/hardware/discovery",
    "GET /api/health",
    "GET /api/operator/status",
    "POST /api/operator/window",
    "GET /api/diagnose",
    "GET /api/system_info",
    "POST /api/restart",
    "POST /api/config_reset",
];

const ESP_WINDOWED_ENDPOINTS: &[&str] = &[
    "GET /api/metrics",
    "GET /api/resource",
    "GET /api/tools",
    "GET /api/channel_connectivity",
    "GET /api/sessions",
    "DELETE /api/sessions",
    "GET /api/memory/status",
    "GET /api/capability_packages",
    "POST /api/capability_packages",
    "GET /api/skills",
    "POST /api/skills",
    "DELETE /api/skills",
    "POST /api/skills/import",
    "GET /api/soul",
    "POST /api/soul",
    "GET /api/user",
    "POST /api/user",
];

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
    pub endpoints: Vec<&'static str>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windowed_endpoints: Vec<&'static str>,
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
    window_active: bool,
    ota_supported: bool,
) -> ControlPlaneInventory {
    if !is_embedded_surface(memory_system_kind) {
        let mut endpoints = vec![
            "GET /pairing",
            "GET /wifi",
            "GET /api/pairing_code",
            "POST /api/pairing_code",
            "GET /api/config",
            "POST /api/config/llm",
            "POST /api/config/channels",
            "POST /api/config/system",
            "GET /api/config/hardware",
            "POST /api/config/hardware",
            "GET /api/config/audio",
            "POST /api/config/audio",
            "GET /api/hardware/discovery",
            "GET /api/wifi/scan",
            "GET /api/health",
            "GET /api/operator/status",
            "GET /api/diagnose",
            "GET /api/system_info",
            "GET /api/channel_connectivity",
            "GET /api/tools",
            "GET /api/soul",
            "GET /api/user",
            "POST /api/soul",
            "POST /api/user",
            "GET /api/sessions",
            "GET /api/memory/status",
            "GET /api/capability_packages",
            "POST /api/capability_packages",
            "GET /api/skills",
            "POST /api/skills",
            "DELETE /api/skills",
            "POST /api/skills/import",
            "POST /api/restart",
            "POST /api/config_reset",
            "POST /api/webhook",
        ];
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        {
            endpoints.push("POST /api/feishu/event");
            endpoints.push("POST /api/dingtalk/webhook");
            endpoints.push("GET /api/wecom/webhook");
            endpoints.push("POST /api/wecom/webhook");
            endpoints.push("POST /api/webhook/qq");
        }
        if ota_supported {
            endpoints.push("GET /api/ota/check");
            endpoints.push("POST /api/ota");
        }
        return ControlPlaneInventory {
            endpoints,
            windowed_endpoints: Vec::new(),
            operator_window: None,
        };
    }

    let mut endpoints = ESP_ALWAYS_ON_ENDPOINTS.to_vec();
    if ota_supported {
        endpoints.push("GET /api/ota/check");
        endpoints.push("POST /api/ota");
    }
    let mut windowed_endpoints = ESP_WINDOWED_ENDPOINTS.to_vec();
    if ota_supported {
        windowed_endpoints.push("GET /api/ota/check");
        windowed_endpoints.push("POST /api/ota");
    }

    ControlPlaneInventory {
        endpoints,
        windowed_endpoints,
        operator_window: Some(operator_window_snapshot_from_active(window_active)),
    }
}

pub fn route_requires_operator_window(memory_system_kind: MemorySystemKind, path: &str) -> bool {
    is_embedded_surface(memory_system_kind)
        && matches!(
            path,
            "/api/metrics"
                | "/api/resource"
                | "/api/tools"
                | "/api/channel_connectivity"
                | "/api/sessions"
                | "/api/memory/status"
                | "/api/capability_packages"
                | "/api/skills"
                | "/api/skills/import"
                | "/api/soul"
                | "/api/user"
        )
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
