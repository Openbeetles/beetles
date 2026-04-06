//! GET/POST /api/soul：配对后读/写 SOUL 配置（config/SOUL.md）。

use crate::i18n::{locale_from_store, tr, tr_error, Message};
use crate::memory::MAX_SOUL_USER_LEN;
use crate::platform::http_server::common::ApiResponse;
use crate::state;

use super::HandlerContext;

/// GET：Ok(content) 写 200 text/plain，Err(msg) 写 500 JSON。
pub fn get_body(ctx: &HandlerContext) -> Result<String, String> {
    ctx.memory_store
        .get_soul()
        .map_err(|e| state::sanitize_error_for_log(&e))
}

/// POST：body 为原始请求体；is_json 为 true 时从 {"content":"..."} 取 content。
pub fn post(ctx: &HandlerContext, body: String, is_json: bool) -> ApiResponse {
    let loc = locale_from_store(ctx.config_store.as_ref());
    let content = if is_json {
        serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("content").and_then(|c| c.as_str()).map(String::from))
            .unwrap_or(body)
    } else {
        body
    };
    let content = content.trim();
    if content.len() > MAX_SOUL_USER_LEN {
        return ApiResponse::err_400(&tr(Message::ContentTooLong, loc));
    }
    match ctx.memory_store.set_soul(content) {
        Ok(()) => ApiResponse::ok_200_json("{\"ok\":true}"),
        Err(e) => ApiResponse::err_500(&tr_error(&e, loc)),
    }
}
