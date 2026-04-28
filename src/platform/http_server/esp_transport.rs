//! ESP HTTP 服务器薄适配：`Request` → `router::IncomingRequest` → 写回响应。
//! ESP HTTP server thin adapter: map `Request` → `router::IncomingRequest` → write response.

use crate::error::Result;
use crate::platform::http_server::api_contract;
use crate::platform::http_server::common::{
    self, ApiResponse, BodyReadError, HandlerResult, CORS_HEADERS,
};
use crate::platform::http_server::handlers::HandlerContext;
use crate::platform::http_server::lazy_executor::LazyExecutor;
use crate::platform::http_server::router::{
    self,
    catalog::{
        route_worker_memory_requirements, HttpRouteSpec, RouteBodyMode, RouteExecutionClass,
        RouteMethod, RouteRuntimeAdmission, RouteWorkerContract, RouteWorkerLane,
        ACTION_ROUTE_SPECS, MEMORY_AND_SKILL_ROUTE_SPECS, OBSERVABILITY_ROUTE_SPECS,
        PAIRING_AND_CONFIG_ROUTE_SPECS, ROOT_ROUTE_SPECS,
    },
    IncomingBody, IncomingRequest, OutgoingResponse, RestartAction, RouterEnv,
};
use crate::platform::ConfigStore;
use embedded_io::Write as _;
use embedded_svc::http::server::Request;
use embedded_svc::http::{Headers, Method};
use esp_idf_svc::http::server::Connection;
use esp_idf_svc::http::server::EspHttpServer;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{
    sync_channel, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ESP_ROUTE_EXEC_SUBMIT_ATTEMPTS: usize = 2;
const ESP_ROUTE_EXEC_POLL_INTERVAL_MS: u64 = 20;

struct EspRouteJob {
    incoming: IncomingRequest,
    reply_tx: SyncSender<OutgoingResponse>,
    enqueued_at: Instant,
    config_activity_guard: Option<crate::runtime::ConfigActivityGuard>,
}

struct EspRouteExecutorInner {
    submit_tx: SyncSender<EspRouteJob>,
    submit_gate: Arc<RouteSubmitGate>,
}

struct RouteSubmitGate {
    accepting: AtomicBool,
    mutex: Mutex<()>,
}

impl RouteSubmitGate {
    fn new() -> Self {
        Self {
            accepting: AtomicBool::new(true),
            mutex: Mutex::new(()),
        }
    }
}

#[derive(Clone)]
struct EspRouteExecutor {
    contract: RouteWorkerContract,
    inner: LazyExecutor<EspRouteExecutorInner, std::io::Error>,
}

impl EspRouteExecutor {
    fn new(
        class: RouteExecutionClass,
        ctx: &Arc<HandlerContext>,
        config_store: &Arc<dyn ConfigStore + Send + Sync>,
    ) -> Self {
        let contract = class
            .worker_contract()
            .expect("route executor requires worker contract");
        mark_route_worker_lifecycle(
            contract.lane,
            crate::runtime::PlaneLifecycleState::Registered,
            "registered",
        );
        let ctx = Arc::clone(ctx);
        let store = Arc::clone(config_store);
        Self {
            contract,
            inner: LazyExecutor::new(move || {
                let (submit_tx, rx) = sync_channel(contract.queue_capacity);
                let submit_gate = Arc::new(RouteSubmitGate::new());
                let ctx = Arc::clone(&ctx);
                let store = Arc::clone(&store);
                let worker_gate = Arc::clone(&submit_gate);
                let thread_name = route_worker_thread_name(contract.lane);
                mark_route_worker_lifecycle(
                    contract.lane,
                    crate::runtime::PlaneLifecycleState::Starting,
                    "spawn",
                );
                let spawn_result = crate::runtime::thread_util::spawn_planned_handle(
                    thread_name,
                    contract.stack_size,
                    move || {
                        run_esp_route_executor(thread_name, contract, ctx, store, rx, worker_gate)
                    },
                );
                let _task = match spawn_result {
                    Ok(task) => task,
                    Err(err) => {
                        mark_route_worker_lifecycle(
                            contract.lane,
                            crate::runtime::PlaneLifecycleState::Failed,
                            "spawn_error",
                        );
                        return Err(err);
                    }
                };
                mark_route_worker_lifecycle(
                    contract.lane,
                    crate::runtime::PlaneLifecycleState::Active,
                    "spawn_ok",
                );
                log::info!(
                    "[http_server] {} lazy-started class={:?} lane={:?} stack={} workers={} queue_cap={} timeout={}s idle_timeout={}s reject_status={} socket_reserve={} counter={}",
                    thread_name,
                    class,
                    contract.lane,
                    contract.stack_size,
                    contract.worker_threads,
                    contract.queue_capacity,
                    contract.timeout_secs,
                    contract.idle_timeout_secs,
                    contract.reject_status,
                    contract.socket_reserve,
                    contract.counter_name
                );
                crate::orchestrator::log_startup_memory_checkpoint(route_worker_spawn_stage(
                    contract.lane,
                ));
                Ok(EspRouteExecutorInner {
                    submit_tx,
                    submit_gate,
                })
            }),
        }
    }

    fn execute(
        &self,
        store: &dyn ConfigStore,
        incoming: IncomingRequest,
        spec: HttpRouteSpec,
        memory_system_kind: crate::memory::MemorySystemKind,
        mut config_activity_guard: Option<crate::runtime::ConfigActivityGuard>,
    ) -> OutgoingResponse {
        if let Some(response) = router::auth::worker_route_pre_admission_response(
            store,
            memory_system_kind,
            spec,
            incoming.uri.as_str(),
            &incoming.headers,
        ) {
            finish_config_activity_guard(&mut config_activity_guard, response.status);
            return api_response_to_outgoing(response);
        }
        if let Some(detail) = route_worker_memory_reject_detail(self.contract) {
            let out = route_worker_reject_response(
                store,
                self.contract,
                spec.path,
                "http_route_worker_admission",
                detail,
            );
            finish_config_activity_guard(&mut config_activity_guard, out.status);
            return out;
        }
        let (reply_tx, reply_rx) = sync_channel(1);
        let mut pending_job = Some(EspRouteJob {
            incoming,
            reply_tx,
            enqueued_at: Instant::now(),
            config_activity_guard,
        });
        for attempt in 0..ESP_ROUTE_EXEC_SUBMIT_ATTEMPTS {
            let inner = match self.inner.get() {
                Ok(inner) => inner,
                Err(err) => {
                    let mut job = pending_job.take();
                    let out = route_worker_reject_response(
                        store,
                        self.contract,
                        spec.path,
                        "http_route_worker_start",
                        format!("dispatch worker start failed: {}", err),
                    );
                    if let Some(job) = job.as_mut() {
                        finish_config_activity_guard(&mut job.config_activity_guard, out.status);
                    }
                    return out;
                }
            };
            let job = pending_job
                .take()
                .expect("route executor retry must keep pending job");
            let submit_guard = inner
                .submit_gate
                .mutex
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !inner.submit_gate.accepting.load(Ordering::Acquire) {
                drop(submit_guard);
                let _ = self.inner.clear_if(&inner);
                pending_job = Some(job);
                if attempt + 1 == ESP_ROUTE_EXEC_SUBMIT_ATTEMPTS {
                    let out = route_worker_reject_response(
                        store,
                        self.contract,
                        spec.path,
                        "http_route_worker_submit",
                        "dispatch worker exited before accepting job".to_string(),
                    );
                    if let Some(job) = pending_job.as_mut() {
                        finish_config_activity_guard(&mut job.config_activity_guard, out.status);
                    }
                    return out;
                }
                continue;
            }
            match inner.submit_tx.try_send(job) {
                Ok(()) => {
                    drop(submit_guard);
                    return match route_recv_timeout(
                        &reply_rx,
                        Duration::from_secs(self.contract.timeout_secs),
                    ) {
                        Ok(out) => out,
                        Err(RecvTimeoutError::Timeout) => {
                            crate::metrics::record_http_route_timeout();
                            route_worker_reject_response(
                                store,
                                self.contract,
                                spec.path,
                                "http_route_worker_wait",
                                "dispatch timed out".to_string(),
                            )
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            let _ = self.inner.clear_if(&inner);
                            mark_route_worker_lifecycle(
                                self.contract.lane,
                                crate::runtime::PlaneLifecycleState::Failed,
                                "worker_disconnected",
                            );
                            route_worker_reject_response(
                                store,
                                self.contract,
                                spec.path,
                                "http_route_worker_wait",
                                "dispatch worker stopped".to_string(),
                            )
                        }
                    };
                }
                Err(TrySendError::Disconnected(job)) => {
                    drop(submit_guard);
                    let _ = self.inner.clear_if(&inner);
                    mark_route_worker_lifecycle(
                        self.contract.lane,
                        crate::runtime::PlaneLifecycleState::Failed,
                        "worker_disconnected",
                    );
                    pending_job = Some(job);
                    if attempt + 1 == ESP_ROUTE_EXEC_SUBMIT_ATTEMPTS {
                        return route_worker_reject_response(
                            store,
                            self.contract,
                            spec.path,
                            "http_route_worker_submit",
                            "dispatch queue unavailable after worker restart".to_string(),
                        );
                    }
                }
                Err(TrySendError::Full(job)) => {
                    drop(submit_guard);
                    let mut job = job;
                    let out = route_worker_reject_response(
                        store,
                        self.contract,
                        spec.path,
                        "http_route_worker_submit",
                        "dispatch queue full".to_string(),
                    );
                    finish_config_activity_guard(&mut job.config_activity_guard, out.status);
                    return out;
                }
            }
        }
        let mut job = pending_job;
        let out = route_worker_reject_response(
            store,
            self.contract,
            spec.path,
            "http_route_worker_submit",
            "dispatch queue unavailable".to_string(),
        );
        if let Some(job) = job.as_mut() {
            finish_config_activity_guard(&mut job.config_activity_guard, out.status);
        }
        out
    }
}

fn finish_config_activity_guard(
    guard: &mut Option<crate::runtime::ConfigActivityGuard>,
    status: u16,
) {
    if let Some(guard) = guard.as_mut() {
        guard.finish_status(status);
    }
}

fn route_worker_memory_reject_detail(contract: RouteWorkerContract) -> Option<String> {
    let snap = crate::orchestrator::memory_snapshot_live();
    crate::orchestrator::apply_memory_snapshot(snap);
    let requirements = route_worker_memory_requirements(contract);
    if (snap.heap_largest_block as usize) >= requirements.required_largest
        && (snap.heap_free_internal as usize) >= requirements.required_internal
    {
        return None;
    }
    Some(format!(
        "insufficient internal heap for {:?} route worker: internal_free={} largest_block={} stack_size={} required_internal={} required_largest={}",
        contract.lane,
        snap.heap_free_internal,
        snap.heap_largest_block,
        contract.stack_size,
        requirements.required_internal,
        requirements.required_largest
    ))
}

#[derive(Clone)]
struct EspRouteExecutors {
    snapshot: EspRouteExecutor,
    config: EspRouteExecutor,
    diagnostic: EspRouteExecutor,
}

impl EspRouteExecutors {
    fn new(ctx: &Arc<HandlerContext>, config_store: &Arc<dyn ConfigStore + Send + Sync>) -> Self {
        Self {
            snapshot: EspRouteExecutor::new(RouteExecutionClass::SnapshotRoute, ctx, config_store),
            config: EspRouteExecutor::new(RouteExecutionClass::AsyncConfigRoute, ctx, config_store),
            diagnostic: EspRouteExecutor::new(
                RouteExecutionClass::SlowDiagnosticRoute,
                ctx,
                config_store,
            ),
        }
    }

    fn for_class(&self, class: RouteExecutionClass) -> Option<&EspRouteExecutor> {
        match class {
            RouteExecutionClass::ImmediateRoute | RouteExecutionClass::RejectedRoute => None,
            RouteExecutionClass::SnapshotRoute => Some(&self.snapshot),
            RouteExecutionClass::AsyncConfigRoute => Some(&self.config),
            RouteExecutionClass::SlowDiagnosticRoute => Some(&self.diagnostic),
        }
    }
}

fn route_worker_thread_name(lane: RouteWorkerLane) -> &'static str {
    match lane {
        RouteWorkerLane::Snapshot => "http_snapshot_exec",
        RouteWorkerLane::Config => "http_config_exec",
        RouteWorkerLane::Diagnostic => "http_diag_exec",
    }
}

