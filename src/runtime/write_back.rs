//! Debounced write-back wrappers for hot runtime stores.
//! 将热路径上的小型持久化写入从用户/语音临界区中移出，复用现有 delayed task，
//! 并由独立 write-back 执行面承接 SPIFFS/serde/session flush 重活。

use crate::agent::{ActiveWorkRecord, ActiveWorkStore};
use crate::channels::inbound_backpressure::{self, EventIngressSource};
use crate::error::{Error, Result};
use crate::memory::{
    derive_recent_persona_evidence, AutonomyStrategy, AutonomyStrategyStore, CoreRevisionLedger,
    CoreRevisionLedgerStore, ExecutionState, ExecutionStateStore, FeltSignificance,
    FeltSignificanceStore, ImportantMessageStore, InnerConflict, InnerConflictStore, InnerLife,
    InnerLifeStore, LongTermMemoryExtractionState, LongTermMemoryExtractionStateStore,
    MentalPrivacyState, MentalPrivacyStore, OuterVoice, OuterVoiceStore, RecentPersonaEvidence,
    RelationshipConstitution, RelationshipConstitutionStore, RelationshipPortfolio,
    RelationshipPortfolioStore, RelationshipTopology, RelationshipTopologyStore, SelfAuthoredCore,
    SelfAuthoredCoreStore, SelfContinuity, SelfContinuityStore, SelfModel, SelfModelStore,
    SessionMessage, SessionStore, SessionSummaryStore, TemperamentContinuity,
    TemperamentContinuityStore, TurnLedger, TurnLedgerStore, WorldSense, WorldSenseStore,
    RECENT_PERSONA_EVIDENCE_MEANINGFUL_TURNS,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const WRITE_BACK_DELAY_MS: u64 = 75;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const WRITE_BACK_DELAY_MS: u64 = 25;

type WriteBackTask = Box<dyn FnOnce() + Send + 'static>;

struct WriteBackJob {
    label: &'static str,
    due_at: Instant,
    task: Option<WriteBackTask>,
}

struct WriteBackQueueState {
    jobs: Vec<WriteBackJob>,
    worker_started: bool,
}

struct WriteBackScheduler {
    state: Mutex<WriteBackQueueState>,
}

// Keep enough slots for one flush job from every buffered runtime store family.
// This is a queue of small delayed closures, not a stack reservation.
const WRITE_BACK_RUNTIME_DOMAIN_FLOOR: usize = 24;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) const WRITE_BACK_QUEUE_MAX: usize = WRITE_BACK_RUNTIME_DOMAIN_FLOOR + 8;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(crate) const WRITE_BACK_QUEUE_MAX: usize = WRITE_BACK_RUNTIME_DOMAIN_FLOOR + 40;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
// SPIFFS + serde flushes need their own ESP stack budget, separate from agent_loop.
// 24KB matches the old bg_timer write-back budget without adding an oversized SRAM reserve.
const WRITE_BACK_WORKER_STACK: usize = 24 * 1024;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
// Host tests and Linux embedded targets still execute the same serde/session flush
// path; 8KB can overflow before the worker reaches its idle-stop release point.
const WRITE_BACK_WORKER_STACK: usize = 24 * 1024;

const WRITE_BACK_ADMISSION_DEFER_MS: u64 = 500;
const WRITE_BACK_FOREGROUND_ADMISSION_DEFER_MS: u64 = 2_000;
#[cfg(test)]
const WRITE_BACK_IDLE_STOP_MS: u64 = 30;
#[cfg(all(any(target_arch = "xtensa", target_arch = "riscv32"), not(test)))]
const WRITE_BACK_IDLE_STOP_MS: u64 = 1_500;
#[cfg(all(not(any(target_arch = "xtensa", target_arch = "riscv32")), not(test)))]
const WRITE_BACK_IDLE_STOP_MS: u64 = 1_000;
#[cfg(test)]
const WRITE_BACK_POLL_MS: u64 = 5;
#[cfg(not(test))]
const WRITE_BACK_POLL_MS: u64 = 25;

const WRITE_BACK_LEASE_OWNER: crate::runtime::lease::LeaseOwner =
    crate::runtime::lease::LeaseOwner::new("storage", "write_back");
const WRITE_BACK_LIFECYCLE_OWNER: &str = "write_back";

#[cfg(test)]
const BUFFERED_RUNTIME_WRITE_BACK_LABELS: &[&str] = &[
    "execution_state_write_back",
    "self_model_write_back",
    "self_authored_core_write_back",
    "core_revision_ledger_write_back",
    "relationship_constitution_write_back",
    "world_sense_write_back",
    "outer_voice_write_back",
    "autonomy_strategy_write_back",
    "inner_life_write_back",
    "self_continuity_write_back",
    "felt_significance_write_back",
    "temperament_continuity_write_back",
    "inner_conflict_write_back",
    "mental_privacy_write_back",
    "relationship_portfolio_write_back",
    "relationship_topology_write_back",
    "long_term_extraction_state_write_back",
    "active_work_write_back",
    "turn_ledger_write_back",
    "session_summary_write_back",
    "important_message_write_back",
    "session_store",
];

static WRITE_BACK_DEFERRED_TOTAL: AtomicU32 = AtomicU32::new(0);
static WRITE_BACK_DROPPED_TOTAL: AtomicU32 = AtomicU32::new(0);
static WRITE_BACK_COALESCED_TOTAL: AtomicU32 = AtomicU32::new(0);
static WRITE_BACK_WORKER_STARTS_TOTAL: AtomicU32 = AtomicU32::new(0);
static WRITE_BACK_RETRY_SCHEDULED: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
static WRITE_BACK_TEST_AUTO_SERVICE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq, Eq)]
pub struct WriteBackSnapshot {
    pub queued: usize,
    pub worker_started: bool,
    pub deferred_total: u64,
    pub dropped_total: u64,
    pub coalesced_total: u64,
    pub worker_starts_total: u64,
}

fn write_back_scheduler() -> &'static WriteBackScheduler {
    static SCHEDULER: OnceLock<WriteBackScheduler> = OnceLock::new();
    SCHEDULER.get_or_init(|| WriteBackScheduler {
        state: Mutex::new(WriteBackQueueState {
            jobs: Vec::new(),
            worker_started: false,
        }),
    })
}

fn should_auto_service_write_back_tasks() -> bool {
    #[cfg(test)]
    {
        WRITE_BACK_TEST_AUTO_SERVICE.load(Ordering::Acquire)
    }
    #[cfg(not(test))]
    {
        true
    }
}

fn is_coalescible_write_back_label(label: &str) -> bool {
    matches!(
        label,
        "execution_state_write_back"
            | "self_model_write_back"
            | "self_authored_core_write_back"
            | "relationship_constitution_write_back"
            | "world_sense_write_back"
            | "outer_voice_write_back"
            | "autonomy_strategy_write_back"
            | "inner_life_write_back"
            | "self_continuity_write_back"
            | "felt_significance_write_back"
            | "temperament_continuity_write_back"
            | "inner_conflict_write_back"
            | "mental_privacy_write_back"
            | "relationship_portfolio_write_back"
            | "relationship_topology_write_back"
            | "long_term_extraction_state_write_back"
            | "active_work_write_back"
            | "session_summary_write_back"
            | "session_store"
    )
}

fn record_write_back_deferred(count: usize) {
    WRITE_BACK_DEFERRED_TOTAL.fetch_add(count.min(u32::MAX as usize) as u32, Ordering::Relaxed);
}

fn pending_write_back_jobs() -> bool {
    !write_back_scheduler()
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .jobs
        .is_empty()
}

fn schedule_write_back_retry(delay: Duration) {
    if WRITE_BACK_RETRY_SCHEDULED.swap(true, Ordering::AcqRel) {
        return;
    }
    let due_at = Instant::now() + delay;
    let task = Box::new(|| {
        WRITE_BACK_RETRY_SCHEDULED.store(false, Ordering::Release);
        service_write_back_tasks();
    });
    if crate::runtime::schedule_critical_delayed_task(due_at, task).is_err() {
        WRITE_BACK_RETRY_SCHEDULED.store(false, Ordering::Release);
        log::warn!("[write_back] failed to schedule admission retry");
    }
}

fn write_back_retry_scheduled() -> bool {
    WRITE_BACK_RETRY_SCHEDULED.load(Ordering::Acquire)
}

fn mark_write_back_lifecycle(state: crate::runtime::PlaneLifecycleState, reason: &'static str) {
    crate::runtime::plane_lifecycle::mark(
        crate::runtime::PlaneId::StorageWriteBack,
        WRITE_BACK_LIFECYCLE_OWNER,
        state,
        reason,
    );
}

fn schedule_write_back_task(label: &'static str, due_at: Instant, task: WriteBackTask) -> bool {
    let scheduler = write_back_scheduler();
    {
        let mut state = scheduler.state.lock().unwrap_or_else(|e| e.into_inner());
        if is_coalescible_write_back_label(label) {
            if let Some(existing) = state.jobs.iter_mut().find(|job| job.label == label) {
                if due_at < existing.due_at {
                    existing.due_at = due_at;
                }
                WRITE_BACK_COALESCED_TOTAL.fetch_add(1, Ordering::Relaxed);
                inbound_backpressure::record_cancelled(EventIngressSource::WriteBack);
                return true;
            }
        }
        if state.jobs.len() >= WRITE_BACK_QUEUE_MAX {
            WRITE_BACK_DROPPED_TOTAL.fetch_add(1, Ordering::Relaxed);
            inbound_backpressure::record_rejected(EventIngressSource::WriteBack);
            log::warn!(
                "[write_back:{}] write-back queue full, keeping pending writes queued",
                label
            );
            return false;
        }
        state.jobs.push(WriteBackJob {
            label,
            due_at,
            task: Some(task),
        });
        inbound_backpressure::record_enqueued(EventIngressSource::WriteBack);
    }
    if should_auto_service_write_back_tasks() {
        if write_back_retry_scheduled() {
            return true;
        }
        if let Some(delay) = write_back_admission_delay() {
            record_write_back_deferred(1);
            schedule_write_back_retry(delay);
            return true;
        }
        if ensure_write_back_worker_started_for_pending_jobs() {
            // The worker polls with a short sleep on ESP to avoid std timed-condvar
            // paths in lazy worker idle waits.
        }
    }
    true
}

