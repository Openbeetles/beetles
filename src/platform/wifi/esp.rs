//! WiFi：SoftAP（配置热点） + 可选 STA（连接用户路由器）。
//! 初次启动先开 SoftAP；当启用了配置/急救面时，STA 拿到 IP 后也继续保留 SoftAP，
//! 避免设备在运行期失去用户可达的恢复入口。
//! 支持通过通道向 WiFi 线程请求扫描，供 GET /api/wifi/scan 使用。

use crate::config::AppConfig;
use crate::constants::{WIFI_ESP_CONNECT_MAIN_WAIT_SECS, WIFI_SCAN_TIMEOUT_SECS};
use crate::error::{Error, Result};
use embedded_svc::wifi::{
    AccessPointConfiguration, AuthMethod, ClientConfiguration, Configuration,
};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TAG: &str = "platform::wifi";
const SCAN_RESP_TIMEOUT: Duration = Duration::from_secs(WIFI_SCAN_TIMEOUT_SECS);
const SCAN_RETRY: u32 = 3;
const SCAN_RETRY_DELAY: Duration = Duration::from_millis(400);
/// WiFi worker 负责 ESP WiFi 驱动 + 扫描 + STA 保活。
/// 当前路径已去掉阻塞式 `BlockingWifi::connect()`；常驻循环只做 poll/scan/重连驱动，
/// 继续收回到 8KB internal SRAM。
const WIFI_WORKER_STACK_BYTES: usize = 8 * 1024;
/// STA 状态轮询间隔（毫秒）。
const STA_POLL_INTERVAL_MS: u64 = 5_000;
/// 发起 connect() 后的冷却期（毫秒）：给 WiFi 驱动足够时间完成 auth/assoc/DHCP，
/// 冷却期内不再检查也不再发起 connect()，避免频繁重连干扰驱动状态机。
const STA_RECONNECT_COOLDOWN_MS: u64 = 15_000;
/// STA 刚拿到 DHCP 地址后，不要立刻把 APSTA 切成纯 STA。
/// ESP-IDF 6.0 在 DHCP / netif 事件刚落地的瞬间切 mode，偶发会撞进 pthread 断言。
/// 这里等一个稳定窗口，再执行 SoftAP auto-close。
const STA_SOFTAP_DISABLE_GRACE_MS: u64 = 2_500;
/// 连续多少次 poll 都确认 STA 链路不在，才触发一次 reconnect。
/// 避免瞬时读不到 netif/IP 就自激重连。
const STA_LINK_MISS_THRESHOLD: u8 = 2;
/// 当前启动是否期望 STA 出站网络；纯 SoftAP 配网模式下为 false，避免全局等待卡死。
static WIFI_STA_EXPECTED: AtomicBool = AtomicBool::new(false);
/// SoftAP 已完成启动并配置好本地 IP；用于区分“主线程没等到 ready 信号”和“WiFi 根本没起来”。
static WIFI_SOFTAP_READY: AtomicBool = AtomicBool::new(false);
/// STA 刚拿到 IP 后额外等待一小段时间，再允许外联 DNS/TLS，减少启动瞬间假失败。
const STA_OUTBOUND_READY_GRACE_SECS: u64 = 3;

fn should_keep_softap_available_for_recovery() -> bool {
    cfg!(feature = "config_api")
}

#[derive(Clone)]
struct StaSoftApConfig {
    sta: ClientConfiguration,
    ap: AccessPointConfiguration,
}

/// 其他线程查询 WiFi STA 是否就绪（已连接且有 IP）。
pub fn is_wifi_sta_connected() -> bool {
    crate::state::wifi_sta_connected()
}

/// 读取当前 STA IPv4（点分十进制），无可用地址时返回 None。
pub fn wifi_sta_ip() -> Option<String> {
    crate::state::wifi_sta_ip()
}

pub fn refresh_runtime_state() {}

pub fn passive_scan_handle() -> Option<WifiScanHandle> {
    None
}

