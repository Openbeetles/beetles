//! GET /api/system_info：供系统信息页展示用，返回 product_name、system_status、current_time、firmware_version、locale、lan_ip。
//! `current_time`：Host 用系统时钟；ESP 在 SNTP 同步后由 `util::current_unix_secs()` 提供 UTC 字符串，未同步时返回 "—"。
//! `lan_ip`：STA 模式下路由器 DHCP 分配的 IPv4（点分十进制）；未连接或无地址时为 "—"。
//! `board_id`：运行期拼装（ESP：`esp_chip_info`+Flash 与 manifest 档位对齐；Linux：`linux`）。`hardware_model`：ESP 为摘要句；Linux 为设备树/DMI 等（若有）。

use super::HandlerContext;
use crate::config;
use crate::i18n::{locale_from_store, tr, Message};
use crate::platform::http_server::common::to_io;
use crate::state;
use std::sync::atomic::Ordering;

/// SNTP 未同步时系统时间多在 1970 附近；低于此阈值不在 API 中冒充墙钟。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const MIN_TRUSTWORTHY_UNIX_SECS: u64 = 1577836800; // 2020-01-01 00:00:00 UTC

fn current_unix_secs_wallclock() -> Option<u64> {
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
    }
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        Some(crate::util::current_unix_secs())
    }
}

fn format_unix_utc(secs: u64) -> String {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    if secs < MIN_TRUSTWORTHY_UNIX_SECS {
        return "—".to_string();
    }
    let t = secs % 86400;
    let h = (t / 3600) as u32;
    let m = (t % 3600 / 60) as u32;
    let s = (t % 60) as u32;
    let d = secs / 86400;
    let (y, mo, day) = days_to_ymd(d);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        y, mo, day, h, m, s
    )
}

fn current_time_str() -> String {
    match current_unix_secs_wallclock() {
        Some(secs) => format_unix_utc(secs),
        None => "—".to_string(),
    }
}

fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    let mut d = days;
    let mut y = 1970u32;
    let is_leap = |y: u32| (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400);
    let days_in_year = |y: u32| if is_leap(y) { 366 } else { 365 };
    while d >= days_in_year(y) as u64 {
        d -= days_in_year(y) as u64;
        y += 1;
    }
    let mon_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mon_days = mon_days;
    if is_leap(y) {
        mon_days[1] = 29;
    }
    let mut m = 0usize;
    let mut acc = 0u64;
    while m < 12 && acc + mon_days[m] <= d {
        acc += mon_days[m];
        m += 1;
    }
    let (mo, day) = if m >= 12 {
        let dec_start = acc - mon_days[11];
        (12u32, ((d - dec_start + 1) as u32).min(mon_days[11] as u32))
    } else {
        let day_raw = (d - acc + 1) as u32;
        (m as u32 + 1, day_raw.min(mon_days[m] as u32))
    };
    (y, mo, day)
}

/// 生成 system_info JSON：product_name, system_status, current_time, firmware_version, lan_ip 等。
pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let memory_loaded = ctx.memory_store.get_memory().is_ok();
    let soul_loaded = ctx.memory_store.get_soul().is_ok();
    let storage_ok = memory_loaded || soul_loaded;
    let last_error = state::get_last_error();
    let inc = ctx.inbound_depth.load(Ordering::Relaxed);
    let out = ctx.outbound_depth.load(Ordering::Relaxed);
    let sta_up = crate::platform::is_wifi_sta_connected();
    let loc = locale_from_store(ctx.config_store.as_ref());
    let system_status = if sta_up && storage_ok && last_error.is_none() && inc <= 6 && out <= 6 {
        tr(Message::SystemStatusOk, loc)
    } else if !sta_up {
        tr(Message::SystemStatusWifiDisconnected, loc)
    } else if !storage_ok {
        tr(Message::SystemStatusStorage, loc)
    } else if last_error.is_some() {
        tr(Message::SystemStatusChannel, loc)
    } else {
        tr(Message::SystemStatusRunning, loc)
    };
    let product_name = "beetle";
    let current_time = current_time_str();
    let firmware_version = ctx.version.as_ref();
    let ota_available = cfg!(feature = "ota");
    let locale = config::get_locale(ctx.config_store.as_ref());
    let lan_ip = crate::platform::wifi::wifi_sta_ip().unwrap_or_else(|| "—".to_string());
    #[allow(unused_mut)]
    let mut json = serde_json::json!({
        "product_name": product_name,
        "system_status": system_status,
        "current_time": current_time,
        "firmware_version": firmware_version,
        "board_id": ctx.board_id.as_ref(),
        "ota_available": ota_available,
        "locale": locale,
        "lan_ip": lan_ip,
    });

    // Linux 特有字段
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        if let Some(obj) = json.as_object_mut() {
            obj.insert("os_type".to_string(), serde_json::json!("Linux"));
            if let Some(kernel) = get_kernel_version() {
                obj.insert("kernel_version".to_string(), serde_json::json!(kernel));
            }
            if let Some(cpu) = get_cpu_model() {
                obj.insert("cpu_model".to_string(), serde_json::json!(cpu));
            }
            obj.insert("cpu_cores".to_string(), serde_json::json!(get_cpu_cores()));
        }
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let hw = crate::platform::runtime_board::hardware_summary_line();
        if let Some(obj) = json.as_object_mut() {
            obj.insert("hardware_model".to_string(), serde_json::json!(hw));
        }
    }

    #[cfg(all(
        not(any(target_arch = "xtensa", target_arch = "riscv32")),
        target_os = "linux"
    ))]
    {
        if let Some(hw) = crate::platform::board_info::linux_machine_display_name() {
            if hw != ctx.board_id.as_ref() {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("hardware_model".to_string(), serde_json::json!(hw));
                }
            }
        }
    }

    serde_json::to_string(&json).map_err(to_io)
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn get_kernel_version() -> Option<String> {
    use std::fs;
    fs::read_to_string("/proc/version")
        .ok()
        .and_then(|s| s.split_whitespace().nth(2).map(String::from))
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn get_cpu_model() -> Option<String> {
    use std::fs;
    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        for line in content.lines() {
            if line.starts_with("model name") {
                if let Some(model) = line.split(':').nth(1) {
                    return Some(model.trim().to_string());
                }
            }
        }
    }
    None
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn get_cpu_cores() -> u32 {
    use std::fs;
    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        return content
            .lines()
            .filter(|l| l.starts_with("processor"))
            .count() as u32;
    }
    1
}
