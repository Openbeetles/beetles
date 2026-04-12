//! Shared host observability helpers for Linux and other host targets.
//! 共享宿主观测辅助：给 `board_info` / `system_info` / `network` 复用。

use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct HostStorageSnapshot {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct HostNetworkInterfaceSnapshot {
    pub name: String,
    pub mac: String,
    pub operstate: String,
    pub mtu: u32,
    pub carrier: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, Default)]
pub struct HostDnsConfigSnapshot {
    pub nameservers: Vec<String>,
    pub search: Vec<String>,
    pub options: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct HostDefaultRouteSnapshot {
    pub interface: String,
    pub gateway: String,
    pub mask: String,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct LinuxHostObservabilitySnapshot {
    pub arch: String,
    pub hostname: String,
    pub os_line: String,
    pub distro_pretty: String,
    pub distro_id: String,
    pub kernel_release: String,
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub hardware_model: Option<String>,
    pub load_avg_1: f32,
    pub load_avg_5: f32,
    pub load_avg_15: f32,
    pub process_count: u32,
    pub mem_total_bytes: u64,
    pub mem_available_bytes: u64,
    pub mem_usage_percent: f32,
    pub storage: Option<HostStorageSnapshot>,
    pub storage_usage_percent: f32,
    pub temperature_celsius: Option<f32>,
    pub network_interfaces: Vec<HostNetworkInterfaceSnapshot>,
    pub dns: HostDnsConfigSnapshot,
    pub default_route: Option<HostDefaultRouteSnapshot>,
}

#[cfg(all(not(any(target_arch = "xtensa", target_arch = "riscv32")), unix))]
fn disk_usage_for_path(path: &std::path::Path) -> Option<HostStorageSnapshot> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut vfs: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut vfs) };
    if rc != 0 {
        log::warn!(
            "[host_observability] statvfs {:?}: {}",
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
    Some(HostStorageSnapshot {
        total_bytes: total,
        used_bytes: used,
        free_bytes: free,
    })
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn host_state_root_usage() -> Option<(u64, u64)> {
    None
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn host_state_root_usage() -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        let path = crate::platform::state_mount_path();
        disk_usage_for_path(&path).map(|storage| (storage.total_bytes, storage.used_bytes))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(all(not(any(target_arch = "xtensa", target_arch = "riscv32")), unix))]
pub fn host_storage_for_state_root() -> Option<HostStorageSnapshot> {
    disk_usage_for_path(&crate::platform::state_mount_path())
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn host_storage_for_state_root() -> Option<HostStorageSnapshot> {
    None
}

#[cfg(all(not(any(target_arch = "xtensa", target_arch = "riscv32")), unix))]
pub fn hostname_best_effort() -> String {
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
pub fn hostname_best_effort() -> String {
    String::new()
}

#[cfg(all(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    unix,
    target_os = "linux"
))]
pub(crate) fn cpu_core_count() -> u32 {
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
pub(crate) fn cpu_core_count() -> u32 {
    let n = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if n > 0 {
        n as u32
    } else {
        0
    }
}

#[cfg(all(not(any(target_arch = "xtensa", target_arch = "riscv32")), not(unix)))]
pub(crate) fn cpu_core_count() -> u32 {
    0
}

#[cfg(any(target_os = "linux", test))]
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

#[cfg(any(target_os = "linux", test))]
pub(crate) fn linux_os_release_from_str(content: &str) -> (String, String) {
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
        let key = line[..eq].trim();
        let value = unquote_os_release_value(&line[eq + 1..]);
        match key {
            "PRETTY_NAME" => pretty = Some(value),
            "NAME" => name = Some(value),
            "VERSION" => version = Some(value),
            "VERSION_ID" => version_id = Some(value),
            "ID" => id = Some(value),
            _ => {}
        }
    }
    let distro_pretty = pretty.unwrap_or_else(|| match (&name, &version, &version_id) {
        (Some(name), Some(version), _) if !version.is_empty() => format!("{name} {version}"),
        (Some(name), _, Some(version_id)) => format!("{name} {version_id}"),
        (Some(name), _, _) => name.clone(),
        _ => id.clone().unwrap_or_default(),
    });
    (distro_pretty, id.unwrap_or_default())
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_os_release_summary() -> (String, String) {
    match std::fs::read_to_string("/etc/os-release") {
        Ok(content) => linux_os_release_from_str(&content),
        Err(_) => (String::new(), String::new()),
    }
}

#[cfg(target_os = "linux")]
fn linux_device_tree_model() -> String {
    std::fs::read("/proc/device-tree/model")
        .ok()
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .trim_end_matches('\0')
                .trim()
                .to_string()
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_kernel_release() -> String {
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
pub(crate) fn parse_proc_cpu_model() -> String {
    let Ok(content) = std::fs::read_to_string("/proc/cpuinfo") else {
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
    for line in content.lines() {
        let Some(i) = line.find(':') else {
            continue;
        };
        let key = line[..i].trim();
        let value = line[i + 1..].trim();
        if value.is_empty() {
            continue;
        }
        match key {
            "model name" if model_name.is_empty() => model_name = value.to_string(),
            "Model" if model_dt.is_empty() => model_dt = value.to_string(),
            "Processor" if processor.is_empty() => processor = value.to_string(),
            "Hardware" if hardware.is_empty() => hardware = value.to_string(),
            "cpu model" if cpu_model.is_empty() => cpu_model = value.to_string(),
            "CPU architecture" if cpu_arch.is_empty() => cpu_arch = value.to_string(),
            "CPU implementer" if cpu_impl.is_empty() => cpu_impl = value.to_string(),
            "CPU part" if cpu_part.is_empty() => cpu_part = value.to_string(),
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
    let value = std::fs::read_to_string(path).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if PLACEHOLDERS.iter().any(|placeholder| lower == *placeholder) {
        return None;
    }
    Some(trimmed.to_string())
}

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
        if let Some(value) = dmi_product_line_trimmed(path) {
            return Some(value);
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
    let content = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let parts: Vec<&str> = content.split_whitespace().collect();
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
    let process_count = parts
        .get(3)
        .and_then(|s| s.split('/').nth(1).and_then(|n| n.parse::<u32>().ok()))
        .unwrap_or(0);
    (load1, load5, load15, process_count)
}

#[cfg(target_os = "linux")]
fn linux_thermal_temp() -> Option<f32> {
    for i in 0..10 {
        let path = format!("/sys/class/thermal/thermal_zone{}/temp", i);
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(millidegrees) = content.trim().parse::<i32>() {
                return Some(millidegrees as f32 / 1000.0);
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
pub fn list_linux_network_interfaces() -> Vec<HostNetworkInterfaceSnapshot> {
    let mut interfaces = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "lo" {
                continue;
            }
            let base = format!("/sys/class/net/{name}");
            let mac = std::fs::read_to_string(format!("{base}/address"))
                .ok()
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let operstate = std::fs::read_to_string(format!("{base}/operstate"))
                .ok()
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let mtu = std::fs::read_to_string(format!("{base}/mtu"))
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .unwrap_or(0);
            let carrier = std::fs::read_to_string(format!("{base}/carrier"))
                .ok()
                .map(|s| s.trim() == "1")
                .unwrap_or(false);
            interfaces.push(HostNetworkInterfaceSnapshot {
                name,
                mac,
                operstate,
                mtu,
                carrier,
            });
        }
    }
    interfaces.sort_by(|left, right| left.name.cmp(&right.name));
    interfaces
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn parse_resolv_conf(raw: &str) -> HostDnsConfigSnapshot {
    let mut nameservers = Vec::new();
    let mut search = Vec::new();
    let mut options = Vec::new();
    for line in raw.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(kind) = parts.next() else {
            continue;
        };
        match kind {
            "nameserver" => {
                if let Some(value) = parts.next() {
                    nameservers.push(value.to_string());
                }
            }
            "search" => search.extend(parts.map(str::to_string)),
            "options" => options.extend(parts.map(str::to_string)),
            _ => {}
        }
    }
    HostDnsConfigSnapshot {
        nameservers,
        search,
        options,
    }
}

#[cfg(target_os = "linux")]
pub fn read_linux_dns_config() -> HostDnsConfigSnapshot {
    std::fs::read_to_string("/etc/resolv.conf")
        .map(|raw| parse_resolv_conf(&raw))
        .unwrap_or_default()
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn decode_ipv4_hex_le(raw: &str) -> Option<String> {
    let value = u32::from_str_radix(raw, 16).ok()?;
    Some(std::net::Ipv4Addr::from(value.to_le_bytes()).to_string())
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn parse_default_route(raw: &str) -> Option<HostDefaultRouteSnapshot> {
    for line in raw.lines().skip(1) {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 8 || columns[1] != "00000000" {
            continue;
        }
        return Some(HostDefaultRouteSnapshot {
            interface: columns[0].to_string(),
            gateway: decode_ipv4_hex_le(columns[2])?,
            mask: decode_ipv4_hex_le(columns[7]).unwrap_or_else(|| "0.0.0.0".to_string()),
        });
    }
    None
}

#[cfg(target_os = "linux")]
pub fn read_linux_default_route() -> Option<HostDefaultRouteSnapshot> {
    std::fs::read_to_string("/proc/net/route")
        .ok()
        .and_then(|raw| parse_default_route(&raw))
}

#[cfg(target_os = "linux")]
pub fn collect_linux_host_observability(
    snap: &crate::orchestrator::ResourceSnapshot,
) -> LinuxHostObservabilitySnapshot {
    use crate::platform::memory_linux::{meminfo_kb_to_bytes, parse_meminfo_kb};

    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mem_total_bytes = parse_meminfo_kb(&meminfo, "MemTotal:")
        .map(meminfo_kb_to_bytes)
        .unwrap_or(0);
    let mem_available_bytes = u64::from(snap.heap_free_internal);
    let mem_usage_percent = if mem_total_bytes > 0 {
        ((mem_total_bytes - mem_available_bytes) as f32 / mem_total_bytes as f32) * 100.0
    } else {
        0.0
    };

    let storage = host_storage_for_state_root();
    let storage_usage_percent = storage
        .as_ref()
        .filter(|storage| storage.total_bytes > 0)
        .map(|storage| (storage.used_bytes as f32 / storage.total_bytes as f32) * 100.0)
        .unwrap_or(0.0);

    let os_line = std::fs::read_to_string("/proc/version")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .next()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
        })
        .unwrap_or_default();
    let (load_avg_1, load_avg_5, load_avg_15, process_count) = linux_load_avg();
    let (distro_pretty, distro_id) = linux_os_release_summary();

    LinuxHostObservabilitySnapshot {
        arch: std::env::consts::ARCH.to_string(),
        hostname: hostname_best_effort(),
        os_line,
        distro_pretty,
        distro_id,
        kernel_release: linux_kernel_release(),
        cpu_model: parse_proc_cpu_model(),
        cpu_cores: {
            let cores = cpu_core_count();
            if cores > 0 {
                cores
            } else {
                std::fs::read_to_string("/proc/cpuinfo")
                    .ok()
                    .map(|content| {
                        content
                            .lines()
                            .filter_map(|line| line.find(':').map(|i| line[..i].trim()))
                            .filter(|key| *key == "processor")
                            .count() as u32
                    })
                    .unwrap_or(0)
            }
        },
        hardware_model: linux_machine_display_name(),
        load_avg_1,
        load_avg_5,
        load_avg_15,
        process_count,
        mem_total_bytes,
        mem_available_bytes,
        mem_usage_percent,
        storage,
        storage_usage_percent,
        temperature_celsius: linux_thermal_temp(),
        network_interfaces: list_linux_network_interfaces(),
        dns: read_linux_dns_config(),
        default_route: read_linux_default_route(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decode_ipv4_hex_le, linux_os_release_from_str, parse_default_route, parse_resolv_conf,
    };

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

    #[test]
    fn parse_resolver_config_extracts_core_fields() {
        let raw = "\
nameserver 1.1.1.1\n\
nameserver 8.8.8.8\n\
search lan local\n\
options timeout:2 attempts:3\n";
        let parsed = parse_resolv_conf(raw);
        assert_eq!(parsed.nameservers, vec!["1.1.1.1", "8.8.8.8"]);
        assert_eq!(parsed.search, vec!["lan", "local"]);
        assert_eq!(parsed.options, vec!["timeout:2", "attempts:3"]);
    }

    #[test]
    fn parse_default_route_reads_gateway() {
        let raw = "\
Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n\
eth0\t00000000\t0101A8C0\t0003\t0\t0\t0\t00000000\n";
        let route = parse_default_route(raw).expect("default route");
        assert_eq!(route.interface, "eth0");
        assert_eq!(route.gateway, "192.168.1.1");
        assert_eq!(route.mask, "0.0.0.0");
    }

    #[test]
    fn decode_ipv4_hex_le_handles_route_encoding() {
        assert_eq!(decode_ipv4_hex_le("0101A8C0"), Some("192.168.1.1".to_string()));
    }
}
