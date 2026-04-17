//! Export/import of core continuity state for migration and bootstrap.
#![allow(clippy::too_many_arguments)]

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::DefaultHasher, HashSet};
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};

use super::{
    board_subject_scope_id, select_relationship_portfolio_targets,
    select_relationship_topology_targets, write_governed_shared_memory, CoreRevisionLedger,
    CoreRevisionLedgerStore, ExecutionState, ExecutionStateStore, LongTermMemoryDraft,
    LongTermMemoryEntry, LongTermMemoryKind, LongTermMemoryStore, RelationshipConstitution,
    RelationshipConstitutionStore, RelationshipPortfolio, RelationshipPortfolioSelectorInput,
    RelationshipPortfolioStore, RelationshipSelectionTarget, RelationshipSelectorInput,
    RelationshipTopology, RelationshipTopologyStore, SelfAuthoredCore, SelfAuthoredCoreStore,
    SelfContinuity, SelfContinuityStore, SelfModel, SelfModelStore, SessionStore,
    SessionSummaryStore, SharedMemoryWriteOutcome, SharedMemoryWriteSource,
};

const CONTINUITY_SNAPSHOT_VERSION: u32 = 5;
const BOOTSTRAP_MAX_FACTS: usize = 16;
const FULL_RESTORE_MAX_FACTS: usize = 48;
const PERSONALITY_GOVERNANCE_ACTIVE_WINDOW_SECS: u64 = 7 * 86_400;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
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

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuitySnapshotManifest {
    #[serde(default)]
    pub content_fingerprint: String,
    #[serde(default)]
    pub long_term_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub long_term_kind_counts: Vec<ContinuitySnapshotKindCount>,
    #[serde(default)]
    pub includes_summary: bool,
    #[serde(default)]
    pub includes_self_model: bool,
    #[serde(default)]
    pub includes_self_authored_core: bool,
    #[serde(default)]
    pub includes_core_revision_ledger: bool,
    #[serde(default)]
    pub includes_self_continuity: bool,
    #[serde(default)]
    pub includes_relationship_portfolio: bool,
    #[serde(default)]
    pub includes_relationship_constitution: bool,
    #[serde(default)]
    pub includes_execution_state: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship_scope_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuitySnapshotKindCount {
    pub kind: LongTermMemoryKind,
    pub count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuitySnapshot {
    pub version: u32,
    pub exported_at: u64,
    pub mode: ContinuitySnapshotMode,
    pub chat_id: String,
    #[serde(default)]
    pub subject_id: String,
    #[serde(default)]
    pub manifest: ContinuitySnapshotManifest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_message_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub long_term_memory: Vec<LongTermMemoryEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_model: Option<SelfModel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_authored_core: Option<SelfAuthoredCore>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_revision_ledger: Option<CoreRevisionLedger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_continuity: Option<SelfContinuity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship_portfolio: Option<RelationshipPortfolio>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship_constitution: Option<RelationshipConstitution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_state: Option<ExecutionState>,
}

pub struct ContinuitySnapshotExportContext<'a> {
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub self_authored_core_store: &'a dyn SelfAuthoredCoreStore,
    pub core_revision_ledger_store: &'a dyn CoreRevisionLedgerStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub relationship_constitution_store: &'a dyn RelationshipConstitutionStore,
    pub relationship_portfolio_store: &'a dyn RelationshipPortfolioStore,
    pub relationship_topology_store: &'a dyn RelationshipTopologyStore,
}

pub struct ContinuitySnapshotImportContext<'a> {
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub self_authored_core_store: &'a dyn SelfAuthoredCoreStore,
    pub core_revision_ledger_store: &'a dyn CoreRevisionLedgerStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub relationship_constitution_store: &'a dyn RelationshipConstitutionStore,
    pub relationship_portfolio_store: &'a dyn RelationshipPortfolioStore,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuitySnapshotImportOutcome {
    pub long_term_imported: usize,
    #[serde(default)]
    pub manifest: ContinuitySnapshotManifest,
    pub summary_restored: bool,
    pub self_model_restored: bool,
    pub self_authored_core_restored: bool,
    pub core_revision_ledger_restored: bool,
    pub self_continuity_restored: bool,
    pub relationship_constitution_restored: bool,
    pub relationship_portfolio_restored: bool,
    pub execution_state_restored: bool,
    #[serde(default)]
    pub long_term_write_outcome: SharedMemoryWriteOutcome,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<ContinuitySnapshotImportDecision>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuitySnapshotImportDecision {
    pub layer: String,
    pub action: String,
    pub reason: String,
}

pub fn export_continuity_snapshot(
    ctx: ContinuitySnapshotExportContext<'_>,
    chat_id: &str,
    mode: ContinuitySnapshotMode,
    exported_at: u64,
) -> Result<ContinuitySnapshot> {
    let subject_id = board_subject_scope_id();
    let (summary_text, summary_message_count) = ctx
        .session_summary_store
        .get_with_count(chat_id)?
        .map_or((None, None), |(summary, count)| {
            let summary = (!summary.trim().is_empty()).then_some(summary);
            let count = summary.as_ref().map(|_| count);
            (summary, count)
        });
    let self_model = ctx.self_model_store.get(subject_id)?;
    let self_authored_core = ctx.self_authored_core_store.get(subject_id)?;
    let core_revision_ledger = ctx.core_revision_ledger_store.get(subject_id)?;
    let self_continuity = ctx.self_continuity_store.get(subject_id)?;
    let relationship_portfolio = ctx.relationship_portfolio_store.get(subject_id)?;
    let relationship_topology = ctx.relationship_topology_store.get(subject_id)?;
    let relationship_scope_id = select_snapshot_relationship_scope_id(
        chat_id,
        self_continuity.as_ref(),
        relationship_portfolio.as_ref(),
        relationship_topology.as_ref(),
    );
    let relationship_constitution = relationship_scope_id
        .as_deref()
        .map(|scope_id| ctx.relationship_constitution_store.get(scope_id))
        .transpose()?
        .flatten();
    let execution_state = ctx.execution_state_store.get(chat_id)?;
    let long_term_memory = select_snapshot_long_term_memory(
        ctx.long_term_memory_store.list(FULL_RESTORE_MAX_FACTS)?,
        chat_id,
        mode,
    );
    let mut snapshot = ContinuitySnapshot {
        version: CONTINUITY_SNAPSHOT_VERSION,
        exported_at,
        mode,
        chat_id: chat_id.to_string(),
        subject_id: subject_id.to_string(),
        manifest: ContinuitySnapshotManifest::default(),
        summary_text,
        summary_message_count,
        long_term_memory,
        self_model,
        self_authored_core,
        core_revision_ledger,
        self_continuity,
        relationship_constitution,
        relationship_portfolio,
        execution_state: matches!(mode, ContinuitySnapshotMode::FullRestore)
            .then_some(execution_state)
            .flatten(),
    };
    snapshot.manifest = build_snapshot_manifest(&snapshot, relationship_scope_id.as_deref());
    Ok(snapshot)
}

pub fn import_continuity_snapshot(
    ctx: ContinuitySnapshotImportContext<'_>,
    target_chat_id: &str,
    snapshot: &ContinuitySnapshot,
    mode: ContinuitySnapshotImportMode,
) -> Result<ContinuitySnapshotImportOutcome> {
    let manifest = snapshot_manifest(snapshot);
    let target_subject_id = if snapshot.subject_id.trim().is_empty() {
        board_subject_scope_id()
    } else {
        snapshot.subject_id.trim()
    };
    let selected = select_import_long_term_memory(snapshot, target_chat_id, mode);
    let drafts = selected
        .iter()
        .map(long_term_entry_to_draft)
        .collect::<Vec<_>>();
    let long_term_write_outcome = if drafts.is_empty() {
        SharedMemoryWriteOutcome {
            source: SharedMemoryWriteSource::SnapshotImport,
            ..SharedMemoryWriteOutcome::default()
        }
    } else {
        write_governed_shared_memory(
            ctx.long_term_memory_store,
            &drafts,
            snapshot.exported_at,
            SharedMemoryWriteSource::SnapshotImport,
        )?
    };

    let mut outcome = ContinuitySnapshotImportOutcome {
        long_term_imported: long_term_write_outcome.changed,
        manifest,
        long_term_write_outcome,
        ..ContinuitySnapshotImportOutcome::default()
    };
    if outcome.long_term_write_outcome.submitted > 0 {
        outcome.decisions.push(ContinuitySnapshotImportDecision {
            layer: "long_term_memory".to_string(),
            action: if outcome.long_term_imported > 0 {
                "restored".to_string()
            } else if outcome.long_term_write_outcome.rejected > 0 {
                "partially_rejected".to_string()
            } else {
                "accepted_without_change".to_string()
            },
            reason: format!(
                "accepted={}, rejected={}, changed={}",
                outcome.long_term_write_outcome.accepted,
                outcome.long_term_write_outcome.rejected,
                outcome.long_term_write_outcome.changed
            ),
        });
    }
    if let Some(summary_text) = snapshot
        .summary_text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let local_summary = ctx.session_summary_store.get_with_count(target_chat_id)?;
        let should_restore = match (snapshot.summary_message_count, local_summary.as_ref()) {
            (_, None) => true,
            (Some(snapshot_count), Some((_, local_count))) => snapshot_count >= *local_count,
            (None, Some(_)) => false,
        };
        if should_restore {
            ctx.session_summary_store.set_with_count(
                target_chat_id,
                summary_text,
                snapshot.summary_message_count.unwrap_or(0),
            )?;
            outcome.summary_restored = true;
            outcome.decisions.push(import_decision(
                "session_summary",
                "restored",
                "snapshot summary is newer than local state or local state was missing",
            ));
        } else {
            outcome.decisions.push(import_decision(
                "session_summary",
                "skipped",
                "local session summary is newer than the snapshot",
            ));
        }
    } else {
        outcome.decisions.push(import_decision(
            "session_summary",
            "skipped",
            "snapshot did not include a non-empty summary",
        ));
    }
    if let Some(self_model) = snapshot.self_model.as_ref() {
        let should_restore = ctx
            .self_model_store
            .get(target_subject_id)?
            .is_none_or(|existing| existing.updated_at <= self_model.updated_at);
        if should_restore {
            ctx.self_model_store.set(target_subject_id, self_model)?;
            outcome.self_model_restored = true;
            outcome.decisions.push(import_decision(
                "self_model",
                "restored",
                "snapshot self_model is newer than local state or local state was missing",
            ));
        } else {
            outcome.decisions.push(import_decision(
                "self_model",
                "skipped",
                "local self_model is newer than the snapshot",
            ));
        }
    } else {
        outcome.decisions.push(import_decision(
            "self_model",
            "skipped",
            "snapshot did not include self_model",
        ));
    }
    if let Some(self_authored_core) = snapshot.self_authored_core.as_ref() {
        let should_restore = ctx
            .self_authored_core_store
            .get(target_subject_id)?
            .is_none_or(|existing| existing.updated_at <= self_authored_core.updated_at);
        if should_restore {
            ctx.self_authored_core_store
                .set(target_subject_id, self_authored_core)?;
            outcome.self_authored_core_restored = true;
            outcome.decisions.push(import_decision(
                "self_authored_core",
                "restored",
                "snapshot self_authored_core is newer than local state or local state was missing",
            ));
        } else {
            outcome.decisions.push(import_decision(
                "self_authored_core",
                "skipped",
                "local self_authored_core is newer than the snapshot",
            ));
        }
    } else {
        outcome.decisions.push(import_decision(
            "self_authored_core",
            "skipped",
            "snapshot did not include self_authored_core",
        ));
    }
    if let Some(core_revision_ledger) = snapshot.core_revision_ledger.as_ref() {
        let should_restore = ctx
            .core_revision_ledger_store
            .get(target_subject_id)?
            .is_none_or(|existing| existing.updated_at <= core_revision_ledger.updated_at);
        if should_restore {
            ctx.core_revision_ledger_store
                .set(target_subject_id, core_revision_ledger)?;
            outcome.core_revision_ledger_restored = true;
            outcome.decisions.push(import_decision(
                "core_revision_ledger",
                "restored",
                "snapshot core revision ledger is newer than local state or local state was missing",
            ));
        } else {
            outcome.decisions.push(import_decision(
                "core_revision_ledger",
                "skipped",
                "local core revision ledger is newer than the snapshot",
            ));
        }
    } else {
        outcome.decisions.push(import_decision(
            "core_revision_ledger",
            "skipped",
            "snapshot did not include core revision ledger",
        ));
    }
    if let Some(self_continuity) = snapshot.self_continuity.as_ref() {
        let should_restore = ctx
            .self_continuity_store
            .get(target_subject_id)?
            .is_none_or(|existing| existing.updated_at <= self_continuity.updated_at);
        if should_restore {
            ctx.self_continuity_store
                .set(target_subject_id, self_continuity)?;
            outcome.self_continuity_restored = true;
            outcome.decisions.push(import_decision(
                "self_continuity",
                "restored",
                "snapshot self_continuity is newer than local state or local state was missing",
            ));
        } else {
            outcome.decisions.push(import_decision(
                "self_continuity",
                "skipped",
                "local self_continuity is newer than the snapshot",
            ));
        }
    } else {
        outcome.decisions.push(import_decision(
            "self_continuity",
            "skipped",
            "snapshot did not include self_continuity",
        ));
    }
    if let Some(relationship_constitution) = snapshot.relationship_constitution.as_ref() {
        let should_restore = ctx
            .relationship_constitution_store
            .get(relationship_constitution.scope_id.as_str())?
            .is_none_or(|existing| existing.updated_at <= relationship_constitution.updated_at);
        if should_restore {
            ctx.relationship_constitution_store.set(
                relationship_constitution.scope_id.as_str(),
                relationship_constitution,
            )?;
            outcome.relationship_constitution_restored = true;
            outcome.decisions.push(import_decision(
                "relationship_constitution",
                "restored",
                "snapshot relationship constitution is newer than local state or local state was missing",
            ));
        } else {
            outcome.decisions.push(import_decision(
                "relationship_constitution",
                "skipped",
                "local relationship constitution is newer than the snapshot",
            ));
        }
    } else {
        outcome.decisions.push(import_decision(
            "relationship_constitution",
            "skipped",
            "snapshot did not include relationship constitution",
        ));
    }
    if let Some(relationship_portfolio) = snapshot.relationship_portfolio.as_ref() {
        let should_restore = ctx
            .relationship_portfolio_store
            .get(target_subject_id)?
            .is_none_or(|existing| existing.updated_at <= relationship_portfolio.updated_at);
        if should_restore {
            ctx.relationship_portfolio_store
                .set(target_subject_id, relationship_portfolio)?;
            outcome.relationship_portfolio_restored = true;
            outcome.decisions.push(import_decision(
                "relationship_portfolio",
                "restored",
                "snapshot relationship portfolio is newer than local state or local state was missing",
            ));
        } else {
            outcome.decisions.push(import_decision(
                "relationship_portfolio",
                "skipped",
                "local relationship portfolio is newer than the snapshot",
            ));
        }
    } else {
        outcome.decisions.push(import_decision(
            "relationship_portfolio",
            "skipped",
            "snapshot did not include relationship portfolio",
        ));
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
                outcome.decisions.push(import_decision(
                    "execution_state",
                    "restored",
                    "snapshot execution state is newer than local state or local state was missing",
                ));
            } else {
                outcome.decisions.push(import_decision(
                    "execution_state",
                    "skipped",
                    "local execution state is newer than the snapshot",
                ));
            }
        } else {
            outcome.decisions.push(import_decision(
                "execution_state",
                "skipped",
                "snapshot did not include execution_state",
            ));
        }
    } else {
        outcome.decisions.push(import_decision(
            "execution_state",
            "skipped",
            "bootstrap_import does not restore execution_state",
        ));
    }
    Ok(outcome)
}