fn route_worker_spawn_stage(lane: RouteWorkerLane) -> &'static str {
    match lane {
        RouteWorkerLane::Snapshot => "http_snapshot_exec_spawn",
        RouteWorkerLane::Config => "http_config_exec_spawn",
        RouteWorkerLane::Diagnostic => "http_diag_exec_spawn",
    }
}

fn route_worker_lifecycle_identity(
    lane: RouteWorkerLane,
) -> (crate::runtime::PlaneId, &'static str) {
    match lane {
        RouteWorkerLane::Snapshot => (crate::runtime::PlaneId::Diagnostic, "http_snapshot"),
        RouteWorkerLane::Config => (crate::runtime::PlaneId::ConfigRecovery, "http_config"),
        RouteWorkerLane::Diagnostic => (crate::runtime::PlaneId::Diagnostic, "http_diagnostic"),
    }
}

fn route_worker_lease_identity(
    lane: RouteWorkerLane,
) -> (
    crate::runtime::lease::LeaseKind,
    crate::runtime::lease::LeaseOwner,
) {
    let owner = match lane {
        RouteWorkerLane::Snapshot => "http_snapshot",
        RouteWorkerLane::Config => "http_config",
        RouteWorkerLane::Diagnostic => "http_diagnostic",
    };
    (
        lane.lease_kind(),
        crate::runtime::lease::LeaseOwner::new("http_route", owner),
    )
}

