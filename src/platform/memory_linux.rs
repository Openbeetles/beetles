//! Linux `/proc/meminfo` 解析，供 [`LinuxPlatform`](super::linux::LinuxPlatform) 内存快照。
//! Parse `/proc/meminfo` for Linux platform memory snapshot.

use crate::platform::abstraction::MemorySnapshot;

/// 构建 Linux 内存快照。
/// - `heap_free_internal` = `MemAvailable`（与 ESP `internal` 语义对齐：可分配量）。
/// - `heap_largest_block` = **0**（无等价于 ESP「最大连续空闲块」的内核指标；0 表示 N/A，非「零字节块」）。
/// - `heap_free_spiram` = 0（Linux 无 PSRAM）。
/// - `*_min_*` / `*_total_spiram` / `*_largest_block_spiram` = 0（Linux 无等价 ESP heap_caps 口径）。
pub fn linux_memory_snapshot() -> MemorySnapshot {
    let s = match std::fs::read_to_string("/proc/meminfo") {
        Ok(x) => x,
        Err(e) => {
            log::warn!("[memory_linux] read /proc/meminfo: {}", e);
            return MemorySnapshot {
                heap_free_internal: 0,
                heap_min_free_internal: 0,
                heap_free_spiram: 0,
                heap_total_spiram: 0,
                heap_min_free_spiram: 0,
                heap_largest_block_spiram: 0,
                heap_largest_block: 0,
            };
        }
    };
    let internal = parse_meminfo_kb(&s, "MemAvailable:")
        .or_else(|| {
            log::warn!("[memory_linux] MemAvailable not found, trying MemFree");
            parse_meminfo_kb(&s, "MemFree:")
        })
        .map(kb_to_bytes_saturating_u32)
        .unwrap_or(0);
    MemorySnapshot {
        heap_free_internal: internal,
        heap_min_free_internal: 0,
        heap_free_spiram: 0,
        heap_total_spiram: 0,
        heap_min_free_spiram: 0,
        heap_largest_block_spiram: 0,
        heap_largest_block: 0,
    }
}

pub(crate) fn parse_meminfo_kb(content: &str, prefix: &str) -> Option<u64> {
    for line in content.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix(prefix) {
            let mut parts = rest.split_whitespace();
            let num = parts.next()?.parse::<u64>().ok()?;
            return Some(num);
        }
    }
    None
}

pub(crate) fn kb_to_bytes_saturating_u32(kb: u64) -> u32 {
    let bytes = kb.saturating_mul(1024);
    if bytes >= u64::from(u32::MAX) {
        u32::MAX
    } else {
        bytes as u32
    }
}

/// `/proc/meminfo` 中 kB 数值转为字节（u64），供 board_info 等需要完整精度的路径。
/// Convert meminfo kB line value to bytes (u64) for board_info and similar.
#[cfg(target_os = "linux")]
pub(crate) fn meminfo_kb_to_bytes(kb: u64) -> u64 {
    kb.saturating_mul(1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_memavailable_kb() {
        let sample = "MemTotal:        500000 kB\nMemAvailable:   123456 kB\n";
        assert_eq!(parse_meminfo_kb(sample, "MemAvailable:"), Some(123456));
    }

    #[test]
    fn parses_memfree_fallback() {
        let sample = "MemTotal:        500000 kB\nMemFree:         99999 kB\n";
        assert_eq!(parse_meminfo_kb(sample, "MemFree:"), Some(99999));
    }
}
