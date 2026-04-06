//! GET /api/pairing_code：返回是否已设置配对码（不返回明文）及当前 locale。POST：仅未设置时接受 body 设置码。

use crate::config;
use crate::i18n::{locale_from_store, tr, Message};
use crate::platform::http_server::common::ApiResponse;
use crate::platform::pairing;

use super::HandlerContext;

/// GET 响应：`{"code_set":true|false,"locale":"zh"|"en"}`。
pub fn body(ctx: &HandlerContext) -> String {
    let code_set = pairing::code_set(ctx.config_store.as_ref());
    let locale = config::get_locale(ctx.config_store.as_ref());
    format!(r#"{{"code_set":{},"locale":"{}"}}"#, code_set, locale)
}

/// POST 请求体。
#[derive(serde::Deserialize)]
pub struct SetCodePayload {
    #[serde(default)]
    pub code: String,
}

/// POST 处理：仅当未设置时写入 6 位码。返回 ApiResponse。
pub fn post_body(ctx: &HandlerContext, body_json: &str) -> ApiResponse {
    let loc = locale_from_store(ctx.config_store.as_ref());
    if pairing::code_set(ctx.config_store.as_ref()) {
        return ApiResponse::err_400(&tr(Message::PairingCodeAlreadySet, loc));
    }
    let payload: SetCodePayload = match serde_json::from_str(body_json) {
        Ok(p) => p,
        Err(_) => return ApiResponse::err_400(&tr(Message::InvalidJson, loc)),
    };
    let code = payload.code.trim();
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return ApiResponse::err_400(&tr(Message::CodeMustBe6Digits, loc));
    }
    match pairing::set_code(ctx.config_store.as_ref(), code) {
        Ok(true) => {
            // 首次激活时顺带创建空 SOUL/USER 文件，避免后续 get_soul/get_user 报 No such file
            let _ = ctx
                .platform
                .write_config_file(crate::memory::REL_PATH_SOUL, b"");
            let _ = ctx
                .platform
                .write_config_file(crate::memory::REL_PATH_USER, b"");
            ApiResponse::ok_200_json(r#"{"ok":true}"#)
        }
        Ok(false) => ApiResponse::err_400(&tr(Message::PairingCodeAlreadySet, loc)),
        Err(_) => ApiResponse::err_500(&tr(Message::FailedToSaveCode, loc)),
    }
}