/// 阻塞直到出站网络就绪（STA 已连接）；轮询 2s 并喂狗。仅 ESP 生效，host 立即返回。
/// 供 WSS、通道发送、Agent 等对外请求入口在发起请求前调用，避免无网时无意义请求与资源耗尽。
///
/// 须在首次 `feed_current_task` 前将当前任务加入 TWDT（`main` 中本函数早于 `register_current_task_to_task_wdt` 的其它调用点）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn wait_for_network_ready() {
    if !WIFI_STA_EXPECTED.load(Ordering::Relaxed) {
        return;
    }
    crate::platform::task_wdt::register_current_task_to_task_wdt();
    let deadline = Instant::now() + Duration::from_secs(WIFI_ESP_CONNECT_MAIN_WAIT_SECS);
    while !crate::state::wifi_sta_settled_for_outbound(STA_OUTBOUND_READY_GRACE_SECS) {
        crate::platform::task_wdt::feed_current_task();
        if Instant::now() >= deadline {
            log::warn!(
                "[{}] wait_for_network_ready timed out after {}s; continuing startup without STA",
                TAG,
                WIFI_ESP_CONNECT_MAIN_WAIT_SECS
            );
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn wait_for_network_ready() {}

/// GET /api/wifi/scan 返回的单个 AP；按信号强度排序后供前端下拉选择。
#[derive(Clone, Debug, serde::Serialize)]
pub struct WifiApEntry {
    pub ssid: String,
    pub rssi: i8,
}

/// SoftAP 固定 SSID，供用户连接后访问 192.168.4.1
const SOFTAP_SSID: &str = "Beetle";
/// SoftAP 无密码（开放热点），便于开箱配置
const SOFTAP_PASSWORD: &str = "";

/// 通道内扫描结果：成功为列表，失败为错误字符串（避免与 crate::error::Result 混淆）。
#[derive(Clone)]
enum ScanResponse {
    Ok(Arc<[WifiApEntry]>),
    Err(String),
}

/// 扫描句柄：通过通道向 WiFi 线程请求一次扫描，返回 AP 列表（按 RSSI 降序）。
#[derive(Clone)]
pub struct WifiScanHandle {
    req_tx: mpsc::Sender<()>,
    resp_rx: Arc<Mutex<mpsc::Receiver<ScanResponse>>>,
}

/// 向设备请求一次 WiFi 扫描的 trait；由 Platform::wifi_scan() 返回。
pub trait WifiScan: Send + Sync {
    fn request_scan(&self) -> Result<Vec<WifiApEntry>>;
}

impl WifiScan for WifiScanHandle {
    fn request_scan(&self) -> Result<Vec<WifiApEntry>> {
        let _ = self.req_tx.send(());
        let guard = self.resp_rx.lock().map_err(|e| Error::Other {
            source: Box::new(std::io::Error::other(e.to_string())),
            stage: "wifi_scan_lock",
        })?;
        match guard.recv_timeout(SCAN_RESP_TIMEOUT) {
            Ok(ScanResponse::Ok(list)) => Ok(list.as_ref().to_vec()),
            Ok(ScanResponse::Err(msg)) => Err(Error::config("wifi_scan", msg)),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(Error::config("wifi_scan", "scan timeout")),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::Other {
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "wifi scan channel closed",
                )),
                stage: "wifi_scan",
            }),
        }
    }
}