fn import_decision(layer: &str, action: &str, reason: &str) -> ContinuitySnapshotImportDecision {
    ContinuitySnapshotImportDecision {
        layer: layer.to_string(),
        action: action.to_string(),
        reason: reason.to_string(),
    }
}

fn snapshot_manifest(snapshot: &ContinuitySnapshot) -> ContinuitySnapshotManifest {
    if !snapshot.manifest.content_fingerprint.trim().is_empty() {
        snapshot.manifest.clone()
    } else {
        build_snapshot_manifest(
            snapshot,
            snapshot
                .relationship_constitution
                .as_ref()
                .map(|constitution| constitution.scope_id.as_str()),
        )
    }
}

fn build_snapshot_manifest(
    snapshot: &ContinuitySnapshot,
    relationship_scope_id: Option<&str>,
) -> ContinuitySnapshotManifest {
    let mut manifest = ContinuitySnapshotManifest {
        long_term_count: snapshot.long_term_memory.len(),
        long_term_kind_counts: collect_snapshot_kind_counts(&snapshot.long_term_memory),
        includes_summary: snapshot
            .summary_text
            .as_deref()
            .is_some_and(|summary| !summary.trim().is_empty()),
        includes_self_model: snapshot.self_model.is_some(),
        includes_self_authored_core: snapshot.self_authored_core.is_some(),
        includes_core_revision_ledger: snapshot.core_revision_ledger.is_some(),
        includes_self_continuity: snapshot.self_continuity.is_some(),
        includes_relationship_portfolio: snapshot.relationship_portfolio.is_some(),
        includes_relationship_constitution: snapshot.relationship_constitution.is_some(),
        includes_execution_state: snapshot.execution_state.is_some(),
        relationship_scope_id: relationship_scope_id.map(str::to_string),
        ..ContinuitySnapshotManifest::default()
    };
    manifest.content_fingerprint = snapshot_content_fingerprint(snapshot, &manifest);
    manifest
}

