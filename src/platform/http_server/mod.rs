//! HTTP 配置 API 服务器：ESP 用 `esp-idf-svc` HTTPD；Linux/host 用 `tiny_http`，路由与 handler 与 ESP 共用。
//! Config HTTP API: ESP uses IDF HTTPD; Linux/host uses `tiny_http` with shared router/handlers.

mod router;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
mod esp_transport;

use crate::error::{Error, Result};
use std::sync::Arc;

pub(crate) mod common;
mod handlers;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
#[allow(clippy::too_many_arguments)]
pub fn run(
    platform: std::sync::Arc<dyn crate::platform::Platform>,
    tool_registry: Arc<crate::tools::ToolRegistry>,
    inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    inbound_tx: crate::bus::InboundTx,
    shared_config: Arc<std::sync::RwLock<crate::config::AppConfig>>,
) -> Result<()> {
    let config_store = platform.config_store();
    let config_file_store: std::sync::Arc<dyn crate::config::ConfigFileStore + Send + Sync> =
        std::sync::Arc::new(crate::config::PlatformConfigFileStore(
            std::sync::Arc::clone(&platform),
        ));
    let skill_storage = platform.skill_storage();
    let skill_meta_store = platform.skill_meta_store();
    use crate::platform::http_server::common::MAX_OPEN_SOCKETS;
    use esp_idf_svc::http::server::{Configuration, EspHttpServer};

    let server_config = Configuration {
        max_open_sockets: MAX_OPEN_SOCKETS,
        max_uri_handlers: 96,
        stack_size: 12 * 1024,
        ..Default::default()
    };

    let mut server = EspHttpServer::new(&server_config).map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "http_server_new",
    })?;

    let ctx = Arc::new(handlers::HandlerContext {
        config_store: Arc::clone(&config_store),
        config_file_store: Arc::clone(&config_file_store),
        platform: Arc::clone(&platform),
        memory_store: Arc::clone(&memory_store),
        session_store: Arc::clone(&session_store),
        skill_storage: Arc::clone(&skill_storage),
        skill_meta_store: Arc::clone(&skill_meta_store),
        tool_registry,
        inbound_depth: Arc::clone(&inbound_depth),
        outbound_depth: Arc::clone(&outbound_depth),
        version: Arc::from(env!("CARGO_PKG_VERSION")),
        board_id: Arc::from(crate::platform::runtime_board::resolved_board_id()),
        cached_config: shared_config,
    });

    let router_env = router::RouterEnv::new(inbound_tx.clone());
    esp_transport::register_all_esp_routes(&mut server, &ctx, &router_env, &config_store)?;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn linux_max_body_bytes(path: &str, method: &str) -> usize {
    let m = method.to_ascii_uppercase();
    if matches!(m.as_str(), "GET" | "OPTIONS" | "HEAD" | "DELETE") {
        return 0;
    }
    match path {
        "/api/soul" | "/api/user" => crate::memory::MAX_SOUL_USER_LEN,
        "/api/feishu/event" => 64 * 1024,
        "/api/webhook/qq" => crate::channels::QQ_WEBHOOK_BODY_MAX,
        _ => common::POST_BODY_MAX_LEN,
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const LINUX_HTTP_WORKERS: usize = 4;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn handle_linux_request(
    ctx: &Arc<handlers::HandlerContext>,
    router_env: &router::RouterEnv,
    mut request: tiny_http::Request,
) {
    use std::io::Read as _;

    let method = request.method().as_str().to_string();
    let uri = request.url().to_string();
    let path = uri.split('?').next().unwrap_or("/").to_string();
    let mut hdrs = Vec::new();
    for h in request.headers() {
        hdrs.push((h.field.to_string(), h.value.as_str().to_string()));
    }
    let max_body = linux_max_body_bytes(&path, &method);
    let mut body = Vec::new();
    if max_body > 0 {
        if let Err(e) = request
            .as_reader()
            .take(max_body as u64)
            .read_to_end(&mut body)
        {
            log::warn!("http_config_body_read: {}", e);
            let mut resp =
                tiny_http::Response::from_data(br#"{"error":"internal error"}"#.to_vec())
                    .with_status_code(tiny_http::StatusCode(500));
            for (k, v) in common::CORS_HEADERS {
                if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
                    resp.add_header(h);
                }
            }
            if let Err(e) = request.respond(resp) {
                log::warn!("http_config_respond: {}", e);
            }
            return;
        }
    }
    let incoming = router::IncomingRequest {
        method,
        uri,
        headers: hdrs,
        body,
    };
    let out = router::dispatch(ctx.as_ref(), router_env, incoming).unwrap_or_else(|e| {
        log::warn!("http_config_dispatch: {}", e);
        router::OutgoingResponse::json(
            500,
            "Internal Server Error",
            common::CORS_HEADERS,
            br#"{"error":"internal error"}"#.to_vec(),
        )
    });
    let mut resp = tiny_http::Response::from_data(out.body)
        .with_status_code(tiny_http::StatusCode(out.status));
    for (k, v) in out.headers {
        if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            resp.add_header(h);
        }
    }
    if let Err(e) = request.respond(resp) {
        log::warn!("http_config_respond: {}", e);
    }
    if out.restart == router::RestartAction::After300Ms {
        let platform = Arc::clone(&ctx.platform);
        let restart_reason = format!("http_restart{}", path);
        crate::util::spawn_guarded_with_profile(
            "restart_defer",
            4096,
            Some(crate::util::SpawnCore::Core0),
            crate::util::HttpThreadRole::Background,
            move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                crate::runtime::request_restart_with_continuity_flush(
                    platform,
                    None,
                    restart_reason.as_str(),
                );
            },
        );
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[allow(clippy::too_many_arguments)]
pub fn run(
    platform: std::sync::Arc<dyn crate::platform::Platform>,
    tool_registry: Arc<crate::tools::ToolRegistry>,
    inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    inbound_tx: crate::bus::InboundTx,
    msg_id_cache: crate::channels::QqMsgIdCache,
    qq_webhook_enabled: bool,
    qq_app_id: String,
    qq_secret: String,
    shared_config: Arc<std::sync::RwLock<crate::config::AppConfig>>,
) -> Result<()> {
    use std::time::Duration;

    let config_store = platform.config_store();
    let config_file_store: std::sync::Arc<dyn crate::config::ConfigFileStore + Send + Sync> =
        std::sync::Arc::new(crate::config::PlatformConfigFileStore(
            std::sync::Arc::clone(&platform),
        ));
    let skill_storage = platform.skill_storage();
    let skill_meta_store = platform.skill_meta_store();
    let ctx = Arc::new(handlers::HandlerContext {
        config_store: Arc::clone(&config_store),
        config_file_store: Arc::clone(&config_file_store),
        platform: Arc::clone(&platform),
        memory_store: Arc::clone(&memory_store),
        session_store: Arc::clone(&session_store),
        skill_storage: Arc::clone(&skill_storage),
        skill_meta_store: Arc::clone(&skill_meta_store),
        tool_registry,
        inbound_depth: Arc::clone(&inbound_depth),
        outbound_depth: Arc::clone(&outbound_depth),
        version: Arc::from(env!("CARGO_PKG_VERSION")),
        board_id: Arc::from(crate::platform::runtime_board::resolved_board_id()),
        cached_config: shared_config,
    });
    let router_env = router::RouterEnv::new(
        inbound_tx.clone(),
        msg_id_cache.clone(),
        qq_webhook_enabled,
        qq_app_id,
        qq_secret,
    );
    let listen =
        std::env::var("BEETLE_CONFIG_HTTP_LISTEN").unwrap_or_else(|_| "0.0.0.0:80".to_string());
    let server = Arc::new(tiny_http::Server::http(&listen).map_err(|e| Error::Other {
        source: Box::new(std::io::Error::other(e.to_string())),
        stage: "http_config_listen",
    })?);
    log::info!(
        "beetle HTTP config API listening on {} (override with BEETLE_CONFIG_HTTP_LISTEN)",
        listen
    );
    for i in 0..LINUX_HTTP_WORKERS {
        let worker_name = format!("http_config_worker_{}", i);
        let server = Arc::clone(&server);
        let ctx = Arc::clone(&ctx);
        let router_env = router_env.clone();
        crate::util::spawn_guarded(&worker_name, move || loop {
            match server.recv() {
                Ok(request) => handle_linux_request(&ctx, &router_env, request),
                Err(e) => {
                    log::warn!("http_config_recv: {}", e);
                    break;
                }
            }
        });
    }
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}
