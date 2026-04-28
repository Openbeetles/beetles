//! Runtime continuity flush helpers for reboot/handoff boundaries.

use crate::error::{Error, Result};
use crate::memory::{
    export_continuity_snapshot, render_continuity_snapshot_markdown,
    select_active_continuity_snapshot_chat_ids, ContinuitySnapshot,
    ContinuitySnapshotExportContext, ContinuitySnapshotMode,
};
use crate::Platform;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CONTINUITY_FLUSH_BUNDLE_VERSION: u32 = 1;
const CONTINUITY_FLUSH_ACTIVE_WINDOW_SECS: u64 = 7 * 86_400;
const CONTINUITY_FLUSH_MAX_CHATS: usize = 4;
pub const REL_PATH_REBOOT_CONTINUITY_BUNDLE: &str =
    "memory/continuity_snapshots/runtime/latest_reboot_bundle.json";
pub const REL_PATH_REBOOT_CONTINUITY_MARKDOWN: &str =
    "memory/continuity_snapshots/runtime/latest_reboot_bundle.md";
static DELAYED_RESTART_SCHEDULED: AtomicBool = AtomicBool::new(false);
static DELAYED_RESTART_SCHEDULE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuitySnapshotBundle {
    pub version: u32,
    pub reason: String,
    pub flushed_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snapshots: Vec<ContinuitySnapshot>,
}

fn append_reboot_request_workflow_audit(
    disposition: crate::runtime::WorkflowDisposition,
    rationale: &str,
    effect: crate::runtime::WorkflowEffect,
    primary_chat_id: Option<&str>,
) {
    crate::runtime::append_workflow_audit(
        crate::runtime::WorkflowAuditRecord::new(
            crate::runtime::WorkflowKind::RebootRecovery,
            crate::runtime::WorkflowTrigger::ModeTransition,
            disposition,
            effect,
            crate::runtime::WorkflowRecoveryPolicy::ReplayAfterBoot,
            rationale,
            crate::util::current_unix_secs(),
        )
        .with_target(None, None, primary_chat_id),
    );
}

pub fn flush_reboot_continuity_bundle(
    platform: &dyn Platform,
    preferred_chat_id: Option<&str>,
    reason: &str,
    now_secs: u64,
) -> Result<usize> {
    let normalized_reason = normalize_restart_reason(reason);
    flush_reboot_continuity_bundle_with_reason(
        platform,
        preferred_chat_id,
        &normalized_reason,
        now_secs,
    )
}

fn flush_reboot_continuity_bundle_with_reason(
    platform: &dyn Platform,
    preferred_chat_id: Option<&str>,
    normalized_reason: &str,
    now_secs: u64,
) -> Result<usize> {
    let session_store = platform.session_store();
    let self_continuity_store = platform.self_continuity_store();
    let relationship_portfolio_store = platform.relationship_portfolio_store();
    let relationship_topology_store = platform.relationship_topology_store();
    let relationship_constitution_store = platform.relationship_constitution_store();
    let long_term_memory_store = platform.long_term_memory_store();
    let session_summary_store = platform.session_summary_store();
    let execution_state_store = platform.execution_state_store();
    let self_model_store = platform.self_model_store();
    let self_authored_core_store = platform.self_authored_core_store();
    let core_revision_ledger_store = platform.core_revision_ledger_store();
    let chat_ids = select_active_continuity_snapshot_chat_ids(
        session_store.as_ref(),
        self_continuity_store.as_ref(),
        relationship_portfolio_store.as_ref(),
        relationship_topology_store.as_ref(),
        preferred_chat_id,
        now_secs,
        CONTINUITY_FLUSH_ACTIVE_WINDOW_SECS,
        CONTINUITY_FLUSH_MAX_CHATS,
    );
    if chat_ids.is_empty() {
        return Ok(0);
    }
    let export_ctx = ContinuitySnapshotExportContext {
        long_term_memory_store: long_term_memory_store.as_ref(),
        session_summary_store: session_summary_store.as_ref(),
        execution_state_store: execution_state_store.as_ref(),
        self_model_store: self_model_store.as_ref(),
        self_authored_core_store: self_authored_core_store.as_ref(),
        core_revision_ledger_store: core_revision_ledger_store.as_ref(),
        self_continuity_store: self_continuity_store.as_ref(),
        relationship_constitution_store: relationship_constitution_store.as_ref(),
        relationship_portfolio_store: relationship_portfolio_store.as_ref(),
        relationship_topology_store: relationship_topology_store.as_ref(),
    };
    let mut snapshots = Vec::with_capacity(chat_ids.len());
    for chat_id in &chat_ids {
        match export_continuity_snapshot(
            ContinuitySnapshotExportContext {
                long_term_memory_store: export_ctx.long_term_memory_store,
                session_summary_store: export_ctx.session_summary_store,
                execution_state_store: export_ctx.execution_state_store,
                self_model_store: export_ctx.self_model_store,
                self_authored_core_store: export_ctx.self_authored_core_store,
                core_revision_ledger_store: export_ctx.core_revision_ledger_store,
                self_continuity_store: export_ctx.self_continuity_store,
                relationship_constitution_store: export_ctx.relationship_constitution_store,
                relationship_portfolio_store: export_ctx.relationship_portfolio_store,
                relationship_topology_store: export_ctx.relationship_topology_store,
            },
            chat_id,
            ContinuitySnapshotMode::FullRestore,
            now_secs,
        ) {
            Ok(snapshot) => snapshots.push(snapshot),
            Err(error) => {
                log::warn!(
                    "[continuity_flush] export snapshot failed chat_id={}: {}",
                    chat_id,
                    error
                );
            }
        }
    }
    if snapshots.is_empty() {
        return Ok(0);
    }
    let bundle = ContinuitySnapshotBundle {
        version: CONTINUITY_FLUSH_BUNDLE_VERSION,
        reason: normalized_reason.to_string(),
        flushed_at: now_secs,
        primary_chat_id: preferred_chat_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        snapshots,
    };
    let payload = serde_json::to_vec_pretty(&bundle)
        .map_err(|error| Error::config("continuity_flush", error.to_string()))?;
    platform
        .state_fs()
        .write(REL_PATH_REBOOT_CONTINUITY_BUNDLE, &payload)?;
    let markdown = render_reboot_continuity_bundle_markdown(&bundle);
    platform
        .state_fs()
        .write(REL_PATH_REBOOT_CONTINUITY_MARKDOWN, markdown.as_bytes())?;
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    crate::runtime::soul_kernel::invalidate_platform_soul_kernel_status_cache();
    Ok(bundle.snapshots.len())
}

