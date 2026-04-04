//! 直连 `wpa_supplicant` ctrl_interface Unix 套接字（与 `wpa_cli` 相同文本协议：`COMMAND\\n`）。
//! Direct `wpa_supplicant` ctrl_interface Unix socket (same text protocol as `wpa_cli`).

use crate::error::Result;
use std::path::PathBuf;
use std::time::Duration;

use super::ctrl_iface;
use super::WPA_CTRL_INTERFACE_DIR;

/// 发送一条控制命令并读取直到 `OK`/`FAIL`/`PONG`（与 wpa_cli 行为对齐）。
/// Send one ctrl command and read until `OK`/`FAIL`/`PONG` (aligned with wpa_cli).
pub fn request(iface: &str, cmd: &str, timeout: Duration, stage: &'static str) -> Result<String> {
    let path = PathBuf::from(WPA_CTRL_INTERFACE_DIR).join(iface);
    ctrl_iface::request_unix(&path, cmd, timeout, stage)
}
