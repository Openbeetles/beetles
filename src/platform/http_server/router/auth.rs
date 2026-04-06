//! 配对 / CSRF 检查，替代仅 ESP 宏可用的逻辑。
//! Pairing and CSRF checks (replaces macros that need Esp request types).

use crate::i18n::{locale_from_store, tr, Message};
use crate::platform::csrf;
use crate::platform::http_server::common::{self, ApiResponse};
use crate::platform::pairing;
use crate::platform::ConfigStore;

/// 未激活则返回 401 JSON（与 `require_activated!` 一致）。
pub fn require_activated(store: &dyn ConfigStore) -> Option<ApiResponse> {
    if !pairing::code_set(store) {
        let loc = locale_from_store(store);
        let msg = tr(Message::PairingRequired, loc);
        return Some(ApiResponse::err_401(&msg));
    }
    None
}

/// 配对码鉴权：已激活且本次请求带配对码（header / query，与 `guard_pairing_csrf` 一致）。
/// 用于所有敏感配置读写接口；写操作额外叠加 CSRF。
pub fn require_pairing_code(
    store: &dyn ConfigStore,
    uri: &str,
    headers: &[(String, String)],
) -> Option<ApiResponse> {
    if !pairing::code_set(store) {
        let loc = locale_from_store(store);
        let msg = tr(Message::PairingRequired, loc);
        return Some(ApiResponse::err_401(&msg));
    }
    let code = common::code_from_uri(uri)
        .map(String::from)
        .or_else(|| header_ci(headers, "X-Pairing-Code").map(String::from));
    match code.as_deref() {
        Some(c) if !c.is_empty() => {
            if !pairing::verify_code(store, c) {
                let loc = locale_from_store(store);
                let msg = tr(Message::PairingCodeWrong, loc);
                return Some(ApiResponse::err_401(&msg));
            }
        }
        _ => {
            let loc = locale_from_store(store);
            let msg = tr(Message::PairingCodeWrong, loc);
            return Some(ApiResponse::err_401(&msg));
        }
    }
    None
}

fn header_ci<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// CSRF（与 `require_csrf!` 一致）。
pub fn require_csrf(store: &dyn ConfigStore, headers: &[(String, String)]) -> Option<ApiResponse> {
    let loc = locale_from_store(store);
    let token = header_ci(headers, "X-CSRF-Token").or_else(|| header_ci(headers, "x-csrf-token"));
    match token {
        Some(t) if csrf::verify_token(t) => None,
        Some(_) => Some(ApiResponse::err_403(&tr(Message::CsrfInvalidToken, loc))),
        None => Some(ApiResponse::err_403(&tr(Message::CsrfTokenRequired, loc))),
    }
}
