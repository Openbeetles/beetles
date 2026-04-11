//! Host/Linux：基于 `ureq` 的同步 HTTP(S) 客户端（rustls TLS），与 ESP 侧 `EspHttpClient` 契约对齐。
//! Host/Linux: synchronous HTTP(S) client via `ureq` (rustls TLS), aligned with ESP `EspHttpClient`.

use std::io::Read;
use std::time::Duration;

use crate::config::{parse_proxy_url_to_host_port, AppConfig};
use crate::error::{Error, Result};
use crate::orchestrator::Priority;
use crate::platform::ResponseBody;
use ureq::OrAnyStatus;

const TAG: &str = "platform::http_client";
/// 连接超时（毫秒），与 ESP `REQUEST_TIMEOUT_MS` 一致。
const CONNECT_TIMEOUT_MS: u64 = 15_000;
/// 读超时（毫秒）。Linux 流式 LLM 响应可能较慢，给 60s per-read 窗口（ESP 30s 受限于 lwIP）。
const READ_TIMEOUT_MS: u64 = 60_000;
/// 写超时（毫秒）。请求体通常不大，30s 足够。
const WRITE_TIMEOUT_MS: u64 = 30_000;
/// 与 ESP `TLS_ADMISSION_TIMEOUT_SECS` 一致。
const TLS_ADMISSION_TIMEOUT_SECS: u64 = 30;
const RESPONSE_READ_CHUNK: usize = 16 * 1024;
const INITIAL_RESPONSE_BODY_CAP: usize = 64 * 1024;
const MAX_DRAIN_BYTES: usize = 512 * 1024;

/// `https://` 代理 URL 在 ureq 中按 HTTP CONNECT 使用（与常见企业代理一致）。
fn normalize_proxy_for_ureq(trimmed: &str) -> String {
    if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("http://{}", rest)
    } else {
        trimmed.to_string()
    }
}

fn build_agent(proxy_url: Option<&str>, stage: &'static str) -> Result<ureq::Agent> {
    let mut b = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(CONNECT_TIMEOUT_MS))
        .timeout_read(Duration::from_millis(READ_TIMEOUT_MS))
        .timeout_write(Duration::from_millis(WRITE_TIMEOUT_MS));
    if let Some(url) = proxy_url {
        if !url.is_empty() {
            let proxy = ureq::Proxy::new(url).map_err(|e| Error::Other {
                source: Box::new(std::io::Error::other(e.to_string())),
                stage,
            })?;
            b = b.proxy(proxy);
        }
    }
    Ok(b.build())
}

fn transport_to_error(e: ureq::Transport, stage: &'static str) -> Error {
    Error::Other {
        source: Box::new(std::io::Error::other(e.to_string())),
        stage,
    }
}

