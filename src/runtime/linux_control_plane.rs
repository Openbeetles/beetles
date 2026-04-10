//! Linux supervisor-owned control plane.

use crate::bus::new_inbound_channel;
use crate::channel_capability::build_channel_capability_registry;
use crate::channels::QqMsgIdCache;
use crate::config::AppConfig;
use crate::error::{Error, Result};
use crate::platform::http_server::common::{self, CORS_HEADERS};
use crate::platform::http_server::handlers::{ControlPlaneRouteContract, HandlerContext};
use crate::platform::http_server::router::{
    self, IncomingRequest, OutgoingResponse, RestartAction, RouterEnv,
};
use crate::platform::Platform;
use std::collections::HashMap;
use std::io::Read as _;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, RwLock};

const LINUX_HTTP_WORKERS: usize = 4;

struct ConfigPlaneActiveGuard;

impl ConfigPlaneActiveGuard {
    fn enter() -> Self {
        crate::state::set_config_plane_active(true);
        Self
    }
}

impl Drop for ConfigPlaneActiveGuard {
    fn drop(&mut self) {
        crate::state::set_config_plane_active(false);
    }
}

pub fn spawn(platform: Arc<dyn Platform>) -> Result<crate::util::TaskHandle> {
    crate::util::spawn_guarded_with_profile_handle(
        "linux_control_plane",
        crate::util::STACK_CHANNEL_SENDER,
        Some(crate::util::SpawnCore::Core0),
        crate::util::HttpThreadRole::Background,
        move || {
            if let Err(error) = run(platform) {
                log::error!("[linux_control_plane] server exited: {}", error);
                crate::state::set_last_error(&error);
            }
        },
    )
    .map_err(|e| Error::io("linux_control_plane_spawn", e))
}

