//! 进程内共享状态：最近错误等，供 CLI 与 HTTP /api/health 共用。
//! In-process shared state (e.g. last error) for CLI and HTTP.

use crate::error::Error;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const LAST_ERRORS_COUNT_CAP: usize = 10;
const CURRENT_ERROR_TTL_SECS: u64 = 60;

static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);
static CURRENT_ERROR: Mutex<Option<TimedError>> = Mutex::new(None);
static LAST_ERRORS_COUNT: AtomicUsize = AtomicUsize::new(0);
/// 最近一次 memory 加载是否成功（由 build_context 等设置，供 diagnose/health 暴露）。
static MEMORY_LOAD_OK: AtomicBool = AtomicBool::new(false);
/// 最近一次 soul 加载是否成功。
static SOUL_LOAD_OK: AtomicBool = AtomicBool::new(false);
/// 当前 WiFi STA 是否已拿到有效 IP。
static WIFI_STA_CONNECTED: AtomicBool = AtomicBool::new(false);
/// 当前 WiFi STA 连通状态建立时间（unix secs）；供外联探测做短暂 settle window。
static WIFI_STA_CONNECTED_SINCE_SECS: AtomicU32 = AtomicU32::new(0);
/// 当前 WiFi STA IPv4。
static WIFI_STA_IP: OnceLock<Mutex<Option<String>>> = OnceLock::new();
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
/// ESP operator / deep-inspection window expiry timestamp.
static ESP_OPERATOR_WINDOW_UNTIL_SECS: AtomicU32 = AtomicU32::new(0);

#[derive(Clone)]
struct TimedError {
    message: String,
    seen_at_secs: u64,
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

/// 设置最近一次 memory 加载结果（build_context 等调用，供可观测性）。
pub fn set_memory_load_ok(ok: bool) {
    MEMORY_LOAD_OK.store(ok, Ordering::Relaxed);
}

/// 设置最近一次 soul 加载结果。
pub fn set_soul_load_ok(ok: bool) {
    SOUL_LOAD_OK.store(ok, Ordering::Relaxed);
}

/// 更新当前 WiFi STA 状态；业务域只读此状态，不直接依赖 platform helper。
pub fn set_wifi_sta_state(connected: bool, ip: Option<String>) {
    let was_connected = WIFI_STA_CONNECTED.swap(connected, Ordering::Relaxed);
    if connected {
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
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn current_error_expires_without_erasing_last_error_history() {
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
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
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
        set_wifi_sta_state(true, Some("192.168.1.2".to_string()));
        assert!(wifi_sta_connected());
        assert_eq!(wifi_sta_ip().as_deref(), Some("192.168.1.2"));

        clear_wifi_sta_state();
        assert!(!wifi_sta_connected());
        assert_eq!(wifi_sta_ip(), None);
    }

    #[test]
    fn wifi_sta_must_settle_before_outbound_ready() {
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
        set_wifi_sta_state(true, Some("192.168.1.2".to_string()));
        assert!(!wifi_sta_settled_for_outbound(1));
        clear_wifi_sta_state();
    }

    #[test]
    fn voice_exclusive_flag_round_trips() {
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
        set_voice_exclusive_active(true);
        assert!(voice_exclusive_active());
        set_voice_exclusive_active(false);
        assert!(!voice_exclusive_active());
    }

    #[test]
    fn background_maintenance_flag_round_trips() {
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
        set_background_maintenance_active(true);
        assert!(background_maintenance_active());
        set_background_maintenance_active(false);
        assert!(!background_maintenance_active());
    }

    #[test]
    fn config_plane_flag_round_trips() {
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
        set_config_plane_active(true);
        assert!(config_plane_active());
        set_config_plane_active(false);
        assert!(!config_plane_active());
    }

    #[test]
    fn boot_phase_flag_round_trips() {
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
        set_boot_phase_active(true);
        assert!(boot_phase_active());
        set_boot_phase_active(false);
        assert!(!boot_phase_active());
    }

    #[test]
    fn pairing_flags_round_trip() {
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
        set_pairing_state_known(true);
        set_pairing_required(true);
        assert!(pairing_state_known());
        assert!(pairing_required());
        set_pairing_required(false);
        assert!(!pairing_required());
    }

    #[test]
    fn recovery_safe_mode_flag_round_trips() {
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
        set_recovery_safe_mode_active(true);
        assert!(recovery_safe_mode_active());
        set_recovery_safe_mode_active(false);
        assert!(!recovery_safe_mode_active());
    }

    #[test]
    fn esp_operator_window_expires_and_can_be_cleared() {
        let _guard = test_lock().lock().unwrap_or_else(|e| e.into_inner());
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
