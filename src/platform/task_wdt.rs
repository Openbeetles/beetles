//! 任务看门狗：将当前任务加入 TWDT，使 HTTP 请求与空闲等待时 feed 有效，避免 "task not found"。
//! Task watchdog: add current task to TWDT so feed/reset during HTTP or idle recv_timeout is valid.

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
/// Task watchdog ownership policy for a runtime thread.
///
/// `Owner` threads are long-lived and may subscribe to ESP-IDF TWDT. `FeedOnly`
/// threads may call feed at safe boundaries, but must not mutate the TWDT task
/// list. `Unmanaged` threads have no watchdog contract.
pub enum TaskWdtThreadPolicy {
    Owner,
    FeedOnly,
    Unmanaged,
}

/// Return the TWDT policy for a named runtime thread.
///
/// This is intentionally a small explicit list. Native-task surface is not an
/// ownership signal: short-lived native workers must stay feed-only/unmanaged.
pub fn thread_policy_for_name(name: &str) -> TaskWdtThreadPolicy {
    match name {
        "agent_loop" | "wifi_worker" | "audio_io_worker" | "runtime_bootstrap" => {
            TaskWdtThreadPolicy::Owner
        }
        "http_snapshot_exec"
        | "http_config_exec"
        | "http_diag_exec"
        | "http_ota_exec"
        | "dispatch"
        | "os_outbound"
        | "bg_timer"
        | "heartbeat"
        | "qq_ws"
        | "feishu_ws"
        | "wecom_aibot"
        | "dingtalk_stream"
        | "qq_sender"
        | "tg_sender"
        | "fs_sender"
        | "dt_sender"
        | "wc_sender"
        | "tg_poll"
        | "voice_session"
        | "voice_session_worker"
        | "voice_realtime"
        | "voice_realtime_connect"
        | "display" => TaskWdtThreadPolicy::FeedOnly,
        _ => TaskWdtThreadPolicy::Unmanaged,
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const ESP_OK: i32 = 0;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const ESP_ERR_INVALID_ARG: i32 = 0x102;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const ESP_ERR_INVALID_STATE: i32 = 0x103;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const ESP_ERR_NOT_FOUND: i32 = 0x105;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TaskWdtSubscriptionState {
    Subscribed,
    NotSubscribed,
    Uninitialized,
    Unknown(i32),
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
fn classify_subscription_status(status: i32) -> TaskWdtSubscriptionState {
    match status {
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        ESP_OK => TaskWdtSubscriptionState::Subscribed,
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        ESP_ERR_NOT_FOUND => TaskWdtSubscriptionState::NotSubscribed,
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        ESP_ERR_INVALID_STATE => TaskWdtSubscriptionState::Uninitialized,
        other => TaskWdtSubscriptionState::Unknown(other),
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn current_task_handle() -> esp_idf_svc::sys::TaskHandle_t {
    unsafe { esp_idf_svc::sys::xTaskGetCurrentTaskHandle() }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
thread_local! {
    static CURRENT_TASK_WDT_OWNER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn mark_current_task_wdt_owner(owner: bool) {
    CURRENT_TASK_WDT_OWNER.with(|slot| slot.set(owner));
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn current_task_is_wdt_owner() -> bool {
    CURRENT_TASK_WDT_OWNER.with(|slot| slot.get())
}

/// 将当前任务加入任务看门狗。在运行 agent 循环（会发起长时间 HTTP）的线程中调用一次即可。
/// 幂等：同一任务多次调用安全。IDF 5+ 先查 `esp_task_wdt_status`，已订阅则不再 `add`，避免 IDF 侧
/// `task is already subscribed`（`esp_task_wdt_add` 返回 `ESP_ERR_INVALID_ARG` / 258）。IDF 4 无 status API
/// 时仅调用 `add`，并对 `INVALID_ARG`、`INVALID_STATE` 静默。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn register_current_task_to_task_wdt() {
    let current = current_task_handle();
    if current.is_null() {
        return;
    }

    #[cfg(not(esp_idf_version_major = "4"))]
    match classify_subscription_status(unsafe { esp_idf_svc::sys::esp_task_wdt_status(current) }) {
        TaskWdtSubscriptionState::Subscribed => {
            mark_current_task_wdt_owner(true);
            return;
        }
        TaskWdtSubscriptionState::NotSubscribed | TaskWdtSubscriptionState::Uninitialized => {}
        TaskWdtSubscriptionState::Unknown(code) => {
            log::warn!("[platform::task_wdt] esp_task_wdt_status failed: {}", code);
        }
    }

    let ret = unsafe { esp_idf_svc::sys::esp_task_wdt_add(current) };
    if ret == ESP_OK || ret == ESP_ERR_INVALID_ARG {
        mark_current_task_wdt_owner(true);
        return;
    }
    if ret != ESP_OK && ret != ESP_ERR_INVALID_ARG && ret != ESP_ERR_INVALID_STATE {
        log::warn!("[platform::task_wdt] esp_task_wdt_add failed: {}", ret);
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn register_current_task_to_task_wdt() {}

/// 当前任务退出前从 TWDT 取消订阅，避免短生命周期 pthread 残留在看门狗里。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn unregister_current_task_from_task_wdt() {
    let current = current_task_handle();
    if current.is_null() {
        return;
    }

    #[cfg(not(esp_idf_version_major = "4"))]
    match classify_subscription_status(unsafe { esp_idf_svc::sys::esp_task_wdt_status(current) }) {
        TaskWdtSubscriptionState::Subscribed => {}
        TaskWdtSubscriptionState::NotSubscribed | TaskWdtSubscriptionState::Uninitialized => {
            mark_current_task_wdt_owner(false);
            return;
        }
        TaskWdtSubscriptionState::Unknown(code) => {
            log::warn!(
                "[platform::task_wdt] esp_task_wdt_status failed before delete: {}",
                code
            );
            mark_current_task_wdt_owner(false);
            return;
        }
    }

    let ret = unsafe { esp_idf_svc::sys::esp_task_wdt_delete(current) };
    mark_current_task_wdt_owner(false);
    if ret != ESP_OK && ret != ESP_ERR_INVALID_ARG && ret != ESP_ERR_INVALID_STATE {
        log::warn!("[platform::task_wdt] esp_task_wdt_delete failed: {}", ret);
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn unregister_current_task_from_task_wdt() {}

/// 喂当前任务看门狗。agent 循环在 recv_timeout 超时后调用，避免长时间等消息时触发 TWDT。
#[cfg(all(
    any(target_arch = "xtensa", target_arch = "riscv32"),
    esp_idf_version_major = "4"
))]
pub fn feed_current_task() {
    if !current_task_is_wdt_owner() {
        return;
    }
    unsafe {
        let _ = esp_idf_svc::sys::esp_task_wdt_feed();
    }
}

#[cfg(all(
    any(target_arch = "xtensa", target_arch = "riscv32"),
    not(esp_idf_version_major = "4")
))]
pub fn feed_current_task() {
    if !current_task_is_wdt_owner() {
        return;
    }
    let current = current_task_handle();
    if current.is_null() {
        return;
    }

    match classify_subscription_status(unsafe { esp_idf_svc::sys::esp_task_wdt_status(current) }) {
        TaskWdtSubscriptionState::Subscribed => {}
        TaskWdtSubscriptionState::NotSubscribed | TaskWdtSubscriptionState::Uninitialized => {
            return;
        }
        TaskWdtSubscriptionState::Unknown(code) => {
            log::warn!(
                "[platform::task_wdt] esp_task_wdt_status failed before reset: {}",
                code
            );
            return;
        }
    }

    unsafe {
        let ret = esp_idf_svc::sys::esp_task_wdt_reset();
        if ret != ESP_OK && ret != ESP_ERR_NOT_FOUND {
            log::warn!("[platform::task_wdt] esp_task_wdt_reset failed: {}", ret);
        }
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn feed_current_task() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_subscription_status_maps_core_states() {
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        {
            assert_eq!(
                classify_subscription_status(ESP_OK),
                TaskWdtSubscriptionState::Subscribed
            );
            assert_eq!(
                classify_subscription_status(ESP_ERR_NOT_FOUND),
                TaskWdtSubscriptionState::NotSubscribed
            );
            assert_eq!(
                classify_subscription_status(ESP_ERR_INVALID_STATE),
                TaskWdtSubscriptionState::Uninitialized
            );
        }
    }

    #[test]
    fn classify_subscription_status_preserves_unknown_errors() {
        assert_eq!(
            classify_subscription_status(-77),
            TaskWdtSubscriptionState::Unknown(-77)
        );
    }

    #[test]
    fn subscription_state_variants_are_exercised_in_unit_tests() {
        let _ = TaskWdtSubscriptionState::Subscribed;
        let _ = TaskWdtSubscriptionState::NotSubscribed;
        let _ = TaskWdtSubscriptionState::Uninitialized;
    }

    #[test]
    fn thread_policy_uses_explicit_owner_allowlist() {
        assert_eq!(
            thread_policy_for_name("agent_loop"),
            TaskWdtThreadPolicy::Owner
        );
        assert_eq!(
            thread_policy_for_name("wifi_worker"),
            TaskWdtThreadPolicy::Owner
        );
        assert_eq!(
            thread_policy_for_name("audio_io_worker"),
            TaskWdtThreadPolicy::Owner
        );
        assert_eq!(
            thread_policy_for_name("runtime_bootstrap"),
            TaskWdtThreadPolicy::Owner
        );
        assert_eq!(
            thread_policy_for_name("runtime_guard"),
            TaskWdtThreadPolicy::Unmanaged
        );
        assert_eq!(
            thread_policy_for_name("http_snapshot_exec"),
            TaskWdtThreadPolicy::FeedOnly
        );
        assert_eq!(
            thread_policy_for_name("http_config_exec"),
            TaskWdtThreadPolicy::FeedOnly
        );
        assert_eq!(
            thread_policy_for_name("os_outbound"),
            TaskWdtThreadPolicy::FeedOnly
        );
        assert_eq!(
            thread_policy_for_name("voice_session_worker"),
            TaskWdtThreadPolicy::FeedOnly
        );
        assert_eq!(
            thread_policy_for_name("native_worker"),
            TaskWdtThreadPolicy::Unmanaged
        );
    }
}
