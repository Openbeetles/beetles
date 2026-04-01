//! GET /api/health：仅生成响应体 JSON，配对与写响应在 mod.rs。含 metrics 与 orchestrator resource 快照。
//! Health JSON includes metrics and orchestrator [`crate::orchestrator::ResourceSnapshot`] for UI/ops.

use super::HandlerContext;
use crate::metrics;
use crate::orchestrator;
use crate::runtime;
use crate::state;
use std::sync::atomic::Ordering;

#[derive(serde::Serialize)]
struct DisplayHealth {
    available: bool,
}

#[derive(serde::Serialize)]
struct HealthBody {
    wifi: &'static str,
    inbound_depth: usize,
    outbound_depth: usize,
    last_error: String,
    display: DisplayHealth,
    metrics: metrics::MetricsSnapshot,
    resource: orchestrator::ResourceSnapshot,
    threads: runtime::ThreadRegistrySnapshot,
}

/// 生成 health JSON body（含 metrics 与 resource 快照，无敏感信息）。
pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let wifi = if crate::platform::is_wifi_sta_connected() {
        "connected"
    } else {
        "disconnected"
    };
    let last_err = state::get_last_error().unwrap_or_else(|| "none".to_string());
    let payload = HealthBody {
        wifi,
        inbound_depth: ctx.inbound_depth.load(Ordering::Relaxed),
        outbound_depth: ctx.outbound_depth.load(Ordering::Relaxed),
        last_error: last_err,
        display: DisplayHealth {
            available: ctx.platform.display_available(),
        },
        metrics: metrics::snapshot(),
        resource: orchestrator::snapshot(),
        threads: runtime::thread_registry::snapshot(),
    };
    serde_json::to_string(&payload).map_err(std::io::Error::other)
}