#[derive(Debug)]
struct RouteWorkerLeaseGuard {
    kind: crate::runtime::lease::LeaseKind,
    owner: crate::runtime::lease::LeaseOwner,
    token: u64,
}

impl Drop for RouteWorkerLeaseGuard {
    fn drop(&mut self) {
        let _ = crate::runtime::lease::release_token(self.kind, self.owner, self.token);
    }
}

fn acquire_route_worker_lease(
    contract: RouteWorkerContract,
) -> crate::error::Result<RouteWorkerLeaseGuard> {
    acquire_route_worker_lease_inner(contract, None)
}

#[cfg(test)]
fn acquire_route_worker_lease_at(
    contract: RouteWorkerContract,
    now_ms: u64,
) -> crate::error::Result<RouteWorkerLeaseGuard> {
    acquire_route_worker_lease_inner(contract, Some(now_ms))
}

fn acquire_route_worker_lease_inner(
    contract: RouteWorkerContract,
    now_ms: Option<u64>,
) -> crate::error::Result<RouteWorkerLeaseGuard> {
    let (kind, owner) = route_worker_lease_identity(contract.lane);
    let decision = match now_ms {
        Some(now_ms) => crate::runtime::lease::try_acquire_at(
            kind,
            owner,
            contract.lane.lease_mode(),
            None,
            now_ms,
        ),
        None => crate::runtime::lease::try_acquire(kind, owner, contract.lane.lease_mode(), None),
    };
    match decision {
        crate::runtime::lease::LeaseDecision::Acquired(record)
        | crate::runtime::lease::LeaseDecision::Reentered(record)
        | crate::runtime::lease::LeaseDecision::ReplacedExpired {
            current: record, ..
        } => Ok(RouteWorkerLeaseGuard {
            kind,
            owner,
            token: record.token,
        }),
        crate::runtime::lease::LeaseDecision::Denied(denial) => Err(crate::Error::config(
            "http_route_worker_lease",
            format!(
                "lane={:?} owner={}:{} denied reason={} held_by={:?}",
                contract.lane, owner.plane, owner.name, denial.reason, denial.held_by
            ),
        )),
    }
}

