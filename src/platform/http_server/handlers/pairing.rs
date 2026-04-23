//! GET /api/pairing_code：返回是否已设置配对码（不返回明文）及当前 locale。POST：仅未设置时接受 body 设置码。

use crate::config;
use crate::platform::http_server::api_contract;
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
    let payload: SetCodePayload = match serde_json::from_str(body_json) {
        Ok(p) => p,
        Err(_) => return ApiResponse::err_400_key(api_contract::COMMON_INVALID_JSON),
    };
    let code = payload.code.trim();
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return ApiResponse::err_400_key(api_contract::PAIRING_CODE_INVALID);
    }
    match pairing::set_code_checked(ctx.config_store.as_ref(), code) {
        Ok(pairing::SetCodeOutcome::Stored) => {
            crate::runtime::sync_pairing_state_from_store(ctx.config_store.as_ref());
            ApiResponse::ok_200_json(r#"{"ok":true}"#)
        }
        Ok(pairing::SetCodeOutcome::AlreadySet) => {
            ApiResponse::err_400_key(api_contract::PAIRING_ALREADY_SET)
        }
        Ok(pairing::SetCodeOutcome::Invalid) => {
            ApiResponse::err_400_key(api_contract::PAIRING_CODE_INVALID)
        }
        Err(_) => ApiResponse::err_500_key(api_contract::PAIRING_SAVE_FAILED),
    }
}
