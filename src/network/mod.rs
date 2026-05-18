//! Unified transport governance for HTTP/TLS/WSS ownership.
//! 统一网络治理面：HTTP/TLS/WSS、external WSS 模式状态、stream HTTP 槽位都以此为唯一权威。

use crate::channels::{
    connect_wss, connect_wss_with_headers_and_profile, WssConnectProfile, WssConnection,
};
use crate::config::AppConfig;
use crate::constants::MAX_CONCURRENT_HTTP;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::constants::{
    TLS_ADMISSION_MIN_INTERNAL_BYTES, TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES,
    TLS_ADMISSION_NO_PSRAM_MIN_BYTES,
};
use crate::error::{Error, Result};
use crate::platform::PlatformHttpClient;
use crate::Platform;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const TRY_INTERVAL_MS: u64 = 50;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const TRY_INTERVAL_MS_INTERACTIVE: u64 = 20;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const TRY_INTERVAL_MS_BACKGROUND: u64 = 90;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const BACKGROUND_PRE_ADMISSION_YIELD_MS: u64 = 120;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const REALTIME_WSS_DRAIN_WAIT_MS: u64 = 2_500;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const REALTIME_WSS_DRAIN_POLL_MS: u64 = 50;
const EXTERNAL_WSS_SUSPEND_WAIT_WARN_MS: u64 = 2_500;
const EXTERNAL_WSS_SUSPEND_WAIT_POLL_MS: u64 = 50;
const EXTERNAL_WSS_SUSPEND_WAIT_TIMEOUT_MS: u64 = 30_000;
const REALTIME_WSS_DRAIN_TIMEOUT_MS: u64 = 15_000;
const REALTIME_TLS_ADMISSION_RETRY_MAX: usize = 12;
const REALTIME_TLS_ADMISSION_RETRY_MS: u64 = 150;
const VOICE_EXCLUSIVE_WAIT_MS: u64 = 500;
const STREAM_HTTP_STATS_LOG_EVERY: u32 = 50;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const ROLE_SLOTS_MAX: usize = 16;

/// ESP external gateway WSS waits longer than generic HTTP after STA gets IP.
/// 2026-05-02 S3 boot-idle logs showed QQ token HTTP could still hit
/// `No buffer space available` about 9s after DHCP, while retrying around 15s
/// succeeded. External WSS is steady-state channel capacity, so this startup
/// settle window is preferable to consuming a failed TLS/HTTP attempt.
pub const EXTERNAL_WSS_OUTBOUND_SETTLE_SECS: u64 = 15;

static ACTIVE_HTTP_COUNT: AtomicU32 = AtomicU32::new(0);
static ACTIVE_WSS_COUNT: AtomicU32 = AtomicU32::new(0);
static ACTIVE_EXTERNAL_WSS_COUNT: AtomicU32 = AtomicU32::new(0);
static ACTIVE_REALTIME_WSS_COUNT: AtomicU32 = AtomicU32::new(0);
static EXTERNAL_WSS_CONNECTING_COUNT: AtomicU32 = AtomicU32::new(0);
static EXTERNAL_WSS_MANAGED_PRESENT: AtomicBool = AtomicBool::new(false);
static EXTERNAL_WSS_SUSPEND_REQUEST_COUNT: AtomicU32 = AtomicU32::new(0);
static EXTERNAL_WSS_VOICE_SUSPEND_REQUEST_COUNT: AtomicU32 = AtomicU32::new(0);
static EXTERNAL_WSS_CONFIG_SUSPEND_REQUEST_COUNT: AtomicU32 = AtomicU32::new(0);
static EXTERNAL_WSS_WORKER_EVICT_REQUEST_COUNT: AtomicU32 = AtomicU32::new(0);
static EXTERNAL_WSS_WORKER_EVICT_VOICE_REQUEST_COUNT: AtomicU32 = AtomicU32::new(0);
static EXTERNAL_WSS_WORKER_EVICT_CONFIG_REQUEST_COUNT: AtomicU32 = AtomicU32::new(0);
static EXTERNAL_WSS_WORKER_EVICT_OUTBOUND_REQUEST_COUNT: AtomicU32 = AtomicU32::new(0);
static EXTERNAL_WSS_SUSPENDED: AtomicBool = AtomicBool::new(false);
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
static TLS_PERMIT: Mutex<()> = Mutex::new(());
static STREAM_HTTP_SLOT_OPS: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static STREAM_EDITOR_HTTP_SLOT: std::cell::RefCell<Option<Box<dyn PlatformHttpClient>>> =
        const { std::cell::RefCell::new(None) };
}

/// HTTP client lane used by transport governance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttpClientClass {
    Background,
    Interactive,
}

/// Shared network governor used by startup assembly and runtime planes.
#[derive(Clone)]
pub struct NetworkGovernor {
    platform: Arc<dyn Platform>,
    config: Arc<AppConfig>,
}

impl NetworkGovernor {
    pub fn new(platform: Arc<dyn Platform>, config: Arc<AppConfig>) -> Self {
        Self { platform, config }
    }

    pub fn open_http_client(&self, class: HttpClientClass) -> Result<Box<dyn PlatformHttpClient>> {
        create_http_client_with_config(self.platform.as_ref(), self.config.as_ref(), class)
    }

    pub fn http_factory(&self, class: HttpClientClass) -> Arc<HttpFactory> {
        let governor = self.clone();
        Arc::new(move || governor.open_http_client(class))
    }
}

/// HTTP client factory function type.
pub type HttpFactory = dyn Fn() -> Result<Box<dyn PlatformHttpClient>> + Send + Sync;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExternalWssRuntimeSnapshot {
    pub managed_present: bool,
    pub connecting_count: u32,
    pub active_external_count: u32,
    pub active_realtime_count: u32,
    pub suspend_requested: bool,
    pub suspended: bool,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
mod thread_role_store {
    use crate::orchestrator::HttpThreadRole;
    use std::sync::{Mutex, OnceLock};

    fn role_slots() -> &'static Mutex<Vec<(usize, HttpThreadRole)>> {
        static SLOTS: OnceLock<Mutex<Vec<(usize, HttpThreadRole)>>> = OnceLock::new();
        SLOTS.get_or_init(|| Mutex::new(Vec::with_capacity(super::ROLE_SLOTS_MAX)))
    }

    #[inline]
    fn current_task_key() -> usize {
        crate::platform::task_affinity::current_task_handle_key()
    }

    pub fn set_current_http_thread_role(role: HttpThreadRole) {
        let task_key = current_task_key();
        if task_key == 0 {
            return;
        }
        let mut slots = role_slots().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, current_role)) = slots.iter_mut().find(|(key, _)| *key == task_key) {
            *current_role = role;
            return;
        }
        if slots.len() >= super::ROLE_SLOTS_MAX {
            slots.remove(0);
        }
        slots.push((task_key, role));
    }

    pub fn current_http_thread_role() -> HttpThreadRole {
        let task_key = current_task_key();
        if task_key == 0 {
            return HttpThreadRole::Background;
        }
        let slots = role_slots().lock().unwrap_or_else(|e| e.into_inner());
        slots
            .iter()
            .find(|(key, _)| *key == task_key)
            .map(|(_, role)| *role)
            .unwrap_or(HttpThreadRole::Background)
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod thread_role_store {
    use crate::orchestrator::HttpThreadRole;
    use std::cell::Cell;

    thread_local! {
        static HTTP_THREAD_ROLE: Cell<HttpThreadRole> = const { Cell::new(HttpThreadRole::Background) };
    }

    pub fn set_current_http_thread_role(role: HttpThreadRole) {
        HTTP_THREAD_ROLE.with(|r| r.set(role));
    }

    pub fn current_http_thread_role() -> HttpThreadRole {
        HTTP_THREAD_ROLE.with(Cell::get)
    }
}

/// Low-level transport HTTP permit guard.
pub struct TransportHttpPermitGuard {
    _tls_guard: Option<std::sync::MutexGuard<'static, ()>>,
}

impl Drop for TransportHttpPermitGuard {
    fn drop(&mut self) {
        ACTIVE_HTTP_COUNT.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Runtime class for a live WSS session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportWssProfile {
    ExternalGateway,
    RealtimeVoice,
}

/// Reason why the external WSS plane is being suspended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalWssSuspendReason {
    VoiceExclusive,
    ConfigPersisting,
    OutboundHttpRecovery,
}

impl ExternalWssSuspendReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VoiceExclusive => "voice_exclusive_suspend",
            Self::ConfigPersisting => "config_persisting_suspend",
            Self::OutboundHttpRecovery => "outbound_http_recovery_suspend",
        }
    }
}

