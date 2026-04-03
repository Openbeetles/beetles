//! GET /api/channel_connectivity：返回后台缓存的通道连通性结果，供设备页展示。
//! 前台 handler 只读缓存，避免在 httpd 任务内串行执行外网 HTTP。

use super::HandlerContext;
use crate::i18n::locale_from_store;

const REFRESH_MAX_AGE_SECS: u64 = 60;

/// 成功返回 `{ "channels": [ ... ] }` 字符串，失败返回 Err（mod 层写 500，不暴露内部细节）。
pub fn body(ctx: &HandlerContext) -> Result<String, String> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let config = ctx.config().clone();
    let snapshot = ctx.channel_connectivity_cache.snapshot_or_fallback(
        &config,
        loc,
        crate::util::current_unix_secs(),
        REFRESH_MAX_AGE_SECS,
    );
    serde_json::to_string(&snapshot).map_err(|e| e.to_string())
}
