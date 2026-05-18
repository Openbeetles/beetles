use super::*;
use crate::agent::{ActiveWorkRecord, ActiveWorkStore};
use crate::bus::IngressKind;
use crate::error::{Error, Result};
use crate::llm::{
    LlmClient, LlmHttpClient, LlmResponse, Message, StopReason, ToolChoicePolicy, ToolSpec,
};
use crate::orchestrator::PressureLevel;
use crate::platform::{ResponseBody, SkillStorage};
use crate::runtime::mode::{snapshot_from_source, RuntimeModeSource};
use crate::skills::{RuntimeSkillReuseOutcome, RuntimeSkillWrite, RuntimeSkillWriteSource};
use crate::task::{TaskItem, TaskQuery, TaskStore};
use crate::task_execution::{
    TaskArtifactRecord, TaskArtifactStore, TaskLearningRecord, TaskLearningStore, TaskRunRecord,
    TaskRunStore,
};
use crate::tools::{MemoryManageTool, SessionManageTool, Tool, ToolContext};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::recall_router::PromptRecallRouterInput;

const CHAT_ID: &str = "memory-harness-chat";
const CHANNEL: &str = "test";
const NOW_SECS: u64 = 1_800_000_000;

#[derive(Default)]
struct HarnessLongTermMemoryStore {
    entries: Mutex<Vec<LongTermMemoryEntry>>,
}

impl HarnessLongTermMemoryStore {
    fn with_entry(entry: LongTermMemoryEntry) -> Self {
        Self {
            entries: Mutex::new(vec![entry]),
        }
    }
}

impl LongTermMemoryStore for HarnessLongTermMemoryStore {
    fn upsert_many(&self, drafts: &[LongTermMemoryDraft], now_secs: u64) -> Result<usize> {
        let mut changed = 0usize;
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        for draft in drafts {
            let Some(id) = draft.stable_id() else {
                continue;
            };
            let Some(entry) = long_term_memory_entry_from_draft(draft, id.clone(), now_secs) else {
                continue;
            };
            if let Some(existing) = entries.iter_mut().find(|existing| existing.id == id) {
                *existing = entry;
            } else {
                entries.push(entry);
            }
            changed = changed.saturating_add(1);
        }
        Ok(changed)
    }

    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>> {
        let query = query.to_lowercase();
        let mut matches = self
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|entry| {
                source_chat_id.is_none()
                    || entry.source_chat_id.as_deref() == source_chat_id
                    || !matches!(entry.source_scope, LongTermMemorySourceScope::Chat)
            })
            .filter(|entry| {
                let haystack = format!(
                    "{} {} {}",
                    entry.topic,
                    entry.content,
                    entry.keywords.join(" ")
                )
                .to_lowercase();
                query
                    .split_whitespace()
                    .any(|term| term.len() >= 3 && haystack.contains(term))
            })
            .cloned()
            .collect::<Vec<_>>();
        matches.truncate(limit);
        Ok(matches)
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        Ok(self
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|entry| entry.id == id)
            .cloned())
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        entries.truncate(limit);
        Ok(entries)
    }

    fn delete(&self, id: &str) -> Result<bool> {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let before = entries.len();
        entries.retain(|entry| entry.id != id);
        Ok(entries.len() != before)
    }

    fn delete_slot(&self, slot: &LongTermMemorySlot) -> Result<bool> {
        let Some(id) = slot.stable_id() else {
            return Ok(false);
        };
        self.delete(&id)
    }

    fn count(&self) -> Result<usize> {
        Ok(self.entries.lock().unwrap_or_else(|e| e.into_inner()).len())
    }
}

#[derive(Default)]
struct HarnessMemoryStore {
    memory: Mutex<String>,
    notes: Mutex<HashMap<String, String>>,
}

impl MemoryStore for HarnessMemoryStore {
    fn get_memory(&self) -> Result<String> {
        Ok(self
            .memory
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone())
    }

    fn set_memory(&self, content: &str) -> Result<()> {
        *self.memory.lock().unwrap_or_else(|e| e.into_inner()) = content.to_string();
        Ok(())
    }

    fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
        let mut names = self
            .notes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        names.sort_by(|left, right| right.cmp(left));
        names.truncate(recent_n);
        Ok(names)
    }

    fn get_daily_note(&self, name: &str) -> Result<String> {
        Ok(self
            .notes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .cloned()
            .unwrap_or_default())
    }

    fn write_daily_note(&self, name: &str, content: &str) -> Result<()> {
        self.notes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.to_string(), content.to_string());
        Ok(())
    }
}

#[derive(Default)]
struct HarnessSessionStore {
    chats: Mutex<HashMap<String, Vec<SessionMessage>>>,
}

impl HarnessSessionStore {
    fn seed(&self, chat_id: &str, messages: Vec<SessionMessage>) {
        self.chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(chat_id.to_string(), messages);
    }
}

impl SessionStore for HarnessSessionStore {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
        self.chats
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

    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
        let messages = self
            .chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(chat_id)
            .cloned()
            .unwrap_or_default();
        let start = messages.len().saturating_sub(n);
        Ok(messages.into_iter().skip(start).collect())
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(chat_id.to_string(), Vec::new());
        Ok(())
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        let mut ids = self
            .chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        ids.sort();
        Ok(ids)
    }

    fn delete(&self, chat_id: &str) -> Result<()> {
        self.chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(chat_id);
        Ok(())
    }
}

#[derive(Default)]
struct HarnessTurnLedgerStore {
    ledgers: Mutex<HashMap<String, TurnLedger>>,
}

impl TurnLedgerStore for HarnessTurnLedgerStore {
    fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
        Ok(self
            .ledgers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(chat_id)
            .cloned())
    }

    fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()> {
        self.ledgers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(chat_id.to_string(), ledger.clone());
        Ok(())
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.ledgers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(chat_id);
        Ok(())
    }
}

#[derive(Default)]
struct HarnessTurnContinuityEvidenceStore {
    evidence: Mutex<HashMap<String, Vec<TurnContinuityEvidence>>>,
}

impl TurnContinuityEvidenceStore for HarnessTurnContinuityEvidenceStore {
    fn append(&self, chat_id: &str, evidence: &TurnContinuityEvidence) -> Result<()> {
        self.evidence
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(chat_id.to_string())
            .or_default()
            .push(evidence.clone());
        Ok(())
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.evidence
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(chat_id);
        Ok(())
    }