/// 启动 WiFi：若配置了 STA，则先开 SoftAP+STA；
/// 当配置/急救面启用时，STA 真正拿到 IP 后仍保留 SoftAP，确保用户始终可回到恢复入口；
/// 若未配置 STA，则保持纯 SoftAP。
/// 返回 `Ok(Some(handle))` 表示 WiFi 驱动已就绪且可请求扫描；STA 失败或超时仍返回 `Some`，
/// 以便用户连热点改配；`is_wifi_sta_connected()` 反映 STA 是否真正连上。
pub fn connect(config: &AppConfig) -> Result<Option<WifiScanHandle>> {
    let ssid = config.wifi_ssid.clone();
    let pass = config.wifi_pass.clone();
    let has_sta = !ssid.trim().is_empty();
    WIFI_STA_EXPECTED.store(has_sta, Ordering::Relaxed);
    WIFI_SOFTAP_READY.store(false, Ordering::Relaxed);
    if !has_sta {
        clear_sta_ip_cache();
    }

    let (tx, rx) = mpsc::channel();
    let (scan_req_tx, scan_req_rx) = mpsc::channel();
    let (scan_resp_tx, scan_resp_rx) = mpsc::channel::<ScanResponse>();
    crate::util::spawn_guarded_with_profile(
        "wifi_worker",
        WIFI_WORKER_STACK_BYTES,
        Some(crate::util::SpawnCore::Core0),
        crate::util::HttpThreadRole::Io,
        move || {
            do_connect(ssid.as_str(), pass.as_str(), tx, scan_req_rx, scan_resp_tx);
        },
    );

    let result = match rx.recv_timeout(Duration::from_secs(WIFI_ESP_CONNECT_MAIN_WAIT_SECS)) {
        Ok(Ok(())) => {
            if has_sta && should_keep_softap_available_for_recovery() {
                log::info!(
                    "[{}] WiFi ready (SoftAP recovery path stays available after STA DHCP)",
                    TAG
                );
            } else {
                log::info!(
                    "[{}] WiFi ready (SoftAP bootstrap active; STA will auto-close AP after DHCP)",
                    TAG
                );
            }
            Ok(Some(WifiScanHandle {
                req_tx: scan_req_tx,
                resp_rx: Arc::new(Mutex::new(scan_resp_rx)),
            }))
        }
        Ok(Err(e)) => Err(e),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            if WIFI_SOFTAP_READY.load(Ordering::Relaxed) {
                log::warn!(
                    "[{}] WiFi ready signal missed startup deadline ({}s), but SoftAP is already up; continuing with provisioning path",
                    TAG,
                    WIFI_ESP_CONNECT_MAIN_WAIT_SECS
                );
                Ok(Some(WifiScanHandle {
                    req_tx: scan_req_tx,
                    resp_rx: Arc::new(Mutex::new(scan_resp_rx)),
                }))
            } else {
                log::warn!(
                    "[{}] WiFi main thread wait exhausted ({}s) before first ready signal and SoftAP is not ready",
                    TAG,
                    WIFI_ESP_CONNECT_MAIN_WAIT_SECS
                );
                Err(Error::config(
                    "wifi_connect",
                    format!(
                        "main thread wait {}s for WiFi ready signal",
                        WIFI_ESP_CONNECT_MAIN_WAIT_SECS
                    ),
                ))
            }
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::Other {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "wifi thread disconnected",
            )),
            stage: "wifi_connect",
        }),
    };
    result
}

