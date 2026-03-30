//! Linux 原生 GENL/nl80211 套接字层，替换所有 `iw` 子进程调用。
//! 使用 NETLINK_GENERIC 套接字直接与内核 nl80211 子系统通信；libc 已在非 ESP 目标依赖中，无需引入新 crate。
//!
//! 公开（对 super）的函数：
//! - [`detect_wifi_iface`]  — sysfs 探测无线接口名
//! - [`probe_phy_capabilities`]  — GET_WIPHY → `PhyCapabilities`
//! - [`get_interface_channel`]  — GET_INTERFACE → `Option<u8>` 信道号
//! - [`create_virtual_ap_iface`]  — NEW_INTERFACE（ap0 等虚拟 AP 口）
//! - [`delete_virtual_iface`]  — DEL_INTERFACE
//! - [`scan_bounded`]  — TRIGGER_SCAN + 等待完成 + GET_SCAN DUMP

use crate::error::{Error, Result};
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

// ── netlink 协议常量 ─────────────────────────────────────────────────────────
const NETLINK_GENERIC: libc::c_int = 16;
const NLM_F_REQUEST: u16 = 0x0001;
const NLM_F_ACK: u16 = 0x0004;
const NLM_F_ROOT: u16 = 0x0100;
const NLM_F_MATCH: u16 = 0x0200;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLMSG_HDR: usize = 16; // sizeof(nlmsghdr)
const GENL_HDR: usize = 4;   // sizeof(genlmsghdr)
const NLA_HDR: usize = 4;    // sizeof(nlattr)

// ── GENL 控制族 ──────────────────────────────────────────────────────────────
const GENL_ID_CTRL: u16 = 0x10;
const CTRL_CMD_GETFAMILY: u8 = 3;
const CTRL_VERSION: u8 = 2;
const CTRL_ATTR_FAMILY_NAME: u16 = 2;
const CTRL_ATTR_FAMILY_ID: u16 = 1;
const CTRL_ATTR_MCAST_GROUPS: u16 = 7;
const CTRL_ATTR_MCAST_GRP_NAME: u16 = 1;
const CTRL_ATTR_MCAST_GRP_ID: u16 = 2;
const NL80211_GENL_NAME: &str = "nl80211";
const NL80211_SCAN_GROUP_NAME: &str = "scan";

// ── nl80211 命令 ─────────────────────────────────────────────────────────────
const CMD_GET_WIPHY: u8 = 1;
const CMD_GET_INTERFACE: u8 = 5;
const CMD_NEW_INTERFACE: u8 = 7;
const CMD_DEL_INTERFACE: u8 = 8;
const CMD_GET_SCAN: u8 = 32;
const CMD_TRIGGER_SCAN: u8 = 33;
const CMD_NEW_SCAN_RESULTS: u8 = 34;
const CMD_SCAN_ABORTED: u8 = 35;

// ── nl80211 属性（须与 `include/uapi/linux/nl80211.h` 中 `enum nl80211_attrs` 一致）────────
const ATTR_WIPHY: u16 = 1;
const ATTR_IFINDEX: u16 = 3;
const ATTR_IFNAME: u16 = 4;
const ATTR_IFTYPE: u16 = 5;
const ATTR_SUPPORTED_IFTYPES: u16 = 32;
const ATTR_WIPHY_FREQ: u16 = 38;
const ATTR_WIPHY_BANDS: u16 = 22;
const ATTR_BSS: u16 = 47;
const ATTR_INTERFACE_COMBINATIONS: u16 = 120;

// nl80211 接口类型（作为 u32 载荷写入 ATTR_IFTYPE；作为 u16 用于 SUPPORTED_IFTYPES/IFACE_LIMIT_TYPES 类型比较）
pub(super) const IFTYPE_AP: u32 = 3;
const IFTYPE_STATION_KEY: u16 = 2; // NL80211_IFTYPE_STATION，作为 NLA type key
const IFTYPE_AP_KEY: u16 = 3;      // NL80211_IFTYPE_AP，作为 NLA type key

// 频段属性
const BAND_ATTR_FREQS: u16 = 1;
const FREQ_ATTR_FREQ: u16 = 1;

// BSS 属性（嵌套在 ATTR_BSS 内）
const BSS_SIGNAL_MBM: u16 = 7;
const BSS_INFORMATION_ELEMENTS: u16 = 6;