fn mark_route_worker_lifecycle(
    lane: RouteWorkerLane,
    state: crate::runtime::PlaneLifecycleState,
    reason: &'static str,
) {
    let (plane, owner) = route_worker_lifecycle_identity(lane);
    let _record = crate::runtime::plane_lifecycle::mark(plane, owner, state, reason);
}

fn status_text(status: u16) -> &'static str {
    match status {
        409 => "Conflict",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    }
}

fn route_worker_reject_response(
    _store: &dyn ConfigStore,
    contract: RouteWorkerContract,
    path: &str,
    stage: &'static str,
    detail: String,
) -> OutgoingResponse {
    crate::metrics::record_http_route_reject();
    log::warn!(
        "{}: lane={:?} path={} {}",
        stage,
        contract.lane,
        path,
        detail
    );
    let mut extra = serde_json::Map::new();
    extra.insert(
        "route_lane".to_string(),
        serde_json::Value::String(format!("{:?}", contract.lane)),
    );
    extra.insert(
        "path".to_string(),
        serde_json::Value::String(path.to_string()),
    );
    extra.insert(
        "queue_capacity".to_string(),
        serde_json::Value::from(contract.queue_capacity as u64),
    );
    extra.insert(
        "stack_size".to_string(),
        serde_json::Value::from(contract.stack_size as u64),
    );
    extra.insert(
        "socket_reserve".to_string(),
        serde_json::Value::from(contract.socket_reserve as u64),
    );
    OutgoingResponse {
        status: contract.reject_status,
        status_text: status_text(contract.reject_status),
        headers: CORS_HEADERS,
        body: ApiResponse::err_key_with_meta(
            contract.reject_status,
            status_text(contract.reject_status),
            "http.route_worker_busy",
            Some(stage),
            None,
            None,
            None,
            extra,
        )
        .body,
        restart: RestartAction::None,
    }
}