/// 常驻循环：
/// 1. 每 `STA_POLL_INTERVAL_MS` 检查 STA 连接状态，断连时非阻塞发起 reconnect。
/// 2. 响应扫描请求（scan_req_rx）。
///
/// `has_sta` 为 true 时才做 STA 保活检测（纯 AP 模式不需要）。
/// `initial_cooldown` 为 true 时首轮进入冷却（初始 `connect()` 刚发起，等驱动完成，不要抢跑）。
///
/// **重连策略**：只调用底层 `esp_wifi_connect()` 发起一次非阻塞重连，不使用
/// `BlockingWifi::connect()`。后者会在 `wifi_worker` 中同步等待到“已连接”再返回，
/// 把本线程整个卡进等待路径；在 AP+STA 混合模式下，这会让应用层保活循环与驱动
/// 状态迁移互相顶牛，最终表现为反复断连，严重时还会把 Core0 上的网络栈拖进异常状态。
/// 改为“只提交连接请求，后续轮询观察结果”，避免应用层对底层状态机做阻塞式二次驱动。
///
/// **判定 STA 是否仍在线**：勿单独使用 `Wifi::is_connected()`。在 APSTA 下该值为
/// `(AP started) ∧ (STA connected)`，与 SoftAP 事件不同步时会出现短暂假阴性，进而误触发
/// `connect()`，把已关联 STA 打回 `run -> init`。此处以 `WifiDriver::is_sta_connected` 与
/// STA netif 上的有效 IPv4 为准；二者任一成立则视为链路仍在，不发起重连。
///
/// **冷却期仍更新状态**：`WIFI_STA_CONNECTED` 在每轮都刷新，确保其他线程
/// 能及时感知 STA 恢复，而不是等 15s cooldown 结束。
fn run_scan_loop(
    wifi: &mut BlockingWifi<EspWifi>,
    scan_req_rx: &mpsc::Receiver<()>,
    scan_resp_tx: &mpsc::Sender<ScanResponse>,
    has_sta: bool,
    initial_cooldown: bool,
    sta_softap_config: Option<&StaSoftApConfig>,
) {
    let mut cooldown_until: Option<Instant> = if initial_cooldown {
        Some(Instant::now() + Duration::from_millis(STA_RECONNECT_COOLDOWN_MS))
    } else {
        None
    };
    let mut sta_link_miss_count = 0u8;
    let mut next_sta_poll = Instant::now();
    let mut sta_ip_stable_since: Option<Instant> = None;
    // Mixed mode starts with SoftAP enabled whenever STA is configured.
    // The previous inverted initialization kept this false, so the auto-close
    // branch never ran even after STA acquired a DHCP lease.
    let mut softap_enabled = true;

    loop {
        crate::platform::task_wdt::feed_current_task();
        if has_sta && Instant::now() >= next_sta_poll {
            poll_sta_link(
                wifi,
                &mut cooldown_until,
                &mut sta_link_miss_count,
                &mut sta_ip_stable_since,
                &mut softap_enabled,
                sta_softap_config,
            );
            next_sta_poll = Instant::now() + Duration::from_millis(STA_POLL_INTERVAL_MS);
            continue;
        }

        let recv_result = if has_sta {
            let wait = next_sta_poll.saturating_duration_since(Instant::now());
            scan_req_rx.recv_timeout(crate::platform::esp_runtime_policy::bounded_watchdog_wait(
                Some(wait),
            ))
        } else {
            scan_req_rx.recv_timeout(crate::platform::esp_runtime_policy::bounded_watchdog_wait(
                None,
            ))
        };

        match recv_result {
            Ok(()) => {
                let mut pending_requests = 1usize;
                while scan_req_rx.try_recv().is_ok() {
                    pending_requests += 1;
                }
                let result = perform_wifi_scan(wifi);
                for _ in 0..pending_requests {
                    let _ = scan_resp_tx.send(result.clone());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if has_sta {
                    crate::platform::task_wdt::feed_current_task();
                    std::thread::sleep(Duration::from_millis(200));
                    crate::platform::task_wdt::feed_current_task();
                } else {
                    break;
                }
            }
        }
    }
}

fn poll_sta_link(
    wifi: &mut BlockingWifi<EspWifi>,
    cooldown_until: &mut Option<Instant>,
    sta_link_miss_count: &mut u8,
    sta_ip_stable_since: &mut Option<Instant>,
    softap_enabled: &mut bool,
    sta_softap_config: Option<&StaSoftApConfig>,
) {
    let sta_l2 = wifi.wifi().driver().is_sta_connected().unwrap_or(false);
    let sta_ip = read_sta_ipv4_string().filter(|s| s != "0.0.0.0");
    let sta_ip_ok = sta_ip.is_some();
    let sta_link_up = sta_l2 || sta_ip_ok;
    let was_connected = crate::state::wifi_sta_connected();

    if let Some(ip) = sta_ip {
        if !was_connected {
            log::info!("[{}] STA connected (detected in poll)", TAG);
        }
        crate::state::set_wifi_sta_state(true, Some(ip));
        let stable_since = sta_ip_stable_since.get_or_insert_with(Instant::now);
        if *softap_enabled {
            let ready_to_disable =
                stable_since.elapsed() >= Duration::from_millis(STA_SOFTAP_DISABLE_GRACE_MS);
            if ready_to_disable && !should_keep_softap_available_for_recovery() {
                if let Some(config) = sta_softap_config {
                    if let Err(e) = set_softap_enabled(wifi, config, softap_enabled, false) {
                        log::warn!(
                            "[{}] failed to disable SoftAP after STA connect: {}",
                            TAG,
                            e
                        );
                    }
                }
            }
        }
        *sta_link_miss_count = 0;
    } else if sta_l2 {
        // L2 仍在线时保留既有 STA 状态，避免 netif/IP 读的瞬时空窗把上层误判为断网。
        *sta_ip_stable_since = None;
        *sta_link_miss_count = 0;
    } else {
        *sta_ip_stable_since = None;
        *sta_link_miss_count = sta_link_miss_count.saturating_add(1);
        if was_connected && *sta_link_miss_count == 1 {
            log::warn!("[{}] STA disconnected, will reconnect", TAG);
            crate::metrics::record_wifi_failure_stage("wifi_sta_link_down");
        }
        crate::state::clear_wifi_sta_state();
    }

    let in_cooldown = cooldown_until.is_some_and(|t| Instant::now() < t);
    if in_cooldown {
        return;
    }
    if sta_link_up {
        *cooldown_until = None;
        return;
    }
    if *sta_link_miss_count < STA_LINK_MISS_THRESHOLD {
        return;
    }

    if !*softap_enabled {
        if let Some(config) = sta_softap_config {
            if let Err(e) = set_softap_enabled(wifi, config, softap_enabled, true) {
                log::warn!(
                    "[{}] failed to restore SoftAP after STA disconnect: {}",
                    TAG,
                    e
                );
            }
        }
    }
    crate::state::clear_wifi_sta_state();
    crate::metrics::record_wifi_reconnect();
    match issue_sta_connect(wifi) {
        Ok(()) => {
            *sta_link_miss_count = 0;
            log::info!(
                "[{}] STA connect() issued after {} misses, cooldown {}ms",
                TAG,
                *sta_link_miss_count,
                STA_RECONNECT_COOLDOWN_MS
            );
        }
        Err(e) => {
            crate::metrics::record_wifi_failure_stage("wifi_connect");
            log::warn!("[{}] STA connect() failed: {}", TAG, e);
        }
    }
    *cooldown_until = Some(Instant::now() + Duration::from_millis(STA_RECONNECT_COOLDOWN_MS));
}

fn issue_sta_connect(wifi: &mut BlockingWifi<EspWifi>) -> Result<()> {
    wifi.wifi_mut().connect().map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "wifi_connect",
    })
}

