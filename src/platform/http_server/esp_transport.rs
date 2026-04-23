//! ESP HTTP 服务器薄适配：`Request` → `router::IncomingRequest` → 写回响应。
//! ESP HTTP server thin adapter: map `Request` → `router::IncomingRequest` → write response.

use crate::error::Result;
use crate::platform::http_server::api_contract;
use crate::platform::http_server::common::{
    self, ApiResponse, BodyReadError, HandlerResult, CORS_HEADERS,
};
use crate::platform::http_server::handlers::HandlerContext;
use crate::platform::http_server::lazy_executor::LazyExecutor;
#[cfg(feature = "ota")]
use crate::platform::http_server::router::catalog::OTA_ROUTE_SPECS;
use crate::platform::http_server::router::{
    self,
    catalog::{
        HttpRouteSpec, RouteBodyMode, RouteDispatchMode, RouteMethod, ACTION_ROUTE_SPECS,
        MEMORY_AND_SKILL_ROUTE_SPECS, OBSERVABILITY_ROUTE_SPECS, PAIRING_AND_CONFIG_ROUTE_SPECS,
        ROOT_ROUTE_SPECS,
    },
    IncomingRequest, OutgoingResponse, RestartAction, RouterEnv,
};
use crate::platform::ConfigStore;
use embedded_io::Write as _;
use embedded_svc::http::server::Request;
use embedded_svc::http::{Headers, Method};
use esp_idf_svc::http::server::Connection;
use esp_idf_svc::http::server::EspHttpServer;
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SendError, SyncSender};
use std::sync::Arc;
use std::time::Duration;

const ESP_ROUTE_EXEC_QUEUE_CAPACITY: usize = 4;
const ESP_ROUTE_EXEC_TIMEOUT: Duration = Duration::from_secs(20);
const ESP_ROUTE_EXEC_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const ESP_ROUTE_EXEC_SUBMIT_ATTEMPTS: usize = 2;

struct EspRouteJob {
    incoming: IncomingRequest,
    reply_tx: SyncSender<OutgoingResponse>,
}

struct EspRouteExecutorInner {
    submit_tx: SyncSender<EspRouteJob>,
}

#[derive(Clone)]
struct EspRouteExecutor {
    inner: LazyExecutor<EspRouteExecutorInner>,
}

impl EspRouteExecutor {
    fn new(
        ctx: &Arc<HandlerContext>,
        env: &RouterEnv,
        config_store: &Arc<dyn ConfigStore + Send + Sync>,
    ) -> Self {
        let ctx = Arc::clone(ctx);
        let env = env.clone();
        let store = Arc::clone(config_store);
        Self {
            inner: LazyExecutor::new(move || {
                let (submit_tx, rx) = sync_channel(ESP_ROUTE_EXEC_QUEUE_CAPACITY);
                let ctx = Arc::clone(&ctx);
                let env = env.clone();
                let store = Arc::clone(&store);
                crate::util::spawn_guarded_with_profile(
                    "http_route_exec",
                    crate::util::STACK_HTTP_ROUTE_WORKER,
                    Some(crate::util::SpawnCore::Core1),
                    crate::util::HttpThreadRole::Io,
                    move || run_esp_route_executor(ctx, env, store, rx),
                );
                log::info!("[http_server] http_route_exec lazy-started on first request");
                crate::orchestrator::log_startup_memory_checkpoint("http_route_exec_spawn");
                EspRouteExecutorInner { submit_tx }
            }),
        }
    }