impl From<WssConnectProfile> for TransportWssProfile {
    fn from(profile: WssConnectProfile) -> Self {
        match profile {
            WssConnectProfile::Gateway => Self::ExternalGateway,
            WssConnectProfile::Realtime => Self::RealtimeVoice,
        }
    }
}

/// Low-level transport WSS session guard.
pub struct TransportWssSessionGuard {
    profile: TransportWssProfile,
}

impl Drop for TransportWssSessionGuard {
    fn drop(&mut self) {
        ACTIVE_WSS_COUNT.fetch_sub(1, Ordering::Relaxed);
        match self.profile {
            TransportWssProfile::ExternalGateway => {
                ACTIVE_EXTERNAL_WSS_COUNT.fetch_sub(1, Ordering::Relaxed);
            }
            TransportWssProfile::RealtimeVoice => {
                ACTIVE_REALTIME_WSS_COUNT.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }
}

/// Scoped marker for an external WSS TCP/TLS/WebSocket handshake in progress.
pub struct ExternalWssConnectAttemptGuard;

impl Drop for ExternalWssConnectAttemptGuard {
    fn drop(&mut self) {
        let _ = EXTERNAL_WSS_CONNECTING_COUNT.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |count| count.checked_sub(1),
        );
    }
}

#[derive(Debug)]
pub struct ExternalWssLeaseGuard {
    owner: crate::runtime::lease::LeaseOwner,
    token: u64,
}

impl Drop for ExternalWssLeaseGuard {
    fn drop(&mut self) {
        let _ = crate::runtime::lease::release_token(
            crate::runtime::lease::LeaseKind::ExternalWss,
            self.owner,
            self.token,
        );
    }
}

#[must_use = "dropping the guard releases the TLS handshake lease"]
pub(crate) struct TlsHandshakeLeaseGuard {
    owner: crate::runtime::lease::LeaseOwner,
    token: u64,
}

impl Drop for TlsHandshakeLeaseGuard {
    fn drop(&mut self) {
        let _ = crate::runtime::lease::release_token(
            crate::runtime::lease::LeaseKind::TlsHandshake,
            self.owner,
            self.token,
        );
    }
}

struct LeasedExternalWssConnection {
    inner: Box<dyn WssConnection>,
    _external_wss_lease: ExternalWssLeaseGuard,
}

impl WssConnection for LeasedExternalWssConnection {
    fn send_binary(&mut self, data: &[u8]) -> Result<()> {
        self.inner.send_binary(data)
    }

    fn send_text(&mut self, text: &str) -> Result<()> {
        self.inner.send_text(text)
    }