fn ensure_write_back_worker_started_for_pending_jobs() -> bool {
    ensure_write_back_worker_started_inner(true)
}

fn ensure_write_back_worker_started_inner(require_pending_job: bool) -> bool {
    let scheduler = write_back_scheduler();
    {
        let mut state = scheduler.state.lock().unwrap_or_else(|e| e.into_inner());
        if require_pending_job && state.jobs.is_empty() {
            return false;
        }
        if require_pending_job && write_back_admission_delay().is_some() {
            return false;
        }
        if state.worker_started {
            return true;
        }
        state.worker_started = true;
    }
    mark_write_back_lifecycle(crate::runtime::PlaneLifecycleState::Starting, "spawn");
    match crate::util::spawn_guarded_with_profile_handle(
        "write_back",
        WRITE_BACK_WORKER_STACK,
        Some(crate::util::SpawnCore::Core1),
        crate::util::HttpThreadRole::Background,
        write_back_worker_loop,
    ) {
        Ok(_) => {
            WRITE_BACK_WORKER_STARTS_TOTAL.fetch_add(1, Ordering::Relaxed);
            true
        }
        Err(error) => {
            let mut state = scheduler.state.lock().unwrap_or_else(|e| e.into_inner());
            state.worker_started = false;
            mark_write_back_lifecycle(crate::runtime::PlaneLifecycleState::Failed, "spawn_failed");
            log::error!("[write_back] failed to start write-back worker: {}", error);
            false
        }
    }
}

fn take_due_write_back_jobs(state: &mut WriteBackQueueState, now: Instant) -> Vec<WriteBackJob> {
    let mut due = Vec::new();
    let mut index = 0usize;
    while index < state.jobs.len() {
        if state.jobs[index].due_at <= now {
            due.push(state.jobs.swap_remove(index));
        } else {
            index += 1;
        }
    }
    due.sort_by_key(|job| job.due_at);
    due
}

fn next_write_back_wait(jobs: &[WriteBackJob], now: Instant) -> Option<Duration> {
    jobs.iter()
        .map(|job| job.due_at.saturating_duration_since(now))
        .min()
}

fn defer_write_back_jobs_and_stop_worker(mut jobs: Vec<WriteBackJob>, delay: Duration) {
    let scheduler = write_back_scheduler();
    let due_at = Instant::now() + delay;
    record_write_back_deferred(jobs.len());
    let mut state = scheduler.state.lock().unwrap_or_else(|e| e.into_inner());
    for job in &mut jobs {
        job.due_at = due_at;
    }
    state.jobs.extend(jobs);
    state.worker_started = false;
    drop(state);
    if should_auto_service_write_back_tasks() {
        schedule_write_back_retry(delay);
    }
}

fn write_back_admission_delay() -> Option<Duration> {
    let resource = crate::orchestrator::snapshot();
    write_back_admission_delay_for_resource(&resource, crate::runtime::config_activity_active())
}

fn write_back_admission_delay_for_resource(
    resource: &crate::orchestrator::ResourceSnapshot,
    config_active: bool,
) -> Option<Duration> {
    let largest_block_low = resource.heap_largest_block_internal > 0
        && resource.heap_largest_block_internal
            < crate::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32;
    if resource.pressure == crate::orchestrator::PressureLevel::Critical
        || largest_block_low
        || resource.storage_contention_risk == crate::orchestrator::StorageContentionRisk::Critical
    {
        return Some(Duration::from_millis(WRITE_BACK_ADMISSION_DEFER_MS));
    }
    if resource.storage_contention_risk == crate::orchestrator::StorageContentionRisk::Cautious
        && write_back_storage_risk_foreground_activity_active(resource, config_active)
    {
        return Some(Duration::from_millis(WRITE_BACK_ADMISSION_DEFER_MS));
    }
    if write_back_interactive_foreground_activity_active(resource) {
        return Some(Duration::from_millis(
            WRITE_BACK_FOREGROUND_ADMISSION_DEFER_MS,
        ));
    }
    None
}

fn write_back_storage_risk_foreground_activity_active(
    resource: &crate::orchestrator::ResourceSnapshot,
    config_active: bool,
) -> bool {
    // Established WSS sessions are steady-state channel capacity, not short foreground work.
    config_active
        || resource.active_http_count > 0
        || resource.active_agent_tasks > 0
        || resource.inbound_depth > 0
        || resource.outbound_depth > 0
}

fn write_back_interactive_foreground_activity_active(
    resource: &crate::orchestrator::ResourceSnapshot,
) -> bool {
    resource.active_http_count > 0 || resource.active_agent_tasks > 0
}

struct WriteBackLeaseGuard {
    token: u64,
}

impl Drop for WriteBackLeaseGuard {
    fn drop(&mut self) {
        let _ = crate::runtime::lease::release_token(
            crate::runtime::lease::LeaseKind::StorageSessionWrite,
            WRITE_BACK_LEASE_OWNER,
            self.token,
        );
    }
}

fn try_acquire_write_back_lease() -> Option<WriteBackLeaseGuard> {
    match crate::runtime::lease::try_acquire(
        crate::runtime::lease::LeaseKind::StorageSessionWrite,
        WRITE_BACK_LEASE_OWNER,
        crate::runtime::lease::LeaseMode::Exclusive,
        None,
    ) {
        crate::runtime::lease::LeaseDecision::Acquired(record)
        | crate::runtime::lease::LeaseDecision::Reentered(record)
        | crate::runtime::lease::LeaseDecision::ReplacedExpired {
            current: record, ..
        } => Some(WriteBackLeaseGuard {
            token: record.token,
        }),
        crate::runtime::lease::LeaseDecision::Denied(denial) => {
            log::warn!(
                "[write_back] storage write lease denied reason={} held_by={:?}",
                denial.reason,
                denial.held_by
            );
            None
        }
    }
}

fn write_back_worker_loop() {
    let mut idle_started = Instant::now();
    loop {
        let due = match next_write_back_worker_step(&mut idle_started) {
            WriteBackWorkerStep::Run(due) => due,
            WriteBackWorkerStep::Sleep(wait) => {
                std::thread::sleep(wait);
                continue;
            }
            WriteBackWorkerStep::Stop => {
                mark_write_back_lifecycle(
                    crate::runtime::PlaneLifecycleState::Draining,
                    "idle_timeout",
                );
                mark_write_back_lifecycle(
                    crate::runtime::PlaneLifecycleState::Unloaded,
                    "idle_stop",
                );
                return;
            }
        };

        if let Some(delay) = write_back_admission_delay() {
            mark_write_back_lifecycle(
                crate::runtime::PlaneLifecycleState::Draining,
                "pressure_defer",
            );
            defer_write_back_jobs_and_stop_worker(due, delay);
            mark_write_back_lifecycle(
                crate::runtime::PlaneLifecycleState::Unloaded,
                "pressure_defer",
            );
            return;
        }

        let Some(_lease) = try_acquire_write_back_lease() else {
            mark_write_back_lifecycle(
                crate::runtime::PlaneLifecycleState::Draining,
                "lease_denied_defer",
            );
            defer_write_back_jobs_and_stop_worker(
                due,
                Duration::from_millis(WRITE_BACK_ADMISSION_DEFER_MS),
            );
            mark_write_back_lifecycle(
                crate::runtime::PlaneLifecycleState::Unloaded,
                "lease_denied_defer",
            );
            return;
        };

        mark_write_back_lifecycle(crate::runtime::PlaneLifecycleState::Active, "run_due_jobs");
        for mut job in due {
            if let Some(task) = job.task.take() {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
                if result.is_err() {
                    log::error!("[write_back] write-back task panicked");
                }
            }
        }
        mark_write_back_lifecycle(crate::runtime::PlaneLifecycleState::Starting, "idle_poll");
    }
}

enum WriteBackWorkerStep {
    Run(Vec<WriteBackJob>),
    Sleep(Duration),
    Stop,
}

fn next_write_back_worker_step(idle_started: &mut Instant) -> WriteBackWorkerStep {
    let scheduler = write_back_scheduler();
    let mut state = scheduler.state.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let due = take_due_write_back_jobs(&mut state, now);
    if !due.is_empty() {
        *idle_started = now;
        return WriteBackWorkerStep::Run(due);
    }

    if let Some(wait) = next_write_back_wait(&state.jobs, now) {
        *idle_started = now;
        return WriteBackWorkerStep::Sleep(wait.min(Duration::from_millis(WRITE_BACK_POLL_MS)));
    }

    let idle_for = now.saturating_duration_since(*idle_started);
    if idle_for >= Duration::from_millis(WRITE_BACK_IDLE_STOP_MS) {
        state.worker_started = false;
        return WriteBackWorkerStep::Stop;
    }

    let remaining = Duration::from_millis(WRITE_BACK_IDLE_STOP_MS).saturating_sub(idle_for);
    WriteBackWorkerStep::Sleep(remaining.min(Duration::from_millis(WRITE_BACK_POLL_MS)))
}

