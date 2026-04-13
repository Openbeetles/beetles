//! Soul kernel continuity contract and recovery helpers.
//! 板级主体内核的连续性体检与恢复合同。

use crate::memory::{
    board_subject_scope_id, import_continuity_snapshot, select_active_continuity_snapshot_chat_ids,
    ContinuitySnapshotImportContext, ContinuitySnapshotImportMode, CoreRevisionLedger,
    CoreRevisionLedgerStore, ExecutionStateStore, LongTermMemoryEntry, LongTermMemoryKind,
    LongTermMemoryStore, RelationshipConstitutionStore, RelationshipPortfolioStore,
    RelationshipTopologyStore, SelfAuthoredCore, SelfAuthoredCoreStore, SelfContinuity,
    SelfContinuityStore, SelfModel, SelfModelStore, SessionStore, SessionSummaryStore,
};
use crate::platform::{Platform, StateFs};
use crate::runtime::continuity_flush::{
    ContinuitySnapshotBundle, REL_PATH_REBOOT_CONTINUITY_BUNDLE,
};
use serde::{Deserialize, Serialize};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::sync::{Mutex, OnceLock};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::time::{Duration, Instant};

const SOUL_KERNEL_ACTIVE_WINDOW_SECS: u64 = 7 * 86_400;
const SOUL_KERNEL_ACTIVE_CHAT_LIMIT: usize = 4;
const SOUL_KERNEL_KEY_MEMORY_SCAN_LIMIT: usize = 64;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const SOUL_KERNEL_STATUS_CACHE_TTL: Duration = Duration::from_secs(30);

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
#[derive(Clone, Debug)]
struct CachedSoulKernelStatus {
    cached_at: Instant,
    status: SoulKernelStatus,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
static SOUL_KERNEL_STATUS_CACHE: OnceLock<Mutex<Option<CachedSoulKernelStatus>>> = OnceLock::new();
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
static RUNTIME_BUNDLE_STATUS_CACHE: OnceLock<Mutex<Option<SoulKernelRuntimeBundleStatus>>> =
    OnceLock::new();

fn append_reboot_recovery_workflow_audit(
    disposition: crate::runtime::WorkflowDisposition,
    rationale: &str,
    effect: crate::runtime::WorkflowEffect,
    recovery_policy: crate::runtime::WorkflowRecoveryPolicy,
    primary_chat_id: Option<&str>,
) {
    crate::runtime::append_workflow_audit(
        crate::runtime::WorkflowAuditRecord::new(
            crate::runtime::WorkflowKind::RebootRecovery,
            crate::runtime::WorkflowTrigger::BootRecovery,
            disposition,
            effect,
            recovery_policy,
            rationale,
            crate::util::current_unix_secs(),
        )
        .with_target(None, None, primary_chat_id),
    );
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoulKernelPromptProjection {
    pub personality_governance_gate_text: Option<String>,
    pub self_authored_core_text: Option<String>,
    pub relationship_constitution_text: Option<String>,
    pub persona_priority_text: Option<String>,
    pub mental_privacy_adjudication_text: Option<String>,
}

impl SoulKernelPromptProjection {
    pub fn constitutional_stack_text(&self) -> Option<String> {
        let mut out = String::new();
        for part in [
            self.personality_governance_gate_text.as_deref(),
            self.self_authored_core_text.as_deref(),
            self.relationship_constitution_text.as_deref(),
            self.persona_priority_text.as_deref(),
            self.mental_privacy_adjudication_text.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(trimmed);
        }
        (!out.is_empty()).then_some(out)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoulKernelLayerStatus {
    pub readable: bool,
    pub present: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoulKernelRuntimeBundleStatus {
    pub present: bool,
    pub loadable: bool,
    #[serde(default)]
    pub snapshot_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flushed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoulKernelStatus {
    pub subject_id: String,
    pub session_chat_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_chat_ids: Vec<String>,
    pub self_model: SoulKernelLayerStatus,
    pub self_authored_core: SoulKernelLayerStatus,
    pub core_revision_ledger: SoulKernelLayerStatus,
    pub self_continuity: SoulKernelLayerStatus,
    pub key_memory_readable: bool,
    pub key_memory_count: usize,
    pub expected_bootstrap_empty: bool,
    pub minimum_viable: bool,
    pub safe_mode_minimum_readable: bool,
    pub degraded: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degradation_reasons: Vec<String>,
    pub runtime_bundle: SoulKernelRuntimeBundleStatus,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SoulKernelRecoveryAction {
    #[default]
    NotNeeded,
    RestoredFromRuntimeBundle,
    RestoreAttemptedNoChange,
    NoBundleAvailable,
    BundleUnreadable,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoulKernelRecoveryReport {
    pub action: SoulKernelRecoveryAction,
    pub restore_attempted: bool,
    pub restored_snapshots: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restored_layers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
    pub status_before: SoulKernelStatus,
    pub status_after: SoulKernelStatus,
}

pub struct SoulKernelInspectContext<'a> {
    pub state_fs: &'a dyn StateFs,
    pub session_store: &'a dyn SessionStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub self_authored_core_store: &'a dyn SelfAuthoredCoreStore,
    pub core_revision_ledger_store: &'a dyn CoreRevisionLedgerStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub relationship_portfolio_store: &'a dyn RelationshipPortfolioStore,
    pub relationship_topology_store: &'a dyn RelationshipTopologyStore,
}

pub struct SoulKernelRecoveryContext<'a> {
    pub inspect: SoulKernelInspectContext<'a>,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub relationship_constitution_store: &'a dyn RelationshipConstitutionStore,
}

pub fn inspect_platform_soul_kernel(platform: &dyn Platform, now_secs: u64) -> SoulKernelStatus {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    if let Some(status) = cached_platform_soul_kernel_status() {
        return status;
    }

    let status = inspect_soul_kernel(
        SoulKernelInspectContext {
            state_fs: platform.state_fs().as_ref(),
            session_store: platform.session_store().as_ref(),
            long_term_memory_store: platform.long_term_memory_store().as_ref(),
            self_model_store: platform.self_model_store().as_ref(),
            self_authored_core_store: platform.self_authored_core_store().as_ref(),
            core_revision_ledger_store: platform.core_revision_ledger_store().as_ref(),
            self_continuity_store: platform.self_continuity_store().as_ref(),
            relationship_portfolio_store: platform.relationship_portfolio_store().as_ref(),
            relationship_topology_store: platform.relationship_topology_store().as_ref(),
        },
        now_secs,
    );

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    update_platform_soul_kernel_status_cache(&status);

    status
}

pub fn ensure_platform_soul_kernel_recovery(
    platform: &dyn Platform,
    now_secs: u64,
) -> SoulKernelRecoveryReport {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    invalidate_platform_soul_kernel_status_cache();

    let report = ensure_soul_kernel_recovery(
        SoulKernelRecoveryContext {
            inspect: SoulKernelInspectContext {
                state_fs: platform.state_fs().as_ref(),
                session_store: platform.session_store().as_ref(),
                long_term_memory_store: platform.long_term_memory_store().as_ref(),
                self_model_store: platform.self_model_store().as_ref(),
                self_authored_core_store: platform.self_authored_core_store().as_ref(),
                core_revision_ledger_store: platform.core_revision_ledger_store().as_ref(),
                self_continuity_store: platform.self_continuity_store().as_ref(),
                relationship_portfolio_store: platform.relationship_portfolio_store().as_ref(),
                relationship_topology_store: platform.relationship_topology_store().as_ref(),
            },
            session_summary_store: platform.session_summary_store().as_ref(),
            execution_state_store: platform.execution_state_store().as_ref(),
            relationship_constitution_store: platform.relationship_constitution_store().as_ref(),
        },
        now_secs,
    );

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    update_platform_soul_kernel_status_cache(&report.status_after);

    report
}

pub fn inspect_soul_kernel(ctx: SoulKernelInspectContext<'_>, now_secs: u64) -> SoulKernelStatus {
    let runtime_bundle = read_runtime_bundle_status(ctx.state_fs);
    inspect_soul_kernel_with_runtime_bundle_status(ctx, now_secs, runtime_bundle)
}

fn inspect_soul_kernel_with_runtime_bundle_status(
    ctx: SoulKernelInspectContext<'_>,
    now_secs: u64,
    runtime_bundle: SoulKernelRuntimeBundleStatus,
) -> SoulKernelStatus {
    let subject_id = board_subject_scope_id().to_string();
    let session_chat_count = ctx
        .session_store
        .list_chat_ids()
        .map(|chat_ids| chat_ids.len())
        .unwrap_or(0);

    let self_model = load_layer_status(
        || ctx.self_model_store.get(&subject_id),
        |value: &SelfModel| value.is_meaningful(),
        |value: &SelfModel| value.updated_at,
    );
    let self_authored_core = load_layer_status(
        || ctx.self_authored_core_store.get(&subject_id),
        |value: &SelfAuthoredCore| value.is_meaningful(),
        |value: &SelfAuthoredCore| value.updated_at,
    );
    let core_revision_ledger = load_layer_status(
        || ctx.core_revision_ledger_store.get(&subject_id),
        |value: &CoreRevisionLedger| value.is_meaningful(),
        |value: &CoreRevisionLedger| value.updated_at,
    );
    let self_continuity = load_layer_status(
        || ctx.self_continuity_store.get(&subject_id),
        |value: &SelfContinuity| value.is_meaningful(),
        |value: &SelfContinuity| value.updated_at,
    );

    let active_chat_ids = select_active_continuity_snapshot_chat_ids(
        ctx.session_store,
        ctx.self_continuity_store,
        ctx.relationship_portfolio_store,
        ctx.relationship_topology_store,
        None,
        now_secs,
        SOUL_KERNEL_ACTIVE_WINDOW_SECS,
        SOUL_KERNEL_ACTIVE_CHAT_LIMIT,
    );

    let (key_memory_readable, key_memory_count, key_memory_error) =
        count_key_memory(ctx.long_term_memory_store, &active_chat_ids);

    let identity_anchor_ready = self_authored_core.present || self_model.present;
    let continuity_anchor_ready = self_continuity.present;
    let expected_bootstrap_empty = session_chat_count == 0
        && key_memory_count == 0
        && !runtime_bundle.present
        && !self_model.present
        && !self_authored_core.present
        && !core_revision_ledger.present
        && !self_continuity.present;
    let minimum_viable = identity_anchor_ready && continuity_anchor_ready;
    let safe_mode_minimum_readable = minimum_viable
        || (runtime_bundle.present && runtime_bundle.loadable && runtime_bundle.snapshot_count > 0);

    let mut degradation_reasons = Vec::new();
    push_layer_degradation("self_model", &self_model, &mut degradation_reasons);
    push_layer_degradation(
        "self_authored_core",
        &self_authored_core,
        &mut degradation_reasons,
    );
    push_layer_degradation(
        "core_revision_ledger",
        &core_revision_ledger,
        &mut degradation_reasons,
    );
    push_layer_degradation(
        "self_continuity",
        &self_continuity,
        &mut degradation_reasons,
    );
    if !key_memory_readable {
        degradation_reasons
            .push(key_memory_error.unwrap_or_else(|| "key_memory_unreadable".to_string()));
    }
    if !runtime_bundle.loadable {
        if let Some(error) = runtime_bundle.error.as_deref() {
            degradation_reasons.push(format!("runtime_bundle_unreadable:{error}"));
        } else if runtime_bundle.present {
            degradation_reasons.push("runtime_bundle_unreadable".to_string());
        }
    }
    if !expected_bootstrap_empty {
        if !identity_anchor_ready {
            degradation_reasons.push("missing_identity_anchor".to_string());
        }
        if !continuity_anchor_ready {
            degradation_reasons.push("missing_continuity_anchor".to_string());
        }
        if session_chat_count > 0 && key_memory_count == 0 {
            degradation_reasons.push("missing_key_memory".to_string());
        }
        if !minimum_viable && !runtime_bundle.present {
            degradation_reasons.push("no_runtime_bundle_for_recovery".to_string());
        }
    }
    dedup_strings(&mut degradation_reasons);

    SoulKernelStatus {
        subject_id,
        session_chat_count,
        active_chat_ids,
        self_model,
        self_authored_core,
        core_revision_ledger,
        self_continuity,
        key_memory_readable,
        key_memory_count,
        expected_bootstrap_empty,
        minimum_viable,
        safe_mode_minimum_readable,
        degraded: !expected_bootstrap_empty && !degradation_reasons.is_empty(),
        degradation_reasons,
        runtime_bundle,
    }
}

pub fn ensure_soul_kernel_recovery(
    ctx: SoulKernelRecoveryContext<'_>,
    now_secs: u64,
) -> SoulKernelRecoveryReport {
    let runtime_bundle_load = load_runtime_bundle(ctx.inspect.state_fs);
    let runtime_bundle_status = runtime_bundle_status_from_load_result(&runtime_bundle_load);
    let status_before = inspect_soul_kernel_with_runtime_bundle_status(
        SoulKernelInspectContext {
            state_fs: ctx.inspect.state_fs,
            session_store: ctx.inspect.session_store,
            long_term_memory_store: ctx.inspect.long_term_memory_store,
            self_model_store: ctx.inspect.self_model_store,
            self_authored_core_store: ctx.inspect.self_authored_core_store,
            core_revision_ledger_store: ctx.inspect.core_revision_ledger_store,
            self_continuity_store: ctx.inspect.self_continuity_store,
            relationship_portfolio_store: ctx.inspect.relationship_portfolio_store,
            relationship_topology_store: ctx.inspect.relationship_topology_store,
        },
        now_secs,
        runtime_bundle_status.clone(),
    );

    if status_before.expected_bootstrap_empty
        || (status_before.minimum_viable && !status_before.degraded)
    {
        append_reboot_recovery_workflow_audit(
            crate::runtime::WorkflowDisposition::NoTrigger,
            "soul_kernel_recovery_not_needed",
            crate::runtime::WorkflowEffect::Noop,
            crate::runtime::WorkflowRecoveryPolicy::ReplayAfterBoot,
            None,
        );
        return SoulKernelRecoveryReport {
            action: SoulKernelRecoveryAction::NotNeeded,
            restore_attempted: false,
            restored_snapshots: 0,
            restored_layers: Vec::new(),
            errors: Vec::new(),
            status_after: status_before.clone(),
            status_before,
        };
    }

    let bundle = match runtime_bundle_load {
        Ok(Some(bundle)) => bundle,
        Ok(None) => {
            append_reboot_recovery_workflow_audit(
                crate::runtime::WorkflowDisposition::NoTrigger,
                "runtime_bundle_unavailable",
                crate::runtime::WorkflowEffect::Noop,
                crate::runtime::WorkflowRecoveryPolicy::ReplayAfterBoot,
                None,
            );
            return SoulKernelRecoveryReport {
                action: SoulKernelRecoveryAction::NoBundleAvailable,
                restore_attempted: false,
                restored_snapshots: 0,
                restored_layers: Vec::new(),
                errors: Vec::new(),
                status_after: status_before.clone(),
                status_before,
            };
        }
        Err(error) => {
            append_reboot_recovery_workflow_audit(
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "runtime_bundle_unreadable",
                crate::runtime::WorkflowEffect::Noop,
                crate::runtime::WorkflowRecoveryPolicy::OperatorAckRequired,
                None,
            );
            return SoulKernelRecoveryReport {
                action: SoulKernelRecoveryAction::BundleUnreadable,
                restore_attempted: false,
                restored_snapshots: 0,
                restored_layers: Vec::new(),
                errors: vec![error],
                status_after: status_before.clone(),
                status_before,
            };
        }
    };

    let import_ctx = ContinuitySnapshotImportContext {
        long_term_memory_store: ctx.inspect.long_term_memory_store,
        session_summary_store: ctx.session_summary_store,
        execution_state_store: ctx.execution_state_store,
        self_model_store: ctx.inspect.self_model_store,
        self_authored_core_store: ctx.inspect.self_authored_core_store,
        core_revision_ledger_store: ctx.inspect.core_revision_ledger_store,
        self_continuity_store: ctx.inspect.self_continuity_store,
        relationship_constitution_store: ctx.relationship_constitution_store,
        relationship_portfolio_store: ctx.inspect.relationship_portfolio_store,
    };

    let mut restored_snapshots = 0usize;
    let mut restored_layers = Vec::new();
    let mut errors = Vec::new();
    for snapshot in ordered_bundle_snapshots(&bundle) {
        let target_chat_id = snapshot.chat_id.trim();
        if target_chat_id.is_empty() {
            errors.push("runtime_bundle_snapshot_missing_chat_id".to_string());
            continue;
        }
        match import_continuity_snapshot(
            ContinuitySnapshotImportContext {
                long_term_memory_store: import_ctx.long_term_memory_store,
                session_summary_store: import_ctx.session_summary_store,
                execution_state_store: import_ctx.execution_state_store,
                self_model_store: import_ctx.self_model_store,
                self_authored_core_store: import_ctx.self_authored_core_store,
                core_revision_ledger_store: import_ctx.core_revision_ledger_store,
                self_continuity_store: import_ctx.self_continuity_store,
                relationship_constitution_store: import_ctx.relationship_constitution_store,
                relationship_portfolio_store: import_ctx.relationship_portfolio_store,
            },
            target_chat_id,
            snapshot,
            ContinuitySnapshotImportMode::FullRestore,
        ) {
            Ok(outcome) => {
                if outcome.summary_restored
                    || outcome.self_model_restored
                    || outcome.self_authored_core_restored
                    || outcome.core_revision_ledger_restored
                    || outcome.self_continuity_restored
                    || outcome.relationship_constitution_restored
                    || outcome.relationship_portfolio_restored
                    || outcome.execution_state_restored
                    || outcome.long_term_imported > 0
                {
                    restored_snapshots = restored_snapshots.saturating_add(1);
                }
                if outcome.self_model_restored {
                    restored_layers.push("self_model".to_string());
                }
                if outcome.self_authored_core_restored {
                    restored_layers.push("self_authored_core".to_string());
                }
                if outcome.core_revision_ledger_restored {
                    restored_layers.push("core_revision_ledger".to_string());
                }
                if outcome.self_continuity_restored {
                    restored_layers.push("self_continuity".to_string());
                }
                if outcome.relationship_constitution_restored {
                    restored_layers.push("relationship_constitution".to_string());
                }
                if outcome.relationship_portfolio_restored {
                    restored_layers.push("relationship_portfolio".to_string());
                }
                if outcome.execution_state_restored {
                    restored_layers.push("execution_state".to_string());
                }
                if outcome.summary_restored {
                    restored_layers.push("session_summary".to_string());
                }
                if outcome.long_term_imported > 0 {
                    restored_layers.push("key_memory".to_string());
                }
            }
            Err(error) => errors.push(format!(
                "runtime_bundle_import_failed:{}:{}",
                target_chat_id, error
            )),
        }
    }
    dedup_strings(&mut restored_layers);

    let status_after = inspect_soul_kernel_with_runtime_bundle_status(
        SoulKernelInspectContext {
            state_fs: ctx.inspect.state_fs,
            session_store: ctx.inspect.session_store,
            long_term_memory_store: ctx.inspect.long_term_memory_store,
            self_model_store: ctx.inspect.self_model_store,
            self_authored_core_store: ctx.inspect.self_authored_core_store,
            core_revision_ledger_store: ctx.inspect.core_revision_ledger_store,
            self_continuity_store: ctx.inspect.self_continuity_store,
            relationship_portfolio_store: ctx.inspect.relationship_portfolio_store,
            relationship_topology_store: ctx.inspect.relationship_topology_store,
        },
        now_secs,
        runtime_bundle_status,
    );

    let action = if restored_snapshots > 0 || !restored_layers.is_empty() {
        SoulKernelRecoveryAction::RestoredFromRuntimeBundle
    } else {
        SoulKernelRecoveryAction::RestoreAttemptedNoChange
    };

    append_reboot_recovery_workflow_audit(
        crate::runtime::WorkflowDisposition::ExecuteNow,
        match action {
            SoulKernelRecoveryAction::RestoredFromRuntimeBundle => "soul_kernel_recovery_restored",
            SoulKernelRecoveryAction::RestoreAttemptedNoChange => {
                "soul_kernel_recovery_attempted_no_change"
            }
            SoulKernelRecoveryAction::NotNeeded
            | SoulKernelRecoveryAction::NoBundleAvailable
            | SoulKernelRecoveryAction::BundleUnreadable => "soul_kernel_recovery_completed",
        },
        crate::runtime::WorkflowEffect::RunRepairPass,
        crate::runtime::WorkflowRecoveryPolicy::ReplayAfterBoot,
        bundle.primary_chat_id.as_deref(),
    );

    SoulKernelRecoveryReport {
        action,
        restore_attempted: true,
        restored_snapshots,
        restored_layers,
        errors,
        status_before,
        status_after,
    }
}

fn load_layer_status<T>(
    read: impl FnOnce() -> crate::error::Result<Option<T>>,
    is_meaningful: impl Fn(&T) -> bool,
    updated_at: impl Fn(&T) -> u64,
) -> SoulKernelLayerStatus {
    match read() {
        Ok(Some(value)) => SoulKernelLayerStatus {
            readable: true,
            present: is_meaningful(&value),
            updated_at: Some(updated_at(&value)).filter(|value| *value > 0),
            error: None,
        },
        Ok(None) => SoulKernelLayerStatus {
            readable: true,
            present: false,
            updated_at: None,
            error: None,
        },
        Err(error) => SoulKernelLayerStatus {
            readable: false,
            present: false,
            updated_at: None,
            error: Some(error.to_string()),
        },
    }
}

fn count_key_memory(
    store: &dyn LongTermMemoryStore,
    active_chat_ids: &[String],
) -> (bool, usize, Option<String>) {
    match store.list(SOUL_KERNEL_KEY_MEMORY_SCAN_LIMIT) {
        Ok(entries) => (
            true,
            entries
                .into_iter()
                .filter(|entry| is_soul_kernel_key_memory(entry, active_chat_ids))
                .count(),
            None,
        ),
        Err(error) => (false, 0, Some(format!("key_memory_unreadable:{error}"))),
    }
}

fn is_soul_kernel_key_memory(entry: &LongTermMemoryEntry, active_chat_ids: &[String]) -> bool {
    if !matches!(
        entry.kind,
        LongTermMemoryKind::Relationship
            | LongTermMemoryKind::Profile
            | LongTermMemoryKind::Preference
            | LongTermMemoryKind::Constraint
            | LongTermMemoryKind::Project
            | LongTermMemoryKind::Fact
    ) {
        return false;
    }
    active_chat_ids.is_empty()
        || entry.source_chat_id.is_none()
        || active_chat_ids
            .iter()
            .any(|chat_id| entry.source_chat_id.as_deref() == Some(chat_id.as_str()))
}

fn read_runtime_bundle_status(state_fs: &dyn StateFs) -> SoulKernelRuntimeBundleStatus {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    if let Some(status) = cached_runtime_bundle_status() {
        return status;
    }

    let status = runtime_bundle_status_from_load_result(&load_runtime_bundle(state_fs));

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    update_runtime_bundle_status_cache(&status);

    status
}

fn runtime_bundle_status_from_load_result(
    result: &std::result::Result<Option<ContinuitySnapshotBundle>, String>,
) -> SoulKernelRuntimeBundleStatus {
    match result {
        Ok(Some(bundle)) => SoulKernelRuntimeBundleStatus {
            present: true,
            loadable: true,
            snapshot_count: bundle.snapshots.len(),
            primary_chat_id: bundle.primary_chat_id.clone(),
            reason: Some(bundle.reason.clone()),
            flushed_at: Some(bundle.flushed_at),
            error: None,
        },
        Ok(None) => SoulKernelRuntimeBundleStatus::default(),
        Err(error) => SoulKernelRuntimeBundleStatus {
            present: true,
            loadable: false,
            snapshot_count: 0,
            primary_chat_id: None,
            reason: None,
            flushed_at: None,
            error: Some(error.clone()),
        },
    }
}

fn load_runtime_bundle(
    state_fs: &dyn StateFs,
) -> std::result::Result<Option<ContinuitySnapshotBundle>, String> {
    let Some(bytes) = state_fs
        .read(REL_PATH_REBOOT_CONTINUITY_BUNDLE)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    serde_json::from_slice::<ContinuitySnapshotBundle>(&bytes)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn ordered_bundle_snapshots(
    bundle: &ContinuitySnapshotBundle,
) -> Vec<&crate::memory::ContinuitySnapshot> {
    let mut snapshots = bundle.snapshots.iter().collect::<Vec<_>>();
    if let Some(primary_chat_id) = bundle.primary_chat_id.as_deref() {
        snapshots.sort_by_key(|snapshot| (snapshot.chat_id.as_str() != primary_chat_id) as u8);
    }
    snapshots
}

fn push_layer_degradation(layer_name: &str, status: &SoulKernelLayerStatus, out: &mut Vec<String>) {
    if !status.readable {
        if let Some(error) = status.error.as_deref() {
            out.push(format!("{layer_name}_unreadable:{error}"));
        } else {
            out.push(format!("{layer_name}_unreadable"));
        }
    }
}

fn dedup_strings(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn soul_kernel_status_cache() -> &'static Mutex<Option<CachedSoulKernelStatus>> {
    SOUL_KERNEL_STATUS_CACHE.get_or_init(|| Mutex::new(None))
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn runtime_bundle_status_cache() -> &'static Mutex<Option<SoulKernelRuntimeBundleStatus>> {
    RUNTIME_BUNDLE_STATUS_CACHE.get_or_init(|| Mutex::new(None))
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn cached_platform_soul_kernel_status() -> Option<SoulKernelStatus> {
    let guard = soul_kernel_status_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let cached = guard.as_ref()?;
    (cached.cached_at.elapsed() <= SOUL_KERNEL_STATUS_CACHE_TTL).then(|| cached.status.clone())
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn cached_runtime_bundle_status() -> Option<SoulKernelRuntimeBundleStatus> {
    runtime_bundle_status_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn update_runtime_bundle_status_cache(status: &SoulKernelRuntimeBundleStatus) {
    let mut guard = runtime_bundle_status_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    *guard = Some(status.clone());
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn update_platform_soul_kernel_status_cache(status: &SoulKernelStatus) {
    let mut guard = soul_kernel_status_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    *guard = Some(CachedSoulKernelStatus {
        cached_at: Instant::now(),
        status: status.clone(),
    });
    update_runtime_bundle_status_cache(&status.runtime_bundle);
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn clear_platform_soul_kernel_status_cache() {
    let mut guard = soul_kernel_status_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    *guard = None;
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn clear_runtime_bundle_status_cache() {
    let mut guard = runtime_bundle_status_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    *guard = None;
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) fn invalidate_platform_soul_kernel_status_cache() {
    clear_platform_soul_kernel_status_cache();
    clear_runtime_bundle_status_cache();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{
        board_subject_scope_id, CoreRevisionLedger, ExecutionState, ExecutionStateStore,
        LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryKind, LongTermMemorySlot,
        RelationshipConstitution, RelationshipConstitutionStore, RelationshipPortfolio,
        RelationshipPortfolioStore, RelationshipTopology, RelationshipTopologyStore,
        SelfAuthoredCore, SelfAuthoredCoreStore, SelfContinuity, SelfContinuityStore, SelfModel,
        SelfModelStore, SessionMessage, SessionSummaryStore,
    };
    use crate::platform::StateFs;
    use crate::runtime::workflow::{reset_workflow_audit_for_tests, workflow_audit_snapshot};
    use std::collections::{BTreeMap, BTreeSet, HashMap};
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryStateFs {
        files: Mutex<BTreeMap<String, Vec<u8>>>,
    }

    impl StateFs for MemoryStateFs {
        fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(rel_path)
                .cloned())
        }

        fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove(&self, rel_path: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(rel_path);
            Ok(())
        }

        fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
            let prefix = if rel_path.is_empty() {
                String::new()
            } else {
                format!("{}/", rel_path.trim_end_matches('/'))
            };
            let files = self.files.lock().unwrap_or_else(|error| error.into_inner());
            let mut names = BTreeSet::new();
            for key in files.keys() {
                if !key.starts_with(&prefix) {
                    continue;
                }
                let tail = &key[prefix.len()..];
                if tail.is_empty() {
                    continue;
                }
                if let Some((dir, _)) = tail.split_once('/') {
                    names.insert(format!("{dir}/"));
                } else {
                    names.insert(tail.to_string());
                }
            }
            Ok(names.into_iter().collect())
        }
    }

    #[derive(Default)]
    struct CountingStateFs {
        inner: MemoryStateFs,
        reads: Mutex<HashMap<String, usize>>,
    }

    impl CountingStateFs {
        fn read_count(&self, rel_path: &str) -> usize {
            self.reads
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(rel_path)
                .copied()
                .unwrap_or(0)
        }
    }

    impl StateFs for CountingStateFs {
        fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            self.reads
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .entry(rel_path.to_string())
                .and_modify(|count| *count += 1)
                .or_insert(1);
            self.inner.read(rel_path)
        }

        fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.inner.write(rel_path, data)
        }

        fn remove(&self, rel_path: &str) -> Result<()> {
            self.inner.remove(rel_path)
        }

        fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
            self.inner.list_dir(rel_path)
        }
    }

    #[derive(Default)]
    struct TestSessionStore {
        chat_ids: Mutex<Vec<String>>,
    }

    impl SessionStore for TestSessionStore {
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
            Ok(self
                .chat_ids
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone())
        }
    }

    #[derive(Default)]
    struct TestLongTermStore {
        entries: Mutex<Vec<LongTermMemoryEntry>>,
    }

    impl LongTermMemoryStore for TestLongTermStore {
        fn upsert_many(&self, _drafts: &[LongTermMemoryDraft], _now_secs: u64) -> Result<usize> {
            Ok(0)
        }

        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .iter()
                .find(|entry| entry.id == id)
                .cloned())
        }

        fn list(&self, _limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len())
        }
    }

    #[derive(Default)]
    struct TestSelfModelStore {
        values: Mutex<HashMap<String, SelfModel>>,
    }

    impl SelfModelStore for TestSelfModelStore {
        fn get(&self, chat_id: &str) -> Result<Option<SelfModel>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, model: &SelfModel) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(chat_id.to_string(), model.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestSelfAuthoredCoreStore {
        values: Mutex<HashMap<String, SelfAuthoredCore>>,
    }

    impl SelfAuthoredCoreStore for TestSelfAuthoredCoreStore {
        fn get(&self, scope_id: &str) -> Result<Option<SelfAuthoredCore>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(scope_id)
                .cloned())
        }

        fn set(&self, scope_id: &str, core: &SelfAuthoredCore) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(scope_id.to_string(), core.clone());
            Ok(())
        }

        fn clear(&self, scope_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(scope_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestCoreRevisionLedgerStore {
        values: Mutex<HashMap<String, CoreRevisionLedger>>,
    }

    impl CoreRevisionLedgerStore for TestCoreRevisionLedgerStore {
        fn get(&self, scope_id: &str) -> Result<Option<CoreRevisionLedger>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(scope_id)
                .cloned())
        }

        fn set(&self, scope_id: &str, ledger: &CoreRevisionLedger) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(scope_id.to_string(), ledger.clone());
            Ok(())
        }

        fn clear(&self, scope_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(scope_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestSelfContinuityStore {
        values: Mutex<HashMap<String, SelfContinuity>>,
    }

    impl SelfContinuityStore for TestSelfContinuityStore {
        fn get(&self, chat_id: &str) -> Result<Option<SelfContinuity>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, continuity: &SelfContinuity) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(chat_id.to_string(), continuity.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestRelationshipPortfolioStore {
        values: Mutex<HashMap<String, RelationshipPortfolio>>,
    }

    impl RelationshipPortfolioStore for TestRelationshipPortfolioStore {
        fn get(&self, scope_id: &str) -> Result<Option<RelationshipPortfolio>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(scope_id)
                .cloned())
        }

        fn set(&self, scope_id: &str, portfolio: &RelationshipPortfolio) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(scope_id.to_string(), portfolio.clone());
            Ok(())
        }

        fn clear(&self, scope_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(scope_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestRelationshipTopologyStore {
        values: Mutex<HashMap<String, RelationshipTopology>>,
    }

    impl RelationshipTopologyStore for TestRelationshipTopologyStore {
        fn get(&self, scope_id: &str) -> Result<Option<RelationshipTopology>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(scope_id)
                .cloned())
        }

        fn set(&self, scope_id: &str, topology: &RelationshipTopology) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(scope_id.to_string(), topology.clone());
            Ok(())
        }

        fn clear(&self, scope_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(scope_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestSessionSummaryStore {
        values: Mutex<HashMap<String, (String, usize)>>,
    }

    impl SessionSummaryStore for TestSessionSummaryStore {
        fn get(&self, chat_id: &str) -> Result<Option<String>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(chat_id)
                .map(|(summary, _)| summary.clone()))
        }

        fn set(&self, chat_id: &str, summary: &str) -> Result<()> {
            self.set_with_count(chat_id, summary, 0)
        }

        fn set_with_count(&self, chat_id: &str, summary: &str, message_count: usize) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(chat_id.to_string(), (summary.to_string(), message_count));
            Ok(())
        }

        fn get_with_count(&self, chat_id: &str) -> Result<Option<(String, usize)>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(chat_id)
                .cloned())
        }
    }

