//! GET /api/channel_connectivity：按当前启用通道现场探测连通性，供设备页展示。

use super::HandlerContext;
use crate::i18n::locale_from_store;

fn should_use_stale_snapshot(
    wifi_settled: bool,
    fragmentation_risk: crate::orchestrator::TlsFragmentationRisk,
) -> bool {
    !wifi_settled || fragmentation_risk.blocks_live_probe()
}

/// 成功返回 `{ "channels": [ ... ] }` 字符串，失败返回 Err（mod 层写 500，不暴露内部细节）。
pub fn body(ctx: &HandlerContext) -> Result<String, String> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let config = ctx.config().clone();
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let wifi_settled = crate::state::wifi_sta_settled_for_outbound(3);
        let fragmentation_risk = crate::orchestrator::current_tls_fragmentation_risk();
        if should_use_stale_snapshot(wifi_settled, fragmentation_risk) {
            log::info!(
                "[channel_connectivity] returning stale snapshot wifi_settled={} tls_fragmentation={:?}",
                wifi_settled,
                fragmentation_risk
            );
            let snapshot = crate::channels::build_unavailable_snapshot(&config, loc);
            return serde_json::to_string(&snapshot).map_err(|e| e.to_string());
        }
    }
    let mut http = ctx
        .platform
        .create_http_client(&config)
        .map_err(|e| e.to_string())?;
    let snapshot = crate::channels::build_snapshot(&config, http.as_mut(), loc);
    serde_json::to_string(&snapshot).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::should_use_stale_snapshot;
    use crate::orchestrator::TlsFragmentationRisk;

    #[test]
    fn live_probe_guard_activates_for_unsettled_wifi_or_fragmentation() {
        assert!(should_use_stale_snapshot(false, TlsFragmentationRisk::Healthy));
        assert!(should_use_stale_snapshot(true, TlsFragmentationRisk::Critical));
        assert!(should_use_stale_snapshot(true, TlsFragmentationRisk::Cautious));
        assert!(!should_use_stale_snapshot(true, TlsFragmentationRisk::Healthy));
    }
}
