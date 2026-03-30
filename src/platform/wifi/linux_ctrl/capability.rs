//! Linux WiFi 能力探测：sysfs + nl80211 GET_WIPHY（替换旧版 `iw` 子进程调用）。
//! WiFi capability detection: sysfs + nl80211 GET_WIPHY (replaces `iw` subprocess calls).

use crate::error::Result;
use std::path::Path;

use super::nl80211;

/// nl80211 能力摘要（用于启动前失败快、降级策略）。
#[derive(Clone, Debug)]
pub struct PhyCapabilities {
    /// 驱动报告支持 AP 模式。
    pub supports_ap: bool,
    /// `valid interface combinations` 中同 phy 可同时存在 managed+AP（启发式）。
    pub supports_sta_ap_concurrent: bool,
    pub has_2ghz: bool,
    pub has_5ghz: bool,
}

/// 探测第一个可用的无线接口名（`wlan0` / `wlan1`）。
/// 基于 sysfs，无子进程依赖。
pub fn detect_wifi_iface() -> Result<String> {
    nl80211::detect_wifi_iface()
}

/// 探测接口对应 phy 的能力（AP 支持、STA+AP 并发、频段）。
/// 使用 GENL nl80211 GET_WIPHY；无子进程依赖。
pub fn probe_phy(iface: &str) -> Result<PhyCapabilities> {
    nl80211::probe_phy_capabilities(iface)
}

/// 接口是否存在（sysfs）。
#[allow(dead_code)]
pub fn iface_exists(name: &str) -> bool {
    Path::new("/sys/class/net").join(name).exists()
}
