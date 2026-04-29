//! 同步 GET URL 返回 body；供 HTTP handler 等经 `PlatformHttpClient` 调用。
//! Synchronous GET URL to bytes; used with an injected `PlatformHttpClient`.

use crate::error::Result;
use crate::platform::response::check_2xx_and_truncate_body;
use crate::platform::{PlatformHttpClient, ResponseBody};

/// 用已有的 HTTP 客户端 GET url，返回 body 截断至 max_len。
pub fn fetch_url_with_client(
    client: &mut dyn PlatformHttpClient,
    url: &str,
    max_len: usize,
) -> Result<ResponseBody> {
    let (status, body) = client.get(url, &[])?;
    check_2xx_and_truncate_body("fetch_url", status, body, max_len)
}
