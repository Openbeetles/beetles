//! HTTP(S) 客户端：GET/POST、超时、响应体大小上限；ESP 不支持 proxy CONNECT。
//! HTTP(S) client: GET/POST, timeout, response size limit; ESP does not support proxy CONNECT.

use crate::config::{validate_proxy_url_for_target, AppConfig};
use crate::error::{Error, Result};
use crate::orchestrator::Priority;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::platform::heap::alloc_spiram_buffer;
use crate::platform::http_client::response_buffer::{
    choose_response_body_read_plan, ResponseBodyReadPlan,
};
use crate::platform::ResponseBody;
use embedded_svc::http::client::Client as HttpClient;
use embedded_svc::http::Method;
use embedded_svc::io::{Read, Write};
use esp_idf_svc::http::client::{Configuration as HttpConfig, EspHttpConnection};
use esp_idf_svc::io::EspIOError;
use std::time::{Duration, Instant};

const TAG: &str = "platform::http_client";
/// 单次请求超时（毫秒）。
const REQUEST_TIMEOUT_MS: i32 = 30_000;
/// 读响应体时的块大小；放栈上，不宜过大以免在 httpd 等小栈任务中溢出（如 GET /api/channel_connectivity 会多次 HTTP）。
const RESPONSE_READ_CHUNK: usize = 1024;
/// `ESP_ERR_HTTP_EAGAIN` = `ESP_ERR_HTTP_BASE + 7` (0x7007).
/// `esp-idf-svc` 在 HTTP body 未完成但暂时无数据时会返回它，语义是“稍后重读”。
const ESP_HTTP_EAGAIN_CODE: i32 = 0x7007;
/// 可恢复读的轮询等待间隔；短等待即可避免把瞬时 EAGAIN 放大成整请求失败。
const HTTP_READ_RETRY_SLEEP_MS: u64 = 20;

/// 喂任务看门狗；长时间 HTTP/LLM 请求前调用，避免 TWDT 复位。
/// 统一使用 `task_wdt::feed_current_task()`，不再维护重复实现。
#[inline]
fn feed_task_watchdog() {
    crate::platform::task_wdt::feed_current_task();
}

#[inline]
fn is_http_eagain(err: &(dyn std::error::Error + 'static)) -> bool {
    err.downcast_ref::<EspIOError>()
        .map(|e| e.0.code().abs() == ESP_HTTP_EAGAIN_CODE)
        .unwrap_or(false)
}

fn map_http_read_error(err: impl std::error::Error + 'static, stage: &'static str) -> Error {
    Error::Other {
        source: Box::new(std::io::Error::other(format!("{:?}", err))),
        stage,
    }
}

fn read_with_retry<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize>
where
    R::Error: std::error::Error + 'static,
{
    let deadline = Instant::now() + Duration::from_millis(REQUEST_TIMEOUT_MS as u64);
    loop {
        feed_task_watchdog();
        match r.read(buf) {
            Ok(n) => return Ok(n),
            Err(e) if is_http_eagain(&e) => {
                if Instant::now() >= deadline {
                    return Err(Error::io(
                        "http_read",
                        std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "http_read retry exceeded timeout waiting for response body",
                        ),
                    ));
                }
                std::thread::sleep(Duration::from_millis(HTTP_READ_RETRY_SLEEP_MS));
            }
            Err(e) => return Err(map_http_read_error(e, "http_read")),
        }
    }
}

/// ESP 上单次 TLS 准入等待最长时间（与请求超时同量级，避免长时间占锁）。
const TLS_ADMISSION_TIMEOUT_SECS: u64 = 30;

/// 封装 ESP HTTP 请求参数；连接对象按请求临时创建并在请求结束后立即释放，
/// 避免 agent / sender / stream editor 这类长生命周期线程长期占住 internal heap。
pub struct EspHttpClient {
    /// 若设置，请求应经 CONNECT 隧道；当前未实现则 get/post 返回错误。
    proxy_host: Option<String>,
    /// HTTP 请求优先级，用于 orchestrator 准入控制。
    priority: Priority,
}

impl EspHttpClient {
    /// 新建 HTTPS 客户端（无 proxy）；直连。默认 Normal 优先级。
    pub fn new() -> Result<Self> {
        Self::new_optional_proxy(None, Priority::Normal)
    }

    /// 新建客户端，指定优先级。
    pub fn new_with_priority(priority: Priority) -> Result<Self> {
        Self::new_optional_proxy(None, priority)
    }

