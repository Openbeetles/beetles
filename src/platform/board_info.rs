//! 板级状态 JSON。ESP：芯片、堆、SPIFFS 等；Linux：`platform` 为 `linux`；其它操作系统为 `std::env::consts::OS`（如 `macos`、`windows`）。供 `Platform::board_info_json` 与工具层复用。
//! Board status JSON: ESP; Linux (`platform` = `linux`); other OS (`platform` = `std::env::consts::OS`, e.g. `macos`, `windows`).

use serde_json::json;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn collect_esp() -> String {
    let (chip_model, chip_revision, cores) =
        crate::platform::runtime_board::esp_chip_model_revision_cores();
    let snap = crate::orchestrator::snapshot();
    let heap_internal = snap.heap_free_internal as usize;
    let psram_free = snap.heap_free_spiram as usize;
    let heap_total = heap_internal.saturating_add(psram_free);
    let heap_min_free = crate::platform::heap::heap_min_free_internal() as u64;
    let uptime_secs = crate::platform::time::uptime_secs();
    let idf_version = option_env!("IDF_VERSION").unwrap_or("unknown");
    let wifi_sta_connected = crate::platform::is_wifi_sta_connected();
    let (spiffs, spiffs_usage_pct) = crate::platform::spiffs_usage()
        .map(|(total, used)| {
            let free = total.saturating_sub(used);
            let pct = if total > 0 {
                (used as f32 / total as f32) * 100.0
            } else {
                0.0
            };
            (
                json!({
                    "total_bytes": total,
                    "used_bytes": used,
                    "free_bytes": free,
                }),
                pct,
            )
        })
        .unwrap_or((serde_json::Value::Null, 0.0));

    let out = json!({
        "platform": "esp32",
        "chip_model": chip_model,
        "chip_revision": chip_revision,
        "cores": cores,
        // Keep heap_free for backward compatibility; it represents internal + PSRAM total.
        "heap_free": heap_total,
        "heap_free_total": heap_total,
        "heap_free_internal": heap_internal,
        "heap_min_free": heap_min_free,
        "psram_free": psram_free,
        "uptime_secs": uptime_secs,
        "idf_version": idf_version,
        "pressure_level": format!("{:?}", snap.pressure),
        "hint": snap.budget.llm_hint,
        "wifi_sta_connected": wifi_sta_connected,
        "spiffs": spiffs,
        "spiffs_usage_percent": spiffs_usage_pct,
    });
    out.to_string()
}

#[cfg(all(not(any(target_arch = "xtensa", target_arch = "riscv32")), unix))]
fn disk_usage_for_path(path: &std::path::Path) -> Option<(u64, u64, u64)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut vfs: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut vfs) };
    if rc != 0 {
        log::warn!(
            "[board_info] statvfs {:?}: {}",
            path,
            std::io::Error::last_os_error()
        );
        return None;
    }
    let frsize = vfs.f_frsize as u64;
    let blocks = vfs.f_blocks as u64;
    let bavail = vfs.f_bavail as u64;
    let total = blocks.saturating_mul(frsize);
    let free = bavail.saturating_mul(frsize);
    let used = total.saturating_sub(free);
    Some((total, used, free))
}

/// 返回 state_root 所在文件系统的 (total_bytes, used_bytes)，语义对齐 ESP `spiffs_usage`。
/// Non-unix 返回 None。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn host_state_root_usage() -> Option<(usize, usize)> {
    #[cfg(unix)]
    {
        let path = crate::platform::state_mount_path();
        disk_usage_for_path(&path).map(|(total, used, _free)| (total as usize, used as usize))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(all(not(any(target_arch = "xtensa", target_arch = "riscv32")), unix))]
fn disk_storage_json(path: &std::path::Path) -> serde_json::Value {
    disk_usage_for_path(path).map_or(serde_json::Value::Null, |(total, used, free)| {
        json!({
            "total_bytes": total,
            "used_bytes": used,
            "free_bytes": free,
        })
    })
}

#[cfg(all(not(any(target_arch = "xtensa", target_arch = "riscv32")), not(unix)))]
fn disk_storage_json(_path: &std::path::Path) -> serde_json::Value {
    serde_json::Value::Null
}

#[cfg(all(not(any(target_arch = "xtensa", target_arch = "riscv32")), unix))]
fn hostname_best_effort() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/proc/sys/kernel/hostname")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| {
            let mut buf = [0u8; 256];
            let ok =
                unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) } == 0;
            if ok {
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                return String::from_utf8_lossy(&buf[..len]).to_string();
            }
            String::new()
        })
}

