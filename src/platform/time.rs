//! 单调时间抽象：ESP 用 esp_timer_get_time；host 侧区分宿主机 uptime 与 beetle 进程 uptime。
//! Monotonic time: ESP via esp_timer_get_time; host distinguishes host uptime from beetle process uptime.

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
