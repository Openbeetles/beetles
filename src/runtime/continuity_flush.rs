//! Runtime continuity flush helpers for reboot/handoff boundaries.

use crate::error::{Error, Result};
use crate::memory::{
    export_continuity_snapshot, render_continuity_snapshot_markdown,
    select_active_continuity_snapshot_chat_ids, ContinuitySnapshot,
    ContinuitySnapshotExportContext, ContinuitySnapshotMode,
};
use crate::Platform;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const CONTINUITY_FLUSH_BUNDLE_VERSION: u32 = 1;
const CONTINUITY_FLUSH_ACTIVE_WINDOW_SECS: u64 = 7 * 86_400;
const CONTINUITY_FLUSH_MAX_CHATS: usize = 4;
pub const REL_PATH_REBOOT_CONTINUITY_BUNDLE: &str =
    "memory/continuity_snapshots/runtime/latest_reboot_bundle.json";
pub const REL_PATH_REBOOT_CONTINUITY_MARKDOWN: &str =
    "memory/continuity_snapshots/runtime/latest_reboot_bundle.md";

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

pub fn flush_reboot_continuity_bundle(
    platform: &dyn Platform,
    preferred_chat_id: Option<&str>,
    reason: &str,
    now_secs: u64,
) -> Result<usize> {
    let session_store = platform.session_store();
    let self_continuity_store = platform.self_continuity_store();
    let long_term_memory_store = platform.long_term_memory_store();
    let session_summary_store = platform.session_summary_store();
    let execution_state_store = platform.execution_state_store();
    let self_model_store = platform.self_model_store();
    let chat_ids = select_active_continuity_snapshot_chat_ids(
        session_store.as_ref(),
        self_continuity_store.as_ref(),
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
        self_continuity_store: self_continuity_store.as_ref(),
    };
    let mut snapshots = Vec::with_capacity(chat_ids.len());
    for chat_id in &chat_ids {
        match export_continuity_snapshot(
            ContinuitySnapshotExportContext {
                long_term_memory_store: export_ctx.long_term_memory_store,
                session_summary_store: export_ctx.session_summary_store,
                execution_state_store: export_ctx.execution_state_store,
                self_model_store: export_ctx.self_model_store,
                self_continuity_store: export_ctx.self_continuity_store,
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
        reason: normalize_reason(reason),
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
    Ok(bundle.snapshots.len())
}

pub fn request_restart_with_continuity_flush(
    platform: Arc<dyn Platform>,
    preferred_chat_id: Option<&str>,
    reason: &str,
) {
    let now_secs = crate::util::current_unix_secs();
    match flush_reboot_continuity_bundle(platform.as_ref(), preferred_chat_id, reason, now_secs) {
        Ok(count) => {
            if count > 0 {
                log::info!(
                    "[continuity_flush] reboot bundle flushed reason={} snapshots={}",
                    normalize_reason(reason),
                    count
                );
            } else {
                log::info!(
                    "[continuity_flush] reboot requested reason={} snapshots=0",
                    normalize_reason(reason)
                );
            }
        }
        Err(error) => {
            log::warn!(
                "[continuity_flush] reboot flush failed reason={}: {}",
                normalize_reason(reason),
                error
            );
        }
    }
    platform.request_restart();
}

fn normalize_reason(reason: &str) -> String {
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
