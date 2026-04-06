//! 直连 hostapd `ctrl_interface` Unix 套接字（`HOSTAPD_CTRL_INTERFACE_DIR` + `<iface>`，与 `hostapd.conf` 一致）。
//! Direct hostapd `ctrl_interface` Unix socket (`HOSTAPD_CTRL_INTERFACE_DIR` + `<iface>`, matches `hostapd.conf`).

use crate::error::Result;
use std::path::PathBuf;
use std::time::Duration;

use super::ctrl_iface;
use super::HOSTAPD_CTRL_INTERFACE_DIR;

pub fn request(iface: &str, cmd: &str, timeout: Duration, stage: &'static str) -> Result<String> {
    let path = PathBuf::from(HOSTAPD_CTRL_INTERFACE_DIR).join(iface);
    ctrl_iface::request_unix(&path, cmd, timeout, stage)
}

/// 优雅退出（等价 `hostapd_cli terminate`），失败时调用方可继续 `kill`。
/// Graceful shutdown (same as `hostapd_cli terminate`); caller may fall back to `kill` on error.
pub fn try_terminate(iface: &str, timeout: Duration) {
    let _ = request(iface, "TERMINATE", timeout, "wifi_ap_stop");
}
