//! HTTP 服务器公共常量与辅助函数，与架构无关。

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
use embedded_io::Read;
use std::fmt::Debug;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const MAX_OPEN_SOCKETS: usize = 4;
pub const POST_BODY_MAX_LEN: usize = 4096;
pub const RESTART_COOLDOWN_SECS: u64 = 60;

/// CORS 头：所有 API 及 GET / 响应必须带，供外置配置页跨域调用。
pub const CORS_HEADERS: &[(&str, &str)] = &[
    ("Access-Control-Allow-Origin", "*"),
    ("Access-Control-Allow-Private-Network", "true"),
];
/// CORS + Content-Type: text/plain，用于返回纯文本正文的 200 响应。
pub const CORS_AND_TEXT_PLAIN: &[(&str, &str)] = &[
    ("Access-Control-Allow-Origin", "*"),
    ("Access-Control-Allow-Private-Network", "true"),
    ("Content-Type", "text/plain"),
];
/// OPTIONS 预检响应：带 1 字节 body，迫使部分嵌入式栈先发送头再写 body，避免"响应头为空"。
pub const CORS_OPTIONS_HEADERS: &[(&str, &str)] = &[
    ("Access-Control-Allow-Origin", "*"),
    ("Access-Control-Allow-Private-Network", "true"),
    ("Access-Control-Allow-Methods", "GET, POST, DELETE, OPTIONS"),
    (
        "Access-Control-Allow-Headers",
        "Content-Type, X-Pairing-Code, X-CSRF-Token, x-csrf-token",
    ),
    ("Content-Type", "text/plain; charset=utf-8"),
    ("Content-Length", "1"),
];
/// 读 body 时的错误：读失败或非 UTF-8。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
#[derive(Debug)]
pub enum BodyReadError {
    ReadFailed,
    InvalidUtf8,
}

/// 无 Content-Length 时首次分配大小，避免小 POST 也占满 4KB。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
const BODY_READ_CHUNK_INITIAL: usize = 1024;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
const BODY_READ_CHUNK_SIZE: usize = 512;

/// 从请求体读取 UTF-8 字符串，上限 max_len。有 content_len 时单次分配；无时按块读取，减少小 body 的分配。
/// 使用 embedded_io::Read，与 ESP 的 Request 实现一致。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
pub fn read_body_utf8_impl<R: Read>(
    r: &mut R,
    content_len: Option<u64>,
    max_len: usize,
) -> Result<String, BodyReadError> {
    let target_len = content_len
        .map(|l| (l.min(max_len as u64)) as usize)
        .unwrap_or(BODY_READ_CHUNK_INITIAL.min(max_len));
    let mut buf = Vec::with_capacity(target_len);
    let mut chunk = [0u8; BODY_READ_CHUNK_SIZE];
    loop {
        let remain = max_len.saturating_sub(buf.len());
        if remain == 0 {
            break;
        }
        let n = Read::read(r, &mut chunk[..remain.min(BODY_READ_CHUNK_SIZE)])
            .map_err(|_| BodyReadError::ReadFailed)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if content_len.is_some() && buf.len() >= target_len {
            break;
        }
    }
    String::from_utf8(buf).map_err(|_| BodyReadError::InvalidUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn read_body_utf8_impl_respects_max_len_with_known_content_length() {
        let payload = b"hello-world";
        let mut cursor = payload.as_slice();
        let body =
            read_body_utf8_impl(&mut cursor, Some(payload.len() as u64), 5).expect("read body");
        assert_eq!(body, "hello");
    }

    #[test]
    fn api_response_error_body_uses_error_key_contract() {
        let body = ApiResponse::err_400_key("common.invalid_json").body;
        let parsed: Value = serde_json::from_slice(&body).expect("parse api error body");

        assert_eq!(parsed["error_key"], "common.invalid_json");
        assert!(
            parsed.get("error").is_none(),
            "body={}",
            String::from_utf8_lossy(&body)
        );
    }

    #[test]
    fn api_response_error_body_can_include_upstream_error() {
        let body = ApiResponse::err_400_key_with_upstream(
            "office.provider_error",
            Some("imap login failed"),
            Some(401),
        )
        .body;
        let parsed: Value = serde_json::from_slice(&body).expect("parse api error body");

        assert_eq!(parsed["error_key"], "office.provider_error");
        assert_eq!(parsed["upstream_error"], "imap login failed");
        assert_eq!(parsed["upstream_status"], 401);
        assert!(
            parsed.get("error").is_none(),
            "body={}",
            String::from_utf8_lossy(&body)
        );
    }
}

/// 常量时间比较，避免 token 时序侧信道。
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// 从 URI 中按不区分大小写的 key 提取 query 参数值；空值视为缺失。
pub fn query_param_from_uri<'a>(uri: &'a str, key: &str) -> Option<&'a str> {
    let query = uri.find('?').map(|i| &uri[i + 1..]).unwrap_or("");
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it
            .next()
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(key))
        {
            return it.next().filter(|value| !value.trim().is_empty());
        }
    }
    None
}

