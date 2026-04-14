//! Structured operator-requested maintenance workflow bridge.

use crate::bus::{PcMsg, SystemInboundTx};
use crate::error::{Error, Result};
use crate::memory::board_subject_scope_id;
use crate::runtime::{
    append_workflow_audit, WorkflowAuditRecord, WorkflowDisposition, WorkflowEffect, WorkflowKind,
    WorkflowRecoveryPolicy, WorkflowTrigger,
};
use serde::{Deserialize, Serialize};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use std::path::PathBuf;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

pub const CHANNEL_OPERATOR_MAINTENANCE: &str = "_operator_maintenance";
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const REL_DIR_OPERATOR_MAINTENANCE_REQUESTS: &str = "runtime/operator_maintenance/requests";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperatorMaintenanceAction {
    RunRepairPlan,
    RebuildContinuitySnapshot,
    ReconcileRelationshipGovernance,
    ReplayRecovery,
    RefreshOperatorDigest,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorMaintenanceRequest {
    pub request_id: String,
    pub action: OperatorMaintenanceAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    pub requested_at: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub requested_via: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct OperatorMaintenanceSubmission {
    pub accepted: bool,
    pub request_id: String,
    pub action: OperatorMaintenanceAction,
    pub delivery: &'static str,
}

impl OperatorMaintenanceRequest {
    pub fn new(
        action: OperatorMaintenanceAction,
        chat_id: Option<String>,
        channel: Option<String>,
        requested_via: impl Into<String>,
    ) -> Self {
        Self {
            request_id: new_request_id(),
            action,
            chat_id: chat_id
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            channel: channel
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            requested_at: crate::util::current_unix_secs(),
            requested_via: requested_via.into().trim().to_string(),
        }
    }

    pub fn queue_chat_id(&self) -> &str {
        if let Some(chat_id) = self
            .chat_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            chat_id
        } else {
            board_subject_scope_id()
        }
    }
}

pub fn submit_operator_maintenance_request(
    system_inbound_tx: Option<&SystemInboundTx>,
    request: OperatorMaintenanceRequest,
) -> Result<OperatorMaintenanceSubmission> {
    if let Some(system_inbound_tx) = system_inbound_tx {
        let msg = build_operator_maintenance_msg(&request)?;
        match system_inbound_tx.try_send(msg) {
            Ok(()) => {
                append_operator_request_workflow_audit(
                    &request,
                    WorkflowDisposition::ExecuteNow,
                    "operator_request_enqueued",
                    WorkflowEffect::EnqueueSystemJob,
                    WorkflowRecoveryPolicy::RetryAfterModeResume,
                );
                return Ok(OperatorMaintenanceSubmission {
                    accepted: true,
                    request_id: request.request_id.clone(),
                    action: request.action,
                    delivery: "in_memory",
                });
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_))
            | Err(std::sync::mpsc::TrySendError::Full(_)) => {}
        }
    }

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        persist_operator_maintenance_request(&request)?;
        append_operator_request_workflow_audit(
            &request,
            WorkflowDisposition::DeferUntil,
            "operator_request_persisted",
            WorkflowEffect::EnqueueSystemJob,
            WorkflowRecoveryPolicy::RetryAfterModeResume,
        );
        Ok(OperatorMaintenanceSubmission {
            accepted: true,
            request_id: request.request_id.clone(),
            action: request.action,
            delivery: "persisted_bridge",
        })
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        Err(Error::config(
            "operator_maintenance_submit",
            "operator maintenance queue unavailable",
        ))
    }
}

pub fn drain_persisted_operator_maintenance_requests(
    system_inbound_tx: &SystemInboundTx,
    limit: usize,
) -> Result<usize> {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let _ = system_inbound_tx;
        let _ = limit;
        Ok(0)
    }

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        let request_dir = operator_request_dir();
        let entries = match std::fs::read_dir(&request_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(Error::io("operator_maintenance_bridge", error)),
        };
        let mut files = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|value| value == "json"))
            .collect::<Vec<_>>();
        files.sort();

        let mut drained = 0usize;
        for path in files.into_iter().take(limit.max(1)) {
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(Error::io("operator_maintenance_bridge", error)),
            };
            let request: OperatorMaintenanceRequest = match serde_json::from_slice(&bytes) {
                Ok(request) => request,
                Err(error) => {
                    log::warn!(
                        "[operator_maintenance] dropping invalid persisted request path={}: {}",
                        path.display(),
                        error
                    );
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
            };
            let msg = match build_operator_maintenance_msg(&request) {
                Ok(msg) => msg,
                Err(error) => {
                    log::warn!(
                        "[operator_maintenance] dropping invalid persisted request path={}: {}",
                        path.display(),
                        error
                    );
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
            };
            match system_inbound_tx.try_send(msg) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&path);
                    drained = drained.saturating_add(1);
                }
                Err(std::sync::mpsc::TrySendError::Full(_)) => break,
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
            }
        }
        Ok(drained)
    }
}

