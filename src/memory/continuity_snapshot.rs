//! Export/import of core continuity state for migration and bootstrap.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use super::{
    ExecutionState, ExecutionStateStore, LongTermMemoryDraft, LongTermMemoryEntry,
    LongTermMemoryKind, LongTermMemoryStore, SelfContinuity, SelfContinuityStore, SelfModel,
    SelfModelStore, SessionStore, SessionSummaryStore,
};

const CONTINUITY_SNAPSHOT_VERSION: u32 = 1;
const BOOTSTRAP_MAX_FACTS: usize = 16;
const FULL_RESTORE_MAX_FACTS: usize = 48;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContinuitySnapshotMode {
    Bootstrap,
    FullRestore,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContinuitySnapshotImportMode {
    BootstrapImport,
    FullRestore,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuitySnapshot {
    pub version: u32,
    pub exported_at: u64,
    pub mode: ContinuitySnapshotMode,
    pub chat_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub long_term_memory: Vec<LongTermMemoryEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_model: Option<SelfModel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_continuity: Option<SelfContinuity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_state: Option<ExecutionState>,
}

pub struct ContinuitySnapshotExportContext<'a> {
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
}

pub struct ContinuitySnapshotImportContext<'a> {
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuitySnapshotImportOutcome {
    pub long_term_imported: usize,
    pub summary_restored: bool,
    pub self_model_restored: bool,
    pub self_continuity_restored: bool,
    pub execution_state_restored: bool,
}

pub fn export_continuity_snapshot(
    ctx: ContinuitySnapshotExportContext<'_>,
    chat_id: &str,
    mode: ContinuitySnapshotMode,
    exported_at: u64,
) -> Result<ContinuitySnapshot> {
    let summary_text = ctx
        .session_summary_store
        .get_with_count(chat_id)?
        .map(|(summary, _)| summary)
        .filter(|summary| !summary.trim().is_empty());
    let self_model = ctx.self_model_store.get(chat_id)?;
    let self_continuity = ctx.self_continuity_store.get(chat_id)?;
    let execution_state = ctx.execution_state_store.get(chat_id)?;
    let long_term_memory = select_snapshot_long_term_memory(
        ctx.long_term_memory_store.list(FULL_RESTORE_MAX_FACTS)?,
        chat_id,
        mode,
    );
    Ok(ContinuitySnapshot {
        version: CONTINUITY_SNAPSHOT_VERSION,
        exported_at,
        mode,
        chat_id: chat_id.to_string(),
        summary_text,
        long_term_memory,
        self_model,
        self_continuity,
        execution_state: matches!(mode, ContinuitySnapshotMode::FullRestore)
            .then_some(execution_state)
            .flatten(),
    })
}

pub fn import_continuity_snapshot(
    ctx: ContinuitySnapshotImportContext<'_>,
    target_chat_id: &str,
    snapshot: &ContinuitySnapshot,
    mode: ContinuitySnapshotImportMode,
) -> Result<ContinuitySnapshotImportOutcome> {
    let selected = select_import_long_term_memory(snapshot, target_chat_id, mode);
    let drafts = selected
        .iter()
        .map(long_term_entry_to_draft)
        .collect::<Vec<_>>();
    let long_term_imported = if drafts.is_empty() {
        0
    } else {
        ctx.long_term_memory_store
            .upsert_many(&drafts, snapshot.exported_at)?
    };

    let mut outcome = ContinuitySnapshotImportOutcome {
        long_term_imported,
        ..ContinuitySnapshotImportOutcome::default()
    };
    if let Some(summary_text) = snapshot
        .summary_text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        ctx.session_summary_store
            .set(target_chat_id, summary_text)?;
        outcome.summary_restored = true;
    }
    if let Some(self_model) = snapshot.self_model.as_ref() {
        let should_restore = ctx
            .self_model_store
            .get(target_chat_id)?
            .is_none_or(|existing| existing.updated_at <= self_model.updated_at);
        if should_restore {
            ctx.self_model_store.set(target_chat_id, self_model)?;
            outcome.self_model_restored = true;
        }
    }
    if let Some(self_continuity) = snapshot.self_continuity.as_ref() {
        let should_restore = ctx
            .self_continuity_store
            .get(target_chat_id)?
            .is_none_or(|existing| existing.updated_at <= self_continuity.updated_at);
        if should_restore {
            ctx.self_continuity_store
                .set(target_chat_id, self_continuity)?;
            outcome.self_continuity_restored = true;
        }
    }
    if matches!(mode, ContinuitySnapshotImportMode::FullRestore) {
        if let Some(execution_state) = snapshot.execution_state.as_ref() {
            let should_restore = ctx
                .execution_state_store
                .get(target_chat_id)?
                .is_none_or(|existing| existing.updated_at <= execution_state.updated_at);
            if should_restore {
                ctx.execution_state_store
                    .set(target_chat_id, execution_state)?;
                outcome.execution_state_restored = true;
            }
        }
    }
    Ok(outcome)
}

