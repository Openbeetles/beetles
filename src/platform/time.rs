//! 单调时间抽象：ESP 用 esp_timer_get_time；host 侧区分宿主机 uptime 与 beetle 进程 uptime。
//! Monotonic time: ESP via esp_timer_get_time; host distinguishes host uptime from beetle process uptime.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// 低于该阈值的墙钟一律视为“不可信”，避免把 1970 之类未同步时间当作真实 UTC。
/// Treat wall-clock values below this threshold as unsynchronized / untrustworthy.
pub const TRUSTWORTHY_WALL_CLOCK_THRESHOLD_SECS: u64 = 1_700_000_000;
const WALL_CLOCK_WAIT_POLL_SLICE_MS: u64 = 250;

fn wall_clock_wake_generation() -> &'static AtomicU32 {
    static GENERATION: AtomicU32 = AtomicU32::new(0);
    &GENERATION
}

/// 系统启动后经过的秒数（单调递增）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn uptime_secs() -> u64 {
    let us = unsafe { esp_idf_svc::sys::esp_timer_get_time() };
    if us >= 0 {
        us as u64 / 1_000_000
    } else {
        0
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn process_start_instant() -> &'static std::time::Instant {
    use std::sync::OnceLock;
    use std::time::Instant;

    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now)
}

/// beetle 进程启动后的运行秒数。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn app_uptime_secs() -> u64 {
    uptime_secs()
}

/// beetle 进程启动后的运行秒数。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn app_uptime_secs() -> u64 {
    process_start_instant().elapsed().as_secs()
}

/// Linux：读 `/proc/uptime` 获取宿主机内核运行时间（与 beetle 进程重启无关）；
/// 其它 host（macOS/Windows CI）：回退到 beetle 进程 uptime。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn host_uptime_secs() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = std::fs::read_to_string("/proc/uptime") {
            if let Some(secs_str) = s.split_whitespace().next() {
                if let Ok(f) = secs_str.parse::<f64>() {
                    return f as u64;
                }
            }
        }
    }
    app_uptime_secs()
}

/// beetle 运行秒数（对外日志 / API / 显示统一口径）。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn uptime_secs() -> u64 {
    app_uptime_secs()
}

/// 当前墙钟 UNIX 秒；仅表示系统当前时间读数，不代表它已经可信。
pub fn wall_clock_unix_secs() -> Option<u64> {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// 判断给定墙钟值是否已超过“可用于 TLS / UTC 展示”的可信阈值。
pub const fn is_trustworthy_wall_clock_secs(secs: u64) -> bool {
    secs >= TRUSTWORTHY_WALL_CLOCK_THRESHOLD_SECS
}

/// 当前墙钟是否已经可信。
pub fn wall_clock_is_trustworthy() -> bool {
    wall_clock_unix_secs().is_some_and(is_trustworthy_wall_clock_secs)
}

/// 当前可信墙钟 UNIX 秒；未同步或回拨到异常范围时返回 None。
pub fn trusted_wall_clock_unix_secs() -> Option<u64> {
    wall_clock_unix_secs().filter(|secs| is_trustworthy_wall_clock_secs(*secs))
}

/// 通知等待者墙钟可能已经可信；等待者仍会重新读取系统时间并应用可信阈值。
/// Notify waiters that the wall clock may now be trustworthy; waiters still re-check the threshold.
pub fn notify_wall_clock_trustworthy() {
    wall_clock_wake_generation().fetch_add(1, Ordering::Release);
    crate::bg_timer::notify_deadline_changed();
}

/// 等待墙钟变为可信，直到收到同步通知或超时；返回前始终重新检查可信阈值。
/// Wait for a trusted wall clock until notified or timed out; always re-checks the threshold.
pub fn wait_for_wall_clock_trustworthy(timeout: Duration) -> bool {
    wait_for_wall_clock_trustworthy_with(
        timeout,
        Duration::from_millis(WALL_CLOCK_WAIT_POLL_SLICE_MS),
        wall_clock_is_trustworthy,
    )
}

fn wait_for_wall_clock_trustworthy_with<F>(
    timeout: Duration,
    poll_slice: Duration,
    mut is_trustworthy: F,
) -> bool
where
    F: FnMut() -> bool,
{
    if is_trustworthy() {
        return true;
    }
    if timeout.is_zero() {
        return false;
    }

    let deadline = Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(Instant::now);
    let mut observed_generation = wall_clock_wake_generation().load(Ordering::Acquire);
    let poll_slice = if poll_slice.is_zero() {
        timeout
    } else {
        poll_slice
    };

    loop {
        if is_trustworthy() {
            return true;
        }

        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return false;
        };

        let generation = wall_clock_wake_generation().load(Ordering::Acquire);
        if generation != observed_generation {
            observed_generation = generation;
            continue;
        }

        std::thread::sleep(remaining.min(poll_slice));
        crate::platform::task_wdt::feed_current_task();
    }
}

#[cfg(test)]
mod tests {
    use super::{is_trustworthy_wall_clock_secs, TRUSTWORTHY_WALL_CLOCK_THRESHOLD_SECS};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    };
    use std::time::Duration;

    #[test]
    fn trustworthy_wall_clock_threshold_rejects_epoch_like_values() {
        assert!(!is_trustworthy_wall_clock_secs(0));
        assert!(!is_trustworthy_wall_clock_secs(
            TRUSTWORTHY_WALL_CLOCK_THRESHOLD_SECS - 1
        ));
        assert!(is_trustworthy_wall_clock_secs(
            TRUSTWORTHY_WALL_CLOCK_THRESHOLD_SECS
        ));
    }

    #[test]
    fn wall_clock_wait_wakes_when_trustworthy_time_is_notified() {
        let trustworthy = Arc::new(AtomicBool::new(false));
        let waiter_trustworthy = Arc::clone(&trustworthy);
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let woke = super::wait_for_wall_clock_trustworthy_with(
                Duration::from_secs(5),
                Duration::from_millis(10),
                || waiter_trustworthy.load(Ordering::SeqCst),
            );
            tx.send(woke).unwrap();
        });

        assert!(rx.recv_timeout(Duration::from_millis(5)).is_err());
        trustworthy.store(true, Ordering::SeqCst);
        super::notify_wall_clock_trustworthy();
        assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap());
    }
}
