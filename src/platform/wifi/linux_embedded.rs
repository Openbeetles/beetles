//! Linux embedded WiFi：STA/AP/扫描、能力探测、守护与降级（rtnetlink + nl80211 + ctrl 套接字）。

use crate::config::AppConfig;
use crate::constants::{
    SOFTAP_DEFAULT_IPV4, SOFTAP_FALLBACK_IPV4, WIFI_LINUX_AP_VIRT_IFACE,
    WIFI_LINUX_DAEMON_WATCH_INTERVAL_SECS, WIFI_RETRY_BACKOFF_SECS, WIFI_SCAN_TIMEOUT_SECS,
};
use crate::error::{Error, Result};
use crate::metrics;
use crate::platform::wifi::linux_ctrl::{
    capability::{self, PhyCapabilities},
    hostapd, iw_scan, net, process, wpa,
};
use crate::platform::wifi::linux_startup_policy::{
    effective_wifi_runtime_state, EffectiveWifiRuntimeState, LinuxWifiStartup,
};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const TAG: &str = "platform::wifi_linux";
const SOFTAP_SSID: &str = "Beetle";
const SOFTAP_DEFAULT_CHANNEL: u8 = 1;

static WIFI_IFACE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

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

#[derive(Clone)]
pub struct WifiScanHandle {
    iface: String,
    /// 继承系统 WiFi 或无并发 STA 时不用 `wpa_cli`，改用 `iw dev … scan`。
    scan_via_iw: bool,
}

