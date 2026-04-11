//! Linux 网络栈：STA IPv4 读取与 AP 地址配置，均经 rtnetlink（NETLINK_ROUTE），无 `ip` 子进程。
//! Linux networking: STA IPv4 read and AP address setup via rtnetlink (NETLINK_ROUTE), no `ip` subprocess.

use crate::error::{Error, Result};
use futures::stream::TryStreamExt;
use rtnetlink::packet_route::{
    address::{AddressAttribute, AddressMessage},
    AddressFamily,
};
use rtnetlink::{new_connection, Handle, LinkUnspec};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::runtime::Runtime;

static RTNETLINK_RUNTIME: OnceLock<std::result::Result<Runtime, String>> = OnceLock::new();
const RTNETLINK_OP_TIMEOUT: Duration = Duration::from_secs(5);

fn rtnetlink_runtime() -> Result<&'static Runtime> {
    match RTNETLINK_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())
    }) {
        Ok(rt) => Ok(rt),
        Err(e) => Err(Error::config(
            "rtnetlink_runtime",
            format!("tokio runtime init failed: {}", e),
        )),
    }
}

fn map_rt_stage(e: rtnetlink::Error, stage: &'static str) -> Error {
    Error::Other {
        source: Box::new(e),
        stage,
    }
}

async fn rt_call<T, F>(stage: &'static str, future: F) -> Result<T>
where
    F: Future<Output = std::result::Result<T, rtnetlink::Error>>,
{
    match tokio::time::timeout(RTNETLINK_OP_TIMEOUT, future).await {
        Ok(result) => result.map_err(|e| map_rt_stage(e, stage)),
        Err(_) => Err(Error::config(
            stage,
            format!("rtnetlink timeout after {:?}", RTNETLINK_OP_TIMEOUT),
        )),
    }
}

/// 与 `ip -4 -o addr show dev <iface>` 首条 IPv4 语义对齐：按 netlink 返回顺序取首个可用 IPv4。
/// Aligns with the first IPv4 line from `ip -4 -o addr show dev <iface>`.
pub fn read_sta_ip(iface: &str) -> Result<Option<String>> {
    let rt = rtnetlink_runtime()?;
    rt.block_on(read_sta_ip_async(iface))
}

/// 返回当前默认上行接口的 IPv4，用于 system_info.lan_ip。
/// Reads the IPv4 bound to the current default-route interface.
pub fn read_primary_lan_ipv4() -> Result<Option<String>> {
    let raw = std::fs::read_to_string("/proc/net/route").map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "lan_ip",
    })?;
    let Some(iface) = default_route_iface(&raw) else {
        return Ok(None);
    };
    read_sta_ip(&iface).map_err(|e| e.with_stage("lan_ip"))
}

/// 返回当前默认路由出口接口名；用于判断当前有效上行是否正走 WiFi 接口。
pub fn default_route_iface_name() -> Result<Option<String>> {
    let raw = std::fs::read_to_string("/proc/net/route").map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "default_route_iface",
    })?;
    Ok(default_route_iface(&raw))
}

async fn read_sta_ip_async(iface: &str) -> Result<Option<String>> {
    let (connection, handle, _) = new_connection().map_err(|e| Error::io("wifi_sta_ip", e))?;
    tokio::spawn(connection);

    let mut links = handle.link().get().match_name(iface.to_string()).execute();
    let Some(link) = rt_call("wifi_sta_ip", links.try_next()).await? else {
        return Ok(None);
    };

    let mut addresses = handle
        .address()
        .get()
        .set_link_index_filter(link.header.index)
        .execute();

    while let Some(msg) = rt_call("wifi_sta_ip", addresses.try_next()).await? {
        if msg.header.family != AddressFamily::Inet {
            continue;
        }
        if let Some(v4) = first_ipv4_from_message(&msg) {
            return Ok(Some(v4.to_string()));
        }
    }
    Ok(None)
}