pub fn select_active_continuity_snapshot_chat_ids(
    session_store: &dyn SessionStore,
    self_continuity_store: &dyn SelfContinuityStore,
    preferred_chat_id: Option<&str>,
    now_secs: u64,
    active_window_secs: u64,
    limit: usize,
) -> Vec<String> {
    let mut scored = session_store
        .list_chat_ids()
        .unwrap_or_default()
        .into_iter()
        .map(|chat_id| {
            let continuity = self_continuity_store.get(&chat_id).ok().flatten();
            let last_activity = continuity
                .as_ref()
                .map(|continuity| {
                    continuity
                        .last_user_turn_at
                        .max(continuity.last_autonomy_run_at)
                        .max(continuity.updated_at)
                })
                .unwrap_or(0);
            let preferred = preferred_chat_id == Some(chat_id.as_str());
            (chat_id, last_activity, preferred)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .2
            .cmp(&left.2)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.0.cmp(&right.0))
    });
    let limit = limit.max(1);
    let mut selected = Vec::with_capacity(limit);
    for (chat_id, last_activity, preferred) in scored {
        let active = preferred
            || last_activity == 0
            || now_secs == 0
            || now_secs.saturating_sub(last_activity) <= active_window_secs;
        if !active {
            continue;
        }
        if selected.iter().any(|existing| existing == &chat_id) {
            continue;
        }
        selected.push(chat_id);
        if selected.len() >= limit {
            break;
        }
    }
    if selected.is_empty() {
        if let Some(preferred_chat_id) = preferred_chat_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            selected.push(preferred_chat_id.to_string());
        }
    }
    selected
}

