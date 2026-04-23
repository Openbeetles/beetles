//! 配对 / CSRF 检查，替代仅 ESP 宏可用的逻辑。
//! Pairing and CSRF checks (replaces macros that need Esp request types).

use crate::platform::csrf;
use crate::platform::http_server::common::{self, ApiResponse};
use crate::platform::pairing;
use crate::platform::ConfigStore;

/// 未激活则返回 401 JSON（与 `require_activated!` 一致）。
pub fn require_activated(store: &dyn ConfigStore) -> Option<ApiResponse> {
    if !pairing::code_set(store) {
        return Some(ApiResponse::err_401_key("auth.pairing_required"));
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
    let stored_code = match pairing::get_valid_code(store) {
        Some(code) => code,
        None => return Some(ApiResponse::err_401_key("auth.pairing_required")),
    };
    let code = common::code_from_uri(uri)
        .map(String::from)
        .or_else(|| header_ci(headers, "X-Pairing-Code").map(String::from));
    match code.as_deref() {
        Some(candidate) if !candidate.is_empty() => {
            if !pairing::verify_code_value(&stored_code, candidate) {
                return Some(ApiResponse::err_401_key("auth.pairing_invalid"));
            }
        }
        _ => {
            return Some(ApiResponse::err_401_key("auth.pairing_invalid"));
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
pub fn require_csrf(_store: &dyn ConfigStore, headers: &[(String, String)]) -> Option<ApiResponse> {
    let token = header_ci(headers, "X-CSRF-Token").or_else(|| header_ci(headers, "x-csrf-token"));
    match token {
        Some(t) if csrf::verify_token(t) => None,
        Some(_) => Some(ApiResponse::err_403_key("auth.csrf_invalid")),
        None => Some(ApiResponse::err_403_key("auth.csrf_required")),
    }
}

#[cfg(test)]
mod tests {
    use super::require_pairing_code;
    use crate::error::Result;
    use crate::platform::ConfigStore;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingStore {
        reads: AtomicUsize,
    }

    impl ConfigStore for CountingStore {
        fn read_string(&self, key: &str) -> Result<Option<String>> {
            assert_eq!(key, "pairing_code");
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(Some("123456".to_string()))
        }

        fn write_string(&self, _key: &str, _value: &str) -> Result<()> {
            Ok(())
        }

        fn erase_keys(&self, _keys: &[&str]) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn require_pairing_code_reads_pairing_code_once_on_valid_request() {
        let store = CountingStore {
            reads: AtomicUsize::new(0),
        };
        let headers = [("X-Pairing-Code".to_string(), "123456".to_string())];

        assert!(require_pairing_code(&store, "/api/config/system", &headers).is_none());
        assert_eq!(store.reads.load(Ordering::SeqCst), 1);
    }
}