    fn execute(&self, store: &dyn ConfigStore, incoming: IncomingRequest) -> OutgoingResponse {
        let (reply_tx, reply_rx) = sync_channel(1);
        let mut pending_job = Some(EspRouteJob { incoming, reply_tx });
        for attempt in 0..ESP_ROUTE_EXEC_SUBMIT_ATTEMPTS {
            let inner = self.inner.get();
            let job = pending_job
                .take()
                .expect("route executor retry must keep pending job");
            match inner.submit_tx.send(job) {
                Ok(()) => {
                    return match reply_rx.recv_timeout(ESP_ROUTE_EXEC_TIMEOUT) {
                        Ok(out) => out,
                        Err(RecvTimeoutError::Timeout) => internal_server_error_response(
                            store,
                            "http_route_exec_wait",
                            "dispatch timed out".to_string(),
                        ),
                        Err(RecvTimeoutError::Disconnected) => {
                            let _ = self.inner.clear_if(&inner);
                            internal_server_error_response(
                                store,
                                "http_route_exec_wait",
                                "dispatch worker stopped".to_string(),
                            )
                        }
                    };
                }
                Err(SendError(job)) => {
                    let _ = self.inner.clear_if(&inner);
                    pending_job = Some(job);
                    if attempt + 1 == ESP_ROUTE_EXEC_SUBMIT_ATTEMPTS {
                        return internal_server_error_response(
                            store,
                            "http_route_exec_submit",
                            "dispatch queue unavailable after worker restart".to_string(),
                        );
                    }
                }
            }
        }
        internal_server_error_response(
            store,
            "http_route_exec_submit",
            "dispatch queue unavailable".to_string(),
        )
    }
}

fn internal_server_error_response(
    _store: &dyn ConfigStore,
    stage: &'static str,
    detail: String,
) -> OutgoingResponse {
    log::warn!("{}: {}", stage, detail);
    OutgoingResponse {
        status: 500,
        status_text: "Internal Server Error",
        headers: CORS_HEADERS,
        body: ApiResponse::err_500_key(api_contract::COMMON_OPERATION_FAILED).body,
        restart: RestartAction::None,
    }
}

fn routed_error_response(_store: &dyn ConfigStore, error: crate::error::Error) -> OutgoingResponse {
    let detail = error.to_string();
    let stage = error.stage();
    let error_key = api_contract::error_key(&error);
    let (status, status_text) = if error_key == api_contract::COMMON_INVALID_UTF8 {
        (400, "Bad Request")
    } else {
        (500, "Internal Server Error")
    };
    log::warn!("{}: {}", stage, detail);
    OutgoingResponse {
        status,
        status_text,
        headers: CORS_HEADERS,
        body: ApiResponse::err_key(status, status_text, error_key).body,
        restart: RestartAction::None,
    }
}

fn dispatch_incoming(
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    store: &dyn ConfigStore,
    incoming: IncomingRequest,
) -> OutgoingResponse {
    match router::dispatch(ctx.as_ref(), env, incoming) {
        Ok(out) => out,
        Err(error) => routed_error_response(store, error),
    }
}

