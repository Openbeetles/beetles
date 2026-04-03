//! OTA implementation kept inside `platform/` so business modules do not touch ESP-IDF APIs.
//! OTA 实现收口到 `platform/`，避免业务域直接依赖 ESP-IDF。

#![cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]

use crate::error::{Error, Result};
use std::ffi::CString;

const TAG: &str = "ota";
const OTA_TIMEOUT_MS: i32 = 120_000;
const OTA_BUFFER_SIZE: i32 = 4096;

/// 从 URL 拉取固件并写入 OTA 分区；成功后由调用方重启。失败返回 Error（带 stage），不破坏当前分区。
pub fn ota_update_from_url(url: &str) -> Result<()> {
    let url_c = CString::new(url).map_err(|e| Error::config("ota_url", e.to_string()))?;

    let mut http_config = esp_idf_svc::sys::esp_http_client_config_t::default();
    http_config.url = url_c.as_ptr();
    http_config.timeout_ms = OTA_TIMEOUT_MS;
    http_config.buffer_size = OTA_BUFFER_SIZE;
    http_config.crt_bundle_attach = Some(esp_idf_svc::sys::esp_crt_bundle_attach);

    let mut ota_config = esp_idf_svc::sys::esp_https_ota_config_t::default();
    ota_config.http_config = &http_config;

    log::info!("[{}] Starting OTA from: {}", TAG, url);

    let ret = unsafe { esp_idf_svc::sys::esp_https_ota(&ota_config) };

    if ret == esp_idf_svc::sys::ESP_OK {
        log::info!("[{}] OTA successful", TAG);
        Ok(())
    } else {
        let stage = if ret == esp_idf_svc::sys::ESP_ERR_OTA_VALIDATE_FAILED {
            "ota_validate"
        } else if ret == esp_idf_svc::sys::ESP_ERR_FLASH_OP_TIMEOUT
            || ret == esp_idf_svc::sys::ESP_ERR_FLASH_OP_FAIL
        {
            "ota_write"
        } else {
            "ota_download"
        };
        log::error!("[{}] OTA failed: {} (0x{:x})", TAG, ret, ret as u32);
        Err(Error::esp(stage, ret))
    }
}
