//! 唯一路由分发：与 ESP `mod.rs` 中 `register!` 行为等价。
//! Single route dispatch; behavior matches ESP `register!` in `mod.rs`.

use super::auth;
use super::types::{IncomingRequest, OutgoingResponse, RestartAction, RouterEnv};
use crate::error::{Error, Result};
use crate::i18n::{locale_from_store, tr, Message};
use crate::platform::http_server::common::{
    self, ApiResponse, CORS_AND_TEXT_PLAIN, CORS_HEADERS, CORS_OPTIONS_HEADERS, CSS_HEADERS,
    HTML_HEADERS, JS_HEADERS, REDIRECT_PAIRING_HEADERS,
};
use crate::platform::http_server::handlers::{self, HandlerContext};
use crate::platform::{HardwareCapability, HardwareDiscoveryBus};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use std::sync::Arc;

const OPTIONS_BODY: &[u8] = b" ";

#[inline(never)]
fn path_only(uri: &str) -> &str {
    uri.split('?').next().unwrap_or("/")
}

/// Extract `chat_id` query parameter from URI (case-insensitive key match).
#[inline(never)]
fn chat_id_from_uri(uri: &str) -> Option<String> {
    let query = uri.find('?').map(|i| &uri[i + 1..]).unwrap_or("");
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next().is_some_and(|k| k.eq_ignore_ascii_case("chat_id")) {
            return it.next().filter(|s| !s.is_empty()).map(String::from);
        }
    }
    None
}

/// Extract pagination parameters from URI: page (default 1) and limit (default 20).
#[inline(never)]
fn pagination_from_uri(uri: &str) -> (usize, usize) {
    let query = uri.find('?').map(|i| &uri[i + 1..]).unwrap_or("");
    let mut page = 1;
    let mut limit = 20;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if let Some(key) = it.next() {
            if let Some(val) = it.next() {
                if key.eq_ignore_ascii_case("page") {
                    page = val.parse().unwrap_or(1);
                } else if key.eq_ignore_ascii_case("limit") {
                    limit = val.parse().unwrap_or(20);
                }
            }
        }
    }
    (page, limit)
}

/// Extract format parameter from URI (e.g., ?format=prometheus).
#[inline(never)]
fn format_from_uri(uri: &str) -> Option<&str> {
    let query = uri.find('?').map(|i| &uri[i + 1..]).unwrap_or("");
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next().is_some_and(|k| k.eq_ignore_ascii_case("format")) {
            return it.next();
        }
    }
    None
}

#[inline(never)]
fn query_param_from_uri<'a>(uri: &'a str, key: &str) -> Option<&'a str> {
    let query = uri.find('?').map(|i| &uri[i + 1..]).unwrap_or("");
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next().is_some_and(|k| k.eq_ignore_ascii_case(key)) {
            return it.next().filter(|value| !value.is_empty());
        }
    }
    None
}

#[inline(never)]
fn hardware_bus_from_uri(uri: &str) -> Option<HardwareDiscoveryBus> {
    query_param_from_uri(uri, "bus").and_then(HardwareDiscoveryBus::parse)
}

#[inline(never)]
fn hardware_capability_from_uri(uri: &str) -> Option<HardwareCapability> {
    query_param_from_uri(uri, "capability").and_then(HardwareCapability::parse)
}

#[inline(never)]
fn api_to_out(r: ApiResponse) -> OutgoingResponse {
    OutgoingResponse {
        status: r.status,
        status_text: r.status_text,
        headers: CORS_HEADERS,
        body: r.body,
        restart: RestartAction::None,
    }
}

#[inline(never)]
fn utf8_body(body: &[u8]) -> Result<&str> {
    std::str::from_utf8(body).map_err(|_| Error::Other {
        source: Box::new(std::io::Error::other("invalid utf8")),
        stage: "http_router_dispatch",
    })
}

/// 写操作鉴权：配对码 + CSRF；命中则返回已组装的 JSON 响应。
#[inline(never)]
fn guard_pairing_csrf(
    store: &dyn crate::platform::ConfigStore,
    uri: &str,
    headers: &[(String, String)],
) -> Option<OutgoingResponse> {
    if let Some(r) = auth::require_pairing_code(store, uri, headers) {
        return Some(api_to_out(r));
    }
    if let Some(r) = auth::require_csrf(store, headers) {
        return Some(api_to_out(r));
    }
    None
}