fn run_esp_route_executor(
    ctx: Arc<HandlerContext>,
    env: RouterEnv,
    store: Arc<dyn ConfigStore + Send + Sync>,
    rx: Receiver<EspRouteJob>,
) {
    loop {
        match rx.recv_timeout(ESP_ROUTE_EXEC_IDLE_TIMEOUT) {
            Ok(job) => {
                let out = match router::dispatch(ctx.as_ref(), &env, job.incoming) {
                    Ok(out) => out,
                    Err(error) => routed_error_response(store.as_ref(), error),
                };
                let _ = job.reply_tx.send(out);
            }
            Err(RecvTimeoutError::Timeout) => {
                log::info!(
                    "[http_server] http_route_exec idle-stopped after {}s",
                    ESP_ROUTE_EXEC_IDLE_TIMEOUT.as_secs()
                );
                break;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn esp_method(method: RouteMethod) -> Method {
    match method {
        RouteMethod::Get => Method::Get,
        RouteMethod::Post => Method::Post,
        RouteMethod::Delete => Method::Delete,
        RouteMethod::Options => Method::Options,
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
    mode: RouteBodyMode,
) -> std::result::Result<Vec<u8>, ApiResponse> {
    match mode {
        RouteBodyMode::None => Ok(Vec::new()),
        RouteBodyMode::Utf8(max) => {
            match common::read_body_utf8_impl(req, req.content_len(), max) {
                Ok(s) => Ok(s.into_bytes()),
                Err(BodyReadError::ReadFailed) => {
                    let _ = store;
                    Err(ApiResponse::err_500_key(
                        api_contract::COMMON_BODY_READ_FAILED,
                    ))
                }
                Err(BodyReadError::InvalidUtf8) => {
                    let _ = store;
                    Err(ApiResponse::err_400_key(api_contract::COMMON_INVALID_UTF8))
                }
            }
        }
        RouteBodyMode::Utf8SoulUser => {
            let max = crate::memory::MAX_SOUL_USER_LEN;
            match common::read_body_utf8_impl(req, req.content_len(), max) {
                Ok(s) => Ok(s.into_bytes()),
                Err(BodyReadError::ReadFailed) => {
                    let _ = store;
                    Err(ApiResponse::err_500_key(
                        api_contract::COMMON_BODY_READ_FAILED,
                    ))
                }
                Err(BodyReadError::InvalidUtf8) => {
                    let _ = store;
                    Err(ApiResponse::err_400_key(api_contract::COMMON_INVALID_UTF8))
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
            crate::util::STACK_RESTART_DEFER,
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
fn esp_dispatch_route<C: Connection>(
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    store: &Arc<dyn ConfigStore + Send + Sync>,
    executor: &EspRouteExecutor,
    mut req: Request<C>,
    spec: HttpRouteSpec,
) -> HandlerResult {
    let uri = req.uri().to_string();
    let restart_reason = uri.clone();
    let headers = collect_headers(&req);
    let body = match read_body_esp(&mut req, store.as_ref(), spec.body_mode) {
        Ok(b) => b,
        Err(r) => return write_api_resp(req, r),
    };
    let incoming = IncomingRequest {
        method: spec.method.as_str().to_string(),
        uri,
        headers,
        body,
    };
    let out = match spec.dispatch_mode {
        RouteDispatchMode::Direct => dispatch_incoming(ctx, env, store.as_ref(), incoming),
        RouteDispatchMode::Worker => executor.execute(store.as_ref(), incoming),
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
    executor: &EspRouteExecutor,
    spec: HttpRouteSpec,
) -> Result<()> {
    let ctx = Arc::clone(ctx);
    let env = env.clone();
    let store = Arc::clone(config_store);
    let executor = executor.clone();
    server
        .fn_handler(
            spec.path,
            esp_method(spec.method),
            move |req| -> HandlerResult {
                esp_dispatch_route(&ctx, &env, &store, &executor, req, spec)
            },
        )
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
    executor: &EspRouteExecutor,
    specs: &[HttpRouteSpec],
) -> Result<()> {
    for spec in specs {
        register_esp_route(server, ctx, env, config_store, executor, *spec)?;
    }
    Ok(())
}

const ROOT_ROUTES: &[HttpRouteSpec] = ROOT_ROUTE_SPECS;
const PAIRING_AND_CONFIG_ROUTES: &[HttpRouteSpec] = PAIRING_AND_CONFIG_ROUTE_SPECS;
const OBSERVABILITY_ROUTES: &[HttpRouteSpec] = OBSERVABILITY_ROUTE_SPECS;
const MEMORY_AND_SKILL_ROUTES: &[HttpRouteSpec] = MEMORY_AND_SKILL_ROUTE_SPECS;
const ACTION_ROUTES: &[HttpRouteSpec] = ACTION_ROUTE_SPECS;
#[cfg(feature = "ota")]
const OTA_ROUTES: &[HttpRouteSpec] = OTA_ROUTE_SPECS;

/// 注册与历史 `register!` 等价的全量 URI handler。
pub(super) fn register_all_esp_routes(
    server: &mut EspHttpServer<'static>,
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    config_store: &Arc<dyn ConfigStore + Send + Sync>,
) -> Result<()> {
    let executor = EspRouteExecutor::new(ctx, env, config_store);
    register_esp_route_specs(server, ctx, env, config_store, &executor, ROOT_ROUTES)?;
    register_esp_route_specs(
        server,
        ctx,
        env,
        config_store,
        &executor,
        PAIRING_AND_CONFIG_ROUTES,
    )?;
    register_esp_route_specs(
        server,
        ctx,
        env,
        config_store,
        &executor,
        OBSERVABILITY_ROUTES,
    )?;
    register_esp_route_specs(
        server,
        ctx,
        env,
        config_store,
        &executor,
        MEMORY_AND_SKILL_ROUTES,
    )?;
    register_esp_route_specs(server, ctx, env, config_store, &executor, ACTION_ROUTES)?;
    #[cfg(feature = "ota")]
    register_esp_route_specs(server, ctx, env, config_store, &executor, OTA_ROUTES)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "ota")]
    use super::OTA_ROUTES;
    use super::{
        ACTION_ROUTES, MEMORY_AND_SKILL_ROUTES, OBSERVABILITY_ROUTES, PAIRING_AND_CONFIG_ROUTES,
        ROOT_ROUTES,
    };
    use crate::platform::http_server::router::catalog::RouteDispatchMode;
    use embedded_svc::http::Method;

    fn dispatch_mode_for(path: &str, method: Method) -> Option<RouteDispatchMode> {
        for routes in [
            ROOT_ROUTES,
            PAIRING_AND_CONFIG_ROUTES,
            OBSERVABILITY_ROUTES,
            MEMORY_AND_SKILL_ROUTES,
            ACTION_ROUTES,
        ] {
            if let Some(spec) = routes
                .iter()
                .find(|spec| spec.path == path && esp_method(spec.method) == method)
            {
                return Some(spec.dispatch_mode);
            }
        }
        #[cfg(feature = "ota")]
        if let Some(spec) = OTA_ROUTES
            .iter()
            .find(|spec| spec.path == path && esp_method(spec.method) == method)
        {
            return Some(spec.dispatch_mode);
        }
        None
    }

    #[test]
    fn lightweight_control_plane_routes_dispatch_directly() {
        assert_eq!(
            dispatch_mode_for("/api/pairing_code", Method::Get),
            Some(RouteDispatchMode::Direct)
        );
        assert_eq!(
            dispatch_mode_for("/api/csrf_token", Method::Get),
            Some(RouteDispatchMode::Direct)
        );
        assert_eq!(
            dispatch_mode_for("/api/config/system", Method::Get),
            Some(RouteDispatchMode::Direct)
        );
        assert_eq!(
            dispatch_mode_for("/api/health", Method::Get),
            Some(RouteDispatchMode::Direct)
        );
        assert_eq!(
            dispatch_mode_for("/api/metrics", Method::Get),
            Some(RouteDispatchMode::Direct)
        );
        assert_eq!(
            dispatch_mode_for("/api/operator/window", Method::Post),
            Some(RouteDispatchMode::Direct)
        );
    }

    #[test]
    fn heavy_esp_observability_routes_dispatch_on_worker_lane() {
        assert_eq!(
            dispatch_mode_for("/api/operator/status", Method::Get),
            Some(RouteDispatchMode::Worker)
        );
        assert_eq!(
            dispatch_mode_for("/api/resource", Method::Get),
            Some(RouteDispatchMode::Worker)
        );
        assert_eq!(
            dispatch_mode_for("/api/diagnose", Method::Get),
            Some(RouteDispatchMode::Worker)
        );
        assert_eq!(
            dispatch_mode_for("/api/system_info", Method::Get),
            Some(RouteDispatchMode::Worker)
        );
    }

    #[test]
    fn slow_or_external_routes_stay_on_worker_lane() {
        assert_eq!(
            dispatch_mode_for("/api/wifi/scan", Method::Get),
            Some(RouteDispatchMode::Worker)
        );
        assert_eq!(
            dispatch_mode_for("/api/hardware/discovery", Method::Get),
            Some(RouteDispatchMode::Worker)
        );
        assert_eq!(
            dispatch_mode_for("/api/channel_connectivity", Method::Get),
            Some(RouteDispatchMode::Worker)
        );
        assert_eq!(
            dispatch_mode_for("/api/skills/import", Method::Post),
            Some(RouteDispatchMode::Worker)
        );
        #[cfg(feature = "ota")]
        assert_eq!(
            dispatch_mode_for("/api/ota/check", Method::Get),
            Some(RouteDispatchMode::Worker)
        );
    }
}
