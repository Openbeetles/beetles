//! GET /api/operator/status: unified operator-facing status contract.

use super::HandlerContext;
use crate::platform::operator_status::{OperatorStatusInput, build_operator_status};
use std::sync::atomic::Ordering;

pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let snapshot = build_operator_status(OperatorStatusInput {
        platform: ctx.platform.as_ref(),
        tool_registry: ctx.tool_registry.as_ref(),
        inbound_depth: ctx.inbound_depth.load(Ordering::Relaxed),
        outbound_depth: ctx.outbound_depth.load(Ordering::Relaxed),
        version: ctx.version.as_ref(),
        board_id: ctx.board_id.as_ref(),
    })
    .map_err(std::io::Error::other)?;
    serde_json::to_string(&snapshot).map_err(std::io::Error::other)
}