pub fn render_continuity_snapshot_markdown(snapshot: &ContinuitySnapshot) -> String {
    let mut out = String::with_capacity(2048);
    let _ = writeln!(out, "# Continuity Snapshot");
    let _ = writeln!(out, "- chat_id: {}", snapshot.chat_id);
    let _ = writeln!(out, "- mode: {:?}", snapshot.mode);
    let _ = writeln!(out, "- exported_at: {}", snapshot.exported_at);
    if let Some(summary) = snapshot.summary_text.as_deref() {
        let _ = writeln!(out, "\n## Session Summary\n{}", summary.trim());
    }
    if let Some(self_model) = snapshot.self_model.as_ref() {
        let _ = writeln!(out, "\n## Self Model");
        if !self_model.continuity_anchor.trim().is_empty() {
            let _ = writeln!(out, "- anchor: {}", self_model.continuity_anchor.trim());
        }
        if !self_model.self_narrative.trim().is_empty() {
            let _ = writeln!(out, "- narrative: {}", self_model.self_narrative.trim());
        }
        if !self_model.relationship_state.trim().is_empty() {
            let _ = writeln!(
                out,
                "- relationship: {}",
                self_model.relationship_state.trim()
            );
        }
    }
    if let Some(self_continuity) = snapshot.self_continuity.as_ref() {
        let _ = writeln!(out, "\n## Self Continuity");
        if !self_continuity.wake_anchor.trim().is_empty() {
            let _ = writeln!(out, "- wake_anchor: {}", self_continuity.wake_anchor.trim());
        }
        if !self_continuity.current_self_state.trim().is_empty() {
            let _ = writeln!(
                out,
                "- current_self_state: {}",
                self_continuity.current_self_state.trim()
            );
        }
        if !self_continuity.continuity_bridge.trim().is_empty() {
            let _ = writeln!(
                out,
                "- continuity_bridge: {}",
                self_continuity.continuity_bridge.trim()
            );
        }
    }
    if !snapshot.long_term_memory.is_empty() {
        let _ = writeln!(out, "\n## Shared Facts");
        for entry in &snapshot.long_term_memory {
            let _ = writeln!(
                out,
                "- [{}:{}] {}",
                entry.kind.label(),
                entry.topic,
                entry.content
            );
        }
    }
    if let Some(execution_state) = snapshot.execution_state.as_ref() {
        let _ = writeln!(out, "\n## Execution State");
        if !execution_state.goal.trim().is_empty() {
            let _ = writeln!(out, "- goal: {}", execution_state.goal.trim());
        }
        if !execution_state.progress.trim().is_empty() {
            let _ = writeln!(out, "- progress: {}", execution_state.progress.trim());
        }
        if !execution_state.next_action.trim().is_empty() {
            let _ = writeln!(out, "- next_action: {}", execution_state.next_action.trim());
        }
    }
    out.trim_end().to_string()
}

fn select_snapshot_long_term_memory(
    mut entries: Vec<LongTermMemoryEntry>,
    chat_id: &str,
    mode: ContinuitySnapshotMode,
) -> Vec<LongTermMemoryEntry> {
    entries.retain(|entry| {
        entry
            .source_chat_id
            .as_deref()
            .is_none_or(|source_chat_id| source_chat_id == chat_id)
            || !matches!(entry.source_scope, super::LongTermMemorySourceScope::Chat)
    });
    entries.sort_by(|a, b| {
        snapshot_kind_priority(&b.kind)
            .cmp(&snapshot_kind_priority(&a.kind))
            .then_with(|| b.evidence_count.cmp(&a.evidence_count))
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| a.topic.cmp(&b.topic))
    });
    let limit = match mode {
        ContinuitySnapshotMode::Bootstrap => BOOTSTRAP_MAX_FACTS,
        ContinuitySnapshotMode::FullRestore => FULL_RESTORE_MAX_FACTS,
    };
    if matches!(mode, ContinuitySnapshotMode::Bootstrap) {
        entries.retain(|entry| {
            matches!(
                entry.kind,
                LongTermMemoryKind::Profile
                    | LongTermMemoryKind::Relationship
                    | LongTermMemoryKind::Preference
                    | LongTermMemoryKind::Constraint
                    | LongTermMemoryKind::Project
            )
        });
    }
    entries.truncate(limit);
    entries
}

fn select_import_long_term_memory(
    snapshot: &ContinuitySnapshot,
    target_chat_id: &str,
    mode: ContinuitySnapshotImportMode,
) -> Vec<LongTermMemoryEntry> {
    let mut entries = snapshot.long_term_memory.clone();
    if matches!(mode, ContinuitySnapshotImportMode::BootstrapImport) {
        entries.retain(|entry| {
            matches!(
                entry.kind,
                LongTermMemoryKind::Profile
                    | LongTermMemoryKind::Relationship
                    | LongTermMemoryKind::Preference
                    | LongTermMemoryKind::Constraint
                    | LongTermMemoryKind::Project
            )
        });
    }
    for entry in &mut entries {
        if matches!(entry.source_scope, super::LongTermMemorySourceScope::Chat) {
            entry.source_chat_id = Some(target_chat_id.to_string());
        }
    }
    entries
}