    /// 新建客户端；ESP 不支持 proxy CONNECT，非空 `proxy_url` 会直接返回配置错误。
    pub fn new_with_config(config: &AppConfig) -> Result<Self> {
        Self::new_with_config_and_priority(config, Priority::Normal)
    }

    /// 新建客户端并显式指定优先级。
    pub fn new_with_config_and_priority(config: &AppConfig, priority: Priority) -> Result<Self> {
        validate_proxy_url_for_target(config.proxy_url.trim(), false)?;
        Self::new_optional_proxy(None, priority)
    }

    fn default_http_config() -> HttpConfig {
        HttpConfig {
            crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
            timeout: Some(std::time::Duration::from_millis(REQUEST_TIMEOUT_MS as u64)),
            ..Default::default()
        }
    }

    fn new_optional_proxy(proxy: Option<(String, String)>, priority: Priority) -> Result<Self> {
        if proxy.is_some() {
            return Err(Error::config(
                "proxy_connect",
                "proxy_url is not supported on ESP; leave proxy_url empty",
            ));
        }
        let proxy_host = proxy.map(|(host, _port)| host);
        Ok(EspHttpClient {
            proxy_host,
            priority,
        })
    }

    fn check_proxy_and_watchdog(&self) -> Result<()> {
        if self.proxy_host.is_some() {
            return Err(Error::Other {
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "proxy CONNECT tunnel not implemented",
                )),
                stage: "proxy_connect",
            });
        }
        feed_task_watchdog();
        Ok(())
    }

    fn open_connection() -> Result<EspHttpConnection> {
        let config = Self::default_http_config();
        EspHttpConnection::new(&config).map_err(|e| Error::Other {
            source: Box::new(e),
            stage: "http_client_new",
        })
    }

    fn execute_request<T, F>(&mut self, action: F) -> Result<T>
    where
        F: FnOnce(&mut EspHttpConnection) -> Result<T>,
    {
        let role = crate::orchestrator::current_http_thread_role();
        let admission_timeout_secs = match role {
            crate::orchestrator::HttpThreadRole::Interactive => TLS_ADMISSION_TIMEOUT_SECS,
            crate::orchestrator::HttpThreadRole::Io => TLS_ADMISSION_TIMEOUT_SECS,
            crate::orchestrator::HttpThreadRole::Background => TLS_ADMISSION_TIMEOUT_SECS / 2,
        };
        let _permit = crate::orchestrator::request_http_permit(
            self.priority,
            std::time::Duration::from_secs(admission_timeout_secs.max(1)),
        )?;
        let mut conn = Self::open_connection()?;
        action(&mut conn)
    }

    /// 请求级连接模型下，无需保留旧连接；保留此入口给统一 trait 调用方使用。
    pub fn replace_connection(&mut self) -> Result<()> {
        Ok(())
    }

    fn do_get(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        self.execute_request(|conn| {
            let mut client = HttpClient::wrap(conn);
            let request = client
                .request(Method::Get, url, headers)
                .map_err(|e| Error::Other {
                    source: Box::new(e),
                    stage: "http_get_request",
                })?;
            let mut response = request.submit().map_err(|e| Error::Other {
                source: Box::new(e),
                stage: "http_get_submit",
            })?;
            let status = response.status();
            let content_length_hint = response
                .header("Content-Length")
                .and_then(|v| v.trim().parse::<usize>().ok());
            match read_response_body(&mut response, content_length_hint) {
                Ok(body) => Ok((status, body)),
                Err(e) => {
                    drain_response(&mut response);
                    Err(e)
                }
            }
        })
    }

    fn do_request_with_body(
        &mut self,
        method: Method,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.execute_request(|conn| {
            let mut client = HttpClient::wrap(conn);
            let mut request = client
                .request(method, url, headers)
                .map_err(|e| Error::Other {
                    source: Box::new(e),
                    stage: "http_post_request",
                })?;
            request.write_all(body).map_err(|e| Error::Other {
                source: Box::new(std::io::Error::other(format!("{:?}", e))),
                stage: "http_post_write",
            })?;
            request.flush().map_err(|e| Error::Other {
                source: Box::new(std::io::Error::other(format!("{:?}", e))),
                stage: "http_post_flush",
            })?;
            let mut response = request.submit().map_err(|e| Error::Other {
                source: Box::new(e),
                stage: "http_post_submit",
            })?;
            let status = response.status();
            let content_length_hint = response
                .header("Content-Length")
                .and_then(|v| v.trim().parse::<usize>().ok());
            match read_response_body(&mut response, content_length_hint) {
                Ok(resp_body) => Ok((status, resp_body)),
                Err(e) => {
                    drain_response(&mut response);
                    Err(e)
                }
            }
        })
    }

    /// GET 请求；返回 (status_code, body)，body 不超过当前 resource budget 的 response_body_max。若已配置 proxy 且 CONNECT 未实现则返回错误。
    pub fn get(&mut self, url: &str) -> Result<(u16, ResponseBody)> {
        self.check_proxy_and_watchdog()?;
        self.do_get(url, &[])
    }

    /// GET 请求，自定义 headers；供 ToolContext 使用（如 Brave API key）。内部实现，避免与 trait 重名。
    pub fn get_with_headers_inner(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, ResponseBody)> {
        self.check_proxy_and_watchdog()?;
        self.do_get(url, headers)
    }

    /// POST 请求；body 为请求体；返回 (status_code, response_body)。若已配置 proxy 且 CONNECT 未实现则返回错误。
    pub fn post(&mut self, url: &str, body: &[u8]) -> Result<(u16, ResponseBody)> {
        self.check_proxy_and_watchdog()?;
        let mut cl_buf = [0u8; 20];
        let content_length = crate::util::usize_to_decimal_buf(&mut cl_buf, body.len());
        let headers = [
            ("content-type", "application/json"),
            ("content-length", content_length),
        ];
        self.do_request_with_body(Method::Post, url, &headers, body)
    }

    /// POST 请求，自定义 headers（须含 content-type、content-length）；供 LlmHttpClient 使用。
    pub fn post_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.check_proxy_and_watchdog()?;
        self.do_request_with_body(Method::Post, url, headers, body)
    }

    /// PATCH 请求，自定义 headers；供飞书编辑消息等使用。
    pub fn patch_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.check_proxy_and_watchdog()?;
        self.do_request_with_body(Method::Patch, url, headers, body)
    }

    /// 流式 POST：发送请求后循环 read + 回调 on_chunk，不将完整响应体读入内存。
    /// 每次 read 前喂看门狗；`max_response_bytes` 为 None 时无限制（适用于边到达边消费的场景如 TTS）。
    pub fn do_post_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        self.check_proxy_and_watchdog()?;
        self.execute_request(|conn| {
            let mut client = HttpClient::wrap(conn);
            let mut request = client.post(url, headers).map_err(|e| Error::Other {
                source: Box::new(e),
                stage: "http_post_request",
            })?;
            request.write_all(body).map_err(|e| Error::Other {
                source: Box::new(std::io::Error::other(format!("{:?}", e))),
                stage: "http_post_write",
            })?;
            request.flush().map_err(|e| Error::Other {
                source: Box::new(std::io::Error::other(format!("{:?}", e))),
                stage: "http_post_flush",
            })?;
            let mut response = request.submit().map_err(|e| Error::Other {
                source: Box::new(e),
                stage: "http_post_submit",
            })?;
            let status = response.status();

            let max_len = max_response_bytes
                .unwrap_or_else(|| crate::orchestrator::current_budget().response_body_max);
            let enforce_limit = max_response_bytes.is_some();
            let mut total = 0usize;
            let mut buf = [0u8; RESPONSE_READ_CHUNK];
            loop {
                let n = read_with_retry(&mut response, &mut buf)?;
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
                    drain_response(&mut response);
                    break;
                }
                on_chunk(&buf[..n])?;
            }

            Ok(status)
        })
    }
}