    fn list_recent(&self, chat_id: &str, limit: usize) -> Result<Vec<TurnContinuityEvidence>> {
        let mut items = self
            .evidence
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(chat_id)
            .cloned()
            .unwrap_or_default();
        items.reverse();
        items.truncate(limit);
        Ok(items)
    }
}

#[derive(Default)]
struct HarnessLongTermMemoryExtractionStateStore {
    state: Mutex<Option<LongTermMemoryExtractionState>>,
}

impl LongTermMemoryExtractionStateStore for HarnessLongTermMemoryExtractionStateStore {
    fn get(&self, _chat_id: &str) -> Result<Option<LongTermMemoryExtractionState>> {
        Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }

    fn set(&self, _chat_id: &str, state: &LongTermMemoryExtractionState) -> Result<()> {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.clone());
        Ok(())
    }

    fn clear(&self, _chat_id: &str) -> Result<()> {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }
}

#[derive(Default)]
struct HarnessSkillStorage {
    files: Mutex<HashMap<String, Vec<u8>>>,
}

impl HarnessSkillStorage {
    fn names(&self) -> Vec<String> {
        let mut names = self
            .files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        names.sort();
        names
    }
}

impl SkillStorage for HarnessSkillStorage {
    fn list_names(&self) -> Result<Vec<String>> {
        Ok(self.names())
    }

    fn read(&self, name: &str) -> Result<Vec<u8>> {
        self.files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .cloned()
            .ok_or_else(|| Error::config("memory_harness_skill_read", "missing skill"))
    }

    fn write(&self, name: &str, content: &[u8]) -> Result<()> {
        self.files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.to_string(), content.to_vec());
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<()> {
        self.files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
        Ok(())
    }
}

#[derive(Default)]
struct HarnessStores {
    session: Arc<HarnessSessionStore>,
    memory: Arc<HarnessMemoryStore>,
    long_term: Arc<HarnessLongTermMemoryStore>,
    turn_ledger: HarnessTurnLedgerStore,
    turn_continuity_evidence: HarnessTurnContinuityEvidenceStore,
    extraction_state: HarnessLongTermMemoryExtractionStateStore,
    skills: Arc<HarnessSkillStorage>,
    summary: EmptySessionSummaryStore,
    execution_state: EmptyExecutionStateStore,
    active_work: EmptyActiveWorkStore,
    task_runs: EmptyTaskRunStore,
    task_artifacts: EmptyTaskArtifactStore,
    task_learning: EmptyTaskLearningStore,
    self_model: EmptySelfModelStore,
    self_authored_core: EmptySelfAuthoredCoreStore,
    relationship_constitution: EmptyRelationshipConstitutionStore,
    relationship_portfolio: EmptyRelationshipPortfolioStore,
    relationship_topology: EmptyRelationshipTopologyStore,
    world_sense: EmptyWorldSenseStore,
    autonomy_strategy: EmptyAutonomyStrategyStore,
    outer_voice: EmptyOuterVoiceStore,
    inner_life: EmptyInnerLifeStore,
    self_continuity: EmptySelfContinuityStore,
    felt_significance: EmptyFeltSignificanceStore,
    temperament_continuity: EmptyTemperamentContinuityStore,
    inner_conflict: EmptyInnerConflictStore,
    private_docs: EmptyPrivateDocStore,
    private_garden: EmptyPrivateGardenStore,
    mental_privacy: EmptyMentalPrivacyStore,
    reminders: EmptyRemindAtStore,
    tasks: EmptyTaskStore,
    continuity: EmptyContinuityCapsuleStore,
}

impl HarnessStores {
    fn prompt_params<'a>(
        &'a self,
        user_query: &'a str,
        memory_system_kind: MemorySystemKind,
        participation_plan: PromptParticipationPlan,
    ) -> PromptMemoryContextParams<'a> {
        PromptMemoryContextParams {
            chat_id: CHAT_ID,
            current_channel: CHANNEL,
            user_query,
            memory_system_kind,
            system_max_len: 1024,
            now_secs: NOW_SECS,
            participation_plan,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: false,
            session_store: self.session.as_ref(),
            memory_store: self.memory.as_ref(),
            session_summary_store: &self.summary,
            long_term_memory_store: self.long_term.as_ref(),
            execution_state_store: &self.execution_state,
            active_work_store: &self.active_work,
            task_run_store: &self.task_runs,
            task_artifact_store: &self.task_artifacts,
            task_learning_store: &self.task_learning,
            self_model_store: &self.self_model,
            self_authored_core_store: &self.self_authored_core,
            relationship_constitution_store: &self.relationship_constitution,
            relationship_portfolio_store: &self.relationship_portfolio,
            relationship_topology_store: &self.relationship_topology,
            world_sense_store: &self.world_sense,
            autonomy_strategy_store: &self.autonomy_strategy,
            outer_voice_store: &self.outer_voice,
            inner_life_store: &self.inner_life,
            self_continuity_store: &self.self_continuity,
            felt_significance_store: &self.felt_significance,
            temperament_continuity_store: &self.temperament_continuity,
            inner_conflict_store: &self.inner_conflict,
            private_doc_store: &self.private_docs,
            private_garden_store: &self.private_garden,
            mental_privacy_store: &self.mental_privacy,
            remind_store: &self.reminders,
            task_store: &self.tasks,
            turn_continuity_evidence_store: &self.turn_continuity_evidence,
            turn_ledger_store: &self.turn_ledger,
            skill_storage: self.skills.as_ref(),
            continuity_capsule_store: &self.continuity,
        }
    }
}

#[derive(Default)]
struct EmptySessionSummaryStore;
impl SessionSummaryStore for EmptySessionSummaryStore {
    fn get(&self, _chat_id: &str) -> Result<Option<String>> {
        Ok(None)
    }
    fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
        Ok(())
    }
    fn get_with_count(&self, _chat_id: &str) -> Result<Option<(String, usize)>> {
        Ok(None)
    }
}