    fn send_binary_owned(&mut self, data: Vec<u8>) -> Result<()> {
        self.inner.send_binary_owned(data)
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<crate::channels::WssEvent>> {
        self.inner.recv_timeout(timeout)
    }
}

/// Scoped external WSS suspend request. Multiple owners may hold this concurrently.
pub struct ExternalWssSuspendGuard {
    active: bool,
    reason: ExternalWssSuspendReason,
}

/// Scoped external WSS worker eviction request for voice-exclusive resource windows.
pub struct ExternalWssWorkerEvictGuard {
    active: bool,
    reason: ExternalWssSuspendReason,
}

#[derive(Debug)]
struct VoiceExclusiveLeaseGuard {
    owner: crate::runtime::lease::LeaseOwner,
    token: u64,
}

impl Drop for VoiceExclusiveLeaseGuard {
    fn drop(&mut self) {
        let _ = crate::runtime::lease::release_token(
            crate::runtime::lease::LeaseKind::VoiceExclusive,
            self.owner,
            self.token,
        );
    }
}

impl Drop for ExternalWssSuspendGuard {
    fn drop(&mut self) {
        if self.active {
            release_external_wss_suspend_request(self.reason);
            self.active = false;
        }
    }
}

impl Drop for ExternalWssWorkerEvictGuard {
    fn drop(&mut self) {
        if self.active {
            release_external_wss_worker_evict_request(self.reason);
            self.active = false;
        }
    }
}

pub fn set_current_http_thread_role(role: crate::orchestrator::HttpThreadRole) {
    thread_role_store::set_current_http_thread_role(role);
}

pub fn current_http_thread_role() -> crate::orchestrator::HttpThreadRole {
    thread_role_store::current_http_thread_role()
}

pub fn active_http_count() -> u32 {
    ACTIVE_HTTP_COUNT.load(Ordering::Relaxed)
}

pub fn active_wss_count() -> u32 {
    ACTIVE_WSS_COUNT.load(Ordering::Relaxed)
}

pub fn active_external_wss_count() -> u32 {
    ACTIVE_EXTERNAL_WSS_COUNT.load(Ordering::Relaxed)
}

pub fn active_realtime_wss_count() -> u32 {
    ACTIVE_REALTIME_WSS_COUNT.load(Ordering::Relaxed)
}

pub fn active_external_wss_lease_count() -> usize {
    crate::runtime::lease::active_count_for_kind(crate::runtime::lease::LeaseKind::ExternalWss)
}

pub fn external_wss_connecting_count() -> u32 {
    EXTERNAL_WSS_CONNECTING_COUNT.load(Ordering::Relaxed)
}

fn channel_wss_lease_owner(owner: &'static str) -> crate::runtime::lease::LeaseOwner {
    crate::runtime::lease::LeaseOwner::new("channel_wss", owner)
}

#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
fn http_tls_lease_owner(
    role: crate::orchestrator::HttpThreadRole,
) -> crate::runtime::lease::LeaseOwner {
    let name = match role {
        crate::orchestrator::HttpThreadRole::Interactive => "http_interactive",
        crate::orchestrator::HttpThreadRole::Io => "http_io",
        crate::orchestrator::HttpThreadRole::Background => "http_background",
    };
    crate::runtime::lease::LeaseOwner::new("http_tls", name)
}

pub fn acquire_external_wss_lease(owner: &'static str) -> Result<ExternalWssLeaseGuard> {
    acquire_external_wss_lease_inner(owner, None)
}

#[cfg(test)]
fn acquire_external_wss_lease_at(
    owner: &'static str,
    now_ms: u64,
) -> Result<ExternalWssLeaseGuard> {
    acquire_external_wss_lease_inner(owner, Some(now_ms))
}

fn acquire_external_wss_lease_inner(
    owner: &'static str,
    now_ms: Option<u64>,
) -> Result<ExternalWssLeaseGuard> {
    let owner = channel_wss_lease_owner(owner);
    let decision = match now_ms {
        Some(now_ms) => crate::runtime::lease::try_acquire_at(
            crate::runtime::lease::LeaseKind::ExternalWss,
            owner,
            crate::runtime::lease::LeaseMode::Exclusive,
            None,
            now_ms,
        ),
        None => crate::runtime::lease::try_acquire(
            crate::runtime::lease::LeaseKind::ExternalWss,
            owner,
            crate::runtime::lease::LeaseMode::Exclusive,
            None,
        ),
    };
    match decision {
        crate::runtime::lease::LeaseDecision::Acquired(record)
        | crate::runtime::lease::LeaseDecision::Reentered(record)
        | crate::runtime::lease::LeaseDecision::ReplacedExpired {
            current: record, ..
        } => Ok(ExternalWssLeaseGuard {
            owner,
            token: record.token,
        }),
        crate::runtime::lease::LeaseDecision::Denied(denial) => Err(Error::config(
            "external_wss_lease",
            format!(
                "owner={}:{} denied reason={} held_by={:?}",
                owner.plane, owner.name, denial.reason, denial.held_by
            ),
        )),
    }
}

fn acquire_tls_handshake_lease(owner: &'static str) -> Result<TlsHandshakeLeaseGuard> {
    let owner = channel_wss_lease_owner(owner);
    acquire_tls_handshake_lease_for_owner(owner)
}

fn acquire_tls_handshake_lease_for_owner(
    owner: crate::runtime::lease::LeaseOwner,
) -> Result<TlsHandshakeLeaseGuard> {
    match crate::runtime::lease::try_acquire(
        crate::runtime::lease::LeaseKind::TlsHandshake,
        owner,
        crate::runtime::lease::LeaseMode::Exclusive,
        None,
    ) {
        crate::runtime::lease::LeaseDecision::Acquired(record)
        | crate::runtime::lease::LeaseDecision::Reentered(record)
        | crate::runtime::lease::LeaseDecision::ReplacedExpired {
            current: record, ..
        } => Ok(TlsHandshakeLeaseGuard {
            owner,
            token: record.token,
        }),
        crate::runtime::lease::LeaseDecision::Denied(denial) => Err(Error::config(
            "tls_handshake_lease",
            format!(
                "owner={}:{} denied reason={} held_by={:?}",
                owner.plane, owner.name, denial.reason, denial.held_by
            ),
        )),
    }
}

/// Acquire a precise TLS handshake lease for HTTP client transports.
#[cfg(any(test, target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) fn acquire_http_client_tls_handshake_lease(
    role: crate::orchestrator::HttpThreadRole,
) -> Result<TlsHandshakeLeaseGuard> {
    acquire_tls_handshake_lease_for_owner(http_tls_lease_owner(role))
}

pub fn request_http_permit(
    priority: crate::orchestrator::Priority,
    timeout: Duration,
    pressure: crate::orchestrator::PressureLevel,
    _active_agent_tasks: u32,
) -> Result<TransportHttpPermitGuard> {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    let role = current_http_thread_role();
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    let active_agent_tasks = _active_agent_tasks;

    if pressure == crate::orchestrator::PressureLevel::Critical
        && priority < crate::orchestrator::Priority::High
    {
        return Err(Error::Other {
            source: Box::new(std::io::Error::other(
                "critical pressure, low priority rejected",
            )),
            stage: "tls_admission",
        });
    }

    let active_http = active_http_count();
    let active_wss = active_wss_count();
    let active_network = active_http.saturating_add(active_wss);
    if active_network >= MAX_CONCURRENT_HTTP as u32
        && priority < crate::orchestrator::Priority::High
    {
        return Err(Error::Other {
            source: Box::new(std::io::Error::other(format!(
                "max active network sessions reached (http={}, wss={}, limit={}), low priority rejected",
                active_http, active_wss, MAX_CONCURRENT_HTTP
            ))),
            stage: "tls_admission",
        });
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    let tls_guard = {
        if role == crate::orchestrator::HttpThreadRole::Background
            && priority <= crate::orchestrator::Priority::Normal
            && active_agent_tasks > 0
        {
            crate::platform::task_wdt::feed_current_task();
            std::thread::sleep(Duration::from_millis(BACKGROUND_PRE_ADMISSION_YIELD_MS));
        }
        let try_interval_ms = match role {
            crate::orchestrator::HttpThreadRole::Interactive => TRY_INTERVAL_MS_INTERACTIVE,
            crate::orchestrator::HttpThreadRole::Io => TRY_INTERVAL_MS,
            crate::orchestrator::HttpThreadRole::Background => TRY_INTERVAL_MS_BACKGROUND,
        };
        let start = Instant::now();
        let guard = loop {
            match TLS_PERMIT.try_lock() {
                Ok(guard) => break guard,
                Err(std::sync::TryLockError::Poisoned(e)) => {
                    log::warn!("[network] TLS permit mutex was poisoned, recovering");
                    TLS_PERMIT.clear_poison();
                    break e.into_inner();
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    if start.elapsed() >= timeout {
                        return Err(Error::Other {
                            source: Box::new(std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                "tls admission permit timeout",
                            )),
                            stage: "tls_admission",
                        });
                    }
                    crate::platform::task_wdt::feed_current_task();
                    std::thread::sleep(Duration::from_millis(try_interval_ms));
                }
            }
        };
        crate::metrics::record_http_permit_wait_ms(start.elapsed().as_millis());
        Some(guard)
    };
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let tls_guard = {
        let _ = timeout;
        None
    };

    check_internal_heap_for_tls()?;
    ACTIVE_HTTP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(TransportHttpPermitGuard {
        _tls_guard: tls_guard,
    })
}

pub fn begin_wss_session(profile: TransportWssProfile) -> TransportWssSessionGuard {
    ACTIVE_WSS_COUNT.fetch_add(1, Ordering::Relaxed);
    match profile {
        TransportWssProfile::ExternalGateway => {
            ACTIVE_EXTERNAL_WSS_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        TransportWssProfile::RealtimeVoice => {
            ACTIVE_REALTIME_WSS_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }
    TransportWssSessionGuard { profile }
}

pub fn begin_external_wss_connect_attempt() -> ExternalWssConnectAttemptGuard {
    EXTERNAL_WSS_CONNECTING_COUNT.fetch_add(1, Ordering::Relaxed);
    ExternalWssConnectAttemptGuard
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn check_internal_heap_for_tls() -> Result<()> {
    let snap = crate::orchestrator::memory_snapshot_live();
    crate::orchestrator::apply_memory_snapshot(snap);
    let free = snap.heap_free_internal;
    let largest = snap.heap_largest_block;
    let spiram = snap.heap_free_spiram;
    let min_free = if spiram > 0 {
        TLS_ADMISSION_MIN_INTERNAL_BYTES as u32
    } else {
        TLS_ADMISSION_NO_PSRAM_MIN_BYTES as u32
    };
    if free < min_free {
        return Err(Error::Other {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::OutOfMemory,
                format!(
                    "internal heap too low for TLS: free={} min={} spiram={}",
                    free, min_free, spiram
                ),
            )),
            stage: "tls_admission",
        });
    }
    if spiram > 0 && largest < TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32 {
        return Err(Error::Other {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::OutOfMemory,
                format!(
                    "internal heap fragmented for TLS: largest={} min={}",
                    largest, TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES
                ),
            )),
            stage: "tls_admission",
        });
    }
    Ok(())
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn check_internal_heap_for_tls() -> Result<()> {
    Ok(())
}

/// Create a governed HTTP client from an explicit platform/config pair.
pub fn create_http_client_with_config(
    platform: &dyn Platform,
    config: &AppConfig,
    class: HttpClientClass,
) -> Result<Box<dyn PlatformHttpClient>> {
    // On ESP, outbound callers own the STA settle wait. Keep it on the
    // governed HTTP entrypoint instead of blocking the entire startup path.
    if !crate::platform::wait_for_network_ready() {
        return Err(Error::config(
            "wifi_not_ready",
            "STA outbound network is not ready for HTTP client creation",
        ));
    }
    match class {
        HttpClientClass::Background => platform.create_http_client(config),
        HttpClientClass::Interactive => platform.create_interactive_http_client(config),
    }
}

fn ensure_outbound_network_ready(stage: &'static str, operation: &'static str) -> Result<()> {
    if crate::platform::wait_for_network_ready() {
        return Ok(());
    }
    Err(Error::config(
        stage,
        format!("STA outbound network is not ready for {operation}"),
    ))
}

/// RAII guard for entering voice-exclusive transport ownership.
pub struct VoiceExclusiveTransportGuard {
    log_tag: &'static str,
    worker_evict_guard: Option<ExternalWssWorkerEvictGuard>,
    suspend_guard: Option<ExternalWssSuspendGuard>,
    voice_exclusive_lease: Option<VoiceExclusiveLeaseGuard>,
}

impl VoiceExclusiveTransportGuard {
    pub fn enter(platform: &dyn Platform, log_tag: &'static str) -> Result<Self> {
        let voice_exclusive_lease = acquire_voice_exclusive_lease()?;
        let suspend_guard =
            begin_external_wss_suspend_request(ExternalWssSuspendReason::VoiceExclusive);
        let worker_evict_guard =
            begin_external_wss_worker_evict_request(ExternalWssSuspendReason::VoiceExclusive);
        crate::state::set_voice_exclusive_active(true);
        log::info!(
            "[{}] realtime session switching runtime mode (external WSS suspended)",
            log_tag
        );
        if let Err(error) = wait_for_external_wss_to_suspend_and_drain(platform, log_tag) {
            crate::state::set_voice_exclusive_active(false);
            drop(worker_evict_guard);
            drop(suspend_guard);
            drop(voice_exclusive_lease);
            return Err(error);
        }
        log::info!(
            "[{}] realtime session entering voice-exclusive mode",
            log_tag
        );
        Ok(Self {
            log_tag,
            worker_evict_guard: Some(worker_evict_guard),
            suspend_guard: Some(suspend_guard),
            voice_exclusive_lease: Some(voice_exclusive_lease),
        })
    }
}

impl Drop for VoiceExclusiveTransportGuard {
    fn drop(&mut self) {
        crate::state::set_voice_exclusive_active(false);
        self.worker_evict_guard.take();
        self.suspend_guard.take();
        self.voice_exclusive_lease.take();
        log::info!(
            "[{}] realtime session left voice-exclusive mode",
            self.log_tag
        );
    }
}

fn acquire_voice_exclusive_lease() -> Result<VoiceExclusiveLeaseGuard> {
    let owner = crate::runtime::lease::LeaseOwner::new("voice", "voice_exclusive");
    match crate::runtime::lease::try_acquire(
        crate::runtime::lease::LeaseKind::VoiceExclusive,
        owner,
        crate::runtime::lease::LeaseMode::Exclusive,
        None,
    ) {
        crate::runtime::lease::LeaseDecision::Acquired(record)
        | crate::runtime::lease::LeaseDecision::Reentered(record)
        | crate::runtime::lease::LeaseDecision::ReplacedExpired {
            current: record, ..
        } => Ok(VoiceExclusiveLeaseGuard {
            owner,
            token: record.token,
        }),
        crate::runtime::lease::LeaseDecision::Denied(denial) => Err(Error::config(
            "voice_exclusive_lease",
            format!(
                "owner={}:{} denied reason={} held_by={:?}",
                owner.plane, owner.name, denial.reason, denial.held_by
            ),
        )),
    }
}

pub fn external_wss_runtime_snapshot() -> ExternalWssRuntimeSnapshot {
    ExternalWssRuntimeSnapshot {
        managed_present: external_wss_managed_present(),
        connecting_count: external_wss_connecting_count(),
        active_external_count: active_external_wss_count(),
        active_realtime_count: active_realtime_wss_count(),
        suspend_requested: external_wss_suspend_requested(),
        suspended: external_wss_suspended(),
    }
}

pub fn external_wss_network_suspend_reason(
    snapshot: &crate::state::NetworkRuntimeSnapshot,
) -> Option<&'static str> {
    if !snapshot.sta_expected || !snapshot.sta_configured {
        Some("wifi_not_configured")
    } else if !snapshot.outbound_settled {
        Some("wifi_not_ready")
    } else if !snapshot.wall_clock_trustworthy {
        Some("wall_clock_untrusted")
    } else {
        None
    }
}