/// Wake the background write-back execution plane.
///
/// This function is intentionally light enough for `agent_loop`: heavy
/// SPIFFS/serde/session flush closures run only on the write-back worker stack.
pub fn service_write_back_tasks() {
    if write_back_retry_scheduled() {
        return;
    }
    if let Some(delay) = write_back_admission_delay() {
        if pending_write_back_jobs() {
            record_write_back_deferred(1);
            if should_auto_service_write_back_tasks() {
                schedule_write_back_retry(delay);
            }
        }
        return;
    }
    let _ = ensure_write_back_worker_started_for_pending_jobs();
}

pub fn snapshot() -> WriteBackSnapshot {
    let scheduler = write_back_scheduler();
    let state = scheduler.state.lock().unwrap_or_else(|e| e.into_inner());
    WriteBackSnapshot {
        queued: state.jobs.len(),
        worker_started: state.worker_started,
        deferred_total: WRITE_BACK_DEFERRED_TOTAL.load(Ordering::Relaxed) as u64,
        dropped_total: WRITE_BACK_DROPPED_TOTAL.load(Ordering::Relaxed) as u64,
        coalesced_total: WRITE_BACK_COALESCED_TOTAL.load(Ordering::Relaxed) as u64,
        worker_starts_total: WRITE_BACK_WORKER_STARTS_TOTAL.load(Ordering::Relaxed) as u64,
    }
}

pub fn format_baseline_log_line() -> String {
    let snap = snapshot();
    format!(
        "write_back queued={} worker_started={} deferred_total={} dropped_total={} coalesced_total={} worker_starts_total={}",
        snap.queued,
        snap.worker_started,
        snap.deferred_total,
        snap.dropped_total,
        snap.coalesced_total,
        snap.worker_starts_total
    )
}

#[cfg(test)]
fn reset_write_back_queue_for_tests() {
    WRITE_BACK_RETRY_SCHEDULED.store(false, Ordering::Release);
    let scheduler = write_back_scheduler();
    {
        let mut state = scheduler.state.lock().unwrap_or_else(|e| e.into_inner());
        state.jobs.clear();
    }
    let deadline = Instant::now() + Duration::from_millis(1_000);
    while Instant::now() < deadline {
        let worker_started = scheduler
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .worker_started;
        if !worker_started {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
fn write_back_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone)]
enum PendingValue<V> {
    Set(V),
    Clear,
}

struct PendingMapBuffer<V> {
    label: &'static str,
    pending: Mutex<HashMap<String, PendingValue<V>>>,
    flush_scheduled: AtomicBool,
}

impl<V> PendingMapBuffer<V> {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            pending: Mutex::new(HashMap::new()),
            flush_scheduled: AtomicBool::new(false),
        }
    }

    fn store_set(&self, chat_id: &str, value: V) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(chat_id.to_string(), PendingValue::Set(value));
    }

    fn store_clear(&self, chat_id: &str) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(chat_id.to_string(), PendingValue::Clear);
    }

    fn peek(&self, chat_id: &str) -> Option<Option<V>>
    where
        V: Clone,
    {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(chat_id)
            .cloned()
            .map(|value| match value {
                PendingValue::Set(value) => Some(value),
                PendingValue::Clear => None,
            })
    }

    fn take_all(&self) -> HashMap<String, PendingValue<V>> {
        std::mem::take(&mut *self.pending.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn restore_missing(&self, drained: HashMap<String, PendingValue<V>>) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        for (chat_id, value) in drained {
            pending.entry(chat_id).or_insert(value);
        }
    }

    fn has_pending(&self) -> bool {
        !self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }

    fn try_mark_scheduled(&self) -> bool {
        !self.flush_scheduled.swap(true, Ordering::AcqRel)
    }

    fn clear_scheduled(&self) {
        self.flush_scheduled.store(false, Ordering::Release);
    }
}

fn schedule_map_flush<T, V>(
    inner: Arc<T>,
    pending: Arc<PendingMapBuffer<V>>,
    apply: fn(&T, &str, PendingValue<V>) -> Result<()>,
) where
    T: ?Sized + Send + Sync + 'static,
    V: Clone + Send + 'static,
{
    if !pending.try_mark_scheduled() {
        return;
    }
    let due_at = Instant::now() + Duration::from_millis(WRITE_BACK_DELAY_MS);
    let delayed_inner = Arc::clone(&inner);
    let delayed_pending = Arc::clone(&pending);
    let task = Box::new(move || flush_map(delayed_inner, delayed_pending, apply));
    if !schedule_write_back_task(pending.label, due_at, task) {
        pending.clear_scheduled();
    }
}

fn flush_map<T, V>(
    inner: Arc<T>,
    pending: Arc<PendingMapBuffer<V>>,
    apply: fn(&T, &str, PendingValue<V>) -> Result<()>,
) where
    T: ?Sized + Send + Sync + 'static,
    V: Clone + Send + 'static,
{
    let drained = pending.take_all();
    if drained.is_empty() {
        pending.clear_scheduled();
        return;
    }
    let mut failed = HashMap::new();
    for (chat_id, value) in drained {
        if let Err(error) = apply(inner.as_ref(), &chat_id, value.clone()) {
            log::warn!(
                "[write_back:{}] flush failed chat_id={}: {}",
                pending.label,
                chat_id,
                error
            );
            if should_retry_write_back_error(&error) {
                failed.insert(chat_id, value);
            } else {
                log::warn!(
                    "[write_back:{}] dropping non-retryable pending write chat_id={} stage={}",
                    pending.label,
                    chat_id,
                    error.stage()
                );
            }
        }
    }
    if !failed.is_empty() {
        pending.restore_missing(failed);
    }
    pending.clear_scheduled();
    if pending.has_pending() {
        schedule_map_flush(inner, pending, apply);
    }
}

fn should_retry_write_back_error(error: &Error) -> bool {
    match error {
        Error::Config { .. } => false,
        Error::Io { source, .. } => !matches!(source.raw_os_error(), Some(2 | 28 | 36 | 63 | 91)),
        _ => true,
    }
}

macro_rules! define_buffered_chat_store {
    ($name:ident, $trait:path, $value:ty, $label:literal) => {
        pub struct $name {
            inner: Arc<dyn $trait + Send + Sync>,
            pending: Arc<PendingMapBuffer<$value>>,
        }

        impl $name {
            pub fn wrap(inner: Arc<dyn $trait + Send + Sync>) -> Arc<dyn $trait + Send + Sync> {
                Arc::new(Self {
                    inner,
                    pending: Arc::new(PendingMapBuffer::new($label)),
                }) as Arc<dyn $trait + Send + Sync>
            }

            fn schedule_flush(&self) {
                schedule_map_flush(
                    Arc::clone(&self.inner),
                    Arc::clone(&self.pending),
                    Self::apply_pending,
                );
            }

            fn apply_pending(
                inner: &(dyn $trait + Send + Sync),
                chat_id: &str,
                value: PendingValue<$value>,
            ) -> Result<()> {
                match value {
                    PendingValue::Set(value) => inner.set(chat_id, &value),
                    PendingValue::Clear => inner.clear(chat_id),
                }
            }
        }

        impl $trait for $name {
            fn get(&self, chat_id: &str) -> Result<Option<$value>> {
                if let Some(value) = self.pending.peek(chat_id) {
                    return Ok(value);
                }
                self.inner.get(chat_id)
            }

            fn set(&self, chat_id: &str, value: &$value) -> Result<()> {
                self.pending.store_set(chat_id, value.clone());
                self.schedule_flush();
                Ok(())
            }

            fn clear(&self, chat_id: &str) -> Result<()> {
                self.pending.store_clear(chat_id);
                self.schedule_flush();
                Ok(())
            }
        }
    };
}

define_buffered_chat_store!(
    BufferedExecutionStateStore,
    ExecutionStateStore,
    ExecutionState,
    "execution_state_write_back"
);
define_buffered_chat_store!(
    BufferedSelfModelStore,
    SelfModelStore,
    SelfModel,
    "self_model_write_back"
);
define_buffered_chat_store!(
    BufferedSelfAuthoredCoreStore,
    SelfAuthoredCoreStore,
    SelfAuthoredCore,
    "self_authored_core_write_back"
);
define_buffered_chat_store!(
    BufferedCoreRevisionLedgerStore,
    CoreRevisionLedgerStore,
    CoreRevisionLedger,
    "core_revision_ledger_write_back"
);
define_buffered_chat_store!(
    BufferedRelationshipConstitutionStore,
    RelationshipConstitutionStore,
    RelationshipConstitution,
    "relationship_constitution_write_back"
);
define_buffered_chat_store!(
    BufferedWorldSenseStore,
    WorldSenseStore,
    WorldSense,
    "world_sense_write_back"
);
define_buffered_chat_store!(
    BufferedOuterVoiceStore,
    OuterVoiceStore,
    OuterVoice,
    "outer_voice_write_back"
);
define_buffered_chat_store!(
    BufferedAutonomyStrategyStore,
    AutonomyStrategyStore,
    AutonomyStrategy,
    "autonomy_strategy_write_back"
);
define_buffered_chat_store!(
    BufferedInnerLifeStore,
    InnerLifeStore,
    InnerLife,
    "inner_life_write_back"
);
define_buffered_chat_store!(
    BufferedSelfContinuityStore,
    SelfContinuityStore,
    SelfContinuity,
    "self_continuity_write_back"
);
define_buffered_chat_store!(
    BufferedFeltSignificanceStore,
    FeltSignificanceStore,
    FeltSignificance,
    "felt_significance_write_back"
);
define_buffered_chat_store!(
    BufferedTemperamentContinuityStore,
    TemperamentContinuityStore,
    TemperamentContinuity,
    "temperament_continuity_write_back"
);
define_buffered_chat_store!(
    BufferedInnerConflictStore,
    InnerConflictStore,
    InnerConflict,
    "inner_conflict_write_back"
);
define_buffered_chat_store!(
    BufferedMentalPrivacyStore,
    MentalPrivacyStore,
    MentalPrivacyState,
    "mental_privacy_write_back"
);
define_buffered_chat_store!(
    BufferedRelationshipPortfolioStore,
    RelationshipPortfolioStore,
    RelationshipPortfolio,
    "relationship_portfolio_write_back"
);
define_buffered_chat_store!(
    BufferedRelationshipTopologyStore,
    RelationshipTopologyStore,
    RelationshipTopology,
    "relationship_topology_write_back"
);
define_buffered_chat_store!(
    BufferedLongTermExtractionStateStore,
    LongTermMemoryExtractionStateStore,
    LongTermMemoryExtractionState,
    "long_term_extraction_state_write_back"
);
define_buffered_chat_store!(
    BufferedActiveWorkStore,
    ActiveWorkStore,
    ActiveWorkRecord,
    "active_work_write_back"
);