#[derive(Default)]
struct EmptyExecutionStateStore;
impl ExecutionStateStore for EmptyExecutionStateStore {
    fn get(&self, _chat_id: &str) -> Result<Option<ExecutionState>> {
        Ok(None)
    }
    fn set(&self, _chat_id: &str, _state: &ExecutionState) -> Result<()> {
        Ok(())
    }
    fn clear(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct EmptyActiveWorkStore;
impl ActiveWorkStore for EmptyActiveWorkStore {
    fn get(&self, _chat_id: &str) -> Result<Option<ActiveWorkRecord>> {
        Ok(None)
    }
    fn set(&self, _chat_id: &str, _record: &ActiveWorkRecord) -> Result<()> {
        Ok(())
    }
    fn clear(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct EmptyTaskRunStore;
impl TaskRunStore for EmptyTaskRunStore {
    fn get(&self, _run_id: &str) -> Result<Option<TaskRunRecord>> {
        Ok(None)
    }
    fn upsert(&self, _record: &TaskRunRecord) -> Result<()> {
        Ok(())
    }
    fn list_recent(&self, _limit: usize) -> Result<Vec<TaskRunRecord>> {
        Ok(Vec::new())
    }
    fn list_active_for_chat(
        &self,
        _channel: &str,
        _chat_id: &str,
        _limit: usize,
    ) -> Result<Vec<TaskRunRecord>> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct EmptyTaskArtifactStore;
impl TaskArtifactStore for EmptyTaskArtifactStore {
    fn put(&self, _record: &TaskArtifactRecord) -> Result<()> {
        Ok(())
    }
    fn list_for_run(&self, _run_id: &str, _limit: usize) -> Result<Vec<TaskArtifactRecord>> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct EmptyTaskLearningStore;
impl TaskLearningStore for EmptyTaskLearningStore {
    fn get(&self, _learning_id: &str) -> Result<Option<TaskLearningRecord>> {
        Ok(None)
    }
    fn upsert(&self, _record: &TaskLearningRecord) -> Result<()> {
        Ok(())
    }
    fn list_recent(&self, _limit: usize) -> Result<Vec<TaskLearningRecord>> {
        Ok(Vec::new())
    }
    fn list_for_chat(
        &self,
        _channel: &str,
        _chat_id: &str,
        _limit: usize,
    ) -> Result<Vec<TaskLearningRecord>> {
        Ok(Vec::new())
    }
    fn list_for_run(&self, _run_id: &str, _limit: usize) -> Result<Vec<TaskLearningRecord>> {
        Ok(Vec::new())
    }
}

macro_rules! empty_chat_store {
    ($name:ident, $trait_name:ident, $ty:ty) => {
        #[derive(Default)]
        struct $name;
        impl $trait_name for $name {
            fn get(&self, _chat_id: &str) -> Result<Option<$ty>> {
                Ok(None)
            }
            fn set(&self, _chat_id: &str, _value: &$ty) -> Result<()> {
                Ok(())
            }
            fn clear(&self, _chat_id: &str) -> Result<()> {
                Ok(())
            }
        }
    };
}

macro_rules! empty_scope_store {
    ($name:ident, $trait_name:ident, $ty:ty) => {
        #[derive(Default)]
        struct $name;
        impl $trait_name for $name {
            fn get(&self, _scope_id: &str) -> Result<Option<$ty>> {
                Ok(None)
            }
            fn set(&self, _scope_id: &str, _value: &$ty) -> Result<()> {
                Ok(())
            }
            fn clear(&self, _scope_id: &str) -> Result<()> {
                Ok(())
            }
        }
    };
}

empty_chat_store!(EmptySelfModelStore, SelfModelStore, SelfModel);
empty_scope_store!(
    EmptySelfAuthoredCoreStore,
    SelfAuthoredCoreStore,
    SelfAuthoredCore
);
empty_scope_store!(
    EmptyRelationshipConstitutionStore,
    RelationshipConstitutionStore,
    RelationshipConstitution
);
empty_scope_store!(
    EmptyRelationshipPortfolioStore,
    RelationshipPortfolioStore,
    RelationshipPortfolio
);
empty_scope_store!(
    EmptyRelationshipTopologyStore,
    RelationshipTopologyStore,
    RelationshipTopology
);
empty_chat_store!(EmptyWorldSenseStore, WorldSenseStore, WorldSense);
empty_chat_store!(
    EmptyAutonomyStrategyStore,
    AutonomyStrategyStore,
    AutonomyStrategy
);
empty_chat_store!(EmptyOuterVoiceStore, OuterVoiceStore, OuterVoice);
empty_chat_store!(EmptyInnerLifeStore, InnerLifeStore, InnerLife);
empty_chat_store!(
    EmptySelfContinuityStore,
    SelfContinuityStore,
    SelfContinuity
);
empty_scope_store!(
    EmptyFeltSignificanceStore,
    FeltSignificanceStore,
    FeltSignificance
);
empty_scope_store!(
    EmptyTemperamentContinuityStore,
    TemperamentContinuityStore,
    TemperamentContinuity
);
empty_scope_store!(EmptyInnerConflictStore, InnerConflictStore, InnerConflict);
empty_chat_store!(EmptyPrivateDocStore, PrivateDocStore, PrivateDocWorkspace);
empty_chat_store!(
    EmptyMentalPrivacyStore,
    MentalPrivacyStore,
    MentalPrivacyState
);

#[derive(Default)]
struct EmptyPrivateGardenStore;
impl PrivateGardenStore for EmptyPrivateGardenStore {
    fn list(&self, _chat_id: &str, _limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
        Ok(Vec::new())
    }
    fn read(&self, _chat_id: &str, _doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
        Ok(None)
    }
    fn write(
        &self,
        _chat_id: &str,
        _doc_path: &str,
        _content: &str,
        _now_secs: u64,
    ) -> Result<PrivateGardenDocRecord> {
        Err(Error::config(
            "memory_harness_private_garden",
            "not writable",
        ))
    }
    fn move_doc(
        &self,
        _chat_id: &str,
        _from_path: &str,
        _to_path: &str,
        _now_secs: u64,
    ) -> Result<Option<PrivateGardenDocRecord>> {
        Ok(None)
    }
    fn delete(&self, _chat_id: &str, _doc_path: &str) -> Result<bool> {
        Ok(false)
    }
}

#[derive(Default)]
struct EmptyRemindAtStore;
impl RemindAtStore for EmptyRemindAtStore {
    fn get(
        &self,
        _channel: &str,
        _chat_id: &str,
        _id: &str,
    ) -> Result<Option<crate::reminder::ReminderItem>> {
        Ok(None)
    }
    fn upsert(&self, _reminder: &crate::reminder::ReminderItem) -> Result<()> {
        Ok(())
    }
    fn delete(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<bool> {
        Ok(false)
    }
    fn list_due(
        &self,
        _now_unix_secs: u64,
        _limit: usize,
    ) -> Result<Vec<crate::reminder::ReminderItem>> {
        Ok(Vec::new())
    }
    fn delete_due(&self, _reminder: &crate::reminder::ReminderItem) -> Result<bool> {
        Ok(false)
    }
    fn list_upcoming(
        &self,
        _channel: &str,
        _chat_id: &str,
        _now_unix_secs: u64,
        _limit: usize,
    ) -> Result<Vec<crate::reminder::ReminderItem>> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct EmptyTaskStore;
impl TaskStore for EmptyTaskStore {
    fn list(&self, _channel: &str, _chat_id: &str, _query: TaskQuery) -> Result<Vec<TaskItem>> {
        Ok(Vec::new())
    }
    fn get(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<Option<TaskItem>> {
        Ok(None)
    }
    fn upsert(&self, _task: &TaskItem) -> Result<()> {
        Ok(())
    }
    fn delete(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<bool> {
        Ok(false)
    }
    fn list_due_unnotified(&self, _now_unix_secs: u64, _limit: usize) -> Result<Vec<TaskItem>> {
        Ok(Vec::new())
    }
    fn mark_due_notified(&self, _task: &TaskItem, _notified_at_unix_secs: u64) -> Result<bool> {
        Ok(false)
    }
}

#[derive(Default)]
struct EmptyContinuityCapsuleStore;
impl ContinuityCapsuleStore for EmptyContinuityCapsuleStore {
    fn upsert_many(
        &self,
        _drafts: &[ContinuityCapsuleDraft],
        _now_secs: u64,
    ) -> Result<ContinuityCapsuleWriteOutcome> {
        Ok(ContinuityCapsuleWriteOutcome::default())
    }
    fn get(&self, _capsule_id: &str) -> Result<Option<ContinuityCapsule>> {
        Ok(None)
    }
    fn list(&self, _limit: usize) -> Result<Vec<ContinuityCapsule>> {
        Ok(Vec::new())
    }
    fn count(&self) -> Result<usize> {
        Ok(0)
    }
}

#[derive(Default)]
struct NullToolContext;
impl ToolContext for NullToolContext {
    fn get_with_headers(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
    ) -> Result<(u16, ResponseBody)> {
        Err(Error::config(
            "memory_harness_tool_context",
            "network disabled",
        ))
    }
    fn post_with_headers(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        Err(Error::config(
            "memory_harness_tool_context",
            "network disabled",
        ))
    }

    fn user_locale(&self) -> crate::i18n::Locale {
        crate::i18n::Locale::Zh
    }
}

struct NullLlmHttpClient;

impl LlmHttpClient for NullLlmHttpClient {
    fn do_post(
        &mut self,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        Err(Error::config("memory_harness_http", "network disabled"))
    }
}

struct DeterministicReplayLlm;

impl LlmClient for DeterministicReplayLlm {
    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        system: &str,
        messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let content = if system == LONG_TERM_MEMORY_EXTRACTION_SYSTEM_PROMPT {
            let prompt = messages
                .first()
                .map(|message| message.content.as_str())
                .unwrap_or("");
            if !prompt.contains("When we do engineering review, keep the response in Chinese.")
                || !prompt.contains("Remember this as the release checklist for later work.")
            {
                return Err(Error::config(
                    "memory_harness_l2_replay",
                    "extraction prompt missing seeded transcript",
                ));
            }
            json!([
                {
                    "plane": "factual",
                    "op": "upsert",
                    "kind": "preference",
                    "topic": "preferred_engineering_language",
                    "content": "User prefers Chinese for engineering review conversations.",
                    "keywords": ["Chinese", "engineering", "review"],
                    "source_type": "conversation",
                    "source_scope": "user",
                    "confidence": "high",
                    "freshness": "stable",
                    "stale_hint": "none"
                },
                {
                    "plane": "skill",
                    "topic": "release_checklist",
                    "content": "- validate the diff\n- run cargo test\n- run the analyzer before claiming release readiness",
                    "skill_summary": "Run release checks before claiming readiness."
                }
            ])
            .to_string()
        } else if system.contains("compact live execution state") {
            "null".to_string()
        } else if system.contains("conversation summarizer") {
            "User prefers Chinese for engineering review and expects release readiness to be backed by tests and analyzer evidence.".to_string()
        } else {
            "[]".to_string()
        };
        Ok(LlmResponse {
            content,
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}

struct MemoryHarnessL2ReplayResult {
    extraction_request_outcome: LongTermMemoryRefreshRequestOutcome,
    refresh_changed_count: usize,
    extraction_state_processed_clean: bool,
    prompt_shared_ids_before_refresh: Vec<String>,
    prompt_shared_ids_after_refresh: Vec<String>,
    inspection_shared_ids_after_refresh: Vec<String>,
    prompt_runtime_skill_ids_after_refresh: Vec<String>,
    inspection_runtime_skill_ids_after_refresh: Vec<String>,
    stored_factual_content_after_refresh: String,
    inspection_runtime_skill_text_after_refresh: String,
}

fn run_memory_harness_l2_production_replay() -> MemoryHarnessL2ReplayResult {
    let stores = HarnessStores::default();
    stores.session.seed(
        CHAT_ID,
        vec![
            SessionMessage {
                role: "user".to_string(),
                content: "When we do engineering review, keep the response in Chinese.".to_string(),
            },
            SessionMessage {
                role: "assistant".to_string(),
                content: "I will keep engineering review responses in Chinese and stay evidence-first."
                    .to_string(),
            },
            SessionMessage {
                role: "user".to_string(),
                content: "For release readiness, validate the diff and run tests before claiming it."
                    .to_string(),
            },
            SessionMessage {
                role: "assistant".to_string(),
                content: "Release readiness should cite test and analyzer evidence before any claim."
                    .to_string(),
            },
            SessionMessage {
                role: "user".to_string(),
                content: "Remember this as the release checklist for later work.".to_string(),
            },
            SessionMessage {
                role: "assistant".to_string(),
                content:
                    "I will use the release checklist before saying a memory or runtime change is ready."
                        .to_string(),
            },
        ],
    );

    let query = "preferred engineering language Chinese release checklist";
    let participation_plan = PromptParticipationPlan {
        load_l1_constitutional: true,
        load_l1_session: true,
        load_l2_governed_recall: true,
        load_l2_background_governance: false,
        load_l3_private_depth: false,
    };
    let before_prompt = load_prompt_memory_context(stores.prompt_params(
        query,
        MemorySystemKind::LinuxFull,
        participation_plan,
    ));
    let prompt_shared_ids_before_refresh = before_prompt
        .shared_factual_recall_report
        .selected_ids
        .clone();
    let before_carry = before_prompt.into_runtime_carry();

    let mut http = NullLlmHttpClient;
    let llm = DeterministicReplayLlm;
    let mut refresh_enqueued = false;
    let maintenance = run_post_reply_memory_maintenance(
        &mut http,
        &llm,
        PostReplyMemoryMaintenanceContext {
            session_store: stores.session.as_ref(),
            memory_store: stores.memory.as_ref(),
            session_summary_store: &stores.summary,
            execution_state_store: &stores.execution_state,
            active_work_store: &stores.active_work,
            long_term_memory_store: stores.long_term.as_ref(),
            continuity_capsule_store: &stores.continuity,
            extraction_state_store: &stores.extraction_state,
            turn_ledger_store: &stores.turn_ledger,
            skill_storage: stores.skills.as_ref(),
            task_run_store: &stores.task_runs,
            task_artifact_store: &stores.task_artifacts,
            task_learning_store: &stores.task_learning,
        },
        PostReplyMemoryMaintenanceInput {
            chat_id: CHAT_ID,
            ingress: IngressKind::User,
            channel: CHANNEL,
            user_content: "When we do engineering review, keep the response in Chinese.",
            reply_content:
                "I will use Chinese for engineering review and run release checks before readiness claims.",
            pressure: PressureLevel::Normal,
            memory_profile: MemoryProfile::Standard,
            tool_calls: 0,
            external_content_used: false,
            prompt_recall_intent: before_carry.prompt_recall_intent,
            runtime_skill_selected_ids: before_carry.runtime_skill_selected_ids,
            task_learning_selected_ids: before_carry.task_recall_selected_ids,
            reuse_outcome: RuntimeSkillReuseOutcome::Neutral,
            reuse_outcome_note: "",
            now_secs: NOW_SECS,
        },
        || {
            refresh_enqueued = true;
            true
        },
    );
    assert!(refresh_enqueued);

    let refresh = run_long_term_memory_refresh(
        &mut http,
        &llm,
        LongTermMemoryRefreshContext {
            memory_store: stores.memory.as_ref(),
            session_store: stores.session.as_ref(),
            session_summary_store: &stores.summary,
            long_term_memory_store: stores.long_term.as_ref(),
            extraction_state_store: &stores.extraction_state,
            turn_ledger_store: &stores.turn_ledger,
            skill_storage: stores.skills.as_ref(),
        },
        CHAT_ID,
        PressureLevel::Normal,
        MemoryProfile::Standard,
    );
    let refresh_changed_count = match &refresh {
        LongTermMemoryRefreshOutcome::Processed { changed_count, .. } => *changed_count,
        LongTermMemoryRefreshOutcome::Deferred { .. } => {
            panic!("expected production replay refresh to process")
        }
        LongTermMemoryRefreshOutcome::Failed { error, .. } => {
            panic!("expected production replay refresh to process, got {error}")
        }
    };
    refresh.persist(&stores.extraction_state, CHAT_ID);

    let after_prompt = load_prompt_memory_context(stores.prompt_params(
        query,
        MemorySystemKind::LinuxFull,
        participation_plan,
    ));
    let inspection = inspect_working_recall(WorkingRecallInspectionInput {
        chat_id: CHAT_ID,
        query,
        summary_text: None,
        recent: &after_prompt.recent_messages,
        system_max_len: 1024,
        profile: MemoryProfile::Standard,
        current_channel: Some(CHANNEL),
        session_store: stores.session.as_ref(),
        memory_store: stores.memory.as_ref(),
        long_term_memory_store: stores.long_term.as_ref(),
        active_work_store: Some(&stores.active_work),
        continuity_capsule_store: &stores.continuity,
        turn_ledger_store: &stores.turn_ledger,
        skill_storage: Some(stores.skills.as_ref()),
        task_run_store: Some(&stores.task_runs),
        task_learning_store: Some(&stores.task_learning),
    });

    MemoryHarnessL2ReplayResult {
        extraction_request_outcome: maintenance.extraction_request_outcome,
        refresh_changed_count,
        extraction_state_processed_clean: stores
            .extraction_state
            .get(CHAT_ID)
            .unwrap()
            .map(|state| {
                !state.pending
                    && state.dirty_since_count == 0
                    && state.dirty_turns == 0
                    && state.last_processed_at_count >= 6
            })
            .unwrap_or(false),
        prompt_shared_ids_before_refresh,
        prompt_shared_ids_after_refresh: after_prompt
            .shared_factual_recall_report
            .selected_ids
            .clone(),
        inspection_shared_ids_after_refresh: inspection.shared_factual_report.selected_ids.clone(),
        prompt_runtime_skill_ids_after_refresh: after_prompt
            .runtime_skill_recall_report
            .selected_ids
            .clone(),
        inspection_runtime_skill_ids_after_refresh: inspection
            .runtime_skill_report
            .selected_ids
            .clone(),
        stored_factual_content_after_refresh: stores
            .long_term
            .list(8)
            .unwrap()
            .into_iter()
            .find(|entry| entry.topic == "preferred_engineering_language")
            .map(|entry| entry.content)
            .unwrap_or_default(),
        inspection_runtime_skill_text_after_refresh: inspection
            .runtime_skill_text
            .unwrap_or_default(),
    }
}

fn factual_draft(
    kind: LongTermMemoryKind,
    topic: &str,
    content: &str,
    confidence: LongTermMemoryConfidence,
    observed_at: u64,
) -> LongTermMemoryDraft {
    LongTermMemoryDraft {
        kind,
        topic: topic.to_string(),
        content: content.to_string(),
        keywords: topic
            .split('_')
            .map(str::to_string)
            .collect::<Vec<String>>(),
        source_chat_id: Some(CHAT_ID.to_string()),
        source_type: Some(LongTermMemorySourceType::Conversation),
        source_scope: Some(LongTermMemorySourceScope::User),
        confidence: Some(confidence),
        freshness: Some(LongTermMemoryFreshness::Stable),
        stale_hint: Some(LongTermMemoryStaleHint::None),
        supporting_citations: Vec::new(),
        evidence_count: None,
        observed_at: Some(observed_at),
        last_confirmed_at: Some(observed_at),
        source_revision: None,
    }
}

fn entry_from_draft(draft: &LongTermMemoryDraft, now_secs: u64) -> LongTermMemoryEntry {
    long_term_memory_entry_from_draft(
        draft,
        draft.stable_id().expect("draft should have a stable id"),
        now_secs,
    )
    .expect("draft should become an entry")
}

fn selected_report(plane: RecallPlane, id: &str, score: u32) -> RecallSelectionReport {
    RecallSelectionReport {
        plane,
        query: RecallQuery {
            plane,
            raw_query: "how should I run the release checklist".to_string(),
            normalized_query: "how should i run the release checklist".to_string(),
            requested_limit: 4,
            max_chars: 1024,
            ..RecallQuery::default()
        },
        backend: "harness".to_string(),
        candidate_count: 1,
        selected_count: 1,
        selected_ids: vec![id.to_string()],
        candidates: vec![RecallCandidate {
            plane,
            candidate_id: id.to_string(),
            title: id.to_string(),
            excerpt: "release checklist".to_string(),
            selected: true,
            score: RecallScoreBreakdown {
                exact_match_score: score,
                lexical_score: score,
                total_score: score,
                reason_fragments: vec!["harness".to_string()],
                ..RecallScoreBreakdown::default()
            },
            ..RecallCandidate::default()
        }],
        ..RecallSelectionReport::default()
    }
}

fn empty_report(plane: RecallPlane) -> RecallSelectionReport {
    RecallSelectionReport {
        plane,
        query: RecallQuery {
            plane,
            ..RecallQuery::default()
        },
        miss_reason: Some("no_candidates".to_string()),
        ..RecallSelectionReport::default()
    }
}

fn assert_report_surface_matches(
    prompt: &RecallSelectionReport,
    inspection: &RecallSelectionReport,
) {
    assert_eq!(prompt.selected_ids, inspection.selected_ids);
    assert_eq!(prompt.miss_reason, inspection.miss_reason);
}

#[test]
fn memory_harness_write_governance() {
    let store = HarnessLongTermMemoryStore::default();
    let accepted = factual_draft(
        LongTermMemoryKind::Preference,
        "preferred_engineering_language",
        "User prefers Chinese for engineering review conversations.",
        LongTermMemoryConfidence::High,
        100,
    );

    let outcome = write_governed_shared_memory(
        &store,
        std::slice::from_ref(&accepted),
        NOW_SECS,
        SharedMemoryWriteSource::ManualTool,
    )
    .unwrap();

    assert_eq!(outcome.accepted, 1);
    assert_eq!(outcome.rejected, 0);
    assert_eq!(outcome.changed, 1);
    assert_eq!(outcome.reports[0].action, SharedMemoryWriteAction::Accepted);
    assert_eq!(
        outcome.reports[0].reason,
        SharedMemoryWriteReason::DurableFact
    );
    assert_eq!(store.count().unwrap(), 1);

    let raw = LongTermMemoryDraft {
        content: "{\"request\":{\"api_key\":\"secret\",\"headers\":{\"x\":1}},\"response\":{\"body\":{\"nested\":true}}}".to_string(),
        ..accepted.clone()
    };
    let raw_outcome = write_governed_shared_memory(
        &store,
        &[raw],
        NOW_SECS,
        SharedMemoryWriteSource::Extraction,
    )
    .unwrap();
    assert_eq!(raw_outcome.accepted, 0);
    assert_eq!(
        raw_outcome.reports[0].reason,
        SharedMemoryWriteReason::RawPayloadOrLog
    );

    let procedure = LongTermMemoryDraft {
        kind: LongTermMemoryKind::Task,
        topic: "release_checklist".to_string(),
        content: "- open release report\n- run cargo test\n- verify analyzer output".to_string(),
        source_type: Some(LongTermMemorySourceType::ManualTool),
        ..accepted.clone()
    };
    let routed_outcome = write_governed_shared_memory(
        &store,
        &[procedure],
        NOW_SECS,
        SharedMemoryWriteSource::ManualTool,
    )
    .unwrap();
    assert_eq!(
        routed_outcome.reports[0].reason,
        SharedMemoryWriteReason::RoutedToSkill
    );

    let older = LongTermMemoryDraft {
        content: "User prefers English for engineering review conversations.".to_string(),
        observed_at: Some(50),
        confidence: Some(LongTermMemoryConfidence::High),
        ..accepted.clone()
    };
    let older_outcome = write_governed_shared_memory(
        &store,
        &[older],
        NOW_SECS,
        SharedMemoryWriteSource::Extraction,
    )
    .unwrap();
    assert_eq!(
        older_outcome.reports[0].reason,
        SharedMemoryWriteReason::OlderThanExisting
    );

    let lower_confidence = LongTermMemoryDraft {
        content: "User prefers short English summaries for engineering review conversations."
            .to_string(),
        observed_at: Some(150),
        confidence: Some(LongTermMemoryConfidence::Low),
        ..accepted
    };
    let lower_outcome = write_governed_shared_memory(
        &store,
        &[lower_confidence],
        NOW_SECS,
        SharedMemoryWriteSource::Extraction,
    )
    .unwrap();
    assert_eq!(
        lower_outcome.reports[0].reason,
        SharedMemoryWriteReason::LowerConfidenceThanExisting
    );
}

#[test]
fn memory_harness_recall_route() {
    let shared = selected_report(RecallPlane::SharedFactual, "fact-1", 30);
    let empty_continuity = empty_report(RecallPlane::ContinuityCapsule);
    let empty_archive = empty_report(RecallPlane::Archive);
    let runtime = selected_report(RecallPlane::RuntimeSkill, "skill-1", 48);

    let procedural = decide_prompt_recall_route(PromptRecallRouterInput {
        user_query: "how should I run the release checklist",
        has_active_continuity: false,
        has_active_task_run: false,
        shared_factual_report: &shared,
        continuity_capsule_report: &empty_continuity,
        archive_report: &empty_archive,
        runtime_skill_report: &runtime,
        task_recall_report: None,
    });
    assert_eq!(procedural.intent, PromptRecallIntent::Procedural);

    let exact_lookup = RecallSelectionReport {
        query: RecallQuery {
            plane: RecallPlane::SharedFactual,
            exact_lookup: Some("preference:preferred_engineering_language".to_string()),
            ..RecallQuery::default()
        },
        ..shared.clone()
    };
    let factual = decide_prompt_recall_route(PromptRecallRouterInput {
        user_query: "slot preference:preferred_engineering_language",
        has_active_continuity: false,
        has_active_task_run: false,
        shared_factual_report: &exact_lookup,
        continuity_capsule_report: &empty_continuity,
        archive_report: &empty_archive,
        runtime_skill_report: &empty_report(RecallPlane::RuntimeSkill),
        task_recall_report: None,
    });
    assert_eq!(factual.intent, PromptRecallIntent::Factual);
}

#[test]
fn memory_harness_profile_contract() {
    let normal_mode = snapshot_from_source(RuntimeModeSource {
        wifi_sta_connected: true,
        pairing_state_known: true,
        channel_plane_alive: true,
        agent_plane_alive: true,
        ..RuntimeModeSource::default()
    });

    let esp_user = decide_prompt_assembly(
        MemorySystemKind::EspCompact,
        IngressKind::User,
        false,
        normal_mode,
        PressureLevel::Normal,
        8192,
    );
    assert!(esp_user.participation_plan.load_l1_constitutional);
    assert!(esp_user.participation_plan.load_l1_session);
    assert!(!esp_user.participation_plan.load_l2_governed_recall);
    assert!(!esp_user.participation_plan.load_l2_background_governance);
    assert!(!esp_user.participation_plan.load_l3_private_depth);

    let linux_system = decide_prompt_assembly(
        MemorySystemKind::LinuxFull,
        IngressKind::System,
        false,
        normal_mode,
        PressureLevel::Normal,
        8192,
    );
    assert!(linux_system.participation_plan.load_l2_governed_recall);
    assert!(
        linux_system
            .participation_plan
            .load_l2_background_governance
    );
}

#[test]
fn memory_harness_cross_entry_write_contract() {
    let stores = HarnessStores::default();
    let mut ctx = NullToolContext;
    let tool = MemoryManageTool::new(
        stores.memory.clone(),
        stores.long_term.clone(),
        stores.skills.clone(),
    );

    let manual_result = tool
        .execute(
            &json!({
                "op": "upsert_long_term",
                "kind": "preference",
                "topic": "preferred_engineering_language",
                "content": "User prefers Chinese for engineering review conversations.",
                "confidence": "high",
                "source_scope": "user"
            })
            .to_string(),
            &mut ctx,
        )
        .unwrap();
    let manual_json: serde_json::Value = serde_json::from_str(&manual_result).unwrap();
    assert_eq!(manual_json["plane"], "factual");
    assert_eq!(stores.long_term.count().unwrap(), 1);

    let extraction = ParsedLongTermMemoryExtraction {
        upserts: vec![factual_draft(
            LongTermMemoryKind::Preference,
            "preferred_engineering_language",
            "User prefers Chinese for engineering review conversations.",
            LongTermMemoryConfidence::High,
            NOW_SECS,
        )],
        deletes: Vec::new(),
        skill_writes: vec![RuntimeSkillWrite {
            name: "runtime_skill__release_checklist".to_string(),
            topic: "release_checklist".to_string(),
            title: "release checklist".to_string(),
            summary: "Run release checks before claiming readiness.".to_string(),
            content: "- run cargo test\n- run analyzer\n- verify release report".to_string(),
            citations: Vec::new(),
            source_chat_id: Some(CHAT_ID.to_string()),
            observed_at: NOW_SECS,
        }],
    };
    let changed = apply_long_term_memory_extraction(
        stores.long_term.as_ref(),
        stores.skills.as_ref(),
        &extraction,
        NOW_SECS,
    )
    .unwrap();
    assert!(changed >= 1);
    assert_eq!(stores.long_term.count().unwrap(), 1);
    assert!(stores
        .skills
        .names()
        .iter()
        .any(|name| name == "runtime_skill__release_checklist"));
}

#[test]
fn memory_harness_external_content_contract() {
    let user_fact = factual_draft(
        LongTermMemoryKind::Preference,
        "preferred_engineering_language",
        "User prefers Chinese for engineering review conversations.",
        LongTermMemoryConfidence::High,
        200,
    );
    let store = HarnessLongTermMemoryStore::with_entry(entry_from_draft(&user_fact, NOW_SECS));
    let external_conflict = LongTermMemoryDraft {
        content: "External observation says user prefers English engineering summaries."
            .to_string(),
        source_type: Some(LongTermMemorySourceType::ExternalObservation),
        source_scope: Some(LongTermMemorySourceScope::World),
        confidence: None,
        observed_at: Some(250),
        ..user_fact
    };

    let outcome = write_governed_shared_memory(
        &store,
        &[external_conflict],
        NOW_SECS,
        SharedMemoryWriteSource::Extraction,
    )
    .unwrap();
    assert_eq!(outcome.accepted, 0);
    assert_eq!(outcome.rejected, 1);
    assert_eq!(
        outcome.reports[0].reason,
        SharedMemoryWriteReason::LowerConfidenceThanExisting
    );
}

#[test]
fn memory_harness_prompt_inspection_parity() {
    let stores = HarnessStores::default();
    let query = "preferred engineering language Chinese release checklist";
    let draft = factual_draft(
        LongTermMemoryKind::Preference,
        "preferred_engineering_language",
        "User prefers Chinese for engineering review conversations.",
        LongTermMemoryConfidence::High,
        NOW_SECS,
    );
    stores
        .long_term
        .upsert_many(std::slice::from_ref(&draft), NOW_SECS)
        .unwrap();
    crate::skills::write_governed_runtime_skills(
        stores.skills.as_ref(),
        &[RuntimeSkillWrite {
            name: "runtime_skill__release_checklist".to_string(),
            topic: "release checklist".to_string(),
            title: "release checklist".to_string(),
            summary: "Run release checks before claiming readiness.".to_string(),
            content: "- validate diff\n- run cargo test\n- run analyzer\n- verify release report"
                .to_string(),
            citations: Vec::new(),
            source_chat_id: Some(CHAT_ID.to_string()),
            observed_at: NOW_SECS,
        }],
        RuntimeSkillWriteSource::Manual,
    )
    .unwrap();

    let participation_plan = PromptParticipationPlan {
        load_l1_constitutional: true,
        load_l1_session: true,
        load_l2_governed_recall: true,
        load_l2_background_governance: false,
        load_l3_private_depth: false,
    };
    let prompt = load_prompt_memory_context(stores.prompt_params(
        query,
        MemorySystemKind::LinuxFull,
        participation_plan,
    ));
    let inspection = inspect_working_recall(WorkingRecallInspectionInput {
        chat_id: CHAT_ID,
        query,
        summary_text: None,
        recent: &prompt.recent_messages,
        system_max_len: 1024,
        profile: MemoryProfile::Standard,
        current_channel: Some(CHANNEL),
        session_store: stores.session.as_ref(),
        memory_store: stores.memory.as_ref(),
        long_term_memory_store: stores.long_term.as_ref(),
        active_work_store: Some(&stores.active_work),
        continuity_capsule_store: &stores.continuity,
        turn_ledger_store: &stores.turn_ledger,
        skill_storage: Some(stores.skills.as_ref()),
        task_run_store: Some(&stores.task_runs),
        task_learning_store: Some(&stores.task_learning),
    });

    assert_report_surface_matches(
        &prompt.shared_factual_recall_report,
        &inspection.shared_factual_report,
    );
    assert_report_surface_matches(
        &prompt.continuity_capsule_report,
        &inspection.continuity_capsule_report,
    );
    assert_report_surface_matches(
        &prompt.archive_recall_report,
        &inspection.archive_recall_report,
    );
    assert_report_surface_matches(
        &prompt.runtime_skill_recall_report,
        &inspection.runtime_skill_report,
    );
    assert_eq!(
        prompt.runtime_skill_recall_report.selected_ids,
        vec!["runtime_skill__release_checklist".to_string()]
    );
    assert!(prompt
        .runtime_skill_text
        .as_deref()
        .unwrap_or_default()
        .contains("release checklist"));
    assert!(inspection
        .runtime_skill_text
        .as_deref()
        .unwrap_or_default()
        .contains("release checklist"));
    assert_eq!(
        prompt.into_runtime_carry().prompt_recall_intent,
        inspection.prompt_recall_intent
    );
}

#[test]
fn memory_harness_forget_scope_contract() {
    let stores = HarnessStores::default();
    stores.session.seed(
        CHAT_ID,
        vec![SessionMessage {
            role: "user".to_string(),
            content: "remember my engineering language preference".to_string(),
        }],
    );
    let fact = factual_draft(
        LongTermMemoryKind::Preference,
        "preferred_engineering_language",
        "User prefers Chinese for engineering review conversations.",
        LongTermMemoryConfidence::High,
        NOW_SECS,
    );
    stores
        .long_term
        .upsert_many(std::slice::from_ref(&fact), NOW_SECS)
        .unwrap();
    crate::skills::write_governed_runtime_skills(
        stores.skills.as_ref(),
        &[RuntimeSkillWrite {
            name: "runtime_skill__release_checklist".to_string(),
            topic: "release_checklist".to_string(),
            title: "release checklist".to_string(),
            summary: "Run release checks before claiming readiness.".to_string(),
            content: "- run cargo test\n- run analyzer\n- verify release report".to_string(),
            citations: Vec::new(),
            source_chat_id: Some(CHAT_ID.to_string()),
            observed_at: NOW_SECS,
        }],
        RuntimeSkillWriteSource::Manual,
    )
    .unwrap();

    let mut ctx = NullToolContext;
    let session_tool = SessionManageTool::new(stores.session.clone());
    session_tool
        .execute(
            &json!({"op": "delete", "chat_id": CHAT_ID}).to_string(),
            &mut ctx,
        )
        .unwrap();
    assert!(stores.session.load_recent(CHAT_ID, 10).unwrap().is_empty());
    assert_eq!(stores.long_term.count().unwrap(), 1);
    assert!(stores
        .skills
        .names()
        .iter()
        .any(|name| name == "runtime_skill__release_checklist"));

    let memory_tool = MemoryManageTool::new(
        stores.memory.clone(),
        stores.long_term.clone(),
        stores.skills.clone(),
    );
    let fact_id = fact.stable_id().unwrap();
    memory_tool
        .execute(
            &json!({"op": "delete_long_term", "id": fact_id}).to_string(),
            &mut ctx,
        )
        .unwrap();
    assert_eq!(stores.long_term.count().unwrap(), 0);
    assert!(stores
        .skills
        .names()
        .iter()
        .any(|name| name == "runtime_skill__release_checklist"));
}

#[test]
fn memory_harness_l2_production_replay_closes_refresh_and_recall() {
    let replay = run_memory_harness_l2_production_replay();

    assert_eq!(
        replay.extraction_request_outcome,
        LongTermMemoryRefreshRequestOutcome::Requested
    );
    assert!(replay.refresh_changed_count >= 2);
    assert!(replay.extraction_state_processed_clean);
    assert!(replay.prompt_shared_ids_before_refresh.is_empty());
    assert_eq!(
        replay.prompt_shared_ids_after_refresh,
        replay.inspection_shared_ids_after_refresh
    );
    assert_eq!(
        replay.prompt_runtime_skill_ids_after_refresh,
        replay.inspection_runtime_skill_ids_after_refresh
    );
    assert_eq!(
        replay.prompt_runtime_skill_ids_after_refresh,
        vec!["runtime_skill__release_checklist".to_string()]
    );
    assert!(!replay.prompt_shared_ids_after_refresh.is_empty());
    assert!(replay
        .stored_factual_content_after_refresh
        .contains("Chinese"));
    assert!(replay
        .inspection_runtime_skill_text_after_refresh
        .contains("release checklist"));
}