fn set_softap_enabled(
    wifi: &mut BlockingWifi<EspWifi>,
    config: &StaSoftApConfig,
    softap_enabled: &mut bool,
    enable: bool,
) -> Result<()> {
    if *softap_enabled == enable {
        return Ok(());
    }

    let next = if enable {
        Configuration::Mixed(config.sta.clone(), config.ap.clone())
    } else {
        Configuration::Client(config.sta.clone())
    };
    wifi.set_configuration(&next).map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "wifi_set_config",
    })?;

    if enable {
        if let Err(e) = crate::platform::softap_ip::set_softap_ip() {
            log::warn!("[{}] SoftAP IP set failed after restore: {}", TAG, e);
        }
        log::info!("[{}] SoftAP restored because STA is unavailable", TAG);
    } else {
        log::info!(
            "[{}] STA obtained local IP; SoftAP stopped to free WiFi SRAM",
            TAG
        );
    }
    *softap_enabled = enable;
    Ok(())
}

fn perform_wifi_scan(wifi: &mut BlockingWifi<EspWifi>) -> ScanResponse {
    let mut last_err_msg = String::new();
    for attempt in 0..SCAN_RETRY {
        match wifi.scan() {
            Ok(aps) => {
                let mut entries: Vec<WifiApEntry> = aps
                    .into_iter()
                    .map(|ap| WifiApEntry {
                        ssid: ap.ssid.as_str().to_string(),
                        rssi: ap.signal_strength,
                    })
                    .collect();
                entries.sort_by(|a, b| b.rssi.cmp(&a.rssi));
                return ScanResponse::Ok(Arc::from(entries.into_boxed_slice()));
            }
            Err(e) => {
                last_err_msg = e.to_string();
                if attempt + 1 < SCAN_RETRY {
                    std::thread::sleep(SCAN_RETRY_DELAY);
                }
            }
        }
    }
    let hint = if last_err_msg.contains("FAIL") || last_err_msg.contains("STATE") {
        " (WiFi busy, try again later)"
    } else {
        ""
    };
    ScanResponse::Err(format!("{}{}", last_err_msg, hint))
}