pub struct BufferedTurnLedgerStore {
    inner: Arc<dyn TurnLedgerStore + Send + Sync>,
    pending: Arc<PendingMapBuffer<TurnLedger>>,
    cache: Arc<Mutex<HashMap<String, Option<TurnLedger>>>>,
}

const TURN_LEDGER_CACHE_MAX_CHATS: usize = 64;

impl BufferedTurnLedgerStore {
    pub fn wrap(
        inner: Arc<dyn TurnLedgerStore + Send + Sync>,
    ) -> Arc<dyn TurnLedgerStore + Send + Sync> {
        Arc::new(Self {
            inner,
            pending: Arc::new(PendingMapBuffer::new("turn_ledger_write_back")),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }) as Arc<dyn TurnLedgerStore + Send + Sync>
    }

    fn schedule_flush(&self) {
        schedule_map_flush(
            Arc::clone(&self.inner),
            Arc::clone(&self.pending),
            Self::apply_pending,
        );
    }

    fn cache_value(&self, chat_id: &str, value: Option<TurnLedger>) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if !cache.contains_key(chat_id) && cache.len() >= TURN_LEDGER_CACHE_MAX_CHATS {
            cache.clear();
        }
        cache.insert(chat_id.to_string(), value);
    }

    fn apply_pending(
        inner: &(dyn TurnLedgerStore + Send + Sync),
        chat_id: &str,
        value: PendingValue<TurnLedger>,
    ) -> Result<()> {
        match value {
            PendingValue::Set(value) => inner.set(chat_id, &value),
            PendingValue::Clear => inner.clear(chat_id),
        }
    }
}

impl TurnLedgerStore for BufferedTurnLedgerStore {
    fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
        if let Some(value) = self.pending.peek(chat_id) {
            return Ok(value);
        }
        if let Some(value) = self
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(chat_id)
            .cloned()
        {
            return Ok(value);
        }
        let loaded = self.inner.get(chat_id)?;
        self.cache_value(chat_id, loaded.clone());
        Ok(loaded)
    }

    fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()> {
        self.pending.store_set(chat_id, ledger.clone());
        self.cache_value(chat_id, Some(ledger.clone()));
        self.schedule_flush();
        Ok(())
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.pending.store_clear(chat_id);
        self.cache_value(chat_id, None);
        self.schedule_flush();
        Ok(())
    }

    fn list_recent(&self, chat_id: &str, limit: usize) -> Result<Vec<TurnLedger>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut recent = self.inner.list_recent(chat_id, limit)?;
        if let Some(pending) = self.pending.peek(chat_id) {
            match pending {
                Some(ledger) if ledger.status.is_terminal() => {
                    recent.retain(|existing| {
                        let same_req_id =
                            !ledger.req_id.trim().is_empty() && existing.req_id == ledger.req_id;
                        let same_started_at = ledger.started_at_ms > 0
                            && existing.started_at_ms == ledger.started_at_ms;
                        !(same_req_id || same_started_at)
                    });
                    recent.insert(0, ledger);
                }
                None => return Ok(Vec::new()),
                _ => {}
            }
        }
        recent.truncate(limit);
        Ok(recent)
    }

    fn recent_persona_evidence(&self, chat_id: &str) -> Result<Option<RecentPersonaEvidence>> {
        let pending = self.pending.peek(chat_id);
        match pending {
            Some(None) => Ok(None),
            Some(Some(ledger)) if ledger.status.is_terminal() => {
                Ok(derive_recent_persona_evidence(
                    std::slice::from_ref(&ledger),
                    RECENT_PERSONA_EVIDENCE_MEANINGFUL_TURNS,
                ))
            }
            _ => self.inner.recent_persona_evidence(chat_id),
        }
    }
}

#[derive(Clone)]
struct PendingSummary {
    summary: String,
    message_count: usize,
}

pub struct BufferedSessionSummaryStore {
    inner: Arc<dyn SessionSummaryStore + Send + Sync>,
    pending: Arc<Mutex<HashMap<String, PendingSummary>>>,
    flush_scheduled: Arc<AtomicBool>,
}

impl BufferedSessionSummaryStore {
    pub fn wrap(
        inner: Arc<dyn SessionSummaryStore + Send + Sync>,
    ) -> Arc<dyn SessionSummaryStore + Send + Sync> {
        Arc::new(Self {
            inner,
            pending: Arc::new(Mutex::new(HashMap::new())),
            flush_scheduled: Arc::new(AtomicBool::new(false)),
        }) as Arc<dyn SessionSummaryStore + Send + Sync>
    }

    fn schedule_flush(&self) {
        if self.flush_scheduled.swap(true, Ordering::AcqRel) {
            return;
        }
        let due_at = Instant::now() + Duration::from_millis(WRITE_BACK_DELAY_MS);
        let inner = Arc::clone(&self.inner);
        let pending = Arc::clone(&self.pending);
        let flush_scheduled = Arc::clone(&self.flush_scheduled);
        let task = Box::new(move || flush_session_summary_store(inner, pending, flush_scheduled));
        if !schedule_write_back_task("session_summary_write_back", due_at, task) {
            self.flush_scheduled.store(false, Ordering::Release);
        }
    }
}

impl SessionSummaryStore for BufferedSessionSummaryStore {
    fn get(&self, chat_id: &str) -> Result<Option<String>> {
        if let Some(value) = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(chat_id)
            .cloned()
        {
            return Ok(Some(value.summary));
        }
        self.inner.get(chat_id)
    }

    fn set(&self, chat_id: &str, summary: &str) -> Result<()> {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                chat_id.to_string(),
                PendingSummary {
                    summary: summary.to_string(),
                    message_count: 0,
                },
            );
        self.schedule_flush();
        Ok(())
    }

    fn set_with_count(&self, chat_id: &str, summary: &str, message_count: usize) -> Result<()> {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                chat_id.to_string(),
                PendingSummary {
                    summary: summary.to_string(),
                    message_count,
                },
            );
        self.schedule_flush();
        Ok(())
    }

    fn get_with_count(&self, chat_id: &str) -> Result<Option<(String, usize)>> {
        if let Some(value) = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(chat_id)
            .cloned()
        {
            return Ok(Some((value.summary, value.message_count)));
        }
        self.inner.get_with_count(chat_id)
    }
}

fn flush_session_summary_store(
    inner: Arc<dyn SessionSummaryStore + Send + Sync>,
    pending: Arc<Mutex<HashMap<String, PendingSummary>>>,
    flush_scheduled: Arc<AtomicBool>,
) {
    let drained = std::mem::take(&mut *pending.lock().unwrap_or_else(|e| e.into_inner()));
    if drained.is_empty() {
        flush_scheduled.store(false, Ordering::Release);
        return;
    }
    let mut failed = HashMap::new();
    for (chat_id, value) in drained {
        if let Err(error) = inner.set_with_count(&chat_id, &value.summary, value.message_count) {
            log::warn!(
                "[write_back:session_summary_write_back] flush failed chat_id={}: {}",
                chat_id,
                error
            );
            if should_retry_write_back_error(&error) {
                failed.insert(chat_id, value);
            } else {
                log::warn!(
                    "[write_back:session_summary_write_back] dropping non-retryable pending write chat_id={} stage={}",
                    chat_id,
                    error.stage()
                );
            }
        }
    }
    if !failed.is_empty() {
        let mut guard = pending.lock().unwrap_or_else(|e| e.into_inner());
        for (chat_id, value) in failed {
            guard.entry(chat_id).or_insert(value);
        }
    }
    flush_scheduled.store(false, Ordering::Release);
    let has_pending = !pending.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
    if has_pending {
        let due_at = Instant::now() + Duration::from_millis(WRITE_BACK_DELAY_MS);
        if !flush_scheduled.swap(true, Ordering::AcqRel) {
            let next_inner = Arc::clone(&inner);
            let next_pending = Arc::clone(&pending);
            let next_flag = Arc::clone(&flush_scheduled);
            let task =
                Box::new(move || flush_session_summary_store(next_inner, next_pending, next_flag));
            if !schedule_write_back_task("session_summary_write_back", due_at, task) {
                flush_scheduled.store(false, Ordering::Release);
            }
        }
    }
}

pub struct BufferedImportantMessageStore {
    inner: Arc<dyn ImportantMessageStore + Send + Sync>,
    pending: Arc<PendingMapBuffer<u32>>,
}

impl BufferedImportantMessageStore {
    pub fn wrap(
        inner: Arc<dyn ImportantMessageStore + Send + Sync>,
    ) -> Arc<dyn ImportantMessageStore + Send + Sync> {
        Arc::new(Self {
            inner,
            pending: Arc::new(PendingMapBuffer::new("important_message_write_back")),
        }) as Arc<dyn ImportantMessageStore + Send + Sync>
    }