fn err_other(stage: &'static str, msg: impl std::fmt::Display) -> Error {
    Error::Other {
        source: Box::new(std::io::Error::other(msg.to_string())),
        stage,
    }
}

fn operator_window_required_response(path: &str) -> OutgoingResponse {
    let body = serde_json::json!({
        "error": "operator window required",
        "path": path,
        "open_endpoint": "POST /api/operator/window",
    });
    OutgoingResponse::json(
        403,
        "Forbidden",
        CORS_HEADERS,
        serde_json::to_vec(&body)
            .unwrap_or_else(|_| br#"{"error":"operator window required"}"#.to_vec()),
    )
}

/// 配置 API 唯一入口：ESP / Linux 在组装 `IncomingRequest` 后调用。
#[inline(never)]
pub fn dispatch(
    ctx: &HandlerContext,
    env: &RouterEnv,
    incoming: IncomingRequest,
) -> Result<OutgoingResponse> {
    let path = path_only(&incoming.uri);
    let method = incoming.method.as_str();
    let uri = incoming.uri.as_str();
    let store = ctx.config_store.as_ref();

    // OPTIONS 预检：与 `resp_options!` 一致
    if method.eq_ignore_ascii_case("OPTIONS") {
        return Ok(OutgoingResponse {
            status: 200,
            status_text: "OK",
            headers: CORS_OPTIONS_HEADERS,
            body: OPTIONS_BODY.to_vec(),
            restart: RestartAction::None,
        });
    }

    let memory_system_kind = ctx.platform.memory_system_kind();
    if crate::platform::operator_surface::route_requires_operator_window(memory_system_kind, path)
        && crate::platform::operator_surface::current_operator_surface_budget(memory_system_kind)
            .window_required_for_deep_routes
    {
        return Ok(operator_window_required_response(path));
    }

    match (method, path) {
        ("GET", "/") => {
            if !crate::platform::pairing::code_set(store) {
                return Ok(OutgoingResponse {
                    status: 302,
                    status_text: "Found",
                    headers: REDIRECT_PAIRING_HEADERS,
                    body: Vec::new(),
                    restart: RestartAction::None,
                });
            }
            let body =
                handlers::root::body(ctx).map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/wifi") => {
            let html = handlers::config_page::html();
            Ok(OutgoingResponse::json(
                200,
                "OK",
                HTML_HEADERS,
                html.as_bytes().to_vec(),
            ))
        }
        ("GET", "/pairing") => {
            let html = handlers::config_page::pairing_html();
            Ok(OutgoingResponse::json(
                200,
                "OK",
                HTML_HEADERS,
                html.as_bytes().to_vec(),
            ))
        }
        ("GET", "/common.css") => {
            let css = handlers::config_page::common_css();
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CSS_HEADERS,
                css.as_bytes().to_vec(),
            ))
        }
        ("GET", "/common.js") => {
            let js = handlers::config_page::common_js();
            Ok(OutgoingResponse::json(
                200,
                "OK",
                JS_HEADERS,
                js.as_bytes().to_vec(),
            ))
        }
        ("GET", "/api/pairing_code") => {
            let body = handlers::pairing::body(ctx);
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("POST", "/api/pairing_code") => {
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::pairing::post_body(ctx, body_str);
            Ok(api_to_out(r))
        }
        ("GET", "/api/config") => {
            if let Some(r) = auth::require_pairing_code(store, uri, &incoming.headers) {
                return Ok(api_to_out(r));
            }
            let body = handlers::config::get_body(ctx)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("POST", "/api/config/wifi") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::config::post_wifi(ctx, body_str)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            let mut restart = RestartAction::None;
            if r.status == 200 && common::restart_requested_from_uri(uri) {
                restart = RestartAction::After300Ms;
            }
            let mut out = api_to_out(r);
            out.restart = restart;
            Ok(out)
        }
        ("POST", "/api/config/llm") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::config::post_llm(ctx, body_str)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(api_to_out(r))
        }
        ("POST", "/api/config/channels") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::config::post_channels(ctx, body_str)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(api_to_out(r))
        }
        ("POST", "/api/config/system") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::config::post_system(ctx, body_str)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(api_to_out(r))
        }
        ("GET", "/api/config/hardware") => {
            if let Some(r) = auth::require_pairing_code(store, uri, &incoming.headers) {
                return Ok(api_to_out(r));
            }
            let body = handlers::config::get_hardware_body(ctx)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("POST", "/api/config/hardware") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::config::post_hardware(ctx, body_str)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(api_to_out(r))
        }
        ("GET", "/api/config/audio") => {
            if let Some(r) = auth::require_pairing_code(store, uri, &incoming.headers) {
                return Ok(api_to_out(r));
            }
            let body = handlers::config::get_audio_body(ctx)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("POST", "/api/config/audio") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::config::post_audio(ctx, body_str)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            let mut restart = RestartAction::None;
            if r.status == 200 && common::restart_requested_from_uri(uri) {
                restart = RestartAction::After300Ms;
            }
            let mut out = api_to_out(r);
            out.restart = restart;
            Ok(out)
        }
        ("GET", "/api/config/display") => {
            if let Some(r) = auth::require_pairing_code(store, uri, &incoming.headers) {
                return Ok(api_to_out(r));
            }
            let body = handlers::config::get_display_body(ctx)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("POST", "/api/config/display") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::config::post_display(ctx, body_str)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            let mut restart = RestartAction::None;
            if r.status == 200 && common::restart_requested_from_uri(uri) {
                restart = RestartAction::After300Ms;
            }
            let mut out = api_to_out(r);
            out.restart = restart;
            Ok(out)
        }
        ("GET", "/api/wifi/scan") => match handlers::wifi_scan::get_body(ctx) {
            Ok(body) => Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            )),
            Err(handlers::wifi_scan::WifiScanError::Unavailable) => {
                let body = serde_json::json!({ "error": "wifi scan not available (non-ESP or wifi not ready)" }).to_string();
                Ok(OutgoingResponse::json(
                    503,
                    "Service Unavailable",
                    CORS_HEADERS,
                    body.into_bytes(),
                ))
            }
            Err(handlers::wifi_scan::WifiScanError::Other(e)) => {
                let body = serde_json::json!({ "error": e.to_string() }).to_string();
                Ok(OutgoingResponse::json(
                    500,
                    "Internal Server Error",
                    CORS_HEADERS,
                    body.into_bytes(),
                ))
            }
        },
        ("GET", "/api/hardware/discovery") => {
            if let Some(r) = auth::require_pairing_code(store, uri, &incoming.headers) {
                return Ok(api_to_out(r));
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
                    let body = serde_json::json!({ "error": "hardware discovery not available" })
                        .to_string();
                    Ok(OutgoingResponse::json(
                        503,
                        "Service Unavailable",
                        CORS_HEADERS,
                        body.into_bytes(),
                    ))
                }
                Err(handlers::hardware_discovery::HardwareDiscoveryError::Other(e)) => {
                    let body = serde_json::json!({ "error": e.to_string() }).to_string();
                    Ok(OutgoingResponse::json(
                        500,
                        "Internal Server Error",
                        CORS_HEADERS,
                        body.into_bytes(),
                    ))
                }
            }
        }
        ("GET", "/api/health") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            match handlers::health::body(ctx) {
                Ok(body) => Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_HEADERS,
                    body.into_bytes(),
                )),
                Err(_) => {
                    let loc = locale_from_store(store);
                    let msg = tr(Message::OperationFailed, loc);
                    Ok(api_to_out(ApiResponse::err_500(&msg)))
                }
            }
        }
        ("GET", "/api/operator/status") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            let body = handlers::operator_status::body(ctx)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/api/metrics") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            let format = format_from_uri(uri);
            if format == Some("prometheus") {
                let body = handlers::metrics::body_prometheus(ctx)
                    .map_err(|e| err_other("http_router_dispatch", e))?;
                Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_AND_TEXT_PLAIN,
                    body.into_bytes(),
                ))
            } else {
                let body = handlers::metrics::body(ctx)
                    .map_err(|e| err_other("http_router_dispatch", e))?;
                Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_HEADERS,
                    body.into_bytes(),
                ))
            }
        }
        ("GET", "/api/resource") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            let body =
                handlers::resource::body(ctx).map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/api/tools") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            let body =
                handlers::tools::body(ctx).map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/api/csrf_token") => {
            let body = handlers::csrf_token::body(ctx)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/api/diagnose") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            let body =
                handlers::diagnose::body(ctx).map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/api/system_info") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            let body = handlers::system_info::body(ctx)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("POST", "/api/operator/window") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body = handlers::operator_window::post(ctx)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", "/api/channel_connectivity") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            match handlers::channel_connectivity::body(ctx) {
                Ok(body) => Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_HEADERS,
                    body.into_bytes(),
                )),
                Err(msg) => {
                    let body = format!(r#"{{"error":"{}"}}"#, msg.replace('"', "\\\""));
                    Ok(OutgoingResponse::json(
                        500,
                        "Internal Server Error",
                        CORS_HEADERS,
                        body.into_bytes(),
                    ))
                }
            }
        }
        ("GET", "/api/sessions") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            let chat_id = common::name_from_uri(uri).or_else(|| chat_id_from_uri(uri));
            let result = match chat_id {
                Some(id) => handlers::sessions::detail(ctx, &id),
                None => {
                    let (page, limit) = pagination_from_uri(uri);
                    handlers::sessions::body(ctx, page, limit)
                }
            };
            match result {
                Ok(body) => Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_HEADERS,
                    body.into_bytes(),
                )),
                Err(msg) => {
                    let body = format!(r#"{{"error":"{}"}}"#, msg.replace('"', "\\\""));
                    Ok(OutgoingResponse::json(
                        500,
                        "Internal Server Error",
                        CORS_HEADERS,
                        body.into_bytes(),
                    ))
                }
            }
        }
        ("DELETE", "/api/sessions") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let chat_id = chat_id_from_uri(uri);
            match chat_id {
                Some(id) => match handlers::sessions::delete(ctx, &id) {
                    Ok(body) => Ok(OutgoingResponse::json(
                        200,
                        "OK",
                        CORS_HEADERS,
                        body.into_bytes(),
                    )),
                    Err(msg) => Ok(api_to_out(ApiResponse::err_500(&msg))),
                },
                None => Ok(api_to_out(ApiResponse::err_400(
                    "missing chat_id query param",
                ))),
            }
        }
        ("GET", "/api/memory/status") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            let body = handlers::memory::body(ctx, uri)
                .map_err(|error| err_other("http_router_dispatch", error))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("POST", "/api/memory/maintenance") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::memory_maintenance::post(ctx, body_str);
            Ok(api_to_out(r))
        }
        ("GET", "/api/capability_packages") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            let body = handlers::capability_packages::get(ctx)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("POST", "/api/capability_packages") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::capability_packages::post(ctx, body_str);
            Ok(api_to_out(r))
        }
        ("GET", "/api/skills") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            let name = common::name_from_uri(uri);
            match handlers::skills::get(ctx, name) {
                Ok(handlers::skills::SkillsGetResult::TextPlain(s)) => Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_AND_TEXT_PLAIN,
                    s.into_bytes(),
                )),
                Ok(handlers::skills::SkillsGetResult::Json(s)) => Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_HEADERS,
                    s.into_bytes(),
                )),
                Err(r) => Ok(api_to_out(r)),
            }
        }
        ("POST", "/api/skills") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::skills::post(ctx, body_str);
            Ok(api_to_out(r))
        }
        ("DELETE", "/api/skills") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let name = match common::name_from_uri(uri) {
                Some(n) => n,
                None => {
                    let loc = locale_from_store(store);
                    let msg = tr(Message::MissingNameQuery, loc);
                    return Ok(api_to_out(ApiResponse::err_400(&msg)));
                }
            };
            let r = handlers::skills::delete(ctx, &name);
            Ok(api_to_out(r))
        }
        ("POST", "/api/skills/import") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::skills::import(ctx, body_str)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(api_to_out(r))
        }
        ("GET", "/api/soul") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            match handlers::soul::get_body(ctx) {
                Ok(content) => Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_AND_TEXT_PLAIN,
                    content.into_bytes(),
                )),
                Err(_) => {
                    let loc = locale_from_store(store);
                    let msg = tr(Message::OperationFailed, loc);
                    Ok(api_to_out(ApiResponse::err_500(&msg)))
                }
            }
        }
        ("GET", "/api/user") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            match handlers::user::get_body(ctx) {
                Ok(content) => Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_AND_TEXT_PLAIN,
                    content.into_bytes(),
                )),
                Err(_) => {
                    let loc = locale_from_store(store);
                    let msg = tr(Message::OperationFailed, loc);
                    Ok(api_to_out(ApiResponse::err_500(&msg)))
                }
            }
        }
        ("POST", "/api/soul") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let is_json = incoming
                .header_ci("Content-Type")
                .map(|ct| ct.starts_with("application/json"))
                .unwrap_or(false);
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::soul::post(ctx, body_str.to_string(), is_json);
            Ok(api_to_out(r))
        }
        ("POST", "/api/user") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let is_json = incoming
                .header_ci("Content-Type")
                .map(|ct| ct.starts_with("application/json"))
                .unwrap_or(false);
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::user::post(ctx, body_str.to_string(), is_json);
            Ok(api_to_out(r))
        }
        ("POST", "/api/restart") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let (r, do_restart) =
                handlers::restart::post(ctx).map_err(|e| err_other("http_router_dispatch", e))?;
            let mut out = api_to_out(r);
            if do_restart {
                out.restart = RestartAction::After300Ms;
            }
            Ok(out)
        }
        ("POST", "/api/config_reset") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let r = handlers::config_reset::post(ctx)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(api_to_out(r))
        }
        ("POST", "/api/webhook") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = utf8_body(&incoming.body)?;
            let token = incoming
                .header_ci("X-Webhook-Token")
                .or_else(|| common::token_from_uri(uri));
            let provided = token.unwrap_or("");
            let r = handlers::webhook::post(ctx, &env.inbound_tx, body_str.to_string(), provided)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(api_to_out(r))
        }
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        ("POST", "/api/feishu/event") => {
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::feishu_event::post(ctx, &env.inbound_tx, body_str)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(api_to_out(r))
        }
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        ("POST", "/api/dingtalk/webhook") => {
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::dingtalk_webhook::post(&env.inbound_tx, body_str)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(api_to_out(r))
        }
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        ("GET", "/api/wecom/webhook") => {
            let config = ctx.config();
            let r = handlers::wecom_webhook::get_verify(uri, &config.wecom_token);
            Ok(OutgoingResponse::json(
                r.status,
                r.status_text,
                CORS_HEADERS,
                r.body.to_vec(),
            ))
        }
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        ("POST", "/api/wecom/webhook") => {
            let body_str = utf8_body(&incoming.body)?;
            let r = handlers::wecom_webhook::post(ctx, uri, &env.inbound_tx, body_str)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(api_to_out(r))
        }
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        ("POST", "/api/webhook/qq") => {
            if !env.qq_webhook_enabled {
                return Ok(OutgoingResponse::json(
                    404,
                    "Not Found",
                    CORS_HEADERS,
                    br#"{"error":"not found"}"#.to_vec(),
                ));
            }
            let ts = incoming.header_ci("X-Signature-Timestamp");
            let sig = incoming.header_ci("X-Signature-Ed25519");
            match handlers::qq_webhook::post(
                store,
                &incoming.body,
                ts,
                sig,
                &env.qq_app_id,
                &env.qq_secret,
                &env.inbound_tx,
                Arc::clone(&env.qq_msg_id_cache),
            ) {
                Ok(handlers::qq_webhook::QqWebhookOutcome::UrlVerification {
                    plain_token,
                    signature,
                }) => {
                    let body = serde_json::json!({
                        "plain_token": plain_token,
                        "signature": signature
                    });
                    Ok(OutgoingResponse::json(
                        200,
                        "OK",
                        CORS_HEADERS,
                        body.to_string().into_bytes(),
                    ))
                }
                Ok(handlers::qq_webhook::QqWebhookOutcome::EventHandled) => {
                    Ok(OutgoingResponse::json(200, "OK", CORS_HEADERS, Vec::new()))
                }
                Err(r) => Ok(api_to_out(r)),
            }
        }
        _ => {
            #[cfg(feature = "ota")]
            {
                if let Some(o) = dispatch_ota(ctx, store, method, path, uri, &incoming)? {
                    return Ok(o);
                }
            }
            Ok(OutgoingResponse::json(
                404,
                "Not Found",
                CORS_HEADERS,
                br#"{"error":"not found"}"#.to_vec(),
            ))
        }
    }
}

