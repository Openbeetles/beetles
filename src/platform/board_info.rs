//! 板级状态 JSON。ESP：芯片、堆、SPIFFS 等；Linux：`platform` 为 `linux`；其它操作系统为 `std::env::consts::OS`（如 `macos`、`windows`）。供 `Platform::board_info_json` 与工具层复用。
//! Board status JSON: ESP; Linux (`platform` = `linux`); other OS (`platform` = `std::env::consts::OS`, e.g. `macos`, `windows`).

use serde_json::json;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
struct EspPayloadInput<'a> {
    chip_model: &'a str,
    chip_revision: u32,
    cores: u32,
    snap: &'a crate::orchestrator::ResourceSnapshot,
    heap_min_free: u64,
    uptime_secs: u64,
    idf_version: &'a str,
    wifi_sta_connected: bool,
    spiffs: serde_json::Value,
    spiffs_usage_pct: f32,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
fn esp_payload(input: EspPayloadInput<'_>) -> serde_json::Value {
    let heap_internal = u64::from(input.snap.heap_free_internal);
    let psram_free = u64::from(input.snap.heap_free_spiram);
    let heap_total = heap_internal.saturating_add(psram_free);
    json!({
        "platform": "esp32",
        "chip_model": input.chip_model,
        "chip_revision": input.chip_revision,
        "cores": input.cores,
        // heap_free now matches the actual internal heap free bytes used by pressure/TLS checks.
        // Keep heap_free_total for whole-device free memory across internal SRAM + PSRAM.
        "heap_free": heap_internal,
        "heap_free_internal": heap_internal,
        "heap_free_total": heap_total,
        "psram_free": psram_free,
        "psram_total": input.snap.heap_total_spiram,
        "psram_used_est": input.snap.heap_used_spiram_est,
        "psram_min_free": input.snap.heap_min_free_spiram,
        "psram_largest_block": input.snap.heap_largest_block_spiram,
        "heap_min_free": input.heap_min_free,
        "heap_min_free_internal": input.snap.heap_min_free_internal,
        "heap_largest_block_internal": input.snap.heap_largest_block_internal,
        "tls_fragmentation_risk": input.snap.tls_fragmentation_risk,
        "uptime_secs": input.uptime_secs,
        "idf_version": input.idf_version,
        "pressure_level": format!("{:?}", input.snap.pressure),
        "hint": input.snap.budget.llm_hint,
        "runtime_capabilities": crate::orchestrator::runtime_capability_summary(),
        "wifi_sta_connected": input.wifi_sta_connected,
        "spiffs": input.spiffs,
        "spiffs_usage_percent": input.spiffs_usage_pct,
    })
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn collect_esp() -> String {
    let (chip_model, chip_revision, cores) =
        crate::platform::runtime_board::esp_chip_model_revision_cores();
    let snap = crate::orchestrator::snapshot();
    let heap_min_free = crate::platform::heap::heap_min_free_internal() as u64;
    let uptime_secs = crate::platform::time::app_uptime_secs();
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

    let out = esp_payload(EspPayloadInput {
        chip_model: &chip_model,
        chip_revision,
        cores,
        snap: &snap,
        heap_min_free,
        uptime_secs,
        idf_version,
        wifi_sta_connected,
        spiffs,
        spiffs_usage_pct,
    });
    out.to_string()
}

/// 返回 state_root 所在文件系统的 (total_bytes, used_bytes)，语义对齐 ESP `spiffs_usage`。
/// Non-unix 返回 None。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn host_state_root_usage() -> Option<(u64, u64)> {
    crate::host_observability::host_state_root_usage()
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
    let obs = crate::host_observability::collect_linux_host_observability(snap);
    // 使用 orchestrator 的 delta 采样值，与 /api/resource 口径一致，避免重复采样。
    let cpu_usage = snap.cpu_usage_percent;

    json!({
        "platform": "linux",
        "uptime_secs": uptime_secs,
        "pressure_level": format!("{:?}", snap.pressure),
        "hint": snap.budget.llm_hint,
        "runtime_capabilities": crate::orchestrator::runtime_capability_summary(),
        "wifi_sta_connected": wifi_sta_connected,
        "storage": obs.storage,
        "storage_usage_percent": obs.storage_usage_percent,
        "arch": obs.arch,
        "hostname": obs.hostname,
        "os": obs.os_line,
        "distro_pretty": obs.distro_pretty,
        "distro_id": obs.distro_id,
        "kernel_release": obs.kernel_release,
        "cpu_model": obs.cpu_model,
        "cpu_cores": obs.cpu_cores,
        "cpu_usage_percent": cpu_usage,
        "load_avg_1": obs.load_avg_1,
        "load_avg_5": obs.load_avg_5,
        "load_avg_15": obs.load_avg_15,
        "process_count": obs.process_count,
        "mem_total_bytes": obs.mem_total_bytes,
        "mem_available_bytes": obs.mem_available_bytes,
        "mem_usage_percent": obs.mem_usage_percent,
        "temperature_celsius": obs.temperature_celsius,
        "network_interfaces": obs.network_interfaces,
        "dns": obs.dns,
        "default_route": obs.default_route,
        "hardware_model": obs.hardware_model,
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
    let hostname = crate::host_observability::hostname_best_effort();
    let storage = crate::host_observability::host_storage_for_state_root();
    let mem_available_bytes = u64::from(snap.heap_free_internal);
    let platform_os = std::env::consts::OS;

    json!({
        "platform": platform_os,
        "uptime_secs": uptime_secs,
        "pressure_level": format!("{:?}", snap.pressure),
        "hint": snap.budget.llm_hint,
        "runtime_capabilities": crate::orchestrator::runtime_capability_summary(),
        "wifi_sta_connected": wifi_sta_connected,
        "storage": storage,
        "arch": std::env::consts::ARCH,
        "hostname": hostname,
        "cpu_cores": crate::host_observability::cpu_core_count(),
        "mem_available_bytes": mem_available_bytes,
    })
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn collect_host() -> String {
    let snap = crate::orchestrator::snapshot();
    let wifi_sta_connected = crate::platform::is_wifi_sta_connected();
    let uptime_secs = crate::platform::time::app_uptime_secs();

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

#[cfg(test)]
mod tests {
    use super::{esp_payload, EspPayloadInput};
    use crate::orchestrator::{
        pressure::{budget_for_level, PressureLevel},
        ResourceSnapshot, TlsFragmentationRisk,
    };
    use serde_json::json;

    #[test]
    fn esp_payload_uses_heap_free_for_internal_heap_only() {
        let snap = sample_resource_snapshot();
        let payload = esp_payload(EspPayloadInput {
            chip_model: "esp32s3",
            chip_revision: 2,
            cores: 2,
            snap: &snap,
            heap_min_free: 69_800,
            uptime_secs: 84,
            idf_version: "v6.0",
            wifi_sta_connected: true,
            spiffs: json!({"total_bytes": 100, "used_bytes": 2, "free_bytes": 98}),
            spiffs_usage_pct: 2.0,
        });

        assert_eq!(payload["heap_free"].as_u64(), Some(90_700));
        assert_eq!(payload["heap_free_internal"].as_u64(), Some(90_700));
        assert_eq!(payload["heap_free_total"].as_u64(), Some(7_780_700));
        assert_eq!(payload["psram_free"].as_u64(), Some(7_690_000));
        assert_eq!(payload["psram_total"].as_u64(), Some(8_388_608));
        assert_eq!(payload["psram_used_est"].as_u64(), Some(698_608));
        assert_eq!(payload["psram_min_free"].as_u64(), Some(7_100_000));
        assert_eq!(payload["psram_largest_block"].as_u64(), Some(6_900_000));
        assert_eq!(payload["heap_min_free_internal"].as_u64(), Some(69_800));
        assert_eq!(
            payload["heap_largest_block_internal"].as_u64(),
            Some(18_432)
        );
        assert_eq!(payload["tls_fragmentation_risk"].as_str(), Some("critical"));
    }

    fn sample_resource_snapshot() -> ResourceSnapshot {
        ResourceSnapshot {
            pressure: PressureLevel::Cautious,
            tls_fragmentation_risk: TlsFragmentationRisk::Critical,
            storage_contention_risk: crate::orchestrator::StorageContentionRisk::Healthy,
            heap_free_internal: 90_700,
            heap_min_free_internal: 69_800,
            heap_free_spiram: 7_690_000,
            heap_total_spiram: 8_388_608,
            heap_min_free_spiram: 7_100_000,
            heap_largest_block_spiram: 6_900_000,
            heap_used_spiram_est: 698_608,
            heap_largest_block_internal: 18_432,
            active_http_count: 0,
            active_wss_count: 0,
            active_agent_tasks: 0,
            inbound_depth: 0,
            outbound_depth: 0,
            budget: budget_for_level(PressureLevel::Cautious),
            channels: crate::orchestrator::state::ChannelsHealthSnapshot::all(
                empty_channel_health(),
            ),
            leases: crate::runtime::lease::snapshot(),
            session_count: 0,
            storage_used_kb: 0,
            storage_total_kb: 0,
            audio_recording: false,
            audio_playing: false,
            audio_interrupt_listening: false,
            audio_interrupt_requested: false,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            cpu_usage_percent: 0.0,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            load_average: (0.0, 0.0, 0.0),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            process_memory_kb: 0,
        }
    }

    fn empty_channel_health() -> crate::orchestrator::state::ChannelHealthSnapshot {
        crate::orchestrator::state::ChannelHealthSnapshot::healthy()
    }
}