#[cfg(all(not(any(target_arch = "xtensa", target_arch = "riscv32")), not(unix)))]
fn hostname_best_effort() -> String {
    String::new()
}

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    unix,
    target_os = "linux"
))]
fn cpu_core_count() -> u32 {
    for sc in [libc::_SC_NPROCESSORS_ONLN, libc::_SC_NPROCESSORS_CONF] {
        let n = unsafe { libc::sysconf(sc) };
        if n > 0 {
            return n as u32;
        }
    }
    0
}

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    unix,
    not(target_os = "linux")
))]
fn cpu_core_count() -> u32 {
    let n = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if n > 0 {
        n as u32
    } else {
        0
    }
}

#[cfg(all(not(any(target_arch = "xtensa", target_arch = "riscv32")), not(unix)))]
fn cpu_core_count() -> u32 {
    0
}

/// 去掉 `/etc/os-release` 里 KEY="value" 的引号。Strip quotes from os-release values.
#[cfg(target_os = "linux")]
fn unquote_os_release_value(raw: &str) -> String {
    let s = raw.trim();
    let Some(first) = s.chars().next() else {
        return String::new();
    };
    if (first == '"' || first == '\'') && s.ends_with(first) && s.len() >= 2 {
        s[1..s.len() - 1].replace("\\\"", "\"").replace("\\n", "\n")
    } else {
        s.to_string()
    }
}

/// 解析 os-release 文本，供单测与运行时共用。Parse os-release text (tests + runtime).
#[cfg(target_os = "linux")]
fn linux_os_release_from_str(content: &str) -> (String, String) {
    let mut pretty = None::<String>;
    let mut name = None::<String>;
    let mut version = None::<String>;
    let mut version_id = None::<String>;
    let mut id = None::<String>;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(eq) = line.find('=') else {
            continue;
        };
        let k = line[..eq].trim();
        let v = unquote_os_release_value(&line[eq + 1..]);
        match k {
            "PRETTY_NAME" => pretty = Some(v),
            "NAME" => name = Some(v),
            "VERSION" => version = Some(v),
            "VERSION_ID" => version_id = Some(v),
            "ID" => id = Some(v),
            _ => {}
        }
    }
    let distro_pretty = pretty.unwrap_or_else(|| match (&name, &version, &version_id) {
        (Some(n), Some(ver), _) if !ver.is_empty() => format!("{} {}", n, ver),
        (Some(n), _, Some(vid)) => format!("{} {}", n, vid),
        (Some(n), _, _) => n.clone(),
        _ => id.clone().unwrap_or_default(),
    });
    let distro_id = id.unwrap_or_default();
    (distro_pretty, distro_id)
}

#[cfg(target_os = "linux")]
fn linux_os_release_summary() -> (String, String) {
    match std::fs::read_to_string("/etc/os-release") {
        Ok(s) => linux_os_release_from_str(&s),
        Err(_) => (String::new(), String::new()),
    }
}

