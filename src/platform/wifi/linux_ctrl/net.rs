//! Linux 网络操作（AP/STA 数据面）。
//! 数据面地址管理经 rtnetlink；接口创建/删除与信道读取经 nl80211 GENL（不再依赖 `iw`）。

use crate::error::{Error, Result};
use std::path::Path;
use std::time::{Duration, Instant};

pub fn setup_ap_address(iface: &str, cidr: &str) -> Result<()> {
    super::net_rt::setup_ap_address(iface, cidr)
}

pub fn read_sta_ip(iface: &str) -> Result<Option<String>> {
    super::net_rt::read_sta_ip(iface)
}

pub fn read_primary_lan_ipv4() -> Result<Option<String>> {
    super::net_rt::read_primary_lan_ipv4()
}

pub fn ensure_root_or_cap_net_admin() -> Result<()> {
    super::net_rt::ensure_netlink_access()
}

/// 清空接口上的全部 IPv4 地址。
pub fn clear_ipv4_addresses(iface: &str) -> Result<()> {
    super::net_rt::clear_ipv4_addresses(iface)
}

/// 在 `phy_iface` 同 phy 上创建虚拟 AP 接口（`NL80211_IFTYPE_AP`），供 hostapd 使用。
/// 使用 nl80211 NEW_INTERFACE；不再调用 `iw dev <phy> interface add <ap> type __ap`。
pub fn create_virtual_ap_iface(phy_iface: &str, ap_iface: &str) -> Result<()> {
    super::nl80211::create_virtual_ap_iface(phy_iface, ap_iface)?;
    wait_iface_sysfs_ready(ap_iface, "wifi_virt_iface_add")?;
    Ok(())
}

/// 读取当前 WiFi 信道号（通过 nl80211 GET_INTERFACE → ATTR_WIPHY_FREQ）。
/// 无信道信息（未关联/驱动未上报）时返回 `Ok(None)`。
pub fn read_wifi_channel(iface: &str) -> Result<Option<u8>> {
    super::nl80211::get_interface_channel(iface)
}

/// 删除虚拟接口（nl80211 DEL_INTERFACE）；best-effort，接口不存在时忽略错误。
pub fn delete_virtual_iface(ap_iface: &str) -> Result<()> {
    match super::nl80211::delete_virtual_iface(ap_iface) {
        Ok(()) => Ok(()),
        Err(e) if e.stage() == "wifi_capability_check" => {
            // get_ifindex 失败 → 接口不存在，视为成功
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// NEW_INTERFACE 后 sysfs 与 rtnetlink 可能短暂不一致；轮询 sysfs 就绪。
fn wait_iface_sysfs_ready(name: &str, stage: &'static str) -> Result<()> {
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
        stage,
        format!("timeout waiting for sysfs {}", path.display()),
    ))
}
