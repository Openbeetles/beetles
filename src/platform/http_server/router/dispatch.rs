//! 唯一路由分发：与 ESP `mod.rs` 中 `register!` 行为等价。
//! Single route dispatch; behavior matches ESP `register!` in `mod.rs`.

use super::auth;
use super::catalog::{
    self, OperatorRouteAccess, RouteBodyMode, RouteExecutionClass, ROUTE_CAPABILITY_PACKAGES,
    ROUTE_CHANNEL_CONNECTIVITY, ROUTE_CHANNEL_CONNECTIVITY_REFRESH, ROUTE_CONFIG_AUDIO,
    ROUTE_CONFIG_CHANNELS, ROUTE_CONFIG_DISPLAY, ROUTE_CONFIG_HARDWARE, ROUTE_CONFIG_LLM,
    ROUTE_CONFIG_RESET, ROUTE_CONFIG_SYSTEM, ROUTE_CSRF_TOKEN, ROUTE_DIAGNOSE,
    ROUTE_HARDWARE_DISCOVERY, ROUTE_HEALTH, ROUTE_MEMORY_MAINTENANCE, ROUTE_MEMORY_STATUS,
    ROUTE_METRICS, ROUTE_OPERATOR_STATUS, ROUTE_OPERATOR_WINDOW, ROUTE_PAIRING_CODE,
    ROUTE_RESOURCE, ROUTE_RESTART, ROUTE_ROOT, ROUTE_SESSIONS, ROUTE_SKILLS, ROUTE_SKILLS_IMPORT,
    ROUTE_SYSTEM_INFO, ROUTE_TOOLS, ROUTE_WEBHOOK, ROUTE_WIFI_SCAN,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use super::catalog::{
    ROUTE_CONFIG_ACCOUNTS, ROUTE_CONFIG_ACCOUNTS_PREFIX, ROUTE_CONFIG_CAPABILITIES,
    ROUTE_CONFIG_CAPABILITIES_PREFIX, ROUTE_CONFIG_PROVIDERS,
};
#[cfg(feature = "ota")]
use super::catalog::{ROUTE_OTA, ROUTE_OTA_CHECK};
use super::types::{IncomingRequest, OutgoingResponse, RestartAction, RouterEnv};
use crate::error::{Error, Result};
use crate::platform::http_server::api_contract;
use crate::platform::http_server::common::{
    self, ApiResponse, CORS_AND_TEXT_PLAIN, CORS_HEADERS, CORS_OPTIONS_HEADERS,
};
use crate::platform::http_server::handlers::{self, HandlerContext};
use crate::platform::{HardwareCapability, HardwareDiscoveryBus};

const OPTIONS_BODY: &[u8] = b" ";

#[inline(never)]
fn path_only(uri: &str) -> &str {
    uri.split('?').next().unwrap_or(ROUTE_ROOT)
}

/// Extract `chat_id` query parameter from URI (case-insensitive key match).
#[inline(never)]
fn chat_id_from_uri(uri: &str) -> Option<String> {
    common::query_param_from_uri(uri, "chat_id").map(str::to_string)
}

/// Extract pagination parameters from URI: page (default 1) and limit (default 20).
#[inline(never)]
fn pagination_from_uri(uri: &str) -> (usize, usize) {
    let page = common::query_param_from_uri(uri, "page")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let limit = common::query_param_from_uri(uri, "limit")
        .and_then(|value| value.parse().ok())
        .unwrap_or(20);
    (page, limit)
}

/// Extract format parameter from URI (e.g., ?format=prometheus).
#[inline(never)]
fn format_from_uri(uri: &str) -> Option<&str> {
    common::query_param_from_uri(uri, "format")
}

#[inline(never)]
fn hardware_bus_from_uri(uri: &str) -> Option<HardwareDiscoveryBus> {
    common::query_param_from_uri(uri, "bus").and_then(HardwareDiscoveryBus::parse)
}

#[inline(never)]
fn hardware_capability_from_uri(uri: &str) -> Option<HardwareCapability> {
    common::query_param_from_uri(uri, "capability").and_then(HardwareCapability::parse)
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
#[derive(Clone, Debug, PartialEq, Eq)]
enum AccountConfigRoute<'a> {
    Collection,
    Detail(&'a str),
    Config(&'a str),
    Probe(&'a str),
    Revoke(&'a str),
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
#[derive(Clone, Debug, PartialEq, Eq)]
enum CapabilityConfigRoute<'a> {
    Collection,
    Detail(&'a str),
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
#[derive(Clone, Debug, PartialEq, Eq)]
enum ProviderConfigRoute {
    Collection,
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn parse_account_config_route(path: &str) -> Option<AccountConfigRoute<'_>> {
    if path == ROUTE_CONFIG_ACCOUNTS {
        return Some(AccountConfigRoute::Collection);
    }
    let rest = path.strip_prefix(ROUTE_CONFIG_ACCOUNTS_PREFIX)?;
    let mut parts = rest.split('/');
    let account_key = parts.next()?.trim();
    if account_key.is_empty() {
        return None;
    }
    match (parts.next(), parts.next()) {
        (None, None) => Some(AccountConfigRoute::Detail(account_key)),
        (Some("config"), None) => Some(AccountConfigRoute::Config(account_key)),
        (Some("probe"), None) => Some(AccountConfigRoute::Probe(account_key)),
        (Some("revoke"), None) => Some(AccountConfigRoute::Revoke(account_key)),
        _ => None,
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn parse_capability_config_route(path: &str) -> Option<CapabilityConfigRoute<'_>> {
    if path == ROUTE_CONFIG_CAPABILITIES {
        return Some(CapabilityConfigRoute::Collection);
    }
    let capability = path.strip_prefix(ROUTE_CONFIG_CAPABILITIES_PREFIX)?.trim();
    if capability.is_empty() {
        return None;
    }
    Some(CapabilityConfigRoute::Detail(capability))
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn parse_provider_config_route(path: &str) -> Option<ProviderConfigRoute> {
    if path == ROUTE_CONFIG_PROVIDERS {
        return Some(ProviderConfigRoute::Collection);
    }
    None
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
        stage: "http_body_utf8",
    })
}

fn read_route_body(body: &[u8], body_mode: RouteBodyMode) -> Result<&str> {
    match body_mode {
        RouteBodyMode::None => Ok(""),
        RouteBodyMode::Utf8(_) => utf8_body(body),
    }
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

/// 读操作鉴权：仅要求配对码；命中则返回已组装的 JSON 响应。
#[inline(never)]
fn guard_pairing(
    store: &dyn crate::platform::ConfigStore,
    uri: &str,
    headers: &[(String, String)],
) -> Option<OutgoingResponse> {
    auth::require_pairing_code(store, uri, headers).map(api_to_out)
}

fn err_other(stage: &'static str, msg: impl std::fmt::Display) -> Error {
    Error::Other {
        source: Box::new(std::io::Error::other(msg.to_string())),
        stage,
    }
}

fn json_ok(body: String) -> OutgoingResponse {
    OutgoingResponse::json(200, "OK", CORS_HEADERS, body.into_bytes())
}

fn rejected_route_response(path: &str) -> OutgoingResponse {
    let mut extra = serde_json::Map::new();
    extra.insert(
        "path".to_string(),
        serde_json::Value::String(path.to_string()),
    );
    api_to_out(ApiResponse::err_key_with_meta(
        410,
        "Gone",
        "http.route_removed",
        Some("http_router_dispatch"),
        Some("route is not available in this runtime"),
        None,
        None,
        extra,
    ))
}

fn apply_restart_action(
    mut response: OutgoingResponse,
    uri: &str,
    enabled: bool,
) -> OutgoingResponse {
    if enabled && response.status == 200 && common::restart_requested_from_uri(uri) {
        response.restart = RestartAction::After300Ms;
    }
    response
}

fn dispatch_guarded_route<F>(
    guard: Option<OutgoingResponse>,
    build_response: F,
) -> Result<OutgoingResponse>
where
    F: FnOnce() -> Result<OutgoingResponse>,
{
    if let Some(response) = guard {
        return Ok(response);
    }
    build_response()
}

fn dispatch_api_body_route<F>(
    guard: Option<OutgoingResponse>,
    body: &[u8],
    uri: &str,
    restart_on_success: bool,
    body_mode: RouteBodyMode,
    execute: F,
) -> Result<OutgoingResponse>
where
    F: FnOnce(&str) -> Result<ApiResponse>,
{
    if let Some(response) = guard {
        return Ok(response);
    }
    let body_str = read_route_body(body, body_mode)?;
    let response = api_to_out(execute(body_str)?);
    Ok(apply_restart_action(response, uri, restart_on_success))
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn dispatch_account_config(
    ctx: &HandlerContext,
    store: &dyn crate::platform::ConfigStore,
    method: &str,
    path: &str,
    uri: &str,
    incoming: &IncomingRequest,
) -> Result<Option<OutgoingResponse>> {
    let Some(route) = parse_account_config_route(path) else {
        return Ok(None);
    };
    let decoded_key = |value: &str| crate::util::percent_decode_query(value);
    let response = match (method, route) {
        ("GET", AccountConfigRoute::Collection) => Some(dispatch_guarded_route(
            guard_pairing(store, uri, &incoming.headers),
            || {
                Ok(
                    match handlers::config::get_accounts_body(
                        ctx,
                        common::query_param_from_uri(uri, "provider_kind"),
                        common::query_param_from_uri(uri, "capability"),
                    ) {
                        Ok(body) => json_ok(body),
                        Err(_) => api_to_out(ApiResponse::err_400_key(
                            api_contract::COMMON_OPERATION_FAILED,
                        )),
                    },
                )
            },
        )?),
        ("POST", AccountConfigRoute::Collection) => Some(dispatch_api_body_route(
            guard_pairing_csrf(store, uri, &incoming.headers),
            &incoming.body,
            uri,
            false,
            RouteBodyMode::Utf8(common::POST_BODY_MAX_LEN),
            |body_str| {
                handlers::config::post_accounts(ctx, body_str)
                    .map_err(|e| err_other("http_router_dispatch", e))
            },
        )?),
        ("GET", AccountConfigRoute::Detail(account_key)) => Some(dispatch_guarded_route(
            guard_pairing(store, uri, &incoming.headers),
            || {
                Ok(
                    match handlers::config::get_account_detail_body(ctx, &decoded_key(account_key))
                    {
                        Ok(body) => json_ok(body),
                        Err(_) => api_to_out(ApiResponse::err_400_key(
                            api_contract::COMMON_OPERATION_FAILED,
                        )),
                    },
                )
            },
        )?),
        ("DELETE", AccountConfigRoute::Detail(account_key)) => {
            let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) else {
                let r = handlers::config::delete_account(ctx, &decoded_key(account_key))
                    .map_err(|e| err_other("http_router_dispatch", e))?;
                let out = api_to_out(r);
                return Ok(Some(out));
            };
            return Ok(Some(o));
        }
        ("POST", AccountConfigRoute::Config(account_key)) => Some(dispatch_api_body_route(
            guard_pairing_csrf(store, uri, &incoming.headers),
            &incoming.body,
            uri,
            false,
            RouteBodyMode::Utf8(common::POST_BODY_MAX_LEN),
            |body_str| {
                handlers::config::post_account_config(ctx, &decoded_key(account_key), body_str)
                    .map_err(|e| err_other("http_router_dispatch", e))
            },
        )?),
        ("POST", AccountConfigRoute::Probe(account_key)) => {
            let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) else {
                let r = handlers::config::post_account_probe(ctx, &decoded_key(account_key))
                    .map_err(|e| err_other("http_router_dispatch", e))?;
                let out = api_to_out(r);
                return Ok(Some(out));
            };
            return Ok(Some(o));
        }
        ("POST", AccountConfigRoute::Revoke(account_key)) => Some(dispatch_api_body_route(
            guard_pairing_csrf(store, uri, &incoming.headers),
            &incoming.body,
            uri,
            false,
            RouteBodyMode::Utf8(common::POST_BODY_MAX_LEN),
            |body_str| {
                handlers::config::post_account_revoke(ctx, &decoded_key(account_key), body_str)
                    .map_err(|e| err_other("http_router_dispatch", e))
            },
        )?),
        _ => None,
    };
    Ok(response)
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn dispatch_capability_config(
    ctx: &HandlerContext,
    store: &dyn crate::platform::ConfigStore,
    method: &str,
    path: &str,
    uri: &str,
    incoming: &IncomingRequest,
) -> Result<Option<OutgoingResponse>> {
    let Some(route) = parse_capability_config_route(path) else {
        return Ok(None);
    };
    let response = match (method, route) {
        ("GET", CapabilityConfigRoute::Collection) => Some(dispatch_guarded_route(
            guard_pairing(store, uri, &incoming.headers),
            || {
                Ok(match handlers::config::get_capabilities_body(ctx) {
                    Ok(body) => json_ok(body),
                    Err(_) => api_to_out(ApiResponse::err_400_key(
                        api_contract::COMMON_OPERATION_FAILED,
                    )),
                })
            },
        )?),
        ("GET", CapabilityConfigRoute::Detail(capability)) => Some(dispatch_guarded_route(
            guard_pairing(store, uri, &incoming.headers),
            || {
                Ok(
                    match handlers::config::get_capability_detail_body(ctx, capability) {
                        Ok(body) => json_ok(body),
                        Err(error) => api_to_out(office_config_error_response(&error)),
                    },
                )
            },
        )?),
        _ => None,
    };
    Ok(response)
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn dispatch_provider_config(
    ctx: &HandlerContext,
    store: &dyn crate::platform::ConfigStore,
    method: &str,
    path: &str,
    uri: &str,
    incoming: &IncomingRequest,
) -> Result<Option<OutgoingResponse>> {
    let Some(route) = parse_provider_config_route(path) else {
        return Ok(None);
    };
    let response = match (method, route) {
        ("GET", ProviderConfigRoute::Collection) => Some(dispatch_guarded_route(
            guard_pairing(store, uri, &incoming.headers),
            || {
                Ok(
                    match handlers::config::get_providers_body(
                        ctx,
                        common::query_param_from_uri(uri, "capability"),
                    ) {
                        Ok(body) => json_ok(body),
                        Err(error) => api_to_out(office_config_error_response(&error)),
                    },
                )
            },
        )?),
        _ => None,
    };
    Ok(response)
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn office_config_error_response(error: &Error) -> ApiResponse {
    let error_key = api_contract::error_key(error);
    if error_key == api_contract::OFFICE_CAPABILITY_INVALID {
        ApiResponse::err_400_key(error_key)
    } else {
        ApiResponse::err_500_key(error_key)
    }
}

fn operator_window_required_response(path: &str) -> OutgoingResponse {
    let mut extra = serde_json::Map::new();
    extra.insert("path".to_string(), serde_json::json!(path));
    extra.insert(
        "open_endpoint".to_string(),
        serde_json::json!("POST /api/operator/window"),
    );
    let body = ApiResponse::err_key_with_meta(
        403,
        "Forbidden",
        "system.operator_window_required",
        None,
        None,
        None,
        None,
        extra,
    );
    OutgoingResponse::json(body.status, body.status_text, CORS_HEADERS, body.body)
}

/// 配置 API 唯一入口：ESP / Linux 在组装 `IncomingRequest` 后调用。
#[inline(never)]
pub fn dispatch(
    ctx: &HandlerContext,
    env: &RouterEnv,
    incoming: IncomingRequest,
) -> Result<OutgoingResponse> {
    dispatch_impl(ctx, Some(env), incoming)
}

/// Dispatch a worker route that is known not to need inbound webhook state.
///
/// ESP worker lanes use this entry point so worker startup does not clone the
/// inbound bus sender. The system-owned `/api/webhook` route stays on the
/// immediate lane, where `RouterEnv` is still available.
#[inline(never)]
#[cfg_attr(
    not(any(target_arch = "xtensa", target_arch = "riscv32")),
    allow(dead_code)
)]
pub fn dispatch_without_inbound(
    ctx: &HandlerContext,
    incoming: IncomingRequest,
) -> Result<OutgoingResponse> {
    dispatch_impl(ctx, None, incoming)
}

#[inline(never)]
fn dispatch_impl(
    ctx: &HandlerContext,
    env: Option<&RouterEnv>,
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
    let route_spec = catalog::route_spec_for(method, path);
    let route_body_mode = route_spec.map_or(RouteBodyMode::None, |spec| spec.body_mode);
    let route_operator_access =
        route_spec.map_or(OperatorRouteAccess::Hidden, |spec| spec.operator_access);
    if route_spec.is_some_and(|spec| spec.execution_class == RouteExecutionClass::RejectedRoute) {
        return Ok(rejected_route_response(path));
    }
    if route_operator_access == OperatorRouteAccess::Windowed
        && crate::platform::operator_surface::current_operator_surface_budget(memory_system_kind)
            .window_required_for_deep_routes
    {
        return Ok(operator_window_required_response(path));
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    if let Some(response) = dispatch_provider_config(ctx, store, method, path, uri, &incoming)? {
        return Ok(response);
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    if let Some(response) = dispatch_account_config(ctx, store, method, path, uri, &incoming)? {
        return Ok(response);
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    if let Some(response) = dispatch_capability_config(ctx, store, method, path, uri, &incoming)? {
        return Ok(response);
    }

    match (method, path) {
        ("GET", ROUTE_ROOT) => {
            let body =
                handlers::root::body(ctx).map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", ROUTE_PAIRING_CODE) => {
            let body = handlers::pairing::body(ctx);
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("POST", ROUTE_PAIRING_CODE) => {
            let body_str = read_route_body(&incoming.body, route_body_mode)?;
            let r = handlers::pairing::post_body(ctx, body_str);
            Ok(api_to_out(r))
        }
        ("POST", ROUTE_CONFIG_LLM) => dispatch_api_body_route(
            guard_pairing_csrf(store, uri, &incoming.headers),
            &incoming.body,
            uri,
            false,
            route_body_mode,
            |body_str| {
                handlers::config::post_llm(ctx, body_str)
                    .map_err(|e| err_other("http_router_dispatch", e))
            },
        ),
        ("GET", ROUTE_CONFIG_LLM) => {
            dispatch_guarded_route(guard_pairing(store, uri, &incoming.headers), || {
                let body = handlers::config::get_llm_body(ctx)
                    .map_err(|e| err_other("http_router_dispatch", e))?;
                Ok(json_ok(body))
            })
        }
        ("GET", ROUTE_CONFIG_CHANNELS) => {
            dispatch_guarded_route(guard_pairing(store, uri, &incoming.headers), || {
                let body = handlers::config::get_channels_body(ctx)
                    .map_err(|e| err_other("http_router_dispatch", e))?;
                Ok(json_ok(body))
            })
        }
        ("POST", ROUTE_CONFIG_CHANNELS) => dispatch_api_body_route(
            guard_pairing_csrf(store, uri, &incoming.headers),
            &incoming.body,
            uri,
            false,
            route_body_mode,
            |body_str| {
                handlers::config::post_channels(ctx, body_str)
                    .map_err(|e| err_other("http_router_dispatch", e))
            },
        ),
        ("GET", ROUTE_CONFIG_SYSTEM) => {
            dispatch_guarded_route(guard_pairing(store, uri, &incoming.headers), || {
                let body = handlers::config::get_system_body(ctx)
                    .map_err(|e| err_other("http_router_dispatch", e))?;
                Ok(json_ok(body))
            })
        }
        ("POST", ROUTE_CONFIG_SYSTEM) => dispatch_api_body_route(
            guard_pairing_csrf(store, uri, &incoming.headers),
            &incoming.body,
            uri,
            false,
            route_body_mode,
            |body_str| {
                handlers::config::post_system(ctx, body_str)
                    .map_err(|e| err_other("http_router_dispatch", e))
            },
        ),
        ("GET", ROUTE_CONFIG_HARDWARE) => {
            dispatch_guarded_route(guard_pairing(store, uri, &incoming.headers), || {
                let body = handlers::config::get_hardware_body(ctx)
                    .map_err(|e| err_other("http_router_dispatch", e))?;
                Ok(json_ok(body))
            })
        }
        ("POST", ROUTE_CONFIG_HARDWARE) => dispatch_api_body_route(
            guard_pairing_csrf(store, uri, &incoming.headers),
            &incoming.body,
            uri,
            false,
            route_body_mode,
            |body_str| {
                handlers::config::post_hardware(ctx, body_str)
                    .map_err(|e| err_other("http_router_dispatch", e))
            },
        ),
        ("GET", ROUTE_CONFIG_AUDIO) => {
            dispatch_guarded_route(guard_pairing(store, uri, &incoming.headers), || {
                let body = handlers::config::get_audio_body(ctx)
                    .map_err(|e| err_other("http_router_dispatch", e))?;
                Ok(json_ok(body))
            })
        }
        ("POST", ROUTE_CONFIG_AUDIO) => dispatch_api_body_route(
            guard_pairing_csrf(store, uri, &incoming.headers),
            &incoming.body,
            uri,
            true,
            route_body_mode,
            |body_str| {
                handlers::config::post_audio(ctx, body_str)
                    .map_err(|e| err_other("http_router_dispatch", e))
            },
        ),
        ("GET", ROUTE_CONFIG_DISPLAY) => {
            dispatch_guarded_route(guard_pairing(store, uri, &incoming.headers), || {
                let body = handlers::config::get_display_body(ctx)
                    .map_err(|e| err_other("http_router_dispatch", e))?;
                Ok(json_ok(body))
            })
        }
        ("POST", ROUTE_CONFIG_DISPLAY) => dispatch_api_body_route(
            guard_pairing_csrf(store, uri, &incoming.headers),
            &incoming.body,
            uri,
            true,
            route_body_mode,
            |body_str| {
                handlers::config::post_display(ctx, body_str)
                    .map_err(|e| err_other("http_router_dispatch", e))
            },
        ),
        ("GET", ROUTE_WIFI_SCAN) => match handlers::wifi_scan::get_body(ctx) {
            Ok(body) => Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            )),
            Err(handlers::wifi_scan::WifiScanError::Unavailable) => Ok(api_to_out(
                ApiResponse::err_503_key("network.wifi_scan_unavailable"),
            )),
            Err(handlers::wifi_scan::WifiScanError::Other(error)) => {
                Ok(api_to_out(ApiResponse::err_500_key_with_upstream(
                    api_contract::COMMON_OPERATION_FAILED,
                    Some(&error.to_string()),
                    error.http_status_code(),
                )))
            }
        },
        ("GET", ROUTE_HARDWARE_DISCOVERY) => {
            if let Some(r) = auth::require_pairing_code(store, uri, &incoming.headers) {
                return Ok(api_to_out(r));
            }
            let Some(bus) = hardware_bus_from_uri(uri) else {
                return Ok(api_to_out(ApiResponse::err_400_key(
                    "hardware.discovery_invalid_bus",
                )));
            };
            let Some(capability) = hardware_capability_from_uri(uri) else {
                return Ok(api_to_out(ApiResponse::err_400_key(
                    "hardware.discovery_invalid_capability",
                )));
            };
            match handlers::hardware_discovery::get_body(ctx, bus, capability) {
                Ok(body) => Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_HEADERS,
                    body.into_bytes(),
                )),
                Err(handlers::hardware_discovery::HardwareDiscoveryError::Unavailable) => Ok(
                    api_to_out(ApiResponse::err_503_key("hardware.discovery_unavailable")),
                ),
                Err(handlers::hardware_discovery::HardwareDiscoveryError::Other(error)) => {
                    Ok(api_to_out(ApiResponse::err_500_key_with_upstream(
                        api_contract::COMMON_OPERATION_FAILED,
                        Some(&error.to_string()),
                        error.http_status_code(),
                    )))
                }
            }
        }
        ("GET", ROUTE_HEALTH) => {
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
                Err(_) => Ok(api_to_out(ApiResponse::err_500_key(
                    "common.operation_failed",
                ))),
            }
        }
        ("GET", ROUTE_OPERATOR_STATUS) => {
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
        ("GET", ROUTE_METRICS) => {
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
        ("GET", ROUTE_RESOURCE) => {
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
        ("GET", ROUTE_TOOLS) => {
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
        ("GET", ROUTE_CSRF_TOKEN) => {
            let body = handlers::csrf_token::body(ctx)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(OutgoingResponse::json(
                200,
                "OK",
                CORS_HEADERS,
                body.into_bytes(),
            ))
        }
        ("GET", ROUTE_DIAGNOSE) => {
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
        ("GET", ROUTE_SYSTEM_INFO) => {
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
        ("POST", ROUTE_OPERATOR_WINDOW) => {
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
        ("GET", ROUTE_CHANNEL_CONNECTIVITY) => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            match handlers::channel_connectivity::body(ctx, false) {
                Ok(body) => Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_HEADERS,
                    body.into_bytes(),
                )),
                Err(msg) => Ok(api_to_out(ApiResponse::err_key_with_meta(
                    500,
                    "Internal Server Error",
                    "channel.snapshot_failed",
                    Some("channel_connectivity"),
                    Some(msg.as_str()),
                    None,
                    None,
                    serde_json::Map::new(),
                ))),
            }
        }
        ("POST", ROUTE_CHANNEL_CONNECTIVITY_REFRESH) => {
            if let Some(r) = auth::require_activated(store) {
                return Ok(api_to_out(r));
            }
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            match handlers::channel_connectivity::body(ctx, true) {
                Ok(body) => Ok(OutgoingResponse::json(
                    200,
                    "OK",
                    CORS_HEADERS,
                    body.into_bytes(),
                )),
                Err(msg) => Ok(api_to_out(ApiResponse::err_key_with_meta(
                    500,
                    "Internal Server Error",
                    "channel.snapshot_failed",
                    Some("channel_connectivity"),
                    Some(msg.as_str()),
                    None,
                    None,
                    serde_json::Map::new(),
                ))),
            }
        }
        ("GET", ROUTE_SESSIONS) => {
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
                Err(_) => Ok(api_to_out(ApiResponse::err_key_with_meta(
                    500,
                    "Internal Server Error",
                    api_contract::COMMON_OPERATION_FAILED,
                    Some("sessions"),
                    None,
                    None,
                    None,
                    serde_json::Map::new(),
                ))),
            }
        }
        ("DELETE", ROUTE_SESSIONS) => {
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
        ("GET", ROUTE_MEMORY_STATUS) => {
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
        ("POST", ROUTE_MEMORY_MAINTENANCE) => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = read_route_body(&incoming.body, route_body_mode)?;
            let r = handlers::memory_maintenance::post(ctx, body_str);
            Ok(api_to_out(r))
        }
        ("GET", ROUTE_CAPABILITY_PACKAGES) => {
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
        ("POST", ROUTE_CAPABILITY_PACKAGES) => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = read_route_body(&incoming.body, route_body_mode)?;
            let r = handlers::capability_packages::post(ctx, body_str);
            Ok(api_to_out(r))
        }
        ("GET", ROUTE_SKILLS) => {
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
        ("POST", ROUTE_SKILLS) => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = read_route_body(&incoming.body, route_body_mode)?;
            let r = handlers::skills::post(ctx, body_str);
            Ok(api_to_out(r))
        }
        ("DELETE", ROUTE_SKILLS) => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let name = match common::name_from_uri(uri) {
                Some(n) => n,
                None => {
                    return Ok(api_to_out(ApiResponse::err_400_key(
                        "skill.name_query_required",
                    )));
                }
            };
            let r = handlers::skills::delete(ctx, &name);
            Ok(api_to_out(r))
        }
        ("POST", ROUTE_SKILLS_IMPORT) => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let body_str = read_route_body(&incoming.body, route_body_mode)?;
            let r = handlers::skills::import(ctx, body_str)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(api_to_out(r))
        }
        ("POST", ROUTE_RESTART) => {
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
        ("POST", ROUTE_CONFIG_RESET) => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let r = handlers::config_reset::post(ctx)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(api_to_out(r))
        }
        ("POST", ROUTE_WEBHOOK) => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(o);
            }
            let Some(env) = env else {
                return Ok(OutgoingResponse::json(
                    404,
                    "Not Found",
                    CORS_HEADERS,
                    br#"{"ok":false,"error":"webhook ingress is not available on this route lane"}"#
                        .to_vec(),
                ));
            };
            let body_str = read_route_body(&incoming.body, route_body_mode)?;
            let token = incoming
                .header_ci("X-Webhook-Token")
                .or_else(|| common::token_from_uri(uri));
            let provided = token.unwrap_or("");
            let r = handlers::webhook::post(ctx, &env.inbound_tx, body_str.to_string(), provided)
                .map_err(|e| err_other("http_router_dispatch", e))?;
            Ok(api_to_out(r))
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
                br#"{"error_key":"common.not_found"}"#.to_vec(),
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
        ("GET", ROUTE_OTA_CHECK) => {
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
        ("POST", ROUTE_OTA) => {
            if let Some(o) = guard_pairing_csrf(store, uri, &incoming.headers) {
                return Ok(Some(o));
            }
            let body_str = read_route_body(
                &incoming.body,
                RouteBodyMode::Utf8(common::POST_BODY_MAX_LEN),
            )?;
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
    use crate::config;
    use crate::platform::http_server::handlers::{
        build_default_test_handler_context, default_test_handler_context_guard, HandlerContext,
    };
    use crate::platform::http_server::router::{IncomingBody, IncomingRequest, RouterEnv};
    use crate::runtime::{OperatorMaintenanceAction, OperatorMaintenanceRequest};
    use serde_json::Value;
    use std::sync::OnceLock;
    #[cfg(feature = "capability_office")]
    use std::sync::{Arc, Mutex};

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    use crate::config::OfficeAccountsSegment;
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeCapability, OfficeCredential,
        OfficeHttpClient, OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult,
    };
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    use std::sync::MutexGuard;

    fn build_router_env() -> RouterEnv {
        let (inbound_tx, _inbound_rx, _inbound_depth) =
            new_inbound_channel(crate::constants::DEFAULT_CAPACITY);
        RouterEnv::new(inbound_tx)
    }

    fn build_authed_ctx() -> HandlerContext {
        static CSRF_INIT: OnceLock<()> = OnceLock::new();
        let ctx = build_default_test_handler_context();
        crate::platform::pairing::set_code(ctx.config_store.as_ref(), "123456")
            .expect("set pairing code");
        CSRF_INIT.get_or_init(|| crate::platform::csrf::init().expect("init csrf"));
        ctx
    }

    #[test]
    fn social_channel_webhook_routes_are_not_registered() {
        let _guard = default_test_handler_context_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();
        for (method, uri) in [
            ("POST", "/api/feishu/event"),
            ("POST", "/api/dingtalk/webhook"),
            ("GET", "/api/wecom/webhook"),
            ("POST", "/api/wecom/webhook"),
            ("POST", "/api/webhook/qq"),
        ] {
            let out = dispatch(
                &ctx,
                &env,
                IncomingRequest {
                    method: method.to_string(),
                    uri: uri.to_string(),
                    headers: Vec::new(),
                    body: IncomingBody::empty(),
                },
            )
            .expect("dispatch");
            assert_eq!(out.status, 404, "{method} {uri}");
        }
    }

    #[test]
    fn custom_webhook_route_reaches_handler() {
        let _guard = default_test_handler_context_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();
        let csrf = crate::platform::csrf::get_token().expect("csrf token");
        let out = dispatch(
            &ctx,
            &env,
            IncomingRequest {
                method: "POST".to_string(),
                uri: "/api/webhook".to_string(),
                headers: vec![
                    ("X-Pairing-Code".to_string(), "123456".to_string()),
                    ("X-CSRF-Token".to_string(), csrf),
                    ("X-Webhook-Token".to_string(), "token".to_string()),
                ],
                body: IncomingBody::from_vec(br#"{"ignored":true}"#.to_vec()),
            },
        )
        .expect("dispatch");

        assert_eq!(out.status, 403);
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    fn build_office_authed_ctx(
        probe_adapters: Vec<Arc<dyn OfficeProbeAdapter + Send + Sync>>,
    ) -> HandlerContext {
        let mut ctx = build_authed_ctx();
        ctx.office_probe_adapters = Some(probe_adapters);
        ctx
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    fn office_test_guard() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[derive(Clone)]
    struct ReadyProbeAdapter {
        provider_kind: &'static str,
        reason: &'static str,
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    impl OfficeProbeAdapter for ReadyProbeAdapter {
        fn provider_kind(&self) -> &'static str {
            self.provider_kind
        }

        fn probe(
            &self,
            _http: &mut dyn OfficeHttpClient,
            account: &OfficeAccount,
            _credential: &OfficeCredential,
        ) -> crate::error::Result<OfficeProbeResult> {
            Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: true,
                disposition: OfficeProbeDisposition::Ready,
                reason: self.reason.to_string(),
            })
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[derive(Clone)]
    struct FailingProbeAdapter;

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    impl OfficeProbeAdapter for FailingProbeAdapter {
        fn provider_kind(&self) -> &'static str {
            "imap_smtp"
        }

        fn probe(
            &self,
            _http: &mut dyn OfficeHttpClient,
            _account: &OfficeAccount,
            _credential: &OfficeCredential,
        ) -> crate::error::Result<OfficeProbeResult> {
            Err(crate::error::Error::config(
                "office_probe_test",
                "imap login failed",
            ))
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    fn authed_get(uri: &str) -> IncomingRequest {
        IncomingRequest {
            method: "GET".to_string(),
            uri: uri.to_string(),
            headers: vec![("X-Pairing-Code".to_string(), "123456".to_string())],
            body: IncomingBody::empty(),
        }
    }

    fn plain_get(uri: &str) -> IncomingRequest {
        IncomingRequest {
            method: "GET".to_string(),
            uri: uri.to_string(),
            headers: Vec::new(),
            body: IncomingBody::empty(),
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    fn authed_post(uri: &str, body: serde_json::Value) -> IncomingRequest {
        let csrf = crate::platform::csrf::get_token().expect("csrf token");
        IncomingRequest {
            method: "POST".to_string(),
            uri: uri.to_string(),
            headers: vec![
                ("X-Pairing-Code".to_string(), "123456".to_string()),
                ("X-CSRF-Token".to_string(), csrf),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body: IncomingBody::from_vec(
                serde_json::to_vec(&body).expect("serialize request body"),
            ),
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    fn authed_delete(uri: &str) -> IncomingRequest {
        let csrf = crate::platform::csrf::get_token().expect("csrf token");
        IncomingRequest {
            method: "DELETE".to_string(),
            uri: uri.to_string(),
            headers: vec![
                ("X-Pairing-Code".to_string(), "123456".to_string()),
                ("X-CSRF-Token".to_string(), csrf),
            ],
            body: IncomingBody::empty(),
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    fn seed_account(ctx: &HandlerContext, account_key: &str) {
        let mut segment = OfficeAccountsSegment::default();
        segment.registry.insert(OfficeAccount {
            account_key: account_key.to_string(),
            provider_kind: "imap_smtp".to_string(),
            external_account_id: String::new(),
            account_label: "Work mail".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Mail],
        });
        let body = serde_json::to_string(&segment).expect("serialize accounts segment");
        config::save_office_accounts_segment(ctx.config_file_store.as_ref(), &body)
            .expect("save accounts");
        let credential_body = serde_json::json!({
            "items": [{
                "account_key": account_key,
                "access_token": "secret-token",
                "metadata": {
                    "mail_imap_host": "imap.example.com",
                    "mail_smtp_host": "smtp.example.com",
                    "mail_username": "alice"
                }
            }]
        });
        config::save_office_credentials_segment(
            ctx.platform.office_credential_store().as_ref(),
            &credential_body.to_string(),
        )
        .expect("save office credentials");
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    fn seed_accounts(ctx: &HandlerContext, accounts: &[OfficeAccount]) {
        let mut segment = OfficeAccountsSegment::default();
        for account in accounts {
            segment.registry.insert(account.clone());
        }
        let body = serde_json::to_string(&segment).expect("serialize accounts segment");
        config::save_office_accounts_segment(ctx.config_file_store.as_ref(), &body)
            .expect("save accounts");
    }

    #[test]
    fn llm_config_route_returns_llm_segment_without_full_app_config_payload() {
        let _guard = default_test_handler_context_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();
        let body = serde_json::json!({
            "llm_sources": [{
                "provider": "openai",
                "api_key": "segment-key",
                "model": "gpt-4o-mini",
                "api_url": "https://api.openai.com/v1",
                "max_tokens": 2048
            }],
            "llm_router_source_index": 0,
            "llm_worker_source_index": 0
        });
        config::save_llm_segment(ctx.config_file_store.as_ref(), &body.to_string())
            .expect("save llm segment");
        ctx.reload_config();

        let request = IncomingRequest {
            method: "GET".to_string(),
            uri: "/api/config/llm".to_string(),
            headers: vec![("X-Pairing-Code".to_string(), "123456".to_string())],
            body: IncomingBody::empty(),
        };

        let response = dispatch(&ctx, &env, request).expect("dispatch llm config route");
        assert_eq!(response.status, 200);

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse llm segment");
        assert_eq!(parsed["llm_sources"][0]["provider"], "openai");
        assert_eq!(parsed["llm_sources"][0]["api_key"], "segment-key");
        assert_eq!(parsed["llm_sources"][0]["model"], "gpt-4o-mini");
        assert_eq!(parsed["llm_router_source_index"], 0);
        assert_eq!(parsed["llm_worker_source_index"], 0);
        assert!(parsed.get("locale").is_none());
        assert!(parsed.get("build_package").is_none());
    }

    #[test]
    fn root_route_returns_inventory_even_before_pairing_is_initialized() {
        let _guard = default_test_handler_context_guard();
        let ctx = build_default_test_handler_context();
        let env = build_router_env();

        let response = dispatch(&ctx, &env, plain_get("/")).expect("dispatch root route");

        assert_eq!(response.status, 200);
        assert!(
            !response
                .headers
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case("location")),
            "headers={:?}",
            response.headers
        );

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse root body");
        assert_eq!(parsed["name"], "beetle");
    }

    #[test]
    fn operator_maintenance_route_accepts_structured_runtime_request() {
        let _guard = default_test_handler_context_guard();
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
            body: IncomingBody::from_vec(br#"{"action":"run_repair_plan"}"#.to_vec()),
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

    #[test]
    fn operator_window_required_response_uses_error_key_contract() {
        let response = super::operator_window_required_response("/api/tools");
        assert_eq!(response.status, 403);

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["error_key"], "system.operator_window_required");
        assert_eq!(parsed["open_endpoint"], "POST /api/operator/window");
        assert_eq!(parsed["path"], "/api/tools");
        assert!(
            parsed.get("error").is_none(),
            "body={}",
            String::from_utf8_lossy(&response.body)
        );
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_accounts_get_returns_summary_instead_of_raw_segment() {
        let _guard = office_test_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();
        seed_account(&ctx, "test-http-mail-summary");

        let response = dispatch(&ctx, &env, authed_get("/api/config/accounts"))
            .expect("dispatch accounts summary");
        assert_eq!(response.status, 200);

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert!(
            parsed["items"].is_array(),
            "summary should expose items array"
        );
        assert!(
            parsed.get("registry").is_none(),
            "raw accounts segment should not leak"
        );
        assert!(
            parsed["items"]
                .as_array()
                .expect("items array")
                .iter()
                .any(|item| item["account_key"] == "test-http-mail-summary"),
            "body={}",
            String::from_utf8_lossy(&response.body)
        );
        assert!(parsed["items"].as_array().expect("items array")[0]
            .get("provider_display_name")
            .is_none());
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_office_credentials_route_is_not_publicly_exposed() {
        let _guard = office_test_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();

        let get_response = dispatch(&ctx, &env, authed_get("/api/config/office_credentials"))
            .expect("dispatch legacy credentials get");
        assert_eq!(
            get_response.status,
            404,
            "body={}",
            String::from_utf8_lossy(&get_response.body)
        );

        let post_response = dispatch(
            &ctx,
            &env,
            authed_post(
                "/api/config/office_credentials",
                serde_json::json!({
                    "credentials": []
                }),
            ),
        )
        .expect("dispatch legacy credentials post");
        assert_eq!(
            post_response.status,
            404,
            "body={}",
            String::from_utf8_lossy(&post_response.body)
        );
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_accounts_get_supports_provider_kind_filter() {
        let _guard = office_test_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();
        seed_accounts(
            &ctx,
            &[
                OfficeAccount {
                    account_key: "mail-imap".to_string(),
                    provider_kind: "imap_smtp".to_string(),
                    external_account_id: String::new(),
                    account_label: "IMAP".to_string(),
                    identity_class: OfficeAccountIdentityClass::Work,
                    enabled_capabilities: vec![OfficeCapability::Mail],
                },
                OfficeAccount {
                    account_key: "mail-feishu".to_string(),
                    provider_kind: "feishu_mail".to_string(),
                    external_account_id: String::new(),
                    account_label: "Feishu".to_string(),
                    identity_class: OfficeAccountIdentityClass::Work,
                    enabled_capabilities: vec![OfficeCapability::Mail],
                },
            ],
        );

        let response = dispatch(
            &ctx,
            &env,
            authed_get("/api/config/accounts?provider_kind=feishu_mail"),
        )
        .expect("dispatch filtered accounts");
        assert_eq!(response.status, 200);

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        let items = parsed["items"].as_array().expect("items array");
        assert_eq!(
            items.len(),
            1,
            "body={}",
            String::from_utf8_lossy(&response.body)
        );
        assert_eq!(items[0]["account_key"], "mail-feishu");
        assert!(items[0].get("provider_display_name").is_none());
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_providers_get_returns_provider_catalog_with_create_contract() {
        let _guard = office_test_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();

        let response = dispatch(&ctx, &env, authed_get("/api/config/providers"))
            .expect("dispatch provider catalog");
        assert_eq!(response.status, 200);

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        let items = parsed["items"].as_array().expect("items array");
        let imap = items
            .iter()
            .find(|item| item["provider_kind"] == "imap_smtp")
            .expect("imap_smtp provider");
        assert!(imap.get("display_name").is_none());
        assert_eq!(imap["display_name_key"], "accounts.providers.imap_smtp");
        assert!(
            !imap["account_fields"]
                .as_array()
                .expect("account_fields array")
                .iter()
                .any(|field| field["key"] == "account_key"),
            "body={}",
            String::from_utf8_lossy(&response.body)
        );
        let access_token = imap["config_fields"]
            .as_array()
            .expect("config_fields array")
            .iter()
            .find(|field| field["key"] == "access_token")
            .expect("access_token field");
        assert_eq!(
            access_token["label_key"],
            "accounts.providerFieldLabels.access_token"
        );
        assert_eq!(
            access_token["description_key"],
            "accounts.providerFieldDescriptions.access_token"
        );
        assert!(access_token.get("label").is_none());
        assert!(access_token.get("description").is_none());
        assert!(
            imap["config_fields"]
                .as_array()
                .expect("config_fields array")
                .iter()
                .any(|field| field["key"] == "mail_imap_host"),
            "body={}",
            String::from_utf8_lossy(&response.body)
        );
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_account_detail_returns_fields_and_masks_secret_values() {
        let _guard = office_test_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();
        seed_account(&ctx, "test-http-mail-detail");

        let response = dispatch(
            &ctx,
            &env,
            authed_get("/api/config/accounts/test-http-mail-detail"),
        )
        .expect("dispatch account detail");
        assert_eq!(
            response.status,
            200,
            "body={}",
            String::from_utf8_lossy(&response.body)
        );

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["account"]["account_key"], "test-http-mail-detail");
        assert_eq!(
            parsed["account"]["display_name_key"],
            "accounts.providers.imap_smtp"
        );
        let fields = parsed["fields"].as_array().expect("fields array");
        let access_token = fields
            .iter()
            .find(|field| field["key"] == "access_token")
            .expect("access_token field");
        assert_eq!(access_token["configured"], true);
        assert!(access_token["current_value"].is_null());
        assert_eq!(
            access_token["label_key"],
            "accounts.providerFieldLabels.access_token"
        );
        assert_eq!(
            access_token["description_key"],
            "accounts.providerFieldDescriptions.access_token"
        );
        assert!(access_token.get("label").is_none());
        assert!(access_token.get("description").is_none());
        let imap_host = fields
            .iter()
            .find(|field| field["key"] == "mail_imap_host")
            .expect("mail_imap_host field");
        assert_eq!(imap_host["current_value"], "imap.example.com");
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_account_save_preserves_existing_secret_when_omitted() {
        let _guard = office_test_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();
        let account_key = "test-http-mail-save";
        seed_account(&ctx, account_key);

        let response = dispatch(
            &ctx,
            &env,
            authed_post(
                &format!("/api/config/accounts/{account_key}/config"),
                serde_json::json!({
                    "fields": {
                        "external_account_id": "mailbox-123",
                        "mail_imap_host": "imap.changed.example.com"
                    }
                }),
            ),
        )
        .expect("dispatch account config save");
        assert_eq!(response.status, 200);

        let credential = ctx
            .platform
            .office_credential_store()
            .get(account_key)
            .expect("load saved credential")
            .expect("credential exists");
        assert_eq!(credential.access_token, "secret-token");
        assert_eq!(
            credential.metadata_value("mail_imap_host"),
            Some("imap.changed.example.com")
        );

        let detail = dispatch(
            &ctx,
            &env,
            authed_get(&format!("/api/config/accounts/{account_key}")),
        )
        .expect("dispatch saved account detail");
        let parsed: Value = serde_json::from_slice(&detail.body).expect("parse detail");
        assert_eq!(parsed["account"]["external_account_id"], "mailbox-123");
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_accounts_post_upserts_single_account_registration() {
        let _guard = office_test_guard();
        let ctx = build_office_authed_ctx(vec![Arc::new(ReadyProbeAdapter {
            provider_kind: "imap_smtp",
            reason: "imap_login_ok",
        })]);
        let env = build_router_env();

        let response = dispatch(
            &ctx,
            &env,
            authed_post(
                "/api/config/accounts",
                serde_json::json!({
                    "provider_kind": "imap_smtp",
                    "identity_class": "work",
                    "account_label": "Upserted mail",
                    "email": "upserted@example.com",
                    "access_token": "secret-token",
                    "imap_host": "imap.example.com",
                    "smtp_host": "smtp.example.com"
                }),
            ),
        )
        .expect("dispatch account upsert");
        assert_eq!(response.status, 200);

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["account"]["account_label"], "Upserted mail");
        assert_eq!(parsed["account"]["enabled_capabilities"][0], "mail");
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_accounts_post_creates_account_with_initial_provider_config() {
        let _guard = office_test_guard();
        let ctx = build_office_authed_ctx(vec![Arc::new(ReadyProbeAdapter {
            provider_kind: "imap_smtp",
            reason: "imap_login_ok",
        })]);
        let env = build_router_env();

        let response = dispatch(
            &ctx,
            &env,
            authed_post(
                "/api/config/accounts",
                serde_json::json!({
                    "provider_kind": "imap_smtp",
                    "identity_class": "work",
                    "account_label": "Primary mail",
                    "email": "alice@example.com",
                    "access_token": "secret-token",
                    "imap_host": "imap.example.com",
                    "smtp_host": "smtp.example.com"
                }),
            ),
        )
        .expect("dispatch account create");
        assert_eq!(
            response.status,
            200,
            "body={}",
            String::from_utf8_lossy(&response.body)
        );

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        let account_key = parsed["account"]["account_key"]
            .as_str()
            .expect("account_key string");
        assert!(!account_key.trim().is_empty());
        let credential = ctx
            .platform
            .office_credential_store()
            .get(account_key)
            .expect("load credential")
            .expect("credential exists");
        assert_eq!(credential.access_token, "secret-token");
        assert_eq!(
            credential.metadata_value("mail_username"),
            Some("alice@example.com")
        );
        assert_eq!(
            credential.metadata_value("mail_imap_host"),
            Some("imap.example.com")
        );
        assert_eq!(
            credential.metadata_value("mail_smtp_host"),
            Some("smtp.example.com")
        );
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_accounts_post_requires_identity_class() {
        let _guard = office_test_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();

        let response = dispatch(
            &ctx,
            &env,
            authed_post(
                "/api/config/accounts",
                serde_json::json!({
                    "provider_kind": "imap_smtp",
                    "account_label": "Primary mail",
                    "email": "alice@example.com",
                    "access_token": "secret-token",
                    "imap_host": "imap.example.com",
                    "smtp_host": "smtp.example.com"
                }),
            ),
        )
        .expect("dispatch account create");
        assert_eq!(
            response.status,
            400,
            "body={}",
            String::from_utf8_lossy(&response.body)
        );
        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["disposition"], "needs_user_facts");
        assert_eq!(parsed["reason"], "missing_user_facts");
        assert!(parsed["missing_fields"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == "identity_class")));
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_accounts_post_rejects_legacy_nested_public_wrappers() {
        let _guard = office_test_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();

        let response = dispatch(
            &ctx,
            &env,
            authed_post(
                "/api/config/accounts",
                serde_json::json!({
                    "op": "apply_account",
                    "account": {
                        "provider_kind": "imap_smtp",
                        "identity_class": "work",
                        "email": "alice@example.com"
                    },
                    "credential": {
                        "password": "secret-token",
                        "imap_host": "imap.example.com",
                        "smtp_host": "smtp.example.com"
                    }
                }),
            ),
        )
        .expect("dispatch account create");
        assert_eq!(
            response.status,
            400,
            "body={}",
            String::from_utf8_lossy(&response.body)
        );
        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["error_key"], "config.rejected");
        assert!(parsed.get("error").is_none());
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_accounts_post_returns_structured_probe_failure_without_persisting() {
        let _guard = office_test_guard();
        let ctx = build_office_authed_ctx(vec![Arc::new(FailingProbeAdapter)]);
        let env = build_router_env();
        let accounts_before = config::get_office_accounts_segment(ctx.config_file_store.as_ref())
            .and_then(|body| {
                serde_json::from_str::<OfficeAccountsSegment>(&body).map_err(|error| {
                    crate::error::Error::config(
                        "config_accounts_post_probe_failure_test",
                        error.to_string(),
                    )
                })
            })
            .expect("load initial accounts segment");
        let credentials_before = ctx
            .platform
            .office_credential_store()
            .list()
            .expect("load initial office credentials");
        let runtime_before = ctx
            .platform
            .office_runtime_status_store()
            .list()
            .expect("load initial office runtime status");

        let response = dispatch(
            &ctx,
            &env,
            authed_post(
                "/api/config/accounts",
                serde_json::json!({
                    "provider_kind": "imap_smtp",
                    "identity_class": "work",
                    "account_label": "Primary mail",
                    "email": "alice@example.com",
                    "access_token": "secret-token",
                    "imap_host": "imap.example.com",
                    "smtp_host": "smtp.example.com"
                }),
            ),
        )
        .expect("dispatch account create");
        assert_eq!(
            response.status,
            400,
            "body={}",
            String::from_utf8_lossy(&response.body)
        );
        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["disposition"], "probe_failed");
        assert_eq!(parsed["reason"], "probe_error");
        assert_eq!(parsed["error_stage"], "office_probe_test");
        assert_eq!(parsed["error_key"], "office.provider_error");
        assert_eq!(
            parsed["upstream_error"],
            "config: imap login failed (stage: office_probe_test)"
        );
        assert!(parsed.get("error_message").is_none());

        let accounts_after = config::get_office_accounts_segment(ctx.config_file_store.as_ref())
            .and_then(|body| {
                serde_json::from_str::<OfficeAccountsSegment>(&body).map_err(|error| {
                    crate::error::Error::config(
                        "config_accounts_post_probe_failure_test",
                        error.to_string(),
                    )
                })
            })
            .expect("load office accounts segment");
        let credentials_after = ctx
            .platform
            .office_credential_store()
            .list()
            .expect("load office credentials");
        let runtime_after = ctx
            .platform
            .office_runtime_status_store()
            .list()
            .expect("load office runtime status");
        assert_eq!(accounts_after, accounts_before);
        assert_eq!(credentials_after, credentials_before);
        assert_eq!(runtime_after, runtime_before);
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_accounts_delete_removes_account_and_related_state() {
        let _guard = office_test_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();
        let account_key = "test-http-mail-delete";
        seed_account(&ctx, account_key);
        let runtime = crate::office::OfficeAccountRuntimeStatus {
            account_key: account_key.to_string(),
            probe_ok: false,
            last_error: "auth_failed".to_string(),
            last_probe_at_unix_secs: 1,
            last_activity_kind: String::new(),
            last_activity_ok: false,
            last_activity_at_unix_secs: 0,
            updated_at: 1,
        };
        ctx.platform
            .office_runtime_status_store()
            .set(&runtime)
            .expect("seed runtime");

        let response = dispatch(
            &ctx,
            &env,
            authed_delete(&format!("/api/config/accounts/{account_key}")),
        )
        .expect("dispatch account delete");
        assert_eq!(
            response.status,
            200,
            "body={}",
            String::from_utf8_lossy(&response.body)
        );

        assert!(ctx
            .platform
            .office_credential_store()
            .get(account_key)
            .expect("load credential")
            .is_none());
        assert!(ctx
            .platform
            .office_runtime_status_store()
            .get(account_key)
            .expect("load runtime")
            .is_none());

        let detail = dispatch(
            &ctx,
            &env,
            authed_get(&format!("/api/config/accounts/{account_key}")),
        )
        .expect("dispatch deleted account detail");
        assert_eq!(detail.status, 400);

        let capabilities = dispatch(&ctx, &env, authed_get("/api/config/capabilities/mail"))
            .expect("dispatch capability detail");
        let parsed: Value = serde_json::from_slice(&capabilities.body).expect("parse capability");
        assert_eq!(parsed["selection_status"], "missing");
        assert!(parsed["selected_account_key"].is_null());
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_capabilities_get_returns_capability_statuses() {
        let _guard = office_test_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();
        seed_account(&ctx, "test-http-mail-capability");

        let response = dispatch(&ctx, &env, authed_get("/api/config/capabilities"))
            .expect("dispatch capabilities summary");
        assert_eq!(
            response.status,
            200,
            "body={}",
            String::from_utf8_lossy(&response.body)
        );

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        let items = parsed["items"].as_array().expect("items array");
        let mail = items
            .iter()
            .find(|item| item["capability"] == "mail")
            .expect("mail capability");
        assert!(mail.get("default_account_key").is_none());
        assert_eq!(mail["selection_status"], "selected");
        assert_eq!(mail["selected_account_key"], "test-http-mail-capability");
        assert!(mail["accounts"].is_array());
        assert!(mail["accounts"].as_array().expect("accounts array")[0]
            .get("provider_display_name")
            .is_none());
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_capabilities_detail_returns_single_capability() {
        let _guard = office_test_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();
        seed_account(&ctx, "test-http-mail-capability-detail");

        let response = dispatch(&ctx, &env, authed_get("/api/config/capabilities/mail"))
            .expect("dispatch capability detail");
        assert_eq!(
            response.status,
            200,
            "body={}",
            String::from_utf8_lossy(&response.body)
        );

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["capability"], "mail");
        assert_eq!(
            parsed["selected_account_key"],
            "test-http-mail-capability-detail"
        );
        assert_eq!(parsed["accounts"].as_array().expect("accounts").len(), 1);
        assert_eq!(
            parsed["accounts"].as_array().expect("accounts")[0]["display_name_key"],
            "accounts.providers.imap_smtp"
        );
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_capabilities_detail_invalid_capability_uses_error_key_contract() {
        let _guard = office_test_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();

        let response = dispatch(&ctx, &env, authed_get("/api/config/capabilities/invalid"))
            .expect("dispatch invalid capability detail");
        assert_eq!(
            response.status,
            400,
            "body={}",
            String::from_utf8_lossy(&response.body)
        );

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["error_key"], "office.capability_invalid");
        assert!(parsed.get("error").is_none());
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn config_providers_invalid_capability_uses_error_key_contract() {
        let _guard = office_test_guard();
        let ctx = build_authed_ctx();
        let env = build_router_env();

        let response = dispatch(
            &ctx,
            &env,
            authed_get("/api/config/providers?capability=invalid"),
        )
        .expect("dispatch invalid provider catalog");
        assert_eq!(
            response.status,
            400,
            "body={}",
            String::from_utf8_lossy(&response.body)
        );

        let parsed: Value = serde_json::from_slice(&response.body).expect("parse response");
        assert_eq!(parsed["error_key"], "office.capability_invalid");
        assert!(parsed.get("error").is_none());
    }
}
