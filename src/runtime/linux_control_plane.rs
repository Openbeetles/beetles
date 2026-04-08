//! Linux supervisor-owned control plane.

use crate::channel_capability::build_channel_capability_registry;
use crate::error::{Error, Result};
use crate::platform::http_server::common::{
    self, ApiResponse, CORS_AND_TEXT_PLAIN, CORS_HEADERS, CORS_OPTIONS_HEADERS, CSS_HEADERS,
    HTML_HEADERS, JS_HEADERS, REDIRECT_PAIRING_HEADERS,
};
use crate::platform::http_server::handlers::{self, HandlerContext};
use crate::platform::http_server::router::{
    auth, IncomingRequest, OutgoingResponse, RestartAction,
};
use crate::platform::Platform;
use std::io::Read as _;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, RwLock};

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
    });
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
        crate::util::spawn_guarded_with_profile(
            &worker_name,
            crate::util::STACK_CHANNEL_SENDER,
            Some(crate::util::SpawnCore::Core0),
            crate::util::HttpThreadRole::Background,
            move || loop {
                match server.recv() {
                    Ok(request) => handle_request(&ctx, request),
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

fn handle_request(ctx: &Arc<HandlerContext>, mut request: tiny_http::Request) {
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
    let response = dispatch(ctx, incoming).unwrap_or_else(|error| {
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

fn dispatch(ctx: &HandlerContext, incoming: IncomingRequest) -> Result<OutgoingResponse> {
    let path = incoming.uri.split('?').next().unwrap_or("/");
    let method = incoming.method.as_str();
    let store = ctx.config_store.as_ref();
    let uri = incoming.uri.as_str();

    if method.eq_ignore_ascii_case("OPTIONS") {
        return Ok(OutgoingResponse::json(
            200,
            "OK",
            CORS_OPTIONS_HEADERS,
            b" ".to_vec(),
        ));
    }

    match (method, path) {
        ("GET", "/") => {
            if !crate::platform::pairing::code_set(store) {
                return Ok(OutgoingResponse::json(
                    302,
                    "Found",
                    REDIRECT_PAIRING_HEADERS,
                    Vec::new(),
                ));
            }
            let body = control_plane_root_body(ctx)?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/wifi") => Ok(OutgoingResponse::json(
            200,
            "OK",
            HTML_HEADERS,
            handlers::config_page::html().as_bytes().to_vec(),
        )),
        ("GET", "/pairing") => Ok(OutgoingResponse::json(
            200,
            "OK",
            HTML_HEADERS,
            handlers::config_page::pairing_html().as_bytes().to_vec(),
        )),
        ("GET", "/common.css") => Ok(OutgoingResponse::json(
            200,
            "OK",
            CSS_HEADERS,
            handlers::config_page::common_css().as_bytes().to_vec(),
        )),
        ("GET", "/common.js") => Ok(OutgoingResponse::json(
            200,
            "OK",
            JS_HEADERS,
            handlers::config_page::common_js().as_bytes().to_vec(),
        )),
        ("GET", "/api/pairing_code") => Ok(OutgoingResponse::json(
            200,
            "OK",
            CORS_HEADERS,
            handlers::pairing::body(ctx).into_bytes(),
        )),
        ("POST", "/api/pairing_code") => {
            let body = utf8_body(&incoming.body)?;
            Ok(api_to_out(handlers::pairing::post_body(ctx, body)))
        }
        ("GET", "/api/csrf_token") => {
            let body = handlers::csrf_token::body(ctx)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/api/config") => {
            if let Some(response) = auth::require_pairing_code(store, uri, &incoming.headers) {
                return Ok(api_to_out(response));
            }
            let body = handlers::config::get_body(ctx)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("POST", "/api/config/wifi") => {
            if let Some(response) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(response);
            }
            let body = utf8_body(&incoming.body)?;
            let response = handlers::config::post_wifi(ctx, body)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            Ok(with_optional_restart(response, uri))
        }
        ("POST", "/api/config/llm") => {
            if let Some(response) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(response);
            }
            let body = utf8_body(&incoming.body)?;
            Ok(api_to_out(handlers::config::post_llm(ctx, body).map_err(
                |e| route_error("linux_control_plane_dispatch", e),
            )?))
        }
        ("POST", "/api/config/channels") => {
            if let Some(response) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(response);
            }
            let body = utf8_body(&incoming.body)?;
            Ok(api_to_out(
                handlers::config::post_channels(ctx, body)
                    .map_err(|e| route_error("linux_control_plane_dispatch", e))?,
            ))
        }
        ("POST", "/api/config/system") => {
            if let Some(response) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(response);
            }
            let body = utf8_body(&incoming.body)?;
            Ok(api_to_out(
                handlers::config::post_system(ctx, body)
                    .map_err(|e| route_error("linux_control_plane_dispatch", e))?,
            ))
        }
        ("GET", "/api/config/hardware") => {
            if let Some(response) = auth::require_pairing_code(store, uri, &incoming.headers) {
                return Ok(api_to_out(response));
            }
            let body = handlers::config::get_hardware_body(ctx)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("POST", "/api/config/hardware") => {
            if let Some(response) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(response);
            }
            let body = utf8_body(&incoming.body)?;
            Ok(api_to_out(
                handlers::config::post_hardware(ctx, body)
                    .map_err(|e| route_error("linux_control_plane_dispatch", e))?,
            ))
        }
        ("GET", "/api/config/audio") => {
            if let Some(response) = auth::require_pairing_code(store, uri, &incoming.headers) {
                return Ok(api_to_out(response));
            }
            let body = handlers::config::get_audio_body(ctx)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("POST", "/api/config/audio") => {
            if let Some(response) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(response);
            }
            let body = utf8_body(&incoming.body)?;
            let response = handlers::config::post_audio(ctx, body)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            Ok(with_optional_restart(response, uri))
        }
        ("GET", "/api/config/display") => {
            if let Some(response) = auth::require_pairing_code(store, uri, &incoming.headers) {
                return Ok(api_to_out(response));
            }
            let body = handlers::config::get_display_body(ctx)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("POST", "/api/config/display") => {
            if let Some(response) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(response);
            }
            let body = utf8_body(&incoming.body)?;
            let response = handlers::config::post_display(ctx, body)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            Ok(with_optional_restart(response, uri))
        }
        ("GET", "/api/wifi/scan") => match handlers::wifi_scan::get_body(ctx) {
            Ok(body) => Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            )),
            Err(handlers::wifi_scan::WifiScanError::Unavailable) => Ok(OutgoingResponse::json(
                503,
                "Service Unavailable",
                CORS_HEADERS,
                br#"{"error":"wifi scan not available"}"#.to_vec(),
            )),
            Err(handlers::wifi_scan::WifiScanError::Other(error)) => Ok(OutgoingResponse::json(
                500,
                "Internal Server Error",
                CORS_HEADERS,
                format!(r#"{{"error":"{}"}}"#, escape_json(&error.to_string())).into_bytes(),
            )),
        },
        ("GET", "/api/hardware/discovery") => {
            if let Some(response) = auth::require_pairing_code(store, uri, &incoming.headers) {
                return Ok(api_to_out(response));
            }
            let Some(bus) = hardware_bus_from_uri(uri) else {
                return Ok(api_to_out(ApiResponse::err_400("missing or invalid bus")));
            };
            let Some(capability) = hardware_capability_from_uri(uri) else {
                return Ok(api_to_out(ApiResponse::err_400(
                    "missing or invalid capability",
                )));
            };
            match handlers::hardware_discovery::get_body(ctx, bus, capability) {
                Ok(body) => Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_HEADERS,
                    body.into_bytes(),
                )),
                Err(handlers::hardware_discovery::HardwareDiscoveryError::Unavailable) => {
                    Ok(OutgoingResponse::json(
                        503,
                        "Service Unavailable",
                        CORS_HEADERS,
                        br#"{"error":"hardware discovery not available"}"#.to_vec(),
                    ))
                }
                Err(handlers::hardware_discovery::HardwareDiscoveryError::Other(error)) => {
                    Ok(OutgoingResponse::json(
                        500,
                        "Internal Server Error",
                        CORS_HEADERS,
                        format!(r#"{{"error":"{}"}}"#, escape_json(&error.to_string()))
                            .into_bytes(),
                    ))
                }
            }
        }
        ("GET", "/api/health") => {
            if let Some(response) = auth::require_activated(store) {
                return Ok(api_to_out(response));
            }
            let body = handlers::health::body(ctx)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/api/resource") => {
            if let Some(response) = auth::require_activated(store) {
                return Ok(api_to_out(response));
            }
            let body = handlers::resource::body(ctx)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/api/metrics") => {
            if let Some(response) = auth::require_activated(store) {
                return Ok(api_to_out(response));
            }
            if uri.contains("format=prometheus") {
                let body = handlers::metrics::body_prometheus(ctx)
                    .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
                Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_AND_TEXT_PLAIN,
                    body.into_bytes(),
                ))
            } else {
                let body = handlers::metrics::body(ctx)
                    .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
                Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_HEADERS,
                    body.into_bytes(),
                ))
            }
        }
        ("GET", "/api/operator/status") => {
            if let Some(response) = auth::require_activated(store) {
                return Ok(api_to_out(response));
            }
            let body = handlers::operator_status::body(ctx)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/api/diagnose") => {
            if let Some(response) = auth::require_activated(store) {
                return Ok(api_to_out(response));
            }
            let body = handlers::diagnose::body(ctx)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/api/system_info") => {
            if let Some(response) = auth::require_activated(store) {
                return Ok(api_to_out(response));
            }
            let body = handlers::system_info::body(ctx)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/api/channel_connectivity") => {
            if let Some(response) = auth::require_activated(store) {
                return Ok(api_to_out(response));
            }
            match handlers::channel_connectivity::body(ctx) {
                Ok(body) => Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_HEADERS,
                    body.into_bytes(),
                )),
                Err(message) => Ok(OutgoingResponse::json(
                    500,
                    "Internal Server Error",
                    CORS_HEADERS,
                    format!(r#"{{"error":"{}"}}"#, escape_json(&message)).into_bytes(),
                )),
            }
        }
        ("POST", "/api/restart") => {
            if let Some(response) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(response);
            }
            let (response, should_restart) = handlers::restart::post(ctx)
                .map_err(|e| route_error("linux_control_plane_dispatch", e))?;
            let mut outgoing = api_to_out(response);
            if should_restart {
                outgoing.restart = RestartAction::After300Ms;
            }
            Ok(outgoing)
        }
        ("POST", "/api/config_reset") => {
            if let Some(response) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(response);
            }
            Ok(api_to_out(handlers::config_reset::post(ctx).map_err(
                |e| route_error("linux_control_plane_dispatch", e),
            )?))
        }
        _ => Ok(OutgoingResponse::json(
            404,
            "Not Found",
            CORS_HEADERS,
            br#"{"error":"not found"}"#.to_vec(),
        )),
    }
}

