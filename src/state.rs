//! 进程内共享状态：最近错误等，供 CLI 与 HTTP /api/health 共用。
//! In-process shared state (e.g. last error) for CLI and HTTP.

use crate::error::Error;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const LAST_ERRORS_COUNT_CAP: usize = 10;
const CURRENT_ERROR_TTL_SECS: u64 = 60;

static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);
static CURRENT_ERROR: Mutex<Option<TimedError>> = Mutex::new(None);
static LAST_ERRORS_COUNT: AtomicUsize = AtomicUsize::new(0);
/// 当前 WiFi STA 是否已拿到有效 IP。
static WIFI_STA_CONNECTED: AtomicBool = AtomicBool::new(false);
/// 当前 WiFi STA 连通状态建立时间（unix secs）；供外联探测做短暂 settle window。
static WIFI_STA_CONNECTED_SINCE_SECS: AtomicU32 = AtomicU32::new(0);
/// 当前 WiFi STA IPv4。
static WIFI_STA_IP: OnceLock<Mutex<Option<String>>> = OnceLock::new();
/// 当前是否期望 STA 出站网络。
static NETWORK_STA_EXPECTED: AtomicBool = AtomicBool::new(false);
/// 当前是否已有非空 STA 配置。只记录是否存在，不记录 SSID/密码。
static NETWORK_STA_CONFIGURED: AtomicBool = AtomicBool::new(false);
/// 最近 WiFi 阶段。
static NETWORK_WIFI_STAGE: AtomicU8 = AtomicU8::new(NetworkWifiStage::ApOnly as u8);
/// 最近 WiFi 非敏感 reason code；u32::MAX 表示无。
static NETWORK_WIFI_REASON_CODE: AtomicU32 = AtomicU32::new(u32::MAX);
/// 当前 STA L2 是否已关联。
static NETWORK_STA_L2_CONNECTED: AtomicBool = AtomicBool::new(false);
/// 当前是否处于语音独占窗口；ESP 上对外 WSS 通道在该窗口内主动让路。
static VOICE_EXCLUSIVE_ACTIVE: AtomicBool = AtomicBool::new(false);
/// 当前是否有后台自治/维护作业在 agent 执行面运行。
static BACKGROUND_MAINTENANCE_ACTIVE: AtomicBool = AtomicBool::new(false);
/// 当前 config plane 是否真正处于 active serving 状态。
static CONFIG_PLANE_ACTIVE: AtomicBool = AtomicBool::new(false);
/// 当前进程是否仍处于启动引导阶段；steady-state 建立后显式清除。
static BOOT_PHASE_ACTIVE: AtomicBool = AtomicBool::new(false);
/// runtime mode source 是否已掌握 pairing 要求状态。
static PAIRING_STATE_KNOWN: AtomicBool = AtomicBool::new(false);
/// 当前是否仍要求 pairing。
static PAIRING_REQUIRED: AtomicBool = AtomicBool::new(false);
/// 当前是否处于 recovery safe mode。
static RECOVERY_SAFE_MODE_ACTIVE: AtomicBool = AtomicBool::new(false);
/// 当前是否处于升级/OTA 资源窗口。
static UPGRADE_ACTIVE: AtomicBool = AtomicBool::new(false);
/// ESP operator / deep-inspection window expiry timestamp.
static ESP_OPERATOR_WINDOW_UNTIL_SECS: AtomicU32 = AtomicU32::new(0);

#[derive(Clone)]
struct TimedError {
    message: String,
    seen_at_secs: u64,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkWifiStage {
    ApOnly = 0,
    StaConnecting = 1,
    StaAuthFailed = 2,
    StaApNotFound = 3,
    StaL2Connected = 4,
    StaWaitingDhcp = 5,
    StaIpReady = 6,
    StaRecovering = 7,
    StaFallbackAp = 8,
}

impl NetworkWifiStage {
    const fn from_byte(raw: u8) -> Self {
        match raw {
            1 => Self::StaConnecting,
            2 => Self::StaAuthFailed,
            3 => Self::StaApNotFound,
            4 => Self::StaL2Connected,
            5 => Self::StaWaitingDhcp,
            6 => Self::StaIpReady,
            7 => Self::StaRecovering,
            8 => Self::StaFallbackAp,
            _ => Self::ApOnly,
        }
    }

