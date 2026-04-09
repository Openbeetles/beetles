//! POST /api/operator/window: open a temporary ESP deep-inspection window.

use super::HandlerContext;

pub fn post(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let Some(window) =
        crate::platform::operator_surface::open_operator_window(ctx.platform.memory_system_kind())
    else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "operator window is only available on embedded surface",
        ));
    };
    let payload = serde_json::json!({
        "opened": true,
        "operator_window": window,
        "windowed_endpoints": crate::platform::operator_surface::control_plane_inventory(
            ctx.platform.memory_system_kind(),
            true,
            cfg!(feature = "ota"),
            ctx.route_contract.inbound_webhooks_enabled,
        )
        .windowed_endpoints,
    });
    serde_json::to_string(&payload).map_err(std::io::Error::other)
}
