//! HTTP 响应公共处理：2xx 检查与 body 截断，供 fetch_url 与 HandlerContext 复用。
//! Shared 2xx check + body truncation for GET responses.

use crate::error::{Error, Result};
use crate::platform::ResponseBody;

/// 若 status 在 200..300 则截断 body 至 max_len 并返回，否则返回 Err。
pub fn check_2xx_and_truncate(
    stage: &'static str,
    status: u16,
    mut body: Vec<u8>,
    max_len: usize,
) -> Result<Vec<u8>> {
    if !(200..300).contains(&status) {
        return Err(Error::http(stage, status));
    }
    if body.len() > max_len {
        body.truncate(max_len);
    }
    Ok(body)
}

/// 2xx 检查并原地截断 [`ResponseBody`]，避免 PSRAM 响应体在热路径回拷到 `Vec`。
pub fn check_2xx_and_truncate_body(
    stage: &'static str,
    status: u16,
    mut body: ResponseBody,
    max_len: usize,
) -> Result<ResponseBody> {
    if !(200..300).contains(&status) {
        return Err(Error::http(stage, status));
    }
    body.truncate(max_len);
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::check_2xx_and_truncate_body;
    use crate::platform::ResponseBody;

    #[test]
    fn check_2xx_and_truncate_body_keeps_response_body_borrowable() {
        let body = ResponseBody::Heap(b"abcdef".to_vec());

        let body = check_2xx_and_truncate_body("fetch_url", 200, body, 4).expect("2xx body");

        assert_eq!(body.as_slice(), b"abcd");
    }
}