fn api_response_to_outgoing(r: ApiResponse) -> OutgoingResponse {
    OutgoingResponse::json(r.status, r.status_text, CORS_HEADERS, r.body)
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

fn execute_esp_route_job(
    contract: RouteWorkerContract,
    ctx: &Arc<HandlerContext>,
    store: &Arc<dyn ConfigStore + Send + Sync>,
    mut job: EspRouteJob,
) {
    let queue_wait = job.enqueued_at.elapsed();
    crate::metrics::record_http_route_queue_wait_ms(queue_wait.as_millis());
    log::debug!(
        "{}: lane={:?} counter={}",
        contract.begin_stage,
        contract.lane,
        contract.counter_name
    );
    crate::platform::task_wdt::feed_current_task();
    let out = {
        let _wdt_pause =
            crate::platform::esp_runtime_policy::TaskWdtSubscriptionPause::current_task();
        let route_path = job.incoming.uri.clone();
        let _route_worker_lease = match acquire_route_worker_lease(contract) {
            Ok(lease) => lease,
            Err(error) => {
                let out = route_worker_reject_response(
                    store.as_ref(),
                    contract,
                    route_path.as_str(),
                    "http_route_worker_lease",
                    error.to_string(),
                );
                finish_config_activity_guard(&mut job.config_activity_guard, out.status);
                let _ = job.reply_tx.send(out);
                crate::platform::task_wdt::feed_current_task();
                return;
            }
        };
        let handler_start = Instant::now();
        let out = match router::dispatch_without_inbound(ctx.as_ref(), job.incoming) {
            Ok(out) => out,
            Err(error) => routed_error_response(store.as_ref(), error),
        };
        crate::metrics::record_http_route_handler_ms(handler_start.elapsed().as_millis());
        out
    };
    finish_config_activity_guard(&mut job.config_activity_guard, out.status);
    let _ = job.reply_tx.send(out);
    log::debug!(
        "{}: lane={:?} counter={}",
        contract.complete_stage,
        contract.lane,
        contract.counter_name
    );
    crate::platform::task_wdt::feed_current_task();
}

fn route_recv_timeout<T>(
    rx: &Receiver<T>,
    timeout: Duration,
) -> std::result::Result<T, RecvTimeoutError> {
    // ESP-IDF's pthread timed-condvar path has crashed during lazy route-worker
    // idle waits. Polling keeps workers evictable without entering that path.
    let started = Instant::now();
    let poll_interval = Duration::from_millis(ESP_ROUTE_EXEC_POLL_INTERVAL_MS);
    loop {
        match rx.try_recv() {
            Ok(value) => return Ok(value),
            Err(TryRecvError::Disconnected) => return Err(RecvTimeoutError::Disconnected),
            Err(TryRecvError::Empty) => {}
        }

        let elapsed = started.elapsed();
        if elapsed >= timeout {
            return Err(RecvTimeoutError::Timeout);
        }
        let remaining = timeout.saturating_sub(elapsed);
        let sleep_for = if remaining < poll_interval {
            remaining
        } else {
            poll_interval
        };
        if sleep_for.is_zero() {
            std::thread::yield_now();
        } else {
            std::thread::sleep(sleep_for);
        }
    }
}

fn run_esp_route_executor(
    name: &'static str,
    contract: RouteWorkerContract,
    ctx: Arc<HandlerContext>,
    store: Arc<dyn ConfigStore + Send + Sync>,
    rx: Receiver<EspRouteJob>,
    submit_gate: Arc<RouteSubmitGate>,
) {
    let mut idle_stopped = false;
    loop {
        match route_recv_timeout(&rx, Duration::from_secs(contract.idle_timeout_secs)) {
            Ok(job) => {
                execute_esp_route_job(contract, &ctx, &store, job);
            }
            Err(RecvTimeoutError::Timeout) => {
                let _submit_guard = submit_gate
                    .mutex
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match rx.try_recv() {
                    Ok(job) => {
                        drop(_submit_guard);
                        execute_esp_route_job(contract, &ctx, &store, job);
                    }
                    Err(TryRecvError::Empty) => {
                        submit_gate.accepting.store(false, Ordering::Release);
                        mark_route_worker_lifecycle(
                            contract.lane,
                            crate::runtime::PlaneLifecycleState::Draining,
                            "idle_timeout",
                        );
                        log::info!(
                            "[http_server] {} idle-stopping lane={:?} after {}s",
                            name,
                            contract.lane,
                            contract.idle_timeout_secs
                        );
                        idle_stopped = true;
                        break;
                    }
                    Err(TryRecvError::Disconnected) => break,
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    if idle_stopped {
        mark_route_worker_lifecycle(
            contract.lane,
            crate::runtime::PlaneLifecycleState::Unloaded,
            "idle_stop",
        );
    }
    log::info!("[http_server] {} stopped", name);
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
) -> std::result::Result<IncomingBody, ApiResponse> {
    match mode {
        RouteBodyMode::None => Ok(IncomingBody::empty()),
        RouteBodyMode::Utf8(max) => {
            match common::read_body_bytes_impl(req, req.content_len(), max) {
                Ok(body) => {
                    if std::str::from_utf8(body.as_ref()).is_ok() {
                        Ok(body)
                    } else {
                        let _ = store;
                        Err(ApiResponse::err_400_key(api_contract::COMMON_INVALID_UTF8))
                    }
                }
                Err(BodyReadError::ReadFailed) => {
                    let _ = store;
                    Err(ApiResponse::err_500_key(
                        api_contract::COMMON_BODY_READ_FAILED,
                    ))
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

fn route_runtime_admission_response(
    path: &str,
    admission: RouteRuntimeAdmission,
) -> Option<ApiResponse> {
    let RouteRuntimeAdmission::Rejected {
        status,
        error_key,
        stage,
        reason,
    } = admission
    else {
        return None;
    };
    crate::metrics::record_http_route_reject();
    let mut extra = serde_json::Map::new();
    extra.insert(
        "path".to_string(),
        serde_json::Value::String(path.to_string()),
    );
    extra.insert(
        "reason".to_string(),
        serde_json::Value::String(reason.to_string()),
    );
    if error_key == "runtime.config_blocked_by_voice" {
        extra.insert(
            "retry_after_secs".to_string(),
            serde_json::Value::from(crate::runtime::CONFIG_ACTIVITY_WINDOW_SECS),
        );
    }
    Some(ApiResponse::err_key_with_meta(
        status,
        status_text(status),
        error_key,
        Some(stage),
        None,
        None,
        None,
        extra,
    ))
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
    executors: &EspRouteExecutors,
    mut req: Request<C>,
    spec: HttpRouteSpec,
) -> HandlerResult {
    let uri = req.uri().to_string();
    let restart_reason = uri.clone();
    let headers = collect_headers(&req);
    if spec.execution_class == RouteExecutionClass::RejectedRoute {
        let incoming = IncomingRequest {
            method: spec.method.as_str().to_string(),
            uri,
            headers,
            body: IncomingBody::empty(),
        };
        let out = dispatch_incoming(ctx, env, store.as_ref(), incoming);
        return write_outgoing(ctx, req, out, restart_reason.as_str());
    }
    let runtime_admission =
        spec.runtime_mode_admission(crate::runtime::thread_registry::runtime_mode_snapshot());
    if let Some(response) = route_runtime_admission_response(spec.path, runtime_admission) {
        return write_api_resp(req, response);
    }
    let mut config_read_burst_guard = spec
        .tracks_config_read_burst()
        .then(|| crate::runtime::ConfigReadBurstGuard::enter(spec.path));
    if !matches!(
        spec.execution_class,
        RouteExecutionClass::ImmediateRoute | RouteExecutionClass::RejectedRoute
    ) {
        if let Some(response) = router::auth::worker_route_pre_admission_response(
            store.as_ref(),
            ctx.platform.memory_system_kind(),
            spec,
            uri.as_str(),
            &headers,
        ) {
            return write_api_resp(req, response);
        }
    }
    let mut config_activity_guard = spec
        .config_activity_phase()
        .map(|phase| crate::runtime::ConfigActivityGuard::enter(phase, spec.path));
    let body = match read_body_esp(&mut req, store.as_ref(), spec.body_mode) {
        Ok(b) => b,
        Err(r) => {
            if let Some(guard) = config_activity_guard.as_mut() {
                guard.finish_status(r.status);
            }
            if let Some(guard) = config_read_burst_guard.as_mut() {
                guard.finish_status(r.status);
            }
            return write_api_resp(req, r);
        }
    };
    let incoming = IncomingRequest {
        method: spec.method.as_str().to_string(),
        uri,
        headers,
        body,
    };
    let out = match spec.execution_class {
        RouteExecutionClass::ImmediateRoute => {
            dispatch_incoming(ctx, env, store.as_ref(), incoming)
        }
        RouteExecutionClass::RejectedRoute => {
            unreachable!("rejected routes return before body read")
        }
        class => executors
            .for_class(class)
            .expect("worker route must have executor")
            .execute(
                store.as_ref(),
                incoming,
                spec,
                ctx.platform.memory_system_kind(),
                config_activity_guard.take(),
            ),
    };
    if let Some(guard) = config_activity_guard.as_mut() {
        guard.finish_status(out.status);
    }
    if let Some(guard) = config_read_burst_guard.as_mut() {
        guard.finish_status(out.status);
    }
    write_outgoing(ctx, req, out, restart_reason.as_str())
}

#[cold]
#[inline(never)]
fn register_esp_route(
    server: &mut EspHttpServer<'static>,
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    config_store: &Arc<dyn ConfigStore + Send + Sync>,
    executors: &EspRouteExecutors,
    spec: HttpRouteSpec,
) -> Result<()> {
    let ctx = Arc::clone(ctx);
    let env = env.clone();
    let store = Arc::clone(config_store);
    let executors = executors.clone();
    server
        .fn_handler(
            spec.path,
            esp_method(spec.method),
            move |req| -> HandlerResult {
                esp_dispatch_route(&ctx, &env, &store, &executors, req, spec)
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
    executors: &EspRouteExecutors,
    specs: &[HttpRouteSpec],
) -> Result<()> {
    for spec in specs {
        register_esp_route(server, ctx, env, config_store, executors, *spec)?;
    }
    Ok(())
}

const ROOT_ROUTES: &[HttpRouteSpec] = ROOT_ROUTE_SPECS;
const PAIRING_AND_CONFIG_ROUTES: &[HttpRouteSpec] = PAIRING_AND_CONFIG_ROUTE_SPECS;
const OBSERVABILITY_ROUTES: &[HttpRouteSpec] = OBSERVABILITY_ROUTE_SPECS;
const MEMORY_AND_SKILL_ROUTES: &[HttpRouteSpec] = MEMORY_AND_SKILL_ROUTE_SPECS;
const ACTION_ROUTES: &[HttpRouteSpec] = ACTION_ROUTE_SPECS;

/// 注册与历史 `register!` 等价的全量 URI handler。
pub(super) fn register_all_esp_routes(
    server: &mut EspHttpServer<'static>,
    ctx: &Arc<HandlerContext>,
    env: &RouterEnv,
    config_store: &Arc<dyn ConfigStore + Send + Sync>,
) -> Result<()> {
    let executors = EspRouteExecutors::new(ctx, config_store);
    register_esp_route_specs(server, ctx, env, config_store, &executors, ROOT_ROUTES)?;
    register_esp_route_specs(
        server,
        ctx,
        env,
        config_store,
        &executors,
        PAIRING_AND_CONFIG_ROUTES,
    )?;
    register_esp_route_specs(
        server,
        ctx,
        env,
        config_store,
        &executors,
        OBSERVABILITY_ROUTES,
    )?;
    register_esp_route_specs(
        server,
        ctx,
        env,
        config_store,
        &executors,
        MEMORY_AND_SKILL_ROUTES,
    )?;
    register_esp_route_specs(server, ctx, env, config_store, &executors, ACTION_ROUTES)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        EspRouteExecutor, ACTION_ROUTES, MEMORY_AND_SKILL_ROUTES, OBSERVABILITY_ROUTES,
        PAIRING_AND_CONFIG_ROUTES, ROOT_ROUTES,
    };
    use crate::platform::http_server::handlers::{
        build_default_test_handler_context, default_test_handler_context_guard,
    };
    use crate::platform::http_server::router::catalog::{RouteExecutionClass, RouteWorkerLane};
    use crate::platform::http_server::router::{IncomingBody, IncomingRequest};
    use crate::runtime::PlaneId;
    use embedded_svc::http::Method;
    use serde_json::Value;
    use std::sync::Arc;

    fn execution_class_for(path: &str, method: Method) -> Option<RouteExecutionClass> {
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
                return Some(spec.execution_class);
            }
        }
        None
    }

    #[test]
    fn lightweight_control_plane_routes_dispatch_directly() {
        assert_eq!(
            execution_class_for("/api/pairing_code", Method::Get),
            Some(RouteExecutionClass::ImmediateRoute)
        );
        assert_eq!(
            execution_class_for("/api/csrf_token", Method::Get),
            Some(RouteExecutionClass::ImmediateRoute)
        );
        assert_eq!(
            execution_class_for("/api/health", Method::Get),
            Some(RouteExecutionClass::ImmediateRoute)
        );
    }

    #[test]
    fn worker_routes_authenticate_before_memory_admission() {
        let _guard = default_test_handler_context_guard();
        let ctx = Arc::new(build_default_test_handler_context());
        let store = Arc::clone(&ctx.config_store);
        let executor = EspRouteExecutor::new(RouteExecutionClass::AsyncConfigRoute, &ctx, &store);
        let incoming = IncomingRequest {
            method: "POST".to_string(),
            uri: "/api/config/system".to_string(),
            headers: Vec::new(),
            body: IncomingBody::empty(),
        };

        let spec = crate::platform::http_server::router::catalog::route_spec_for(
            "POST",
            "/api/config/system",
        )
        .expect("config route spec");
        let out = executor.execute(
            store.as_ref(),
            incoming,
            spec,
            ctx.platform.memory_system_kind(),
            None,
        );

        assert_eq!(out.status, 401);
        let parsed: Value = serde_json::from_slice(&out.body).expect("parse auth response");
        assert_eq!(parsed["error_key"], "auth.pairing_required");
        assert_ne!(parsed["error_key"], "http.route_worker_busy");
        assert!(
            !String::from_utf8_lossy(&out.body).contains("internal_free"),
            "auth failure must not leak heap details: {}",
            String::from_utf8_lossy(&out.body)
        );
    }

    #[test]
    fn cached_config_reads_dispatch_directly_and_writes_use_config_lane() {
        assert_eq!(
            execution_class_for("/api/config/system", Method::Get),
            Some(RouteExecutionClass::ImmediateRoute)
        );
        assert_eq!(
            execution_class_for("/api/config/llm", Method::Get),
            Some(RouteExecutionClass::ImmediateRoute)
        );
        assert_eq!(
            execution_class_for("/api/config/channels", Method::Get),
            Some(RouteExecutionClass::ImmediateRoute)
        );
        assert_eq!(
            execution_class_for("/api/config/hardware", Method::Get),
            Some(RouteExecutionClass::ImmediateRoute)
        );
        assert_eq!(
            execution_class_for("/api/config/audio", Method::Get),
            Some(RouteExecutionClass::ImmediateRoute)
        );
        assert_eq!(
            execution_class_for("/api/config/display", Method::Get),
            Some(RouteExecutionClass::ImmediateRoute)
        );
        assert_eq!(
            execution_class_for("/api/config/system", Method::Post),
            Some(RouteExecutionClass::AsyncConfigRoute)
        );
        assert_eq!(
            execution_class_for("/api/config/hardware", Method::Post),
            Some(RouteExecutionClass::AsyncConfigRoute)
        );
        assert_eq!(
            execution_class_for("/api/operator/window", Method::Post),
            Some(RouteExecutionClass::ImmediateRoute)
        );
    }

    #[test]
    fn snapshot_routes_are_not_config_or_diagnostic_workers() {
        assert_eq!(
            execution_class_for("/api/channel_connectivity", Method::Get),
            Some(RouteExecutionClass::ImmediateRoute)
        );
        assert_eq!(
            execution_class_for("/api/metrics", Method::Get),
            Some(RouteExecutionClass::ImmediateRoute)
        );
    }

    #[test]
    fn skills_inventory_get_is_lightweight_on_esp_control_plane() {
        assert_eq!(
            execution_class_for("/api/skills", Method::Get),
            Some(RouteExecutionClass::SnapshotRoute)
        );
        assert_eq!(
            execution_class_for("/api/skills", Method::Post),
            Some(RouteExecutionClass::SlowDiagnosticRoute)
        );
        assert_eq!(
            execution_class_for("/api/skills", Method::Delete),
            Some(RouteExecutionClass::SlowDiagnosticRoute)
        );
    }

    #[test]
    fn esp_observability_routes_keep_crash_sensitive_paths_immediate() {
        assert_eq!(
            execution_class_for("/api/operator/status", Method::Get),
            Some(RouteExecutionClass::SlowDiagnosticRoute)
        );
        assert_eq!(
            execution_class_for("/api/resource", Method::Get),
            Some(RouteExecutionClass::ImmediateRoute)
        );
        assert_eq!(
            execution_class_for("/api/diagnose", Method::Get),
            Some(RouteExecutionClass::SlowDiagnosticRoute)
        );
        assert_eq!(
            execution_class_for("/api/system_info", Method::Get),
            Some(RouteExecutionClass::ImmediateRoute)
        );
    }

    #[test]
    fn slow_or_external_routes_stay_on_worker_lane() {
        assert_eq!(
            execution_class_for("/api/wifi/scan", Method::Get),
            Some(RouteExecutionClass::SlowDiagnosticRoute)
        );
        assert_eq!(
            execution_class_for("/api/hardware/discovery", Method::Get),
            Some(RouteExecutionClass::SlowDiagnosticRoute)
        );
        assert_eq!(
            execution_class_for("/api/channel_connectivity/refresh", Method::Post),
            Some(RouteExecutionClass::SlowDiagnosticRoute)
        );
        assert_eq!(
            execution_class_for("/api/skills/import", Method::Post),
            Some(RouteExecutionClass::SlowDiagnosticRoute)
        );
    }

    #[test]
    fn http_route_worker_lifecycle_identity_matches_route_lanes() {
        assert_eq!(
            route_worker_lifecycle_identity(RouteWorkerLane::Snapshot),
            (PlaneId::Diagnostic, "http_snapshot")
        );
        assert_eq!(
            route_worker_lifecycle_identity(RouteWorkerLane::Config),
            (PlaneId::ConfigRecovery, "http_config")
        );
        assert_eq!(
            route_worker_lifecycle_identity(RouteWorkerLane::Diagnostic),
            (PlaneId::Diagnostic, "http_diagnostic")
        );
    }

    #[test]
    fn http_route_worker_lease_identity_matches_route_lanes() {
        assert_eq!(
            route_worker_lease_identity(RouteWorkerLane::Snapshot),
            (
                crate::runtime::lease::LeaseKind::SnapshotHttpWorker,
                crate::runtime::lease::LeaseOwner::new("http_route", "http_snapshot")
            )
        );
        assert_eq!(
            route_worker_lease_identity(RouteWorkerLane::Config),
            (
                crate::runtime::lease::LeaseKind::ConfigHttpWorker,
                crate::runtime::lease::LeaseOwner::new("http_route", "http_config")
            )
        );
        assert_eq!(
            route_worker_lease_identity(RouteWorkerLane::Diagnostic),
            (
                crate::runtime::lease::LeaseKind::DiagnosticHttpWorker,
                crate::runtime::lease::LeaseOwner::new("http_route", "http_diagnostic")
            )
        );
    }

    #[test]
    fn route_worker_lease_guard_holds_and_releases_lane_resource() {
        let _guard = crate::runtime::lease::lease_test_guard();
        let contract = RouteExecutionClass::AsyncConfigRoute
            .worker_contract()
            .expect("config worker contract");

        let lease = acquire_route_worker_lease_at(contract, 100).expect("config worker lease");
        assert_eq!(
            crate::runtime::lease::active_count_for_kind_at(
                crate::runtime::lease::LeaseKind::ConfigHttpWorker,
                101
            ),
            1
        );

        drop(lease);
        assert_eq!(
            crate::runtime::lease::active_count_for_kind_at(
                crate::runtime::lease::LeaseKind::ConfigHttpWorker,
                102
            ),
            0
        );
    }

    #[test]
    fn route_worker_lease_denies_same_lane_second_owner() {
        let _guard = crate::runtime::lease::lease_test_guard();
        let contract = RouteExecutionClass::SlowDiagnosticRoute
            .worker_contract()
            .expect("diagnostic worker contract");

        let _first = acquire_route_worker_lease_at(contract, 100).expect("first diag lease");
        let error = match acquire_route_worker_lease_at(contract, 101) {
            Ok(_) => panic!("second diagnostic lease should conflict"),
            Err(error) => error,
        };

        assert_eq!(error.stage(), "http_route_worker_lease");
        assert!(error.to_string().contains("exclusive_conflict"));
    }
}
