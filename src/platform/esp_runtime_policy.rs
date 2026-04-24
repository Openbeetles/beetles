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

/// Temporarily remove the current task from TWDT while running opaque work that
/// cannot cooperatively feed the watchdog.
///
/// The task is re-subscribed when the guard is dropped. Use this only around
/// control-plane work whose caller already has an explicit timeout.
pub(crate) struct TaskWdtSubscriptionPause;

impl TaskWdtSubscriptionPause {
    pub(crate) fn current_task() -> Self {
        crate::platform::task_wdt::feed_current_task();
        crate::platform::task_wdt::unregister_current_task_from_task_wdt();
        Self
    }
}

impl Drop for TaskWdtSubscriptionPause {
    fn drop(&mut self) {
        crate::platform::task_wdt::register_current_task_to_task_wdt();
        crate::platform::task_wdt::feed_current_task();
    }
}