impl WifiScan for WifiScanHandle {
    fn request_scan(&self) -> Result<Vec<WifiApEntry>> {
        const MAX_RETRIES: u32 = 20;
        let deadline = Instant::now() + Duration::from_secs(WIFI_SCAN_TIMEOUT_SECS);
        let mut attempts = 0;
        loop {
            let r = if self.scan_via_iw {
                iw_scan::scan_bounded(&self.iface, deadline)
            } else {
                wpa::scan_bounded(&self.iface, deadline)
            };
            match r {
                Ok(list) => return Ok(list),
                Err(e) => {
                    attempts += 1;
                    if Instant::now() >= deadline || attempts >= MAX_RETRIES {
                        return Err(e.with_stage("wifi_scan"));
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }
    }
}

pub fn is_wifi_sta_connected() -> bool {
    refresh_runtime_state();
    crate::state::wifi_sta_connected()
}

pub fn wifi_sta_ip() -> Option<String> {
    refresh_runtime_state();
    crate::state::wifi_sta_ip()
}

pub fn lan_ipv4() -> Option<String> {
    refresh_runtime_state();
    net::read_primary_lan_ipv4().ok().flatten()
}

pub fn refresh_runtime_state() {
    let Some(iface) = cached_or_detect_iface() else {
        clear_sta_state();
        return;
    };
    apply_runtime_state(&iface, probe_effective_wifi_state(&iface));
}

pub fn passive_scan_handle() -> Option<WifiScanHandle> {
    let iface = cached_or_detect_iface()?;
    Some(WifiScanHandle {
        iface,
        scan_via_iw: true,
    })
}

/// Linux 启动后不阻塞全局启动流程；连接状态由后台与 API 查询。
pub fn wait_for_network_ready() {}

/// 若 iface 上已有 STA 地址落在 `192.168.4.0/24`，则 AP 避让至备用网段，避免与 SoftAP 默认网段冲突。
fn choose_ap_ip(iface: &str) -> &'static str {
    match net::read_sta_ip(iface).ok().flatten() {
        Some(ip) if ip.starts_with("192.168.4.") => {
            log::info!(
                "[{}] existing STA in 192.168.4.0/24, AP using fallback {}",
                TAG,
                SOFTAP_FALLBACK_IPV4
            );
            SOFTAP_FALLBACK_IPV4
        }
        _ => SOFTAP_DEFAULT_IPV4,
    }
}

/// 智能选择 AP 信道：若 STA 已有信道则优先跟随（提高并发芯片兼容性），否则用默认信道。
fn choose_ap_channel(sta_iface: &str) -> u8 {
    match net::read_wifi_channel(sta_iface).ok().flatten() {
        Some(ch) => {
            log::info!(
                "[{}] adaptive AP channel selected from STA iface: {}",
                TAG,
                ch
            );
            ch
        }
        None => SOFTAP_DEFAULT_CHANNEL,
    }
}

/// 在并发模式下，STA 连上后将 AP 对齐到 STA 当前信道，减少“进程正常但热点难扫描”的兼容性问题。
fn maybe_align_ap_channel_for_concurrency(
    sta_iface: &str,
    ap_iface: &str,
    ap_ip: &str,
    ap_channel: &mut u8,
) -> Result<()> {
    let Some(sta_ch) = net::read_wifi_channel(sta_iface).ok().flatten() else {
        return Ok(());
    };
    if sta_ch == *ap_channel {
        return Ok(());
    }
    log::warn!(
        "[{}] aligning AP channel {} -> {} to match STA iface '{}' for better compatibility",
        TAG,
        *ap_channel,
        sta_ch,
        sta_iface
    );
    hostapd::stop_ap(ap_iface);
    hostapd::start_ap_on_channel(ap_iface, SOFTAP_SSID, ap_ip, sta_ch)?;
    *ap_channel = sta_ch;
    Ok(())
}

/// AP 已用默认地址而 STA DHCP 落在 `192.168.4.0/24` 时，迁移 AP 至备用地址。
/// `ap_iface` 为 hostapd 实际运行的接口（可能是虚拟接口 `ap0`）。
fn migrate_ap_if_subnet_conflict(
    ap_iface: &str,
    ap_ip: &str,
    sta_ip: &Option<String>,
    ap_channel: u8,
) -> Result<()> {
    if ap_ip != SOFTAP_DEFAULT_IPV4 {
        return Ok(());
    }
    let Some(sta) = sta_ip else {
        return Ok(());
    };
    if sta.starts_with("192.168.4.") {
        log::warn!(
            "[{}] STA {} on 192.168.4.0/24 conflicts with AP {}; migrating AP to {}",
            TAG,
            sta,
            SOFTAP_DEFAULT_IPV4,
            SOFTAP_FALLBACK_IPV4
        );
        hostapd::stop_ap(ap_iface);
        match hostapd::start_ap_on_channel(ap_iface, SOFTAP_SSID, SOFTAP_FALLBACK_IPV4, ap_channel)
        {
            Ok(()) => return Ok(()),
            Err(e) => {
                log::error!(
                    "[{}] migrate AP to {} failed ({}); restoring {} so provisioning stays possible",
                    TAG,
                    SOFTAP_FALLBACK_IPV4,
                    e,
                    SOFTAP_DEFAULT_IPV4
                );
                return hostapd::start_ap_on_channel(
                    ap_iface,
                    SOFTAP_SSID,
                    SOFTAP_DEFAULT_IPV4,
                    ap_channel,
                );
            }
        }
    }
    Ok(())
}

fn log_phy_caps(caps: &PhyCapabilities) {
    log::info!(
        "[{}] phy: ap={} concurrent_sta_ap={} band_2g={} band_5g={}",
        TAG,
        caps.supports_ap,
        caps.supports_sta_ap_concurrent,
        caps.has_2ghz,
        caps.has_5ghz
    );
}

fn existing_effective_wifi_startup(iface: &str) -> LinuxWifiStartup {
    match probe_effective_wifi_state(iface) {
        Some(runtime) => LinuxWifiStartup::Inherit {
            ip: runtime.ip.unwrap_or_default(),
            scan_via_iw: runtime.scan_via_iw,
        },
        None => LinuxWifiStartup::Fallback,
    }
}

fn probe_effective_wifi_state(iface: &str) -> Option<EffectiveWifiRuntimeState> {
    let associated = match net::wifi_associated(iface) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                "[{}] WiFi link-state probe failed on '{}': {}",
                TAG,
                iface,
                e
            );
            return None;
        }
    };
    let sta_ip = match net::read_sta_ip(iface) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[{}] WiFi IPv4 probe failed on '{}': {}", TAG, iface, e);
            return None;
        }
    };
    let default_route_iface = match net::default_route_iface_name() {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[{}] default-route probe failed: {}", TAG, e);
            return None;
        }
    };
    let runtime = effective_wifi_runtime_state(
        associated,
        sta_ip.as_deref(),
        default_route_iface.as_deref(),
        iface,
    );
    runtime.connected.then_some(runtime)
}

