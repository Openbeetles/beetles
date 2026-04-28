//! Platform crash evidence source.
//! 平台 crash 证据来源；只上报真实 reset/coredump/boot 事实，不合成 PC。

use crate::orchestrator::CrashMetadataSnapshot;

/// Snapshot crash evidence available to the running process.
pub fn snapshot() -> CrashMetadataSnapshot {
    target_snapshot()
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn target_snapshot() -> CrashMetadataSnapshot {
    CrashMetadataSnapshot::default()
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn target_snapshot() -> CrashMetadataSnapshot {
    let reason = unsafe { esp_idf_svc::sys::esp_reset_reason() };
    let Some(reason_label) = crash_reset_reason_label(reason) else {
        return CrashMetadataSnapshot::default();
    };
    CrashMetadataSnapshot {
        last_panic_reason: Some(format!("reset_reason={reason_label}")),
        last_symbolize_hint: Some(
            "capture serial log then run scripts/parse_esp_panic_log.sh --artifact-dir target/esp-artifacts/<artifact-id> <serial-log>"
                .to_string(),
        ),
        ..CrashMetadataSnapshot::default()
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn crash_reset_reason_label(reason: esp_idf_svc::sys::esp_reset_reason_t) -> Option<&'static str> {
    use esp_idf_svc::sys;
    match reason {
        sys::esp_reset_reason_t_ESP_RST_PANIC => Some("ESP_RST_PANIC"),
        sys::esp_reset_reason_t_ESP_RST_INT_WDT => Some("ESP_RST_INT_WDT"),
        sys::esp_reset_reason_t_ESP_RST_TASK_WDT => Some("ESP_RST_TASK_WDT"),
        sys::esp_reset_reason_t_ESP_RST_WDT => Some("ESP_RST_WDT"),
        sys::esp_reset_reason_t_ESP_RST_CPU_LOCKUP => Some("ESP_RST_CPU_LOCKUP"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::snapshot;

    #[test]
    fn host_crash_evidence_does_not_synthesize_metadata() {
        assert!(snapshot().is_empty());
    }
}