fn collect_snapshot_kind_counts(
    entries: &[LongTermMemoryEntry],
) -> Vec<ContinuitySnapshotKindCount> {
    let mut counts = Vec::new();
    for kind in [
        LongTermMemoryKind::Relationship,
        LongTermMemoryKind::Profile,
        LongTermMemoryKind::Preference,
        LongTermMemoryKind::Constraint,
        LongTermMemoryKind::Project,
        LongTermMemoryKind::Fact,
        LongTermMemoryKind::Task,
    ] {
        let count = entries.iter().filter(|entry| entry.kind == kind).count();
        if count == 0 {
            continue;
        }
        counts.push(ContinuitySnapshotKindCount { kind, count });
    }
    counts
}

fn snapshot_content_fingerprint(
    snapshot: &ContinuitySnapshot,
    manifest: &ContinuitySnapshotManifest,
) -> String {
    let mut hasher = DefaultHasher::new();
    snapshot.version.hash(&mut hasher);
    snapshot.mode.hash(&mut hasher);
    snapshot.chat_id.hash(&mut hasher);
    snapshot.subject_id.hash(&mut hasher);
    snapshot.long_term_memory.len().hash(&mut hasher);
    manifest.long_term_count.hash(&mut hasher);
    manifest
        .relationship_scope_id
        .as_deref()
        .unwrap_or_default()
        .hash(&mut hasher);
    for count in &manifest.long_term_kind_counts {
        count.kind.hash(&mut hasher);
        count.count.hash(&mut hasher);
    }
    if let Some(summary) = snapshot.summary_text.as_deref() {
        summary.trim().hash(&mut hasher);
    }
    if let Some(self_model) = snapshot.self_model.as_ref() {
        self_model.updated_at.hash(&mut hasher);
        self_model.continuity_anchor.hash(&mut hasher);
    }
    if let Some(self_authored_core) = snapshot.self_authored_core.as_ref() {
        self_authored_core.updated_at.hash(&mut hasher);
        self_authored_core.revision.hash(&mut hasher);
    }
    if let Some(self_continuity) = snapshot.self_continuity.as_ref() {
        self_continuity.updated_at.hash(&mut hasher);
        self_continuity.current_self_state.hash(&mut hasher);
    }
    if let Some(relationship_portfolio) = snapshot.relationship_portfolio.as_ref() {
        relationship_portfolio.updated_at.hash(&mut hasher);
        relationship_portfolio.entries.len().hash(&mut hasher);
    }
    if let Some(execution_state) = snapshot.execution_state.as_ref() {
        execution_state.updated_at.hash(&mut hasher);
        execution_state.goal.hash(&mut hasher);
        execution_state.next_action.hash(&mut hasher);
    }
    format!("snapshot-{:016x}", hasher.finish())
}

