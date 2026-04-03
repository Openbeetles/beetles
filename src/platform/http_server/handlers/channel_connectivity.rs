//! GET /api/channel_connectivity：按当前启用通道现场探测连通性，供设备页展示。

use super::HandlerContext;
use crate::i18n::locale_from_store;

/// 成功返回 `{ "channels": [ ... ] }` 字符串，失败返回 Err（mod 层写 500，不暴露内部细节）。
pub fn body(ctx: &HandlerContext) -> Result<String, String> {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let config = ctx.config().clone();
    let mut http = ctx
        .platform
        .create_http_client(&config)
        .map_err(|e| e.to_string())?;
    let snapshot = crate::channels::build_snapshot(&config, http.as_mut(), loc);
    serde_json::to_string(&snapshot).map_err(|e| e.to_string())
}
