//! GET /：始终返回 operator-facing API 信息 JSON。

use super::HandlerContext;

pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let inventory = crate::platform::operator_surface::control_plane_inventory(
        ctx.platform.memory_system_kind(),
        crate::state::esp_operator_window_active(),
        ctx.route_contract.inbound_webhooks_enabled,
    );
    let crate::platform::operator_surface::ControlPlaneInventory {
        endpoints,
        windowed_endpoints,
        operator_window,
    } = inventory;
    let mut payload = serde_json::json!({
        "name": "beetle",
        "version": ctx.version.as_ref(),
        "endpoints": endpoints,
    });
    if let Some(obj) = payload.as_object_mut() {
        if !windowed_endpoints.is_empty() {
            obj.insert(
                "windowed_endpoints".to_string(),
                serde_json::json!(windowed_endpoints),
            );
        }
        if let Some(window) = operator_window {
            obj.insert("operator_window".to_string(), serde_json::json!(window));
        }
    }
    serde_json::to_string(&payload).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use crate::memory::MemorySystemKind;

    #[test]
    fn embedded_root_inventory_separates_minimal_and_windowed_routes() {
        let inventory = crate::platform::operator_surface::control_plane_inventory(
            MemorySystemKind::EspCompact,
            false,
            true,
        );

        assert!(inventory
            .endpoints
            .iter()
            .any(|item| item == "GET /api/health"));
        assert!(inventory
            .endpoints
            .iter()
            .any(|item| item == "POST /api/operator/window"));
        assert!(!inventory
            .endpoints
            .iter()
            .any(|item| item == "GET /api/memory/status"));
        assert!(inventory
            .windowed_endpoints
            .iter()
            .any(|item| item == "GET /api/memory/status"));
    }

    #[test]
    fn root_inventory_no_longer_advertises_removed_bootstrap_pages() {
        let inventory = crate::platform::operator_surface::control_plane_inventory(
            MemorySystemKind::EspCompact,
            false,
            true,
        );

        assert!(!inventory
            .endpoints
            .iter()
            .any(|item| item == "GET /pairing"));
        assert!(!inventory.endpoints.iter().any(|item| item == "GET /wifi"));
    }

    #[test]
    fn root_inventory_no_longer_advertises_removed_narrow_wifi_config_route() {
        let inventory = crate::platform::operator_surface::control_plane_inventory(
            MemorySystemKind::EspCompact,
            false,
            true,
        );

        assert!(!inventory
            .endpoints
            .iter()
            .any(|item| item == "POST /api/config/wifi"));
    }
}
