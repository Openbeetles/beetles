//! Host/Linux：无 WiFi 栈时的桩。
//! Host stub without WiFi stack.

use crate::config::AppConfig;
use crate::error::{Error, Result};

const TAG: &str = "platform::wifi";

/// 其他线程查询 WiFi STA 是否就绪（已连接且有 IP）。
pub fn is_wifi_sta_connected() -> bool {
    crate::state::wifi_sta_connected()
}

/// 读取当前 STA IPv4（点分十进制），无可用地址时返回 None。
pub fn wifi_sta_ip() -> Option<String> {
    crate::state::wifi_sta_ip()
}

/// 阻塞直到出站网络就绪；host 立即返回。
pub fn wait_for_network_ready() {}

/// GET /api/wifi/scan 返回的单个 AP。
#[derive(Clone, Debug, serde::Serialize)]
pub struct WifiApEntry {
    pub ssid: String,
    pub rssi: i8,
}

/// 向设备请求一次 WiFi 扫描的 trait。
pub trait WifiScan: Send + Sync {
    fn request_scan(&self) -> Result<Vec<WifiApEntry>>;
}

/// 占位句柄；host 上 `connect` 仅返回 `None`，不会构造本类型。
#[derive(Clone)]
pub struct WifiScanHandle;

impl WifiScan for WifiScanHandle {
    fn request_scan(&self) -> Result<Vec<WifiApEntry>> {
        Err(Error::config(
            "wifi_scan",
            "WiFi not available on this target",
        ))
    }
}

/// 无 SoftAP/STA；返回 `Ok(None)`。
pub fn connect(_config: &AppConfig) -> Result<Option<WifiScanHandle>> {
    crate::state::clear_wifi_sta_state();
    log::info!("[{}] connect: no-op on host", TAG);
    Ok(None)
}