pub fn connect(config: &AppConfig) -> Result<Option<WifiScanHandle>> {
    let iface = capability::detect_wifi_iface()?;
    set_iface(&iface);

    if let LinuxWifiStartup::Inherit { ip, scan_via_iw } = existing_effective_wifi_startup(&iface) {
        log::info!(
            "[{}] inheriting existing effective WiFi on '{}' with IPv4 {}",
            TAG,
            iface,
            ip
        );
        set_sta_state(Some(ip));
        start_sta_probe_thread(iface.clone());
        return Ok(Some(WifiScanHandle { iface, scan_via_iw }));
    }

    net::ensure_root_or_cap_net_admin()?;
    log::info!(
        "[{}] no effective system WiFi on '{}'; entering Beetle-managed provisioning path",
        TAG,
        iface
    );

    let caps = capability::probe_phy(&iface)?;
    log_phy_caps(&caps);
    if !caps.supports_ap {
        return Err(Error::config(
            "wifi_capability_check",
            "nl80211 does not report AP mode; check driver / cfg80211",
        ));
    }

    let concurrent = caps.supports_sta_ap_concurrent;
    let want_sta = !config.wifi_ssid.trim().is_empty();
    let ap_ip = choose_ap_ip(&iface);
    let mut ap_channel = choose_ap_channel(&iface);

    // Stop any existing AP stack on the physical interface before deciding whether
    // we will re-create AP on the physical iface or a virtual iface.
    hostapd::stop_ap(&iface);
    if let Err(e) = net::clear_ipv4_addresses(&iface) {
        log::warn!(
            "[{}] failed to clear stale IPv4 addresses on '{}': {}",
            TAG,
            iface,
            e
        );
    }

    // When concurrent STA+AP is supported AND STA is requested, use a virtual AP interface
    // so hostapd and wpa_supplicant don't fight over the same nl80211 interface.
    let mut effective_concurrent = concurrent;
    let mut ap_iface = if concurrent && want_sta {
        match net::create_virtual_ap_iface(&iface, WIFI_LINUX_AP_VIRT_IFACE) {
            Ok(()) => {
                log::info!(
                    "[{}] virtual AP interface '{}' created on phy of '{}'",
                    TAG,
                    WIFI_LINUX_AP_VIRT_IFACE,
                    iface
                );
                WIFI_LINUX_AP_VIRT_IFACE.to_string()
            }
            Err(e) => {
                log::warn!(
                    "[{}] failed to create virtual AP iface '{}': {}; degrading to SoftAP-only on '{}'",
                    TAG,
                    WIFI_LINUX_AP_VIRT_IFACE,
                    e,
                    iface
                );
                effective_concurrent = false;
                iface.clone()
            }
        }
    } else {
        iface.clone()
    };

    if let Err(e) = hostapd::start_ap_on_channel(&ap_iface, SOFTAP_SSID, ap_ip, ap_channel) {
        if ap_iface != iface {
            log::warn!(
                "[{}] start AP on virtual iface '{}' failed: {}; deleting iface and degrading to SoftAP-only on '{}'",
                TAG,
                ap_iface,
                e,
                iface
            );
            hostapd::stop_ap(&ap_iface);
            if let Err(del_err) = net::delete_virtual_iface(&ap_iface) {
                log::warn!(
                    "[{}] failed to delete virtual AP iface '{}' after AP start failure: {}",
                    TAG,
                    ap_iface,
                    del_err
                );
            }
            ap_iface = iface.clone();
            effective_concurrent = false;
            ap_channel = choose_ap_channel(&iface);
            hostapd::start_ap_on_channel(&ap_iface, SOFTAP_SSID, ap_ip, ap_channel)?;
        } else {
            return Err(e);
        }
    }
    clear_sta_state();

    let ap_ip_owned = ap_ip.to_string();
    start_daemon_watch_thread(
        iface.clone(),
        ap_iface.clone(),
        ap_ip_owned.clone(),
        ap_channel,
    );

    if !want_sta {
        log::info!("[{}] AP ready (SSID: {})", TAG, SOFTAP_SSID);
        return Ok(Some(WifiScanHandle {
            iface,
            scan_via_iw: false,
        }));
    }

    if !effective_concurrent {
        log::warn!(
            "[{}] phy is not usable for managed+AP concurrent runtime; SoftAP only — STA connect skipped (use provisioning UI, then reboot if driver allows STA-only)",
            TAG
        );
        clear_sta_state();
        start_sta_probe_thread(iface.clone());
        return Ok(Some(WifiScanHandle {
            iface,
            scan_via_iw: true,
        }));
    }

    // STA 连接与 DHCP 可达数十秒；不得阻塞 `connect_wifi` 主路径（与 ESP/Linux P0 一致：AP+配网页先就绪）。
    // 在独立线程中关联 STA，避免 bootstrap 停顿；短暂延迟让 hostapd 在部分驱动上先完成 beacon，再与 wpa 竞争空口。
    let iface_sta = iface.clone();
    let ap_iface_sta = ap_iface.clone();
    let ap_ip_for_migrate = ap_ip_owned;
    let mut ap_channel_for_sta = ap_channel;
    let ssid_owned = config.wifi_ssid.trim().to_string();
    let pass_owned = config.wifi_pass.clone();
    if let Err(e) = std::thread::Builder::new()
        .name("wifi-linux-sta".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            match wpa::connect_sta(&iface_sta, ssid_owned.as_str(), pass_owned.as_str()) {
                Ok(ip) => {
                    if let Err(e) = maybe_align_ap_channel_for_concurrency(
                        &iface_sta,
                        &ap_iface_sta,
                        ap_ip_for_migrate.as_str(),
                        &mut ap_channel_for_sta,
                    ) {
                        log::warn!(
                            "[{}] AP channel alignment skipped due to error; keep current AP config: {}",
                            TAG,
                            e
                        );
                    }
                    if let Err(e) =
                        migrate_ap_if_subnet_conflict(
                            &ap_iface_sta,
                            ap_ip_for_migrate.as_str(),
                            &ip,
                            ap_channel_for_sta,
                        )
                    {
                        log::error!(
                            "[{}] subnet migration failed after restore attempt: {}",
                            TAG,
                            e
                        );
                        if let Err(e2) =
                            hostapd::start_ap_on_channel(
                                &ap_iface_sta,
                                SOFTAP_SSID,
                                ap_ip_for_migrate.as_str(),
                                ap_channel_for_sta,
                            )
                        {
                            log::error!(
                                "[{}] SoftAP emergency recovery failed (user may lose hotspot until reboot): {}",
                                TAG,
                                e2
                            );
                        } else {
                            log::info!(
                                "[{}] SoftAP recovered on emergency retry (STA still up)",
                                TAG
                            );
                        }
                    }
                    set_sta_state(ip);
                }
                Err(e) => {
                    metrics::record_wifi_failure_stage(e.stage());
                    log::warn!(
                        "[{}] STA failed (auth/DHCP/unreachable); SoftAP stays up for provisioning: {}",
                        TAG,
                        e
                    );
                    clear_sta_state();
                }
            }
        })
    {
        log::error!("[{}] STA background thread spawn failed: {}", TAG, e);
        clear_sta_state();
    }
    log::info!(
        "[{}] STA connect running in background (SSID configured); SoftAP already up",
        TAG
    );
    start_sta_probe_thread(iface.clone());
    Ok(Some(WifiScanHandle {
        iface,
        scan_via_iw: false,
    }))
}