pub fn set_external_wss_managed_present(active: bool) {
    EXTERNAL_WSS_MANAGED_PRESENT.store(active, Ordering::Relaxed);
    if active {
        EXTERNAL_WSS_SUSPENDED.store(false, Ordering::Relaxed);
    } else {
        EXTERNAL_WSS_SUSPEND_REQUEST_COUNT.store(0, Ordering::Relaxed);
        EXTERNAL_WSS_VOICE_SUSPEND_REQUEST_COUNT.store(0, Ordering::Relaxed);
        EXTERNAL_WSS_CONFIG_SUSPEND_REQUEST_COUNT.store(0, Ordering::Relaxed);
        EXTERNAL_WSS_WORKER_EVICT_REQUEST_COUNT.store(0, Ordering::Relaxed);
        EXTERNAL_WSS_WORKER_EVICT_VOICE_REQUEST_COUNT.store(0, Ordering::Relaxed);
        EXTERNAL_WSS_WORKER_EVICT_CONFIG_REQUEST_COUNT.store(0, Ordering::Relaxed);
        EXTERNAL_WSS_WORKER_EVICT_OUTBOUND_REQUEST_COUNT.store(0, Ordering::Relaxed);
        EXTERNAL_WSS_CONNECTING_COUNT.store(0, Ordering::Relaxed);
        EXTERNAL_WSS_SUSPENDED.store(false, Ordering::Relaxed);
        let _ = crate::runtime::lease::release_kind(crate::runtime::lease::LeaseKind::ExternalWss);
    }
    crate::bg_timer::notify_deadline_changed();
}

pub fn mark_external_wss_worker_unloaded() {
    EXTERNAL_WSS_MANAGED_PRESENT.store(false, Ordering::Relaxed);
    EXTERNAL_WSS_CONNECTING_COUNT.store(0, Ordering::Relaxed);
    EXTERNAL_WSS_SUSPENDED.store(true, Ordering::Relaxed);
    let _ = crate::runtime::lease::release_kind(crate::runtime::lease::LeaseKind::ExternalWss);
    crate::bg_timer::notify_deadline_changed();
}

pub fn external_wss_managed_present() -> bool {
    EXTERNAL_WSS_MANAGED_PRESENT.load(Ordering::Relaxed)
}

pub fn request_external_wss_suspend() {
    request_external_wss_suspend_for_reason(ExternalWssSuspendReason::VoiceExclusive);
}