#[cfg(feature = "ota")]
fn dispatch_ota(
    ctx: &HandlerContext,
    store: &dyn crate::platform::ConfigStore,
    method: &str,
    path: &str,
    uri: &str,
    incoming: &IncomingRequest,
) -> Result<Option<OutgoingResponse>> {
    use crate::platform::http_server::common::channel_from_uri;
    match (method, path) {
        ("GET", "/api/ota/check") => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(Some(api_to_out(r)));
            }
            let channel = channel_from_uri(uri);
            let body = crate::platform::http_server::handlers::ota::get_check(ctx, &channel)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(Some(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            )))
        }
        ("POST", "/api/ota") => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(Some(o));
            }
            let body_str = utf8_body(&incoming.body)?;
            let (r, do_restart) = crate::platform::http_server::handlers::ota::post(ctx, body_str)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            let mut out = api_to_out(r);
            if do_restart {
                out.restart = RestartAction::After300Ms;
            }
            Ok(Some(out))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::dispatch;
    use crate::bus::new_inbound_channel;
    use crate::platform::http_server::handlers::{
        build_default_test_handler_context, HandlerContext,
    };
    use crate::platform::http_server::router::{IncomingRequest, RouterEnv};
    use crate::runtime::{OperatorMaintenanceAction, OperatorMaintenanceRequest};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn build_router_env() -> RouterEnv {
        let (inbound_tx, _inbound_rx, _inbound_depth) =
            new_inbound_channel(crate::constants::DEFAULT_CAPACITY);
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        {
            let qq_msg_id_cache = Arc::new(Mutex::new(HashMap::new()));
            RouterEnv::new(
                inbound_tx,
                qq_msg_id_cache,
                false,
                String::new(),
                String::new(),
            )
        }
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        {
            RouterEnv::new(inbound_tx)
        }
    }

    fn build_authed_ctx() -> HandlerContext {
        let ctx = build_default_test_handler_context();
        crate::platform::pairing::set_code(ctx.config_store.as_ref(), "123456")
            .expect("set pairing code");
        crate::platform::csrf::init().expect("init csrf");
        ctx
    }

    #[test]
    fn operator_maintenance_route_accepts_structured_runtime_request() {
        let (system_inbound_tx, system_inbound_rx, _system_inbound_depth) =
            new_inbound_channel(crate::constants::DEFAULT_CAPACITY);
        let mut ctx = build_authed_ctx();
        ctx.system_inbound_tx = Some(system_inbound_tx);
        let env = build_router_env();
        let csrf = crate::platform::csrf::get_token().expect("csrf token");
        let request = IncomingRequest {
            method: "POST".to_string(),
            uri: "/api/memory/maintenance".to_string(),
            headers: vec![
                ("X-Pairing-Code".to_string(), "123456".to_string()),
                ("X-CSRF-Token".to_string(), csrf),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body: br#"{"action":"run_repair_plan"}"#.to_vec(),
        };

        let response = dispatch(&ctx, &env, request).expect("dispatch maintenance route");
        assert_eq!(response.status, 202);

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["accepted"], true);
        assert_eq!(parsed["action"], "run_repair_plan");
        assert_eq!(parsed["delivery"], "in_memory");

        let queued = system_inbound_rx
            .try_recv()
            .expect("queued maintenance request");
        assert_eq!(
            queued.channel.as_ref(),
            crate::runtime::CHANNEL_OPERATOR_MAINTENANCE
        );
        let request: OperatorMaintenanceRequest =
            serde_json::from_str(&queued.content).expect("decode queued request");
        assert_eq!(request.action, OperatorMaintenanceAction::RunRepairPlan);
    }
}