/// `sta_iface`: physical interface for wpa_supplicant (e.g. `wlan0`).
/// `ap_iface`: interface where hostapd runs (virtual `ap0` or same as `sta_iface`).
fn start_daemon_watch_thread(sta_iface: String, ap_iface: String, ap_ip: String, ap_channel: u8) {
    let res = std::thread::Builder::new()
        .name("wifi-linux-watch".into())
        .spawn(move || {
            let hostapd_pf = hostapd::daemon_pid_path("hostapd");
            let dnsmasq_pf = hostapd::daemon_pid_path("dnsmasq");
            let wpa_pf = wpa::supplicant_pid_path(&sta_iface);
            let mut current_ap_channel = ap_channel;
            loop {
                std::thread::sleep(Duration::from_secs(WIFI_LINUX_DAEMON_WATCH_INTERVAL_SECS));
                let need_ap = !pid_file_alive(&hostapd_pf) || !pid_file_alive(&dnsmasq_pf);
                if need_ap {
                    let desired_channel = net::read_wifi_channel(&sta_iface)
                        .ok()
                        .flatten()
                        .unwrap_or(current_ap_channel);
                    log::warn!(
                        "[{}] AP stack pid missing or dead; restarting hostapd+dnsmasq on '{}' (channel={})",
                        TAG,
                        ap_iface,
                        desired_channel
                    );
                    metrics::record_wifi_ap_restart();
                    hostapd::stop_ap(&ap_iface);
                    if let Err(e) =
                        hostapd::start_ap_on_channel(&ap_iface, SOFTAP_SSID, &ap_ip, desired_channel)
                    {
                        metrics::record_wifi_failure_stage(e.stage());
                        log::error!("[{}] AP stack restart failed: {}", TAG, e);
                    } else {
                        current_ap_channel = desired_channel;
                    }
                    continue;
                }
                if wpa_pf.exists() {
                    if let Some(pid) = process::read_pid_file(wpa_pf.as_path()) {
                        if !process::is_pid_alive(pid) {
                            log::warn!("[{}] wpa_supplicant not running; re-ensure", TAG);
                            metrics::record_wifi_reconnect();
                            if let Err(e) = wpa::ensure_daemon(&sta_iface) {
                                metrics::record_wifi_failure_stage(e.stage());
                                log::error!("[{}] wpa_supplicant restart failed: {}", TAG, e);
                            }
                        }
                    }
                }
            }
        });
    if let Err(e) = res {
        log::error!("[{}] daemon watch thread spawn failed: {}", TAG, e);
    }
}