pub fn request_external_wss_suspend_for_reason(reason: ExternalWssSuspendReason) {
    increment_external_wss_suspend_reason(reason);
    EXTERNAL_WSS_SUSPEND_REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn request_external_wss_resume() {
    let reason = external_wss_suspend_reason().unwrap_or(ExternalWssSuspendReason::VoiceExclusive);
    release_external_wss_suspend_request(reason);
}

pub fn begin_external_wss_suspend_request(
    reason: ExternalWssSuspendReason,
) -> ExternalWssSuspendGuard {
    request_external_wss_suspend_for_reason(reason);
    ExternalWssSuspendGuard {
        active: true,
        reason,
    }
}

pub fn begin_external_wss_worker_evict_request(
    reason: ExternalWssSuspendReason,
) -> ExternalWssWorkerEvictGuard {
    request_external_wss_worker_evict_for_reason(reason);
    ExternalWssWorkerEvictGuard {
        active: true,
        reason,
    }
}

fn release_external_wss_suspend_request(reason: ExternalWssSuspendReason) {
    decrement_external_wss_suspend_reason(reason);
    let _ = EXTERNAL_WSS_SUSPEND_REQUEST_COUNT.fetch_update(
        Ordering::Relaxed,
        Ordering::Relaxed,
        |count| count.checked_sub(1),
    );
    if !external_wss_suspend_requested() {
        EXTERNAL_WSS_SUSPENDED.store(false, Ordering::Relaxed);
        EXTERNAL_WSS_VOICE_SUSPEND_REQUEST_COUNT.store(0, Ordering::Relaxed);
        EXTERNAL_WSS_CONFIG_SUSPEND_REQUEST_COUNT.store(0, Ordering::Relaxed);
    }
    crate::bg_timer::notify_deadline_changed();
}

fn request_external_wss_worker_evict_for_reason(reason: ExternalWssSuspendReason) {
    increment_external_wss_worker_evict_reason(reason);
    EXTERNAL_WSS_WORKER_EVICT_REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    crate::bg_timer::notify_deadline_changed();
}

fn release_external_wss_worker_evict_request(reason: ExternalWssSuspendReason) {
    decrement_external_wss_worker_evict_reason(reason);
    let _ = EXTERNAL_WSS_WORKER_EVICT_REQUEST_COUNT.fetch_update(
        Ordering::Relaxed,
        Ordering::Relaxed,
        |count| count.checked_sub(1),
    );
    if !external_wss_worker_evict_requested() {
        EXTERNAL_WSS_WORKER_EVICT_VOICE_REQUEST_COUNT.store(0, Ordering::Relaxed);
        EXTERNAL_WSS_WORKER_EVICT_CONFIG_REQUEST_COUNT.store(0, Ordering::Relaxed);
        EXTERNAL_WSS_WORKER_EVICT_OUTBOUND_REQUEST_COUNT.store(0, Ordering::Relaxed);
        if !external_wss_suspend_requested() {
            EXTERNAL_WSS_SUSPENDED.store(false, Ordering::Relaxed);
        }
    }
    crate::bg_timer::notify_deadline_changed();
}

fn increment_external_wss_suspend_reason(reason: ExternalWssSuspendReason) {
    match reason {
        ExternalWssSuspendReason::VoiceExclusive => {
            EXTERNAL_WSS_VOICE_SUSPEND_REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        ExternalWssSuspendReason::ConfigPersisting => {
            EXTERNAL_WSS_CONFIG_SUSPEND_REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        ExternalWssSuspendReason::OutboundHttpRecovery => {}
    }
}

fn decrement_external_wss_suspend_reason(reason: ExternalWssSuspendReason) {
    let counter = match reason {
        ExternalWssSuspendReason::VoiceExclusive => &EXTERNAL_WSS_VOICE_SUSPEND_REQUEST_COUNT,
        ExternalWssSuspendReason::ConfigPersisting => &EXTERNAL_WSS_CONFIG_SUSPEND_REQUEST_COUNT,
        ExternalWssSuspendReason::OutboundHttpRecovery => return,
    };
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
        count.checked_sub(1)
    });
}

fn increment_external_wss_worker_evict_reason(reason: ExternalWssSuspendReason) {
    match reason {
        ExternalWssSuspendReason::VoiceExclusive => {
            EXTERNAL_WSS_WORKER_EVICT_VOICE_REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        ExternalWssSuspendReason::ConfigPersisting => {
            EXTERNAL_WSS_WORKER_EVICT_CONFIG_REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        ExternalWssSuspendReason::OutboundHttpRecovery => {
            EXTERNAL_WSS_WORKER_EVICT_OUTBOUND_REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn decrement_external_wss_worker_evict_reason(reason: ExternalWssSuspendReason) {
    let counter = match reason {
        ExternalWssSuspendReason::VoiceExclusive => &EXTERNAL_WSS_WORKER_EVICT_VOICE_REQUEST_COUNT,
        ExternalWssSuspendReason::ConfigPersisting => {
            &EXTERNAL_WSS_WORKER_EVICT_CONFIG_REQUEST_COUNT
        }
        ExternalWssSuspendReason::OutboundHttpRecovery => {
            &EXTERNAL_WSS_WORKER_EVICT_OUTBOUND_REQUEST_COUNT
        }
    };
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
        count.checked_sub(1)
    });
}

pub fn external_wss_suspend_requested() -> bool {
    EXTERNAL_WSS_SUSPEND_REQUEST_COUNT.load(Ordering::Relaxed) > 0
}

pub fn external_wss_worker_evict_requested() -> bool {
    EXTERNAL_WSS_WORKER_EVICT_REQUEST_COUNT.load(Ordering::Relaxed) > 0
}

pub fn set_external_wss_suspended(active: bool) {
    EXTERNAL_WSS_SUSPENDED.store(active, Ordering::Relaxed);
}

pub fn external_wss_suspended() -> bool {
    EXTERNAL_WSS_SUSPENDED.load(Ordering::Relaxed)
}

pub fn external_wss_suspend_reason() -> Option<ExternalWssSuspendReason> {
    if EXTERNAL_WSS_VOICE_SUSPEND_REQUEST_COUNT.load(Ordering::Relaxed) > 0 {
        Some(ExternalWssSuspendReason::VoiceExclusive)
    } else if EXTERNAL_WSS_CONFIG_SUSPEND_REQUEST_COUNT.load(Ordering::Relaxed) > 0 {
        Some(ExternalWssSuspendReason::ConfigPersisting)
    } else {
        None
    }
}

pub fn external_wss_worker_evict_reason() -> Option<ExternalWssSuspendReason> {
    if EXTERNAL_WSS_WORKER_EVICT_VOICE_REQUEST_COUNT.load(Ordering::Relaxed) > 0 {
        Some(ExternalWssSuspendReason::VoiceExclusive)
    } else if EXTERNAL_WSS_WORKER_EVICT_CONFIG_REQUEST_COUNT.load(Ordering::Relaxed) > 0 {
        Some(ExternalWssSuspendReason::ConfigPersisting)
    } else if EXTERNAL_WSS_WORKER_EVICT_OUTBOUND_REQUEST_COUNT.load(Ordering::Relaxed) > 0 {
        Some(ExternalWssSuspendReason::OutboundHttpRecovery)
    } else {
        None
    }
}

fn external_wss_suspend_target_drained() -> bool {
    active_external_wss_count() == 0
        && active_external_wss_lease_count() == 0
        && external_wss_connecting_count() == 0
        && (!external_wss_managed_present()
            || external_wss_suspended()
            || external_wss_suspend_requested())
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn realtime_voice_pre_spawn_largest_floor() -> usize {
    realtime_voice_pre_spawn_largest_floor_value()
}

#[cfg(test)]
fn realtime_voice_pre_spawn_largest_floor_for_tests() -> usize {
    realtime_voice_pre_spawn_largest_floor_value()
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
fn realtime_voice_pre_spawn_largest_floor_value() -> usize {
    // This gate runs before the connect worker exists. It must prove the worker
    // can be allocated, but the worker re-checks the TLS largest-block floor
    // after its stack is actually allocated and before the handshake starts.
    crate::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES
        .max(crate::util::STACK_VOICE_REALTIME_CONNECT)
}

/// Wait until the external WSS plane is not established or handshaking.
pub fn wait_for_external_wss_suspend(tag: &str) {
    if let Err(error) = wait_for_external_wss_suspend_result(tag) {
        log::warn!("[{}] external WSS suspend wait failed: {}", tag, error);
    }
}

pub fn wait_for_external_wss_suspend_result(tag: &str) -> Result<()> {
    if !external_wss_suspend_requested() {
        return Ok(());
    }
    wait_for_external_wss_suspend_with_timeout(
        tag,
        Duration::from_millis(EXTERNAL_WSS_SUSPEND_WAIT_TIMEOUT_MS),
        Duration::from_millis(EXTERNAL_WSS_SUSPEND_WAIT_POLL_MS),
    )
}

fn wait_for_external_wss_suspend_with_timeout(
    tag: &str,
    timeout: Duration,
    poll: Duration,
) -> Result<()> {
    let mut next_warn_at =
        Instant::now() + Duration::from_millis(EXTERNAL_WSS_SUSPEND_WAIT_WARN_MS);
    let deadline = Instant::now() + timeout;
    loop {
        if external_wss_suspend_target_drained() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Error::config(
                "external_wss_suspend_timeout",
                format!(
                    "external WSS suspend timed out tag={} active_external_wss={} active_realtime_wss={} active_wss_leases={} connecting_wss={}",
                    tag,
                    active_external_wss_count(),
                    active_realtime_wss_count(),
                    active_external_wss_lease_count(),
                    external_wss_connecting_count()
                ),
            ));
        }
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        crate::platform::task_wdt::feed_current_task();
        if Instant::now() >= next_warn_at {
            log::warn!(
                "[{}] waiting for external WSS suspend reason={} active_external_wss={} active_realtime_wss={} active_wss_leases={} connecting_wss={}",
                tag,
                external_wss_suspend_reason()
                    .map(ExternalWssSuspendReason::as_str)
                    .unwrap_or("unknown"),
                active_external_wss_count(),
                active_realtime_wss_count(),
                active_external_wss_lease_count(),
                external_wss_connecting_count()
            );
            next_warn_at =
                Instant::now() + Duration::from_millis(EXTERNAL_WSS_SUSPEND_WAIT_WARN_MS);
        }
        std::thread::sleep(poll);
    }
}

/// Wait until the external WSS plane is allowed to resume.
pub fn wait_for_external_wss_resume(tag: &str) {
    let mut logged = false;
    while external_wss_suspend_requested() {
        if external_wss_worker_evict_requested() {
            set_external_wss_suspended(true);
            return;
        }
        if !logged {
            log::info!(
                "[{}] external WSS suspended reason={}",
                tag,
                external_wss_suspend_reason()
                    .map(ExternalWssSuspendReason::as_str)
                    .unwrap_or("unknown")
            );
            logged = true;
        }
        set_external_wss_suspended(true);
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        crate::platform::task_wdt::feed_current_task();
        std::thread::sleep(Duration::from_millis(VOICE_EXCLUSIVE_WAIT_MS));
    }
    if logged {
        set_external_wss_suspended(false);
        log::info!("[{}] external WSS resume", tag);
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn wait_for_external_wss_to_suspend_and_drain(
    platform: &dyn Platform,
    log_tag: &str,
) -> Result<()> {
    let mut next_warn_at = Instant::now() + Duration::from_millis(REALTIME_WSS_DRAIN_WAIT_MS);
    let deadline = Instant::now() + Duration::from_millis(REALTIME_WSS_DRAIN_TIMEOUT_MS);
    loop {
        crate::platform::task_wdt::feed_current_task();
        let active_wss = active_wss_count();
        let active_wss_leases = active_external_wss_lease_count();
        let connecting_wss = external_wss_connecting_count();
        let active_outbound_workers = crate::channels::active_os_outbound_worker_count();
        let mode_switched = external_wss_suspend_target_drained();
        let snap = platform.memory_snapshot();
        let min_free = if snap.heap_free_spiram > 0 {
            TLS_ADMISSION_MIN_INTERNAL_BYTES as u32
        } else {
            TLS_ADMISSION_NO_PSRAM_MIN_BYTES as u32
        };
        let min_largest = realtime_voice_pre_spawn_largest_floor() as u32;
        let enough_free = snap.heap_free_internal >= min_free;
        let enough_largest = snap.heap_free_spiram == 0 || snap.heap_largest_block >= min_largest;
        let worker_evicted =
            !external_wss_worker_evict_requested() || !external_wss_managed_present();
        if mode_switched
            && active_wss == 0
            && active_wss_leases == 0
            && connecting_wss == 0
            && active_outbound_workers == 0
            && worker_evicted
            && enough_free
            && enough_largest
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Error::config(
                "voice_exclusive_wss_drain_timeout",
                format!(
                    "external WSS did not drain before realtime connect active_wss={} active_wss_leases={} connecting_wss={} active_outbound_workers={} worker_evicted={} free={} largest={} largest_min={} spiram={}",
                    active_wss,
                    active_wss_leases,
                    connecting_wss,
                    active_outbound_workers,
                    worker_evicted,
                    snap.heap_free_internal,
                    snap.heap_largest_block,
                    min_largest,
                    snap.heap_free_spiram
                ),
            ));
        }
        if Instant::now() >= next_warn_at {
            log::warn!(
                "[{}] waiting for external WSS suspend/resources before realtime connect active_wss={} active_wss_leases={} connecting_wss={} active_outbound_workers={} worker_evicted={} free={} largest={} largest_min={} spiram={}",
                log_tag,
                active_wss,
                active_wss_leases,
                connecting_wss,
                active_outbound_workers,
                worker_evicted,
                snap.heap_free_internal,
                snap.heap_largest_block,
                min_largest,
                snap.heap_free_spiram
            );
            next_warn_at = Instant::now() + Duration::from_millis(REALTIME_WSS_DRAIN_WAIT_MS);
        }
        std::thread::sleep(Duration::from_millis(REALTIME_WSS_DRAIN_POLL_MS));
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn wait_for_external_wss_to_suspend_and_drain(
    _platform: &dyn Platform,
    log_tag: &str,
) -> Result<()> {
    wait_for_external_wss_suspend_with_timeout(
        log_tag,
        Duration::from_millis(REALTIME_WSS_DRAIN_TIMEOUT_MS),
        Duration::from_millis(EXTERNAL_WSS_SUSPEND_WAIT_POLL_MS),
    )
}

fn maybe_log_stream_http_stats(trigger: &str) {
    let ops = STREAM_HTTP_SLOT_OPS.load(Ordering::Relaxed);
    if ops == 0 || !ops.is_multiple_of(STREAM_HTTP_STATS_LOG_EVERY) {
        return;
    }
    let snap = crate::metrics::snapshot();
    let hits = snap.stream_http_reuse_hits;
    let creates = snap.stream_http_creates;
    let resets = snap.stream_http_resets;
    let invalidates = snap.stream_http_invalidates;
    let total = hits + creates;
    let reuse_rate = if total == 0 { 0 } else { (hits * 100) / total };
    log::info!(
        "[network] stream_http_stats trigger={} ops={} reuse_hits={} creates={} resets={} invalidates={} reuse_rate={}%",
        trigger,
        ops,
        hits,
        creates,
        resets,
        invalidates,
        reuse_rate
    );
}

pub fn invalidate_stream_http_slot(reason: &str) {
    STREAM_EDITOR_HTTP_SLOT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.take().is_some() {
            crate::metrics::record_stream_http_invalidate();
            log::warn!("[network] stream_http invalidate reason={}", reason);
        }
    });
}

fn reset_stream_http_slot(reason: &str) -> bool {
    let mut reset = false;
    STREAM_EDITOR_HTTP_SLOT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(http) = slot.as_mut() {
            PlatformHttpClient::reset_connection_for_retry(http.as_mut());
            crate::metrics::record_stream_http_reset();
            reset = true;
        }
    });
    if reset {
        log::warn!("[network] stream_http reset_for_retry reason={}", reason);
    }
    reset
}

fn with_stream_http_slot<T>(
    create_http: &HttpFactory,
    op_name: &str,
    op: &mut dyn FnMut(&mut Box<dyn PlatformHttpClient>) -> Result<T>,
) -> Result<T> {
    STREAM_EDITOR_HTTP_SLOT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(create_http()?);
            crate::metrics::record_stream_http_create();
            log::info!("[network] stream_http create op={}", op_name);
        } else {
            crate::metrics::record_stream_http_reuse();
        }
        STREAM_HTTP_SLOT_OPS.fetch_add(1, Ordering::Relaxed);
        let http = slot
            .as_mut()
            .ok_or_else(|| Error::config("stream_http", "http client missing in slot"))?;
        op(http)
    })
}

