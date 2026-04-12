//! HTTP 配置 API 服务器：ESP 用 `esp-idf-svc` HTTPD；Linux/host 用 `tiny_http`，路由与 handler 与 ESP 共用。
//! Config HTTP API: ESP uses IDF HTTPD; Linux/host uses `tiny_http` with shared router/handlers.

pub(crate) mod router;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
mod esp_transport;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
mod lazy_executor;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::error::Error;
use crate::error::Result;
use std::sync::Arc;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::time::Duration;

pub(crate) mod common;
pub(crate) mod handlers;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(crate) mod linux_runtime;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const CONFIG_PLANE_POLL_MS: u64 = 500;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn esp_config_plane_desired() -> bool {
    // User-facing recovery/config access must remain reachable after STA joins.
    // On ESP this server is therefore a budgeted steady-state capability, not a
    // bootstrap-only plane that disappears after initial provisioning.
    true
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
#[allow(clippy::too_many_arguments)]
pub fn run(
    platform: std::sync::Arc<dyn crate::platform::Platform>,
    tool_registry: Arc<crate::tools::ToolRegistry>,
    channel_capability_registry: Arc<crate::ChannelCapabilityRegistry>,
    inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    skill_prompt_cache: Arc<crate::skills::SkillPromptCache>,
    inbound_tx: crate::bus::InboundTx,
    shared_config: Arc<std::sync::RwLock<crate::config::AppConfig>>,
    llm_stream_enabled: bool,
) -> Result<()> {
    use crate::platform::http_server::common::MAX_OPEN_SOCKETS;
    use esp_idf_svc::http::server::{Configuration, EspHttpServer};
    loop {
        while !esp_config_plane_desired() {
            crate::state::set_config_plane_active(false);
            crate::platform::task_wdt::feed_current_task();
            std::thread::sleep(Duration::from_millis(CONFIG_PLANE_POLL_MS));
            crate::platform::task_wdt::feed_current_task();
        }

        let _active_guard = crate::runtime::ConfigPlaneGuard::enter();
        let server_config = Configuration {
            max_open_sockets: MAX_OPEN_SOCKETS,
            max_uri_handlers: 96,
            // Direct-dispatch config/status routes still execute on the IDF callback task.
            // Keep the HTTPD stack at the pre-regression budget until the control-plane split
            // is revalidated on hardware.
            stack_size: 16 * 1024,
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
            Arc::clone(&inbound_depth),
            Arc::clone(&outbound_depth),
            Arc::clone(&memory_store),
            Arc::clone(&session_store),
            Arc::clone(&skill_prompt_cache),
            Arc::clone(&shared_config),
            llm_stream_enabled,
            handlers::ControlPlaneRouteContract::FULL,
        ));

        let router_env = router::RouterEnv::new(inbound_tx.clone());
        let config_store = Arc::clone(&ctx.config_store);
        esp_transport::register_all_esp_routes(&mut server, &ctx, &router_env, &config_store)?;
        log::info!("[http_server] ESP config API serving (WiFi LAN + recovery plane)");

        while esp_config_plane_desired() {
            crate::platform::task_wdt::feed_current_task();
            std::thread::sleep(Duration::from_millis(CONFIG_PLANE_POLL_MS));
            crate::platform::task_wdt::feed_current_task();
        }
        log::info!("[http_server] ESP config API suspended (STA steady-state)");
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const LINUX_HTTP_WORKERS: usize = 4;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[allow(clippy::too_many_arguments)]
pub fn run(
    platform: std::sync::Arc<dyn crate::platform::Platform>,
    tool_registry: Arc<crate::tools::ToolRegistry>,
    channel_capability_registry: Arc<crate::ChannelCapabilityRegistry>,
    inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    skill_prompt_cache: Arc<crate::skills::SkillPromptCache>,
    inbound_tx: crate::bus::InboundTx,
    msg_id_cache: crate::channels::QqMsgIdCache,
    qq_webhook_enabled: bool,
    qq_app_id: String,
    qq_secret: String,
    shared_config: Arc<std::sync::RwLock<crate::config::AppConfig>>,
    llm_stream_enabled: bool,
) -> Result<()> {
    let ctx = Arc::new(handlers::build_runtime_handler_context(
        Arc::clone(&platform),
        tool_registry,
        channel_capability_registry,
        Arc::clone(&inbound_depth),
        Arc::clone(&outbound_depth),
        memory_store,
        session_store,
        skill_prompt_cache,
        shared_config,
        llm_stream_enabled,
        handlers::ControlPlaneRouteContract::FULL,
    ));
    let router_env = router::RouterEnv::new(
        inbound_tx.clone(),
        msg_id_cache.clone(),
        qq_webhook_enabled,
        qq_app_id,
        qq_secret,
    );
    let _active_guard = crate::runtime::ConfigPlaneGuard::enter();
    let listen =
        std::env::var("BEETLE_CONFIG_HTTP_LISTEN").unwrap_or_else(|_| "0.0.0.0:80".to_string());
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
            listen_addr: listen,
            worker_name_prefix: "http_config_worker_",
            worker_count: LINUX_HTTP_WORKERS,
        },
        move |incoming| {
            router::dispatch(dispatch_ctx.as_ref(), &dispatch_router_env, incoming).unwrap_or_else(
                |error| {
                    log::warn!("[http_config] dispatch failed: {}", error);
                    router::OutgoingResponse::json(
                        500,
                        "Internal Server Error",
                        common::CORS_HEADERS,
                        br#"{"error":"internal error"}"#.to_vec(),
                    )
                },
            )
        },
        move |path, restart| {
            if restart != router::RestartAction::After300Ms {
                return;
            }
            let platform = Arc::clone(&restart_platform);
            let restart_reason = format!("http_restart{}", path);
            crate::util::spawn_guarded_with_profile(
                "restart_defer",
                crate::util::STACK_RESTART_DEFER,
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
        },
    )
}