    fn schedule_flush(&self) {
        schedule_map_flush(
            Arc::clone(&self.inner),
            Arc::clone(&self.pending),
            Self::apply_pending,
        );
    }

    fn apply_pending(
        inner: &(dyn ImportantMessageStore + Send + Sync),
        chat_id: &str,
        value: PendingValue<u32>,
    ) -> Result<()> {
        match value {
            PendingValue::Set(value) => inner.set_important_offset_from_end(chat_id, value),
            PendingValue::Clear => inner.clear_important(chat_id),
        }
    }
}

impl ImportantMessageStore for BufferedImportantMessageStore {
    fn set_important_offset_from_end(&self, chat_id: &str, offset_from_end: u32) -> Result<()> {
        self.pending.store_set(chat_id, offset_from_end);
        self.schedule_flush();
        Ok(())
    }

    fn get_important_offset(&self, chat_id: &str) -> Result<Option<u32>> {
        if let Some(value) = self.pending.peek(chat_id) {
            return Ok(value);
        }
        self.inner.get_important_offset(chat_id)
    }

    fn clear_important(&self, chat_id: &str) -> Result<()> {
        self.pending.store_clear(chat_id);
        self.schedule_flush();
        Ok(())
    }
}

#[derive(Clone, Default)]
struct PendingSessionWrite {
    clear: bool,
    appended: Vec<SessionMessage>,
}

struct PendingSessionBuffer {
    pending: Mutex<HashMap<String, PendingSessionWrite>>,
    flush_scheduled: AtomicBool,
}

impl PendingSessionBuffer {
    fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            flush_scheduled: AtomicBool::new(false),
        }
    }

    fn stage_append(&self, chat_id: &str, messages: &[SessionMessage]) {
        if messages.is_empty() {
            return;
        }
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let entry = pending.entry(chat_id.to_string()).or_default();
        entry.appended.extend(messages.iter().cloned());
    }

    fn stage_clear(&self, chat_id: &str) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                chat_id.to_string(),
                PendingSessionWrite {
                    clear: true,
                    appended: Vec::new(),
                },
            );
    }

    fn peek(&self, chat_id: &str) -> Option<PendingSessionWrite> {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(chat_id)
            .cloned()
    }

    fn take_all(&self) -> HashMap<String, PendingSessionWrite> {
        std::mem::take(&mut *self.pending.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn restore_missing(&self, drained: HashMap<String, PendingSessionWrite>) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        for (chat_id, write) in drained {
            match pending.entry(chat_id) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(write);
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let current = entry.get_mut();
                    if current.clear {
                        // A clear staged after the failed write supersedes older appends.
                        continue;
                    }
                    let mut appended = write.appended;
                    appended.append(&mut current.appended);
                    current.clear = write.clear;
                    current.appended = appended;
                }
            }
        }
    }

    fn has_pending(&self) -> bool {
        !self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }

    fn try_mark_scheduled(&self) -> bool {
        !self.flush_scheduled.swap(true, Ordering::AcqRel)
    }

    fn clear_scheduled(&self) {
        self.flush_scheduled.store(false, Ordering::Release);
    }
}

pub struct BufferedSessionStore {
    inner: Arc<dyn SessionStore + Send + Sync>,
    pending: Arc<PendingSessionBuffer>,
}

impl BufferedSessionStore {
    pub fn wrap(inner: Arc<dyn SessionStore + Send + Sync>) -> Arc<dyn SessionStore + Send + Sync> {
        Arc::new(Self {
            inner,
            pending: Arc::new(PendingSessionBuffer::new()),
        }) as Arc<dyn SessionStore + Send + Sync>
    }

    fn schedule_flush(&self) {
        if !self.pending.try_mark_scheduled() {
            return;
        }
        let due_at = Instant::now() + Duration::from_millis(WRITE_BACK_DELAY_MS);
        let inner = Arc::clone(&self.inner);
        let pending = Arc::clone(&self.pending);
        let task = Box::new(move || flush_session_store(inner, pending));
        if !schedule_write_back_task("session_store", due_at, task) {
            self.pending.clear_scheduled();
        }
    }
}

