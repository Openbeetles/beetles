//! ESP HTTP 服务器薄适配：`Request` → `router::IncomingRequest` → 写回响应。
//! ESP HTTP server thin adapter: map `Request` → `router::IncomingRequest` → write response.

use crate::error::Result;
use crate::platform::http_server::common::{
    self, ApiResponse, BodyReadError, HandlerResult, CORS_HEADERS, POST_BODY_MAX_LEN,
};
use crate::platform::http_server::handlers::HandlerContext;
use crate::platform::http_server::router::{
    self, IncomingRequest, OutgoingResponse, RestartAction, RouterEnv,
};
use crate::platform::ConfigStore;
use embedded_io::Write as _;
use embedded_svc::http::server::Request;
use embedded_svc::http::{Headers, Method};
use esp_idf_svc::http::server::Connection;
use esp_idf_svc::http::server::EspHttpServer;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy)]
pub(super) enum EspBodyMode {
    None,
    Utf8(usize),
    Utf8SoulUser,
}

#[derive(Clone, Copy)]
struct EspRouteSpec {
    path: &'static str,
    method: Method,
    body_mode: EspBodyMode,
}

impl EspRouteSpec {
    const fn new(path: &'static str, method: Method, body_mode: EspBodyMode) -> Self {
        Self {
            path,
            method,
            body_mode,
        }
    }
}

fn method_as_str(m: Method) -> &'static str {
    match m {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
        Method::Options => "OPTIONS",
        Method::Head => "HEAD",
        Method::Patch => "PATCH",
        _ => "GET",
    }
}

fn collect_headers(req: &impl Headers) -> Vec<(String, String)> {
    const NAMES: &[&str] = &[
        "Host",
        "Content-Type",
        "X-Pairing-Code",
        "X-CSRF-Token",
        "X-Webhook-Token",
        "X-Signature-Timestamp",
        "X-Signature-Ed25519",
    ];
    let mut v = Vec::new();
    for name in NAMES {
        if let Some(val) = req.header(name) {
            v.push(((*name).to_string(), val.to_string()));
        }
    }
    v
}

#[inline(never)]
fn read_body_esp<C: Connection>(
    req: &mut Request<C>,
    store: &dyn ConfigStore,
    mode: EspBodyMode,
) -> std::result::Result<Vec<u8>, ApiResponse> {
    match mode {
        EspBodyMode::None => Ok(Vec::new()),
        EspBodyMode::Utf8(max) => match common::read_body_utf8_impl(req, req.content_len(), max) {
            Ok(s) => Ok(s.into_bytes()),
            Err(BodyReadError::ReadFailed) => {
                let loc = crate::i18n::locale_from_store(store);
                let msg = crate::i18n::tr(crate::i18n::Message::BodyReadFailed, loc);
                Err(ApiResponse::err_500(&msg))
            }
            Err(BodyReadError::InvalidUtf8) => {
                let loc = crate::i18n::locale_from_store(store);
                let msg = crate::i18n::tr(crate::i18n::Message::InvalidUtf8, loc);
                Err(ApiResponse::err_400(&msg))
            }
        },
        EspBodyMode::Utf8SoulUser => {
            let max = crate::memory::MAX_SOUL_USER_LEN;
            match common::read_body_utf8_impl(req, req.content_len(), max) {
                Ok(s) => Ok(s.into_bytes()),
                Err(BodyReadError::ReadFailed) => {
                    let loc = crate::i18n::locale_from_store(store);
                    let msg = crate::i18n::tr(crate::i18n::Message::BodyReadFailed, loc);
                    Err(ApiResponse::err_500(&msg))
                }
                Err(BodyReadError::InvalidUtf8) => {
                    let loc = crate::i18n::locale_from_store(store);
                    let msg = crate::i18n::tr(crate::i18n::Message::InvalidUtf8, loc);
                    Err(ApiResponse::err_400(&msg))
                }
            }
        }
    }
}

#[inline(never)]
fn write_api_resp<C: Connection>(req: Request<C>, r: ApiResponse) -> HandlerResult {
    let mut resp = req
        .into_response(r.status, Some(r.status_text), CORS_HEADERS)
        .map_err(common::to_io)?;
    resp.write_all(&r.body).map_err(common::to_io)?;
    Ok(())
}

#[inline(never)]
fn write_outgoing<C: Connection>(
    ctx: &Arc<HandlerContext>,
    req: Request<C>,
    out: OutgoingResponse,
    restart_reason: &str,
) -> HandlerResult {
    let mut resp = req
        .into_response(out.status, Some(out.status_text), out.headers)
        .map_err(common::to_io)?;
    resp.write_all(&out.body).map_err(common::to_io)?;
    if out.restart == RestartAction::After300Ms {
        let platform = Arc::clone(&ctx.platform);
        let restart_reason = restart_reason.to_string();
        crate::util::spawn_guarded_with_profile(
            "restart_defer",
            4096,
            Some(crate::util::SpawnCore::Core0),
            crate::util::HttpThreadRole::Background,
            move || {
                std::thread::sleep(Duration::from_millis(300));
                crate::runtime::request_restart_with_continuity_flush(
                    platform,
                    None,
                    restart_reason.as_str(),
                );
            },
        );
    }
    Ok(())
}

