//! Linux network operations for AP/STA（数据面均经 rtnetlink，无 `ip` 命令依赖）。
//! Linux network operations for AP/STA (data plane via rtnetlink, no `ip` dependency).

use super::process::run_checked;
use crate::error::{Error, Result};
use std::path::Path;
use std::time::{Duration, Instant};

pub fn setup_ap_address(iface: &str, cidr: &str) -> Result<()> {
    super::net_rt::setup_ap_address(iface, cidr)
}

pub fn read_sta_ip(iface: &str) -> Result<Option<String>> {
    super::net_rt::read_sta_ip(iface)
}

pub fn ensure_root_or_cap_net_admin() -> Result<()> {
    super::net_rt::ensure_netlink_access()
}

/// Flush all IPv4 addresses from an interface.
/// 清空接口上的全部 IPv4 地址（用于把旧 SoftAP 地址从物理 STA 口移除）。
pub fn clear_ipv4_addresses(iface: &str) -> Result<()> {
    super::net_rt::clear_ipv4_addresses(iface)
}

/// Create a virtual AP interface on the same phy as `phy_iface`.
/// `iw dev <phy_iface> interface add <ap_iface> type __ap`
/// 在 `phy_iface` 同 phy 上创建虚拟 AP 接口，供 hostapd 使用，物理接口留给 wpa_supplicant。
pub fn create_virtual_ap_iface(phy_iface: &str, ap_iface: &str) -> Result<()> {
    let _ = delete_virtual_iface(ap_iface);
    run_checked(
        "iw",
        &[
            "dev",
            phy_iface,
            "interface",
            "add",
            ap_iface,
            "type",
            "__ap",
        ],
        Duration::from_secs(5),
        "wifi_virt_iface_add",
    )?;
    wait_iface_sysfs_ready(ap_iface)?;
    Ok(())
}

/// Read current WiFi channel index from `iw dev <iface> info`.
/// 从 `iw dev <iface> info` 读取当前信道号；无信道信息时返回 `Ok(None)`。
pub fn read_wifi_channel(iface: &str) -> Result<Option<u8>> {
    let out = run_checked(
        "iw",
        &["dev", iface, "info"],
        Duration::from_secs(3),
        "wifi_channel_read",
    )?;
    for line in out.stdout.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("channel ") {
            let token = rest.split_whitespace().next().unwrap_or_default();
            if let Ok(ch) = token.parse::<u16>() {
                if (1..=255).contains(&ch) {
                    return Ok(Some(ch as u8));
                }
            }
        }
    }
    Ok(None)
}

/// After `iw interface add`, sysfs may appear before rtnetlink is consistent; wait briefly.
/// `iw` 创建接口后 sysfs 与 netlink 可能短暂不一致，轮询 sysfs 并小睡再交给 rtnetlink。
fn wait_iface_sysfs_ready(name: &str) -> Result<()> {
    let path = Path::new("/sys/class/net").join(name);
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if path.exists() {
            std::thread::sleep(Duration::from_millis(150));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err(Error::config(
        "wifi_virt_iface_add",
        format!("timeout waiting for sysfs {}", path.display()),
    ))
}

/// Delete a virtual interface. Best-effort; ignores errors (interface may not exist).
/// 删除虚拟接口，忽略错误（接口可能不存在）。
pub fn delete_virtual_iface(ap_iface: &str) -> Result<()> {
    run_checked(
        "iw",
        &["dev", ap_iface, "del"],
        Duration::from_secs(3),
        "wifi_virt_iface_del",
    )?;
    Ok(())
}
