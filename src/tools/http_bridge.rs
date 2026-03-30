//! ToolContext 到 PlatformHttpClient 的桥接，供需要旧 HTTP 接口的模块复用。
//! Bridge ToolContext to PlatformHttpClient for legacy HTTP call sites.

use crate::error::Result;
use crate::platform::{PlatformHttpClient, ResponseBody};
use crate::tools::ToolContext;

pub(crate) struct ToolContextHttpClient<'a> {
    ctx: &'a mut dyn ToolContext,
}

impl<'a> ToolContextHttpClient<'a> {
    pub(crate) fn new(ctx: &'a mut dyn ToolContext) -> Self {
        Self { ctx }
    }
}

impl PlatformHttpClient for ToolContextHttpClient<'_> {
    fn get(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        self.ctx.get_with_headers(url, headers)
    }

    fn post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.ctx.post_with_headers(url, headers, body)
    }

    fn post_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        self.ctx
            .post_streaming(url, headers, body, max_response_bytes, on_chunk)
    }

    fn patch(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.ctx.patch_with_headers(url, headers, body)
    }

    fn put(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.ctx.put_with_headers(url, headers, body)
    }

    fn delete(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        self.ctx.delete_with_headers(url, headers)
    }
}
