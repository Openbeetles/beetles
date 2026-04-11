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
/// CORS + Content-Type: text/plain，用于 GET /api/soul、GET /api/user 的 200 响应。
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
/// 配置页公共 CSS 响应头。
pub const CSS_HEADERS: &[(&str, &str)] = &[
    ("Access-Control-Allow-Origin", "*"),
    ("Content-Type", "text/css; charset=utf-8"),
];
/// 配置页公共 JS 响应头。
pub const JS_HEADERS: &[(&str, &str)] = &[
    ("Access-Control-Allow-Origin", "*"),
    ("Content-Type", "application/javascript; charset=utf-8"),
];
/// GET / 未激活时 302 重定向到配对页。
pub const REDIRECT_PAIRING_HEADERS: &[(&str, &str)] = &[
    ("Access-Control-Allow-Origin", "*"),
    ("Location", "/pairing"),
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

    #[test]
    fn read_body_utf8_impl_respects_max_len_with_known_content_length() {
        let payload = b"hello-world";
        let mut cursor = payload.as_slice();
        let body =
            read_body_utf8_impl(&mut cursor, Some(payload.len() as u64), 5).expect("read body");
        assert_eq!(body, "hello");
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

/// 从 URI 中解析 query 参数 token 的值；无 token 或格式不对返回 None。
pub fn token_from_uri(uri: &str) -> Option<&str> {
    let query = uri.find('?').map(|i| &uri[i + 1..]).unwrap_or("");
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next()?.eq_ignore_ascii_case("token") {
            return it.next().filter(|s| !s.is_empty());
        }
    }
    None
}

/// 从 URI 中解析 query 参数 code 的值（配对码）；无或空返回 None。
pub fn code_from_uri(uri: &str) -> Option<&str> {
    let query = uri.find('?').map(|i| &uri[i + 1..]).unwrap_or("");
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next()?.eq_ignore_ascii_case("code") {
            return it.next().filter(|s| !s.is_empty());
        }
    }
    None
}

/// 从 URI 中解析 query 参数 restart 是否为 1；用于 POST /api/config/wifi 保存成功后可选触发重启。
pub fn restart_requested_from_uri(uri: &str) -> bool {
    let query = uri.find('?').map(|i| &uri[i + 1..]).unwrap_or("");
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next().is_some_and(|k| k.eq_ignore_ascii_case("restart")) {
            return it.next().is_some_and(|v| v.trim() == "1");
        }
    }
    false
}

/// 从 URI 中解析 query 参数 name 的值；无或空返回 None。
pub fn name_from_uri(uri: &str) -> Option<String> {
    let query = uri.find('?').map(|i| &uri[i + 1..]).unwrap_or("");
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next()?.eq_ignore_ascii_case("name") {
            return it
                .next()
                .filter(|s| !s.is_empty())
                .map(crate::util::percent_decode_query);
        }
    }
    None
}

/// 从 URI 中解析 query 参数 channel 的值；无或空返回 "stable"。仅 OTA 检查更新时使用。
#[cfg(feature = "ota")]
pub fn channel_from_uri(uri: &str) -> String {
    let query = uri.find('?').map(|i| &uri[i + 1..]).unwrap_or("");
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        let key = match it.next() {
            Some(k) => k,
            None => continue,
        };
        if !key.eq_ignore_ascii_case("channel") {
            continue;
        }
        let v = it.next().unwrap_or("stable").trim();
        return if v.is_empty() { "stable" } else { v }.to_string();
    }
    "stable".to_string()
}

/// POST /api/config/wifi 请求体。
#[derive(serde::Deserialize)]
pub struct WifiConfigPayload {
    #[serde(default)]
    pub wifi_ssid: String,
    #[serde(default)]
    pub wifi_pass: String,
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
    pub fn err_403(msg: &str) -> Self {
        Self {
            status: 403,
            status_text: "Forbidden",
            body: Self::json_error_body(msg),
        }
    }
    pub fn err_500(msg: &str) -> Self {
        Self {
            status: 500,
            status_text: "Internal Server Error",
            body: Self::json_error_body(msg),
        }
    }
    pub fn err_503(msg: &str) -> Self {
        Self {
            status: 503,
            status_text: "Service Unavailable",
            body: Self::json_error_body(msg),
        }
    }
    pub fn err_413(msg: &str) -> Self {
        Self {
            status: 413,
            status_text: "Payload Too Large",
            body: Self::json_error_body(msg),
        }
    }
    pub fn err_404(msg: &str) -> Self {
        Self {
            status: 404,
            status_text: "Not Found",
            body: Self::json_error_body(msg),
        }
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
/// WiFi 配置页 HTML 响应头。
pub const HTML_HEADERS: &[(&str, &str)] = &[
    ("Access-Control-Allow-Origin", "*"),
    ("Access-Control-Allow-Private-Network", "true"),
    ("Content-Type", "text/html; charset=utf-8"),
];
