//! Shared HTTP route catalog used by transport registration and dispatch.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteMethod {
    Get,
    Post,
    Delete,
    Options,
}

impl RouteMethod {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
        }
    }

    pub(crate) fn parse(method: &str) -> Option<Self> {
        match method {
            "GET" => Some(Self::Get),
            "POST" => Some(Self::Post),
            "DELETE" => Some(Self::Delete),
            "OPTIONS" => Some(Self::Options),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteBodyMode {
    None,
    Utf8(usize),
}

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteExecutionClass {
    ImmediateRoute,
    StreamingRoute,
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
    SnapshotRoute,
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
    ChatHistoryRoute,
    AsyncConfigRoute,
    LocalDiagnosticRoute,
    SlowDiagnosticRoute,
    RejectedRoute,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteWorkerLane {
    Snapshot,
    ChatHistory,
    Config,
    Diagnostic,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
impl RouteWorkerLane {
    pub(crate) const fn lease_kind(self) -> crate::runtime::lease::LeaseKind {
        match self {
            Self::Snapshot => crate::runtime::lease::LeaseKind::SnapshotHttpWorker,
            Self::ChatHistory => crate::runtime::lease::LeaseKind::ChatHistoryHttpWorker,
            Self::Config => crate::runtime::lease::LeaseKind::ConfigHttpWorker,
            Self::Diagnostic => crate::runtime::lease::LeaseKind::DiagnosticHttpWorker,
        }
    }

    pub(crate) const fn lease_mode(self) -> crate::runtime::lease::LeaseMode {
        let _ = self;
        crate::runtime::lease::LeaseMode::Exclusive
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteWorkerContract {
    pub(crate) lane: RouteWorkerLane,
    pub(crate) stack_size: usize,
    pub(crate) reserves_tls_headroom: bool,
    pub(crate) queue_capacity: usize,
    pub(crate) worker_threads: usize,
    pub(crate) timeout_secs: u64,
    pub(crate) idle_timeout_secs: u64,
    pub(crate) reject_status: u16,
    pub(crate) socket_reserve: usize,
    pub(crate) counter_name: &'static str,
    pub(crate) begin_stage: &'static str,
    pub(crate) complete_stage: &'static str,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteWorkerMemoryRequirements {
    pub(crate) required_internal: usize,
    pub(crate) required_largest: usize,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
const ROUTE_WORKER_NON_TLS_INTERNAL_HEADROOM: usize = 8 * 1024;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
const ROUTE_WORKER_NON_TLS_LARGEST_HEADROOM: usize = 3 * 1024;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
const ROUTE_WORKER_CONFIG_LARGEST_HEADROOM: usize = 0;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
pub(crate) const fn route_worker_memory_requirements(
    contract: RouteWorkerContract,
) -> RouteWorkerMemoryRequirements {
    let (internal_headroom, largest_headroom) = if contract.reserves_tls_headroom {
        (
            crate::constants::TLS_ADMISSION_MIN_INTERNAL_BYTES,
            crate::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES,
        )
    } else if matches!(
        contract.lane,
        RouteWorkerLane::ChatHistory | RouteWorkerLane::Config
    ) {
        (
            ROUTE_WORKER_NON_TLS_INTERNAL_HEADROOM,
            ROUTE_WORKER_CONFIG_LARGEST_HEADROOM,
        )
    } else {
        (
            ROUTE_WORKER_NON_TLS_INTERNAL_HEADROOM,
            ROUTE_WORKER_NON_TLS_LARGEST_HEADROOM,
        )
    };
    RouteWorkerMemoryRequirements {
        required_internal: contract.stack_size.saturating_add(internal_headroom),
        required_largest: contract.stack_size.saturating_add(largest_headroom),
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
#[derive(Clone, Copy)]
pub(crate) struct RouteWorkerRuntimeLoad {
    pub(crate) pressure: crate::orchestrator::pressure::PressureLevel,
    pub(crate) storage_contention: crate::orchestrator::StorageContentionRisk,
    pub(crate) active_agent_tasks: u32,
    pub(crate) inbound_depth: u32,
    pub(crate) outbound_depth: u32,
    pub(crate) scheduler_decision: crate::runtime::RuntimeWorkDecision,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
impl RouteWorkerRuntimeLoad {
    pub(crate) fn with_scheduler_decision(
        mut self,
        decision: crate::runtime::RuntimeWorkDecision,
    ) -> Self {
        self.scheduler_decision = decision;
        self
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
impl From<&crate::orchestrator::ResourceSnapshot> for RouteWorkerRuntimeLoad {
    fn from(snapshot: &crate::orchestrator::ResourceSnapshot) -> Self {
        Self {
            pressure: snapshot.pressure,
            storage_contention: snapshot.storage_contention_risk,
            active_agent_tasks: snapshot.active_agent_tasks,
            inbound_depth: snapshot.inbound_depth,
            outbound_depth: snapshot.outbound_depth,
            scheduler_decision: crate::runtime::RuntimeWorkDecision::Proceed,
        }
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
impl From<&crate::orchestrator::ResourceLightSnapshot> for RouteWorkerRuntimeLoad {
    fn from(snapshot: &crate::orchestrator::ResourceLightSnapshot) -> Self {
        Self {
            pressure: snapshot.pressure,
            storage_contention: snapshot.storage_contention_risk,
            active_agent_tasks: snapshot.active_agent_tasks,
            inbound_depth: snapshot.inbound_depth,
            outbound_depth: snapshot.outbound_depth,
            scheduler_decision: crate::runtime::RuntimeWorkDecision::Proceed,
        }
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
pub(crate) fn route_worker_runtime_busy_detail(
    contract: RouteWorkerContract,
    load: RouteWorkerRuntimeLoad,
) -> Option<String> {
    if let Some(detail) = route_worker_scheduler_busy_detail(contract, load.scheduler_decision) {
        return Some(detail);
    }
    let pressure_allows_worker = load.pressure
        == crate::orchestrator::pressure::PressureLevel::Normal
        || (contract.lane == RouteWorkerLane::ChatHistory
            && load.pressure == crate::orchestrator::pressure::PressureLevel::Cautious);
    if !pressure_allows_worker {
        return Some(format!(
            "route worker start deferred for {:?}: pressure={:?}",
            contract.lane, load.pressure
        ));
    }
    if load.storage_contention != crate::orchestrator::StorageContentionRisk::Healthy {
        return Some(format!(
            "route worker start deferred for {:?}: storage_contention={:?}",
            contract.lane, load.storage_contention
        ));
    }
    if load.active_agent_tasks > 0 || load.inbound_depth > 0 || load.outbound_depth > 0 {
        return Some(format!(
            "route worker start deferred for {:?}: active_agent_tasks={} inbound_depth={} outbound_depth={}",
            contract.lane, load.active_agent_tasks, load.inbound_depth, load.outbound_depth
        ));
    }
    None
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
fn route_worker_scheduler_busy_detail(
    contract: RouteWorkerContract,
    decision: crate::runtime::RuntimeWorkDecision,
) -> Option<String> {
    match decision {
        crate::runtime::RuntimeWorkDecision::Proceed => None,
        crate::runtime::RuntimeWorkDecision::Defer {
            reason,
            retry_after_ms,
        } => Some(format!(
            "route worker start deferred for {:?}: runtime_scheduler=defer reason={} retry_after_ms={}",
            contract.lane, reason, retry_after_ms
        )),
        crate::runtime::RuntimeWorkDecision::DrainAndResume {
            reason,
            retry_after_ms,
        } => Some(format!(
            "route worker start deferred for {:?}: runtime_scheduler=drain_and_resume reason={} retry_after_ms={}",
            contract.lane, reason, retry_after_ms
        )),
        crate::runtime::RuntimeWorkDecision::Degrade { reason } => Some(format!(
            "route worker start deferred for {:?}: runtime_scheduler=degrade reason={}",
            contract.lane, reason
        )),
        crate::runtime::RuntimeWorkDecision::Suspend { reason } => Some(format!(
            "route worker start deferred for {:?}: runtime_scheduler=suspend reason={}",
            contract.lane, reason
        )),
        crate::runtime::RuntimeWorkDecision::RejectWithStableKey { key, reason } => {
            Some(format!(
                "route worker start deferred for {:?}: runtime_scheduler=reject key={} reason={}",
                contract.lane, key, reason
            ))
        }
        crate::runtime::RuntimeWorkDecision::RejectWithUserVisibleReason { reason } => {
            Some(format!(
                "route worker start deferred for {:?}: runtime_scheduler=reject reason={}",
                contract.lane, reason
            ))
        }
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
impl RouteExecutionClass {
    pub(crate) const fn runtime_work_class(self) -> Option<crate::runtime::RuntimeWorkClass> {
        match self {
            Self::ImmediateRoute | Self::StreamingRoute | Self::RejectedRoute => None,
            Self::ChatHistoryRoute => {
                Some(crate::runtime::RuntimeWorkClass::ConfigUiChatHistoryRoute)
            }
            Self::SnapshotRoute
            | Self::AsyncConfigRoute
            | Self::LocalDiagnosticRoute
            | Self::SlowDiagnosticRoute => Some(crate::runtime::RuntimeWorkClass::DeepRouteWorker),
        }
    }

    pub(crate) const fn requires_route_worker_runtime_busy_admission(self) -> bool {
        !matches!(self, Self::AsyncConfigRoute)
    }

    pub(crate) const fn worker_contract(self) -> Option<RouteWorkerContract> {
        match self {
            Self::ImmediateRoute | Self::StreamingRoute | Self::RejectedRoute => None,
            Self::SnapshotRoute => Some(RouteWorkerContract {
                lane: RouteWorkerLane::Snapshot,
                stack_size: crate::util::STACK_HTTP_SNAPSHOT_WORKER,
                reserves_tls_headroom: false,
                queue_capacity: 2,
                worker_threads: 1,
                timeout_secs: 5,
                idle_timeout_secs: 5,
                reject_status: 503,
                socket_reserve: 0,
                counter_name: "http_snapshot_worker",
                begin_stage: "http_snapshot_begin",
                complete_stage: "http_snapshot_complete",
            }),
            Self::ChatHistoryRoute => Some(RouteWorkerContract {
                lane: RouteWorkerLane::ChatHistory,
                stack_size: crate::util::STACK_HTTP_CHAT_HISTORY_WORKER,
                reserves_tls_headroom: false,
                queue_capacity: 2,
                worker_threads: 1,
                timeout_secs: 5,
                idle_timeout_secs: 5,
                reject_status: 503,
                socket_reserve: 0,
                counter_name: "http_chat_history_worker",
                begin_stage: "http_chat_history_begin",
                complete_stage: "http_chat_history_complete",
            }),
            Self::AsyncConfigRoute => Some(RouteWorkerContract {
                lane: RouteWorkerLane::Config,
                stack_size: crate::util::STACK_HTTP_CONFIG_WORKER,
                reserves_tls_headroom: false,
                queue_capacity: 2,
                worker_threads: 1,
                timeout_secs: 20,
                idle_timeout_secs: 10,
                reject_status: 503,
                socket_reserve: 3,
                counter_name: "http_config_worker",
                begin_stage: "http_config_begin",
                complete_stage: "http_config_complete",
            }),
            Self::LocalDiagnosticRoute => Some(RouteWorkerContract {
                lane: RouteWorkerLane::Diagnostic,
                stack_size: crate::util::STACK_HTTP_DIAG_WORKER,
                reserves_tls_headroom: false,
                queue_capacity: 2,
                worker_threads: 1,
                timeout_secs: 15,
                idle_timeout_secs: 8,
                reject_status: 503,
                socket_reserve: 0,
                counter_name: "http_local_diagnostic_worker",
                begin_stage: "http_local_diagnostic_begin",
                complete_stage: "http_local_diagnostic_complete",
            }),
            Self::SlowDiagnosticRoute => Some(RouteWorkerContract {
                lane: RouteWorkerLane::Diagnostic,
                stack_size: crate::util::STACK_HTTP_DIAG_WORKER,
                reserves_tls_headroom: true,
                queue_capacity: 2,
                worker_threads: 1,
                timeout_secs: 15,
                idle_timeout_secs: 8,
                reject_status: 503,
                socket_reserve: 3,
                counter_name: "http_diagnostic_worker",
                begin_stage: "http_diagnostic_begin",
                complete_stage: "http_diagnostic_complete",
            }),
        }
    }

    pub(crate) fn requires_route_worker_transport_admission(
        self,
        _mode: crate::runtime::RuntimeModeSnapshot,
    ) -> bool {
        !matches!(self, Self::AsyncConfigRoute)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OperatorRouteAccess {
    Hidden,
    AlwaysOn,
    Windowed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteHandler {
    RootGet,
    PairingCodeGet,
    PairingCodePost,
    ConfigLlmGet,
    ConfigLlmPost,
    ConfigChannelsGet,
    ConfigChannelsPost,
    ConfigSystemGet,
    ConfigSystemPost,
    ConfigHardwareGet,
    ConfigHardwarePost,
    ConfigAudioGet,
    ConfigAudioPost,
    ConfigDisplayGet,
    ConfigDisplayPost,
    WifiScanGet,
    HardwareDiscoveryGet,
    HealthGet,
    OperatorStatusGet,
    OperatorWindowPost,
    MetricsGet,
    ResourceGet,
    DiagnoseGet,
    SystemInfoGet,
    ChannelConnectivityGet,
    ToolsGet,
    SessionsGet,
    SessionsPost,
    SessionsDelete,
    MemoryStatusGet,
    MemoryMaintenancePost,
    CapabilityPackagesGet,
    CapabilityPackagesPost,
    SkillsGet,
    SkillsPost,
    SkillsDelete,
    SkillsImportPost,
    RestartPost,
    ConfigResetPost,
    WebhookPost,
    CsrfTokenGet,
}

#[cfg_attr(
    not(any(target_arch = "xtensa", target_arch = "riscv32", test)),
    allow(dead_code)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteRuntimeAdmission {
    Allowed,
    Rejected {
        status: u16,
        error_key: &'static str,
        stage: &'static str,
        reason: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HttpRouteSpec {
    pub(crate) path: &'static str,
    pub(crate) method: RouteMethod,
    pub(crate) body_mode: RouteBodyMode,
    pub(crate) handler: Option<RouteHandler>,
    pub(crate) execution_class: RouteExecutionClass,
    pub(crate) operator_access: OperatorRouteAccess,
    config_activity_phase: Option<crate::runtime::ConfigActivityPhase>,
    reject_during_voice_exclusive: bool,
}

impl HttpRouteSpec {
    pub(crate) const fn immediate(
        path: &'static str,
        method: RouteMethod,
        body_mode: RouteBodyMode,
    ) -> Self {
        Self {
            path,
            method,
            body_mode,
            handler: None,
            execution_class: RouteExecutionClass::ImmediateRoute,
            operator_access: OperatorRouteAccess::Hidden,
            config_activity_phase: None,
            reject_during_voice_exclusive: false,
        }
    }

    pub(crate) const fn immediate_operator(
        path: &'static str,
        method: RouteMethod,
        body_mode: RouteBodyMode,
        operator_access: OperatorRouteAccess,
    ) -> Self {
        Self {
            path,
            method,
            body_mode,
            handler: None,
            execution_class: RouteExecutionClass::ImmediateRoute,
            operator_access,
            config_activity_phase: None,
            reject_during_voice_exclusive: false,
        }
    }

    pub(crate) const fn streaming_operator(
        path: &'static str,
        method: RouteMethod,
        body_mode: RouteBodyMode,
        operator_access: OperatorRouteAccess,
    ) -> Self {
        Self {
            path,
            method,
            body_mode,
            handler: None,
            execution_class: RouteExecutionClass::StreamingRoute,
            operator_access,
            config_activity_phase: None,
            reject_during_voice_exclusive: false,
        }
    }

    pub(crate) const fn async_config_operator(
        path: &'static str,
        method: RouteMethod,
        body_mode: RouteBodyMode,
        operator_access: OperatorRouteAccess,
    ) -> Self {
        Self {
            path,
            method,
            body_mode,
            handler: None,
            execution_class: RouteExecutionClass::AsyncConfigRoute,
            operator_access,
            config_activity_phase: None,
            reject_during_voice_exclusive: false,
        }
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
    pub(crate) const fn snapshot_operator(
        path: &'static str,
        method: RouteMethod,
        body_mode: RouteBodyMode,
        operator_access: OperatorRouteAccess,
    ) -> Self {
        Self {
            path,
            method,
            body_mode,
            handler: None,
            execution_class: RouteExecutionClass::SnapshotRoute,
            operator_access,
            config_activity_phase: None,
            reject_during_voice_exclusive: false,
        }
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
    pub(crate) const fn chat_history_operator(
        path: &'static str,
        method: RouteMethod,
        body_mode: RouteBodyMode,
        operator_access: OperatorRouteAccess,
    ) -> Self {
        Self {
            path,
            method,
            body_mode,
            handler: None,
            execution_class: RouteExecutionClass::ChatHistoryRoute,
            operator_access,
            config_activity_phase: None,
            reject_during_voice_exclusive: false,
        }
    }

    pub(crate) const fn slow_diagnostic_operator(
        path: &'static str,
        method: RouteMethod,
        body_mode: RouteBodyMode,
        operator_access: OperatorRouteAccess,
    ) -> Self {
        Self {
            path,
            method,
            body_mode,
            handler: None,
            execution_class: RouteExecutionClass::SlowDiagnosticRoute,
            operator_access,
            config_activity_phase: None,
            reject_during_voice_exclusive: false,
        }
    }

    pub(crate) const fn local_diagnostic_operator(
        path: &'static str,
        method: RouteMethod,
        body_mode: RouteBodyMode,
        operator_access: OperatorRouteAccess,
    ) -> Self {
        Self {
            path,
            method,
            body_mode,
            handler: None,
            execution_class: RouteExecutionClass::LocalDiagnosticRoute,
            operator_access,
            config_activity_phase: None,
            reject_during_voice_exclusive: false,
        }
    }

    #[cfg_attr(feature = "capability_office", allow(dead_code))]
    pub(crate) const fn rejected(path: &'static str, method: RouteMethod) -> Self {
        Self {
            path,
            method,
            body_mode: RouteBodyMode::None,
            handler: None,
            execution_class: RouteExecutionClass::RejectedRoute,
            operator_access: OperatorRouteAccess::Hidden,
            config_activity_phase: None,
            reject_during_voice_exclusive: false,
        }
    }

    pub(crate) const fn with_handler(mut self, handler: RouteHandler) -> Self {
        self.handler = Some(handler);
        self
    }

    #[cfg_attr(
        not(any(target_arch = "xtensa", target_arch = "riscv32", test)),
        allow(dead_code)
    )]
    pub(crate) const fn config_activity_phase(self) -> Option<crate::runtime::ConfigActivityPhase> {
        self.config_activity_phase
    }

    #[cfg(test)]
    pub(crate) const fn rejects_during_voice_exclusive(self) -> bool {
        self.reject_during_voice_exclusive
    }

    pub(crate) fn handler(self) -> Option<RouteHandler> {
        self.handler
    }

    #[cfg_attr(
        not(any(target_arch = "xtensa", target_arch = "riscv32", test)),
        allow(dead_code)
    )]
    pub(crate) fn tracks_config_read_burst(self) -> bool {
        matches!(
            (self.method, self.path, self.execution_class),
            (
                RouteMethod::Get,
                ROUTE_CONFIG_LLM,
                RouteExecutionClass::ImmediateRoute
            ) | (
                RouteMethod::Get,
                ROUTE_CONFIG_CHANNELS,
                RouteExecutionClass::ImmediateRoute
            ) | (
                RouteMethod::Get,
                ROUTE_CONFIG_SYSTEM,
                RouteExecutionClass::ImmediateRoute
            ) | (
                RouteMethod::Get,
                ROUTE_CONFIG_HARDWARE,
                RouteExecutionClass::ImmediateRoute
            ) | (
                RouteMethod::Get,
                ROUTE_CONFIG_AUDIO,
                RouteExecutionClass::ImmediateRoute
            ) | (
                RouteMethod::Get,
                ROUTE_CONFIG_DISPLAY,
                RouteExecutionClass::ImmediateRoute
            )
        )
    }

    #[cfg_attr(
        not(any(target_arch = "xtensa", target_arch = "riscv32", test)),
        allow(dead_code)
    )]
    pub(crate) fn uses_response_build_admission(self) -> bool {
        matches!(
            (self.method, self.path),
            (
                RouteMethod::Get,
                ROUTE_SESSIONS | ROUTE_MEMORY_STATUS | ROUTE_DIAGNOSE | ROUTE_SYSTEM_INFO
            )
        )
    }

    pub(crate) const fn with_config_activity(
        mut self,
        phase: crate::runtime::ConfigActivityPhase,
        reject_during_voice_exclusive: bool,
    ) -> Self {
        self.config_activity_phase = Some(phase);
        self.reject_during_voice_exclusive = reject_during_voice_exclusive;
        self
    }

    pub(crate) const fn with_voice_exclusive_reject(mut self) -> Self {
        self.reject_during_voice_exclusive = true;
        self
    }

    #[cfg_attr(
        not(any(target_arch = "xtensa", target_arch = "riscv32", test)),
        allow(dead_code)
    )]
    pub(crate) fn runtime_mode_admission(
        self,
        mode: crate::runtime::RuntimeModeSnapshot,
    ) -> RouteRuntimeAdmission {
        match mode.current_mode {
            crate::runtime::RuntimeMode::RecoverySafeMode => {
                if self.allowed_in_recovery_safe_mode() {
                    RouteRuntimeAdmission::Allowed
                } else {
                    RouteRuntimeAdmission::Rejected {
                        status: 503,
                        error_key: "runtime.route_blocked_by_recovery_safe_mode",
                        stage: "runtime_route_admission",
                        reason: "recovery_safe_mode_allowlist",
                    }
                }
            }
            crate::runtime::RuntimeMode::VoiceExclusive => {
                if self.reject_during_voice_exclusive {
                    RouteRuntimeAdmission::Rejected {
                        status: 409,
                        error_key: "runtime.config_blocked_by_voice",
                        stage: "config_activity_admission",
                        reason: "voice_exclusive_route_suspend",
                    }
                } else {
                    RouteRuntimeAdmission::Allowed
                }
            }
            crate::runtime::RuntimeMode::ConfigActive => {
                if self.blocks_during_config_active() {
                    RouteRuntimeAdmission::Rejected {
                        status: 409,
                        error_key: "runtime.route_blocked_by_config_active",
                        stage: "runtime_route_admission",
                        reason: "config_active_diagnostic_not_admitted",
                    }
                } else {
                    RouteRuntimeAdmission::Allowed
                }
            }
            _ => RouteRuntimeAdmission::Allowed,
        }
    }

    #[cfg_attr(
        not(any(target_arch = "xtensa", target_arch = "riscv32", test)),
        allow(dead_code)
    )]
    fn blocks_during_config_active(self) -> bool {
        self.execution_class == RouteExecutionClass::SlowDiagnosticRoute
            && self.config_activity_phase != Some(crate::runtime::ConfigActivityPhase::Active)
    }

    #[cfg_attr(
        not(any(target_arch = "xtensa", target_arch = "riscv32", test)),
        allow(dead_code)
    )]
    fn allowed_in_recovery_safe_mode(self) -> bool {
        self.method == RouteMethod::Options
            || matches!(
                (self.method, self.path),
                (RouteMethod::Get, ROUTE_ROOT)
                    | (RouteMethod::Get, ROUTE_HEALTH)
                    | (RouteMethod::Get, ROUTE_CSRF_TOKEN)
                    | (RouteMethod::Post, ROUTE_CONFIG_RESET)
            )
    }
}

fn route_spec_groups() -> &'static [&'static [HttpRouteSpec]] {
    &[
        ROOT_ROUTE_SPECS,
        PAIRING_AND_CONFIG_ROUTE_SPECS,
        OBSERVABILITY_ROUTE_SPECS,
        MEMORY_AND_SKILL_ROUTE_SPECS,
        ACTION_ROUTE_SPECS,
    ]
}

pub(crate) fn operator_route_endpoints(
    access: Option<OperatorRouteAccess>,
    inbound_webhooks_enabled: bool,
) -> Vec<String> {
    let mut endpoints = Vec::new();
    push_operator_route_endpoints(&mut endpoints, ROOT_ROUTE_SPECS, access);
    push_operator_route_endpoints(&mut endpoints, PAIRING_AND_CONFIG_ROUTE_SPECS, access);
    push_operator_route_endpoints(&mut endpoints, OBSERVABILITY_ROUTE_SPECS, access);
    push_operator_route_endpoints(&mut endpoints, MEMORY_AND_SKILL_ROUTE_SPECS, access);
    push_operator_route_endpoints(&mut endpoints, ACTION_ROUTE_SPECS, access);
    if access.is_none() && inbound_webhooks_enabled {
        endpoints.push(format!("{} {}", RouteMethod::Post.as_str(), ROUTE_WEBHOOK));
    }
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    if access.is_none() {
        push_host_dynamic_operator_route_endpoints(&mut endpoints);
    }
    endpoints
}

fn push_operator_route_endpoints(
    endpoints: &mut Vec<String>,
    specs: &[HttpRouteSpec],
    access: Option<OperatorRouteAccess>,
) {
    for spec in specs {
        if spec.method == RouteMethod::Options {
            continue;
        }
        match access {
            Some(required) if spec.operator_access != required => continue,
            None if spec.operator_access == OperatorRouteAccess::Hidden => continue,
            _ => {}
        }
        endpoints.push(format!("{} {}", spec.method.as_str(), spec.path));
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn push_host_dynamic_operator_route_endpoints(endpoints: &mut Vec<String>) {
    for (method, path) in [
        (RouteMethod::Get, ROUTE_CONFIG_ACCOUNTS),
        (RouteMethod::Post, ROUTE_CONFIG_ACCOUNTS),
        (RouteMethod::Get, "/api/config/accounts/:account_key"),
        (RouteMethod::Delete, "/api/config/accounts/:account_key"),
        (
            RouteMethod::Post,
            "/api/config/accounts/:account_key/config",
        ),
        (RouteMethod::Post, "/api/config/accounts/:account_key/probe"),
        (
            RouteMethod::Post,
            "/api/config/accounts/:account_key/revoke",
        ),
        (RouteMethod::Get, ROUTE_CONFIG_CAPABILITIES),
        (RouteMethod::Get, "/api/config/capabilities/:capability"),
        (RouteMethod::Get, ROUTE_CONFIG_PROVIDERS),
    ] {
        endpoints.push(format!("{} {}", method.as_str(), path));
    }
}

pub(crate) const ROUTE_ROOT: &str = "/";
pub(crate) const ROUTE_PAIRING_CODE: &str = "/api/pairing_code";
pub(crate) const ROUTE_CONFIG_LLM: &str = "/api/config/llm";
pub(crate) const ROUTE_CONFIG_CHANNELS: &str = "/api/config/channels";
pub(crate) const ROUTE_CONFIG_SYSTEM: &str = "/api/config/system";
pub(crate) const ROUTE_CONFIG_HARDWARE: &str = "/api/config/hardware";
pub(crate) const ROUTE_CONFIG_AUDIO: &str = "/api/config/audio";
pub(crate) const ROUTE_CONFIG_DISPLAY: &str = "/api/config/display";
pub(crate) const ROUTE_CONFIG_ACCOUNTS: &str = "/api/config/accounts";
pub(crate) const ROUTE_CONFIG_ACCOUNTS_PREFIX: &str = "/api/config/accounts/";
pub(crate) const ROUTE_CONFIG_CAPABILITIES: &str = "/api/config/capabilities";
pub(crate) const ROUTE_CONFIG_CAPABILITIES_PREFIX: &str = "/api/config/capabilities/";
pub(crate) const ROUTE_CONFIG_PROVIDERS: &str = "/api/config/providers";
pub(crate) const ROUTE_WIFI_SCAN: &str = "/api/wifi/scan";
pub(crate) const ROUTE_HARDWARE_DISCOVERY: &str = "/api/hardware/discovery";
pub(crate) const ROUTE_CSRF_TOKEN: &str = "/api/csrf_token";
pub(crate) const ROUTE_HEALTH: &str = "/api/health";
pub(crate) const ROUTE_OPERATOR_STATUS: &str = "/api/operator/status";
pub(crate) const ROUTE_OPERATOR_WINDOW: &str = "/api/operator/window";
pub(crate) const ROUTE_METRICS: &str = "/api/metrics";
pub(crate) const ROUTE_RESOURCE: &str = "/api/resource";
pub(crate) const ROUTE_DIAGNOSE: &str = "/api/diagnose";
pub(crate) const ROUTE_SYSTEM_INFO: &str = "/api/system_info";
pub(crate) const ROUTE_CHANNEL_CONNECTIVITY: &str = "/api/channel_connectivity";
pub(crate) const ROUTE_TOOLS: &str = "/api/tools";
pub(crate) const ROUTE_SESSIONS: &str = "/api/sessions";
pub(crate) const ROUTE_MEMORY_STATUS: &str = "/api/memory/status";
pub(crate) const ROUTE_MEMORY_MAINTENANCE: &str = "/api/memory/maintenance";
pub(crate) const ROUTE_CAPABILITY_PACKAGES: &str = "/api/capability_packages";
pub(crate) const ROUTE_SKILLS: &str = "/api/skills";
pub(crate) const ROUTE_SKILLS_IMPORT: &str = "/api/skills/import";
pub(crate) const ROUTE_RESTART: &str = "/api/restart";
pub(crate) const ROUTE_CONFIG_RESET: &str = "/api/config_reset";
pub(crate) const ROUTE_WEBHOOK: &str = "/api/webhook";

pub(crate) const ROOT_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec::immediate(ROUTE_ROOT, RouteMethod::Get, RouteBodyMode::None)
        .with_handler(RouteHandler::RootGet),
    HttpRouteSpec::immediate(ROUTE_ROOT, RouteMethod::Options, RouteBodyMode::None),
];

pub(crate) const PAIRING_AND_CONFIG_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec::immediate_operator(
        ROUTE_PAIRING_CODE,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::PairingCodeGet),
    HttpRouteSpec::immediate_operator(
        ROUTE_PAIRING_CODE,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::PairingCodePost)
    .with_config_activity(crate::runtime::ConfigActivityPhase::Persisting, false),
    HttpRouteSpec::immediate(
        ROUTE_PAIRING_CODE,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::immediate_operator(
        ROUTE_CONFIG_LLM,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ConfigLlmGet),
    HttpRouteSpec::immediate(ROUTE_CONFIG_LLM, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::async_config_operator(
        ROUTE_CONFIG_LLM,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ConfigLlmPost)
    .with_config_activity(crate::runtime::ConfigActivityPhase::Persisting, true),
    HttpRouteSpec::immediate_operator(
        ROUTE_CONFIG_CHANNELS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ConfigChannelsGet),
    HttpRouteSpec::immediate(
        ROUTE_CONFIG_CHANNELS,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::async_config_operator(
        ROUTE_CONFIG_CHANNELS,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ConfigChannelsPost)
    .with_config_activity(crate::runtime::ConfigActivityPhase::Persisting, true),
    HttpRouteSpec::immediate_operator(
        ROUTE_CONFIG_SYSTEM,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ConfigSystemGet),
    HttpRouteSpec::immediate(
        ROUTE_CONFIG_SYSTEM,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::async_config_operator(
        ROUTE_CONFIG_SYSTEM,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ConfigSystemPost)
    .with_config_activity(crate::runtime::ConfigActivityPhase::Persisting, true),
    HttpRouteSpec::immediate_operator(
        ROUTE_CONFIG_HARDWARE,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ConfigHardwareGet),
    HttpRouteSpec::immediate(
        ROUTE_CONFIG_HARDWARE,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::async_config_operator(
        ROUTE_CONFIG_HARDWARE,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ConfigHardwarePost)
    .with_config_activity(crate::runtime::ConfigActivityPhase::Persisting, true),
    HttpRouteSpec::immediate_operator(
        ROUTE_CONFIG_AUDIO,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ConfigAudioGet),
    HttpRouteSpec::immediate(
        ROUTE_CONFIG_AUDIO,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::async_config_operator(
        ROUTE_CONFIG_AUDIO,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ConfigAudioPost)
    .with_config_activity(crate::runtime::ConfigActivityPhase::Persisting, true),
    HttpRouteSpec::immediate_operator(
        ROUTE_CONFIG_DISPLAY,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ConfigDisplayGet),
    HttpRouteSpec::immediate(
        ROUTE_CONFIG_DISPLAY,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::async_config_operator(
        ROUTE_CONFIG_DISPLAY,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ConfigDisplayPost)
    .with_config_activity(crate::runtime::ConfigActivityPhase::Persisting, true),
    HttpRouteSpec::local_diagnostic_operator(
        ROUTE_WIFI_SCAN,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::WifiScanGet)
    .with_config_activity(crate::runtime::ConfigActivityPhase::Active, true),
    HttpRouteSpec::immediate(ROUTE_WIFI_SCAN, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_HARDWARE_DISCOVERY,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::HardwareDiscoveryGet)
    .with_config_activity(crate::runtime::ConfigActivityPhase::Active, true),
    HttpRouteSpec::immediate(
        ROUTE_HARDWARE_DISCOVERY,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::immediate_operator(
        ROUTE_CSRF_TOKEN,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::CsrfTokenGet),
    HttpRouteSpec::immediate(ROUTE_CSRF_TOKEN, RouteMethod::Options, RouteBodyMode::None),
    #[cfg(not(feature = "capability_office"))]
    HttpRouteSpec::rejected(ROUTE_CONFIG_ACCOUNTS, RouteMethod::Get),
    #[cfg(not(feature = "capability_office"))]
    HttpRouteSpec::rejected(ROUTE_CONFIG_ACCOUNTS, RouteMethod::Post),
    #[cfg(not(feature = "capability_office"))]
    HttpRouteSpec::immediate(
        ROUTE_CONFIG_ACCOUNTS,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    #[cfg(not(feature = "capability_office"))]
    HttpRouteSpec::rejected(ROUTE_CONFIG_CAPABILITIES, RouteMethod::Get),
    #[cfg(not(feature = "capability_office"))]
    HttpRouteSpec::immediate(
        ROUTE_CONFIG_CAPABILITIES,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    #[cfg(not(feature = "capability_office"))]
    HttpRouteSpec::rejected(ROUTE_CONFIG_PROVIDERS, RouteMethod::Get),
    #[cfg(not(feature = "capability_office"))]
    HttpRouteSpec::immediate(
        ROUTE_CONFIG_PROVIDERS,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
];

pub(crate) const OBSERVABILITY_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec::immediate_operator(
        ROUTE_HEALTH,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::HealthGet),
    HttpRouteSpec::immediate(ROUTE_HEALTH, RouteMethod::Options, RouteBodyMode::None),
    // Keep the HTTPD callback thread on lightweight summaries only.
    // Routes that inspect runtime/storage/memory state run on explicit route workers.
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_OPERATOR_STATUS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    )
    .with_handler(RouteHandler::OperatorStatusGet),
    HttpRouteSpec::immediate(
        ROUTE_OPERATOR_STATUS,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::immediate_operator(
        ROUTE_OPERATOR_WINDOW,
        RouteMethod::Post,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::OperatorWindowPost),
    HttpRouteSpec::immediate(
        ROUTE_OPERATOR_WINDOW,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    HttpRouteSpec::immediate_operator(
        ROUTE_METRICS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::MetricsGet),
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_METRICS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    )
    .with_handler(RouteHandler::MetricsGet),
    HttpRouteSpec::immediate(ROUTE_METRICS, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::immediate_operator(
        ROUTE_RESOURCE,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ResourceGet),
    HttpRouteSpec::immediate(ROUTE_RESOURCE, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_DIAGNOSE,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    )
    .with_handler(RouteHandler::DiagnoseGet),
    HttpRouteSpec::immediate(ROUTE_DIAGNOSE, RouteMethod::Options, RouteBodyMode::None),
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    HttpRouteSpec::immediate_operator(
        ROUTE_SYSTEM_INFO,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::SystemInfoGet),
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_SYSTEM_INFO,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::SystemInfoGet),
    HttpRouteSpec::immediate(ROUTE_SYSTEM_INFO, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_CHANNEL_CONNECTIVITY,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ChannelConnectivityGet)
    .with_config_activity(crate::runtime::ConfigActivityPhase::Active, true),
    HttpRouteSpec::immediate(
        ROUTE_CHANNEL_CONNECTIVITY,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
];

pub(crate) const MEMORY_AND_SKILL_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec::immediate_operator(
        ROUTE_TOOLS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ToolsGet),
    HttpRouteSpec::immediate(ROUTE_TOOLS, RouteMethod::Options, RouteBodyMode::None),
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
    HttpRouteSpec::chat_history_operator(
        ROUTE_SESSIONS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::SessionsGet),
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32", test)))]
    HttpRouteSpec::immediate_operator(
        ROUTE_SESSIONS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::SessionsGet),
    HttpRouteSpec::streaming_operator(
        ROUTE_SESSIONS,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::AlwaysOn,
    )
    .with_voice_exclusive_reject()
    .with_handler(RouteHandler::SessionsPost),
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_SESSIONS,
        RouteMethod::Delete,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    )
    .with_handler(RouteHandler::SessionsDelete),
    HttpRouteSpec::immediate(ROUTE_SESSIONS, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_MEMORY_STATUS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    )
    .with_handler(RouteHandler::MemoryStatusGet),
    HttpRouteSpec::immediate(
        ROUTE_MEMORY_STATUS,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_MEMORY_MAINTENANCE,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::Windowed,
    )
    .with_handler(RouteHandler::MemoryMaintenancePost),
    HttpRouteSpec::immediate(
        ROUTE_MEMORY_MAINTENANCE,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_CAPABILITY_PACKAGES,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    )
    .with_handler(RouteHandler::CapabilityPackagesGet),
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_CAPABILITY_PACKAGES,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::capability_package::MAX_CAPABILITY_PACKAGE_HTTP_BODY_LEN),
        OperatorRouteAccess::Windowed,
    )
    .with_handler(RouteHandler::CapabilityPackagesPost),
    HttpRouteSpec::immediate(
        ROUTE_CAPABILITY_PACKAGES,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
    HttpRouteSpec::snapshot_operator(
        ROUTE_SKILLS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    )
    .with_handler(RouteHandler::SkillsGet),
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32", test)))]
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_SKILLS,
        RouteMethod::Get,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    )
    .with_handler(RouteHandler::SkillsGet),
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_SKILLS,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::Windowed,
    )
    .with_handler(RouteHandler::SkillsPost),
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_SKILLS,
        RouteMethod::Delete,
        RouteBodyMode::None,
        OperatorRouteAccess::Windowed,
    )
    .with_handler(RouteHandler::SkillsDelete),
    HttpRouteSpec::immediate(ROUTE_SKILLS, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::slow_diagnostic_operator(
        ROUTE_SKILLS_IMPORT,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
        OperatorRouteAccess::Windowed,
    )
    .with_handler(RouteHandler::SkillsImportPost),
    HttpRouteSpec::immediate(
        ROUTE_SKILLS_IMPORT,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
];

pub(crate) const ACTION_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec::immediate_operator(
        ROUTE_RESTART,
        RouteMethod::Post,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::RestartPost)
    .with_config_activity(crate::runtime::ConfigActivityPhase::Stopping, true),
    HttpRouteSpec::immediate(ROUTE_RESTART, RouteMethod::Options, RouteBodyMode::None),
    HttpRouteSpec::async_config_operator(
        ROUTE_CONFIG_RESET,
        RouteMethod::Post,
        RouteBodyMode::None,
        OperatorRouteAccess::AlwaysOn,
    )
    .with_handler(RouteHandler::ConfigResetPost)
    .with_config_activity(crate::runtime::ConfigActivityPhase::Persisting, true),
    HttpRouteSpec::immediate(
        ROUTE_CONFIG_RESET,
        RouteMethod::Options,
        RouteBodyMode::None,
    ),
    HttpRouteSpec::immediate(
        ROUTE_WEBHOOK,
        RouteMethod::Post,
        RouteBodyMode::Utf8(crate::platform::http_server::common::POST_BODY_MAX_LEN),
    )
    .with_handler(RouteHandler::WebhookPost),
    HttpRouteSpec::immediate(ROUTE_WEBHOOK, RouteMethod::Options, RouteBodyMode::None),
];

pub(crate) fn route_spec_for(method: &str, path: &str) -> Option<HttpRouteSpec> {
    let method = RouteMethod::parse(method)?;
    route_spec_for_method(method, path)
}

pub(crate) fn route_spec_for_method(method: RouteMethod, path: &str) -> Option<HttpRouteSpec> {
    for group in route_spec_groups() {
        if let Some(spec) = group
            .iter()
            .copied()
            .find(|spec| spec.method == method && spec.path == path)
        {
            return Some(spec);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compiled_enabled_channel_ids, selectable_channel_entries, CHANNEL_QQ_CHANNEL};

    #[test]
    fn compiled_enabled_channel_ids_follow_selectable_catalog_order() {
        let ids = compiled_enabled_channel_ids();
        assert_eq!(ids.first().copied(), Some(""));
        let selectable = selectable_channel_entries()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        assert_eq!(&ids[1..], selectable.as_slice());
        #[cfg(feature = "dingtalk")]
        assert!(ids.contains(&crate::CHANNEL_DINGTALK));
        #[cfg(not(feature = "dingtalk"))]
        assert!(!ids.contains(&crate::channel_capability::CHANNEL_DINGTALK));
        #[cfg(feature = "qq_channel")]
        assert!(ids.contains(&CHANNEL_QQ_CHANNEL));
        #[cfg(not(feature = "qq_channel"))]
        assert!(!ids.contains(&crate::channel_capability::CHANNEL_QQ_CHANNEL));
    }

    #[test]
    fn route_lookup_returns_spec_metadata_from_catalog() {
        let spec = route_spec_for("POST", ROUTE_CONFIG_CHANNELS).expect("route spec");
        assert_eq!(spec.method.as_str(), "POST");
        assert_eq!(spec.path, ROUTE_CONFIG_CHANNELS);
        assert!(matches!(spec.body_mode, RouteBodyMode::Utf8(_)));
        assert_eq!(spec.execution_class, RouteExecutionClass::AsyncConfigRoute);
        assert_eq!(spec.operator_access, OperatorRouteAccess::AlwaysOn);
        assert_eq!(spec.handler(), Some(RouteHandler::ConfigChannelsPost));
    }

    #[test]
    fn route_catalog_specs_are_dispatch_handler_truth() {
        for group in route_spec_groups() {
            for spec in *group {
                if spec.method == RouteMethod::Options
                    || spec.execution_class == RouteExecutionClass::RejectedRoute
                {
                    assert!(
                        spec.handler().is_none(),
                        "{} {} must not carry a dispatch handler",
                        spec.method.as_str(),
                        spec.path
                    );
                    continue;
                }
                assert!(
                    spec.handler().is_some(),
                    "{} {} must declare its dispatch handler in the catalog",
                    spec.method.as_str(),
                    spec.path
                );
            }
        }
    }

    #[test]
    fn route_lookup_marks_windowed_diagnostic_routes() {
        let spec = route_spec_for("GET", ROUTE_MEMORY_STATUS).expect("route spec");
        assert_eq!(
            spec.execution_class,
            RouteExecutionClass::SlowDiagnosticRoute
        );
        assert_eq!(spec.operator_access, OperatorRouteAccess::Windowed);
    }

    #[test]
    fn tools_route_is_lightweight_always_on_operator_surface() {
        let spec = route_spec_for("GET", ROUTE_TOOLS).expect("route spec");
        assert_eq!(spec.execution_class, RouteExecutionClass::ImmediateRoute);
        assert_eq!(spec.operator_access, OperatorRouteAccess::AlwaysOn);
    }

    #[test]
    fn restart_route_is_lightweight_stopping_action_not_config_worker() {
        let spec = route_spec_for("POST", ROUTE_RESTART).expect("restart route");

        assert_eq!(spec.execution_class, RouteExecutionClass::ImmediateRoute);
        assert_eq!(spec.operator_access, OperatorRouteAccess::AlwaysOn);
        assert_eq!(spec.body_mode, RouteBodyMode::None);
        assert_eq!(spec.handler(), Some(RouteHandler::RestartPost));
        assert_eq!(
            spec.config_activity_phase(),
            Some(crate::runtime::ConfigActivityPhase::Stopping)
        );
        assert!(spec.rejects_during_voice_exclusive());
    }

    #[test]
    fn sessions_routes_are_product_chat_surface() {
        let get = route_spec_for("GET", ROUTE_SESSIONS).expect("sessions get route");
        assert_eq!(get.execution_class, RouteExecutionClass::ChatHistoryRoute);
        assert_eq!(get.operator_access, OperatorRouteAccess::AlwaysOn);
        assert_eq!(get.handler(), Some(RouteHandler::SessionsGet));
        assert!(get.uses_response_build_admission());

        let post = route_spec_for("POST", ROUTE_SESSIONS).expect("sessions post route");
        assert!(matches!(post.body_mode, RouteBodyMode::Utf8(_)));
        assert_eq!(post.execution_class, RouteExecutionClass::StreamingRoute);
        assert_eq!(post.operator_access, OperatorRouteAccess::AlwaysOn);
        assert!(post.rejects_during_voice_exclusive());
        assert!(!post.uses_response_build_admission());
        assert_eq!(post.handler(), Some(RouteHandler::SessionsPost));
    }

    #[test]
    fn large_json_routes_declare_response_build_admission() {
        for (method, path) in [
            ("GET", ROUTE_SESSIONS),
            ("GET", ROUTE_MEMORY_STATUS),
            ("GET", ROUTE_DIAGNOSE),
            ("GET", ROUTE_SYSTEM_INFO),
        ] {
            let spec = route_spec_for(method, path).expect("route spec");
            assert!(
                spec.uses_response_build_admission(),
                "{method} {path} must be admitted before building a large response"
            );
        }

        for (method, path) in [
            ("GET", ROUTE_HEALTH),
            ("GET", ROUTE_RESOURCE),
            ("POST", ROUTE_SESSIONS),
            ("DELETE", ROUTE_SESSIONS),
        ] {
            let spec = route_spec_for(method, path).expect("route spec");
            assert!(
                !spec.uses_response_build_admission(),
                "{method} {path} must not be guarded by large response admission"
            );
        }
    }

    #[test]
    fn skills_inventory_get_requires_operator_window_on_embedded_catalog() {
        let get = route_spec_for("GET", ROUTE_SKILLS).expect("skills get");
        assert_eq!(get.execution_class, RouteExecutionClass::SnapshotRoute);
        assert_eq!(get.operator_access, OperatorRouteAccess::Windowed);

        let post = route_spec_for("POST", ROUTE_SKILLS).expect("skills post");
        assert_eq!(
            post.execution_class,
            RouteExecutionClass::SlowDiagnosticRoute
        );
        let delete = route_spec_for("DELETE", ROUTE_SKILLS).expect("skills delete");
        assert_eq!(
            delete.execution_class,
            RouteExecutionClass::SlowDiagnosticRoute
        );
    }

    #[test]
    fn route_lookup_keeps_memory_maintenance_body_contract() {
        let spec = route_spec_for("POST", ROUTE_MEMORY_MAINTENANCE).expect("route spec");
        assert_eq!(
            spec.execution_class,
            RouteExecutionClass::SlowDiagnosticRoute
        );
        assert_eq!(spec.operator_access, OperatorRouteAccess::Windowed);
        assert!(matches!(spec.body_mode, RouteBodyMode::Utf8(_)));
    }

    #[test]
    fn storage_touching_routes_never_run_on_httpd_callback() {
        for (method, path) in [
            ("DELETE", ROUTE_SESSIONS),
            ("GET", ROUTE_MEMORY_STATUS),
            ("POST", ROUTE_MEMORY_MAINTENANCE),
            ("GET", ROUTE_CAPABILITY_PACKAGES),
            ("POST", ROUTE_CAPABILITY_PACKAGES),
        ] {
            let spec = route_spec_for(method, path).expect("storage route spec");
            assert_ne!(
                spec.execution_class,
                RouteExecutionClass::ImmediateRoute,
                "{} {} must stay off the HTTPD callback",
                method,
                path
            );
            assert_eq!(
                spec.operator_access,
                OperatorRouteAccess::Windowed,
                "{} {} must require an explicit operator window",
                method,
                path
            );
        }
    }

    #[cfg(not(feature = "capability_office"))]
    #[test]
    fn office_config_collection_routes_are_structured_rejections_without_office_feature() {
        for (method, path) in [
            ("GET", ROUTE_CONFIG_ACCOUNTS),
            ("POST", ROUTE_CONFIG_ACCOUNTS),
            ("GET", ROUTE_CONFIG_CAPABILITIES),
            ("GET", ROUTE_CONFIG_PROVIDERS),
        ] {
            let spec = route_spec_for(method, path).expect("unsupported office route spec");
            assert_eq!(spec.execution_class, RouteExecutionClass::RejectedRoute);
            assert_eq!(spec.operator_access, OperatorRouteAccess::Hidden);
        }
    }

    #[test]
    fn immediate_routes_stay_on_strict_lightweight_whitelist() {
        for group in route_spec_groups() {
            for spec in *group {
                if spec.execution_class != RouteExecutionClass::ImmediateRoute {
                    continue;
                }
                if spec.method == RouteMethod::Options {
                    continue;
                }
                assert!(
                    matches!(
                        (spec.method, spec.path),
                        (RouteMethod::Get, ROUTE_ROOT)
                            | (RouteMethod::Get, ROUTE_PAIRING_CODE)
                            | (RouteMethod::Post, ROUTE_PAIRING_CODE)
                            | (RouteMethod::Get, ROUTE_CSRF_TOKEN)
                            | (RouteMethod::Get, ROUTE_CONFIG_LLM)
                            | (RouteMethod::Get, ROUTE_CONFIG_CHANNELS)
                            | (RouteMethod::Get, ROUTE_CONFIG_SYSTEM)
                            | (RouteMethod::Get, ROUTE_CONFIG_HARDWARE)
                            | (RouteMethod::Get, ROUTE_CONFIG_AUDIO)
                            | (RouteMethod::Get, ROUTE_CONFIG_DISPLAY)
                            | (RouteMethod::Get, ROUTE_HEALTH)
                            | (RouteMethod::Get, ROUTE_RESOURCE)
                            | (RouteMethod::Get, ROUTE_TOOLS)
                            | (RouteMethod::Post, ROUTE_OPERATOR_WINDOW)
                            | (RouteMethod::Post, ROUTE_RESTART)
                            | (RouteMethod::Post, ROUTE_WEBHOOK)
                    ),
                    "route {} {} must not run on HTTPD callback",
                    spec.method.as_str(),
                    spec.path
                );
            }
        }
    }

    #[test]
    fn cached_config_reads_are_immediate_but_writes_use_config_lane() {
        for path in [
            ROUTE_CONFIG_LLM,
            ROUTE_CONFIG_CHANNELS,
            ROUTE_CONFIG_SYSTEM,
            ROUTE_CONFIG_HARDWARE,
            ROUTE_CONFIG_AUDIO,
            ROUTE_CONFIG_DISPLAY,
        ] {
            let get = route_spec_for_method(RouteMethod::Get, path).expect("config get");
            assert_eq!(
                get.execution_class,
                RouteExecutionClass::ImmediateRoute,
                "cached {} GET must not require an ESP config worker",
                path
            );
            assert!(
                get.tracks_config_read_burst(),
                "{} GET must be marked as a config read burst",
                path
            );
            let post = route_spec_for_method(RouteMethod::Post, path).expect("config post");
            assert_eq!(
                post.execution_class,
                RouteExecutionClass::AsyncConfigRoute,
                "{} POST must use config worker",
                path
            );
            assert!(
                !post.tracks_config_read_burst(),
                "{} POST must use the config worker lease instead of read-burst tracking",
                path
            );
        }
    }

    #[test]
    fn config_ui_routes_extend_config_activity_for_heavy_or_persisting_work_only() {
        let pairing = route_spec_for("GET", ROUTE_PAIRING_CODE).expect("pairing");
        assert_eq!(pairing.config_activity_phase(), None);
        let csrf = route_spec_for("GET", ROUTE_CSRF_TOKEN).expect("csrf");
        assert_eq!(csrf.config_activity_phase(), None);
        let read_config = route_spec_for("GET", ROUTE_CONFIG_SYSTEM).expect("config get");
        assert_eq!(read_config.config_activity_phase(), None);
        let write_config = route_spec_for("POST", ROUTE_CONFIG_SYSTEM).expect("config post");
        assert_eq!(
            write_config.config_activity_phase(),
            Some(crate::runtime::ConfigActivityPhase::Persisting)
        );
        let diagnostic = route_spec_for("GET", ROUTE_WIFI_SCAN).expect("wifi scan");
        assert_eq!(
            diagnostic.config_activity_phase(),
            Some(crate::runtime::ConfigActivityPhase::Active)
        );
        let operator_window = route_spec_for("POST", ROUTE_OPERATOR_WINDOW).expect("operator");
        assert_eq!(operator_window.config_activity_phase(), None);
        let restart = route_spec_for("POST", ROUTE_RESTART).expect("restart");
        assert_eq!(
            restart.config_activity_phase(),
            Some(crate::runtime::ConfigActivityPhase::Stopping)
        );
        let reset = route_spec_for("POST", ROUTE_CONFIG_RESET).expect("reset");
        assert_eq!(
            reset.config_activity_phase(),
            Some(crate::runtime::ConfigActivityPhase::Persisting)
        );
        let channel_probe =
            route_spec_for("GET", ROUTE_CHANNEL_CONNECTIVITY).expect("channel probe");
        assert_eq!(
            channel_probe.config_activity_phase(),
            Some(crate::runtime::ConfigActivityPhase::Active)
        );
        let resource = route_spec_for("GET", ROUTE_RESOURCE).expect("resource");
        assert_eq!(resource.config_activity_phase(), None);
        let health = route_spec_for("GET", ROUTE_HEALTH).expect("health");
        assert_eq!(health.config_activity_phase(), None);
        let root = route_spec_for("GET", ROUTE_ROOT).expect("root");
        assert_eq!(root.config_activity_phase(), None);
        let custom_webhook = route_spec_for("POST", ROUTE_WEBHOOK).expect("webhook");
        assert_eq!(custom_webhook.config_activity_phase(), None);
        assert!(route_spec_for("GET", "/api/device_snapshot").is_none());
    }

    #[test]
    fn realtime_voice_blocks_heavy_config_activity_routes_only() {
        let config_get = route_spec_for("GET", ROUTE_CONFIG_SYSTEM).expect("config get");
        assert!(!config_get.rejects_during_voice_exclusive());
        let config_post = route_spec_for("POST", ROUTE_CONFIG_SYSTEM).expect("config post");
        assert!(config_post.rejects_during_voice_exclusive());
        let diagnostic = route_spec_for("GET", ROUTE_WIFI_SCAN).expect("wifi scan");
        assert!(diagnostic.rejects_during_voice_exclusive());
        let pairing = route_spec_for("GET", ROUTE_PAIRING_CODE).expect("pairing");
        assert!(!pairing.rejects_during_voice_exclusive());
        let csrf = route_spec_for("GET", ROUTE_CSRF_TOKEN).expect("csrf");
        assert!(!csrf.rejects_during_voice_exclusive());
        let channel_probe =
            route_spec_for("GET", ROUTE_CHANNEL_CONNECTIVITY).expect("channel probe");
        assert!(channel_probe.rejects_during_voice_exclusive());
        let chat_stream = route_spec_for("POST", ROUTE_SESSIONS).expect("sessions post");
        assert!(chat_stream.rejects_during_voice_exclusive());
        assert_eq!(
            channel_probe.operator_access,
            OperatorRouteAccess::AlwaysOn,
            "missing or invalid channel query must reach the API contract before deep window gating"
        );
        let health = route_spec_for("GET", ROUTE_HEALTH).expect("health");
        assert!(!health.rejects_during_voice_exclusive());
    }

    #[test]
    fn route_runtime_mode_admission_blocks_unowned_diagnostics_during_config_active() {
        let mode =
            crate::runtime::mode::snapshot_from_source(crate::runtime::mode::RuntimeModeSource {
                config_active: true,
                config_activity_phase: crate::runtime::ConfigActivityPhase::Active,
                ..crate::runtime::mode::RuntimeModeSource::default()
            });

        let diagnose = route_spec_for("GET", ROUTE_DIAGNOSE).expect("diagnose");
        assert_eq!(
            diagnose.runtime_mode_admission(mode),
            RouteRuntimeAdmission::Rejected {
                status: 409,
                error_key: "runtime.route_blocked_by_config_active",
                stage: "runtime_route_admission",
                reason: "config_active_diagnostic_not_admitted",
            }
        );

        let wifi_scan = route_spec_for("GET", ROUTE_WIFI_SCAN).expect("wifi scan");
        assert_eq!(
            wifi_scan.runtime_mode_admission(mode),
            RouteRuntimeAdmission::Allowed
        );

        let config_write = route_spec_for("POST", ROUTE_CONFIG_SYSTEM).expect("config write");
        assert_eq!(
            config_write.runtime_mode_admission(mode),
            RouteRuntimeAdmission::Allowed
        );
    }

    #[test]
    fn route_runtime_mode_admission_recovery_safe_mode_has_minimal_allowlist() {
        let mode =
            crate::runtime::mode::snapshot_from_source(crate::runtime::mode::RuntimeModeSource {
                recovery_safe_mode_active: true,
                ..crate::runtime::mode::RuntimeModeSource::default()
            });

        let health = route_spec_for("GET", ROUTE_HEALTH).expect("health");
        assert_eq!(
            health.runtime_mode_admission(mode),
            RouteRuntimeAdmission::Allowed
        );

        let csrf = route_spec_for("GET", ROUTE_CSRF_TOKEN).expect("csrf");
        assert_eq!(
            csrf.runtime_mode_admission(mode),
            RouteRuntimeAdmission::Allowed
        );

        let reset = route_spec_for("POST", ROUTE_CONFIG_RESET).expect("config reset");
        assert_eq!(
            reset.runtime_mode_admission(mode),
            RouteRuntimeAdmission::Allowed
        );

        let config_write = route_spec_for("POST", ROUTE_CONFIG_SYSTEM).expect("config write");
        assert_eq!(
            config_write.runtime_mode_admission(mode),
            RouteRuntimeAdmission::Rejected {
                status: 503,
                error_key: "runtime.route_blocked_by_recovery_safe_mode",
                stage: "runtime_route_admission",
                reason: "recovery_safe_mode_allowlist",
            }
        );

        let diagnose = route_spec_for("GET", ROUTE_DIAGNOSE).expect("diagnose");
        assert_eq!(
            diagnose.runtime_mode_admission(mode),
            RouteRuntimeAdmission::Rejected {
                status: 503,
                error_key: "runtime.route_blocked_by_recovery_safe_mode",
                stage: "runtime_route_admission",
                reason: "recovery_safe_mode_allowlist",
            }
        );
    }

    #[test]
    fn diagnostics_snapshots_and_rejected_routes_are_explicit() {
        let wifi_scan = route_spec_for("GET", ROUTE_WIFI_SCAN).expect("wifi scan");
        assert_eq!(
            wifi_scan.execution_class,
            RouteExecutionClass::LocalDiagnosticRoute
        );
        assert!(
            !wifi_scan
                .execution_class
                .worker_contract()
                .expect("wifi scan worker")
                .reserves_tls_headroom
        );
        assert_eq!(
            route_spec_for("GET", ROUTE_HARDWARE_DISCOVERY)
                .expect("hardware discovery")
                .execution_class,
            RouteExecutionClass::SlowDiagnosticRoute
        );
        assert_eq!(
            route_spec_for("GET", ROUTE_CHANNEL_CONNECTIVITY)
                .expect("channel connectivity")
                .execution_class,
            RouteExecutionClass::SlowDiagnosticRoute
        );
        let custom_webhook = route_spec_for("POST", ROUTE_WEBHOOK).expect("custom webhook");
        assert_eq!(
            custom_webhook.execution_class,
            RouteExecutionClass::ImmediateRoute
        );
        assert!(matches!(custom_webhook.body_mode, RouteBodyMode::Utf8(_)));
    }

    #[test]
    fn worker_route_classes_have_complete_contracts() {
        for class in [
            RouteExecutionClass::SnapshotRoute,
            RouteExecutionClass::ChatHistoryRoute,
            RouteExecutionClass::AsyncConfigRoute,
            RouteExecutionClass::LocalDiagnosticRoute,
            RouteExecutionClass::SlowDiagnosticRoute,
        ] {
            let contract = class.worker_contract().expect("worker contract");
            assert_eq!(contract.worker_threads, 1);
            assert!(contract.queue_capacity <= 2);
            assert!(contract.stack_size > 0);
            assert!(contract.timeout_secs > 0);
            assert!(contract.timeout_secs <= 20);
            assert!(contract.idle_timeout_secs > 0);
            assert!(contract.idle_timeout_secs <= 10);
            assert_eq!(contract.reject_status, 503);
            if contract.reserves_tls_headroom {
                assert!(contract.socket_reserve >= 3);
            }
            assert!(!contract.counter_name.is_empty());
            assert!(!contract.begin_stage.is_empty());
            assert!(!contract.complete_stage.is_empty());
        }
        assert!(RouteExecutionClass::ImmediateRoute
            .worker_contract()
            .is_none());
        assert!(RouteExecutionClass::StreamingRoute
            .worker_contract()
            .is_none());
        assert!(RouteExecutionClass::RejectedRoute
            .worker_contract()
            .is_none());
        assert_eq!(
            RouteExecutionClass::SnapshotRoute
                .worker_contract()
                .expect("snapshot worker")
                .stack_size,
            crate::util::STACK_HTTP_SNAPSHOT_WORKER
        );
        assert_eq!(
            RouteExecutionClass::ChatHistoryRoute
                .worker_contract()
                .expect("chat history worker")
                .stack_size,
            crate::util::STACK_HTTP_CHAT_HISTORY_WORKER
        );
        assert_eq!(
            RouteExecutionClass::AsyncConfigRoute
                .worker_contract()
                .expect("config worker")
                .stack_size,
            crate::util::STACK_HTTP_CONFIG_WORKER
        );
        assert_eq!(
            RouteExecutionClass::SlowDiagnosticRoute
                .worker_contract()
                .expect("diagnostic worker")
                .stack_size,
            crate::util::STACK_HTTP_DIAG_WORKER
        );
        assert_eq!(
            RouteExecutionClass::LocalDiagnosticRoute
                .worker_contract()
                .expect("local diagnostic worker")
                .stack_size,
            crate::util::STACK_HTTP_DIAG_WORKER
        );
    }

    #[test]
    fn route_worker_runtime_admission_blocks_front_plane_contention() {
        let contract = RouteExecutionClass::SnapshotRoute
            .worker_contract()
            .expect("snapshot contract");
        let idle = RouteWorkerRuntimeLoad {
            pressure: crate::orchestrator::pressure::PressureLevel::Normal,
            storage_contention: crate::orchestrator::StorageContentionRisk::Healthy,
            active_agent_tasks: 0,
            inbound_depth: 0,
            outbound_depth: 0,
            scheduler_decision: crate::runtime::RuntimeWorkDecision::Proceed,
        };

        assert!(route_worker_runtime_busy_detail(contract, idle).is_none());

        let mut active_agent = idle;
        active_agent.active_agent_tasks = 1;
        assert!(route_worker_runtime_busy_detail(contract, active_agent)
            .expect("active agent should defer snapshot worker")
            .contains("active_agent_tasks=1"));

        let mut storage_busy = idle;
        storage_busy.storage_contention = crate::orchestrator::StorageContentionRisk::Cautious;
        assert!(route_worker_runtime_busy_detail(contract, storage_busy)
            .expect("storage contention should defer snapshot worker")
            .contains("storage_contention=Cautious"));

        let mut pressure_busy = idle;
        pressure_busy.pressure = crate::orchestrator::pressure::PressureLevel::Cautious;
        assert!(route_worker_runtime_busy_detail(contract, pressure_busy)
            .expect("pressure should defer snapshot worker")
            .contains("pressure=Cautious"));

        let chat_history = RouteExecutionClass::ChatHistoryRoute
            .worker_contract()
            .expect("chat history contract");
        assert!(
            route_worker_runtime_busy_detail(chat_history, pressure_busy).is_none(),
            "chat history must remain available in Cautious when concrete worker memory admission still passes"
        );
    }

    #[test]
    fn config_worker_transport_gate_does_not_guard_local_config_routes() {
        let persisting_mode =
            crate::runtime::mode::snapshot_from_source(crate::runtime::mode::RuntimeModeSource {
                config_active: true,
                config_activity_phase: crate::runtime::ConfigActivityPhase::Persisting,
                ..crate::runtime::mode::RuntimeModeSource::default()
            });

        assert!(
            !RouteExecutionClass::AsyncConfigRoute
                .requires_route_worker_transport_admission(persisting_mode),
            "config save worker is local persistence and must not consume NonVoiceHttp transport admission against its own guard"
        );
        let boot_persisting_mode =
            crate::runtime::mode::snapshot_from_source(crate::runtime::mode::RuntimeModeSource {
                boot_phase_active: true,
                config_active: true,
                config_activity_phase: crate::runtime::ConfigActivityPhase::Persisting,
                ..crate::runtime::mode::RuntimeModeSource::default()
            });

        assert!(
            !RouteExecutionClass::AsyncConfigRoute
                .requires_route_worker_transport_admission(boot_persisting_mode),
            "config save remains the recovery plane owner while booting; boot transport suspension must only cover outbound/network workers"
        );
        assert!(
            RouteExecutionClass::SlowDiagnosticRoute
                .requires_route_worker_transport_admission(persisting_mode),
            "diagnostic workers still obey transport admission during config persistence"
        );

        let voice_mode =
            crate::runtime::mode::snapshot_from_source(crate::runtime::mode::RuntimeModeSource {
                voice_exclusive_active: true,
                ..crate::runtime::mode::RuntimeModeSource::default()
            });
        assert!(
            !RouteExecutionClass::AsyncConfigRoute
                .requires_route_worker_transport_admission(voice_mode),
            "voice-exclusive config blocking belongs to route runtime admission, not outbound transport admission"
        );
    }

    #[test]
    fn config_worker_does_not_use_generic_runtime_busy_gate() {
        assert!(
            !RouteExecutionClass::AsyncConfigRoute.requires_route_worker_runtime_busy_admission(),
            "config save is the local config recovery owner; memory admission remains the floor gate"
        );
        assert!(
            RouteExecutionClass::SlowDiagnosticRoute.requires_route_worker_runtime_busy_admission(),
            "deep diagnostic work must still obey generic runtime busy admission"
        );
        assert!(
            RouteExecutionClass::ChatHistoryRoute.requires_route_worker_runtime_busy_admission(),
            "chat history remains a deep UI worker and must not inherit config save priority"
        );
    }

    #[test]
    fn route_execution_classes_map_only_worker_routes_to_scheduler_work() {
        assert_eq!(
            RouteExecutionClass::ImmediateRoute.runtime_work_class(),
            None
        );
        assert_eq!(
            RouteExecutionClass::StreamingRoute.runtime_work_class(),
            None
        );
        assert_eq!(
            RouteExecutionClass::RejectedRoute.runtime_work_class(),
            None
        );
        assert_eq!(
            RouteExecutionClass::SnapshotRoute.runtime_work_class(),
            Some(crate::runtime::RuntimeWorkClass::DeepRouteWorker)
        );
        assert_eq!(
            RouteExecutionClass::ChatHistoryRoute.runtime_work_class(),
            Some(crate::runtime::RuntimeWorkClass::ConfigUiChatHistoryRoute)
        );
        assert_eq!(
            RouteExecutionClass::AsyncConfigRoute.runtime_work_class(),
            Some(crate::runtime::RuntimeWorkClass::DeepRouteWorker)
        );
        assert_eq!(
            RouteExecutionClass::LocalDiagnosticRoute.runtime_work_class(),
            Some(crate::runtime::RuntimeWorkClass::DeepRouteWorker)
        );
        assert_eq!(
            RouteExecutionClass::SlowDiagnosticRoute.runtime_work_class(),
            Some(crate::runtime::RuntimeWorkClass::DeepRouteWorker)
        );
    }

    #[test]
    fn route_worker_runtime_admission_consumes_scheduler_decision_first() {
        let contract = RouteExecutionClass::SnapshotRoute
            .worker_contract()
            .expect("snapshot contract");
        let mut load = RouteWorkerRuntimeLoad {
            pressure: crate::orchestrator::pressure::PressureLevel::Normal,
            storage_contention: crate::orchestrator::StorageContentionRisk::Healthy,
            active_agent_tasks: 0,
            inbound_depth: 0,
            outbound_depth: 0,
            scheduler_decision: crate::runtime::RuntimeWorkDecision::Proceed,
        }
        .with_scheduler_decision(crate::runtime::RuntimeWorkDecision::Defer {
            reason: "foreground_active",
            retry_after_ms: 29_500,
        });

        let detail = route_worker_runtime_busy_detail(contract, load)
            .expect("scheduler defer should block deep route worker");
        assert!(detail.contains("runtime_scheduler=defer"));
        assert!(detail.contains("reason=foreground_active"));
        assert!(detail.contains("retry_after_ms=29500"));

        load.scheduler_decision = crate::runtime::RuntimeWorkDecision::Proceed;
        assert!(
            route_worker_runtime_busy_detail(contract, load).is_none(),
            "resource-idle route worker must proceed once scheduler admits it"
        );
    }

    #[test]
    fn worker_route_lanes_map_to_runtime_lease_kinds() {
        let cases = [
            (
                RouteExecutionClass::SnapshotRoute,
                RouteWorkerLane::Snapshot,
                crate::runtime::lease::LeaseKind::SnapshotHttpWorker,
            ),
            (
                RouteExecutionClass::ChatHistoryRoute,
                RouteWorkerLane::ChatHistory,
                crate::runtime::lease::LeaseKind::ChatHistoryHttpWorker,
            ),
            (
                RouteExecutionClass::AsyncConfigRoute,
                RouteWorkerLane::Config,
                crate::runtime::lease::LeaseKind::ConfigHttpWorker,
            ),
            (
                RouteExecutionClass::LocalDiagnosticRoute,
                RouteWorkerLane::Diagnostic,
                crate::runtime::lease::LeaseKind::DiagnosticHttpWorker,
            ),
            (
                RouteExecutionClass::SlowDiagnosticRoute,
                RouteWorkerLane::Diagnostic,
                crate::runtime::lease::LeaseKind::DiagnosticHttpWorker,
            ),
        ];

        for (class, lane, lease_kind) in cases {
            let contract = class.worker_contract().expect("worker contract");
            assert_eq!(contract.lane, lane);
            assert_eq!(lane.lease_kind(), lease_kind);
            assert_eq!(
                lane.lease_mode(),
                crate::runtime::lease::LeaseMode::Exclusive
            );
        }
    }

    #[test]
    fn official_ota_route_is_not_exposed_without_an_implementation() {
        let removed_ota_route = concat!("/api/", "ota");
        assert!(route_spec_for("POST", removed_ota_route).is_none());
        assert!(route_spec_for("OPTIONS", removed_ota_route).is_none());
        assert!(!operator_route_endpoints(None, false)
            .iter()
            .any(|endpoint| endpoint.contains(removed_ota_route)));
    }

    #[test]
    fn snapshot_worker_budget_does_not_reserve_tls_headroom() {
        const S3_MIN_SNAPSHOT_ROUTE_LARGEST_BLOCK_FLOOR_BYTES: usize = 32 * 1024;

        let contract = RouteExecutionClass::SnapshotRoute
            .worker_contract()
            .expect("snapshot worker");
        let requirements = route_worker_memory_requirements(contract);

        assert!(!contract.reserves_tls_headroom);
        assert_eq!(
            requirements.required_internal,
            contract
                .stack_size
                .saturating_add(ROUTE_WORKER_NON_TLS_INTERNAL_HEADROOM)
        );
        assert_eq!(
            requirements.required_largest,
            contract
                .stack_size
                .saturating_add(ROUTE_WORKER_NON_TLS_LARGEST_HEADROOM)
        );
        assert!(
            requirements.required_largest <= S3_MIN_SNAPSHOT_ROUTE_LARGEST_BLOCK_FLOOR_BYTES,
            "snapshot worker must fit the S3 lowest-admission largest-block floor"
        );
    }

    #[test]
    fn resource_route_stays_cached_light_immediate() {
        let spec = route_spec_for("GET", ROUTE_RESOURCE).expect("resource route");

        assert_eq!(spec.execution_class, RouteExecutionClass::ImmediateRoute);
        assert_eq!(spec.operator_access, OperatorRouteAccess::AlwaysOn);
    }

    #[test]
    fn observability_route_classes_preserve_contract_boundaries() {
        let health = route_spec_for("GET", ROUTE_HEALTH).expect("health route");
        assert_eq!(health.execution_class, RouteExecutionClass::ImmediateRoute);
        assert_eq!(health.operator_access, OperatorRouteAccess::AlwaysOn);

        let resource = route_spec_for("GET", ROUTE_RESOURCE).expect("resource route");
        assert_eq!(
            resource.execution_class,
            RouteExecutionClass::ImmediateRoute
        );
        assert_eq!(resource.operator_access, OperatorRouteAccess::AlwaysOn);

        let operator_status =
            route_spec_for("GET", ROUTE_OPERATOR_STATUS).expect("operator status route");
        assert_eq!(
            operator_status.execution_class,
            RouteExecutionClass::SlowDiagnosticRoute
        );
        assert_eq!(
            operator_status.operator_access,
            OperatorRouteAccess::Windowed
        );

        let diagnose = route_spec_for("GET", ROUTE_DIAGNOSE).expect("diagnose route");
        assert_eq!(
            diagnose.execution_class,
            RouteExecutionClass::SlowDiagnosticRoute
        );
        assert_eq!(diagnose.operator_access, OperatorRouteAccess::Windowed);

        let metrics = route_spec_for("GET", ROUTE_METRICS).expect("metrics route");
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        assert_eq!(metrics.execution_class, RouteExecutionClass::ImmediateRoute);
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        assert_eq!(
            metrics.execution_class,
            RouteExecutionClass::SlowDiagnosticRoute
        );
    }

    #[test]
    fn pairing_and_csrf_routes_stay_immediate_without_config_activity() {
        for (method, path) in [
            ("GET", ROUTE_PAIRING_CODE),
            ("GET", ROUTE_CSRF_TOKEN),
            ("OPTIONS", ROUTE_PAIRING_CODE),
            ("OPTIONS", ROUTE_CSRF_TOKEN),
        ] {
            let spec = route_spec_for(method, path).expect("pairing/csrf route");
            assert_eq!(
                spec.execution_class,
                RouteExecutionClass::ImmediateRoute,
                "{} {} must not start a route worker",
                method,
                path
            );
            assert_eq!(
                spec.config_activity_phase(),
                None,
                "{} {} must not mark config activity",
                method,
                path
            );
        }
    }

    #[test]
    fn default_config_ui_route_allowlist_stays_cached_lightweight() {
        for (method, path) in [
            ("GET", ROUTE_PAIRING_CODE),
            ("GET", ROUTE_CSRF_TOKEN),
            ("GET", ROUTE_CONFIG_SYSTEM),
            ("GET", ROUTE_CONFIG_LLM),
            ("GET", ROUTE_CONFIG_CHANNELS),
            ("GET", ROUTE_CONFIG_HARDWARE),
            ("GET", ROUTE_CONFIG_AUDIO),
            ("GET", ROUTE_CONFIG_DISPLAY),
            ("GET", ROUTE_HEALTH),
            ("GET", ROUTE_RESOURCE),
        ] {
            let spec = route_spec_for(method, path).expect("default UI route");
            assert_eq!(
                spec.execution_class,
                RouteExecutionClass::ImmediateRoute,
                "{} {} must remain immediate cached-light, got {:?}",
                method,
                path,
                spec.execution_class
            );
            assert_ne!(
                spec.operator_access,
                OperatorRouteAccess::Windowed,
                "{} {} must not require the deep operator window by default",
                method,
                path
            );
        }

        for (method, path) in [
            ("GET", ROUTE_WIFI_SCAN),
            ("GET", ROUTE_HARDWARE_DISCOVERY),
            ("GET", ROUTE_DIAGNOSE),
            ("GET", ROUTE_CHANNEL_CONNECTIVITY),
            ("GET", ROUTE_MEMORY_STATUS),
            ("POST", ROUTE_MEMORY_MAINTENANCE),
            ("POST", ROUTE_SKILLS_IMPORT),
        ] {
            let spec = route_spec_for(method, path).expect("deep route");
            assert!(
                matches!(
                    spec.execution_class,
                    RouteExecutionClass::LocalDiagnosticRoute
                        | RouteExecutionClass::SlowDiagnosticRoute
                ),
                "{} {} must stay worker-backed, got {:?}",
                method,
                path,
                spec.execution_class
            );
        }
    }

    #[test]
    fn config_worker_budget_fits_normal_esp_config_mode_largest_block() {
        const S3_NORMAL_CONFIG_LARGEST_BLOCK_FLOOR_BYTES: usize = 32 * 1024;

        let contract = RouteExecutionClass::AsyncConfigRoute
            .worker_contract()
            .expect("config worker");
        let requirements = route_worker_memory_requirements(contract);

        assert!(!contract.reserves_tls_headroom);
        assert_eq!(
            requirements.required_internal,
            contract
                .stack_size
                .saturating_add(ROUTE_WORKER_NON_TLS_INTERNAL_HEADROOM)
        );
        assert_eq!(
            requirements.required_largest,
            contract
                .stack_size
                .saturating_add(ROUTE_WORKER_CONFIG_LARGEST_HEADROOM)
        );
        assert!(
            requirements.required_largest <= S3_NORMAL_CONFIG_LARGEST_BLOCK_FLOOR_BYTES,
            "core config writes must remain available at the observed normal ESP largest-block floor"
        );
    }

    #[test]
    fn diagnostic_worker_budget_preserves_tls_reserve_after_stack_allocation() {
        let contract = RouteExecutionClass::SlowDiagnosticRoute
            .worker_contract()
            .expect("diagnostic worker");
        let requirements = route_worker_memory_requirements(contract);

        assert!(contract.reserves_tls_headroom);
        assert_eq!(
            requirements.required_internal,
            contract
                .stack_size
                .saturating_add(crate::constants::TLS_ADMISSION_MIN_INTERNAL_BYTES)
        );
        assert_eq!(
            requirements.required_largest,
            contract
                .stack_size
                .saturating_add(crate::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES)
        );
    }

    #[test]
    fn route_specs_are_unique_by_method_and_path() {
        let mut seen = std::collections::BTreeSet::new();
        for group in route_spec_groups() {
            for spec in *group {
                assert!(
                    seen.insert((spec.method.as_str(), spec.path)),
                    "duplicate route spec for {} {}",
                    spec.method.as_str(),
                    spec.path
                );
            }
        }
    }
}
