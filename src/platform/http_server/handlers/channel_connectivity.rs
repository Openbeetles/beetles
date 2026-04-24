//! GET /api/channel_connectivity：默认返回不触发外网探测的 stale snapshot，供设备页展示。

use super::HandlerContext;
use crate::i18n::locale_from_store;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
fn should_use_stale_snapshot(
    wifi_settled: bool,
    fragmentation_risk: crate::orchestrator::TlsFragmentationRisk,
    config_active: bool,
) -> bool {
    config_active || !wifi_settled || fragmentation_risk.blocks_live_probe()
}

/// 成功返回 `{ "channels": [ ... ] }` 字符串，失败返回 Err（mod 层写 500，不暴露内部细节）。
pub fn body(ctx: &HandlerContext, allow_live_probe: bool) -> Result<String, String> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let config = ctx.config().clone();
    if !allow_live_probe {
        let snapshot = crate::channels::build_unavailable_snapshot(&config, loc);
        return serde_json::to_string(&snapshot).map_err(|e| e.to_string());
    }
    let config_active = crate::runtime::config_activity_active();
    if config_active {
        log::info!("[channel_connectivity] returning stale snapshot during config_active");
        let snapshot = crate::channels::build_unavailable_snapshot(&config, loc);
        return serde_json::to_string(&snapshot).map_err(|e| e.to_string());
    }
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let wifi_settled = crate::state::wifi_sta_settled_for_outbound(3);
        let fragmentation_risk = crate::orchestrator::current_tls_fragmentation_risk();
        if should_use_stale_snapshot(wifi_settled, fragmentation_risk, config_active) {
            log::info!(
                "[channel_connectivity] returning stale snapshot wifi_settled={} tls_fragmentation={:?}",
                wifi_settled,
                fragmentation_risk
            );
            let snapshot = crate::channels::build_unavailable_snapshot(&config, loc);
            return serde_json::to_string(&snapshot).map_err(|e| e.to_string());
        }
    }
    let mut http = crate::network::create_http_client_with_config(
        ctx.platform.as_ref(),
        &config,
        crate::network::HttpClientClass::Background,
    )
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
        assert!(should_use_stale_snapshot(
            false,
            TlsFragmentationRisk::Healthy,
            false
        ));
        assert!(should_use_stale_snapshot(
            true,
            TlsFragmentationRisk::Critical,
            false
        ));
        assert!(should_use_stale_snapshot(
            true,
            TlsFragmentationRisk::Cautious,
            false
        ));
        assert!(should_use_stale_snapshot(
            true,
            TlsFragmentationRisk::Healthy,
            true
        ));
        assert!(!should_use_stale_snapshot(
            true,
            TlsFragmentationRisk::Healthy,
            false
        ));
    }
}