/// 等价于 `ip addr flush dev` + `ip addr add CIDR dev` + `ip link set dev up`（IPv4 AP 段）。
/// Equivalent to `ip addr flush dev` + `ip addr add CIDR dev` + `ip link set dev up` for IPv4 AP.
pub fn setup_ap_address(iface: &str, cidr: &str) -> Result<()> {
    let rt = rtnetlink_runtime()?;
    rt.block_on(setup_ap_address_async(iface, cidr))
}

/// 等价 `ip -4 addr flush dev <iface>`；用于清理物理 STA 口上残留的旧 SoftAP 地址。
pub fn clear_ipv4_addresses(iface: &str) -> Result<()> {
    let rt = rtnetlink_runtime()?;
    rt.block_on(clear_ipv4_addresses_async(iface))
}

async fn clear_ipv4_addresses_async(iface: &str) -> Result<()> {
    let (connection, handle, _) =
        new_connection().map_err(|e| Error::io("wifi_sta_ip_flush", e))?;
    tokio::spawn(connection);

    let mut links = handle.link().get().match_name(iface.to_string()).execute();
    let Some(link) = rt_call("wifi_sta_ip_flush", links.try_next()).await? else {
        return Err(Error::config("wifi_sta_ip_flush", "interface not found"));
    };

    flush_iface_addresses(&handle, link.header.index).await
}

