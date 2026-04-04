//! 任务看门狗：将当前任务加入 TWDT，使 HTTP 请求与空闲等待时 feed 有效，避免 "task not found"。
//! Task watchdog: add current task to TWDT so feed/reset during HTTP or idle recv_timeout is valid.

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const ESP_OK: i32 = 0;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const ESP_ERR_INVALID_ARG: i32 = 0x102;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const ESP_ERR_INVALID_STATE: i32 = 0x103;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const ESP_ERR_NOT_FOUND: i32 = 0x105;

/// 将当前任务加入任务看门狗。在运行 agent 循环（会发起长时间 HTTP）的线程中调用一次即可。
/// 幂等：同一任务多次调用安全。IDF 5+ 先查 `esp_task_wdt_status`，已订阅则不再 `add`，避免 IDF 侧
/// `task is already subscribed`（`esp_task_wdt_add` 返回 `ESP_ERR_INVALID_ARG` / 258）。IDF 4 无 status API
/// 时仅调用 `add`，并对 `INVALID_ARG`、`INVALID_STATE` 静默。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn register_current_task_to_task_wdt() {
    #[cfg(not(esp_idf_version_major = "4"))]
    {
        let st = unsafe { esp_idf_svc::sys::esp_task_wdt_status(core::ptr::null_mut()) };
        if st == ESP_OK {
            return;
        }
        if st != ESP_ERR_NOT_FOUND && st != ESP_ERR_INVALID_STATE {
            log::warn!("[platform::task_wdt] esp_task_wdt_status failed: {}", st);
        }
    }

    let ret = unsafe { esp_idf_svc::sys::esp_task_wdt_add(core::ptr::null_mut()) };
    if ret == ESP_OK || ret == ESP_ERR_INVALID_ARG {
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
    #[cfg(not(esp_idf_version_major = "4"))]
    {
        let st = unsafe { esp_idf_svc::sys::esp_task_wdt_status(core::ptr::null_mut()) };
        if st == ESP_ERR_NOT_FOUND || st == ESP_ERR_INVALID_STATE {
            return;
        }
        if st != ESP_OK {
            log::warn!(
                "[platform::task_wdt] esp_task_wdt_status failed before delete: {}",
                st
            );
            return;
        }
    }

    let ret = unsafe { esp_idf_svc::sys::esp_task_wdt_delete(core::ptr::null_mut()) };
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
    unsafe {
        let _ = esp_idf_svc::sys::esp_task_wdt_feed();
    }
}

#[cfg(all(
    any(target_arch = "xtensa", target_arch = "riscv32"),
    not(esp_idf_version_major = "4")
))]
pub fn feed_current_task() {
    let st = unsafe { esp_idf_svc::sys::esp_task_wdt_status(core::ptr::null_mut()) };
    if st == ESP_ERR_NOT_FOUND || st == ESP_ERR_INVALID_STATE {
        return;
    }
    if st != ESP_OK {
        log::warn!(
            "[platform::task_wdt] esp_task_wdt_status failed before reset: {}",
            st
        );
        return;
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