#[inline(never)]
pub(super) fn esp_dispatch_route<C: Connection>(
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    store: &Arc<dyn ConfigStore + Send + Sync>,
    mut req: Request<C>,
    method: Method,
    body_mode: EspBodyMode,
) -> HandlerResult {
    let uri = req.uri().to_string();
    let restart_reason = uri.clone();
    let headers = collect_headers(&req);
    let body = match read_body_esp(&mut req, store.as_ref(), body_mode) {
        Ok(b) => b,
        Err(r) => return write_api_resp(req, r),
    };
    let incoming = IncomingRequest {
        method: method_as_str(method).to_string(),
        uri,
        headers,
        body,
    };
    let out = match router::dispatch(ctx.as_ref(), env, incoming) {
        Ok(o) => o,
        Err(e) => {
            log::warn!("http router dispatch: {}", e);
            return Err(common::to_io(e));
        }
    };
    write_outgoing(ctx, req, out, restart_reason.as_str())
}

#[cold]
#[inline(never)]
fn register_esp_route(
    server: &mut EspHttpServer<'static>,
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    config_store: &Arc<dyn ConfigStore + Send + Sync>,
    spec: EspRouteSpec,
) -> Result<()> {
    let ctx = Arc::clone(ctx);
    let env = env.clone();
    let store = Arc::clone(config_store);
    server
        .fn_handler(spec.path, spec.method, move |req| -> HandlerResult {
            esp_dispatch_route(&ctx, &env, &store, req, spec.method, spec.body_mode)
        })
        .map_err(|e| crate::error::Error::Other {
            source: Box::new(e),
            stage: "http_server_handler",
        })?;
    Ok(())
}

#[cold]
#[inline(never)]
fn register_esp_route_specs(
    server: &mut EspHttpServer<'static>,
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    config_store: &Arc<dyn ConfigStore + Send + Sync>,
    specs: &[EspRouteSpec],
) -> Result<()> {
    for spec in specs {
        register_esp_route(server, ctx, env, config_store, *spec)?;
    }
    Ok(())
}

const STATIC_PAGE_ROUTES: &[EspRouteSpec] = &[
    EspRouteSpec::new("/", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/wifi", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/wifi", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/pairing", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/pairing", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/common.css", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/common.css", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/common.js", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/common.js", Method::Options, EspBodyMode::None),
];

const PAIRING_AND_CONFIG_ROUTES: &[EspRouteSpec] = &[
    EspRouteSpec::new("/api/pairing_code", Method::Get, EspBodyMode::None),
    EspRouteSpec::new(
        "/api/pairing_code",
        Method::Post,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN),
    ),
    EspRouteSpec::new("/api/pairing_code", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/config", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/config", Method::Options, EspBodyMode::None),
    EspRouteSpec::new(
        "/api/config/wifi",
        Method::Post,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN),
    ),
    EspRouteSpec::new("/api/config/wifi", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/config/llm", Method::Options, EspBodyMode::None),
    EspRouteSpec::new(
        "/api/config/llm",
        Method::Post,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN),
    ),
    EspRouteSpec::new("/api/config/channels", Method::Options, EspBodyMode::None),
    EspRouteSpec::new(
        "/api/config/channels",
        Method::Post,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN),
    ),
    EspRouteSpec::new("/api/config/system", Method::Options, EspBodyMode::None),
    EspRouteSpec::new(
        "/api/config/system",
        Method::Post,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN),
    ),
    EspRouteSpec::new("/api/config/hardware", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/config/hardware", Method::Options, EspBodyMode::None),
    EspRouteSpec::new(
        "/api/config/hardware",
        Method::Post,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN),
    ),
    EspRouteSpec::new("/api/config/audio", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/config/audio", Method::Options, EspBodyMode::None),
    EspRouteSpec::new(
        "/api/config/audio",
        Method::Post,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN),
    ),
    EspRouteSpec::new("/api/config/display", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/config/display", Method::Options, EspBodyMode::None),
    EspRouteSpec::new(
        "/api/config/display",
        Method::Post,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN),
    ),
    EspRouteSpec::new("/api/wifi/scan", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/wifi/scan", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/csrf_token", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/csrf_token", Method::Options, EspBodyMode::None),
];

const OBSERVABILITY_ROUTES: &[EspRouteSpec] = &[
    EspRouteSpec::new("/api/health", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/health", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/metrics", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/metrics", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/resource", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/resource", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/diagnose", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/diagnose", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/system_info", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/system_info", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/channel_connectivity", Method::Get, EspBodyMode::None),
    EspRouteSpec::new(
        "/api/channel_connectivity",
        Method::Options,
        EspBodyMode::None,
    ),
];