/// 首次分配块大小，避免无 PSRAM 时单次分配过大；后续按 read 循环 grow 至 budget.response_body_max。
const INITIAL_RESPONSE_BODY_CAP: usize = 8 * 1024;
/// 仅在已知响应长度且达到该阈值时，才预分配 PSRAM 响应体缓冲，避免几十字节 JSON 也吃整块大 buffer。
const PSRAM_RESPONSE_PREALLOC_THRESHOLD: usize = 8 * 1024;

/// 最多 drain 的字节数，防止无限读取恶意超长响应。
const MAX_DRAIN_BYTES: usize = 512 * 1024;

/// 将响应体读空（最多 MAX_DRAIN_BYTES），便于当前请求在收尾阶段尽快释放底层连接资源。
fn drain_response<R: Read>(r: &mut R)
where
    R::Error: std::error::Error + 'static,
{
    let mut buf = [0u8; 512];
    let mut total = 0usize;
    loop {
        match read_with_retry(r, &mut buf) {
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

/// S3 上优先从 PSRAM 分配整块读入，返回 ResponseBody（Drop 时释放 PSRAM），无堆拷贝；否则用 Vec 按块增长。
/// 最大长度由 orchestrator::current_budget().response_body_max 决定，压力高时自动缩减。
fn read_response_body<R: Read>(
    r: &mut R,
    content_length_hint: Option<usize>,
) -> Result<ResponseBody>
where
    R::Error: std::error::Error + 'static,
{
    let max_len = crate::orchestrator::current_budget().response_body_max;
    let plan = choose_response_body_read_plan(
        max_len,
        content_length_hint,
        cfg!(any(target_arch = "xtensa", target_arch = "riscv32")),
        INITIAL_RESPONSE_BODY_CAP,
        PSRAM_RESPONSE_PREALLOC_THRESHOLD,
    );
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    match plan {
        ResponseBodyReadPlan::PsramExact { cap } => {
            if let Some(psram_ptr) = alloc_spiram_buffer(cap) {
                return read_response_body_into_psram(psram_ptr, cap, r);
            }
        }
        ResponseBodyReadPlan::GrowThenPsram {
            initial_cap,
            psram_initial_cap,
            psram_max_cap,
            switch_len,
        } => {
            return read_response_body_grow_then_psram(
                initial_cap,
                psram_initial_cap,
                psram_max_cap,
                switch_len,
                max_len,
                r,
            );
        }
        ResponseBodyReadPlan::Heap { .. } => {}
    }

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let _ = plan;
    let initial_cap = match plan {
        ResponseBodyReadPlan::Heap { initial_cap } => initial_cap,
        ResponseBodyReadPlan::PsramExact { cap } => cap.min(INITIAL_RESPONSE_BODY_CAP),
        ResponseBodyReadPlan::GrowThenPsram { initial_cap, .. } => initial_cap,
    };
    read_response_body_into_heap_like(Vec::with_capacity(initial_cap), max_len, r)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn read_response_body_grow_then_psram<R: Read>(
    initial_cap: usize,
    psram_initial_cap: usize,
    psram_max_cap: usize,
    switch_len: usize,
    max_len: usize,
    r: &mut R,
) -> Result<ResponseBody>
where
    R::Error: std::error::Error + 'static,
{
    let mut heap = Vec::with_capacity(initial_cap.min(max_len));
    let mut psram: Option<(*mut u8, usize, usize)> = None;
    let mut buf = [0u8; RESPONSE_READ_CHUNK];
    loop {
        let n = match read_with_retry(r, &mut buf) {
            Ok(n) => n,
            Err(e) => {
                if let Some((ptr, _, _)) = psram.take() {
                    unsafe {
                        crate::platform::heap::free_spiram_buffer(ptr);
                    }
                }
                return Err(e);
            }
        };
        if n == 0 {
            break;
        }
        let current_len = psram.as_ref().map(|(_, len, _)| *len).unwrap_or(heap.len());
        let remain = max_len.saturating_sub(current_len);
        if remain == 0 {
            log::warn!("[{}] response body truncated at {} bytes", TAG, max_len);
            drain_response(r);
            break;
        }
        let take = n.min(remain);
        if let Some((ptr, len, cap)) = psram.as_mut() {
            let required = len.saturating_add(take).min(max_len);
            if required > *cap {
                let new_cap = next_psram_response_cap(required, *cap, psram_max_cap.min(max_len));
                if let Some(new_ptr) = alloc_spiram_buffer(new_cap) {
                    if *len > 0 {
                        unsafe {
                            std::ptr::copy_nonoverlapping(*ptr, new_ptr, *len);
                        }
                    }
                    unsafe {
                        crate::platform::heap::free_spiram_buffer(*ptr);
                    }
                    *ptr = new_ptr;
                    *cap = new_cap;
                } else {
                    let available = cap.saturating_sub(*len);
                    let to_copy = take.min(available);
                    if to_copy > 0 {
                        unsafe {
                            std::ptr::copy_nonoverlapping(buf.as_ptr(), ptr.add(*len), to_copy);
                        }
                        *len += to_copy;
                    }
                    log::warn!("[{}] response body truncated at {} bytes", TAG, *len);
                    drain_response(r);
                    break;
                }
            }
            let available = cap.saturating_sub(*len);
            let to_copy = take.min(available);
            if to_copy > 0 {
                unsafe {
                    std::ptr::copy_nonoverlapping(buf.as_ptr(), ptr.add(*len), to_copy);
                }
                *len += to_copy;
            }
            if to_copy < take {
                log::warn!("[{}] response body truncated at {} bytes", TAG, *cap);
                drain_response(r);
                break;
            }
        } else if heap.len().saturating_add(take) <= switch_len {
            heap.extend_from_slice(&buf[..take]);
        } else {
            let cap = next_psram_response_cap(
                heap.len().saturating_add(take),
                psram_initial_cap,
                psram_max_cap.min(max_len),
            );
            if let Some(ptr) = alloc_spiram_buffer(cap) {
                let mut len = heap.len();
                if len > 0 {
                    unsafe {
                        std::ptr::copy_nonoverlapping(heap.as_ptr(), ptr, len);
                    }
                }
                let available = cap.saturating_sub(len);
                let to_copy = take.min(available);
                if to_copy > 0 {
                    unsafe {
                        std::ptr::copy_nonoverlapping(buf.as_ptr(), ptr.add(len), to_copy);
                    }
                    len += to_copy;
                }
                heap.clear();
                psram = Some((ptr, len, cap));
                if to_copy < take {
                    log::warn!("[{}] response body truncated at {} bytes", TAG, cap);
                    drain_response(r);
                    break;
                }
            } else {
                heap.extend_from_slice(&buf[..take]);
            }
        }
        if take < n {
            drain_response(r);
            break;
        }
    }
    if let Some((ptr, len, _)) = psram {
        Ok(ResponseBody::PSRAM {
            ptr: Some(ptr),
            len,
        })
    } else {
        Ok(ResponseBody::Heap(heap))
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn next_psram_response_cap(required: usize, current_cap: usize, max_cap: usize) -> usize {
    let mut cap = current_cap.max(RESPONSE_READ_CHUNK).min(max_cap);
    while cap < required && cap < max_cap {
        let next = cap.saturating_mul(2).min(max_cap);
        if next <= cap {
            break;
        }
        cap = next;
    }
    cap.max(required).min(max_cap)
}

fn read_response_body_into_heap_like<R: Read>(
    mut out: Vec<u8>,
    max_len: usize,
    r: &mut R,
) -> Result<ResponseBody>
where
    R::Error: std::error::Error + 'static,
{
    let mut buf = [0u8; RESPONSE_READ_CHUNK];
    loop {
        let n = read_with_retry(r, &mut buf)?;
        if n == 0 {
            break;
        }
        let remain = max_len.saturating_sub(out.len());
        if remain == 0 {
            log::warn!("[{}] response body truncated at {} bytes", TAG, max_len);
            drain_response(r);
            break;
        }
        let take = n.min(remain);
        out.extend_from_slice(&buf[..take]);
        if take < n {
            drain_response(r);
            break;
        }
    }
    Ok(ResponseBody::Heap(out))
}

/// 将响应体读入 PSRAM 块，返回 ResponseBody（Drop 时 free），不 to_vec。仅嵌入式 ESP。
/// 读取失败时释放 PSRAM 缓冲区，防止泄漏。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn read_response_body_into_psram<R: Read>(
    ptr: *mut u8,
    buf_cap: usize,
    r: &mut R,
) -> Result<ResponseBody>
where
    R::Error: std::error::Error + 'static,
{
    let mut len = 0usize;
    let mut buf = [0u8; RESPONSE_READ_CHUNK];
    loop {
        let n = match read_with_retry(r, &mut buf) {
            Ok(n) => n,
            Err(e) => {
                unsafe {
                    crate::platform::heap::free_spiram_buffer(ptr);
                }
                return Err(e);
            }
        };
        if n == 0 {
            break;
        }
        let remain = buf_cap.saturating_sub(len);
        if remain == 0 {
            log::warn!("[{}] response body truncated at {} bytes", TAG, buf_cap);
            drain_response(r);
            break;
        }
        let take = n.min(remain);
        unsafe {
            std::ptr::copy_nonoverlapping(buf.as_ptr(), ptr.add(len), take);
        }
        len += take;
        if take < n {
            drain_response(r);
            break;
        }
    }
    Ok(ResponseBody::PSRAM {
        ptr: Some(ptr),
        len,
    })
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
            // 调用方未传 headers 时补上默认 JSON headers（content-type + content-length），
            // 与 EspHttpClient::post() 行为一致。
            let mut cl_buf = [0u8; 20];
            let content_length = crate::util::usize_to_decimal_buf(&mut cl_buf, body.len());
            let default_headers = [
                ("content-type", "application/json"),
                ("content-length", content_length),
            ];
            self.do_request_with_body(Method::Post, url, &default_headers, body)
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
        self.check_proxy_and_watchdog()?;
        self.do_request_with_body(Method::Put, url, headers, body)
    }
    fn delete(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        self.check_proxy_and_watchdog()?;
        self.do_request_with_body(Method::Delete, url, headers, &[])
    }
    fn reset_connection_for_retry(&mut self) {
        let _ = self.replace_connection();
    }
}