/// 设备树板型（ARM 嵌入式常见），如 Luckfox / Rockchip。Device-tree board model (common on ARM SBCs).
#[cfg(target_os = "linux")]
fn linux_device_tree_model() -> String {
    std::fs::read("/proc/device-tree/model")
        .ok()
        .map(|b| {
            String::from_utf8_lossy(&b)
                .trim_end_matches('\0')
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn linux_kernel_release() -> String {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
fn parse_proc_cpu_model() -> String {
    let Ok(s) = std::fs::read_to_string("/proc/cpuinfo") else {
        return linux_device_tree_model();
    };
    let mut model_name = String::new();
    let mut model_dt = String::new();
    let mut processor = String::new();
    let mut hardware = String::new();
    let mut cpu_model = String::new();
    let mut cpu_arch = String::new();
    let mut cpu_impl = String::new();
    let mut cpu_part = String::new();
    for line in s.lines() {
        let Some(i) = line.find(':') else {
            continue;
        };
        let key = line[..i].trim();
        let val = line[i + 1..].trim();
        if val.is_empty() {
            continue;
        }
        match key {
            "model name" if model_name.is_empty() => model_name = val.to_string(),
            "Model" if model_dt.is_empty() => model_dt = val.to_string(),
            "Processor" if processor.is_empty() => processor = val.to_string(),
            "Hardware" if hardware.is_empty() => hardware = val.to_string(),
            "cpu model" if cpu_model.is_empty() => cpu_model = val.to_string(),
            "CPU architecture" if cpu_arch.is_empty() => cpu_arch = val.to_string(),
            "CPU implementer" if cpu_impl.is_empty() => cpu_impl = val.to_string(),
            "CPU part" if cpu_part.is_empty() => cpu_part = val.to_string(),
            _ => {}
        }
    }
    if !model_name.is_empty() {
        return model_name;
    }
    if !model_dt.is_empty() {
        return model_dt;
    }
    if !processor.is_empty() {
        return processor;
    }
    if !hardware.is_empty() {
        return hardware;
    }
    if !cpu_model.is_empty() {
        return cpu_model;
    }
    if !cpu_arch.is_empty() || !cpu_impl.is_empty() || !cpu_part.is_empty() {
        let mut out = String::new();
        if !cpu_arch.is_empty() {
            out.push_str("arch ");
            out.push_str(&cpu_arch);
        }
        if !cpu_impl.is_empty() {
            if !out.is_empty() {
                out.push_str(", ");
            }
            out.push_str("implementer ");
            out.push_str(&cpu_impl);
        }
        if !cpu_part.is_empty() {
            if !out.is_empty() {
                out.push_str(", ");
            }
            out.push_str("part ");
            out.push_str(&cpu_part);
        }
        return out;
    }
    linux_device_tree_model()
}

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
fn dmi_product_line_trimmed(path: &str) -> Option<String> {
    const PLACEHOLDERS: [&str; 4] = [
        "to be filled by o.e.m.",
        "default string",
        "system product name",
        "not specified",
    ];
    let s = std::fs::read_to_string(path).ok()?;
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    if PLACEHOLDERS.iter().any(|p| lower == *p) {
        return None;
    }
    Some(t.to_string())
}

/// Linux：供 HTTP `system_info` 等展示的机型字符串（设备树 model、DMI、cpuinfo、主机名回退）。
/// Runtime machine label for Linux (device-tree, DMI, cpuinfo, hostname); `None` if unavailable.
#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
pub fn linux_machine_display_name() -> Option<String> {
    let dt = linux_device_tree_model();
    if !dt.is_empty() {
        return Some(dt);
    }
    for path in [
        "/sys/class/dmi/id/product_name",
        "/sys/class/dmi/id/board_name",
    ] {
        if let Some(v) = dmi_product_line_trimmed(path) {
            return Some(v);
        }
    }
    let cpu = parse_proc_cpu_model();
    if !cpu.is_empty() {
        return Some(cpu);
    }
    let host = hostname_best_effort();
    if !host.is_empty() {
        return Some(format!("{} ({})", host, std::env::consts::ARCH));
    }
    None
}

#[cfg(target_os = "linux")]
fn linux_load_avg() -> (f32, f32, f32, u32) {
    let s = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let parts: Vec<&str> = s.split_whitespace().collect();
    let load1 = parts
        .first()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.0);
    let load5 = parts
        .get(1)
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.0);
    let load15 = parts
        .get(2)
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.0);
    let procs = parts
        .get(3)
        .and_then(|s| s.split('/').nth(1).and_then(|n| n.parse::<u32>().ok()))
        .unwrap_or(0);
    (load1, load5, load15, procs)
}

#[cfg(target_os = "linux")]
fn linux_cpu_usage() -> f32 {
    let s = std::fs::read_to_string("/proc/stat").unwrap_or_default();
    for line in s.lines() {
        if line.starts_with("cpu ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 5 {
                break;
            }
            let user = parts[1].parse::<u64>().unwrap_or(0);
            let nice = parts[2].parse::<u64>().unwrap_or(0);
            let system = parts[3].parse::<u64>().unwrap_or(0);
            let idle = parts[4].parse::<u64>().unwrap_or(0);
            let total = user + nice + system + idle;
            if total > 0 {
                return ((total - idle) as f32 / total as f32) * 100.0;
            }
            break;
        }
    }
    0.0
}