pub fn request_restart_with_continuity_flush(
    platform: Arc<dyn Platform>,
    preferred_chat_id: Option<&str>,
    reason: &str,
) {
    let now_secs = crate::util::current_unix_secs();
    let normalized_reason = normalize_restart_reason(reason);
    match flush_reboot_continuity_bundle_with_reason(
        platform.as_ref(),
        preferred_chat_id,
        normalized_reason.as_str(),
        now_secs,
    ) {
        Ok(count) => {
            if count > 0 {
                log::info!(
                    "[continuity_flush] reboot bundle flushed reason={} snapshots={}",
                    normalized_reason,
                    count
                );
                append_reboot_request_workflow_audit(
                    crate::runtime::WorkflowDisposition::ExecuteNow,
                    "reboot_bundle_flushed",
                    crate::runtime::WorkflowEffect::PersistRecoveryIntent,
                    preferred_chat_id,
                );
            } else {
                log::info!(
                    "[continuity_flush] reboot requested reason={} snapshots=0",
                    normalized_reason
                );
                append_reboot_request_workflow_audit(
                    crate::runtime::WorkflowDisposition::NoTrigger,
                    "reboot_bundle_empty",
                    crate::runtime::WorkflowEffect::Noop,
                    preferred_chat_id,
                );
            }
        }
        Err(error) => {
            log::warn!(
                "[continuity_flush] reboot flush failed reason={}: {}",
                normalized_reason,
                error
            );
            append_reboot_request_workflow_audit(
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "reboot_bundle_flush_failed",
                crate::runtime::WorkflowEffect::Noop,
                preferred_chat_id,
            );
        }
    }
    append_reboot_request_workflow_audit(
        crate::runtime::WorkflowDisposition::ExecuteNow,
        normalized_reason.as_str(),
        crate::runtime::WorkflowEffect::RequestRestart,
        preferred_chat_id,
    );
    platform.request_restart();
}

/// Schedule a restart through the existing runtime delayed-task coordinator.
///
/// The caller may be an HTTP response path with very little internal heap left;
/// this function must not create a new thread. The runtime background timer
/// executes the already-queued restart closure after the response has left the
/// route worker.
pub fn schedule_restart_with_continuity_flush(
    platform: Arc<dyn Platform>,
    reason: String,
    delay: Duration,
) -> bool {
    let _guard = DELAYED_RESTART_SCHEDULE_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if DELAYED_RESTART_SCHEDULED.load(Ordering::Acquire) {
        log::info!(
            "[continuity_flush] delayed restart already scheduled; coalescing reason={}",
            reason
        );
        return true;
    }
    DELAYED_RESTART_SCHEDULED.store(true, Ordering::Release);
    schedule_delayed_restart_task(platform, reason, delay)
}

fn schedule_delayed_restart_task(
    platform: Arc<dyn Platform>,
    reason: String,
    delay: Duration,
) -> bool {
    let due_at = Instant::now() + delay;
    let log_reason = reason.clone();
    let task = Box::new(move || {
        DELAYED_RESTART_SCHEDULED.store(false, Ordering::Release);
        request_restart_with_continuity_flush(platform, None, reason.as_str());
    });
    match crate::runtime::schedule_critical_delayed_task(due_at, task) {
        Ok(()) => true,
        Err(_task) => {
            DELAYED_RESTART_SCHEDULED.store(false, Ordering::Release);
            log::error!(
                "[continuity_flush] failed to schedule delayed restart reason={}",
                log_reason
            );
            false
        }
    }
}

fn normalize_restart_reason(reason: &str) -> String {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        "restart_requested".to_string()
    } else {
        trimmed.to_string()
    }
}

fn render_reboot_continuity_bundle_markdown(bundle: &ContinuitySnapshotBundle) -> String {
    let mut out = String::from("# Reboot Continuity Flush\n");
    out.push_str(&format!("- reason: {}\n", bundle.reason));
    out.push_str(&format!("- flushed_at: {}\n", bundle.flushed_at));
    if let Some(primary_chat_id) = bundle.primary_chat_id.as_deref() {
        out.push_str(&format!("- primary_chat_id: {}\n", primary_chat_id));
    }
    for snapshot in &bundle.snapshots {
        out.push_str("\n---\n\n");
        out.push_str(&render_continuity_snapshot_markdown(snapshot));
        out.push('\n');
    }
    out
}
