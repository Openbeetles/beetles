//! WiFi 扫描（无 wpa_supplicant 时的路径）。
//! 使用 nl80211 TRIGGER_SCAN + GET_SCAN DUMP（已替换旧版 `iw dev <iface> scan` 子进程调用）。
//!
//! # AP-only 驱动限制
//! 部分驱动在 SoftAP 同口上 `TRIGGER_SCAN` 可能返回 EOPNOTSUPP/EBUSY（stage `wifi_scan`）；
//! 调用方应原样上报错误，不静默降级。

use crate::error::{Error, Result};
use std::time::Instant;

/// 在 `deadline` 前完成一次 nl80211 扫描（与 `wpa::scan_bounded` 墙钟语义对齐）。
pub fn scan_bounded(iface: &str, deadline: Instant) -> Result<Vec<crate::platform::WifiApEntry>> {
    if deadline.saturating_duration_since(Instant::now()).is_zero() {
        return Err(Error::config("wifi_scan", "scan timeout"));
    }
    super::nl80211::scan_bounded(iface, deadline)
}