pub fn execute_stream_http_op<T, F>(
    create_http: &HttpFactory,
    op_name: &str,
    mut op: F,
) -> Result<T>
where
    F: FnMut(&mut Box<dyn PlatformHttpClient>) -> Result<T>,
{
    let first = with_stream_http_slot(create_http, op_name, &mut op);
    match first {
        Ok(v) => {
            maybe_log_stream_http_stats(op_name);
            Ok(v)
        }
        Err(first_err) => {
            let reason = format!("{} first_try: {}", op_name, first_err);
            let _ = reset_stream_http_slot(&reason);
            let second = with_stream_http_slot(create_http, op_name, &mut op);
            match second {
                Ok(v) => {
                    maybe_log_stream_http_stats(op_name);
                    Ok(v)
                }
                Err(second_err) => {
                    invalidate_stream_http_slot(&format!("{} second_try: {}", op_name, second_err));
                    maybe_log_stream_http_stats(op_name);
                    Err(second_err)
                }
            }
        }
    }
}

fn connect_external_wss_with_connector<C, Conn>(
    url: &str,
    owner: &'static str,
    mut connect: Conn,
) -> Result<Box<dyn WssConnection>>
where
    C: WssConnection + 'static,
    Conn: FnMut(&str) -> Result<C>,
{
    loop {
        if external_wss_worker_evict_requested() {
            return Err(Error::config(
                "external_wss_worker_evict",
                "external WSS worker evicted for voice-exclusive resource window",
            ));
        }
        ensure_outbound_network_ready("external_wss_network_ready", "external WSS connect")?;
        wait_for_external_wss_resume("external_wss_connect");
        if external_wss_worker_evict_requested() {
            return Err(Error::config(
                "external_wss_worker_evict",
                "external WSS worker evicted for voice-exclusive resource window",
            ));
        }
        let _connect_guard = begin_external_wss_connect_attempt();
        if external_wss_suspend_requested() {
            continue;
        }
        let external_wss_lease = acquire_external_wss_lease(owner)?;
        if external_wss_suspend_requested() {
            drop(external_wss_lease);
            continue;
        }
        let conn = {
            let _tls_handshake_lease = acquire_tls_handshake_lease(owner)?;
            connect(url)?
        };
        return Ok(Box::new(LeasedExternalWssConnection {
            inner: Box::new(conn),
            _external_wss_lease: external_wss_lease,
        }));
    }
}