    #[derive(Default)]
    struct TestExecutionStateStore {
        values: Mutex<HashMap<String, ExecutionState>>,
    }

    impl ExecutionStateStore for TestExecutionStateStore {
        fn get(&self, chat_id: &str) -> Result<Option<ExecutionState>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, state: &ExecutionState) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(chat_id.to_string(), state.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestRelationshipConstitutionStore {
        values: Mutex<HashMap<String, RelationshipConstitution>>,
    }

    impl RelationshipConstitutionStore for TestRelationshipConstitutionStore {
        fn get(&self, scope_id: &str) -> Result<Option<RelationshipConstitution>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(scope_id)
                .cloned())
        }

        fn set(&self, scope_id: &str, constitution: &RelationshipConstitution) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(scope_id.to_string(), constitution.clone());
            Ok(())
        }

        fn clear(&self, scope_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(scope_id);
            Ok(())
        }
    }

    struct TestInspectStores<'a> {
        state_fs: &'a dyn StateFs,
        session_store: &'a TestSessionStore,
        long_term_store: &'a TestLongTermStore,
        self_model_store: &'a TestSelfModelStore,
        self_authored_core_store: &'a TestSelfAuthoredCoreStore,
        core_revision_ledger_store: &'a TestCoreRevisionLedgerStore,
        self_continuity_store: &'a TestSelfContinuityStore,
        relationship_portfolio_store: &'a TestRelationshipPortfolioStore,
        relationship_topology_store: &'a TestRelationshipTopologyStore,
    }

    fn inspect_ctx(stores: TestInspectStores<'_>) -> SoulKernelInspectContext<'_> {
        SoulKernelInspectContext {
            state_fs: stores.state_fs,
            session_store: stores.session_store,
            long_term_memory_store: stores.long_term_store,
            self_model_store: stores.self_model_store,
            self_authored_core_store: stores.self_authored_core_store,
            core_revision_ledger_store: stores.core_revision_ledger_store,
            self_continuity_store: stores.self_continuity_store,
            relationship_portfolio_store: stores.relationship_portfolio_store,
            relationship_topology_store: stores.relationship_topology_store,
        }
    }

    #[test]
    fn inspect_marks_bootstrap_empty_when_no_kernel_assets_exist() {
        let state_fs = MemoryStateFs::default();
        let session_store = TestSessionStore::default();
        let long_term_store = TestLongTermStore::default();
        let self_model_store = TestSelfModelStore::default();
        let self_authored_core_store = TestSelfAuthoredCoreStore::default();
        let core_revision_ledger_store = TestCoreRevisionLedgerStore::default();
        let self_continuity_store = TestSelfContinuityStore::default();
        let relationship_portfolio_store = TestRelationshipPortfolioStore::default();
        let relationship_topology_store = TestRelationshipTopologyStore::default();

        let status = inspect_soul_kernel(
            inspect_ctx(TestInspectStores {
                state_fs: &state_fs,
                session_store: &session_store,
                long_term_store: &long_term_store,
                self_model_store: &self_model_store,
                self_authored_core_store: &self_authored_core_store,
                core_revision_ledger_store: &core_revision_ledger_store,
                self_continuity_store: &self_continuity_store,
                relationship_portfolio_store: &relationship_portfolio_store,
                relationship_topology_store: &relationship_topology_store,
            }),
            100,
        );

        assert!(status.expected_bootstrap_empty);
        assert!(!status.degraded);
        assert!(!status.minimum_viable);
    }

    #[test]
    fn inspect_marks_corrupt_runtime_bundle_as_degraded() {
        let state_fs = MemoryStateFs::default();
        state_fs
            .write(REL_PATH_REBOOT_CONTINUITY_BUNDLE, b"{not json")
            .unwrap();
        let session_store = TestSessionStore::default();
        session_store
            .chat_ids
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push("chat-a".to_string());
        let long_term_store = TestLongTermStore::default();
        let self_model_store = TestSelfModelStore::default();
        let self_authored_core_store = TestSelfAuthoredCoreStore::default();
        let core_revision_ledger_store = TestCoreRevisionLedgerStore::default();
        let self_continuity_store = TestSelfContinuityStore::default();
        let relationship_portfolio_store = TestRelationshipPortfolioStore::default();
        let relationship_topology_store = TestRelationshipTopologyStore::default();

        let status = inspect_soul_kernel(
            inspect_ctx(TestInspectStores {
                state_fs: &state_fs,
                session_store: &session_store,
                long_term_store: &long_term_store,
                self_model_store: &self_model_store,
                self_authored_core_store: &self_authored_core_store,
                core_revision_ledger_store: &core_revision_ledger_store,
                self_continuity_store: &self_continuity_store,
                relationship_portfolio_store: &relationship_portfolio_store,
                relationship_topology_store: &relationship_topology_store,
            }),
            100,
        );

        assert!(status.degraded);
        assert!(status.runtime_bundle.present);
        assert!(!status.runtime_bundle.loadable);
    }

    #[test]
    fn restore_runtime_bundle_repairs_missing_core_and_continuity() {
        let _guard = crate::runtime::workflow_audit_test_guard();
        reset_workflow_audit_for_tests();
        let state_fs = MemoryStateFs::default();
        let session_store = TestSessionStore::default();
        session_store
            .chat_ids
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push("chat-a".to_string());
        let long_term_store = TestLongTermStore::default();
        long_term_store
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(LongTermMemoryEntry {
                id: "pref-1".to_string(),
                kind: LongTermMemoryKind::Preference,
                topic: "sleep".to_string(),
                content: "prefer calm reminders".to_string(),
                keywords: vec!["sleep".to_string()],
                source_chat_id: Some("chat-a".to_string()),
                source_type: Default::default(),
                source_scope: Default::default(),
                confidence: Default::default(),
                freshness: Default::default(),
                stale_hint: Default::default(),
                supporting_citations: Vec::new(),
                evidence_count: 1,
                created_at: 10,
                updated_at: 20,
                observed_at: 20,
                last_confirmed_at: 20,
                source_revision: 1,
                last_used_at: 0,
            });
        let self_model_store = TestSelfModelStore::default();
        let self_authored_core_store = TestSelfAuthoredCoreStore::default();
        let core_revision_ledger_store = TestCoreRevisionLedgerStore::default();
        let self_continuity_store = TestSelfContinuityStore::default();
        let relationship_portfolio_store = TestRelationshipPortfolioStore::default();
        let relationship_topology_store = TestRelationshipTopologyStore::default();
        let session_summary_store = TestSessionSummaryStore::default();
        let execution_state_store = TestExecutionStateStore::default();
        let relationship_constitution_store = TestRelationshipConstitutionStore::default();

        let subject_id = board_subject_scope_id().to_string();
        self_model_store
            .set(
                &subject_id,
                &SelfModel {
                    continuity_anchor: "steady self".to_string(),
                    updated_at: 90,
                    ..SelfModel::default()
                },
            )
            .unwrap();
        self_authored_core_store
            .set(
                &subject_id,
                &SelfAuthoredCore {
                    identity_anchor: "board self".to_string(),
                    updated_at: 90,
                    ..SelfAuthoredCore::default()
                },
            )
            .unwrap();
        self_continuity_store
            .set(
                &subject_id,
                &SelfContinuity {
                    current_self_state: "still here".to_string(),
                    last_user_chat_id: "chat-a".to_string(),
                    last_user_channel: "telegram".to_string(),
                    updated_at: 90,
                    ..SelfContinuity::default()
                },
            )
            .unwrap();

        let snapshot = crate::memory::export_continuity_snapshot(
            crate::memory::ContinuitySnapshotExportContext {
                long_term_memory_store: &long_term_store,
                session_summary_store: &session_summary_store,
                execution_state_store: &execution_state_store,
                self_model_store: &self_model_store,
                self_authored_core_store: &self_authored_core_store,
                core_revision_ledger_store: &core_revision_ledger_store,
                self_continuity_store: &self_continuity_store,
                relationship_constitution_store: &relationship_constitution_store,
                relationship_portfolio_store: &relationship_portfolio_store,
                relationship_topology_store: &relationship_topology_store,
            },
            "chat-a",
            crate::memory::ContinuitySnapshotMode::FullRestore,
            100,
        )
        .unwrap();

        let bundle = ContinuitySnapshotBundle {
            version: 1,
            reason: "agent_exit".to_string(),
            flushed_at: 100,
            primary_chat_id: Some("chat-a".to_string()),
            snapshots: vec![snapshot],
        };
        state_fs
            .write(
                REL_PATH_REBOOT_CONTINUITY_BUNDLE,
                serde_json::to_vec(&bundle).unwrap().as_slice(),
            )
            .unwrap();

        self_model_store.clear(&subject_id).unwrap();
        self_authored_core_store.clear(&subject_id).unwrap();
        self_continuity_store.clear(&subject_id).unwrap();

        let report = ensure_soul_kernel_recovery(
            SoulKernelRecoveryContext {
                inspect: inspect_ctx(TestInspectStores {
                    state_fs: &state_fs,
                    session_store: &session_store,
                    long_term_store: &long_term_store,
                    self_model_store: &self_model_store,
                    self_authored_core_store: &self_authored_core_store,
                    core_revision_ledger_store: &core_revision_ledger_store,
                    self_continuity_store: &self_continuity_store,
                    relationship_portfolio_store: &relationship_portfolio_store,
                    relationship_topology_store: &relationship_topology_store,
                }),
                session_summary_store: &session_summary_store,
                execution_state_store: &execution_state_store,
                relationship_constitution_store: &relationship_constitution_store,
            },
            120,
        );

        assert_eq!(
            report.action,
            SoulKernelRecoveryAction::RestoredFromRuntimeBundle
        );
        assert!(report.status_after.minimum_viable);
        assert!(self_model_store.get(&subject_id).unwrap().is_some());
        assert!(self_authored_core_store.get(&subject_id).unwrap().is_some());
        assert!(self_continuity_store.get(&subject_id).unwrap().is_some());
        let audit = workflow_audit_snapshot(4);
        assert!(audit.summary.executed >= 1);
        assert!(audit
            .recent_records
            .iter()
            .any(|record| record.workflow == crate::runtime::WorkflowKind::RebootRecovery));
    }

    #[test]
    fn recovery_without_runtime_bundle_records_no_trigger_workflow_audit() {
        let _guard = crate::runtime::workflow_audit_test_guard();
        reset_workflow_audit_for_tests();
        let state_fs = MemoryStateFs::default();
        let session_store = TestSessionStore::default();
        session_store
            .chat_ids
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push("chat-a".to_string());
        let long_term_store = TestLongTermStore::default();
        let self_model_store = TestSelfModelStore::default();
        let self_authored_core_store = TestSelfAuthoredCoreStore::default();
        let core_revision_ledger_store = TestCoreRevisionLedgerStore::default();
        let self_continuity_store = TestSelfContinuityStore::default();
        let relationship_portfolio_store = TestRelationshipPortfolioStore::default();
        let relationship_topology_store = TestRelationshipTopologyStore::default();
        let session_summary_store = TestSessionSummaryStore::default();
        let execution_state_store = TestExecutionStateStore::default();
        let relationship_constitution_store = TestRelationshipConstitutionStore::default();

        let report = ensure_soul_kernel_recovery(
            SoulKernelRecoveryContext {
                inspect: inspect_ctx(TestInspectStores {
                    state_fs: &state_fs,
                    session_store: &session_store,
                    long_term_store: &long_term_store,
                    self_model_store: &self_model_store,
                    self_authored_core_store: &self_authored_core_store,
                    core_revision_ledger_store: &core_revision_ledger_store,
                    self_continuity_store: &self_continuity_store,
                    relationship_portfolio_store: &relationship_portfolio_store,
                    relationship_topology_store: &relationship_topology_store,
                }),
                session_summary_store: &session_summary_store,
                execution_state_store: &execution_state_store,
                relationship_constitution_store: &relationship_constitution_store,
            },
            120,
        );

        assert_eq!(report.action, SoulKernelRecoveryAction::NoBundleAvailable);
        let audit = workflow_audit_snapshot(4);
        assert_eq!(audit.summary.no_trigger, 1);
        assert_eq!(
            audit.recent_records[0].workflow,
            crate::runtime::WorkflowKind::RebootRecovery
        );
    }

    #[test]
    fn recovery_reads_runtime_bundle_only_once() {
        let state_fs = CountingStateFs::default();
        let session_store = TestSessionStore::default();
        session_store
            .chat_ids
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push("chat-a".to_string());
        let long_term_store = TestLongTermStore::default();
        let self_model_store = TestSelfModelStore::default();
        let self_authored_core_store = TestSelfAuthoredCoreStore::default();
        let core_revision_ledger_store = TestCoreRevisionLedgerStore::default();
        let self_continuity_store = TestSelfContinuityStore::default();
        let relationship_portfolio_store = TestRelationshipPortfolioStore::default();
        let relationship_topology_store = TestRelationshipTopologyStore::default();
        let session_summary_store = TestSessionSummaryStore::default();
        let execution_state_store = TestExecutionStateStore::default();
        let relationship_constitution_store = TestRelationshipConstitutionStore::default();

        let subject_id = board_subject_scope_id().to_string();
        self_model_store
            .set(
                &subject_id,
                &SelfModel {
                    continuity_anchor: "steady self".to_string(),
                    updated_at: 90,
                    ..SelfModel::default()
                },
            )
            .unwrap();
        self_authored_core_store
            .set(
                &subject_id,
                &SelfAuthoredCore {
                    identity_anchor: "board self".to_string(),
                    updated_at: 90,
                    ..SelfAuthoredCore::default()
                },
            )
            .unwrap();
        self_continuity_store
            .set(
                &subject_id,
                &SelfContinuity {
                    current_self_state: "still here".to_string(),
                    last_user_chat_id: "chat-a".to_string(),
                    last_user_channel: "telegram".to_string(),
                    updated_at: 90,
                    ..SelfContinuity::default()
                },
            )
            .unwrap();

        let snapshot = crate::memory::export_continuity_snapshot(
            crate::memory::ContinuitySnapshotExportContext {
                long_term_memory_store: &long_term_store,
                session_summary_store: &session_summary_store,
                execution_state_store: &execution_state_store,
                self_model_store: &self_model_store,
                self_authored_core_store: &self_authored_core_store,
                core_revision_ledger_store: &core_revision_ledger_store,
                self_continuity_store: &self_continuity_store,
                relationship_constitution_store: &relationship_constitution_store,
                relationship_portfolio_store: &relationship_portfolio_store,
                relationship_topology_store: &relationship_topology_store,
            },
            "chat-a",
            crate::memory::ContinuitySnapshotMode::FullRestore,
            100,
        )
        .unwrap();

        let bundle = ContinuitySnapshotBundle {
            version: 1,
            reason: "agent_exit".to_string(),
            flushed_at: 100,
            primary_chat_id: Some("chat-a".to_string()),
            snapshots: vec![snapshot],
        };
        state_fs
            .write(
                REL_PATH_REBOOT_CONTINUITY_BUNDLE,
                serde_json::to_vec(&bundle).unwrap().as_slice(),
            )
            .unwrap();

        self_model_store.clear(&subject_id).unwrap();
        self_authored_core_store.clear(&subject_id).unwrap();
        self_continuity_store.clear(&subject_id).unwrap();

        let _ = ensure_soul_kernel_recovery(
            SoulKernelRecoveryContext {
                inspect: inspect_ctx(TestInspectStores {
                    state_fs: &state_fs,
                    session_store: &session_store,
                    long_term_store: &long_term_store,
                    self_model_store: &self_model_store,
                    self_authored_core_store: &self_authored_core_store,
                    core_revision_ledger_store: &core_revision_ledger_store,
                    self_continuity_store: &self_continuity_store,
                    relationship_portfolio_store: &relationship_portfolio_store,
                    relationship_topology_store: &relationship_topology_store,
                }),
                session_summary_store: &session_summary_store,
                execution_state_store: &execution_state_store,
                relationship_constitution_store: &relationship_constitution_store,
            },
            120,
        );

        assert_eq!(state_fs.read_count(REL_PATH_REBOOT_CONTINUITY_BUNDLE), 1);
    }
}
