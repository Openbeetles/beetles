//! ESP runtime policy helpers shared by hardware-facing workers.
//! ESP 运行态策略：收口常驻任务的看门狗友好等待边界。

use std::time::Duration;

/// Maximum time an ESP native task may block before it gets a chance to feed TWDT.
/// ESP native task 阻塞等待的统一上限，超过该窗口必须回到循环喂狗。
pub(crate) const ESP_TASK_WDT_IDLE_POLL: Duration = Duration::from_millis(500);

/// Bound a blocking wait so TWDT-managed native tasks never sleep indefinitely.
/// 将阻塞等待压到统一窗口内；`None` 表示原本会无限等待。
pub(crate) fn bounded_watchdog_wait(requested: Option<Duration>) -> Duration {
    requested
        .filter(|wait| *wait < ESP_TASK_WDT_IDLE_POLL)
        .unwrap_or(ESP_TASK_WDT_IDLE_POLL)
}

/// Keep a compatibility guard around opaque ESP control-plane work.
///
/// Older versions temporarily removed and re-added the current task from the
/// IDF task watchdog. That mutates IDF's global TWDT subscription list on the
/// HTTP route hot path and can corrupt the list when config-ui fan-out overlaps
/// other watchdog feeds. The guard now only feeds at the boundary; long waits
/// must be split with [`bounded_watchdog_wait`] instead of changing
/// subscription state.
pub(crate) struct TaskWdtSubscriptionPause;

impl TaskWdtSubscriptionPause {
    pub(crate) fn current_task() -> Self {
        crate::platform::task_wdt::feed_current_task();
        Self
    }
}

impl Drop for TaskWdtSubscriptionPause {
    fn drop(&mut self) {
        crate::platform::task_wdt::feed_current_task();
    }
}
