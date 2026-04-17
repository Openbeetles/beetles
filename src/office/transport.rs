#![cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]

use crate::error::{Error, Result};
use crate::platform::ResponseBody;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeStreamingResponse {
    pub status: u16,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficeBoundedBytes {
    pub status: u16,
    pub bytes: Vec<u8>,
    pub truncated: bool,
}

pub trait OfficeHttpClient {
    fn request_with_headers(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<(u16, ResponseBody)> {
        match method {
            "GET" => self.get_with_headers(url, headers),
            "POST" => self.post_with_headers(url, headers, body.unwrap_or_default()),
            "PATCH" => self.patch_with_headers(url, headers, body.unwrap_or_default()),
            "PUT" => self.put_with_headers(url, headers, body.unwrap_or_default()),
            "DELETE" => self.delete_with_headers(url, headers),
            other => Err(Error::config(
                "office_http_method",
                format!("unsupported http method: {other}"),
            )),
        }
    }

    fn get_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, ResponseBody)>;

    fn get_streaming_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        _max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<OfficeStreamingResponse> {
        let (status, body) = self.get_with_headers(url, headers)?;
        on_chunk(body.as_slice())?;
        Ok(OfficeStreamingResponse {
            status,
            truncated: false,
        })
    }

    fn post_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)>;

    fn patch_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.post_with_headers(url, headers, body)
    }

    fn put_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.post_with_headers(url, headers, body)
    }

    fn delete_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, ResponseBody)> {
        self.get_with_headers(url, headers)
    }
}

#[derive(Default)]
pub struct UnavailableOfficeHttpClient;

impl OfficeHttpClient for UnavailableOfficeHttpClient {
    fn get_with_headers(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
    ) -> Result<(u16, ResponseBody)> {
        Err(Error::config(
            "office_http_unavailable",
            "office HTTP transport is not available in this runtime context",
        ))
    }

    fn post_with_headers(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        Err(Error::config(
            "office_http_unavailable",
            "office HTTP transport is not available in this runtime context",
        ))
    }
}

impl OfficeHttpClient for crate::platform::EspHttpClient {
    fn request_with_headers(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<(u16, ResponseBody)> {
        crate::platform::PlatformHttpClient::request(self, method, url, headers, body)
    }

    fn get_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, ResponseBody)> {
        crate::platform::PlatformHttpClient::get(self, url, headers)
    }

    fn get_streaming_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<OfficeStreamingResponse> {
        let mut streamed_bytes = 0usize;
        let limit = max_response_bytes.filter(|value| *value > 0);
        let status = crate::platform::PlatformHttpClient::get_streaming(
            self,
            url,
            headers,
            max_response_bytes,
            &mut |chunk| {
                streamed_bytes = streamed_bytes.saturating_add(chunk.len());
                on_chunk(chunk)
            },
        )?;
        Ok(OfficeStreamingResponse {
            status,
            truncated: limit.map(|value| streamed_bytes >= value).unwrap_or(false),
        })
    }

    fn post_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        crate::platform::PlatformHttpClient::post(self, url, headers, body)
    }

    fn patch_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        crate::platform::PlatformHttpClient::patch(self, url, headers, body)
    }

    fn put_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        crate::platform::PlatformHttpClient::put(self, url, headers, body)
    }

    fn delete_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, ResponseBody)> {
        crate::platform::PlatformHttpClient::delete(self, url, headers)
    }
}

pub fn read_bounded_http_bytes(
    http: &mut dyn OfficeHttpClient,
    stage: &'static str,
    url: &str,
    headers: &[(&str, &str)],
    max_response_bytes: usize,
) -> Result<OfficeBoundedBytes> {
    let mut bytes = Vec::new();
    let response = http.get_streaming_with_headers(
        url,
        headers,
        Some(max_response_bytes.max(1)),
        &mut |chunk| {
            bytes.extend_from_slice(chunk);
            Ok(())
        },
    )?;
    if !(200..=299).contains(&response.status) {
        return Err(Error::http(stage, response.status));
    }
    let truncated = response.truncated || bytes.len() >= max_response_bytes.max(1);
    Ok(OfficeBoundedBytes {
        status: response.status,
        bytes,
        truncated,
    })
}