async fn setup_ap_address_async(iface: &str, cidr: &str) -> Result<()> {
    let (ipv4, prefix) = parse_ipv4_cidr(cidr)?;
    const MAX_ATTEMPTS: u32 = 50;
    const GAP_MS: u64 = 50;
    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(GAP_MS)).await;
        }
        match setup_ap_address_try(iface, ipv4, prefix).await {
            Ok(()) => return Ok(()),
            Err(e) if ap_ip_setup_transient(&e) && attempt + 1 < MAX_ATTEMPTS => {
                log::debug!(
                    "[net_rt] setup_ap_address retry {}/{} on '{}': {}",
                    attempt + 1,
                    MAX_ATTEMPTS,
                    iface,
                    e
                );
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Err(Error::config(
        "wifi_ap_ip_flush",
        "setup_ap_address: exhausted retries",
    ))
}

/// True when rtnetlink may succeed on retry (new virtual iface not ready yet).
fn ap_ip_setup_transient(e: &Error) -> bool {
    match e {
        Error::Config { stage, message } => {
            *stage == "wifi_ap_ip_flush" && message == "interface not found"
        }
        Error::Other { stage, source } => {
            if !matches!(
                *stage,
                "wifi_ap_ip_flush" | "wifi_ap_ip_add" | "wifi_ap_link_up"
            ) {
                return false;
            }
            let mut cur: Option<&dyn std::error::Error> = Some(source.as_ref());
            while let Some(c) = cur {
                let s = c.to_string();
                if s.contains("No such device") || s.contains("os error 19") {
                    return true;
                }
                cur = c.source();
            }
            false
        }
        _ => false,
    }
}

async fn setup_ap_address_try(iface: &str, ipv4: Ipv4Addr, prefix: u8) -> Result<()> {
    let (connection, handle, _) = new_connection().map_err(|e| Error::io("wifi_ap_ip_flush", e))?;
    tokio::spawn(connection);

    let mut links = handle.link().get().match_name(iface.to_string()).execute();
    let Some(link) = rt_call("wifi_ap_ip_flush", links.try_next()).await? else {
        return Err(Error::config("wifi_ap_ip_flush", "interface not found"));
    };
    let index = link.header.index;

    // Virtual `type __ap` ifaces: bring link up before address dump/add avoids ENODEV on some drivers.
    let _ = rt_call(
        "wifi_ap_link_prepare",
        handle
            .link()
            .set(LinkUnspec::new_with_index(index).up().build())
            .execute(),
    )
    .await;

    flush_iface_addresses(&handle, index).await?;

    rt_call(
        "wifi_ap_ip_add",
        handle
            .address()
            .add(index, IpAddr::V4(ipv4), prefix)
            .execute(),
    )
    .await?;

    rt_call(
        "wifi_ap_link_up",
        handle
            .link()
            .set(LinkUnspec::new_with_index(index).up().build())
            .execute(),
    )
    .await?;

    Ok(())
}

async fn flush_iface_addresses(handle: &Handle, index: u32) -> Result<()> {
    let mut addresses = handle
        .address()
        .get()
        .set_link_index_filter(index)
        .execute();
    while let Some(addr) = rt_call("wifi_ap_ip_flush", addresses.try_next()).await? {
        rt_call("wifi_ap_ip_flush", handle.address().del(addr).execute()).await?;
    }
    Ok(())
}

fn parse_ipv4_cidr(s: &str) -> Result<(Ipv4Addr, u8)> {
    let mut parts = s.split('/');
    let addr_s = parts
        .next()
        .ok_or_else(|| Error::config("wifi_ap_ip", "invalid CIDR"))?;
    let prefix_s = parts
        .next()
        .ok_or_else(|| Error::config("wifi_ap_ip", "invalid CIDR"))?;
    if parts.next().is_some() {
        return Err(Error::config("wifi_ap_ip", "invalid CIDR"));
    }
    let addr: Ipv4Addr = addr_s
        .parse()
        .map_err(|_| Error::config("wifi_ap_ip", "invalid IPv4"))?;
    let prefix: u8 = prefix_s
        .parse()
        .map_err(|_| Error::config("wifi_ap_ip", "invalid prefix"))?;
    if prefix > 32 {
        return Err(Error::config("wifi_ap_ip", "invalid prefix"));
    }
    Ok((addr, prefix))
}

/// 与 `ip link show` 等价权限探测：能打开 rtnetlink 并 dump 一条链路即视为具备网络配置能力。
/// Permission probe equivalent to `ip link show`: open rtnetlink and dump one link.
pub fn ensure_netlink_access() -> Result<()> {
    let rt = rtnetlink_runtime()?;
    rt.block_on(ensure_netlink_access_async())
}

async fn ensure_netlink_access_async() -> Result<()> {
    let (connection, handle, _) = new_connection().map_err(|e| Error::io("wifi_permission", e))?;
    tokio::spawn(connection);
    let mut links = handle.link().get().execute();
    let _ = rt_call("wifi_permission", links.try_next()).await?;
    Ok(())
}

fn first_ipv4_from_message(msg: &AddressMessage) -> Option<Ipv4Addr> {
    let mut locals: Vec<Ipv4Addr> = Vec::new();
    let mut addrs: Vec<Ipv4Addr> = Vec::new();
    for attr in &msg.attributes {
        match attr {
            AddressAttribute::Local(IpAddr::V4(ip)) => locals.push(*ip),
            AddressAttribute::Address(IpAddr::V4(ip)) => addrs.push(*ip),
            _ => {}
        }
    }
    for v in locals.iter().chain(addrs.iter()) {
        if !v.is_loopback() {
            return Some(*v);
        }
    }
    locals.first().or(addrs.first()).copied()
}

fn default_route_iface(raw: &str) -> Option<String> {
    for line in raw.lines().skip(1) {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 8 || columns[1] != "00000000" {
            continue;
        }
        let iface = columns[0].trim();
        if iface.is_empty() || iface == "lo" {
            continue;
        }
        return Some(iface.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::default_route_iface;

    #[test]
    fn default_route_iface_prefers_first_default_entry() {
        let raw = "\
Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n\
lo\t00000000\t00000000\t0001\t0\t0\t0\t00000000\n\
eth0\t00000000\t0101A8C0\t0003\t0\t0\t0\t00000000\n\
wlan0\t0008A8C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\n";
        assert_eq!(default_route_iface(raw).as_deref(), Some("eth0"));
    }

    #[test]
    fn default_route_iface_returns_none_without_default_entry() {
        let raw = "\
Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n\
wlan0\t0008A8C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\n";
        assert_eq!(default_route_iface(raw), None);
    }
}