// 接口组合属性
const IFACE_COMB_LIMITS: u16 = 1;
const IFACE_LIMIT_TYPES: u16 = 2;

// nl80211 命令超时（内部实现常量，不暴露为 constants.rs 条目）
const CMD_TIMEOUT: Duration = Duration::from_secs(8);
const SCAN_INIT_TIMEOUT: Duration = Duration::from_secs(5);
// AP-only 模式下无法订阅扫描 multicast group 时的回退等待时间
const SCAN_FALLBACK_WAIT: Duration = Duration::from_secs(3);

// ── 缓存的 GENL 配置 ─────────────────────────────────────────────────────────

#[derive(Clone)]
struct Nl80211Cfg {
    family_id: u16,
    /// nl80211 "scan" multicast group ID；0 表示未找到（部分极简内核）。
    scan_group_id: u32,
}

static NL80211_CFG: OnceLock<std::result::Result<Nl80211Cfg, String>> = OnceLock::new();

fn nl80211_cfg() -> Result<&'static Nl80211Cfg> {
    match NL80211_CFG.get_or_init(|| resolve_nl80211_cfg().map_err(|e| e.to_string())) {
        Ok(c) => Ok(c),
        Err(e) => Err(Error::config("wifi_nl80211_init", e.clone())),
    }
}