/// 从 URI 中提取布尔型 query flag；无值时视为 true。
pub fn query_flag_from_uri(uri: &str, key: &str) -> bool {
    let query = uri.find('?').map(|index| &uri[index + 1..]).unwrap_or("");
    query.split('&').any(|pair| {
        let mut it = pair.splitn(2, '=');
        let Some(candidate) = it.next() else {
            return false;
        };
        if !candidate.eq_ignore_ascii_case(key) {
            return false;
        }
        match it.next().map(str::trim) {
            None => true,
            Some("") => true,
            Some("1" | "true" | "yes" | "on") => true,
            Some(_) => false,
        }
    })
}

/// 从 URI 中解析 query 参数 token 的值；无 token 或格式不对返回 None。
pub fn token_from_uri(uri: &str) -> Option<&str> {
    query_param_from_uri(uri, "token")
}

/// 从 URI 中解析 query 参数 code 的值（配对码）；无或空返回 None。
pub fn code_from_uri(uri: &str) -> Option<&str> {
    query_param_from_uri(uri, "code")
}

/// 从 URI 中解析 query 参数 restart 是否为 1；用于支持 restart=1 的配置保存路由在成功后可选触发重启。
pub fn restart_requested_from_uri(uri: &str) -> bool {
    query_param_from_uri(uri, "restart").is_some_and(|value| value.trim() == "1")
}

/// 从 URI 中解析 query 参数 name 的值；无或空返回 None。
pub fn name_from_uri(uri: &str) -> Option<String> {
    query_param_from_uri(uri, "name").map(crate::util::percent_decode_query)
}

/// 从 URI 中解析 query 参数 channel 的值；无或空返回 "stable"。仅 OTA 检查更新时使用。
#[cfg(feature = "ota")]
pub fn channel_from_uri(uri: &str) -> String {
    query_param_from_uri(uri, "channel")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("stable")
        .to_string()
}

/// 将任意错误转为 std::io::Error，供 handler 闭包统一返回 HandlerResult。
pub fn to_io<E: Debug>(e: E) -> std::io::Error {
    std::io::Error::other(format!("{:?}", e))
}

/// Handler 闭包返回类型。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub type HandlerResult = std::result::Result<(), std::io::Error>;

/// POST 类 handler 统一响应：status + body，由 mod 写入。
#[derive(Clone)]
pub struct ApiResponse {
    pub status: u16,
    pub status_text: &'static str,
    pub body: Vec<u8>,
}

