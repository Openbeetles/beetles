//! 单调时间抽象：ESP 用 esp_timer_get_time，host 用 /proc/uptime 或进程 Instant。
//! Monotonic time: ESP via esp_timer_get_time; host via /proc/uptime or process Instant.

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

/// Linux：读 `/proc/uptime` 获取内核运行时间（与 systemd 重启无关）；
/// 其它 host（macOS/Windows CI）：回退到进程 Instant。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn uptime_secs() -> u64 {
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
    use std::sync::OnceLock;
    use std::time::Instant;
    static BOOT: OnceLock<Instant> = OnceLock::new();
    BOOT.get_or_init(Instant::now).elapsed().as_secs()
}
