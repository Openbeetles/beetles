//! 进程内共享状态：最近错误等，供 CLI 与 HTTP /api/health 共用。
//! In-process shared state (e.g. last error) for CLI and HTTP.

use crate::error::Error;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
/// 当前 WiFi STA IPv4。
static WIFI_STA_IP: OnceLock<Mutex<Option<String>>> = OnceLock::new();

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

/// 最近一次 memory 加载是否成功。
pub fn get_memory_load_ok() -> bool {
    MEMORY_LOAD_OK.load(Ordering::Relaxed)
}

/// 最近一次 soul 加载是否成功。
pub fn get_soul_load_ok() -> bool {
    SOUL_LOAD_OK.load(Ordering::Relaxed)
}

/// 更新当前 WiFi STA 状态；业务域只读此状态，不直接依赖 platform helper。
pub fn set_wifi_sta_state(connected: bool, ip: Option<String>) {
    WIFI_STA_CONNECTED.store(connected, Ordering::Relaxed);
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

/// 当前 WiFi STA IPv4。
pub fn wifi_sta_ip() -> Option<String> {
    WIFI_STA_IP
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|g| g.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_error_expires_without_erasing_last_error_history() {
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
        set_wifi_sta_state(true, Some("192.168.1.2".to_string()));
        assert!(wifi_sta_connected());
        assert_eq!(wifi_sta_ip().as_deref(), Some("192.168.1.2"));

        clear_wifi_sta_state();
        assert!(!wifi_sta_connected());
        assert_eq!(wifi_sta_ip(), None);
    }
}
