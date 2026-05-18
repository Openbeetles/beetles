//! Shared Linux `tiny_http` serving skeleton for config and control planes.

use super::api_contract;
use super::common::{self, CORS_HEADERS};
use super::router::{IncomingBody, IncomingRequest, OutgoingBody, OutgoingResponse, RestartAction};
use crate::error::{Error, Result};
use std::io::Read as _;
use std::sync::Arc;

pub(crate) struct LinuxHttpServerSpec {
    pub log_tag: &'static str,
    pub listen_stage: &'static str,
    pub listen_log: String,
    pub listener: std::net::TcpListener,
    pub worker_name_prefix: &'static str,
    pub worker_count: usize,
}

pub(crate) fn default_linux_max_body_bytes(path: &str, method: &str) -> usize {
    let method = method.to_ascii_uppercase();
    if matches!(method.as_str(), "GET" | "OPTIONS" | "HEAD" | "DELETE") {
        return 0;
    }
    match path {
        "/api/capability_packages" => {
            crate::capability_package::MAX_CAPABILITY_PACKAGE_HTTP_BODY_LEN
        }
        _ => common::POST_BODY_MAX_LEN,
    }
}

pub(crate) fn run_linux_http_server<H, A>(
    spec: LinuxHttpServerSpec,
    handler: H,
    after_send: A,
) -> Result<()>
where
    H: Fn(IncomingRequest) -> OutgoingResponse + Send + Sync + 'static,
    A: Fn(&str, RestartAction) + Send + Sync + 'static,
{
    let server = Arc::new(
        tiny_http::Server::from_listener(spec.listener, None).map_err(|e| Error::Other {
            source: Box::new(std::io::Error::other(e.to_string())),
            stage: spec.listen_stage,
        })?,
    );
    log::info!("{}", spec.listen_log);

    let handler = Arc::new(handler);
    let after_send = Arc::new(after_send);
    let log_tag = spec.log_tag;
    for index in 0..spec.worker_count {
        let worker_name = format!("{}{}", spec.worker_name_prefix, index);
        let server = Arc::clone(&server);
        let handler = Arc::clone(&handler);
        let after_send = Arc::clone(&after_send);
        crate::util::spawn_guarded_with_profile(
            &worker_name,
            crate::util::STACK_CHANNEL_SENDER,
            Some(crate::util::SpawnCore::Core0),
            crate::util::HttpThreadRole::Background,
            move || loop {
                match server.recv() {
                    Ok(request) => handle_linux_request(log_tag, &handler, &after_send, request),
                    Err(error) => {
                        log::warn!("[{}] recv failed: {}", log_tag, error);
                        break;
                    }
                }
            },
        );
    }

    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn handle_linux_request<H, A>(
    log_tag: &'static str,
    handler: &Arc<H>,
    after_send: &Arc<A>,
    mut request: tiny_http::Request,
) where
    H: Fn(IncomingRequest) -> OutgoingResponse + Send + Sync + 'static,
    A: Fn(&str, RestartAction) + Send + Sync + 'static,
{
    let method = request.method().as_str().to_string();
    let uri = request.url().to_string();
    let path = uri.split('?').next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    for header in request.headers() {
        headers.push((header.field.to_string(), header.value.as_str().to_string()));
    }
    let max_body = default_linux_max_body_bytes(&path, &method);
    let mut body = Vec::new();
    if max_body > 0 {
        if let Err(error) = request
            .as_reader()
            .take(max_body as u64)
            .read_to_end(&mut body)
        {
            log::warn!("[{}] body read failed: {}", log_tag, error);
            respond(
                log_tag,
                request,
                OutgoingResponse::json(
                    500,
                    "Internal Server Error",
                    CORS_HEADERS,
                    format!(
                        r#"{{"error_key":"{}"}}"#,
                        api_contract::COMMON_BODY_READ_FAILED
                    )
                    .into_bytes(),
                ),
            );
            return;
        }
    }

    let response = handler(IncomingRequest {
        method,
        uri,
        headers,
        body: IncomingBody::from_vec(body),
    });
    let restart = response.restart;
    respond(log_tag, request, response);
    after_send(&path, restart);
}

fn respond(log_tag: &'static str, request: tiny_http::Request, outgoing: OutgoingResponse) {
    let headers = outgoing
        .headers
        .iter()
        .filter_map(|(key, value)| {
            tiny_http::Header::from_bytes(key.as_bytes(), value.as_bytes()).ok()
        })
        .collect::<Vec<_>>();
    match outgoing.body {
        OutgoingBody::Stream(stream) => {
            let response = tiny_http::Response::new(
                tiny_http::StatusCode(outgoing.status),
                headers,
                SseReceiverReader::new(stream),
                None,
                None,
            );
            if let Err(error) = request.respond(response) {
                log::warn!("[{}] respond failed: {}", log_tag, error);
            }
        }
        OutgoingBody::Bytes(bytes) => {
            let mut response = tiny_http::Response::from_data(bytes.into_vec())
                .with_status_code(tiny_http::StatusCode(outgoing.status));
            for header in headers {
                response.add_header(header);
            }
            if let Err(error) = request.respond(response) {
                log::warn!("[{}] respond failed: {}", log_tag, error);
            }
        }
    }
}

struct SseReceiverReader {
    stream: crate::chat_stream::ChatStreamReceiver,
    buffer: Vec<u8>,
    offset: usize,
}

impl SseReceiverReader {
    fn new(stream: crate::chat_stream::ChatStreamReceiver) -> Self {
        Self {
            stream,
            buffer: Vec::new(),
            offset: 0,
        }
    }
}

impl std::io::Read for SseReceiverReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        while self.offset >= self.buffer.len() {
            match self.stream.recv() {
                Some(chunk) => {
                    self.buffer = chunk;
                    self.offset = 0;
                }
                None => return Ok(0),
            }
        }
        let available = self.buffer.len().saturating_sub(self.offset);
        let n = available.min(out.len());
        out[..n].copy_from_slice(&self.buffer[self.offset..self.offset + n]);
        self.offset += n;
        Ok(n)
    }
}