pub fn select_active_continuity_snapshot_chat_ids(
    session_store: &dyn SessionStore,
    self_continuity_store: &dyn SelfContinuityStore,
    relationship_portfolio_store: &dyn RelationshipPortfolioStore,
    relationship_topology_store: &dyn RelationshipTopologyStore,
    preferred_chat_id: Option<&str>,
    now_secs: u64,
    active_window_secs: u64,
    limit: usize,
) -> Vec<String> {
    let continuity = self_continuity_store
        .get(board_subject_scope_id())
        .ok()
        .flatten();
    let limit = limit.max(1);
    let mut selected = Vec::with_capacity(limit);
    push_unique_chat_id(&mut selected, preferred_chat_id, limit);
    let last_activity = continuity
        .as_ref()
        .map(|continuity| {
            continuity
                .last_user_turn_at
                .max(continuity.last_autonomy_run_at)
                .max(continuity.updated_at)
        })
        .unwrap_or(0);
    let preferred_channel = continuity.as_ref().and_then(|continuity| {
        let channel = continuity.last_user_channel.trim();
        (!channel.is_empty()).then_some(channel)
    });
    let portfolio = relationship_portfolio_store
        .get(board_subject_scope_id())
        .ok()
        .flatten();
    push_portfolio_chat_ids(
        &mut selected,
        portfolio.as_ref(),
        preferred_chat_id,
        preferred_channel,
        now_secs,
        limit,
    );
    let topology = relationship_topology_store
        .get(board_subject_scope_id())
        .ok()
        .flatten();
    push_topology_chat_ids(
        &mut selected,
        topology.as_ref(),
        preferred_chat_id,
        preferred_channel,
        now_secs,
        active_window_secs,
        limit,
    );
    let board_subject_chat_id = continuity.as_ref().and_then(|continuity| {
        let chat_id = continuity.last_user_chat_id.trim();
        (!chat_id.is_empty()).then_some(chat_id)
    });
    let board_subject_is_active = last_activity == 0
        || now_secs == 0
        || now_secs.saturating_sub(last_activity) <= active_window_secs;
    if board_subject_is_active {
        push_unique_chat_id(&mut selected, board_subject_chat_id, limit);
    }
    if selected.is_empty() {
        let mut chat_ids = session_store.list_chat_ids().unwrap_or_default();
        chat_ids.sort();
        for chat_id in chat_ids {
            if selected.len() >= limit {
                break;
            }
            push_unique_chat_id(&mut selected, Some(chat_id.as_str()), limit);
        }
    }
    selected
}

pub fn select_personality_governance_targets(
    self_continuity: Option<&SelfContinuity>,
    relationship_portfolio: Option<&RelationshipPortfolio>,
    relationship_topology: Option<&RelationshipTopology>,
    now_secs: u64,
    max_targets: usize,
) -> Vec<RelationshipSelectionTarget> {
    let max_targets = max_targets.max(1);
    let preferred_chat_id = self_continuity
        .map(|continuity| continuity.last_user_chat_id.trim())
        .filter(|value| !value.is_empty());
    let preferred_channel = self_continuity
        .map(|continuity| continuity.last_user_channel.trim())
        .filter(|value| !value.is_empty());
    let mut selected = Vec::with_capacity(max_targets);
    let mut seen = HashSet::with_capacity(max_targets);
    for target in select_relationship_portfolio_targets(
        relationship_portfolio,
        RelationshipPortfolioSelectorInput {
            preferred_chat_id,
            preferred_channel,
            now_secs,
            max_targets,
        },
    ) {
        push_unique_relationship_target(&mut selected, &mut seen, target, max_targets);
    }
    for target in select_relationship_topology_targets(
        relationship_topology,
        RelationshipSelectorInput {
            preferred_chat_id,
            preferred_channel,
            now_secs,
            max_targets,
            active_window_secs: PERSONALITY_GOVERNANCE_ACTIVE_WINDOW_SECS,
            runtime_cooldown_secs: 0,
        },
    ) {
        push_unique_relationship_target(&mut selected, &mut seen, target, max_targets);
    }
    if selected.is_empty() {
        if let Some(target) = preferred_chat_id.and_then(|chat_id| {
            select_chat_anchor_relationship_target(
                chat_id,
                preferred_channel,
                relationship_portfolio,
                relationship_topology,
            )
        }) {
            push_unique_relationship_target(&mut selected, &mut seen, target, max_targets);
        }
    }
    selected
}

fn select_snapshot_relationship_scope_id(
    chat_id: &str,
    self_continuity: Option<&SelfContinuity>,
    relationship_portfolio: Option<&RelationshipPortfolio>,
    relationship_topology: Option<&RelationshipTopology>,
) -> Option<String> {
    let preferred_channel = self_continuity.and_then(|continuity| {
        (continuity.last_user_chat_id.trim() == chat_id)
            .then_some(continuity.last_user_channel.trim())
            .filter(|value| !value.is_empty())
    });
    select_chat_anchor_relationship_target(
        chat_id,
        preferred_channel,
        relationship_portfolio,
        relationship_topology,
    )
    .map(|target| target.scope_id)
}