fn control_plane_root_body(ctx: &HandlerContext) -> Result<String> {
    let endpoints = vec![
        "GET /pairing",
        "GET /wifi",
        "GET /api/pairing_code",
        "POST /api/pairing_code",
        "GET /api/csrf_token",
        "GET /api/config",
        "POST /api/config/wifi",
        "POST /api/config/llm",
        "POST /api/config/channels",
        "POST /api/config/system",
        "GET /api/config/hardware",
        "POST /api/config/hardware",
        "GET /api/config/audio",
        "POST /api/config/audio",
        "GET /api/config/display",
        "POST /api/config/display",
        "GET /api/wifi/scan",
        "GET /api/hardware/discovery",
        "GET /api/health",
        "GET /api/resource",
        "GET /api/metrics",
        "GET /api/operator/status",
        "GET /api/diagnose",
        "GET /api/system_info",
        "GET /api/channel_connectivity",
        "POST /api/restart",
        "POST /api/config_reset",
    ];
    let endpoints_json = serde_json::to_string(&endpoints)
        .map_err(|e| Error::config("linux_control_plane", e.to_string()))?;
    Ok(format!(
        r#"{{"name":"beetle-control-plane","version":"{}","endpoints":{}}}"#,
        ctx.version.as_ref(),
        endpoints_json
    ))
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

fn with_optional_restart(response: ApiResponse, uri: &str) -> OutgoingResponse {
    let mut outgoing = api_to_out(response);
    if outgoing.status == 200 && common::restart_requested_from_uri(uri) {
        outgoing.restart = RestartAction::After300Ms;
    }
    outgoing
}

fn api_to_out(response: ApiResponse) -> OutgoingResponse {
    OutgoingResponse {
        status: response.status,
        status_text: response.status_text,
        headers: CORS_HEADERS,
        body: response.body,
        restart: RestartAction::None,
    }
}

fn guard_pairing_csrf(
    store: &dyn crate::platform::ConfigStore,
    uri: &str,
    headers: &[(String, String)],
) -> Option<OutgoingResponse> {
    if let Some(response) = auth::require_pairing_code(store, uri, headers) {
        return Some(api_to_out(response));
    }
    auth::require_csrf(store, headers).map(api_to_out)
}

fn linux_max_body_bytes(path: &str, method: &str) -> usize {
    let method = method.to_ascii_uppercase();
    if matches!(method.as_str(), "GET" | "OPTIONS" | "HEAD" | "DELETE") {
        return 0;
    }
    match path {
        "/api/soul" | "/api/user" => crate::memory::MAX_SOUL_USER_LEN,
        _ => common::POST_BODY_MAX_LEN,
    }
}

fn utf8_body(body: &[u8]) -> Result<&str> {
    std::str::from_utf8(body).map_err(|_| route_error("linux_control_plane_body", "invalid utf8"))
}

fn route_error(stage: &'static str, message: impl std::fmt::Display) -> Error {
    Error::Other {
        source: Box::new(std::io::Error::other(message.to_string())),
        stage,
    }
}

fn escape_json(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn query_param_from_uri<'a>(uri: &'a str, key: &str) -> Option<&'a str> {
    let query = uri.find('?').map(|index| &uri[index + 1..]).unwrap_or("");
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case(key))
        {
            return parts.next().filter(|value| !value.is_empty());
        }
    }
    None
}

fn hardware_bus_from_uri(uri: &str) -> Option<crate::platform::HardwareDiscoveryBus> {
    query_param_from_uri(uri, "bus").and_then(crate::platform::HardwareDiscoveryBus::parse)
}

fn hardware_capability_from_uri(uri: &str) -> Option<crate::platform::HardwareCapability> {
    query_param_from_uri(uri, "capability").and_then(crate::platform::HardwareCapability::parse)
}