fn resolve_nl80211_cfg() -> Result<Nl80211Cfg> {
    let sock = Socket::open()?;
    let deadline = Instant::now() + CMD_TIMEOUT;

    let mut msg = MsgBuf::new();
    let hdr = msg.reserve_nlmsghdr();
    msg.push_genlmsghdr(CTRL_CMD_GETFAMILY, CTRL_VERSION);
    msg.push_nlattr_str(CTRL_ATTR_FAMILY_NAME, NL80211_GENL_NAME);
    msg.finalize(hdr, GENL_ID_CTRL, NLM_F_REQUEST | NLM_F_ACK, next_seq());
    sock.send(msg.as_bytes())?;

    let msgs = recv_msgs(&sock, GENL_ID_CTRL, false, deadline)?;

    let mut family_id: Option<u16> = None;
    let mut scan_group_id: u32 = 0;

    for (_, payload) in &msgs {
        for (atype, adata) in nlattr_iter(payload) {
            match atype {
                CTRL_ATTR_FAMILY_ID => {
                    if let Some(id) = read_u16(adata) {
                        family_id = Some(id);
                    }
                }
                CTRL_ATTR_MCAST_GROUPS => {
                    for (_, grp) in nlattr_iter(adata) {
                        let mut name: Option<&str> = None;
                        let mut gid: Option<u32> = None;
                        for (gtype, gdata) in nlattr_iter(grp) {
                            match gtype {
                                CTRL_ATTR_MCAST_GRP_NAME => {
                                    name = std::str::from_utf8(gdata)
                                        .ok()
                                        .map(|s| s.trim_end_matches('\0'));
                                }
                                CTRL_ATTR_MCAST_GRP_ID => gid = read_u32(gdata),
                                _ => {}
                            }
                        }
                        if matches!(name, Some(n) if n == NL80211_SCAN_GROUP_NAME) {
                            if let Some(id) = gid {
                                scan_group_id = id;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let family_id = family_id.ok_or_else(|| {
        Error::config(
            "wifi_nl80211_init",
            "nl80211 family not found; is cfg80211 kernel module loaded?",
        )
    })?;

    Ok(Nl80211Cfg {
        family_id,
        scan_group_id,
    })
}

// ── 公开函数 ─────────────────────────────────────────────────────────────────

/// sysfs 探测无线接口；不需要 `iw`。
pub(super) fn detect_wifi_iface() -> Result<String> {
    for iface in ["wlan0", "wlan1"] {
        if Path::new("/sys/class/net").join(iface).exists() {
            return Ok(iface.to_string());
        }
    }
    Err(Error::config(
        "wifi_capability_check",
        "no wlan interface found (wlan0/wlan1)",
    ))
}

/// 从 sysfs 读取 wiphy 索引（`/sys/class/net/<iface>/phy80211/index`）。
pub(super) fn get_wiphy_index(iface: &str) -> Result<u32> {
    let path = Path::new("/sys/class/net").join(iface).join("phy80211/index");
    read_sysfs_u32(&path, "wifi_capability_check")
}

/// 从 sysfs 读取接口索引（`/sys/class/net/<iface>/ifindex`）。
pub(super) fn get_ifindex(iface: &str) -> Result<u32> {
    let path = Path::new("/sys/class/net").join(iface).join("ifindex");
    read_sysfs_u32(&path, "wifi_capability_check")
}

fn read_sysfs_u32(path: &Path, stage: &'static str) -> Result<u32> {
    std::fs::read_to_string(path)
        .map_err(|e| Error::io(stage, e))?
        .trim()
        .parse()
        .map_err(|_| Error::config(stage, format!("invalid sysfs value at {}", path.display())))
}

/// GET_WIPHY → `PhyCapabilities`（AP 支持、STA+AP 并发、频段）。
pub(super) fn probe_phy_capabilities(
    iface: &str,
) -> Result<super::capability::PhyCapabilities> {
    let cfg = nl80211_cfg()?;
    let ifindex = get_ifindex(iface)?;

    let sock = Socket::open()?;
    let deadline = Instant::now() + CMD_TIMEOUT;

    let mut msg = MsgBuf::new();
    let hdr = msg.reserve_nlmsghdr();
    msg.push_genlmsghdr(CMD_GET_WIPHY, 0);
    msg.push_nlattr_u32(ATTR_IFINDEX, ifindex);
    msg.finalize(hdr, cfg.family_id, NLM_F_REQUEST | NLM_F_ACK, next_seq());
    sock.send(msg.as_bytes())?;

    let msgs = recv_msgs(&sock, cfg.family_id, false, deadline)?;
    parse_wiphy_capabilities(&msgs)
}

fn parse_wiphy_capabilities(
    msgs: &[(u8, Vec<u8>)],
) -> Result<super::capability::PhyCapabilities> {
    let mut supports_ap = false;
    let mut supports_sta_ap_concurrent = false;
    let mut has_2ghz = false;
    let mut has_5ghz = false;

    for (_, payload) in msgs {
        for (atype, adata) in nlattr_iter(payload) {
            match atype {
                ATTR_SUPPORTED_IFTYPES => {
                    // 每个子属性的 NLA type == iftype index
                    for (iftype_key, _) in nlattr_iter(adata) {
                        if iftype_key == IFTYPE_AP_KEY {
                            supports_ap = true;
                        }
                    }
                }
                ATTR_WIPHY_BANDS => {
                    for (_, band_data) in nlattr_iter(adata) {
                        for (btype, bdata) in nlattr_iter(band_data) {
                            if btype == BAND_ATTR_FREQS {
                                for (_, freq_entry) in nlattr_iter(bdata) {
                                    for (ftype, fdata) in nlattr_iter(freq_entry) {
                                        if ftype == FREQ_ATTR_FREQ {
                                            if let Some(mhz) = read_u32(fdata) {
                                                if (2400..=2500).contains(&mhz) {
                                                    has_2ghz = true;
                                                }
                                                if mhz >= 4900 {
                                                    has_5ghz = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                ATTR_INTERFACE_COMBINATIONS => {
                    if has_concurrent_sta_ap(adata) {
                        supports_sta_ap_concurrent = true;
                    }
                }
                _ => {}
            }
        }
    }

    if !has_2ghz && !has_5ghz {
        has_2ghz = true; // 未识别频段时回退假设 2.4 GHz
    }

    Ok(super::capability::PhyCapabilities {
        supports_ap,
        supports_sta_ap_concurrent,
        has_2ghz,
        has_5ghz,
    })
}

/// 检查 ATTR_INTERFACE_COMBINATIONS 中是否存在同时含 managed + AP 的组合。
fn has_concurrent_sta_ap(combinations: &[u8]) -> bool {
    for (_, combo) in nlattr_iter(combinations) {
        let mut has_sta = false;
        let mut has_ap = false;
        for (ctype, cdata) in nlattr_iter(combo) {
            if ctype == IFACE_COMB_LIMITS {
                for (_, limit) in nlattr_iter(cdata) {
                    for (ltype, ldata) in nlattr_iter(limit) {
                        if ltype == IFACE_LIMIT_TYPES {
                            for (iftype_key, _) in nlattr_iter(ldata) {
                                if iftype_key == IFTYPE_STATION_KEY {
                                    has_sta = true;
                                }
                                if iftype_key == IFTYPE_AP_KEY {
                                    has_ap = true;
                                }
                            }
                        }
                    }
                }
            }
        }
        if has_sta && has_ap {
            return true;
        }
    }
    false
}

/// GET_INTERFACE → 当前信道号（MHz → channel index）。未关联或无信道信息时返回 `None`。
pub(super) fn get_interface_channel(iface: &str) -> Result<Option<u8>> {
    let cfg = nl80211_cfg()?;
    let ifindex = get_ifindex(iface)?;

    let sock = Socket::open()?;
    let deadline = Instant::now() + SCAN_INIT_TIMEOUT;

    let mut msg = MsgBuf::new();
    let hdr = msg.reserve_nlmsghdr();
    msg.push_genlmsghdr(CMD_GET_INTERFACE, 0);
    msg.push_nlattr_u32(ATTR_IFINDEX, ifindex);
    msg.finalize(hdr, cfg.family_id, NLM_F_REQUEST | NLM_F_ACK, next_seq());
    sock.send(msg.as_bytes())?;

    let msgs = recv_msgs(&sock, cfg.family_id, false, deadline)?;
    for (_, payload) in &msgs {
        for (atype, adata) in nlattr_iter(payload) {
            if atype == ATTR_WIPHY_FREQ {
                if let Some(mhz) = read_u32(adata) {
                    return Ok(mhz_to_channel(mhz));
                }
            }
        }
    }
    Ok(None)
}

fn mhz_to_channel(mhz: u32) -> Option<u8> {
    match mhz {
        2412..=2472 => {
            let ch = ((mhz - 2412) / 5 + 1) as u8;
            Some(ch)
        }
        2484 => Some(14),
        5180..=5900 => {
            let ch = ((mhz - 5000) / 5) as u8;
            Some(ch)
        }
        _ => None,
    }
}

/// NEW_INTERFACE：在 `phy_iface` 同 phy 上创建虚拟 AP 接口（类型 `NL80211_IFTYPE_AP`）。
/// 创建前先尝试删除同名残留接口（best-effort）。
pub(super) fn create_virtual_ap_iface(phy_iface: &str, ap_iface: &str) -> Result<()> {
    // 先清理可能残留的同名接口
    if let Ok(idx) = get_ifindex(ap_iface) {
        let _ = del_by_ifindex(idx);
    }

    let cfg = nl80211_cfg()?;
    let wiphy_idx = get_wiphy_index(phy_iface)?;

    let sock = Socket::open()?;
    let deadline = Instant::now() + CMD_TIMEOUT;

    let mut msg = MsgBuf::new();
    let hdr = msg.reserve_nlmsghdr();
    msg.push_genlmsghdr(CMD_NEW_INTERFACE, 0);
    msg.push_nlattr_u32(ATTR_WIPHY, wiphy_idx);
    msg.push_nlattr_str(ATTR_IFNAME, ap_iface);
    msg.push_nlattr_u32(ATTR_IFTYPE, IFTYPE_AP);
    msg.finalize(hdr, cfg.family_id, NLM_F_REQUEST | NLM_F_ACK, next_seq());
    sock.send(msg.as_bytes())?;

    let _ = recv_msgs(&sock, cfg.family_id, false, deadline)?;
    Ok(())
}

/// DEL_INTERFACE：按接口名删除虚拟接口。
pub(super) fn delete_virtual_iface(ap_iface: &str) -> Result<()> {
    let ifindex = get_ifindex(ap_iface)?;
    del_by_ifindex(ifindex)
}

fn del_by_ifindex(ifindex: u32) -> Result<()> {
    let cfg = nl80211_cfg()?;
    let sock = Socket::open()?;
    let deadline = Instant::now() + SCAN_INIT_TIMEOUT;

    let mut msg = MsgBuf::new();
    let hdr = msg.reserve_nlmsghdr();
    msg.push_genlmsghdr(CMD_DEL_INTERFACE, 0);
    msg.push_nlattr_u32(ATTR_IFINDEX, ifindex);
    msg.finalize(hdr, cfg.family_id, NLM_F_REQUEST | NLM_F_ACK, next_seq());
    sock.send(msg.as_bytes())?;

    let _ = recv_msgs(&sock, cfg.family_id, false, deadline)?;
    Ok(())
}

/// TRIGGER_SCAN → 等待完成事件 → GET_SCAN DUMP → 解析 BSS 列表。
///
/// 若成功订阅 nl80211 "scan" multicast group，则等待内核 `NEW_SCAN_RESULTS` 事件；
/// 否则回退到固定等待（[`SCAN_FALLBACK_WAIT`]）后直接调用 GET_SCAN，与旧 `iw dev scan` 行为一致。
///
/// # AP-only 驱动限制
/// 部分驱动在 SoftAP 同口上拒绝 `TRIGGER_SCAN`（EOPNOTSUPP/EBUSY）；错误 stage 为 `wifi_scan`，
/// 由调用方上报，不静默回退。接口须已由能力探测与虚拟 AP 创建路径就绪。
pub(super) fn scan_bounded(
    iface: &str,
    deadline: Instant,
) -> Result<Vec<crate::platform::WifiApEntry>> {
    let cfg = nl80211_cfg()?;
    let ifindex = get_ifindex(iface)?;

    // TRIGGER_SCAN 套接字：同时订阅 scan multicast group 以接收完成事件
    let trigger_sock = Socket::open()?;
    let subscribed = cfg.scan_group_id != 0
        && trigger_sock
            .add_mcast_group(cfg.scan_group_id)
            .is_ok();

    // 发送 TRIGGER_SCAN
    {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::config("wifi_scan", "scan timeout"));
        }
        let mut msg = MsgBuf::new();
        let hdr = msg.reserve_nlmsghdr();
        msg.push_genlmsghdr(CMD_TRIGGER_SCAN, 0);
        msg.push_nlattr_u32(ATTR_IFINDEX, ifindex);
        msg.finalize(
            hdr,
            cfg.family_id,
            NLM_F_REQUEST | NLM_F_ACK,
            next_seq(),
        );
        trigger_sock.send(msg.as_bytes())?;
    }

    // 等待扫描完成
    if subscribed {
        wait_scan_complete(&trigger_sock, cfg.family_id, deadline)?;
    } else {
        // 未订阅 multicast：先消费 ACK，再固定等待
        recv_msgs(&trigger_sock, cfg.family_id, false, deadline)?;
        let wait = SCAN_FALLBACK_WAIT.min(deadline.saturating_duration_since(Instant::now()));
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }

    if deadline.saturating_duration_since(Instant::now()).is_zero() {
        return Err(Error::config("wifi_scan", "scan timeout after trigger"));
    }

    // GET_SCAN DUMP — 使用独立套接字避免残留 ACK 干扰 DUMP 终止逻辑
    let get_sock = Socket::open()?;
    let mut msg = MsgBuf::new();
    let hdr = msg.reserve_nlmsghdr();
    msg.push_genlmsghdr(CMD_GET_SCAN, 0);
    msg.push_nlattr_u32(ATTR_IFINDEX, ifindex);
    msg.finalize(
        hdr,
        cfg.family_id,
        NLM_F_REQUEST | NLM_F_ROOT | NLM_F_MATCH,
        next_seq(),
    );
    get_sock.send(msg.as_bytes())?;

    let msgs = recv_msgs(&get_sock, cfg.family_id, true, deadline)?;
    parse_scan_results(&msgs)
}

/// 在 `trigger_sock` 上接收直到同时获得 ACK + NEW_SCAN_RESULTS（或 SCAN_ABORTED）。
fn wait_scan_complete(
    sock: &Socket,
    family_id: u16,
    deadline: Instant,
) -> Result<()> {
    let mut buf = vec![0u8; 65536];
    let mut got_ack = false;
    let mut got_event = false;

    while !got_ack || !got_event {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::config("wifi_scan", "scan timeout waiting for completion event"));
        }
        let timeout_ms = remaining.as_millis().min(5000) as i32;
        let n = sock.recv_into(&mut buf, timeout_ms)?;
        let received = &buf[..n];

        let mut offset = 0;
        while offset + NLMSG_HDR <= received.len() {
            let nlmsg_len = u32_at(received, offset) as usize;
            let nlmsg_type = u16_at(received, offset + 4);
            if nlmsg_len < NLMSG_HDR || offset + nlmsg_len > received.len() {
                break;
            }
            match nlmsg_type {
                NLMSG_ERROR => {
                    if nlmsg_len >= NLMSG_HDR + 4 {
                        let errno = i32_at(received, offset + NLMSG_HDR);
                        if errno < 0 {
                            return Err(map_errno(-errno, "wifi_scan"));
                        }
                    }
                    got_ack = true;
                }
                t if t == family_id => {
                    if nlmsg_len >= NLMSG_HDR + GENL_HDR {
                        let cmd = received[offset + NLMSG_HDR];
                        match cmd {
                            CMD_NEW_SCAN_RESULTS => got_event = true,
                            CMD_SCAN_ABORTED => {
                                return Err(Error::config(
                                    "wifi_scan",
                                    "scan aborted by driver",
                                ));
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            offset += (nlmsg_len + 3) & !3;
        }
    }
    Ok(())
}

fn parse_scan_results(msgs: &[(u8, Vec<u8>)]) -> Result<Vec<crate::platform::WifiApEntry>> {
    let mut out = Vec::new();
    for (_, payload) in msgs {
        for (atype, adata) in nlattr_iter(payload) {
            if atype == ATTR_BSS {
                if let Some(entry) = parse_bss(adata) {
                    out.push(entry);
                }
            }
        }
    }
    out.sort_by(|a, b| b.rssi.cmp(&a.rssi));
    Ok(out)
}

fn parse_bss(bss: &[u8]) -> Option<crate::platform::WifiApEntry> {
    let mut ssid: Option<String> = None;
    let mut rssi: Option<i8> = None;
    for (btype, bdata) in nlattr_iter(bss) {
        match btype {
            BSS_INFORMATION_ELEMENTS => {
                ssid = parse_ssid_ie(bdata);
            }
            BSS_SIGNAL_MBM => {
                if let Some(mbm) = read_i32(bdata) {
                    rssi = Some((mbm / 100).clamp(i8::MIN as i32, i8::MAX as i32) as i8);
                }
            }
            _ => {}
        }
    }
    let ssid = ssid.filter(|s| !s.is_empty())?;
    Some(crate::platform::WifiApEntry {
        ssid,
        rssi: rssi.unwrap_or(-100),
    })
}

fn parse_ssid_ie(ies: &[u8]) -> Option<String> {
    let mut pos = 0;
    while pos + 2 <= ies.len() {
        let tag = ies[pos];
        let len = ies[pos + 1] as usize;
        pos += 2;
        if pos + len > ies.len() {
            break;
        }
        if tag == 0 && len > 0 {
            return String::from_utf8(ies[pos..pos + len].to_vec()).ok();
        }
        pos += len;
    }
    None
}

// ── 接收循环 ─────────────────────────────────────────────────────────────────

/// 接收 GENL 消息直到 ACK（非 dump）或 NLMSG_DONE（dump）。
/// 返回 `(cmd, payload)` 列表（仅属于 `family_id` 的消息）。
fn recv_msgs(
    sock: &Socket,
    family_id: u16,
    dump: bool,
    deadline: Instant,
) -> Result<Vec<(u8, Vec<u8>)>> {
    let mut results = Vec::new();
    let mut buf = vec![0u8; 65536];

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::config("wifi_nl80211_recv", "timeout"));
        }
        let timeout_ms = remaining.as_millis().min(5000) as i32;
        let n = sock.recv_into(&mut buf, timeout_ms)?;
        let received = &buf[..n];

        let mut done = false;
        let mut offset = 0;

        while offset + NLMSG_HDR <= received.len() {
            let nlmsg_len = u32_at(received, offset) as usize;
            let nlmsg_type = u16_at(received, offset + 4);
            if nlmsg_len < NLMSG_HDR || offset + nlmsg_len > received.len() {
                break;
            }
            match nlmsg_type {
                NLMSG_ERROR => {
                    if nlmsg_len >= NLMSG_HDR + 4 {
                        let errno = i32_at(received, offset + NLMSG_HDR);
                        if errno < 0 {
                            return Err(map_errno(-errno, "wifi_nl80211_err"));
                        }
                    }
                    if !dump {
                        done = true;
                    }
                }
                NLMSG_DONE => {
                    done = true;
                }
                t if t == family_id => {
                    if nlmsg_len >= NLMSG_HDR + GENL_HDR {
                        let cmd = received[offset + NLMSG_HDR];
                        let payload =
                            received[offset + NLMSG_HDR + GENL_HDR..offset + nlmsg_len].to_vec();
                        results.push((cmd, payload));
                    }
                }
                _ => {}
            }
            offset += (nlmsg_len + 3) & !3;
        }

        if done {
            break;
        }
    }

    Ok(results)
}

// ── 原始套接字 ───────────────────────────────────────────────────────────────

struct Socket {
    fd: libc::c_int,
}

impl Drop for Socket {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

impl Socket {
    fn open() -> Result<Self> {
        let fd = unsafe {
            libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                NETLINK_GENERIC,
            )
        };
        if fd < 0 {
            return Err(Error::io(
                "wifi_nl80211_socket",
                std::io::Error::last_os_error(),
            ));
        }
        // 绑定：pid=0 让内核分配 port id
        let mut sa: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
        sa.nl_family = libc::AF_NETLINK as libc::sa_family_t;
        let rc = unsafe {
            libc::bind(
                fd,
                &sa as *const libc::sockaddr_nl as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
            )
        };
        if rc < 0 {
            let e = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(Error::io("wifi_nl80211_bind", e));
        }
        Ok(Self { fd })
    }

    fn add_mcast_group(&self, group_id: u32) -> Result<()> {
        let rc = unsafe {
            libc::setsockopt(
                self.fd,
                libc::SOL_NETLINK,
                libc::NETLINK_ADD_MEMBERSHIP,
                &group_id as *const u32 as *const libc::c_void,
                std::mem::size_of::<u32>() as libc::socklen_t,
            )
        };
        if rc < 0 {
            return Err(Error::io(
                "wifi_nl80211_mcast",
                std::io::Error::last_os_error(),
            ));
        }
        Ok(())
    }

    fn send(&self, msg: &[u8]) -> Result<()> {
        let mut offset = 0;
        while offset < msg.len() {
            let rc = unsafe {
                libc::send(
                    self.fd,
                    msg[offset..].as_ptr() as *const libc::c_void,
                    msg.len() - offset,
                    0,
                )
            };
            if rc < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(Error::io("wifi_nl80211_send", e));
            }
            offset += rc as usize;
        }
        Ok(())
    }

    /// 带 poll 超时的 recv；写入 `buf`（使用其已分配容量）并设置 len。
    fn recv_into(&self, buf: &mut Vec<u8>, timeout_ms: i32) -> Result<usize> {
        // poll 等待可读
        let mut pfd = libc::pollfd {
            fd: self.fd,
            events: libc::POLLIN,
            revents: 0,
        };
        loop {
            let rc = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
            if rc < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(Error::io("wifi_nl80211_poll", e));
            }
            if rc == 0 {
                return Err(Error::config("wifi_nl80211_recv", "timeout"));
            }
            break;
        }
        // 确保至少 65536 字节容量
        if buf.capacity() < 65536 {
            buf.reserve(65536 - buf.len());
        }
        loop {
            let n = unsafe {
                libc::recv(
                    self.fd,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.capacity(),
                    0,
                )
            };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(Error::io("wifi_nl80211_recv", e));
            }
            // 安全：已写入 n 字节到已分配但未初始化的区域；只暴露这段区域给调用方
            unsafe { buf.set_len(n as usize) };
            return Ok(n as usize);
        }
    }
}

// ── 消息构建 ─────────────────────────────────────────────────────────────────

struct MsgBuf {
    data: Vec<u8>,
}

impl MsgBuf {
    fn new() -> Self {
        Self {
            data: Vec::with_capacity(256),
        }
    }

    fn reserve_nlmsghdr(&mut self) -> usize {
        let pos = self.data.len();
        self.data.extend_from_slice(&[0u8; NLMSG_HDR]);
        pos
    }

    fn push_genlmsghdr(&mut self, cmd: u8, version: u8) {
        self.data.extend_from_slice(&[cmd, version, 0, 0]);
    }

    fn push_nlattr_u32(&mut self, typ: u16, val: u32) {
        self.data.extend_from_slice(&8u16.to_ne_bytes()); // nla_len = 4 + 4
        self.data.extend_from_slice(&typ.to_ne_bytes());
        self.data.extend_from_slice(&val.to_ne_bytes());
    }

    fn push_nlattr_bytes(&mut self, typ: u16, val: &[u8]) {
        let nla_len = (NLA_HDR + val.len()) as u16;
        self.data.extend_from_slice(&nla_len.to_ne_bytes());
        self.data.extend_from_slice(&typ.to_ne_bytes());
        self.data.extend_from_slice(val);
        // 4 字节对齐填充
        let pad = (4 - (val.len() % 4)) % 4;
        self.data.resize(self.data.len() + pad, 0);
    }

    fn push_nlattr_str(&mut self, typ: u16, s: &str) {
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0); // null 终止符
        self.push_nlattr_bytes(typ, &bytes);
    }

    fn finalize(&mut self, hdr_pos: usize, nlmsg_type: u16, flags: u16, seq: u32) {
        let total = self.data.len() as u32;
        self.data[hdr_pos..hdr_pos + 4].copy_from_slice(&total.to_ne_bytes());
        self.data[hdr_pos + 4..hdr_pos + 6].copy_from_slice(&nlmsg_type.to_ne_bytes());
        self.data[hdr_pos + 6..hdr_pos + 8].copy_from_slice(&flags.to_ne_bytes());
        self.data[hdr_pos + 8..hdr_pos + 12].copy_from_slice(&seq.to_ne_bytes());
        self.data[hdr_pos + 12..hdr_pos + 16].copy_from_slice(&0u32.to_ne_bytes()); // pid=0
    }

    fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}

// ── NLA 迭代器与读取辅助 ─────────────────────────────────────────────────────

struct NlAttrIter<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Iterator for NlAttrIter<'a> {
    type Item = (u16, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos + NLA_HDR > self.data.len() {
            return None;
        }
        let nla_len =
            u16::from_ne_bytes([self.data[self.pos], self.data[self.pos + 1]]) as usize;
        if nla_len < NLA_HDR || self.pos + nla_len > self.data.len() {
            return None;
        }
        // 剥离 NLA_F_NESTED (0x8000) 和 NLA_F_NET_BYTEORDER (0x4000) 标志位
        let raw_type = u16::from_ne_bytes([self.data[self.pos + 2], self.data[self.pos + 3]]);
        let nla_type = raw_type & 0x3FFF;
        let payload = &self.data[self.pos + NLA_HDR..self.pos + nla_len];
        self.pos += (nla_len + 3) & !3;
        Some((nla_type, payload))
    }
}

fn nlattr_iter(data: &[u8]) -> NlAttrIter<'_> {
    NlAttrIter { data, pos: 0 }
}

fn read_u16(data: &[u8]) -> Option<u16> {
    data.get(..2)
        .and_then(|b| b.try_into().ok())
        .map(u16::from_ne_bytes)
}

fn read_u32(data: &[u8]) -> Option<u32> {
    data.get(..4)
        .and_then(|b| b.try_into().ok())
        .map(u32::from_ne_bytes)
}

fn read_i32(data: &[u8]) -> Option<i32> {
    data.get(..4)
        .and_then(|b| b.try_into().ok())
        .map(i32::from_ne_bytes)
}

#[inline]
fn u32_at(data: &[u8], pos: usize) -> u32 {
    let arr: [u8; 4] = data[pos..pos + 4].try_into().unwrap_or([0; 4]);
    u32::from_ne_bytes(arr)
}

#[inline]
fn u16_at(data: &[u8], pos: usize) -> u16 {
    let arr: [u8; 2] = data[pos..pos + 2].try_into().unwrap_or([0; 2]);
    u16::from_ne_bytes(arr)
}

#[inline]
fn i32_at(data: &[u8], pos: usize) -> i32 {
    let arr: [u8; 4] = data[pos..pos + 4].try_into().unwrap_or([0; 4]);
    i32::from_ne_bytes(arr)
}

// ── 序列号 ───────────────────────────────────────────────────────────────────

static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

fn next_seq() -> u32 {
    SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

// ── errno 映射 ───────────────────────────────────────────────────────────────

fn map_errno(errno: i32, stage: &'static str) -> Error {
    match errno {
        libc::EPERM | libc::EACCES => {
            Error::config("wifi_permission", "insufficient permissions for nl80211 operation")
        }
        libc::ENODEV => Error::config(stage, "device not found (ENODEV)"),
        libc::EOPNOTSUPP => {
            Error::config(stage, "operation not supported by driver (EOPNOTSUPP)")
        }
        libc::EBUSY => Error::config(stage, "device busy (EBUSY)"),
        libc::ENOENT => Error::config(stage, "no such entry (ENOENT)"),
        libc::ENOMEM => Error::config(stage, "out of kernel memory (ENOMEM)"),
        libc::EINVAL => Error::config(stage, "invalid argument (EINVAL)"),
        _ => Error::config(stage, format!("nl80211 errno {}", errno)),
    }
}