impl ApiResponse {
    fn json_error_body(msg: &str) -> Vec<u8> {
        let escaped = msg.replace('"', "\\\"");
        let mut body = Vec::with_capacity(escaped.len().saturating_add(12));
        body.extend_from_slice(br#"{"error":""#);
        body.extend_from_slice(escaped.as_bytes());
        body.extend_from_slice(br#""}"#);
        body
    }

    fn json_error_key_body(
        error_key: &str,
        error_params: Option<serde_json::Value>,
        error_stage: Option<&str>,
        upstream_error: Option<&str>,
        upstream_status: Option<u16>,
        provider_kind: Option<&str>,
        mut extra: serde_json::Map<String, serde_json::Value>,
    ) -> Vec<u8> {
        extra.insert(
            "error_key".to_string(),
            serde_json::Value::String(error_key.to_string()),
        );
        if let Some(params) = error_params {
            extra.insert("error_params".to_string(), params);
        }
        if let Some(stage) = error_stage {
            extra.insert(
                "error_stage".to_string(),
                serde_json::Value::String(stage.to_string()),
            );
        }
        if let Some(upstream_error) = upstream_error {
            extra.insert(
                "upstream_error".to_string(),
                serde_json::Value::String(upstream_error.to_string()),
            );
        }
        if let Some(upstream_status) = upstream_status {
            extra.insert(
                "upstream_status".to_string(),
                serde_json::Value::Number(upstream_status.into()),
            );
        }
        if let Some(provider_kind) = provider_kind {
            extra.insert(
                "provider_kind".to_string(),
                serde_json::Value::String(provider_kind.to_string()),
            );
        }
        serde_json::to_vec(&serde_json::Value::Object(extra))
            .unwrap_or_else(|_| br#"{"error_key":"common.operation_failed"}"#.to_vec())
    }

    pub fn err_key(status: u16, status_text: &'static str, error_key: &str) -> Self {
        Self {
            status,
            status_text,
            body: Self::json_error_key_body(
                error_key,
                None,
                None,
                None,
                None,
                None,
                serde_json::Map::new(),
            ),
        }
    }

    pub fn err_key_with_upstream(
        status: u16,
        status_text: &'static str,
        error_key: &str,
        upstream_error: Option<&str>,
        upstream_status: Option<u16>,
    ) -> Self {
        Self {
            status,
            status_text,
            body: Self::json_error_key_body(
                error_key,
                None,
                None,
                upstream_error,
                upstream_status,
                None,
                serde_json::Map::new(),
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn err_key_with_meta(
        status: u16,
        status_text: &'static str,
        error_key: &str,
        error_stage: Option<&str>,
        upstream_error: Option<&str>,
        upstream_status: Option<u16>,
        provider_kind: Option<&str>,
        extra: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        Self {
            status,
            status_text,
            body: Self::json_error_key_body(
                error_key,
                None,
                error_stage,
                upstream_error,
                upstream_status,
                provider_kind,
                extra,
            ),
        }
    }

    pub fn ok_200_json(json: &str) -> Self {
        Self {
            status: 200,
            status_text: "OK",
            body: json.as_bytes().to_vec(),
        }
    }
    pub fn err_400(msg: &str) -> Self {
        Self {
            status: 400,
            status_text: "Bad Request",
            body: Self::json_error_body(msg),
        }
    }
    pub fn err_400_key(error_key: &str) -> Self {
        Self::err_key(400, "Bad Request", error_key)
    }
    #[cfg(test)]
    pub fn err_400_key_with_upstream(
        error_key: &str,
        upstream_error: Option<&str>,
        upstream_status: Option<u16>,
    ) -> Self {
        Self::err_key_with_upstream(
            400,
            "Bad Request",
            error_key,
            upstream_error,
            upstream_status,
        )
    }
    #[allow(dead_code)]
    pub fn err_401_pairing() -> Self {
        Self {
            status: 401,
            status_text: "Unauthorized",
            body: Self::json_error_body("pairing required"),
        }
    }
    pub fn err_401(msg: &str) -> Self {
        Self {
            status: 401,
            status_text: "Unauthorized",
            body: Self::json_error_body(msg),
        }
    }
    pub fn err_401_key(error_key: &str) -> Self {
        Self::err_key(401, "Unauthorized", error_key)
    }
    pub fn err_403(msg: &str) -> Self {
        Self {
            status: 403,
            status_text: "Forbidden",
            body: Self::json_error_body(msg),
        }
    }
    pub fn err_403_key(error_key: &str) -> Self {
        Self::err_key(403, "Forbidden", error_key)
    }
    pub fn err_500(msg: &str) -> Self {
        Self {
            status: 500,
            status_text: "Internal Server Error",
            body: Self::json_error_body(msg),
        }
    }
    pub fn err_500_key(error_key: &str) -> Self {
        Self::err_key(500, "Internal Server Error", error_key)
    }
    pub fn err_500_key_with_upstream(
        error_key: &str,
        upstream_error: Option<&str>,
        upstream_status: Option<u16>,
    ) -> Self {
        Self::err_key_with_upstream(
            500,
            "Internal Server Error",
            error_key,
            upstream_error,
            upstream_status,
        )
    }
    pub fn err_503(msg: &str) -> Self {
        Self {
            status: 503,
            status_text: "Service Unavailable",
            body: Self::json_error_body(msg),
        }
    }
    pub fn err_503_key(error_key: &str) -> Self {
        Self::err_key(503, "Service Unavailable", error_key)
    }
    pub fn err_413(msg: &str) -> Self {
        Self {
            status: 413,
            status_text: "Payload Too Large",
            body: Self::json_error_body(msg),
        }
    }
    pub fn err_404_key(error_key: &str) -> Self {
        Self::err_key(404, "Not Found", error_key)
    }
    #[allow(dead_code)]
    pub fn err_502(msg: &str) -> Self {
        Self {
            status: 502,
            status_text: "Bad Gateway",
            body: Self::json_error_body(msg),
        }
    }
}
