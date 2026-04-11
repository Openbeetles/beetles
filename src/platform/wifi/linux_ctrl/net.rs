//! Linux 网络操作（AP/STA 数据面）。
//! 数据面地址管理经 rtnetlink；接口创建/删除与信道读取经 nl80211 GENL（不再依赖 `iw`）。

use crate::error::{Error, Result};
use std::ffi::CString;
use std::path::Path;
use std::path::PathBuf;
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

pub fn default_route_iface_name() -> Result<Option<String>> {
    super::net_rt::default_route_iface_name()
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
    wait_iface_kernel_ready(ap_iface, "wifi_virt_iface_add")?;
    Ok(())
}

/// 等待接口同时出现在 sysfs 与内核 netdevice 表中。
/// 仅看 `/sys/class/net/<iface>` 不足以证明 dnsmasq / if_nametoindex 已可见，
/// 尤其是强退后同名虚拟接口重建时，sysfs 出现和用户态可解析之间可能仍有短暂窗口。
pub fn wait_iface_kernel_ready(name: &str, stage: &'static str) -> Result<()> {
    let path = Path::new("/sys/class/net").join(name);
    let c_name = CString::new(name)
        .map_err(|_| Error::config(stage, format!("invalid iface name '{}'", name)))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        let ifindex = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
        if path.exists() && ifindex != 0 {
            std::thread::sleep(Duration::from_millis(150));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err(Error::config(
        stage,
        format!(
            "timeout waiting for kernel-visible iface {}",
            path.display()
        ),
    ))
}

/// 读取当前 WiFi 信道号（通过 nl80211 GET_INTERFACE → ATTR_WIPHY_FREQ）。
/// 无信道信息（未关联/驱动未上报）时返回 `Ok(None)`。
pub fn read_wifi_channel(iface: &str) -> Result<Option<u8>> {
    super::nl80211::get_interface_channel(iface)
}

/// 判断 WiFi 接口当前是否存在有效链路。Linux SBC 上以 sysfs `carrier` 为准，
/// 避免为只读继承路径拉起 Beetle 自己的网络守护进程。
pub fn wifi_associated(iface: &str) -> Result<bool> {
    let path: PathBuf = Path::new("/sys/class/net").join(iface).join("carrier");
    let raw = std::fs::read_to_string(&path).map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "wifi_assoc_state",
    })?;
    Ok(raw.trim() == "1")
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
