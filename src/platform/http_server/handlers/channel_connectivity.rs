//! GET /api/channel_connectivity?channel=...：按指定通道执行显式连通性探测。

use super::HandlerContext;
use crate::i18n::locale_from_store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelConnectivityError {
    /// Requested channel is unknown or not compiled into this build.
    InvalidChannel,
    /// ESP runtime resource state currently blocks a live outbound probe.
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    LiveProbeUnavailable(&'static str),
    /// Unexpected infrastructure or serialization failure.
    Internal(String),
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
fn live_probe_block_reason(
    wifi_settled: bool,
    fragmentation_risk: crate::orchestrator::TlsFragmentationRisk,
) -> Option<&'static str> {
    if !wifi_settled {
        return Some("wifi_unsettled");
    }
    if fragmentation_risk.blocks_live_probe() {
        return Some("tls_fragmentation");
    }
    None
}

/// 成功返回单通道 probe JSON；失败由 router 映射为稳定 error_key。
pub fn body(ctx: &HandlerContext, channel_id: &str) -> Result<String, ChannelConnectivityError> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let config = ctx.config().clone();
    if !crate::channels::channel_supports_connectivity(channel_id) {
        return Err(ChannelConnectivityError::InvalidChannel);
    }
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let wifi_settled = crate::state::wifi_sta_settled_for_outbound(3);
        let fragmentation_risk = crate::orchestrator::current_tls_fragmentation_risk();
        if let Some(reason) = live_probe_block_reason(wifi_settled, fragmentation_risk) {
            log::info!(
                "[channel_connectivity] live probe unavailable channel={} reason={} wifi_settled={} tls_fragmentation={:?}",
                channel_id,
                reason,
                wifi_settled,
                fragmentation_risk,
            );
            return Err(ChannelConnectivityError::LiveProbeUnavailable(reason));
        }
    }
    let mut http = crate::network::create_http_client_with_config(
        ctx.platform.as_ref(),
        &config,
        crate::network::HttpClientClass::Background,
    )
    .map_err(|e| ChannelConnectivityError::Internal(e.to_string()))?;
    let response = crate::channels::build_channel_probe(&config, http.as_mut(), loc, channel_id)
        .ok_or(ChannelConnectivityError::InvalidChannel)?;
    serde_json::to_string(&response).map_err(|e| ChannelConnectivityError::Internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::live_probe_block_reason;
    use crate::orchestrator::TlsFragmentationRisk;

    #[test]
    fn live_probe_guard_reports_unavailable_reason() {
        assert_eq!(
            live_probe_block_reason(false, TlsFragmentationRisk::Healthy),
            Some("wifi_unsettled")
        );
        assert_eq!(
            live_probe_block_reason(true, TlsFragmentationRisk::Critical),
            Some("tls_fragmentation")
        );
        assert_eq!(
            live_probe_block_reason(true, TlsFragmentationRisk::Cautious),
            Some("tls_fragmentation")
        );
        assert_eq!(
            live_probe_block_reason(true, TlsFragmentationRisk::Healthy),
            None
        );
    }
}
