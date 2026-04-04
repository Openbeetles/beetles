//! 通道层 HTTP 抽象；由 lib 中 PlatformHttpClient blanket 实现，供 flush 等使用。

use crate::error::Result;

pub trait ChannelHttpClient {
    fn http_get(&mut self, url: &str) -> Result<(u16, crate::platform::ResponseBody)>;
    fn http_get_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, crate::platform::ResponseBody)>;
    fn http_post(&mut self, url: &str, body: &[u8])
        -> Result<(u16, crate::platform::ResponseBody)>;
    fn http_post_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, crate::platform::ResponseBody)>;
    /// HTTP PATCH with custom headers; default falls back to POST (override for real PATCH support).
    fn http_patch_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, crate::platform::ResponseBody)> {
        self.http_post_with_headers(url, headers, body)
    }

    fn reset_connection_for_retry(&mut self) {}
}