fn pid_file_alive(path: &Path) -> bool {
    match process::read_pid_file(path) {
        Some(pid) => process::is_pid_alive(pid),
        None => false,
    }
}

fn start_sta_probe_thread(iface: String) {
    let res = std::thread::Builder::new()
        .name("wifi-linux-probe".into())
        .spawn(move || {
            for attempt in 0..3u32 {
                let iface_cl = iface.clone();
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    probe_loop(&iface_cl);
                }));
                if r.is_err() {
                    log::warn!("[{}] probe thread panic, restart {}/3", TAG, attempt + 1);
                    if attempt == 2 {
                        log::error!("[{}] probe thread aborted after 3 panics", TAG);
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        });
    if let Err(e) = res {
        log::error!("[{}] probe thread spawn failed: {}", TAG, e);
    }
}

fn probe_loop(iface: &str) {
    loop {
        apply_runtime_state(iface, probe_effective_wifi_state(iface));
        std::thread::sleep(Duration::from_secs(WIFI_RETRY_BACKOFF_SECS[0]));
    }
}

fn apply_runtime_state(iface: &str, runtime: Option<EffectiveWifiRuntimeState>) {
    set_iface(iface);
    match runtime.and_then(|state| state.ip) {
        Some(ip) => set_sta_state(Some(ip)),
        None => clear_sta_state(),
    }
}

fn set_sta_state(ip: Option<String>) {
    crate::state::set_wifi_sta_state(ip.is_some(), ip);
}

fn clear_sta_state() {
    crate::state::clear_wifi_sta_state();
}

fn set_iface(iface: &str) {
    if let Ok(mut g) = WIFI_IFACE.get_or_init(|| Mutex::new(None)).lock() {
        *g = Some(iface.to_string());
    }
}

fn cached_or_detect_iface() -> Option<String> {
    if let Ok(g) = WIFI_IFACE.get_or_init(|| Mutex::new(None)).lock() {
        if let Some(iface) = g.as_ref() {
            return Some(iface.clone());
        }
    }
    match capability::detect_wifi_iface() {
        Ok(iface) => {
            set_iface(&iface);
            Some(iface)
        }
        Err(e) => {
            log::warn!("[{}] detect WiFi iface failed: {}", TAG, e);
            None
        }
    }
}