fn select_chat_anchor_relationship_target(
    chat_id: &str,
    preferred_channel: Option<&str>,
    relationship_portfolio: Option<&RelationshipPortfolio>,
    relationship_topology: Option<&RelationshipTopology>,
) -> Option<RelationshipSelectionTarget> {
    if let Some(entry) = relationship_portfolio.and_then(|portfolio| {
        portfolio
            .entries
            .iter()
            .filter(|entry| entry.chat_id.trim() == chat_id && entry.is_meaningful())
            .max_by(|left, right| {
                let left_preferred = (preferred_channel == Some(left.channel.trim())) as u8;
                let right_preferred = (preferred_channel == Some(right.channel.trim())) as u8;
                left_preferred
                    .cmp(&right_preferred)
                    .then_with(|| left.priority_score.cmp(&right.priority_score))
                    .then_with(|| left.last_active_at.cmp(&right.last_active_at))
            })
    }) {
        return Some(RelationshipSelectionTarget {
            scope_id: entry.scope_id.clone(),
            channel: entry.channel.clone(),
            chat_id: entry.chat_id.clone(),
            score: entry.priority_score.max(1),
            reason: "continuity_anchor".to_string(),
        });
    }
    relationship_topology.and_then(|topology| {
        topology
            .entries
            .iter()
            .filter(|entry| entry.chat_id.trim() == chat_id && entry.is_meaningful())
            .max_by(|left, right| {
                let left_preferred = (preferred_channel == Some(left.channel.trim())) as u8;
                let right_preferred = (preferred_channel == Some(right.channel.trim())) as u8;
                left_preferred
                    .cmp(&right_preferred)
                    .then_with(|| left.latest_overlay_at().cmp(&right.latest_overlay_at()))
            })
            .map(|entry| RelationshipSelectionTarget {
                scope_id: entry.scope_id.clone(),
                channel: entry.channel.clone(),
                chat_id: entry.chat_id.clone(),
                score: 1,
                reason: "continuity_anchor".to_string(),
            })
    })
}

fn push_unique_relationship_target(
    selected: &mut Vec<RelationshipSelectionTarget>,
    seen: &mut HashSet<String>,
    target: RelationshipSelectionTarget,
    limit: usize,
) {
    if selected.len() >= limit || !seen.insert(target.scope_id.clone()) {
        return;
    }
    selected.push(target);
}

fn push_portfolio_chat_ids(
    selected: &mut Vec<String>,
    portfolio: Option<&RelationshipPortfolio>,
    preferred_chat_id: Option<&str>,
    preferred_channel: Option<&str>,
    now_secs: u64,
    limit: usize,
) {
    let Some(portfolio) = portfolio else {
        return;
    };
    let targets = select_relationship_portfolio_targets(
        Some(portfolio),
        RelationshipPortfolioSelectorInput {
            preferred_chat_id,
            preferred_channel,
            now_secs,
            max_targets: limit,
        },
    );
    for target in targets {
        if selected.len() >= limit {
            break;
        }
        push_unique_chat_id(selected, Some(target.chat_id.as_str()), limit);
    }
}

fn push_topology_chat_ids(
    selected: &mut Vec<String>,
    topology: Option<&RelationshipTopology>,
    preferred_chat_id: Option<&str>,
    preferred_channel: Option<&str>,
    now_secs: u64,
    active_window_secs: u64,
    limit: usize,
) {
    let Some(topology) = topology else {
        return;
    };
    let targets = select_relationship_topology_targets(
        Some(topology),
        RelationshipSelectorInput {
            preferred_chat_id,
            preferred_channel,
            now_secs,
            max_targets: limit,
            active_window_secs,
            runtime_cooldown_secs: 0,
        },
    );
    for target in targets {
        if selected.len() >= limit {
            break;
        }
        push_unique_chat_id(selected, Some(target.chat_id.as_str()), limit);
    }
}

fn push_unique_chat_id(selected: &mut Vec<String>, chat_id: Option<&str>, limit: usize) {
    if selected.len() >= limit {
        return;
    }
    let Some(chat_id) = chat_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    if selected.iter().any(|existing| existing == chat_id) {
        return;
    }
    selected.push(chat_id.to_string());
}