    const fn implies_l2_connected(self) -> bool {
        matches!(
            self,
            Self::StaL2Connected | Self::StaWaitingDhcp | Self::StaIpReady
        )
    }
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct NetworkRuntimeSnapshot {
    pub sta_expected: bool,
    pub sta_configured: bool,
    pub sta_connecting: bool,
    pub sta_l2_connected: bool,
    pub sta_ip_present: bool,
    pub outbound_settled: bool,
    pub wall_clock_trustworthy: bool,
    pub last_wifi_stage: NetworkWifiStage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_wifi_reason_code: Option<u16>,
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 将 Error 转为可安全打印的摘要；直接使用 Error 的 Display 实现。
pub fn sanitize_error_for_log(e: &Error) -> String {
    e.to_string()
}

/// 设置最近错误摘要（供 health 显示；禁止写入密钥）。同时将最近错误条数 +1，暴露时 cap 为 10。
pub fn set_last_error(e: &Error) {
    let msg = sanitize_error_for_log(e);
    if let Ok(mut g) = LAST_ERROR.lock() {
        *g = Some(msg.clone());
    }
    if let Ok(mut g) = CURRENT_ERROR.lock() {
        *g = Some(TimedError {
            message: msg,
            seen_at_secs: now_unix_secs(),
        });
    }
    let _ = LAST_ERRORS_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// 返回自启动以来 set_last_error 被调用的次数，上限为 10（轻量可观测，不存完整内容）。
pub fn get_last_errors_count() -> usize {
    LAST_ERRORS_COUNT
        .load(Ordering::Relaxed)
        .min(LAST_ERRORS_COUNT_CAP)
}

/// 读取最近错误摘要。
pub fn get_last_error() -> Option<String> {
    LAST_ERROR.lock().ok().and_then(|g| g.clone())
}

fn get_current_error_at(now_secs: u64) -> Option<String> {
    let mut guard = CURRENT_ERROR.lock().ok()?;
    match guard.as_ref() {
        Some(err) if now_secs.saturating_sub(err.seen_at_secs) <= CURRENT_ERROR_TTL_SECS => {
            Some(err.message.clone())
        }
        Some(_) => {
            *guard = None;
            None
        }
        None => None,
    }
}

/// 读取当前仍有效的错误摘要；超过窗口后不再把历史错误冒充当前故障。
pub fn get_current_error() -> Option<String> {
    get_current_error_at(now_unix_secs())
}

/// 更新当前 WiFi STA 状态；业务域只读此状态，不直接依赖 platform helper。
pub fn set_wifi_sta_state(connected: bool, ip: Option<String>) {
    let was_connected = WIFI_STA_CONNECTED.swap(connected, Ordering::Relaxed);
    if connected {
        set_network_wifi_stage(NetworkWifiStage::StaIpReady, None);
        if !was_connected {
            WIFI_STA_CONNECTED_SINCE_SECS.store(now_unix_secs() as u32, Ordering::Relaxed);
        }
    } else {
        WIFI_STA_CONNECTED_SINCE_SECS.store(0, Ordering::Relaxed);
    }
    if let Ok(mut g) = WIFI_STA_IP.get_or_init(|| Mutex::new(None)).lock() {
        *g = if connected { ip } else { None };
    }
}

/// 清空当前 WiFi STA 状态。
pub fn clear_wifi_sta_state() {
    set_wifi_sta_state(false, None);
    NETWORK_STA_L2_CONNECTED.store(false, Ordering::Relaxed);
    let stage = NetworkWifiStage::from_byte(NETWORK_WIFI_STAGE.load(Ordering::Relaxed));
    if !NETWORK_STA_EXPECTED.load(Ordering::Relaxed)
        || !NETWORK_STA_CONFIGURED.load(Ordering::Relaxed)
    {
        set_network_wifi_stage(NetworkWifiStage::ApOnly, None);
    } else if matches!(
        stage,
        NetworkWifiStage::StaL2Connected
            | NetworkWifiStage::StaWaitingDhcp
            | NetworkWifiStage::StaIpReady
    ) {
        set_network_wifi_stage(NetworkWifiStage::StaRecovering, None);
    }
}

/// 设置 STA 期望/配置事实；不保存任何 SSID 或密码。
pub fn set_network_sta_expected(expected: bool, configured: bool) {
    NETWORK_STA_EXPECTED.store(expected, Ordering::Relaxed);
    NETWORK_STA_CONFIGURED.store(configured, Ordering::Relaxed);
    if !expected || !configured {
        set_network_wifi_stage(NetworkWifiStage::ApOnly, None);
    }
}

/// 写入最近 WiFi 阶段与 ESP reason code；reason code 不含凭证。
pub fn set_network_wifi_stage(stage: NetworkWifiStage, reason_code: Option<u16>) {
    NETWORK_WIFI_STAGE.store(stage as u8, Ordering::Relaxed);
    NETWORK_STA_L2_CONNECTED.store(stage.implies_l2_connected(), Ordering::Relaxed);
    NETWORK_WIFI_REASON_CODE.store(
        reason_code.map(u32::from).unwrap_or(u32::MAX),
        Ordering::Relaxed,
    );
}

/// 当前最近 WiFi 阶段。
pub fn network_last_wifi_stage() -> NetworkWifiStage {
    NetworkWifiStage::from_byte(NETWORK_WIFI_STAGE.load(Ordering::Relaxed))
}

/// 最近阶段是否已有明确失败原因；poll 不能把它立即盖成 generic recovering。
pub fn network_last_wifi_stage_has_reasoned_failure() -> bool {
    matches!(
        network_last_wifi_stage(),
        NetworkWifiStage::StaAuthFailed
            | NetworkWifiStage::StaApNotFound
            | NetworkWifiStage::StaFallbackAp
    ) && NETWORK_WIFI_REASON_CODE.load(Ordering::Relaxed) != u32::MAX
}

/// 网络运行态快照。调用方显式传入墙钟可信状态，避免 state 层依赖 platform。
pub fn network_runtime_snapshot(
    wall_clock_trustworthy: bool,
    outbound_settle_secs: u64,
) -> NetworkRuntimeSnapshot {
    let last_wifi_stage = network_last_wifi_stage();
    let sta_ip_present = wifi_sta_connected();
    let reason_code = NETWORK_WIFI_REASON_CODE.load(Ordering::Relaxed);
    NetworkRuntimeSnapshot {
        sta_expected: NETWORK_STA_EXPECTED.load(Ordering::Relaxed),
        sta_configured: NETWORK_STA_CONFIGURED.load(Ordering::Relaxed),
        sta_connecting: matches!(
            last_wifi_stage,
            NetworkWifiStage::StaConnecting | NetworkWifiStage::StaRecovering
        ),
        sta_l2_connected: sta_ip_present || NETWORK_STA_L2_CONNECTED.load(Ordering::Relaxed),
        sta_ip_present,
        outbound_settled: wifi_sta_settled_for_outbound(outbound_settle_secs),
        wall_clock_trustworthy,
        last_wifi_stage,
        last_wifi_reason_code: if reason_code == u32::MAX {
            None
        } else {
            u16::try_from(reason_code).ok()
        },
    }
}

/// 轻量基线日志，避免 heartbeat/API 对网络阶段各自拼接。
pub fn format_network_runtime_baseline_line(snapshot: &NetworkRuntimeSnapshot) -> String {
    format!(
        "network stage={:?} sta_expected={} sta_configured={} l2={} ip={} outbound_settled={} wall_clock={} reason_code={}",
        snapshot.last_wifi_stage,
        snapshot.sta_expected,
        snapshot.sta_configured,
        snapshot.sta_l2_connected,
        snapshot.sta_ip_present,
        snapshot.outbound_settled,
        snapshot.wall_clock_trustworthy,
        snapshot
            .last_wifi_reason_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "none".to_string())
    )
}

/// WiFi STA 是否已连通并获得 IP。
pub fn wifi_sta_connected() -> bool {
    WIFI_STA_CONNECTED.load(Ordering::Relaxed)
}

/// WiFi STA 已连通且稳定超过指定秒数，适合发起 DNS/TLS 等外联。
pub fn wifi_sta_settled_for_outbound(min_connected_secs: u64) -> bool {
    if !wifi_sta_connected() {
        return false;
    }
    let since = WIFI_STA_CONNECTED_SINCE_SECS.load(Ordering::Relaxed) as u64;
    since != 0 && now_unix_secs().saturating_sub(since) >= min_connected_secs
}

/// 当前 WiFi STA IPv4。
pub fn wifi_sta_ip() -> Option<String> {
    WIFI_STA_IP
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|g| g.clone())
}

/// 设置语音独占状态；仅表示高资源的 realtime 会话窗口，不影响待机唤醒监听。
pub fn set_voice_exclusive_active(active: bool) {
    VOICE_EXCLUSIVE_ACTIVE.store(active, Ordering::Relaxed);
}

/// 当前是否处于语音独占状态。
pub fn voice_exclusive_active() -> bool {
    VOICE_EXCLUSIVE_ACTIVE.load(Ordering::Relaxed)
}

/// 设置后台自治/维护作业活动态。
pub fn set_background_maintenance_active(active: bool) {
    BACKGROUND_MAINTENANCE_ACTIVE.store(active, Ordering::Relaxed);
}

/// 当前是否有后台自治/维护作业在执行。
pub fn background_maintenance_active() -> bool {
    BACKGROUND_MAINTENANCE_ACTIVE.load(Ordering::Relaxed)
}

/// 设置 config plane 活动态；用于区分“有监督线程常驻”与“HTTP 配置面真正对外服务”。
pub fn set_config_plane_active(active: bool) {
    CONFIG_PLANE_ACTIVE.store(active, Ordering::Relaxed);
}

/// 当前 config plane 是否真正处于 active serving 状态。
pub fn config_plane_active() -> bool {
    CONFIG_PLANE_ACTIVE.load(Ordering::Relaxed)
}

/// 设置当前进程是否仍在启动引导阶段。
pub fn set_boot_phase_active(active: bool) {
    BOOT_PHASE_ACTIVE.store(active, Ordering::Relaxed);
}

/// 当前进程是否仍在启动引导阶段。
pub fn boot_phase_active() -> bool {
    BOOT_PHASE_ACTIVE.load(Ordering::Relaxed)
}

pub(crate) fn set_pairing_state_known(known: bool) {
    PAIRING_STATE_KNOWN.store(known, Ordering::Relaxed);
}

pub fn pairing_state_known() -> bool {
    PAIRING_STATE_KNOWN.load(Ordering::Relaxed)
}

pub(crate) fn set_pairing_required(required: bool) {
    PAIRING_REQUIRED.store(required, Ordering::Relaxed);
}

pub fn pairing_required() -> bool {
    PAIRING_REQUIRED.load(Ordering::Relaxed)
}

pub(crate) fn set_recovery_safe_mode_active(active: bool) {
    RECOVERY_SAFE_MODE_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn recovery_safe_mode_active() -> bool {
    RECOVERY_SAFE_MODE_ACTIVE.load(Ordering::Relaxed)
}

pub fn set_upgrade_active(active: bool) {
    UPGRADE_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn upgrade_active() -> bool {
    UPGRADE_ACTIVE.load(Ordering::Relaxed)
}

/// 打开 ESP operator window，返回过期时间。
pub fn open_esp_operator_window(ttl_secs: u64) -> u64 {
    let expires_at = now_unix_secs().saturating_add(ttl_secs);
    let expires_at_u32 = u32::try_from(expires_at).unwrap_or(u32::MAX);
    ESP_OPERATOR_WINDOW_UNTIL_SECS.store(expires_at_u32, Ordering::Relaxed);
    u64::from(expires_at_u32)
}

/// 当前 ESP operator window 是否仍然有效。
pub fn esp_operator_window_active() -> bool {
    esp_operator_window_until().is_some()
}

/// 返回 ESP operator window 过期时间；过期后自动清零。
pub fn esp_operator_window_until() -> Option<u64> {
    let until = u64::from(ESP_OPERATOR_WINDOW_UNTIL_SECS.load(Ordering::Relaxed));
    if until == 0 {
        return None;
    }
    if until <= now_unix_secs() {
        ESP_OPERATOR_WINDOW_UNTIL_SECS.store(0, Ordering::Relaxed);
        return None;
    }
    Some(until)
}

/// 清空 ESP operator window。
pub fn clear_esp_operator_window() {
    ESP_OPERATOR_WINDOW_UNTIL_SECS.store(0, Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn test_state_guard() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_error_expires_without_erasing_last_error_history() {
        let _guard = test_state_guard();
        let err = Error::config("test_stage", "boom");
        let expected = sanitize_error_for_log(&err);
        set_last_error(&err);

        let now = now_unix_secs();
        assert_eq!(get_current_error_at(now), Some(expected.clone()));
        assert_eq!(get_current_error_at(now + CURRENT_ERROR_TTL_SECS + 1), None);
        assert_eq!(get_last_error(), Some(expected));
    }

    #[test]
    fn wifi_sta_state_clears_ip_when_disconnected() {
        let _guard = test_state_guard();
        set_wifi_sta_state(true, Some("192.168.1.2".to_string()));
        assert!(wifi_sta_connected());
        assert_eq!(wifi_sta_ip().as_deref(), Some("192.168.1.2"));

        clear_wifi_sta_state();
        assert!(!wifi_sta_connected());
        assert_eq!(wifi_sta_ip(), None);
    }

    #[test]
    fn wifi_sta_must_settle_before_outbound_ready() {
        let _guard = test_state_guard();
        set_wifi_sta_state(true, Some("192.168.1.2".to_string()));
        assert!(!wifi_sta_settled_for_outbound(1));
        clear_wifi_sta_state();
    }

    #[test]
    fn network_runtime_snapshot_explains_ap_only_without_leaking_config() {
        let _guard = test_state_guard();
        set_network_sta_expected(false, false);
        clear_wifi_sta_state();

        let snapshot = network_runtime_snapshot(false, 3);

        assert!(!snapshot.sta_expected);
        assert!(!snapshot.sta_configured);
        assert!(!snapshot.sta_ip_present);
        assert_eq!(snapshot.last_wifi_stage, NetworkWifiStage::ApOnly);
        assert_eq!(snapshot.last_wifi_reason_code, None);
    }

    #[test]
    fn network_runtime_snapshot_tracks_sta_ip_and_failure_stage() {
        let _guard = test_state_guard();
        set_network_sta_expected(true, true);
        set_network_wifi_stage(NetworkWifiStage::StaAuthFailed, Some(202));
        clear_wifi_sta_state();

        let failed = network_runtime_snapshot(false, 3);
        assert!(failed.sta_expected);
        assert!(failed.sta_configured);
        assert_eq!(failed.last_wifi_stage, NetworkWifiStage::StaAuthFailed);
        assert_eq!(failed.last_wifi_reason_code, Some(202));
        assert!(!failed.sta_ip_present);

        set_wifi_sta_state(true, Some("192.168.1.2".to_string()));
        let ready = network_runtime_snapshot(true, 0);
        assert_eq!(ready.last_wifi_stage, NetworkWifiStage::StaIpReady);
        assert!(ready.sta_l2_connected);
        assert!(ready.sta_ip_present);
        assert!(ready.outbound_settled);
        assert!(ready.wall_clock_trustworthy);
    }

    #[test]
    fn clearing_sta_ip_does_not_leave_stale_ready_stage_or_l2_state() {
        let _guard = test_state_guard();
        set_network_sta_expected(true, true);
        set_wifi_sta_state(true, Some("192.168.1.2".to_string()));

        clear_wifi_sta_state();
        let snapshot = network_runtime_snapshot(false, 0);

        assert!(!snapshot.sta_l2_connected);
        assert!(!snapshot.sta_ip_present);
        assert_eq!(snapshot.last_wifi_stage, NetworkWifiStage::StaRecovering);
    }

    #[test]
    fn reasoned_wifi_failure_is_visible_until_next_connect_attempt() {
        let _guard = test_state_guard();
        set_network_sta_expected(true, true);
        set_network_wifi_stage(NetworkWifiStage::StaApNotFound, Some(201));

        assert!(network_last_wifi_stage_has_reasoned_failure());
        clear_wifi_sta_state();
        let failed = network_runtime_snapshot(false, 0);
        assert_eq!(failed.last_wifi_stage, NetworkWifiStage::StaApNotFound);
        assert_eq!(failed.last_wifi_reason_code, Some(201));

        set_network_wifi_stage(NetworkWifiStage::StaConnecting, None);
        assert!(!network_last_wifi_stage_has_reasoned_failure());
    }

    #[test]
    fn voice_exclusive_flag_round_trips() {
        let _guard = test_state_guard();
        set_voice_exclusive_active(true);
        assert!(voice_exclusive_active());
        set_voice_exclusive_active(false);
        assert!(!voice_exclusive_active());
    }

    #[test]
    fn background_maintenance_flag_round_trips() {
        let _guard = test_state_guard();
        set_background_maintenance_active(true);
        assert!(background_maintenance_active());
        set_background_maintenance_active(false);
        assert!(!background_maintenance_active());
    }

    #[test]
    fn config_plane_flag_round_trips() {
        let _guard = test_state_guard();
        set_config_plane_active(true);
        assert!(config_plane_active());
        set_config_plane_active(false);
        assert!(!config_plane_active());
    }

    #[test]
    fn boot_phase_flag_round_trips() {
        let _guard = test_state_guard();
        set_boot_phase_active(true);
        assert!(boot_phase_active());
        set_boot_phase_active(false);
        assert!(!boot_phase_active());
    }

    #[test]
    fn pairing_flags_round_trip() {
        let _guard = test_state_guard();
        set_pairing_state_known(true);
        set_pairing_required(true);
        assert!(pairing_state_known());
        assert!(pairing_required());
        set_pairing_required(false);
        assert!(!pairing_required());
    }

    #[test]
    fn recovery_safe_mode_flag_round_trips() {
        let _guard = test_state_guard();
        set_recovery_safe_mode_active(true);
        assert!(recovery_safe_mode_active());
        set_recovery_safe_mode_active(false);
        assert!(!recovery_safe_mode_active());
    }

    #[test]
    fn upgrade_active_flag_round_trips() {
        let _guard = test_state_guard();
        set_upgrade_active(true);
        assert!(upgrade_active());
        set_upgrade_active(false);
        assert!(!upgrade_active());
    }

    #[test]
    fn esp_operator_window_expires_and_can_be_cleared() {
        let _guard = test_state_guard();
        clear_esp_operator_window();
        assert!(!esp_operator_window_active());

        let expires_at = open_esp_operator_window(5);
        assert!(expires_at >= now_unix_secs());
        assert!(esp_operator_window_active());
        assert!(esp_operator_window_until().is_some());

        clear_esp_operator_window();
        assert!(!esp_operator_window_active());
        assert!(esp_operator_window_until().is_none());
    }
}