const MEMORY_AND_SKILL_ROUTES: &[EspRouteSpec] = &[
    EspRouteSpec::new("/api/tools", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/tools", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/soul", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/user", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/sessions", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/sessions", Method::Delete, EspBodyMode::None),
    EspRouteSpec::new("/api/sessions", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/memory/status", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/memory/status", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/skills", Method::Get, EspBodyMode::None),
    EspRouteSpec::new(
        "/api/skills",
        Method::Post,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN),
    ),
    EspRouteSpec::new("/api/skills", Method::Delete, EspBodyMode::None),
    EspRouteSpec::new("/api/skills", Method::Options, EspBodyMode::None),
    EspRouteSpec::new(
        "/api/skills/import",
        Method::Post,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN),
    ),
    EspRouteSpec::new("/api/skills/import", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/soul", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/user", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/soul", Method::Post, EspBodyMode::Utf8SoulUser),
    EspRouteSpec::new("/api/user", Method::Post, EspBodyMode::Utf8SoulUser),
];

const ACTION_ROUTES: &[EspRouteSpec] = &[
    EspRouteSpec::new("/api/restart", Method::Post, EspBodyMode::None),
    EspRouteSpec::new("/api/restart", Method::Options, EspBodyMode::None),
    EspRouteSpec::new("/api/config_reset", Method::Post, EspBodyMode::None),
    EspRouteSpec::new("/api/config_reset", Method::Options, EspBodyMode::None),
    EspRouteSpec::new(
        "/api/webhook",
        Method::Post,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN),
    ),
    EspRouteSpec::new("/api/webhook", Method::Options, EspBodyMode::None),
];

#[cold]
#[inline(never)]
fn register_static_page_routes(
    server: &mut EspHttpServer<'static>,
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    config_store: &Arc<dyn ConfigStore + Send + Sync>,
) -> Result<()> {
    register_esp_route_specs(server, ctx, env, config_store, STATIC_PAGE_ROUTES)
}

#[cold]
#[inline(never)]
fn register_pairing_and_config_routes(
    server: &mut EspHttpServer<'static>,
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    config_store: &Arc<dyn ConfigStore + Send + Sync>,
) -> Result<()> {
    register_esp_route_specs(server, ctx, env, config_store, PAIRING_AND_CONFIG_ROUTES)
}

#[cold]
#[inline(never)]
fn register_observability_routes(
    server: &mut EspHttpServer<'static>,
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    config_store: &Arc<dyn ConfigStore + Send + Sync>,
) -> Result<()> {
    register_esp_route_specs(server, ctx, env, config_store, OBSERVABILITY_ROUTES)
}

#[cold]
#[inline(never)]
fn register_memory_and_skill_routes(
    server: &mut EspHttpServer<'static>,
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    config_store: &Arc<dyn ConfigStore + Send + Sync>,
) -> Result<()> {
    register_esp_route_specs(server, ctx, env, config_store, MEMORY_AND_SKILL_ROUTES)
}

#[cold]
#[inline(never)]
fn register_action_routes(
    server: &mut EspHttpServer<'static>,
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    config_store: &Arc<dyn ConfigStore + Send + Sync>,
) -> Result<()> {
    register_esp_route_specs(server, ctx, env, config_store, ACTION_ROUTES)
}

#[cfg(feature = "ota")]
const OTA_ROUTES: &[EspRouteSpec] = &[
    EspRouteSpec::new("/api/ota/check", Method::Get, EspBodyMode::None),
    EspRouteSpec::new("/api/ota/check", Method::Options, EspBodyMode::None),
    EspRouteSpec::new(
        "/api/ota",
        Method::Post,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN),
    ),
    EspRouteSpec::new("/api/ota", Method::Options, EspBodyMode::None),
];

#[cfg(feature = "ota")]
#[cold]
#[inline(never)]
fn register_optional_feature_routes(
    server: &mut EspHttpServer<'static>,
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    config_store: &Arc<dyn ConfigStore + Send + Sync>,
) -> Result<()> {
    register_esp_route_specs(server, ctx, env, config_store, OTA_ROUTES)?;
    Ok(())
}

#[cfg(not(feature = "ota"))]
#[cold]
#[inline(never)]
fn register_optional_feature_routes(
    _server: &mut EspHttpServer<'static>,
    _ctx: &Arc<HandlerContext>,
    _env: &RouterEnv,
    _config_store: &Arc<dyn ConfigStore + Send + Sync>,
) -> Result<()> {
    Ok(())
}

/// 注册与历史 `register!` 等价的全量 URI handler。
pub(super) fn register_all_esp_routes(
    server: &mut EspHttpServer<'static>,
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    config_store: &Arc<dyn ConfigStore + Send + Sync>,
) -> Result<()> {
    register_static_page_routes(server, ctx, env, config_store)?;
    register_pairing_and_config_routes(server, ctx, env, config_store)?;
    register_observability_routes(server, ctx, env, config_store)?;
    register_memory_and_skill_routes(server, ctx, env, config_store)?;
    register_action_routes(server, ctx, env, config_store)?;
    register_optional_feature_routes(server, ctx, env, config_store)?;
    Ok(())
}