pub fn render_continuity_snapshot_markdown(snapshot: &ContinuitySnapshot) -> String {
    let manifest = snapshot_manifest(snapshot);
    let mut out = String::with_capacity(2048);
    let _ = writeln!(out, "# Continuity Snapshot");
    let _ = writeln!(out, "- chat_id: {}", snapshot.chat_id);
    if !snapshot.subject_id.trim().is_empty() {
        let _ = writeln!(out, "- subject_id: {}", snapshot.subject_id.trim());
    }
    let _ = writeln!(out, "- mode: {:?}", snapshot.mode);
    let _ = writeln!(out, "- exported_at: {}", snapshot.exported_at);
    let _ = writeln!(out, "- fingerprint: {}", manifest.content_fingerprint);
    if let Some(scope_id) = manifest.relationship_scope_id.as_deref() {
        let _ = writeln!(out, "- relationship_scope_id: {}", scope_id);
    }
    let _ = writeln!(out, "- long_term_count: {}", manifest.long_term_count);
    if !manifest.long_term_kind_counts.is_empty() {
        let kind_counts = manifest
            .long_term_kind_counts
            .iter()
            .map(|count| format!("{}={}", count.kind.label(), count.count))
            .collect::<Vec<_>>()
            .join(" | ");
        let _ = writeln!(out, "- long_term_kind_counts: {}", kind_counts);
    }
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
    if let Some(self_authored_core) = snapshot.self_authored_core.as_ref() {
        let _ = writeln!(out, "\n## Self-Authored Core");
        let _ = writeln!(out, "- revision: {}", self_authored_core.revision.max(1));
        let _ = writeln!(
            out,
            "- stability_score: {}",
            self_authored_core.stability_score
        );
        if !self_authored_core.identity_anchor.trim().is_empty() {
            let _ = writeln!(
                out,
                "- identity_anchor: {}",
                self_authored_core.identity_anchor.trim()
            );
        }
        if !self_authored_core.non_negotiables.is_empty() {
            let _ = writeln!(
                out,
                "- non_negotiables: {}",
                self_authored_core.non_negotiables.join(" | ")
            );
        }
        if !self_authored_core.priority_constitution.is_empty() {
            let _ = writeln!(
                out,
                "- priority_constitution: {}",
                self_authored_core.priority_constitution.join(" > ")
            );
        }
        if !self_authored_core.boundary_doctrine.trim().is_empty() {
            let _ = writeln!(
                out,
                "- boundary_doctrine: {}",
                self_authored_core.boundary_doctrine.trim()
            );
        }
        if !self_authored_core.change_protocol.trim().is_empty() {
            let _ = writeln!(
                out,
                "- change_protocol: {}",
                self_authored_core.change_protocol.trim()
            );
        }
    }
    if let Some(core_revision_ledger) = snapshot.core_revision_ledger.as_ref() {
        let _ = writeln!(out, "\n## Core Revision Ledger");
        let _ = writeln!(out, "- entries: {}", core_revision_ledger.entries.len());
        if let Some(record) = core_revision_ledger.entries.last() {
            let _ = writeln!(out, "- latest_outcome: {}", record.outcome.label());
            let _ = writeln!(
                out,
                "- latest_reason: {}",
                record.adjudication_reason.trim()
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
    if let Some(relationship_portfolio) = snapshot.relationship_portfolio.as_ref() {
        let _ = writeln!(out, "\n## Relationship Portfolio");
        for entry in relationship_portfolio.entries.iter().take(4) {
            let _ = writeln!(
                out,
                "- {}:{} state={} inheritance={} reason={}",
                entry.channel,
                entry.chat_id,
                entry.governance_state.label(),
                entry.inheritance_mode.label(),
                entry.reason.trim()
            );
        }
    }
    if let Some(relationship_constitution) = snapshot.relationship_constitution.as_ref() {
        let _ = writeln!(out, "\n## Relationship Constitution");
        let _ = writeln!(
            out,
            "- scope_id: {}",
            relationship_constitution.scope_id.trim()
        );
        let _ = writeln!(
            out,
            "- governance: {} / inheritance={}",
            relationship_constitution.governance_state.label(),
            relationship_constitution.inheritance_mode.label()
        );
        let _ = writeln!(
            out,
            "- alignment: {}",
            relationship_constitution.alignment.label()
        );
        let _ = writeln!(
            out,
            "- task_scope_ceiling: {}",
            relationship_constitution.task_scope_ceiling.label()
        );
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
        CoreRevisionActionKind, CoreRevisionOutcome, CoreRevisionRecord, CoreRevisionRecordChange,
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
            self.set_with_count(_chat_id, summary, 0)
        }

        fn set_with_count(
            &self,
            _chat_id: &str,
            summary: &str,
            message_count: usize,
        ) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) =
                Some((summary.to_string(), message_count));
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
    struct StubSelfAuthoredCoreStore {
        state: Mutex<Option<SelfAuthoredCore>>,
    }

    impl SelfAuthoredCoreStore for StubSelfAuthoredCoreStore {
        fn get(&self, _scope_id: &str) -> Result<Option<SelfAuthoredCore>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, core: &SelfAuthoredCore) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(core.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubCoreRevisionLedgerStore {
        state: Mutex<Option<CoreRevisionLedger>>,
    }

    impl CoreRevisionLedgerStore for StubCoreRevisionLedgerStore {
        fn get(&self, _scope_id: &str) -> Result<Option<CoreRevisionLedger>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, ledger: &CoreRevisionLedger) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(ledger.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
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

    #[derive(Default)]
    struct StubRelationshipConstitutionStore {
        value: Mutex<Option<RelationshipConstitution>>,
    }

    impl RelationshipConstitutionStore for StubRelationshipConstitutionStore {
        fn get(&self, _scope_id: &str) -> Result<Option<RelationshipConstitution>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, constitution: &RelationshipConstitution) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(constitution.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipPortfolioStore {
        value: Option<RelationshipPortfolio>,
    }

    impl RelationshipPortfolioStore for StubRelationshipPortfolioStore {
        fn get(&self, _scope_id: &str) -> Result<Option<RelationshipPortfolio>> {
            Ok(self.value.clone())
        }

        fn set(&self, _scope_id: &str, _portfolio: &RelationshipPortfolio) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    struct StubRelationshipTopologyStore {
        entries: std::collections::HashMap<String, RelationshipTopology>,
    }

    impl RelationshipTopologyStore for StubRelationshipTopologyStore {
        fn get(&self, scope_id: &str) -> Result<Option<RelationshipTopology>> {
            Ok(self.entries.get(scope_id).cloned())
        }

        fn set(&self, _scope_id: &str, _topology: &RelationshipTopology) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
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
                self_authored_core_store: &StubSelfAuthoredCoreStore::default(),
                core_revision_ledger_store: &StubCoreRevisionLedgerStore::default(),
                self_continuity_store: &StubSelfContinuityStore::default(),
                relationship_constitution_store: &StubRelationshipConstitutionStore::default(),
                relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
                relationship_topology_store: &StubRelationshipTopologyStore {
                    entries: std::collections::HashMap::new(),
                },
            },
            "chat-1",
            ContinuitySnapshotMode::Bootstrap,
            10,
        )
        .unwrap();
        assert_eq!(snapshot.long_term_memory.len(), 2);
        assert_eq!(snapshot.summary_message_count, Some(3));
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
        let self_authored_core_store = StubSelfAuthoredCoreStore::default();
        let core_revision_ledger_store = StubCoreRevisionLedgerStore::default();
        let self_continuity_store = StubSelfContinuityStore::default();
        let relationship_constitution_store = StubRelationshipConstitutionStore::default();
        let relationship_portfolio_store = StubRelationshipPortfolioStore::default();
        let snapshot = ContinuitySnapshot {
            version: CONTINUITY_SNAPSHOT_VERSION,
            exported_at: 11,
            mode: ContinuitySnapshotMode::FullRestore,
            chat_id: "chat-1".to_string(),
            subject_id: board_subject_scope_id().to_string(),
            manifest: ContinuitySnapshotManifest::default(),
            summary_text: None,
            summary_message_count: None,
            long_term_memory: vec![sample_entry(LongTermMemoryKind::Profile, "owner_profile")],
            self_model: Some(SelfModel {
                continuity_anchor: "same line".to_string(),
                self_narrative: String::new(),
                relationship_state: String::new(),
                private_notes: String::new(),
                updated_at: 11,
                ..SelfModel::default()
            }),
            self_authored_core: Some(SelfAuthoredCore {
                revision: 1,
                stability_score: 64,
                last_reviewed_at: 11,
                identity_anchor: "board self".to_string(),
                priority_constitution: vec![
                    "self_authored_core".to_string(),
                    "boundary".to_string(),
                    "user_contract".to_string(),
                ],
                change_protocol: "revise only after stable evidence".to_string(),
                updated_at: 11,
                ..SelfAuthoredCore::default()
            }),
            core_revision_ledger: Some(CoreRevisionLedger {
                entries: vec![CoreRevisionRecord {
                    based_on_revision: 0,
                    resulting_revision: 1,
                    relationship_scope_id: "rel:qq_channel:chat-1".to_string(),
                    source_layers: vec!["self_model".to_string()],
                    outcome: CoreRevisionOutcome::Adopted,
                    evidence_summary: vec!["bootstrap".to_string()],
                    counterevidence: Vec::new(),
                    accepted_changes: vec![CoreRevisionRecordChange {
                        kind: CoreRevisionActionKind::ReviseIdentityAnchor,
                        summary: "bootstrap".to_string(),
                    }],
                    rejected_changes: Vec::new(),
                    conflict_classes: Vec::new(),
                    corrects_revision: None,
                    correction_kind: None,
                    observation_due_at: 11,
                    adjudication_reason: "bootstrap".to_string(),
                    rationale: "seed".to_string(),
                    stability_score: 64,
                    reviewed_at: 11,
                }],
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
                last_user_chat_id: "chat-1".to_string(),
                last_user_channel: "qq_channel".to_string(),
                last_autonomy_run_at: 0,
                updated_at: 11,
            }),
            relationship_constitution: Some(RelationshipConstitution {
                scope_id: "rel:qq_channel:chat-1".to_string(),
                channel: "qq_channel".to_string(),
                chat_id: "chat-1".to_string(),
                board_revision: 1,
                governance_state: crate::memory::RelationshipGovernanceState::Maintain,
                inheritance_mode: crate::memory::RelationshipInheritanceMode::Guarded,
                task_scope_ceiling: crate::memory::RelationshipTaskScopeCeiling::Brief,
                updated_at: 11,
                ..RelationshipConstitution::default()
            }),
            relationship_portfolio: Some(RelationshipPortfolio {
                entries: vec![crate::memory::RelationshipPortfolioEntry {
                    scope_id: "rel:qq_channel:chat-1".to_string(),
                    channel: "qq_channel".to_string(),
                    chat_id: "chat-1".to_string(),
                    governance_state: crate::memory::RelationshipGovernanceState::Maintain,
                    inheritance_mode: crate::memory::RelationshipInheritanceMode::Guarded,
                    priority_score: 220,
                    reason: "maintain".to_string(),
                    source_updated_at: 11,
                    last_active_at: 11,
                    needs_runtime_attention: true,
                    last_selected_at: 0,
                    next_review_at: 0,
                }],
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
                ..ExecutionState::default()
            }),
        };
        let outcome = import_continuity_snapshot(
            ContinuitySnapshotImportContext {
                long_term_memory_store: &store,
                session_summary_store: &StubSummaryStore::default(),
                execution_state_store: &execution_state_store,
                self_model_store: &self_model_store,
                self_authored_core_store: &self_authored_core_store,
                core_revision_ledger_store: &core_revision_ledger_store,
                self_continuity_store: &self_continuity_store,
                relationship_constitution_store: &relationship_constitution_store,
                relationship_portfolio_store: &relationship_portfolio_store,
            },
            "chat-new",
            &snapshot,
            ContinuitySnapshotImportMode::FullRestore,
        )
        .unwrap();
        assert_eq!(outcome.long_term_imported, 1);
        assert!(outcome.self_model_restored);
        assert!(outcome.self_authored_core_restored);
        assert!(outcome.core_revision_ledger_restored);
        assert!(outcome.self_continuity_restored);
        assert!(outcome.relationship_constitution_restored);
        assert!(outcome.relationship_portfolio_restored);
        assert!(outcome.execution_state_restored);
        assert_eq!(
            outcome.long_term_write_outcome.source,
            SharedMemoryWriteSource::SnapshotImport
        );
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
                self_authored_core_store: &StubSelfAuthoredCoreStore::default(),
                core_revision_ledger_store: &StubCoreRevisionLedgerStore::default(),
                self_continuity_store: &StubSelfContinuityStore::default(),
                relationship_constitution_store: &StubRelationshipConstitutionStore::default(),
                relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            },
            "chat-new",
            &ContinuitySnapshot {
                version: CONTINUITY_SNAPSHOT_VERSION,
                exported_at: 20,
                mode: ContinuitySnapshotMode::Bootstrap,
                chat_id: "chat-old".to_string(),
                subject_id: board_subject_scope_id().to_string(),
                manifest: ContinuitySnapshotManifest::default(),
                summary_text: Some("stable summary".to_string()),
                summary_message_count: Some(12),
                long_term_memory: Vec::new(),
                self_model: None,
                self_authored_core: None,
                core_revision_ledger: None,
                self_continuity: None,
                relationship_constitution: None,
                relationship_portfolio: None,
                execution_state: None,
            },
            ContinuitySnapshotImportMode::BootstrapImport,
        )
        .unwrap();
        assert!(outcome.summary_restored);
        assert_eq!(
            summary_store.get_with_count("chat-new").unwrap(),
            Some(("stable summary".to_string(), 12))
        );
    }

    #[test]
    fn full_restore_import_does_not_override_newer_local_summary() {
        let summary_store = StubSummaryStore::default();
        summary_store
            .set_with_count("chat-new", "newer local summary", 18)
            .unwrap();
        let outcome = import_continuity_snapshot(
            ContinuitySnapshotImportContext {
                long_term_memory_store: &StubLongTermMemoryStore::default(),
                session_summary_store: &summary_store,
                execution_state_store: &StubExecutionStateStore::default(),
                self_model_store: &StubSelfModelStore::default(),
                self_authored_core_store: &StubSelfAuthoredCoreStore::default(),
                core_revision_ledger_store: &StubCoreRevisionLedgerStore::default(),
                self_continuity_store: &StubSelfContinuityStore::default(),
                relationship_constitution_store: &StubRelationshipConstitutionStore::default(),
                relationship_portfolio_store: &StubRelationshipPortfolioStore::default(),
            },
            "chat-new",
            &ContinuitySnapshot {
                version: CONTINUITY_SNAPSHOT_VERSION,
                exported_at: 20,
                mode: ContinuitySnapshotMode::Bootstrap,
                chat_id: "chat-old".to_string(),
                subject_id: board_subject_scope_id().to_string(),
                manifest: ContinuitySnapshotManifest::default(),
                summary_text: Some("older snapshot summary".to_string()),
                summary_message_count: Some(11),
                long_term_memory: Vec::new(),
                self_model: None,
                self_authored_core: None,
                core_revision_ledger: None,
                self_continuity: None,
                relationship_constitution: None,
                relationship_portfolio: None,
                execution_state: None,
            },
            ContinuitySnapshotImportMode::BootstrapImport,
        )
        .unwrap();
        assert!(!outcome.summary_restored);
        assert_eq!(
            summary_store.get_with_count("chat-new").unwrap(),
            Some(("newer local summary".to_string(), 18))
        );
    }

    #[test]
    fn select_active_chat_ids_prefers_preferred_and_board_subject_anchor() {
        let session_store = StubSessionStore {
            chat_ids: vec![
                "chat-stale".to_string(),
                "chat-recent".to_string(),
                "chat-preferred".to_string(),
            ],
        };
        let continuity_store = MultiSelfContinuityStore {
            entries: [(
                board_subject_scope_id().to_string(),
                SelfContinuity {
                    wake_anchor: String::new(),
                    current_self_state: String::new(),
                    recent_changes: String::new(),
                    continuity_bridge: String::new(),
                    priority_posture: String::new(),
                    relationship_posture: String::new(),
                    task_posture: String::new(),
                    last_user_turn_at: 990,
                    last_user_chat_id: "chat-recent".to_string(),
                    last_user_channel: "qq_channel".to_string(),
                    last_autonomy_run_at: 995,
                    updated_at: 995,
                },
            )]
            .into_iter()
            .collect(),
        };
        let topology_store = StubRelationshipTopologyStore {
            entries: [(
                board_subject_scope_id().to_string(),
                RelationshipTopology {
                    entries: vec![crate::memory::RelationshipTopologyEntry {
                        scope_id: "rel:qq_channel:chat-recent".to_string(),
                        channel: "qq_channel".to_string(),
                        chat_id: "chat-recent".to_string(),
                        last_user_turn_at: 995,
                        last_persona_turn_at: 995,
                        ..crate::memory::RelationshipTopologyEntry::default()
                    }],
                    updated_at: 995,
                },
            )]
            .into_iter()
            .collect(),
        };
        let portfolio_store = StubRelationshipPortfolioStore {
            value: Some(RelationshipPortfolio {
                entries: vec![crate::memory::RelationshipPortfolioEntry {
                    scope_id: "rel:qq_channel:chat-recent".to_string(),
                    channel: "qq_channel".to_string(),
                    chat_id: "chat-recent".to_string(),
                    governance_state: crate::memory::RelationshipGovernanceState::Maintain,
                    inheritance_mode: crate::memory::RelationshipInheritanceMode::Guarded,
                    priority_score: 240,
                    reason: "maintain".to_string(),
                    source_updated_at: 995,
                    last_active_at: 995,
                    needs_runtime_attention: true,
                    last_selected_at: 0,
                    next_review_at: 0,
                }],
                updated_at: 995,
            }),
        };
        let selected = select_active_continuity_snapshot_chat_ids(
            &session_store,
            &continuity_store,
            &portfolio_store,
            &topology_store,
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

    #[test]
    fn select_personality_governance_targets_falls_back_to_anchor_when_rankers_skip_relation() {
        let continuity = SelfContinuity {
            last_user_chat_id: "chat-a".to_string(),
            last_user_channel: "qq".to_string(),
            updated_at: 950,
            ..SelfContinuity::default()
        };
        let portfolio = RelationshipPortfolio {
            entries: vec![crate::memory::RelationshipPortfolioEntry {
                scope_id: "rel:tg:chat-a".to_string(),
                channel: "tg".to_string(),
                chat_id: "chat-a".to_string(),
                governance_state: crate::memory::RelationshipGovernanceState::Maintain,
                inheritance_mode: crate::memory::RelationshipInheritanceMode::Guarded,
                priority_score: 180,
                reason: "stable_anchor".to_string(),
                source_updated_at: 900,
                last_active_at: 900,
                needs_runtime_attention: false,
                last_selected_at: 900,
                next_review_at: 10_000,
            }],
            updated_at: 900,
        };

        let selected = select_personality_governance_targets(
            Some(&continuity),
            Some(&portfolio),
            None,
            1_000,
            1,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].scope_id, "rel:tg:chat-a");
        assert_eq!(selected[0].reason, "continuity_anchor");
    }

    #[test]
    fn select_personality_governance_targets_prefers_ranked_exact_relation_before_anchor() {
        let continuity = SelfContinuity {
            last_user_chat_id: "chat-a".to_string(),
            last_user_channel: "qq".to_string(),
            updated_at: 950,
            ..SelfContinuity::default()
        };
        let portfolio = RelationshipPortfolio {
            entries: vec![crate::memory::RelationshipPortfolioEntry {
                scope_id: "rel:qq:chat-a".to_string(),
                channel: "qq".to_string(),
                chat_id: "chat-a".to_string(),
                governance_state: crate::memory::RelationshipGovernanceState::Repair,
                inheritance_mode: crate::memory::RelationshipInheritanceMode::Guarded,
                priority_score: 240,
                reason: "repair_due".to_string(),
                source_updated_at: 980,
                last_active_at: 980,
                needs_runtime_attention: true,
                last_selected_at: 0,
                next_review_at: 0,
            }],
            updated_at: 980,
        };

        let selected = select_personality_governance_targets(
            Some(&continuity),
            Some(&portfolio),
            None,
            1_000,
            1,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].scope_id, "rel:qq:chat-a");
        assert!(selected[0].reason.contains("preferred_relation"));
    }

    #[test]
    fn select_personality_governance_targets_deduplicates_portfolio_and_topology_hits() {
        let continuity = SelfContinuity {
            last_user_chat_id: "chat-a".to_string(),
            last_user_channel: "qq".to_string(),
            updated_at: 980,
            ..SelfContinuity::default()
        };
        let portfolio = RelationshipPortfolio {
            entries: vec![crate::memory::RelationshipPortfolioEntry {
                scope_id: "rel:qq:chat-a".to_string(),
                channel: "qq".to_string(),
                chat_id: "chat-a".to_string(),
                governance_state: crate::memory::RelationshipGovernanceState::Repair,
                inheritance_mode: crate::memory::RelationshipInheritanceMode::Guarded,
                priority_score: 220,
                reason: "repair".to_string(),
                source_updated_at: 980,
                last_active_at: 980,
                needs_runtime_attention: true,
                last_selected_at: 0,
                next_review_at: 0,
            }],
            updated_at: 980,
        };
        let topology = RelationshipTopology {
            entries: vec![crate::memory::RelationshipTopologyEntry {
                scope_id: "rel:qq:chat-a".to_string(),
                channel: "qq".to_string(),
                chat_id: "chat-a".to_string(),
                last_active_at: 980,
                last_user_turn_at: 980,
                last_runtime_refresh_at: 900,
                ..crate::memory::RelationshipTopologyEntry::default()
            }],
            updated_at: 980,
        };

        let selected = select_personality_governance_targets(
            Some(&continuity),
            Some(&portfolio),
            Some(&topology),
            1_000,
            3,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].scope_id, "rel:qq:chat-a");
        assert!(selected[0].reason.contains("repair"));
    }
}
