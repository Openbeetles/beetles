//! Debounced write-back wrappers for hot runtime stores.
//! 将热路径上的小型持久化写入从用户/语音临界区中移出，复用现有 delayed task，
//! 不新增常驻线程。

use crate::error::{Error, Result};
use crate::memory::{
    derive_recent_persona_evidence, AutonomyStrategy, AutonomyStrategyStore, CoreRevisionLedger,
    CoreRevisionLedgerStore, ExecutionState, ExecutionStateStore, ImportantMessageStore, InnerLife,
    InnerLifeStore, LongTermMemoryExtractionState, LongTermMemoryExtractionStateStore,
    MentalPrivacyState, MentalPrivacyStore, OuterVoice, OuterVoiceStore, RecentPersonaEvidence,
    RelationshipConstitution, RelationshipConstitutionStore, RelationshipPortfolio,
    RelationshipPortfolioStore, RelationshipTopology, RelationshipTopologyStore, SelfAuthoredCore,
    SelfAuthoredCoreStore, SelfContinuity, SelfContinuityStore, SelfModel, SelfModelStore,
    SessionMessage, SessionStore, SessionSummaryStore, TurnLedger, TurnLedgerStore, WorldSense,
    WorldSenseStore, RECENT_PERSONA_EVIDENCE_MEANINGFUL_TURNS,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const WRITE_BACK_DELAY_MS: u64 = 75;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const WRITE_BACK_DELAY_MS: u64 = 25;

type WriteBackTask = Box<dyn FnOnce() + Send + 'static>;

fn schedule_write_back_task(label: &'static str, due_at: Instant, task: WriteBackTask) -> bool {
    match crate::runtime::schedule_critical_delayed_task(due_at, task) {
        Ok(()) => true,
        Err(_) => {
            log::warn!(
                "[write_back:{}] critical delayed queue full, keeping pending writes queued",
                label
            );
            false
        }
    }
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
        Error::Io { source, .. } => !matches!(source.raw_os_error(), Some(28 | 36 | 63 | 91)),
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
            pending.entry(chat_id).or_insert(write);
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
                .load_recent(chat_id, crate::memory::MAX_SESSION_ENTRIES)?
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

    #[derive(Default)]
    struct StubExecutionStateStore {
        values: Mutex<HashMap<String, ExecutionState>>,
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

    #[test]
    fn buffered_execution_state_reads_pending_before_flush() {
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
    fn buffered_session_store_merges_pending_messages() {
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
    fn buffered_turn_ledger_uses_pending_terminal_for_recent_persona_evidence() {
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
        let (_state_guard, _delayed_guard) =
            crate::runtime::delayed_task::delayed_task_test_scope();
        let due_at = Instant::now() + Duration::from_secs(60);
        let mut accepted = 0usize;
        while crate::runtime::schedule_critical_delayed_task(due_at, Box::new(|| {})).is_ok() {
            accepted += 1;
            assert!(
                accepted < 256,
                "critical delayed queue cap should be finite"
            );
        }
        assert!(accepted > 0);

        let inner = Arc::new(CountingTurnLedgerStore::default());
        let counter = Arc::clone(&inner);
        let store = BufferedTurnLedgerStore::wrap(inner as Arc<dyn TurnLedgerStore + Send + Sync>);

        store.set("chat", &meaningful_turn_ledger()).unwrap();
        let evidence = store.recent_persona_evidence("chat").unwrap();

        assert!(evidence.is_some());
        assert_eq!(
            counter.set_calls.load(Ordering::Relaxed),
            0,
            "full delayed queue must not flush SPIFFS-backed turn ledger inline on agent_loop"
        );
    }

    #[test]
    fn enospc_write_back_errors_are_not_retried() {
        let error = Error::io("spiffs_write", std::io::Error::from_raw_os_error(28));

        assert!(!should_retry_write_back_error(&error));
    }
}