/// Connect an external gateway WSS through the unified transport governor.
pub fn connect_external_wss(url: &str, owner: &'static str) -> Result<Box<dyn WssConnection>> {
    connect_external_wss_with_connector(url, owner, connect_wss)
}

fn connect_realtime_wss(url: &str, headers: &[(&str, &str)]) -> Result<Box<dyn WssConnection>> {
    Ok(Box::new(connect_wss_with_headers_and_profile(
        url,
        headers,
        WssConnectProfile::Realtime,
    )?))
}

/// Connect a realtime WSS through the unified transport governor.
pub fn connect_realtime_wss_with_retry(
    platform: &dyn Platform,
    url: &str,
    headers: &[(&str, &str)],
) -> Result<Box<dyn WssConnection>> {
    ensure_outbound_network_ready("realtime_wss_network_ready", "realtime WSS connect")?;
    let mut last_err: Option<Error> = None;
    for attempt in 0..REALTIME_TLS_ADMISSION_RETRY_MAX {
        crate::platform::task_wdt::feed_current_task();
        wait_for_realtime_admission_window(platform);
        let result = {
            let _tls_handshake_lease = acquire_tls_handshake_lease_for_owner(
                crate::runtime::lease::LeaseOwner::new("voice", "voice_realtime_connect"),
            )?;
            connect_realtime_wss(url, headers)
        };
        match result {
            Ok(conn) => return Ok(conn),
            Err(err)
                if err.is_tls_admission() && attempt + 1 < REALTIME_TLS_ADMISSION_RETRY_MAX =>
            {
                let snap = platform.memory_snapshot();
                log::warn!(
                    "[network] realtime tls admission retry {}/{} internal_free={} largest={} spiram={}",
                    attempt + 1,
                    REALTIME_TLS_ADMISSION_RETRY_MAX,
                    snap.heap_free_internal,
                    snap.heap_largest_block,
                    snap.heap_free_spiram
                );
                last_err = Some(err);
                crate::platform::task_wdt::feed_current_task();
                std::thread::sleep(Duration::from_millis(REALTIME_TLS_ADMISSION_RETRY_MS));
            }
            Err(err) => return Err(err),
        }
    }
    Err(last_err
        .unwrap_or_else(|| Error::config("network_realtime_wss", "realtime wss connect failed")))
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn wait_for_realtime_admission_window(platform: &dyn Platform) {
    let deadline = Instant::now()
        + Duration::from_millis(
            (REALTIME_TLS_ADMISSION_RETRY_MAX as u64) * REALTIME_TLS_ADMISSION_RETRY_MS,
        );
    while Instant::now() < deadline {
        crate::platform::task_wdt::feed_current_task();
        let snap = platform.memory_snapshot();
        let min_free = if snap.heap_free_spiram > 0 {
            TLS_ADMISSION_MIN_INTERNAL_BYTES as u32
        } else {
            TLS_ADMISSION_NO_PSRAM_MIN_BYTES as u32
        };
        let enough_free = snap.heap_free_internal >= min_free;
        let enough_largest = snap.heap_free_spiram == 0
            || snap.heap_largest_block >= TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32;
        let no_external_wss =
            active_external_wss_count() == 0 && external_wss_connecting_count() == 0;
        if enough_free && enough_largest && no_external_wss {
            return;
        }
        std::thread::sleep(Duration::from_millis(REALTIME_TLS_ADMISSION_RETRY_MS));
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn wait_for_realtime_admission_window(_platform: &dyn Platform) {}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeWssConnection;

    impl WssConnection for FakeWssConnection {
        fn send_binary(&mut self, _data: &[u8]) -> Result<()> {
            Ok(())
        }

        fn recv_timeout(
            &mut self,
            _timeout: Duration,
        ) -> Result<Option<crate::channels::WssEvent>> {
            Ok(None)
        }
    }

    #[test]
    fn external_wss_runtime_round_trips() {
        let _guard = crate::state::test_state_guard();
        set_external_wss_managed_present(true);
        {
            let _connect = begin_external_wss_connect_attempt();
            let snap = external_wss_runtime_snapshot();
            assert_eq!(snap.connecting_count, 1);
            assert_eq!(external_wss_connecting_count(), 1);
        }
        request_external_wss_suspend();
        set_external_wss_suspended(true);
        let snap = external_wss_runtime_snapshot();
        assert!(snap.managed_present);
        assert_eq!(snap.connecting_count, 0);
        assert!(snap.suspend_requested);
        assert!(snap.suspended);
        request_external_wss_resume();
        set_external_wss_managed_present(false);
        let snap = external_wss_runtime_snapshot();
        assert!(!snap.managed_present);
        assert!(!snap.suspend_requested);
        assert!(!snap.suspended);
    }

    #[test]
    fn wss_session_counts_external_and_realtime_profiles_separately() {
        let _guard = crate::state::test_state_guard();
        assert_eq!(active_wss_count(), 0);
        assert_eq!(active_external_wss_count(), 0);
        assert_eq!(active_realtime_wss_count(), 0);

        let external = begin_wss_session(TransportWssProfile::ExternalGateway);
        assert_eq!(active_wss_count(), 1);
        assert_eq!(active_external_wss_count(), 1);
        assert_eq!(active_realtime_wss_count(), 0);

        let realtime = begin_wss_session(TransportWssProfile::RealtimeVoice);
        assert_eq!(active_wss_count(), 2);
        assert_eq!(active_external_wss_count(), 1);
        assert_eq!(active_realtime_wss_count(), 1);

        drop(external);
        assert_eq!(active_wss_count(), 1);
        assert_eq!(active_external_wss_count(), 0);
        assert_eq!(active_realtime_wss_count(), 1);

        drop(realtime);
        assert_eq!(active_wss_count(), 0);
        assert_eq!(active_external_wss_count(), 0);
        assert_eq!(active_realtime_wss_count(), 0);
    }

    #[test]
    fn external_wss_suspend_reason_round_trips() {
        let _guard = crate::state::test_state_guard();
        set_external_wss_managed_present(false);
        let guard = begin_external_wss_suspend_request(ExternalWssSuspendReason::ConfigPersisting);
        assert_eq!(
            external_wss_suspend_reason().map(ExternalWssSuspendReason::as_str),
            Some("config_persisting_suspend")
        );
        drop(guard);
        assert_eq!(external_wss_suspend_reason(), None);
    }

    #[test]
    fn external_wss_suspend_reason_tracks_concurrent_owners() {
        let _guard = crate::state::test_state_guard();
        set_external_wss_managed_present(false);
        let config = begin_external_wss_suspend_request(ExternalWssSuspendReason::ConfigPersisting);
        assert_eq!(
            external_wss_suspend_reason(),
            Some(ExternalWssSuspendReason::ConfigPersisting)
        );

        let voice = begin_external_wss_suspend_request(ExternalWssSuspendReason::VoiceExclusive);
        assert_eq!(
            external_wss_suspend_reason(),
            Some(ExternalWssSuspendReason::VoiceExclusive)
        );

        drop(voice);
        assert!(external_wss_suspend_requested());
        assert_eq!(
            external_wss_suspend_reason(),
            Some(ExternalWssSuspendReason::ConfigPersisting)
        );

        drop(config);
        assert!(!external_wss_suspend_requested());
        assert_eq!(external_wss_suspend_reason(), None);
    }

    #[test]
    fn voice_exclusive_evict_request_round_trips() {
        let _guard = crate::state::test_state_guard();
        set_external_wss_managed_present(false);
        assert!(!external_wss_worker_evict_requested());

        let evict =
            begin_external_wss_worker_evict_request(ExternalWssSuspendReason::VoiceExclusive);
        assert!(external_wss_worker_evict_requested());
        assert_eq!(
            external_wss_worker_evict_reason().map(ExternalWssSuspendReason::as_str),
            Some("voice_exclusive_suspend")
        );

        drop(evict);
        assert!(!external_wss_worker_evict_requested());
        assert_eq!(external_wss_worker_evict_reason(), None);
    }

    #[test]
    fn realtime_pre_spawn_largest_floor_uses_two_stage_tls_admission() {
        assert_eq!(
            realtime_voice_pre_spawn_largest_floor_for_tests(),
            crate::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES
                .max(crate::util::STACK_VOICE_REALTIME_CONNECT)
        );
    }

    #[test]
    fn external_wss_lease_blocks_other_channel_until_drop() {
        let _guard = crate::state::test_state_guard();
        let _lease_guard = crate::runtime::lease::lease_test_guard();

        let qq = acquire_external_wss_lease_at("qq_ws", 100).expect("qq external wss lease");
        assert_eq!(active_external_wss_lease_count(), 1);

        let denied =
            acquire_external_wss_lease_at("feishu_ws", 101).expect_err("feishu should conflict");
        assert_eq!(denied.stage(), "external_wss_lease");
        assert!(denied.to_string().contains("exclusive_conflict"));

        drop(qq);
        assert_eq!(active_external_wss_lease_count(), 0);
        let _feishu =
            acquire_external_wss_lease_at("feishu_ws", 102).expect("feishu external wss lease");
        assert_eq!(active_external_wss_lease_count(), 1);
    }

    #[test]
    fn external_wss_connect_wrapper_holds_session_lease_and_releases_handshake() {
        let _guard = crate::state::test_state_guard();
        let _lease_guard = crate::runtime::lease::lease_test_guard();
        set_external_wss_managed_present(true);

        let conn = connect_external_wss_with_connector("wss://example.test", "qq_ws", |_url| {
            assert_eq!(
                crate::runtime::lease::active_count_for_kind(
                    crate::runtime::lease::LeaseKind::ExternalWss
                ),
                1
            );
            assert_eq!(
                crate::runtime::lease::active_count_for_kind(
                    crate::runtime::lease::LeaseKind::TlsHandshake
                ),
                1
            );
            Ok(FakeWssConnection)
        })
        .expect("external wss connect");

        assert_eq!(active_external_wss_lease_count(), 1);
        assert_eq!(
            crate::runtime::lease::active_count_for_kind(
                crate::runtime::lease::LeaseKind::TlsHandshake
            ),
            0
        );

        drop(conn);
        assert_eq!(active_external_wss_lease_count(), 0);
        set_external_wss_managed_present(false);
    }

    #[test]
    fn external_wss_connect_failure_releases_session_and_handshake_leases() {
        let _guard = crate::state::test_state_guard();
        let _lease_guard = crate::runtime::lease::lease_test_guard();
        set_external_wss_managed_present(true);

        let error = match connect_external_wss_with_connector(
            "wss://example.test",
            "qq_ws",
            |_url| -> Result<FakeWssConnection> { Err(Error::config("fake_wss", "boom")) },
        ) {
            Ok(_) => panic!("connect should fail"),
            Err(error) => error,
        };

        assert_eq!(error.stage(), "fake_wss");
        assert_eq!(active_external_wss_lease_count(), 0);
        assert_eq!(
            crate::runtime::lease::active_count_for_kind(
                crate::runtime::lease::LeaseKind::TlsHandshake
            ),
            0
        );
        set_external_wss_managed_present(false);
    }

    #[test]
    fn http_client_tls_handshake_lease_releases_on_drop() {
        let _guard = crate::state::test_state_guard();
        let _lease_guard = crate::runtime::lease::lease_test_guard();

        let lease = acquire_http_client_tls_handshake_lease(
            crate::orchestrator::HttpThreadRole::Interactive,
        )
        .expect("http tls lease");
        assert_eq!(
            crate::runtime::lease::active_count_for_kind(
                crate::runtime::lease::LeaseKind::TlsHandshake
            ),
            1
        );

        drop(lease);
        assert_eq!(
            crate::runtime::lease::active_count_for_kind(
                crate::runtime::lease::LeaseKind::TlsHandshake
            ),
            0
        );
    }

    #[test]
    fn external_wss_worker_unload_does_not_release_other_tls_handshake_owner() {
        let _guard = crate::state::test_state_guard();
        let _lease_guard = crate::runtime::lease::lease_test_guard();
        let owner = crate::runtime::lease::LeaseOwner::new("http", "config_save");
        let _lease = crate::runtime::lease::try_acquire(
            crate::runtime::lease::LeaseKind::TlsHandshake,
            owner,
            crate::runtime::lease::LeaseMode::Exclusive,
            None,
        );
        assert_eq!(
            crate::runtime::lease::active_count_for_kind(
                crate::runtime::lease::LeaseKind::TlsHandshake
            ),
            1
        );

        mark_external_wss_worker_unloaded();

        assert_eq!(
            crate::runtime::lease::active_count_for_kind(
                crate::runtime::lease::LeaseKind::TlsHandshake
            ),
            1
        );
    }

    #[test]
    fn external_wss_suspend_timeout_returns_drain_failure() {
        let _guard = crate::state::test_state_guard();
        let _lease_guard = crate::runtime::lease::lease_test_guard();
        set_external_wss_managed_present(true);
        request_external_wss_suspend();
        let _wss_lease =
            acquire_external_wss_lease_at("qq_ws", 200).expect("held external wss lease");

        let error = match wait_for_external_wss_suspend_with_timeout(
            "test",
            Duration::from_millis(1),
            Duration::from_millis(1),
        ) {
            Ok(()) => panic!("held external WSS lease should prevent suspend drain"),
            Err(error) => error,
        };

        assert_eq!(error.stage(), "external_wss_suspend_timeout");
        set_external_wss_managed_present(false);
    }
}