fn build_operator_maintenance_msg(request: &OperatorMaintenanceRequest) -> Result<PcMsg> {
    let body = serde_json::to_string(request)
        .map_err(|error| Error::config("operator_maintenance_submit", error.to_string()))?;
    PcMsg::new_system(CHANNEL_OPERATOR_MAINTENANCE, request.queue_chat_id(), body)
}

fn append_operator_request_workflow_audit(
    request: &OperatorMaintenanceRequest,
    disposition: WorkflowDisposition,
    rationale: &str,
    effect: WorkflowEffect,
    recovery_policy: WorkflowRecoveryPolicy,
) {
    append_workflow_audit(
        WorkflowAuditRecord::new(
            WorkflowKind::OperatorMaintenance,
            WorkflowTrigger::OperatorRequested,
            disposition,
            effect,
            recovery_policy,
            rationale,
            crate::util::current_unix_secs(),
        )
        .with_target(None, request.channel.as_deref(), request.chat_id.as_deref()),
    );
}

fn new_request_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("om{:x}", nanos)
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn persist_operator_maintenance_request(request: &OperatorMaintenanceRequest) -> Result<()> {
    let path = operator_request_path(request.request_id.as_str());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| Error::io("operator_maintenance_bridge", error))?;
    }
    let payload = serde_json::to_vec_pretty(request)
        .map_err(|error| Error::config("operator_maintenance_bridge", error.to_string()))?;
    crate::platform::fs_atomic::atomic_write(path.as_path(), &payload)
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn operator_request_dir() -> PathBuf {
    crate::platform::state_mount_path().join(REL_DIR_OPERATOR_MAINTENANCE_REQUESTS)
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn operator_request_path(request_id: &str) -> PathBuf {
    let sanitized = sanitize_request_id(request_id);
    operator_request_dir().join(format!("{sanitized}.json"))
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn sanitize_request_id(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    normalized.trim_matches('_').to_string()
}

#[cfg(test)]
pub fn operator_maintenance_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::new_inbound_channel;

    #[test]
    fn direct_submission_enqueues_operator_maintenance_message() {
        let _guard = operator_maintenance_test_guard();
        let (tx, rx, _depth) = new_inbound_channel(crate::constants::DEFAULT_CAPACITY);
        let request = OperatorMaintenanceRequest::new(
            OperatorMaintenanceAction::RunRepairPlan,
            Some("chat-1".to_string()),
            Some("qq_channel".to_string()),
            "test",
        );

        let submission =
            submit_operator_maintenance_request(Some(&tx), request.clone()).expect("submit");

        assert_eq!(submission.delivery, "in_memory");
        let msg = rx.try_recv().expect("queued request");
        assert_eq!(msg.channel.as_ref(), CHANNEL_OPERATOR_MAINTENANCE);
        assert_eq!(msg.chat_id.as_ref(), "chat-1");
        let decoded: OperatorMaintenanceRequest =
            serde_json::from_str(&msg.content).expect("decode request");
        assert_eq!(decoded.action, OperatorMaintenanceAction::RunRepairPlan);
        assert_eq!(decoded.channel.as_deref(), Some("qq_channel"));
    }

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    #[test]
    fn persisted_requests_can_be_drained_back_into_system_queue() {
        let _guard = operator_maintenance_test_guard();
        let request_dir = operator_request_dir();
        let _ = std::fs::remove_dir_all(&request_dir);
        let request = OperatorMaintenanceRequest::new(
            OperatorMaintenanceAction::ReplayRecovery,
            None,
            None,
            "test",
        );
        persist_operator_maintenance_request(&request).expect("persist request");
        let (tx, rx, _depth) = new_inbound_channel(crate::constants::DEFAULT_CAPACITY);

        let drained =
            drain_persisted_operator_maintenance_requests(&tx, 4).expect("drain persisted");

        assert_eq!(drained, 1);
        let msg = rx.try_recv().expect("drained request");
        let decoded: OperatorMaintenanceRequest =
            serde_json::from_str(&msg.content).expect("decode request");
        assert_eq!(decoded.action, OperatorMaintenanceAction::ReplayRecovery);
        let _ = std::fs::remove_dir_all(request_dir);
    }
}