fn drain_reader<R: Read>(r: &mut R) {
    let mut buf = [0u8; 512];
    let mut total = 0usize;
    loop {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                total += n;
                if total >= MAX_DRAIN_BYTES {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn read_response_body_from_reader<R: Read>(mut reader: R) -> Result<ResponseBody> {
    let max_len = crate::orchestrator::current_budget().response_body_max;
    let mut out = Vec::with_capacity(INITIAL_RESPONSE_BODY_CAP.min(max_len));
    let mut buf = [0u8; RESPONSE_READ_CHUNK];
    loop {
        let n = reader.read(&mut buf).map_err(|e| Error::Other {
            source: Box::new(e),
            stage: "http_read",
        })?;
        if n == 0 {
            break;
        }
        let remain = max_len.saturating_sub(out.len());
        if remain == 0 {
            log::warn!("[{}] response body truncated at {} bytes", TAG, max_len);
            drain_reader(&mut reader);
            break;
        }
        let take = n.min(remain);
        out.extend_from_slice(&buf[..take]);
        if take < n {
            drain_reader(&mut reader);
            break;
        }
    }
    Ok(ResponseBody::Heap(out))
}

fn apply_headers(mut req: ureq::Request, headers: &[(&str, &str)]) -> ureq::Request {
    for (k, v) in headers {
        req = req.set(k, v);
    }
    req
}

/// Linux/host 上基于 `ureq::Agent` 的 HTTP 客户端；对外类型名与 ESP 一致，便于 `Platform::create_http_client` 统一。
pub struct EspHttpClient {
    agent: ureq::Agent,
    priority: Priority,
    /// 归一化后的代理 URL（重建连接时复用）；无代理时为 `None`。
    proxy_url_normalized: Option<String>,
}

impl EspHttpClient {
    pub fn new() -> Result<Self> {
        Self::new_optional_proxy(None, Priority::Normal)
    }

    pub fn new_with_priority(priority: Priority) -> Result<Self> {
        Self::new_optional_proxy(None, priority)
    }

    pub fn new_with_config(config: &AppConfig) -> Result<Self> {
        let trimmed = config.proxy_url.trim();
        if trimmed.is_empty() {
            return Self::new_optional_proxy(None, Priority::Normal);
        }
        parse_proxy_url_to_host_port(trimmed)
            .ok_or_else(|| Error::config("http_client_new", "invalid proxy_url"))?;
        let normalized = normalize_proxy_for_ureq(trimmed);
        Self::new_optional_proxy(Some(normalized), Priority::Normal)
    }

    fn new_optional_proxy(proxy_url: Option<String>, priority: Priority) -> Result<Self> {
        let agent = build_agent(proxy_url.as_deref(), "http_client_new")?;
        Ok(EspHttpClient {
            agent,
            priority,
            proxy_url_normalized: proxy_url,
        })
    }

    fn execute_request<T, F>(&mut self, action: F) -> Result<T>
    where
        F: FnOnce(&ureq::Agent) -> Result<T>,
    {
        let role = crate::orchestrator::current_http_thread_role();
        let admission_timeout_secs = match role {
            crate::orchestrator::HttpThreadRole::Interactive => TLS_ADMISSION_TIMEOUT_SECS,
            crate::orchestrator::HttpThreadRole::Io => TLS_ADMISSION_TIMEOUT_SECS,
            crate::orchestrator::HttpThreadRole::Background => TLS_ADMISSION_TIMEOUT_SECS / 2,
        };
        let _permit = crate::orchestrator::request_http_permit(
            self.priority,
            Duration::from_secs(admission_timeout_secs.max(1)),
        )?;
        action(&self.agent)
    }

    fn do_get(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        self.execute_request(|agent| {
            let req = apply_headers(agent.get(url), headers);
            let resp = req
                .call()
                .or_any_status()
                .map_err(|e| transport_to_error(e, "http_get_request"))?;
            let status = resp.status();
            let reader = resp.into_reader();
            let body = read_response_body_from_reader(reader)?;
            Ok((status, body))
        })
    }

    fn do_request_with_body(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.execute_request(|agent| {
            let req = apply_headers(agent.request(method, url), headers);
            let resp = req
                .send_bytes(body)
                .or_any_status()
                .map_err(|e| transport_to_error(e, "http_post_request"))?;
            let status = resp.status();
            let reader = resp.into_reader();
            let rb = read_response_body_from_reader(reader)?;
            Ok((status, rb))
        })
    }

    fn do_delete(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        self.execute_request(|agent| {
            let req = apply_headers(agent.delete(url), headers);
            let resp = req
                .call()
                .or_any_status()
                .map_err(|e| transport_to_error(e, "http_post_request"))?;
            let status = resp.status();
            let reader = resp.into_reader();
            let body = read_response_body_from_reader(reader)?;
            Ok((status, body))
        })
    }

    /// 替换为新建 Agent，用于重试前丢弃不可复用连接。
    pub fn replace_connection(&mut self) -> Result<()> {
        self.agent = build_agent(self.proxy_url_normalized.as_deref(), "http_client_replace")?;
        Ok(())
    }

    pub fn get(&mut self, url: &str) -> Result<(u16, ResponseBody)> {
        self.do_get(url, &[])
    }

    pub fn get_with_headers_inner(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, ResponseBody)> {
        self.do_get(url, headers)
    }

    pub fn post(&mut self, url: &str, body: &[u8]) -> Result<(u16, ResponseBody)> {
        let mut cl_buf = [0u8; 20];
        let content_length = crate::util::usize_to_decimal_buf(&mut cl_buf, body.len());
        let headers = [
            ("content-type", "application/json"),
            ("content-length", content_length),
        ];
        self.do_request_with_body("POST", url, &headers, body)
    }

    pub fn post_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.do_request_with_body("POST", url, headers, body)
    }

    pub fn patch_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.do_request_with_body("PATCH", url, headers, body)
    }

    pub fn do_post_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        let role = crate::orchestrator::current_http_thread_role();
        let admission_timeout_secs = match role {
            crate::orchestrator::HttpThreadRole::Interactive => TLS_ADMISSION_TIMEOUT_SECS,
            crate::orchestrator::HttpThreadRole::Io => TLS_ADMISSION_TIMEOUT_SECS,
            crate::orchestrator::HttpThreadRole::Background => TLS_ADMISSION_TIMEOUT_SECS / 2,
        };
        let _permit = crate::orchestrator::request_http_permit(
            self.priority,
            Duration::from_secs(admission_timeout_secs.max(1)),
        )?;
        let req = apply_headers(self.agent.post(url), headers);
        let resp = req
            .send_bytes(body)
            .or_any_status()
            .map_err(|e| transport_to_error(e, "http_post_request"))?;
        let status = resp.status();
        let mut reader = resp.into_reader();
        let max_len = max_response_bytes
            .unwrap_or_else(|| crate::orchestrator::current_budget().response_body_max);
        let enforce_limit = max_response_bytes.is_some();
        let mut total = 0usize;
        let mut buf = [0u8; RESPONSE_READ_CHUNK];
        loop {
            let n = reader.read(&mut buf).map_err(|e| Error::Other {
                source: Box::new(e),
                stage: "http_read",
            })?;
            if n == 0 {
                break;
            }
            total += n;
            if enforce_limit && total > max_len {
                log::warn!(
                    "[{}] streaming response truncated at {} bytes",
                    TAG,
                    max_len
                );
                drain_reader(&mut reader);
                break;
            }
            on_chunk(&buf[..n])?;
        }
        Ok(status)
    }
}

impl crate::platform::PlatformHttpClient for EspHttpClient {
    fn get(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        self.get_with_headers_inner(url, headers)
    }

    fn post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        if headers.is_empty() {
            let mut cl_buf = [0u8; 20];
            let content_length = crate::util::usize_to_decimal_buf(&mut cl_buf, body.len());
            let default_headers = [
                ("content-type", "application/json"),
                ("content-length", content_length),
            ];
            self.do_request_with_body("POST", url, &default_headers, body)
        } else {
            self.post_with_headers(url, headers, body)
        }
    }

    fn post_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        EspHttpClient::do_post_streaming(self, url, headers, body, max_response_bytes, on_chunk)
    }

    fn patch(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.patch_with_headers(url, headers, body)
    }

    fn put(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.do_request_with_body("PUT", url, headers, body)
    }

    fn delete(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        self.do_delete(url, headers)
    }

    fn reset_connection_for_retry(&mut self) {
        let _ = self.replace_connection();
    }
}