fn flush_session_store(
    inner: Arc<dyn SessionStore + Send + Sync>,
    pending: Arc<PendingSessionBuffer>,
) {
    let drained = pending.take_all();
    if drained.is_empty() {
        pending.clear_scheduled();
        return;
    }
    let mut failed = HashMap::new();
    for (chat_id, write) in drained {
        let result = (|| -> Result<()> {
            if write.clear {
                inner.clear(&chat_id)?;
            }
            if !write.appended.is_empty() {
                inner.append_batch(&chat_id, &write.appended)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            log::warn!(
                "[write_back:session_store] flush failed chat_id={}: {}",
                chat_id,
                error
            );
            if should_retry_write_back_error(&error) {
                failed.insert(chat_id, write);
            } else {
                log::warn!(
                    "[write_back:session_store] dropping non-retryable pending write chat_id={} stage={}",
                    chat_id,
                    error.stage()
                );
            }
        }
    }
    if !failed.is_empty() {
        pending.restore_missing(failed);
    }
    pending.clear_scheduled();
    if pending.has_pending() {
        let wrapper = BufferedSessionStore {
            inner,
            pending: Arc::clone(&pending),
        };
        wrapper.schedule_flush();
    }
}

impl SessionStore for BufferedSessionStore {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
        self.append_batch(
            chat_id,
            &[SessionMessage {
                role: role.to_string(),
                content: content.to_string(),
            }],
        )
    }

    fn append_batch(&self, chat_id: &str, messages: &[SessionMessage]) -> Result<()> {
        self.pending.stage_append(chat_id, messages);
        self.schedule_flush();
        Ok(())
    }

    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
        let Some(write) = self.pending.peek(chat_id) else {
            return self.inner.load_recent(chat_id, n);
        };
        let mut combined = if write.clear {
            Vec::new()
        } else {
            self.inner
                .load_recent(chat_id, n.min(crate::memory::MAX_SESSION_ENTRIES))?
        };
        combined.extend(write.appended);
        if combined.len() > crate::memory::MAX_SESSION_ENTRIES {
            let start = combined.len() - crate::memory::MAX_SESSION_ENTRIES;
            combined = combined.split_off(start);
        }
        let start = combined
            .len()
            .saturating_sub(n.min(crate::memory::MAX_SESSION_ENTRIES));
        Ok(combined.into_iter().skip(start).collect())
    }

    fn message_count(&self, chat_id: &str) -> Result<usize> {
        let Some(write) = self.pending.peek(chat_id) else {
            return self.inner.message_count(chat_id);
        };
        let base = if write.clear {
            0
        } else {
            self.inner.message_count(chat_id)?
        };
        Ok(base
            .saturating_add(write.appended.len())
            .min(crate::memory::MAX_SESSION_ENTRIES))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.pending.stage_clear(chat_id);
        self.schedule_flush();
        Ok(())
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        let mut ids: HashSet<String> = self.inner.list_chat_ids()?.into_iter().collect();
        let pending = self
            .pending
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for (chat_id, write) in pending.iter() {
            if write.clear && write.appended.is_empty() {
                ids.remove(chat_id);
            } else {
                ids.insert(chat_id.clone());
            }
        }
        let mut out: Vec<String> = ids.into_iter().collect();
        out.sort();
        Ok(out)
    }

    fn gc_stale(&self, max_age_secs: u64) -> Result<usize> {
        flush_session_store(Arc::clone(&self.inner), Arc::clone(&self.pending));
        self.inner.gc_stale(max_age_secs)
    }

    fn delete(&self, chat_id: &str) -> Result<()> {
        self.clear(chat_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{ExecutionStatus, SessionStore, TurnLedgerStatus};
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn write_back_worker_stack_budget_covers_storage_flush_path() {
        const {
            assert!(
                WRITE_BACK_WORKER_STACK >= 24 * 1024,
                "write_back runs serde/session flushes and must not use a pure-scheduler stack"
            );
        }
    }

    #[derive(Default)]
    struct StubExecutionStateStore {
        values: Mutex<HashMap<String, ExecutionState>>,
    }

    #[derive(Default)]
    struct StubActiveWorkStore {
        values: Mutex<HashMap<String, ActiveWorkRecord>>,
    }

    impl ExecutionStateStore for StubExecutionStateStore {
        fn get(&self, chat_id: &str) -> Result<Option<ExecutionState>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, state: &ExecutionState) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), state.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    impl ActiveWorkStore for StubActiveWorkStore {
        fn get(&self, chat_id: &str) -> Result<Option<ActiveWorkRecord>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, record: &ActiveWorkRecord) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), record.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSessionStore {
        entries: Mutex<HashMap<String, Vec<SessionMessage>>>,
    }

    #[derive(Default)]
    struct CountingTurnLedgerStore {
        list_recent_calls: AtomicUsize,
        set_calls: AtomicUsize,
    }

    impl TurnLedgerStore for CountingTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            self.set_calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_recent(&self, _chat_id: &str, _limit: usize) -> Result<Vec<TurnLedger>> {
            self.list_recent_calls.fetch_add(1, Ordering::Relaxed);
            Ok(Vec::new())
        }
    }

    fn meaningful_turn_ledger() -> TurnLedger {
        TurnLedger {
            ingress: crate::bus::IngressKind::User,
            status: TurnLedgerStatus::Answered,
            started_at_ms: 1_000,
            updated_at_ms: 2_000,
            finished_at_ms: 2_000,
            final_reply_delivered: true,
            canonical_reply_source: "final_answer".to_string(),
            persona: Some(crate::memory::TurnPersonaLedger {
                reply_scope: "brief".to_string(),
                reply_delivered: true,
                ..crate::memory::TurnPersonaLedger::default()
            }),
            ..TurnLedger::default()
        }
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(chat_id.to_string())
                .or_default()
                .push(SessionMessage {
                    role: role.to_string(),
                    content: content.to_string(),
                });
            Ok(())
        }

        fn append_batch(&self, chat_id: &str, messages: &[SessionMessage]) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(chat_id.to_string())
                .or_default()
                .extend(messages.iter().cloned());
            Ok(())
        }

        fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
            let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let mut values = entries.get(chat_id).cloned().unwrap_or_default();
            let start = values.len().saturating_sub(n);
            Ok(values.split_off(start))
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect())
        }
    }

    fn queued_write_back_labels_for_tests() -> Vec<&'static str> {
        let scheduler = write_back_scheduler();
        let mut labels = scheduler
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .jobs
            .iter()
            .map(|job| job.label)
            .collect::<Vec<_>>();
        labels.sort_unstable();
        labels
    }

    #[test]
    fn buffered_execution_state_reads_pending_before_flush() {
        let _write_back_guard = write_back_test_guard();
        reset_write_back_queue_for_tests();
        let (_state_guard, _delayed_guard) =
            crate::runtime::delayed_task::delayed_task_test_scope();
        let inner: Arc<dyn ExecutionStateStore + Send + Sync> =
            Arc::new(StubExecutionStateStore::default());
        let store = BufferedExecutionStateStore::wrap(inner);
        let state = ExecutionState {
            status: ExecutionStatus::Active,
            goal: "goal".to_string(),
            progress: String::new(),
            blocker: String::new(),
            next_action: "next".to_string(),
            last_output: String::new(),
            updated_at: 0,
            ..ExecutionState::default()
        };

        store.set("chat", &state).unwrap();
        let got = store.get("chat").unwrap().unwrap();
        assert_eq!(got.goal, "goal");
    }

    #[test]
    fn buffered_active_work_reads_pending_before_flush() {
        let _write_back_guard = write_back_test_guard();
        reset_write_back_queue_for_tests();
        let (_state_guard, _delayed_guard) =
            crate::runtime::delayed_task::delayed_task_test_scope();
        let inner: Arc<dyn ActiveWorkStore + Send + Sync> =
            Arc::new(StubActiveWorkStore::default());
        let store = BufferedActiveWorkStore::wrap(inner);
        let record = ActiveWorkRecord {
            kind: crate::agent::ActiveWorkKind::InteractiveAction,
            title: "blocked user turn".to_string(),
            status: crate::agent::ForegroundWorkStatus::AwaitingUser,
            continuity_open: true,
            blocks_background_llm: true,
            progress_summary: "waiting".to_string(),
            blocker: String::new(),
            next_action: String::new(),
            recent_outcome: String::new(),
            active_artifact_refs: Vec::new(),
            updated_at: 1,
        };

        store.set("chat", &record).unwrap();
        let got = store.get("chat").unwrap().unwrap();
        assert_eq!(got.title, "blocked user turn");
        assert!(queued_write_back_labels_for_tests().contains(&"active_work_write_back"));
    }

    #[test]
    fn buffered_session_store_merges_pending_messages() {
        let _write_back_guard = write_back_test_guard();
        reset_write_back_queue_for_tests();
        let (_state_guard, _delayed_guard) =
            crate::runtime::delayed_task::delayed_task_test_scope();
        let inner: Arc<dyn SessionStore + Send + Sync> = Arc::new(StubSessionStore::default());
        let store = BufferedSessionStore::wrap(inner);

        store.append("chat", "user", "hi").unwrap();
        store.append("chat", "assistant", "hello").unwrap();

        let recent = store.load_recent("chat", 8).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].content, "hi");
        assert_eq!(recent[1].content, "hello");
    }

    #[test]
    fn session_restore_prepends_failed_append_before_concurrent_append() {
        let pending = PendingSessionBuffer::new();
        pending.stage_append(
            "chat",
            &[SessionMessage {
                role: "assistant".to_string(),
                content: "new".to_string(),
            }],
        );
        let mut drained = HashMap::new();
        drained.insert(
            "chat".to_string(),
            PendingSessionWrite {
                clear: false,
                appended: vec![SessionMessage {
                    role: "user".to_string(),
                    content: "old".to_string(),
                }],
            },
        );

        pending.restore_missing(drained);

        let restored = pending.peek("chat").expect("pending write");
        assert!(!restored.clear);
        let contents = restored
            .appended
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(contents, vec!["old", "new"]);
    }

    #[test]
    fn session_restore_preserves_newer_clear_over_failed_append() {
        let pending = PendingSessionBuffer::new();
        pending.stage_clear("chat");
        let mut drained = HashMap::new();
        drained.insert(
            "chat".to_string(),
            PendingSessionWrite {
                clear: false,
                appended: vec![SessionMessage {
                    role: "user".to_string(),
                    content: "old".to_string(),
                }],
            },
        );

        pending.restore_missing(drained);

        let restored = pending.peek("chat").expect("pending write");
        assert!(restored.clear);
        assert!(restored.appended.is_empty());
    }

    #[test]
    fn buffered_turn_ledger_uses_pending_terminal_for_recent_persona_evidence() {
        let _write_back_guard = write_back_test_guard();
        reset_write_back_queue_for_tests();
        let (_state_guard, _delayed_guard) =
            crate::runtime::delayed_task::delayed_task_test_scope();
        let inner = Arc::new(CountingTurnLedgerStore::default());
        let counter = Arc::clone(&inner);
        let store = BufferedTurnLedgerStore::wrap(inner as Arc<dyn TurnLedgerStore + Send + Sync>);

        store.set("chat", &meaningful_turn_ledger()).unwrap();
        let evidence = store.recent_persona_evidence("chat").unwrap();

        assert!(evidence.is_some());
        assert_eq!(evidence.unwrap().meaningful_turns, 1);
        assert_eq!(
            counter.list_recent_calls.load(Ordering::Relaxed),
            0,
            "pending terminal evidence must not scan persisted history"
        );
    }

    #[test]
    fn buffered_turn_ledger_keeps_pending_when_delayed_queue_is_full() {
        let _write_back_guard = write_back_test_guard();
        reset_write_back_queue_for_tests();
        let (_state_guard, _delayed_guard) =
            crate::runtime::delayed_task::delayed_task_test_scope();
        let due_at = Instant::now() + Duration::from_secs(60);
        let before = snapshot();
        let metrics_before = crate::metrics::snapshot();
        let mut accepted = 0usize;
        while schedule_write_back_task("test", due_at, Box::new(|| {})) {
            accepted += 1;
            assert!(accepted < 256, "write-back queue cap should be finite");
        }
        assert!(accepted > 0);
        let after_full = snapshot();
        let metrics_after_full = crate::metrics::snapshot();
        assert_eq!(after_full.dropped_total, before.dropped_total + 1);
        assert!(
            metrics_after_full.event_ingress_rejected_total
                > metrics_before.event_ingress_rejected_total
        );

        let inner = Arc::new(CountingTurnLedgerStore::default());
        let counter = Arc::clone(&inner);
        let store = BufferedTurnLedgerStore::wrap(inner as Arc<dyn TurnLedgerStore + Send + Sync>);

        store.set("chat", &meaningful_turn_ledger()).unwrap();
        let evidence = store.recent_persona_evidence("chat").unwrap();

        assert!(evidence.is_some());
        assert_eq!(
            counter.set_calls.load(Ordering::Relaxed),
            0,
            "full write-back queue must not flush SPIFFS-backed turn ledger inline on agent_loop"
        );
        reset_write_back_queue_for_tests();
    }

    #[test]
    fn write_back_scheduler_coalesces_same_production_label() {
        let _write_back_guard = write_back_test_guard();
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
        let before = snapshot();
        let metrics_before = crate::metrics::snapshot();
        let due_at = Instant::now() + Duration::from_secs(60);

        assert!(schedule_write_back_task(
            "session_store",
            due_at,
            Box::new(|| {})
        ));
        assert!(schedule_write_back_task(
            "session_store",
            due_at + Duration::from_secs(1),
            Box::new(|| {})
        ));

        let after = snapshot();
        let metrics_after = crate::metrics::snapshot();
        assert_eq!(after.queued, 1);
        assert_eq!(after.coalesced_total, before.coalesced_total + 1);
        assert!(
            metrics_after.event_ingress_cancelled_total
                > metrics_before.event_ingress_cancelled_total
        );
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
    }

    #[test]
    fn write_back_scheduler_keeps_order_sensitive_labels_uncoalesced() {
        let _write_back_guard = write_back_test_guard();
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
        let due_at = Instant::now() + Duration::from_secs(60);

        assert!(schedule_write_back_task(
            "turn_ledger_write_back",
            due_at,
            Box::new(|| {})
        ));
        assert!(schedule_write_back_task(
            "turn_ledger_write_back",
            due_at + Duration::from_millis(1),
            Box::new(|| {})
        ));

        assert_eq!(
            snapshot().queued,
            2,
            "turn ledger writes are order-sensitive and must not be label-coalesced"
        );
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
    }

    #[test]
    fn write_back_queue_covers_one_full_runtime_store_fanout() {
        let _write_back_guard = write_back_test_guard();
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
        let labels = std::hint::black_box(BUFFERED_RUNTIME_WRITE_BACK_LABELS);
        assert!(
            labels.len() <= WRITE_BACK_RUNTIME_DOMAIN_FLOOR,
            "write-back floor must cover one flush per buffered runtime store family"
        );
        for &label in labels {
            assert!(
                schedule_write_back_task(label, Instant::now(), Box::new(|| {})),
                "write-back queue rejected buffered runtime label {label}"
            );
        }
        assert_eq!(snapshot().queued, labels.len());

        let mut expected = labels.to_vec();
        expected.sort_unstable();
        assert_eq!(queued_write_back_labels_for_tests(), expected);
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
    }

    #[test]
    fn runtime_services_humanization_stores_use_buffered_write_back() {
        let _write_back_guard = write_back_test_guard();
        let platform: Arc<dyn crate::Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let services = crate::RuntimeServices::from_platform(platform);
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);

        services
            .felt_significance_store
            .set(
                "chat",
                &FeltSignificance {
                    significance_summary: "steady weight".to_string(),
                    ..FeltSignificance::default()
                },
            )
            .unwrap();
        services
            .temperament_continuity_store
            .set(
                "chat",
                &TemperamentContinuity {
                    stability_summary: "stable rhythm".to_string(),
                    ..TemperamentContinuity::default()
                },
            )
            .unwrap();
        services
            .inner_conflict_store
            .set(
                "chat",
                &InnerConflict {
                    topic: "boundary".to_string(),
                    pull_a: "stay open".to_string(),
                    pull_b: "preserve limits".to_string(),
                    ..InnerConflict::default()
                },
            )
            .unwrap();

        assert_eq!(
            queued_write_back_labels_for_tests(),
            vec![
                "felt_significance_write_back",
                "inner_conflict_write_back",
                "temperament_continuity_write_back",
            ]
        );
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
    }

    #[test]
    fn service_write_back_tasks_runs_due_work_off_caller_thread() {
        let _write_back_guard = write_back_test_guard();
        reset_write_back_queue_for_tests();
        let caller_thread = std::thread::current().id();
        let (tx, rx) = std::sync::mpsc::channel();

        assert!(schedule_write_back_task(
            "thread_plane_test",
            Instant::now(),
            Box::new(move || {
                tx.send(std::thread::current().id()).unwrap();
            }),
        ));

        service_write_back_tasks();
        let worker_thread = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("due write-back task should be serviced by the background plane");

        assert_ne!(
            worker_thread, caller_thread,
            "service_write_back_tasks must not execute heavy storage work on the caller stack"
        );
    }

    fn write_back_storage_lease_is_active() -> bool {
        crate::runtime::lease::snapshot()
            .records
            .iter()
            .any(|record| {
                !record.expired
                    && record.kind == crate::runtime::lease::LeaseKind::StorageSessionWrite
                    && record.owner == WRITE_BACK_LEASE_OWNER
            })
    }

    #[test]
    fn write_back_worker_holds_storage_session_write_lease_while_running() {
        let _write_back_guard = write_back_test_guard();
        let _lifecycle_guard = crate::runtime::plane_lifecycle::plane_lifecycle_test_guard();
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
        let _ = crate::runtime::lease::release_owner(WRITE_BACK_LEASE_OWNER);
        crate::orchestrator::apply_memory_snapshot(crate::platform::MemorySnapshot {
            heap_free_internal: 256 * 1024,
            heap_min_free_internal: 240 * 1024,
            heap_free_spiram: 8 * 1024 * 1024,
            heap_total_spiram: 8 * 1024 * 1024,
            heap_min_free_spiram: 8 * 1024 * 1024,
            heap_largest_block_spiram: 8 * 1024 * 1024,
            heap_largest_block: 128 * 1024,
        });
        let (tx, rx) = std::sync::mpsc::channel();

        assert!(schedule_write_back_task(
            "lease_test",
            Instant::now(),
            Box::new(move || {
                tx.send((
                    write_back_storage_lease_is_active(),
                    write_back_lifecycle_state(),
                ))
                .unwrap();
            }),
        ));

        service_write_back_tasks();
        let (lease_active, lifecycle_state) = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("write-back task should run");
        assert!(
            lease_active,
            "write-back tasks must run under StorageSessionWrite lease"
        );
        assert_eq!(
            lifecycle_state,
            Some(crate::runtime::PlaneLifecycleState::Active),
            "write-back lifecycle must only become active for due work under the storage lease"
        );

        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if !write_back_storage_lease_is_active() {
                WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
        let _ = crate::runtime::lease::release_owner(WRITE_BACK_LEASE_OWNER);
        panic!("write-back worker must release StorageSessionWrite lease after due work");
    }

    #[test]
    fn service_write_back_tasks_does_not_start_idle_worker_without_pending_jobs() {
        let _write_back_guard = write_back_test_guard();
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);

        service_write_back_tasks();

        let worker_started = write_back_scheduler()
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .worker_started;
        assert!(
            !worker_started,
            "agent idle polling must not reserve the write-back worker stack without pending jobs"
        );
    }

    #[test]
    fn write_back_worker_releases_stack_after_idle_timeout() {
        let _write_back_guard = write_back_test_guard();
        let _lifecycle_guard = crate::runtime::plane_lifecycle::plane_lifecycle_test_guard();
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(true, Ordering::Release);

        assert!(ensure_write_back_worker_started_inner(false));

        let deadline = Instant::now() + Duration::from_millis(1_000);
        while Instant::now() < deadline {
            let worker_started = write_back_scheduler()
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .worker_started;
            if !worker_started {
                WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
                assert_eq!(
                    write_back_lifecycle_state(),
                    Some(crate::runtime::PlaneLifecycleState::Unloaded),
                    "idle timeout must unload the write-back plane lifecycle"
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
        panic!("idle write-back worker must exit and release its stack budget");
    }

    #[test]
    fn write_back_worker_releases_stack_when_pressure_turns_critical() {
        let _write_back_guard = write_back_test_guard();
        let _lifecycle_guard = crate::runtime::plane_lifecycle::plane_lifecycle_test_guard();
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
        crate::orchestrator::apply_memory_snapshot(crate::platform::MemorySnapshot {
            heap_free_internal: 256 * 1024,
            heap_min_free_internal: 240 * 1024,
            heap_free_spiram: 8 * 1024 * 1024,
            heap_total_spiram: 8 * 1024 * 1024,
            heap_min_free_spiram: 8 * 1024 * 1024,
            heap_largest_block_spiram: 8 * 1024 * 1024,
            heap_largest_block: 128 * 1024,
        });

        let (tx, rx) = std::sync::mpsc::channel();
        {
            let scheduler = write_back_scheduler();
            let mut state = scheduler.state.lock().unwrap_or_else(|e| e.into_inner());
            state.worker_started = true;
            state.jobs.push(WriteBackJob {
                label: "critical_defer_test",
                due_at: Instant::now(),
                task: Some(Box::new(move || {
                    tx.send(()).unwrap();
                })),
            });
        }

        let before_defer = snapshot();
        crate::orchestrator::apply_memory_snapshot(crate::platform::MemorySnapshot {
            heap_free_internal: 48 * 1024,
            heap_min_free_internal: 40 * 1024,
            heap_free_spiram: 8 * 1024 * 1024,
            heap_total_spiram: 8 * 1024 * 1024,
            heap_min_free_spiram: 8 * 1024 * 1024,
            heap_largest_block_spiram: 8 * 1024 * 1024,
            heap_largest_block: 16 * 1024,
        });
        write_back_worker_loop();

        assert!(
            rx.try_recv().is_err(),
            "critical pressure must defer due write-back work instead of running it"
        );
        let state = write_back_scheduler()
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert!(
            !state.worker_started,
            "worker must release its ESP stack after deferring under Critical pressure"
        );
        assert_eq!(state.jobs.len(), 1, "deferred job should remain queued");
        drop(state);
        assert_eq!(snapshot().deferred_total, before_defer.deferred_total + 1);
        assert_eq!(
            write_back_lifecycle_state(),
            Some(crate::runtime::PlaneLifecycleState::Unloaded),
            "Critical pressure deferral should release the write-back lifecycle without Failed"
        );
        assert_eq!(
            write_back_lifecycle_failed_count(),
            0,
            "Critical pressure deferral is controlled backpressure, not a lifecycle failure"
        );

        crate::orchestrator::apply_memory_snapshot(crate::platform::MemorySnapshot {
            heap_free_internal: 256 * 1024,
            heap_min_free_internal: 240 * 1024,
            heap_free_spiram: 8 * 1024 * 1024,
            heap_total_spiram: 8 * 1024 * 1024,
            heap_min_free_spiram: 8 * 1024 * 1024,
            heap_largest_block_spiram: 8 * 1024 * 1024,
            heap_largest_block: 128 * 1024,
        });
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
    }

    fn write_back_resource_for_tests(
        pressure: crate::orchestrator::PressureLevel,
        storage_contention_risk: crate::orchestrator::StorageContentionRisk,
    ) -> crate::orchestrator::ResourceSnapshot {
        let mut resource = crate::orchestrator::snapshot();
        resource.pressure = pressure;
        resource.storage_contention_risk = storage_contention_risk;
        resource.active_http_count = 0;
        resource.active_wss_count = 0;
        resource.active_agent_tasks = 0;
        resource.inbound_depth = 0;
        resource.outbound_depth = 0;
        resource
    }

    #[test]
    fn write_back_admission_allows_cautious_storage_when_idle() {
        let resource = write_back_resource_for_tests(
            crate::orchestrator::PressureLevel::Normal,
            crate::orchestrator::StorageContentionRisk::Cautious,
        );

        assert_eq!(
            write_back_admission_delay_for_resource(&resource, false),
            None
        );
    }

    #[test]
    fn write_back_admission_defers_cautious_storage_when_foreground_active() {
        let mut resource = write_back_resource_for_tests(
            crate::orchestrator::PressureLevel::Normal,
            crate::orchestrator::StorageContentionRisk::Cautious,
        );

        resource.active_http_count = 1;
        assert!(write_back_admission_delay_for_resource(&resource, false).is_some());
        resource.active_http_count = 0;
        resource.active_wss_count = 1;
        assert_eq!(
            write_back_admission_delay_for_resource(&resource, false),
            None,
            "long-lived WSS alone must not starve deferred write-back work"
        );
        resource.active_wss_count = 0;
        resource.active_agent_tasks = 1;
        assert!(write_back_admission_delay_for_resource(&resource, false).is_some());
        resource.active_agent_tasks = 0;
        resource.inbound_depth = 1;
        assert!(write_back_admission_delay_for_resource(&resource, false).is_some());
        resource.inbound_depth = 0;
        resource.outbound_depth = 1;
        assert!(write_back_admission_delay_for_resource(&resource, false).is_some());
    }

    #[test]
    fn write_back_admission_defers_cautious_storage_when_config_active() {
        let resource = write_back_resource_for_tests(
            crate::orchestrator::PressureLevel::Normal,
            crate::orchestrator::StorageContentionRisk::Cautious,
        );

        assert!(write_back_admission_delay_for_resource(&resource, true).is_some());
    }

    #[test]
    fn write_back_admission_defers_healthy_storage_when_foreground_active() {
        let mut resource = write_back_resource_for_tests(
            crate::orchestrator::PressureLevel::Normal,
            crate::orchestrator::StorageContentionRisk::Healthy,
        );

        resource.active_http_count = 1;
        assert!(
            write_back_admission_delay_for_resource(&resource, false).is_some(),
            "S3-class ESP baseline must not start background write-back while HTTP work is active"
        );
        resource.active_http_count = 0;

        resource.active_agent_tasks = 1;
        assert!(
            write_back_admission_delay_for_resource(&resource, false).is_some(),
            "S3-class ESP baseline must not start background write-back while agent work is active"
        );
    }

    #[test]
    fn write_back_admission_uses_wider_retry_window_for_foreground_activity() {
        let mut resource = write_back_resource_for_tests(
            crate::orchestrator::PressureLevel::Normal,
            crate::orchestrator::StorageContentionRisk::Healthy,
        );
        resource.active_agent_tasks = 1;

        assert_eq!(
            write_back_admission_delay_for_resource(&resource, false),
            Some(Duration::from_millis(2_000)),
            "foreground write-back retries should not churn every 500ms on the S3 baseline"
        );
    }

    #[test]
    fn write_back_admission_uses_short_retry_window_for_storage_risk() {
        let mut resource = write_back_resource_for_tests(
            crate::orchestrator::PressureLevel::Normal,
            crate::orchestrator::StorageContentionRisk::Cautious,
        );
        resource.active_http_count = 1;

        assert_eq!(
            write_back_admission_delay_for_resource(&resource, false),
            Some(Duration::from_millis(WRITE_BACK_ADMISSION_DEFER_MS)),
            "storage contention should keep the short retry window even during foreground work"
        );
    }

    #[test]
    fn write_back_admission_allows_healthy_storage_with_queue_or_config_activity_only() {
        let mut resource = write_back_resource_for_tests(
            crate::orchestrator::PressureLevel::Normal,
            crate::orchestrator::StorageContentionRisk::Healthy,
        );

        assert_eq!(
            write_back_admission_delay_for_resource(&resource, true),
            None,
            "config activity alone must not starve healthy write-back"
        );

        resource.inbound_depth = 1;
        assert_eq!(
            write_back_admission_delay_for_resource(&resource, false),
            None,
            "queued inbound work alone must not starve healthy write-back"
        );
        resource.inbound_depth = 0;

        resource.outbound_depth = 1;
        assert_eq!(
            write_back_admission_delay_for_resource(&resource, false),
            None,
            "queued outbound work alone must not starve healthy write-back"
        );
    }

    #[test]
    fn scheduled_retry_token_prevents_write_back_worker_churn() {
        let _write_back_guard = write_back_test_guard();
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(true, Ordering::Release);
        WRITE_BACK_RETRY_SCHEDULED.store(true, Ordering::Release);
        crate::orchestrator::apply_memory_snapshot(crate::platform::MemorySnapshot {
            heap_free_internal: 256 * 1024,
            heap_min_free_internal: 240 * 1024,
            heap_free_spiram: 8 * 1024 * 1024,
            heap_total_spiram: 8 * 1024 * 1024,
            heap_min_free_spiram: 8 * 1024 * 1024,
            heap_largest_block_spiram: 8 * 1024 * 1024,
            heap_largest_block: 128 * 1024,
        });
        let starts_before = snapshot().worker_starts_total;

        assert!(schedule_write_back_task(
            "retry_churn_guard_test",
            Instant::now() + Duration::from_millis(250),
            Box::new(|| {})
        ));

        let state = write_back_scheduler()
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert_eq!(state.jobs.len(), 1);
        assert!(
            !state.worker_started,
            "pending retry must remain a lightweight scheduler token, not a 24KB worker probe"
        );
        drop(state);
        assert_eq!(snapshot().worker_starts_total, starts_before);

        WRITE_BACK_RETRY_SCHEDULED.store(false, Ordering::Release);
        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
        reset_write_back_queue_for_tests();
    }

    #[test]
    fn write_back_admission_defers_healthy_storage_when_largest_block_low() {
        let mut resource = write_back_resource_for_tests(
            crate::orchestrator::PressureLevel::Normal,
            crate::orchestrator::StorageContentionRisk::Healthy,
        );
        resource.heap_largest_block_internal =
            (crate::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32).saturating_sub(1);

        assert!(
            write_back_admission_delay_for_resource(&resource, false).is_some(),
            "low internal largest-block must defer the 24KB write-back worker"
        );
    }

    #[test]
    fn write_back_admission_defers_cautious_storage_when_agent_active() {
        let _write_back_guard = write_back_test_guard();
        reset_write_back_queue_for_tests();
        crate::orchestrator::apply_memory_snapshot(crate::platform::MemorySnapshot {
            heap_free_internal: 256 * 1024,
            heap_min_free_internal: 240 * 1024,
            heap_free_spiram: 8 * 1024 * 1024,
            heap_total_spiram: 8 * 1024 * 1024,
            heap_min_free_spiram: 8 * 1024 * 1024,
            heap_largest_block_spiram: 8 * 1024 * 1024,
            heap_largest_block: 128 * 1024,
        });
        crate::metrics::record_spiffs_lock_wait_us(7_500);
        crate::metrics::record_spiffs_lock_hold_us(0);
        let _agent = crate::orchestrator::begin_agent_task();

        assert!(
            write_back_admission_delay().is_some(),
            "Cautious storage contention must defer write-back while agent foreground work is active"
        );

        crate::metrics::record_spiffs_lock_wait_us(0);
        crate::metrics::record_spiffs_lock_hold_us(0);
    }

    #[test]
    fn write_back_defer_schedules_own_retry() {
        let _write_back_guard = write_back_test_guard();
        let (_state_guard, _delayed_guard) =
            crate::runtime::delayed_task::delayed_task_test_scope();
        reset_write_back_queue_for_tests();
        WRITE_BACK_TEST_AUTO_SERVICE.store(true, Ordering::Release);
        crate::orchestrator::apply_memory_snapshot(crate::platform::MemorySnapshot {
            heap_free_internal: 256 * 1024,
            heap_min_free_internal: 240 * 1024,
            heap_free_spiram: 8 * 1024 * 1024,
            heap_total_spiram: 8 * 1024 * 1024,
            heap_min_free_spiram: 8 * 1024 * 1024,
            heap_largest_block_spiram: 8 * 1024 * 1024,
            heap_largest_block: 128 * 1024,
        });
        crate::metrics::record_spiffs_lock_wait_us(7_500);
        crate::metrics::record_spiffs_lock_hold_us(0);
        let agent = crate::orchestrator::begin_agent_task();
        let (tx, rx) = std::sync::mpsc::channel();

        schedule_write_back_task(
            "retry_wake_test",
            Instant::now(),
            Box::new(move || {
                tx.send(()).unwrap();
            }),
        );

        assert!(
            rx.try_recv().is_err(),
            "foreground storage contention should defer the initial write-back run"
        );
        drop(agent);
        std::thread::sleep(Duration::from_millis(
            WRITE_BACK_FOREGROUND_ADMISSION_DEFER_MS + 25,
        ));
        crate::runtime::service_delayed_tasks();
        rx.recv_timeout(Duration::from_secs(1))
            .expect("write-back retry should wake and run after foreground pressure clears");

        WRITE_BACK_TEST_AUTO_SERVICE.store(false, Ordering::Release);
        crate::metrics::record_spiffs_lock_wait_us(0);
        crate::metrics::record_spiffs_lock_hold_us(0);
    }

    fn write_back_lifecycle_state() -> Option<crate::runtime::PlaneLifecycleState> {
        crate::runtime::plane_lifecycle::snapshot()
            .records
            .iter()
            .find(|record| {
                record.plane == crate::runtime::PlaneId::StorageWriteBack
                    && record.owner == WRITE_BACK_LIFECYCLE_OWNER
            })
            .map(|record| record.state)
    }

    fn write_back_lifecycle_failed_count() -> u32 {
        crate::runtime::plane_lifecycle::snapshot()
            .records
            .iter()
            .find(|record| {
                record.plane == crate::runtime::PlaneId::StorageWriteBack
                    && record.owner == WRITE_BACK_LIFECYCLE_OWNER
            })
            .map(|record| record.failure_count)
            .unwrap_or(0)
    }

    #[test]
    fn enospc_write_back_errors_are_not_retried() {
        let error = Error::io("spiffs_write", std::io::Error::from_raw_os_error(28));

        assert!(!should_retry_write_back_error(&error));
    }

    #[test]
    fn enoent_write_back_errors_are_not_retried_indefinitely() {
        let error = Error::io("spiffs_write_json", std::io::Error::from_raw_os_error(2));

        assert!(!should_retry_write_back_error(&error));
    }
}
