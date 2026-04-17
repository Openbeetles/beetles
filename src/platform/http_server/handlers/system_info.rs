//! GET /api/system_info：供系统信息页展示用，返回设备摘要字段。
//! `current_time`：Host 用系统时钟；ESP 在 SNTP 同步后由 `util::current_unix_secs()` 提供 UTC 字符串，未同步时返回 "—"。
//! `lan_ip`：ESP 为 STA IPv4；Linux 为当前默认上行接口的 IPv4（点分十进制）；不可用时为 "—"。
//! `board_id`：运行期拼装（ESP：`esp_chip_info`+Flash 与 manifest 档位对齐；Linux：`linux`）。`hardware_model`：ESP 为摘要句；Linux 为设备树/DMI 等（若有）。

use super::HandlerContext;
use crate::config;
use crate::platform::http_server::common::to_io;

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

/// 生成 system_info JSON：product_name, current_time, firmware_version, lan_ip 等。
pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let product_name = "beetle";
    let current_time = current_time_str();
    let firmware_version = ctx.version.as_ref();
    let ota_available = cfg!(feature = "ota");
    let locale = config::get_locale(ctx.config_store.as_ref());
    let lan_ip = ctx.platform.lan_ipv4().unwrap_or_else(|| "—".to_string());
    #[allow(unused_mut)]
    let mut json = serde_json::json!({
        "product_name": product_name,
        "current_time": current_time,
        "firmware_version": firmware_version,
        "board_id": ctx.board_id.as_ref(),
        "ota_available": ota_available,
        "locale": locale,
        "lan_ip": lan_ip,
        "workflow": crate::runtime::workflow_audit_snapshot(8).summary,
        "programmable_reasoning": crate::programmable_reasoning_system_info_summary(),
    });

    if let Some(obj) = json.as_object_mut() {
        match ctx.platform.storage_media() {
            Ok(media) => {
                obj.insert("storage_media".to_string(), serde_json::json!(media));
            }
            Err(e) => {
                log::warn!("[system_info] storage media probe failed: {}", e);
                obj.insert("storage_media".to_string(), serde_json::json!([]));
                obj.insert(
                    "storage_media_error".to_string(),
                    serde_json::json!(e.to_string()),
                );
            }
        }
    }

    // Linux 特有字段——统一从 board_info 共享函数取值，避免重复解析 /proc。
    #[cfg(target_os = "linux")]
    {
        let host = crate::host_observability::collect_linux_host_observability(
            &crate::orchestrator::snapshot(),
        );
        if let Some(obj) = json.as_object_mut() {
            obj.insert("os_type".to_string(), serde_json::json!("Linux"));
            if !host.kernel_release.is_empty() {
                obj.insert(
                    "kernel_version".to_string(),
                    serde_json::json!(host.kernel_release),
                );
            }
            if !host.cpu_model.is_empty() {
                obj.insert("cpu_model".to_string(), serde_json::json!(host.cpu_model));
            }
            obj.insert("cpu_cores".to_string(), serde_json::json!(host.cpu_cores));
        }
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let has_psram = ctx.platform.memory_snapshot().heap_free_spiram > 0;
        let hw = crate::platform::runtime_board::hardware_summary_line(has_psram);
        if let Some(obj) = json.as_object_mut() {
            obj.insert("hardware_model".to_string(), serde_json::json!(hw));
        }
    }

    #[cfg(all(
        not(any(target_arch = "xtensa", target_arch = "riscv32")),
        target_os = "linux"
    ))]
    {
        if let Some(hw) = crate::host_observability::linux_machine_display_name() {
            if hw != ctx.board_id.as_ref() {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("hardware_model".to_string(), serde_json::json!(hw));
                }
            }
        }
    }

    serde_json::to_string(&json).map_err(to_io)
}

#[cfg(test)]
mod tests {
    use super::body;
    use serde_json::Value;

    #[test]
    fn body_keeps_system_info_as_device_summary_contract() {
        let ctx = build_test_context();

        let payload = body(&ctx).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(
            parsed.get("product_name").and_then(Value::as_str),
            Some("beetle")
        );
        assert!(parsed.get("system_status").is_none());
        assert!(parsed.get("firmware_version").is_some());
        assert!(parsed.get("board_id").is_some());
        assert!(parsed.get("ota_available").is_some());
        assert!(parsed.get("locale").is_some());
        assert!(parsed.get("lan_ip").is_some());
        assert!(parsed.get("workflow").is_some());
        assert!(parsed.get("programmable_reasoning").is_some());
        assert!(parsed["workflow"].get("executed").is_some());
        assert_eq!(
            parsed["programmable_reasoning"]["stage"].as_str(),
            Some("adversarial_arena")
        );
        assert_eq!(
            parsed["programmable_reasoning"]["execution_enabled"].as_bool(),
            Some(cfg!(target_os = "linux"))
        );
        assert!(parsed.get("initiative").is_none());
        assert!(parsed.get("presence").is_none());
        assert!(parsed.get("runtime_mode").is_none());
        assert!(parsed.get("runtime_mode_snapshot").is_none());
        assert!(parsed.get("soul_kernel").is_none());
        assert!(parsed.get("audio_duplex_profile").is_none());
        assert!(parsed.get("audio_duplex_capabilities").is_none());
        assert!(parsed.get("supervisor").is_none());
        assert!(parsed.get("release").is_none());
    }

    fn build_test_context() -> crate::platform::http_server::handlers::HandlerContext {
        crate::platform::http_server::handlers::build_default_test_handler_context()
    }
}