fn long_term_entry_to_draft(entry: &LongTermMemoryEntry) -> LongTermMemoryDraft {
    LongTermMemoryDraft {
        kind: entry.kind.clone(),
        topic: entry.topic.clone(),
        content: entry.content.clone(),
        keywords: entry.keywords.clone(),
        source_chat_id: entry.source_chat_id.clone(),
        source_type: Some(entry.source_type),
        source_scope: Some(entry.source_scope),
        confidence: Some(entry.confidence),
        freshness: Some(entry.freshness),
        stale_hint: Some(entry.stale_hint),
        supporting_citations: entry.supporting_citations.clone(),
        evidence_count: Some(entry.evidence_count),
        observed_at: Some(entry.observed_at),
        last_confirmed_at: Some(entry.last_confirmed_at),
        source_revision: Some(entry.source_revision),
    }
}

fn snapshot_kind_priority(kind: &LongTermMemoryKind) -> u8 {
    match kind {
        LongTermMemoryKind::Relationship => 6,
        LongTermMemoryKind::Profile => 5,
        LongTermMemoryKind::Preference => 4,
        LongTermMemoryKind::Constraint => 3,
        LongTermMemoryKind::Project => 2,
        LongTermMemoryKind::Fact => 1,
        LongTermMemoryKind::Task => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        ExecutionStatus, LongTermMemoryConfidence, LongTermMemoryFreshness,
        LongTermMemorySourceScope, LongTermMemorySourceType, LongTermMemoryStaleHint,
        SessionMessage,
    };
    use std::sync::Mutex;

    fn sample_entry(kind: LongTermMemoryKind, topic: &str) -> LongTermMemoryEntry {
        LongTermMemoryEntry {
            id: format!("{}:{}", kind.label(), topic),
            kind,
            topic: topic.to_string(),
            content: format!("content for {}", topic),
            keywords: vec![topic.to_string()],
            source_chat_id: Some("chat-1".to_string()),
            source_type: LongTermMemorySourceType::Conversation,
            source_scope: LongTermMemorySourceScope::Chat,
            confidence: LongTermMemoryConfidence::High,
            freshness: LongTermMemoryFreshness::Stable,
            stale_hint: LongTermMemoryStaleHint::None,
            supporting_citations: vec!["transcript:chat-1#message=1".to_string()],
            evidence_count: 1,
            created_at: 1,
            updated_at: 2,
            observed_at: 2,
            last_confirmed_at: 2,
            source_revision: 1,
            last_used_at: 0,
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryStore {
        entries: Vec<LongTermMemoryEntry>,
        imported: Mutex<Vec<LongTermMemoryDraft>>,
    }

    impl LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(&self, drafts: &[LongTermMemoryDraft], _now_secs: u64) -> Result<usize> {
            self.imported
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend_from_slice(drafts);
            Ok(drafts.len())
        }

        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn get(&self, _id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(None)
        }

        fn list(&self, _limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self.entries.clone())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &super::super::LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(self.entries.len())
        }
    }

    #[derive(Default)]
    struct StubSummaryStore {
        value: Mutex<Option<(String, usize)>>,
    }

    impl SessionSummaryStore for StubSummaryStore {
        fn get(&self, _chat_id: &str) -> Result<Option<String>> {
            Ok(self
                .value
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .map(|(summary, _)| summary.clone()))
        }

        fn set(&self, _chat_id: &str, summary: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some((summary.to_string(), 1));
            Ok(())
        }

        fn get_with_count(&self, _chat_id: &str) -> Result<Option<(String, usize)>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }
    }

    #[derive(Default)]
    struct StubExecutionStateStore {
        state: Mutex<Option<ExecutionState>>,
    }

    impl ExecutionStateStore for StubExecutionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<ExecutionState>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, state: &ExecutionState) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfModelStore {
        state: Mutex<Option<SelfModel>>,
    }

    impl SelfModelStore for StubSelfModelStore {
        fn get(&self, _chat_id: &str) -> Result<Option<SelfModel>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, model: &SelfModel) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(model.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfContinuityStore {
        state: Mutex<Option<SelfContinuity>>,
    }

    impl SelfContinuityStore for StubSelfContinuityStore {
        fn get(&self, _chat_id: &str) -> Result<Option<SelfContinuity>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, continuity: &SelfContinuity) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(continuity.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    struct StubSessionStore {
        chat_ids: Vec<String>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, _n: usize) -> Result<Vec<SessionMessage>> {
            Ok(Vec::new())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(self.chat_ids.clone())
        }
    }

    struct MultiSelfContinuityStore {
        entries: std::collections::HashMap<String, SelfContinuity>,
    }

    impl SelfContinuityStore for MultiSelfContinuityStore {
        fn get(&self, chat_id: &str) -> Result<Option<SelfContinuity>> {
            Ok(self.entries.get(chat_id).cloned())
        }

        fn set(&self, _chat_id: &str, _continuity: &SelfContinuity) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn bootstrap_snapshot_filters_to_core_fact_kinds() {
        let store = StubLongTermMemoryStore {
            entries: vec![
                sample_entry(LongTermMemoryKind::Profile, "owner_profile"),
                sample_entry(LongTermMemoryKind::Relationship, "owner_relation"),
                sample_entry(LongTermMemoryKind::Task, "task"),
            ],
            ..Default::default()
        };
        let summary = StubSummaryStore::default();
        *summary.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(("summary".to_string(), 3));
        let snapshot = export_continuity_snapshot(
            ContinuitySnapshotExportContext {
                long_term_memory_store: &store,
                session_summary_store: &summary,
                execution_state_store: &StubExecutionStateStore::default(),
                self_model_store: &StubSelfModelStore::default(),
                self_continuity_store: &StubSelfContinuityStore::default(),
            },
            "chat-1",
            ContinuitySnapshotMode::Bootstrap,
            10,
        )
        .unwrap();
        assert_eq!(snapshot.long_term_memory.len(), 2);
        assert!(snapshot
            .long_term_memory
            .iter()
            .all(|entry| entry.kind != LongTermMemoryKind::Task));
    }

    #[test]
    fn full_restore_import_restores_execution_state() {
        let store = StubLongTermMemoryStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let self_model_store = StubSelfModelStore::default();
        let self_continuity_store = StubSelfContinuityStore::default();
        let snapshot = ContinuitySnapshot {
            version: CONTINUITY_SNAPSHOT_VERSION,
            exported_at: 11,
            mode: ContinuitySnapshotMode::FullRestore,
            chat_id: "chat-1".to_string(),
            summary_text: None,
            long_term_memory: vec![sample_entry(LongTermMemoryKind::Profile, "owner_profile")],
            self_model: Some(SelfModel {
                continuity_anchor: "same line".to_string(),
                self_narrative: String::new(),
                relationship_state: String::new(),
                private_notes: String::new(),
                updated_at: 11,
            }),
            self_continuity: Some(SelfContinuity {
                wake_anchor: "same wake".to_string(),
                current_self_state: String::new(),
                recent_changes: String::new(),
                continuity_bridge: String::new(),
                priority_posture: String::new(),
                relationship_posture: String::new(),
                task_posture: String::new(),
                last_user_turn_at: 11,
                last_user_channel: "qq_channel".to_string(),
                last_autonomy_run_at: 0,
                updated_at: 11,
            }),
            execution_state: Some(ExecutionState {
                status: ExecutionStatus::Active,
                goal: "finish migration".to_string(),
                progress: String::new(),
                blocker: String::new(),
                next_action: "boot on new device".to_string(),
                last_output: String::new(),
                updated_at: 11,
            }),
        };
        let outcome = import_continuity_snapshot(
            ContinuitySnapshotImportContext {
                long_term_memory_store: &store,
                session_summary_store: &StubSummaryStore::default(),
                execution_state_store: &execution_state_store,
                self_model_store: &self_model_store,
                self_continuity_store: &self_continuity_store,
            },
            "chat-new",
            &snapshot,
            ContinuitySnapshotImportMode::FullRestore,
        )
        .unwrap();
        assert_eq!(outcome.long_term_imported, 1);
        assert!(outcome.self_model_restored);
        assert!(outcome.self_continuity_restored);
        assert!(outcome.execution_state_restored);
    }

    #[test]
    fn full_restore_import_restores_summary_text() {
        let summary_store = StubSummaryStore::default();
        let outcome = import_continuity_snapshot(
            ContinuitySnapshotImportContext {
                long_term_memory_store: &StubLongTermMemoryStore::default(),
                session_summary_store: &summary_store,
                execution_state_store: &StubExecutionStateStore::default(),
                self_model_store: &StubSelfModelStore::default(),
                self_continuity_store: &StubSelfContinuityStore::default(),
            },
            "chat-new",
            &ContinuitySnapshot {
                version: CONTINUITY_SNAPSHOT_VERSION,
                exported_at: 20,
                mode: ContinuitySnapshotMode::Bootstrap,
                chat_id: "chat-old".to_string(),
                summary_text: Some("stable summary".to_string()),
                long_term_memory: Vec::new(),
                self_model: None,
                self_continuity: None,
                execution_state: None,
            },
            ContinuitySnapshotImportMode::BootstrapImport,
        )
        .unwrap();
        assert!(outcome.summary_restored);
        assert_eq!(
            summary_store
                .get_with_count("chat-new")
                .unwrap()
                .map(|(value, _)| value),
            Some("stable summary".to_string())
        );
    }

    #[test]
    fn select_active_chat_ids_prefers_preferred_and_recent_activity() {
        let session_store = StubSessionStore {
            chat_ids: vec![
                "chat-stale".to_string(),
                "chat-recent".to_string(),
                "chat-preferred".to_string(),
            ],
        };
        let continuity_store = MultiSelfContinuityStore {
            entries: [
                (
                    "chat-stale".to_string(),
                    SelfContinuity {
                        wake_anchor: String::new(),
                        current_self_state: String::new(),
                        recent_changes: String::new(),
                        continuity_bridge: String::new(),
                        priority_posture: String::new(),
                        relationship_posture: String::new(),
                        task_posture: String::new(),
                        last_user_turn_at: 10,
                        last_user_channel: "qq_channel".to_string(),
                        last_autonomy_run_at: 10,
                        updated_at: 10,
                    },
                ),
                (
                    "chat-recent".to_string(),
                    SelfContinuity {
                        wake_anchor: String::new(),
                        current_self_state: String::new(),
                        recent_changes: String::new(),
                        continuity_bridge: String::new(),
                        priority_posture: String::new(),
                        relationship_posture: String::new(),
                        task_posture: String::new(),
                        last_user_turn_at: 990,
                        last_user_channel: "qq_channel".to_string(),
                        last_autonomy_run_at: 995,
                        updated_at: 995,
                    },
                ),
                (
                    "chat-preferred".to_string(),
                    SelfContinuity {
                        wake_anchor: String::new(),
                        current_self_state: String::new(),
                        recent_changes: String::new(),
                        continuity_bridge: String::new(),
                        priority_posture: String::new(),
                        relationship_posture: String::new(),
                        task_posture: String::new(),
                        last_user_turn_at: 100,
                        last_user_channel: "telegram".to_string(),
                        last_autonomy_run_at: 100,
                        updated_at: 100,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        };
        let selected = select_active_continuity_snapshot_chat_ids(
            &session_store,
            &continuity_store,
            Some("chat-preferred"),
            1_000,
            120,
            4,
        );
        assert_eq!(
            selected,
            vec!["chat-preferred".to_string(), "chat-recent".to_string()]
        );
    }
}
