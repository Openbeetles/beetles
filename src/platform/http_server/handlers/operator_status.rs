//! GET /api/operator/status: unified operator-facing status contract.

use super::HandlerContext;
use crate::platform::operator_status::{build_operator_status, OperatorStatusInput};
use std::sync::atomic::Ordering;

pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let config = ctx.config();
    let current_channel = config.enabled_channel.clone();
    drop(config);
    let snapshot = build_operator_status(OperatorStatusInput {
        platform: ctx.platform.as_ref(),
        tool_registry: ctx.tool_registry.as_ref(),
        channel_capability_registry: ctx.channel_capability_registry.as_ref(),
        capability_package_runtime_capabilities: ctx
            .capability_package_runtime_capabilities
            .as_ref(),
        current_channel: current_channel.as_str(),
        inbound_depth: ctx.inbound_depth.load(Ordering::Relaxed),
        outbound_depth: ctx.outbound_depth.load(Ordering::Relaxed),
        version: ctx.version.as_ref(),
        board_id: ctx.board_id.as_ref(),
        llm_stream_enabled: ctx.llm_stream_enabled,
    })
    .map_err(std::io::Error::other)?;
    serde_json::to_string(&snapshot).map_err(std::io::Error::other)
}
