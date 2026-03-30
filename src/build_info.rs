//! 编译期 OTA manifest URL（`option_env!`）；板型键由 `platform::runtime_board::resolved_board_id()` 运行期拼装。
//! Build-time OTA manifest URL; board id is composed at runtime (see `platform::runtime_board`).

/// OTA 渠道清单 URL；空则 GET /api/ota/check 视为渠道未配置。CI/Release 构建时设 OTA_MANIFEST_URL。
#[inline(always)]
pub fn ota_manifest_url() -> &'static str {
    option_env!("OTA_MANIFEST_URL").unwrap_or("")
}
