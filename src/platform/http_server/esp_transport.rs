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

macro_rules! esp_route {
    ($server:expr, $path:expr, $m:ident, $ctx:expr, $env:expr, $store:expr, $mode:expr) => {{
        let ctx = Arc::clone($ctx);
        let env = $env.clone();
        let store = Arc::clone($store);
        let path: &'static str = $path;
        let http_method = Method::$m;
        $server
            .fn_handler(path, http_method, move |req| -> HandlerResult {
                esp_dispatch_route(&ctx, &env, &store, req, http_method, $mode)
            })
            .map_err(|e| crate::error::Error::Other {
                source: Box::new(e),
                stage: "http_server_handler",
            })?;
    }};
}

/// 注册与历史 `register!` 等价的全量 URI handler。
pub(super) fn register_all_esp_routes(
    server: &mut EspHttpServer<'static>,
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    config_store: &Arc<dyn ConfigStore + Send + Sync>,
) -> Result<()> {
    esp_route!(server, "/", Get, ctx, env, config_store, EspBodyMode::None);
    esp_route!(
        server,
        "/",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/wifi",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/wifi",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/pairing",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/pairing",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/common.css",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/common.css",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/common.js",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/common.js",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/pairing_code",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/pairing_code",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN)
    );
    esp_route!(
        server,
        "/api/pairing_code",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/config",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/config",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/config/wifi",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN)
    );
    esp_route!(
        server,
        "/api/config/wifi",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/config/llm",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/config/llm",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN)
    );

    esp_route!(
        server,
        "/api/config/channels",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/config/channels",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN)
    );

    esp_route!(
        server,
        "/api/config/system",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/config/system",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN)
    );

    esp_route!(
        server,
        "/api/config/hardware",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/config/hardware",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/config/hardware",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN)
    );

    esp_route!(
        server,
        "/api/config/audio",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/config/audio",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/config/audio",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN)
    );

    esp_route!(
        server,
        "/api/config/display",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/config/display",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/config/display",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN)
    );

    esp_route!(
        server,
        "/api/wifi/scan",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/wifi/scan",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/health",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/health",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/metrics",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/metrics",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/resource",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/resource",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/tools",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/tools",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/csrf_token",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/csrf_token",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/diagnose",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/diagnose",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/system_info",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/system_info",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/channel_connectivity",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/channel_connectivity",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/soul",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/user",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/sessions",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/sessions",
        Delete,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/sessions",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/memory/status",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/memory/status",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/skills",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/skills",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN)
    );
    esp_route!(
        server,
        "/api/skills",
        Delete,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/skills",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/skills/import",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN)
    );
    esp_route!(
        server,
        "/api/skills/import",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/soul",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/user",
        Get,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/soul",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::Utf8SoulUser
    );
    esp_route!(
        server,
        "/api/user",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::Utf8SoulUser
    );

    esp_route!(
        server,
        "/api/restart",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/restart",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/config_reset",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );
    esp_route!(
        server,
        "/api/config_reset",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    esp_route!(
        server,
        "/api/webhook",
        Post,
        ctx,
        env,
        config_store,
        EspBodyMode::Utf8(POST_BODY_MAX_LEN)
    );
    esp_route!(
        server,
        "/api/webhook",
        Options,
        ctx,
        env,
        config_store,
        EspBodyMode::None
    );

    #[cfg(feature = "ota")]
    {
        esp_route!(
            server,
            "/api/ota/check",
            Get,
            ctx,
            env,
            config_store,
            EspBodyMode::None
        );
        esp_route!(
            server,
            "/api/ota/check",
            Options,
            ctx,
            env,
            config_store,
            EspBodyMode::None
        );
        esp_route!(
            server,
            "/api/ota",
            Post,
            ctx,
            env,
            config_store,
            EspBodyMode::Utf8(POST_BODY_MAX_LEN)
        );
        esp_route!(
            server,
            "/api/ota",
            Options,
            ctx,
            env,
            config_store,
            EspBodyMode::None
        );
    }

    Ok(())
}