fn run(platform: Arc<dyn Platform>) -> Result<()> {
    crate::orchestrator::register_memory_snapshot_provider(Arc::new({
        let platform = Arc::clone(&platform);
        move || platform.memory_snapshot()
    }));
    crate::orchestrator::init();
    if let Err(error) = crate::platform::csrf::init() {
        return Err(Error::config("linux_control_plane_csrf", error.to_string()));
    }
    let soul_kernel_report = crate::runtime::ensure_platform_soul_kernel_recovery(
        platform.as_ref(),
        crate::util::current_unix_secs(),
    );
    if soul_kernel_report.restore_attempted {
        log::info!(
            "[linux_control_plane] soul_kernel recovery action={:?} restored_snapshots={} restored_layers={} degraded_after={}",
            soul_kernel_report.action,
            soul_kernel_report.restored_snapshots,
            soul_kernel_report.restored_layers.len(),
            soul_kernel_report.status_after.degraded,
        );
    } else {
        log::info!(
            "[linux_control_plane] soul_kernel ready={} safe_mode_readable={} degraded={}",
            soul_kernel_report.status_after.minimum_viable,
            soul_kernel_report.status_after.safe_mode_minimum_readable,
            soul_kernel_report.status_after.degraded,
        );
    }

    let config = crate::bootstrap::load_config(&platform);
    let skill_storage = platform.skill_storage();
    let skill_meta_store = platform.skill_meta_store();
    let skill_prompt_cache = Arc::new(crate::skills::SkillPromptCache::new(
        Arc::clone(&skill_meta_store),
        Arc::clone(&skill_storage),
        8192,
    ));
    let (tool_registry, _) = crate::build_default_registry(
        config.as_ref(),
        crate::DefaultRegistryDeps {
            platform: Arc::clone(&platform),
            remind_at_store: platform.remind_at_store(),
            session_store: platform.session_store(),
            memory_store: platform.memory_store(),
            long_term_memory_store: platform.long_term_memory_store(),
            turn_ledger_store: platform.turn_ledger_store(),
            private_garden_store: platform.private_garden_store(),
            config_store: platform.config_store(),
        },
    );
    let tool_registry = Arc::new(tool_registry);
    let channel_capability_registry =
        Arc::new(build_channel_capability_registry(config.as_ref(), false));
    let capability_package_runtime_capabilities =
        Arc::new(crate::build_capability_package_runtime_capabilities(
            channel_capability_registry.as_ref(),
            config.llm_stream,
        ));
    let ctx = Arc::new(HandlerContext {
        config_store: platform.config_store(),
        config_file_store: Arc::new(crate::config::PlatformConfigFileStore(Arc::clone(
            &platform,
        ))),
        platform: Arc::clone(&platform),
        memory_store: platform.memory_store(),
        session_store: platform.session_store(),
        skill_storage,
        skill_meta_store,
        skill_prompt_cache,
        tool_registry,
        channel_capability_registry,
        capability_package_runtime_capabilities,
        inbound_depth: Arc::new(AtomicUsize::new(0)),
        outbound_depth: Arc::new(AtomicUsize::new(0)),
        version: Arc::from(env!("CARGO_PKG_VERSION")),
        board_id: Arc::from(crate::platform::runtime_board::resolved_board_id()),
        cached_config: Arc::new(RwLock::new((*config).clone())),
        llm_stream_enabled: config.llm_stream,
        route_contract: ControlPlaneRouteContract::SUPERVISOR_MINIMAL,
    });
    let router_env = Arc::new(build_router_env(config.as_ref()));
    let listen =
        std::env::var("BEETLE_CONFIG_HTTP_LISTEN").unwrap_or_else(|_| "0.0.0.0:80".to_string());
    let server = Arc::new(tiny_http::Server::http(&listen).map_err(|e| Error::Other {
        source: Box::new(std::io::Error::other(e.to_string())),
        stage: "linux_control_plane_listen",
    })?);
    let _active_guard = ConfigPlaneActiveGuard::enter();
    log::info!(
        "[linux_control_plane] listening on {} (supervisor-owned control plane)",
        listen
    );
    for index in 0..LINUX_HTTP_WORKERS {
        let worker_name = format!("linux_control_plane_worker_{}", index);
        let server = Arc::clone(&server);
        let ctx = Arc::clone(&ctx);
        let router_env = Arc::clone(&router_env);
        crate::util::spawn_guarded_with_profile(
            &worker_name,
            crate::util::STACK_CHANNEL_SENDER,
            Some(crate::util::SpawnCore::Core0),
            crate::util::HttpThreadRole::Background,
            move || loop {
                match server.recv() {
                    Ok(request) => handle_request(&ctx, router_env.as_ref(), request),
                    Err(error) => {
                        log::warn!("[linux_control_plane] recv failed: {}", error);
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

fn handle_request(
    ctx: &Arc<HandlerContext>,
    router_env: &RouterEnv,
    mut request: tiny_http::Request,
) {
    let method = request.method().as_str().to_string();
    let uri = request.url().to_string();
    let path = uri.split('?').next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    for header in request.headers() {
        headers.push((header.field.to_string(), header.value.as_str().to_string()));
    }
    let max_body = linux_max_body_bytes(&path, &method);
    let mut body = Vec::new();
    if max_body > 0 {
        if let Err(error) = request
            .as_reader()
            .take(max_body as u64)
            .read_to_end(&mut body)
        {
            log::warn!("[linux_control_plane] body read failed: {}", error);
            respond(
                request,
                OutgoingResponse::json(
                    500,
                    "Internal Server Error",
                    CORS_HEADERS,
                    br#"{"error":"internal error"}"#.to_vec(),
                ),
            );
            return;
        }
    }
    let incoming = IncomingRequest {
        method,
        uri,
        headers,
        body,
    };
    let response = dispatch(ctx, router_env, incoming).unwrap_or_else(|error| {
        log::warn!("[linux_control_plane] dispatch failed: {}", error);
        OutgoingResponse::json(
            500,
            "Internal Server Error",
            CORS_HEADERS,
            br#"{"error":"internal error"}"#.to_vec(),
        )
    });
    let restart = response.restart;
    respond(request, response);
    if restart == RestartAction::After300Ms {
        crate::util::spawn_guarded_with_profile(
            "linux_control_plane_restart_defer",
            crate::util::STACK_RESTART_DEFER,
            Some(crate::util::SpawnCore::Core0),
            crate::util::HttpThreadRole::Background,
            move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                if let Err(error) = crate::runtime::linux_supervisor::request_restart() {
                    log::warn!(
                        "[linux_control_plane] failed to request restart after response: {}",
                        error
                    );
                }
            },
        );
    }
}

fn dispatch(
    ctx: &HandlerContext,
    router_env: &RouterEnv,
    incoming: IncomingRequest,
) -> Result<OutgoingResponse> {
    let path = incoming.uri.split('?').next().unwrap_or("/");
    if !ctx.route_contract.inbound_webhooks_enabled && supervisor_blocks_webhook_route(path) {
        return Ok(OutgoingResponse::json(
            404,
            "Not Found",
            CORS_HEADERS,
            br#"{"error":"not found"}"#.to_vec(),
        ));
    }
    router::dispatch(ctx, router_env, incoming)
}

fn build_router_env(config: &AppConfig) -> RouterEnv {
    let (inbound_tx, _inbound_rx, _inbound_depth) =
        new_inbound_channel(crate::constants::DEFAULT_CAPACITY);
    let qq_msg_id_cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
    RouterEnv::new(
        inbound_tx,
        qq_msg_id_cache,
        false,
        config.qq_channel_app_id.clone(),
        config.qq_channel_secret.clone(),
    )
}

fn supervisor_blocks_webhook_route(path: &str) -> bool {
    matches!(
        path,
        "/api/webhook"
            | "/api/feishu/event"
            | "/api/dingtalk/webhook"
            | "/api/wecom/webhook"
            | "/api/webhook/qq"
    )
}

fn respond(request: tiny_http::Request, outgoing: OutgoingResponse) {
    let mut response = tiny_http::Response::from_data(outgoing.body)
        .with_status_code(tiny_http::StatusCode(outgoing.status));
    for (key, value) in outgoing.headers {
        if let Ok(header) = tiny_http::Header::from_bytes(*key, *value) {
            response.add_header(header);
        }
    }
    if let Err(error) = request.respond(response) {
        log::warn!("[linux_control_plane] respond failed: {}", error);
    }
}

fn linux_max_body_bytes(path: &str, method: &str) -> usize {
    let method = method.to_ascii_uppercase();
    if matches!(method.as_str(), "GET" | "OPTIONS" | "HEAD" | "DELETE") {
        return 0;
    }
    match path {
        "/api/soul" | "/api/user" => crate::memory::MAX_SOUL_USER_LEN,
        "/api/capability_packages" => {
            crate::capability_package::MAX_CAPABILITY_PACKAGE_HTTP_BODY_LEN
        }
        "/api/feishu/event" => 64 * 1024,
        "/api/webhook/qq" => crate::channels::QQ_WEBHOOK_BODY_MAX,
        _ => common::POST_BODY_MAX_LEN,
    }
}

#[cfg(test)]
mod tests {
    use super::{build_router_env, dispatch};
    use crate::config::AppConfig;
    use crate::platform::http_server::handlers::{ControlPlaneRouteContract, HandlerContext};
    use crate::platform::http_server::router::IncomingRequest;
    use crate::platform::Platform;
    use serde_json::Value;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, RwLock};

    fn build_test_context() -> (HandlerContext, super::RouterEnv) {
        let config = AppConfig::load_from_env();
        let platform: Arc<dyn Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        crate::platform::pairing::clear_code(platform.config_store().as_ref()).unwrap();
        assert!(
            crate::platform::pairing::set_code(platform.config_store().as_ref(), "123456",)
                .unwrap()
        );

        let skill_storage = platform.skill_storage();
        let skill_meta_store = platform.skill_meta_store();
        let skill_prompt_cache = Arc::new(crate::skills::SkillPromptCache::new(
            Arc::clone(&skill_meta_store),
            Arc::clone(&skill_storage),
            8192,
        ));
        let (tool_registry, _) = crate::build_default_registry(
            &config,
            crate::DefaultRegistryDeps {
                platform: Arc::clone(&platform),
                remind_at_store: platform.remind_at_store(),
                session_store: platform.session_store(),
                memory_store: platform.memory_store(),
                long_term_memory_store: platform.long_term_memory_store(),
                turn_ledger_store: platform.turn_ledger_store(),
                private_garden_store: platform.private_garden_store(),
                config_store: platform.config_store(),
            },
        );
        let channel_capability_registry =
            Arc::new(crate::build_channel_capability_registry(&config, false));
        let ctx = HandlerContext {
            config_store: platform.config_store(),
            config_file_store: Arc::new(crate::config::PlatformConfigFileStore(Arc::clone(
                &platform,
            ))),
            platform: Arc::clone(&platform),
            memory_store: platform.memory_store(),
            session_store: platform.session_store(),
            skill_storage,
            skill_meta_store,
            skill_prompt_cache,
            tool_registry: Arc::new(tool_registry),
            channel_capability_registry: Arc::clone(&channel_capability_registry),
            capability_package_runtime_capabilities: Arc::new(
                crate::build_capability_package_runtime_capabilities(
                    channel_capability_registry.as_ref(),
                    false,
                ),
            ),
            inbound_depth: Arc::new(AtomicUsize::new(0)),
            outbound_depth: Arc::new(AtomicUsize::new(0)),
            version: Arc::from("0.0.0"),
            board_id: Arc::from("linux"),
            cached_config: Arc::new(RwLock::new(config.clone())),
            llm_stream_enabled: false,
            route_contract: ControlPlaneRouteContract::SUPERVISOR_MINIMAL,
        };
        let router_env = build_router_env(&config);
        (ctx, router_env)
    }

    fn request(method: &str, uri: &str) -> IncomingRequest {
        IncomingRequest {
            method: method.to_string(),
            uri: uri.to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    #[test]
    fn supervisor_root_inventory_exposes_shared_tools_and_skills_routes() {
        let (ctx, router_env) = build_test_context();
        let out = dispatch(&ctx, &router_env, request("GET", "/")).expect("dispatch /");
        assert_eq!(out.status, 200);

        let body = String::from_utf8(out.body).expect("utf8 body");
        let parsed: Value = serde_json::from_str(&body).expect("root json");
        let endpoints = parsed
            .get("endpoints")
            .and_then(Value::as_array)
            .expect("endpoints array");
        let endpoint_values: Vec<&str> = endpoints.iter().filter_map(Value::as_str).collect();

        assert_eq!(parsed.get("name").and_then(Value::as_str), Some("beetle"));
        assert!(endpoint_values.contains(&"GET /api/tools"));
        assert!(endpoint_values.contains(&"GET /api/skills"));
        assert!(!endpoint_values.contains(&"POST /api/webhook"));
        assert!(!endpoint_values.contains(&"POST /api/webhook/qq"));
    }

    #[test]
    fn supervisor_control_plane_dispatches_tools_and_skills_routes() {
        let (ctx, router_env) = build_test_context();

        let tools = dispatch(&ctx, &router_env, request("GET", "/api/tools")).expect("tools");
        assert_eq!(tools.status, 200);

        let skills = dispatch(&ctx, &router_env, request("GET", "/api/skills")).expect("skills");
        assert_eq!(skills.status, 200);
    }

    #[test]
    fn supervisor_control_plane_keeps_webhook_ingress_unavailable() {
        let (ctx, router_env) = build_test_context();
        let out =
            dispatch(&ctx, &router_env, request("POST", "/api/webhook")).expect("webhook dispatch");
        assert_eq!(out.status, 404);
    }
}
