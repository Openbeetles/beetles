//! HTTP 配置 API 服务器：ESP 用 `esp-idf-svc` HTTPD；Linux/host 用 `tiny_http`，路由与 handler 与 ESP 共用。
//! Config HTTP API: ESP uses IDF HTTPD; Linux/host uses `tiny_http` with shared router/handlers.

pub(crate) mod router;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
mod esp_transport;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
mod lazy_executor;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
mod route_worker_control;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::error::Error;
use crate::error::Result;
use std::sync::Arc;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::time::Duration;

pub(crate) mod api_contract;
pub(crate) mod common;
pub(crate) mod handlers;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(crate) mod linux_runtime;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(crate) mod listen_preflight;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use listen_preflight::bind_tcp_listener;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const CONFIG_PLANE_POLL_MS: u64 = 500;
/// ESP-IDF HTTPD callback task stack.
///
/// Even immediate routes pass through the shared Rust route dispatch frame before
/// returning a small response. Heavier route bodies move to explicit route
/// workers, but the IDF response-write path still calls into lwIP / pthread TLS.
/// 2026-05-03 S3 28KB retest crashed in
/// `httpd_resp_send_chunk -> lwip_send -> pthread_getspecific`, so this callback
/// task must keep 32KB until the response-write path is redesigned.
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
const ESP_HTTPD_CALLBACK_STACK: usize = 32 * 1024;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
#[allow(clippy::too_many_arguments)]
pub fn run(
    platform: std::sync::Arc<dyn crate::platform::Platform>,
    tool_registry: Arc<crate::tools::ToolRegistry>,
    channel_capability_registry: Arc<crate::ChannelCapabilityRegistry>,
    capability_package_runtime_capabilities: Arc<crate::CapabilityPackageRuntimeCapabilities>,
    inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    system_inbound_tx: crate::bus::SystemInboundTx,
    skill_prompt_cache: Arc<crate::skills::SkillPromptCache>,
    inbound_tx: crate::bus::UserInboundTx,
    chat_streams: Arc<crate::chat_stream::ChatStreamBroker>,
    shared_config: Arc<std::sync::RwLock<crate::config::AppConfig>>,
) -> Result<()> {
    use crate::platform::http_server::common::MAX_OPEN_SOCKETS;
    use esp_idf_svc::http::server::{Configuration, EspHttpServer};
    let _active_guard = crate::runtime::ConfigPlaneGuard::enter();
    let server_config = Configuration {
        max_open_sockets: MAX_OPEN_SOCKETS,
        max_uri_handlers: 96,
        // The IDF callback task now only performs route admission, lightweight responses,
        // body reads, and handoff into bounded route workers.
        stack_size: ESP_HTTPD_CALLBACK_STACK,
        ..Default::default()
    };

    let mut server = EspHttpServer::new(&server_config).map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "http_server_new",
    })?;

    let ctx = Arc::new(handlers::build_runtime_handler_context(
        Arc::clone(&platform),
        Arc::clone(&tool_registry),
        Arc::clone(&channel_capability_registry),
        capability_package_runtime_capabilities,
        Arc::clone(&inbound_depth),
        Arc::clone(&outbound_depth),
        Arc::clone(&memory_store),
        Arc::clone(&session_store),
        Some(system_inbound_tx.clone()),
        Arc::clone(&chat_streams),
        Arc::clone(&skill_prompt_cache),
        Arc::clone(&shared_config),
        handlers::ControlPlaneRouteContract::FULL,
    ));

    let router_env = router::RouterEnv::new(inbound_tx.clone());
    let config_store = Arc::clone(&ctx.config_store);
    esp_transport::register_all_esp_routes(&mut server, &ctx, &router_env, &config_store)?;
    log::info!("[http_server] ESP config API serving (WiFi LAN + recovery plane)");

    loop {
        crate::platform::task_wdt::feed_current_task();
        std::thread::sleep(Duration::from_millis(CONFIG_PLANE_POLL_MS));
        crate::platform::task_wdt::feed_current_task();
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const LINUX_HTTP_WORKERS: usize = 4;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn linux_config_http_listen_addr() -> String {
    std::env::var("BEETLE_CONFIG_HTTP_LISTEN").unwrap_or_else(|_| "0.0.0.0:80".to_string())
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn bind_linux_config_http_listener() -> Result<(String, std::net::TcpListener)> {
    let listen = linux_config_http_listen_addr();
    let listener = bind_tcp_listener(&listen, "http_config_listen")?;
    Ok((listen, listener))
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[allow(clippy::too_many_arguments)]
pub fn run_with_bound_listener(
    listener: std::net::TcpListener,
    listen: String,
    platform: std::sync::Arc<dyn crate::platform::Platform>,
    tool_registry: Arc<crate::tools::ToolRegistry>,
    channel_capability_registry: Arc<crate::ChannelCapabilityRegistry>,
    capability_package_runtime_capabilities: Arc<crate::CapabilityPackageRuntimeCapabilities>,
    inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    system_inbound_tx: crate::bus::SystemInboundTx,
    skill_prompt_cache: Arc<crate::skills::SkillPromptCache>,
    inbound_tx: crate::bus::UserInboundTx,
    chat_streams: Arc<crate::chat_stream::ChatStreamBroker>,
    shared_config: Arc<std::sync::RwLock<crate::config::AppConfig>>,
) -> Result<()> {
    let ctx = Arc::new(handlers::build_runtime_handler_context(
        Arc::clone(&platform),
        tool_registry,
        channel_capability_registry,
        capability_package_runtime_capabilities,
        Arc::clone(&inbound_depth),
        Arc::clone(&outbound_depth),
        memory_store,
        session_store,
        Some(system_inbound_tx),
        Arc::clone(&chat_streams),
        skill_prompt_cache,
        shared_config,
        handlers::ControlPlaneRouteContract::FULL,
    ));
    let router_env = router::RouterEnv::new(inbound_tx.clone());
    let _active_guard = crate::runtime::ConfigPlaneGuard::enter();
    let dispatch_ctx = Arc::clone(&ctx);
    let dispatch_router_env = router_env.clone();
    let restart_platform = Arc::clone(&ctx.platform);
    linux_runtime::run_linux_http_server(
        linux_runtime::LinuxHttpServerSpec {
            log_tag: "http_config",
            listen_stage: "http_config_listen",
            listen_log: format!(
                "beetle HTTP config API listening on {} (override with BEETLE_CONFIG_HTTP_LISTEN)",
                listen
            ),
            listener,
            worker_name_prefix: "http_config_worker_",
            worker_count: LINUX_HTTP_WORKERS,
        },
        move |incoming| {
            router::dispatch(dispatch_ctx.as_ref(), &dispatch_router_env, incoming).unwrap_or_else(
                |error| {
                    let error_key = api_contract::error_key(&error);
                    let (status, status_text) = if error_key == api_contract::COMMON_INVALID_UTF8 {
                        (400, "Bad Request")
                    } else {
                        (500, "Internal Server Error")
                    };
                    log::warn!("[http_config] dispatch failed: {}", error);
                    router::OutgoingResponse::json(
                        status,
                        status_text,
                        common::CORS_HEADERS,
                        format!(r#"{{"error_key":"{}"}}"#, error_key).into_bytes(),
                    )
                },
            )
        },
        move |path, restart| {
            if restart != router::RestartAction::After300Ms {
                return;
            }
            let restart_reason = format!("http_restart{}", path);
            if !crate::runtime::schedule_restart_with_continuity_flush(
                Arc::clone(&restart_platform),
                restart_reason,
                std::time::Duration::from_millis(300),
            ) {
                log::error!(
                    "[http_config] delayed restart schedule failed path={}",
                    path
                );
            }
        },
    )
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[allow(clippy::too_many_arguments)]
pub fn run(
    platform: std::sync::Arc<dyn crate::platform::Platform>,
    tool_registry: Arc<crate::tools::ToolRegistry>,
    channel_capability_registry: Arc<crate::ChannelCapabilityRegistry>,
    capability_package_runtime_capabilities: Arc<crate::CapabilityPackageRuntimeCapabilities>,
    inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    system_inbound_tx: crate::bus::SystemInboundTx,
    skill_prompt_cache: Arc<crate::skills::SkillPromptCache>,
    inbound_tx: crate::bus::UserInboundTx,
    chat_streams: Arc<crate::chat_stream::ChatStreamBroker>,
    shared_config: Arc<std::sync::RwLock<crate::config::AppConfig>>,
) -> Result<()> {
    let (listen, listener) = bind_linux_config_http_listener()?;
    run_with_bound_listener(
        listener,
        listen,
        platform,
        tool_registry,
        channel_capability_registry,
        capability_package_runtime_capabilities,
        inbound_depth,
        outbound_depth,
        memory_store,
        session_store,
        system_inbound_tx,
        skill_prompt_cache,
        inbound_tx,
        chat_streams,
        shared_config,
    )
}

#[cfg(test)]
mod tests {
    use super::ESP_HTTPD_CALLBACK_STACK;

    #[test]
    fn esp_httpd_callback_stack_keeps_dispatch_headroom() {
        const {
            assert!(ESP_HTTPD_CALLBACK_STACK >= 32 * 1024);
        }
    }
}
