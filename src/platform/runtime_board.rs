//! 运行期拼装的板型标识与硬件摘要，供 HTTP `board_id`、OTA manifest 键、`system_info` 使用。
//! ESP 产品侧仅识别 **ESP32-S3**（model id 9）与 **ESP32-P4**（18）；其它片型仅生成 `unsupported-soc-*` 便于排障。
//! 不依赖编译期 `BOARD` / `TARGET` 推断机型（构建仍用 `BOARD` 选分区表，与运行时上报独立）。
//! Runtime board id / hardware summary. Product SoCs: **ESP32-S3** (id 9) and **ESP32-P4** (18) only; others use `unsupported-soc-*` for diagnostics.

#[allow(dead_code)]
fn flash_mb_display(flash_bytes: u32) -> u32 {
    let mb = flash_bytes / (1024 * 1024);
    mb.max(1)
}

#[allow(dead_code)]
fn cores_display(cores: u32) -> Option<String> {
    match cores {
        0 => None,
        1 => Some("1-core CPU".to_string()),
        n => Some(format!("{}-core CPU", n)),
    }
}

#[allow(dead_code)]
fn model_display_name(model_label: &str) -> String {
    if model_label.starts_with("Unsupported SoC") {
        format!("Beetle device ({})", model_label)
    } else {
        model_label.to_string()
    }
}

#[allow(dead_code)]
fn human_hardware_summary(
    model_label: &str,
    flash_bytes: u32,
    cores: u32,
    has_psram: bool,
) -> String {
    let mut parts = vec![format!("{}MB Flash", flash_mb_display(flash_bytes))];
    if let Some(core_text) = cores_display(cores) {
        parts.push(core_text);
    }
    if !has_psram {
        parts.push("without PSRAM".to_string());
    }
    format!("{} ({})", model_display_name(model_label), parts.join(", "))
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
mod esp {
    use esp_idf_svc::sys::{esp_flash_default_chip, esp_flash_get_physical_size, ESP_OK};

    /// 与 `esp_hw_support/include/esp_chip_info.h` 中 `esp_chip_info_t` 布局一致。
    /// `esp-idf-sys` 绑定未导出 `esp_chip_info` 时由本地 `extern "C"` 链接 IDF。
    #[repr(C)]
    struct EspChipInfoRaw {
        model: u32,
        features: u32,
        revision: u16,
        cores: u8,
    }

    extern "C" {
        fn esp_chip_info(out_info: *mut EspChipInfoRaw);
    }

    /// OTA manifest `boards` 键与 `board_presets.toml` Flash 档位一致。
    const FLASH_MANIFEST_BUCKETS_MB: &[u32] = &[8, 16, 32];

    fn flash_mb_nearest_manifest_bucket(flash_bytes: u32) -> u32 {
        let mb = flash_bytes / (1024 * 1024);
        if mb == 0 {
            return 16;
        }
        let mut best = FLASH_MANIFEST_BUCKETS_MB[0];
        let mut best_dist = mb.abs_diff(best);
        for &c in &FLASH_MANIFEST_BUCKETS_MB[1..] {
            let d = mb.abs_diff(c);
            if d < best_dist {
                best = c;
                best_dist = d;
            }
        }
        best
    }

    /// 甲壳虫仅正式支持 ESP32-S3 / ESP32-P4；其余片型只作诊断占位，不参与产品矩阵。
    /// Only ESP32-S3 and ESP32-P4 are supported product SoCs; others are diagnostic placeholders.
    fn model_id_slug(model: u32) -> String {
        match model {
            9 => "esp32-s3".to_string(),
            18 => "esp32-p4".to_string(),
            _ => format!("unsupported-soc-{}", model),
        }
    }

    fn model_id_label(model: u32) -> String {
        match model {
            9 => "ESP32-S3".to_string(),
            18 => "ESP32-P4".to_string(),
            _ => format!("Unsupported SoC (id {})", model),
        }
    }

    fn read_flash_bytes() -> u32 {
        let mut sz: u32 = 0;
        let r = unsafe { esp_flash_get_physical_size(esp_flash_default_chip, &mut sz) };
        if r == ESP_OK && sz > 0 {
            sz
        } else {
            16 * 1024 * 1024
        }
    }

    fn chip_info() -> EspChipInfoRaw {
        let mut info: EspChipInfoRaw = unsafe { core::mem::zeroed() };
        unsafe { esp_chip_info(&mut info) };
        info
    }

    pub(super) fn resolved_board_id() -> String {
        let info = chip_info();
        let model = info.model as u32;
        let slug = model_id_slug(model);
        let flash = read_flash_bytes();
        let bucket = flash_mb_nearest_manifest_bucket(flash);
        format!("{}-{}mb", slug, bucket)
    }

    /// `has_psram`: 由调用方从 `Platform::memory_snapshot().heap_free_spiram > 0` 传入，
    /// 避免 platform 层直接依赖 orchestrator。
    pub(super) fn hardware_summary_line(has_psram: bool) -> String {
        let info = chip_info();
        let model = info.model as u32;
        let pretty = model_id_label(model);
        let flash = read_flash_bytes();
        super::human_hardware_summary(&pretty, flash, info.cores as u32, has_psram)
    }

    pub(super) fn chip_model_revision_cores() -> (String, u32, u32) {
        let info = chip_info();
        let model = info.model as u32;
        (
            model_id_label(model),
            info.revision as u32,
            info.cores as u32,
        )
    }
}

/// HTTP / OTA 使用的板型键：ESP 为「片型 + Flash 桶」；Linux 为 `linux`；其它宿主为 `host`。
pub fn resolved_board_id() -> String {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        esp::resolved_board_id()
    }
    #[cfg(all(
        not(any(target_arch = "xtensa", target_arch = "riscv32")),
        target_os = "linux",
    ))]
    {
        "linux".to_string()
    }
    #[cfg(all(
        not(any(target_arch = "xtensa", target_arch = "riscv32")),
        not(target_os = "linux"),
    ))]
    {
        "host".to_string()
    }
}

/// 给人看的单行硬件摘要（ESP）；其它目标由 `system_info` / `board_info` 另行组装。
/// `has_psram`：由调用方从 `Platform::memory_snapshot().heap_free_spiram > 0` 传入。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn hardware_summary_line(has_psram: bool) -> String {
    esp::hardware_summary_line(has_psram)
}

/// 供 `board_info` JSON：`chip_model` 字符串、revision、cores。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn esp_chip_model_revision_cores() -> (String, u32, u32) {
    esp::chip_model_revision_cores()
}

#[cfg(test)]
mod tests {
    use super::human_hardware_summary;

    #[test]
    fn formats_supported_soc_for_humans() {
        assert_eq!(
            human_hardware_summary("ESP32-S3", 16 * 1024 * 1024, 2, true),
            "ESP32-S3 (16MB Flash, 2-core CPU)"
        );
    }

    #[test]
    fn formats_unknown_soc_without_diagnostic_noise() {
        assert_eq!(
            human_hardware_summary("Unsupported SoC (id 7)", 8 * 1024 * 1024, 1, false),
            "Beetle device (Unsupported SoC (id 7)) (8MB Flash, 1-core CPU, without PSRAM)"
        );
    }
}