/// 成功启动后必须让本线程常驻不退出，否则 wifi 被 drop 会关闭热点。
/// 收到 scan_req_rx 时执行一次 scan，结果通过 scan_resp_tx 送回。
fn do_connect(
    sta_ssid: &str,
    sta_password: &str,
    result_tx: mpsc::Sender<Result<()>>,
    scan_req_rx: mpsc::Receiver<()>,
    scan_resp_tx: mpsc::Sender<ScanResponse>,
) {
    let send_err = |e: Error| {
        let _ = result_tx.send(Err(e));
    };
    let peripherals = match Peripherals::take().map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "wifi_peripherals",
    }) {
        Ok(p) => p,
        Err(e) => return send_err(e),
    };
    let sys_loop = match EspSystemEventLoop::take().map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "wifi_event_loop",
    }) {
        Ok(s) => s,
        Err(e) => return send_err(e),
    };
    let nvs = match EspDefaultNvsPartition::take().map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "wifi_nvs",
    }) {
        Ok(n) => n,
        Err(e) => return send_err(e),
    };

    let esp_wifi = match EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs)).map_err(|e| {
        Error::Other {
            source: Box::new(e),
            stage: "wifi_new",
        }
    }) {
        Ok(w) => w,
        Err(e) => return send_err(e),
    };
    let mut wifi = match BlockingWifi::wrap(esp_wifi, sys_loop).map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "wifi_wrap",
    }) {
        Ok(w) => w,
        Err(e) => return send_err(e),
    };

    let ap_config = match (
        SOFTAP_SSID
            .try_into()
            .map_err(|_| Error::config("wifi_ap", "softap ssid too long")),
        SOFTAP_PASSWORD
            .try_into()
            .map_err(|_| Error::config("wifi_ap", "softap password too long")),
    ) {
        (Ok(ssid), Ok(password)) => AccessPointConfiguration {
            ssid,
            password,
            channel: 1,
            ..Default::default()
        },
        (Err(e), _) | (_, Err(e)) => return send_err(e),
    };

    if sta_ssid.is_empty() {
        if let Err(e) = wifi
            .set_configuration(&Configuration::AccessPoint(ap_config))
            .map_err(|e| Error::Other {
                source: Box::new(e),
                stage: "wifi_set_config",
            })
        {
            return send_err(e);
        }
        if let Err(e) = wifi.start().map_err(|e| Error::Other {
            source: Box::new(e),
            stage: "wifi_start",
        }) {
            return send_err(e);
        }
        if let Err(e) = crate::platform::softap_ip::set_softap_ip() {
            log::warn!("[{}] SoftAP IP set failed: {}", TAG, e);
        }
        WIFI_SOFTAP_READY.store(true, Ordering::Relaxed);
        log::info!("[{}] SoftAP started (SSID: {})", TAG, SOFTAP_SSID);
        let _ = result_tx.send(Ok(()));
        run_scan_loop(&mut wifi, &scan_req_rx, &scan_resp_tx, false, false, None);
        return;
    }

    let sta_auth = if sta_password.is_empty() {
        AuthMethod::None
    } else {
        AuthMethod::WPA2Personal
    };
    let sta_config = match (
        sta_ssid
            .try_into()
            .map_err(|_| Error::config("wifi_connect", "ssid too long")),
        sta_password
            .try_into()
            .map_err(|_| Error::config("wifi_connect", "password too long")),
    ) {
        (Ok(ssid), Ok(password)) => ClientConfiguration {
            ssid,
            password,
            auth_method: sta_auth,
            ..Default::default()
        },
        (Err(e), _) | (_, Err(e)) => return send_err(e),
    };

    let ap_config_mixed = match (
        SOFTAP_SSID
            .try_into()
            .map_err(|_| Error::config("wifi_ap", "softap ssid too long")),
        SOFTAP_PASSWORD
            .try_into()
            .map_err(|_| Error::config("wifi_ap", "softap password too long")),
    ) {
        (Ok(ssid), Ok(password)) => AccessPointConfiguration {
            ssid,
            password,
            channel: 1,
            ..Default::default()
        },
        (Err(e), _) | (_, Err(e)) => return send_err(e),
    };
    let sta_softap_config = StaSoftApConfig {
        sta: sta_config.clone(),
        ap: ap_config_mixed.clone(),
    };

    if let Err(e) = wifi
        .set_configuration(&Configuration::Mixed(sta_config, ap_config_mixed))
        .map_err(|e| Error::Other {
            source: Box::new(e),
            stage: "wifi_set_config",
        })
    {
        return send_err(e);
    }
    if let Err(e) = wifi.start().map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "wifi_start",
    }) {
        return send_err(e);
    }

    // AP+STA 下禁用 WiFi 省电：MIN_MODEM 导致 STA 近 50% 时间 sleep，
    // 低 RX 缓冲下极易丢帧，路由器发 DELBA / deauth 导致反复断连。
    unsafe {
        esp_idf_svc::sys::esp_wifi_set_ps(0);
    }

    if let Err(e) = crate::platform::softap_ip::set_softap_ip() {
        log::warn!("[{}] SoftAP IP set failed: {}", TAG, e);
    }
    WIFI_SOFTAP_READY.store(true, Ordering::Relaxed);
    log::info!(
        "[{}] SoftAP started (SSID: {}), connecting STA...",
        TAG,
        SOFTAP_SSID
    );
    if let Err(e) = issue_sta_connect(&mut wifi) {
        log::warn!(
            "[{}] STA connect failed (SoftAP remains active for provisioning): {}",
            TAG,
            e
        );
        clear_sta_ip_cache();
        let _ = result_tx.send(Ok(()));
        run_scan_loop(
            &mut wifi,
            &scan_req_rx,
            &scan_resp_tx,
            true,
            false,
            Some(&sta_softap_config),
        );
        return;
    }
    // 这里只提交一次底层 connect 请求；是否真正拿到 IP 交给后续 scan_loop 观测，
    // 不在启动路径同步等待，避免把 wifi_worker 卡进阻塞 connect。
    let _ = result_tx.send(Ok(()));
    run_scan_loop(
        &mut wifi,
        &scan_req_rx,
        &scan_resp_tx,
        true,
        true,
        Some(&sta_softap_config),
    );
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn clear_sta_ip_cache() {
    crate::state::clear_wifi_sta_state();
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn read_sta_ipv4_string() -> Option<String> {
    const WIFI_STA_DEF: &[u8] = b"WIFI_STA_DEF\0";
    let netif = unsafe {
        esp_idf_svc::sys::esp_netif_get_handle_from_ifkey(WIFI_STA_DEF.as_ptr() as *const _)
    };
    if netif.is_null() {
        return None;
    }
    let mut ip_info: esp_idf_svc::sys::esp_netif_ip_info_t = unsafe { std::mem::zeroed() };
    let ret = unsafe { esp_idf_svc::sys::esp_netif_get_ip_info(netif, &mut ip_info) };
    if ret != esp_idf_svc::sys::ESP_OK {
        return None;
    }
    let octets = ip_info.ip.addr.to_ne_bytes();
    Some(format!(
        "{}.{}.{}.{}",
        octets[0], octets[1], octets[2], octets[3]
    ))
}