#[cfg(target_os = "linux")]
fn linux_thermal_temp() -> Option<f32> {
    for i in 0..10 {
        let path = format!("/sys/class/thermal/thermal_zone{}/temp", i);
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(millidegrees) = s.trim().parse::<i32>() {
                return Some(millidegrees as f32 / 1000.0);
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn linux_network_interfaces() -> Vec<serde_json::Value> {
    let mut ifaces = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "lo" {
                continue;
            }
            let addr = std::fs::read_to_string(format!("/sys/class/net/{}/address", name))
                .ok()
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            ifaces.push(json!({
                "name": name,
                "mac": addr,
            }));
        }
    }
    ifaces
}

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    target_os = "linux"
))]
fn linux_host_payload(
    snap: &crate::orchestrator::ResourceSnapshot,
    wifi_sta_connected: bool,
    uptime_secs: u64,
) -> serde_json::Value {
    use crate::platform::memory_linux::{meminfo_kb_to_bytes, parse_meminfo_kb};

    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mem_total_bytes = parse_meminfo_kb(&meminfo, "MemTotal:")
        .map(meminfo_kb_to_bytes)
        .unwrap_or(0);
    let mem_available_bytes = u64::from(snap.heap_free_internal);

    let cpu_model = parse_proc_cpu_model();
    let mut cpu_cores = cpu_core_count();
    if cpu_cores == 0 {
        if let Ok(ci) = std::fs::read_to_string("/proc/cpuinfo") {
            cpu_cores = ci
                .lines()
                .filter_map(|line| line.find(':').map(|i| line[..i].trim()))
                .filter(|key| *key == "processor")
                .count() as u32;
        }
    }

    let os_line = std::fs::read_to_string("/proc/version")
        .ok()
        .and_then(|s| {
            s.lines()
                .next()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
        })
        .unwrap_or_default();

    let (distro_pretty, distro_id) = linux_os_release_summary();
    let kernel_release = linux_kernel_release();

    let hostname = hostname_best_effort();
    let state_root = crate::platform::state_mount_path();
    let storage = disk_storage_json(&state_root);

    let (load1, load5, load15, proc_count) = linux_load_avg();
    let cpu_usage = linux_cpu_usage();
    let temp = linux_thermal_temp();
    let ifaces = linux_network_interfaces();

    let mem_usage_pct = if mem_total_bytes > 0 {
        ((mem_total_bytes - mem_available_bytes) as f32 / mem_total_bytes as f32) * 100.0
    } else {
        0.0
    };

    let storage_usage_pct = if let Some(obj) = storage.as_object() {
        let total = obj.get("total_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
        let used = obj.get("used_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
        if total > 0 {
            (used as f32 / total as f32) * 100.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    json!({
        "platform": "linux",
        "uptime_secs": uptime_secs,
        "pressure_level": format!("{:?}", snap.pressure),
        "hint": snap.budget.llm_hint,
        "wifi_sta_connected": wifi_sta_connected,
        "storage": storage,
        "storage_usage_percent": storage_usage_pct,
        "arch": std::env::consts::ARCH,
        "hostname": hostname,
        "os": os_line,
        "distro_pretty": distro_pretty,
        "distro_id": distro_id,
        "kernel_release": kernel_release,
        "cpu_model": cpu_model,
        "cpu_cores": cpu_cores,
        "cpu_usage_percent": cpu_usage,
        "load_avg_1": load1,
        "load_avg_5": load5,
        "load_avg_15": load15,
        "process_count": proc_count,
        "mem_total_bytes": mem_total_bytes,
        "mem_available_bytes": mem_available_bytes,
        "mem_usage_percent": mem_usage_pct,
        "temperature_celsius": temp,
        "network_interfaces": ifaces,
    })
}

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    not(target_os = "linux")
))]
fn non_linux_os_payload(
    snap: &crate::orchestrator::ResourceSnapshot,
    wifi_sta_connected: bool,
    uptime_secs: u64,
) -> serde_json::Value {
    let hostname = hostname_best_effort();
    let state_root = crate::platform::state_mount_path();
    let storage = disk_storage_json(&state_root);
    let mem_available_bytes = u64::from(snap.heap_free_internal);
    let platform_os = std::env::consts::OS;

    json!({
        "platform": platform_os,
        "uptime_secs": uptime_secs,
        "pressure_level": format!("{:?}", snap.pressure),
        "hint": snap.budget.llm_hint,
        "wifi_sta_connected": wifi_sta_connected,
        "storage": storage,
        "arch": std::env::consts::ARCH,
        "hostname": hostname,
        "cpu_cores": cpu_core_count(),
        "mem_available_bytes": mem_available_bytes,
    })
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn collect_host() -> String {
    let snap = crate::orchestrator::snapshot();
    let wifi_sta_connected = crate::platform::is_wifi_sta_connected();
    let uptime_secs = crate::platform::time::uptime_secs();

    #[cfg(target_os = "linux")]
    let out = linux_host_payload(&snap, wifi_sta_connected, uptime_secs);

    #[cfg(not(target_os = "linux"))]
    let out = non_linux_os_payload(&snap, wifi_sta_connected, uptime_secs);

    out.to_string()
}

/// 按当前编译目标生成板级 JSON 字符串（ESP / Linux / 其它 OS）。
pub fn board_info_json_string() -> String {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        collect_esp()
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        collect_host()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_os_release_tests {
    use super::linux_os_release_from_str;

    #[test]
    fn pretty_name_wins() {
        let raw = r#"PRETTY_NAME="Buildroot 2024.02"
NAME=Buildroot
ID=buildroot
VERSION_ID=2024.02
"#;
        let (pretty, id) = linux_os_release_from_str(raw);
        assert_eq!(pretty, "Buildroot 2024.02");
        assert_eq!(id, "buildroot");
    }

    #[test]
    fn name_and_version_id_without_pretty() {
        let raw = r#"NAME="Ubuntu"
VERSION_ID="22.04"
ID=ubuntu
"#;
        let (pretty, id) = linux_os_release_from_str(raw);
        assert_eq!(pretty, "Ubuntu 22.04");
        assert_eq!(id, "ubuntu");
    }
}
