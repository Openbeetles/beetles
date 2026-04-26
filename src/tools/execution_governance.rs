//! Centralized tool-execution governance: admission, audit, breaker, and emergency stop.

use crate::bus::IngressKind;
use crate::error::{Error, Result};
use crate::platform::StateFs;
use crate::tools::{
    ToolApprovalMode, ToolEffectClass, ToolExecutionOutcome, ToolExecutionShape, ToolExposure,
    ToolMetadata, ToolRiskLevel,
};
use crate::util::{current_unix_secs, scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

const REL_PATH_TOOL_EXECUTION_GOVERNANCE: &str = "memory/tool_execution_governance.json";
const REL_PATH_TOOL_EXECUTION_GOVERNANCE_CORRUPT_BACKUP: &str =
    "memory/tool_execution_governance.corrupt.json";
const TOOL_GOVERNANCE_MAX_RECORDS: usize = 48;
const TOOL_GOVERNANCE_MAX_BREAKERS: usize = 32;
const TOOL_GOVERNANCE_REASON_MAX_CHARS: usize = 180;
const TOOL_GOVERNANCE_SUMMARY_MAX_CHARS: usize = 240;
const TOOL_GOVERNANCE_RECORD_RENDER_LIMIT: usize = 10;
const TOOL_GOVERNANCE_BREAKER_RENDER_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionRecordStatus {
    Allowed,
    #[default]
    Denied,
    ResourceDenied,
    Succeeded,
    Failed,
}

impl ToolExecutionRecordStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::ResourceDenied => "resource_denied",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolEmergencyStopState {
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCircuitBreakerState {
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub consecutive_failures: u8,
    #[serde(default)]
    pub last_failure_at: u64,
    #[serde(default)]
    pub last_failure_reason: String,
    #[serde(default)]
    pub tripped_until: u64,
}

impl ToolCircuitBreakerState {
    pub fn is_tripped(&self, now_secs: u64) -> bool {
        self.tripped_until > now_secs
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolExecutionRecord {
    #[serde(default)]
    pub recorded_at: u64,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub operation: String,
    #[serde(default)]
    pub ingress: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub status: ToolExecutionRecordStatus,
    #[serde(default)]
    pub effect_class: ToolEffectClass,
    #[serde(default)]
    pub risk_level: ToolRiskLevel,
    #[serde(default)]
    pub approval_mode: ToolApprovalMode,
    #[serde(default)]
    pub rollback_kind: crate::tools::ToolRollbackKind,
    #[serde(default)]
    pub requires_network: bool,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub summary: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolGovernanceFileState {
    #[serde(default)]
    pub repair_count: u32,
    #[serde(default)]
    pub last_repair_at: u64,
    #[serde(default)]
    pub last_repair_reason: String,
    #[serde(default)]
    pub last_backup_path: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolExecutionGovernanceState {
    #[serde(default)]
    pub emergency_stop: ToolEmergencyStopState,
    #[serde(default)]
    pub breakers: Vec<ToolCircuitBreakerState>,
    #[serde(default)]
    pub recent_records: Vec<ToolExecutionRecord>,
    #[serde(default)]
    pub file_state: ToolGovernanceFileState,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolExecutionRequest {
    pub tool_name: String,
    pub ingress: IngressKind,
    pub channel: String,
    pub metadata: ToolMetadata,
    pub shape: ToolExecutionShape,
    pub requires_network: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolExecutionPermit {
    pub(crate) tool_name: String,
    pub(crate) ingress: IngressKind,
    pub(crate) channel: String,
    pub(crate) metadata: ToolMetadata,
    pub(crate) shape: ToolExecutionShape,
    pub(crate) requires_network: bool,
}

impl ToolExecutionPermit {
    pub fn tool_name(&self) -> &str {
        self.tool_name.as_str()
    }

    pub fn requires_network(&self) -> bool {
        self.requires_network
    }

    pub fn shape(&self) -> &ToolExecutionShape {
        &self.shape
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolExecutionGateDecision {
    Allow(ToolExecutionPermit),
    Deny { reason: String },
}

pub struct ToolExecutionGovernance {
    state_fs: Arc<dyn StateFs + Send + Sync>,
    cache: Mutex<Option<ToolExecutionGovernanceState>>,
}

impl ToolExecutionGovernance {
    pub fn new(state_fs: Arc<dyn StateFs + Send + Sync>) -> Self {
        Self {
            state_fs,
            cache: Mutex::new(None),
        }
    }

    pub fn inspect(&self) -> Result<ToolExecutionGovernanceState> {
        self.with_state(|state| Ok(state.clone()))
    }

    pub fn set_emergency_stop(&self, active: bool, reason: &str) -> Result<ToolEmergencyStopState> {
        self.with_state_mut(|state| {
            state.emergency_stop.active = active;
            state.emergency_stop.reason = sanitize_text(reason, TOOL_GOVERNANCE_REASON_MAX_CHARS);
            state.emergency_stop.updated_at = current_unix_secs();
            state.updated_at = state.emergency_stop.updated_at;
            push_record(
                state,
                ToolExecutionRecord {
                    recorded_at: state.emergency_stop.updated_at,
                    tool_name: "tool_governance".to_string(),
                    operation: if active {
                        "emergency_stop_enable".to_string()
                    } else {
                        "emergency_stop_disable".to_string()
                    },
                    ingress: "operator".to_string(),
                    channel: "system".to_string(),
                    status: ToolExecutionRecordStatus::Succeeded,
                    effect_class: ToolEffectClass::SystemControl,
                    risk_level: ToolRiskLevel::High,
                    approval_mode: ToolApprovalMode::OperatorOnly,
                    rollback_kind: crate::tools::ToolRollbackKind::Irreversible,
                    requires_network: false,
                    reason: state.emergency_stop.reason.clone(),
                    summary: if active {
                        "tool emergency stop enabled".to_string()
                    } else {
                        "tool emergency stop cleared".to_string()
                    },
                },
            );
            Ok(state.emergency_stop.clone())
        })
    }

    pub fn assess(&self, request: ToolExecutionRequest) -> Result<ToolExecutionGateDecision> {
        let now_secs = current_unix_secs();
        self.with_state_mut(|state| {
            let denial_reason =
                if matches!(
                    request.metadata.exposure,
                    ToolExposure::Admin | ToolExposure::Debug
                ) || matches!(request.shape.approval_mode, ToolApprovalMode::OperatorOnly)
                {
                    Some("operator_only_tool".to_string())
                } else if matches!(
                    request.shape.approval_mode,
                    ToolApprovalMode::ExplicitIntent
                ) && !request.shape.approval_granted
                {
                    Some("explicit_intent_required".to_string())
                } else if state.emergency_stop.active
                    && (request.shape.effect_class.is_mutating()
                        || request.shape.risk_level >= ToolRiskLevel::High)
                {
                    Some(if state.emergency_stop.reason.trim().is_empty() {
                        "tool_emergency_stop_active".to_string()
                    } else {
                        format!(
                            "tool_emergency_stop_active: {}",
                            state.emergency_stop.reason.trim()
                        )
                    })
                } else if breaker_for_tool(state, request.tool_name.as_str())
                    .is_some_and(|breaker| breaker.is_tripped(now_secs))
                {
                    Some(format!(
                        "tool_circuit_breaker_active until {}",
                        breaker_for_tool(state, request.tool_name.as_str())
                            .map(|breaker| breaker.tripped_until)
                            .unwrap_or(0)
                    ))
                } else {
                    None
                };

            if let Some(reason) = denial_reason {
                push_record(
                    state,
                    ToolExecutionRecord {
                        recorded_at: now_secs,
                        tool_name: request.tool_name.clone(),
                        operation: sanitize_operation(request.shape.operation.as_str()),
                        ingress: ingress_label(request.ingress).to_string(),
                        channel: sanitize_text(request.channel.as_str(), 48),
                        status: ToolExecutionRecordStatus::Denied,
                        effect_class: request.shape.effect_class,
                        risk_level: request.shape.risk_level,
                        approval_mode: request.shape.approval_mode,
                        rollback_kind: request.shape.rollback_kind,
                        requires_network: request.requires_network,
                        reason: sanitize_text(reason.as_str(), TOOL_GOVERNANCE_REASON_MAX_CHARS),
                        summary: String::new(),
                    },
                );
                return Ok(ToolExecutionGateDecision::Deny { reason });
            }

            let permit = ToolExecutionPermit {
                tool_name: request.tool_name.clone(),
                ingress: request.ingress,
                channel: request.channel.clone(),
                metadata: request.metadata,
                shape: request.shape.clone(),
                requires_network: request.requires_network,
            };
            push_record(
                state,
                ToolExecutionRecord {
                    recorded_at: now_secs,
                    tool_name: request.tool_name,
                    operation: sanitize_operation(permit.shape.operation.as_str()),
                    ingress: ingress_label(permit.ingress).to_string(),
                    channel: sanitize_text(permit.channel.as_str(), 48),
                    status: ToolExecutionRecordStatus::Allowed,
                    effect_class: permit.shape.effect_class,
                    risk_level: permit.shape.risk_level,
                    approval_mode: permit.shape.approval_mode,
                    rollback_kind: permit.shape.rollback_kind,
                    requires_network: permit.requires_network,
                    reason: String::new(),
                    summary: String::new(),
                },
            );
            Ok(ToolExecutionGateDecision::Allow(permit))
        })
    }

    pub fn record_resource_denial(&self, permit: &ToolExecutionPermit, reason: &str) -> Result<()> {
        self.append_record(
            permit,
            ToolExecutionRecordStatus::ResourceDenied,
            reason,
            "",
        )
    }

    pub fn record_success(
        &self,
        permit: &ToolExecutionPermit,
        outcome: &ToolExecutionOutcome,
    ) -> Result<()> {
        let summary = build_success_summary(outcome);
        self.with_state_mut(|state| {
            clear_breaker(state, permit.tool_name.as_str());
            push_record(
                state,
                record_from_permit(
                    permit,
                    ToolExecutionRecordStatus::Succeeded,
                    "",
                    summary.as_str(),
                ),
            );
            Ok(())
        })
    }

    pub fn record_failure(
        &self,
        permit: &ToolExecutionPermit,
        error: &crate::error::Error,
    ) -> Result<()> {
        let reason = sanitize_text(&error.to_string(), TOOL_GOVERNANCE_REASON_MAX_CHARS);
        self.with_state_mut(|state| {
            let now_secs = current_unix_secs();
            let Some((threshold, cooldown_secs)) = breaker_policy_for_risk(permit.shape.risk_level)
            else {
                push_record(
                    state,
                    record_from_permit(
                        permit,
                        ToolExecutionRecordStatus::Failed,
                        reason.as_str(),
                        "",
                    ),
                );
                return Ok(());
            };
            {
                let breaker = breaker_for_tool_mut(state, permit.tool_name.as_str());
                breaker.consecutive_failures = breaker.consecutive_failures.saturating_add(1);
                breaker.last_failure_at = now_secs;
                breaker.last_failure_reason = reason.clone();
                if breaker.consecutive_failures >= threshold {
                    breaker.tripped_until = now_secs.saturating_add(cooldown_secs);
                }
            }
            prune_breakers(state);
            push_record(
                state,
                record_from_permit(
                    permit,
                    ToolExecutionRecordStatus::Failed,
                    reason.as_str(),
                    "",
                ),
            );
            Ok(())
        })
    }

    fn append_record(
        &self,
        permit: &ToolExecutionPermit,
        status: ToolExecutionRecordStatus,
        reason: &str,
        summary: &str,
    ) -> Result<()> {
        self.with_state_mut(|state| {
            push_record(state, record_from_permit(permit, status, reason, summary));
            Ok(())
        })
    }

    fn with_state<R>(
        &self,
        f: impl FnOnce(&ToolExecutionGovernanceState) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(load_state(self.state_fs.as_ref()));
        }
        let state = guard.as_ref().expect("state initialized");
        f(state)
    }

    fn with_state_mut<R>(
        &self,
        f: impl FnOnce(&mut ToolExecutionGovernanceState) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(load_state(self.state_fs.as_ref()));
        }
        let state = guard.as_mut().expect("state initialized");
        let result = f(state)?;
        persist_state(self.state_fs.as_ref(), state)?;
        Ok(result)
    }
}

pub fn render_tool_execution_governance_markdown(state: &ToolExecutionGovernanceState) -> String {
    let mut out = String::from("# Tool Execution Governance\n\n");
    out.push_str(&format!(
        "- emergency_stop_active: {}\n- emergency_stop_reason: {}\n- breaker_count: {}\n- recent_records: {}\n- file_repair_count: {}\n- last_file_repair: {}\n- last_file_repair_reason: {}\n- last_file_backup: {}\n",
        state.emergency_stop.active,
        if state.emergency_stop.reason.trim().is_empty() {
            "<none>"
        } else {
            state.emergency_stop.reason.as_str()
        },
        state.breakers.len(),
        state.recent_records.len(),
        state.file_state.repair_count,
        state.file_state.last_repair_at,
        if state.file_state.last_repair_reason.trim().is_empty() {
            "<none>"
        } else {
            state.file_state.last_repair_reason.as_str()
        },
        if state.file_state.last_backup_path.trim().is_empty() {
            "<none>"
        } else {
            state.file_state.last_backup_path.as_str()
        }
    ));
    out.push_str("\n## Active Breakers\n");
    let active_breakers = state
        .breakers
        .iter()
        .filter(|breaker| breaker.tripped_until > 0)
        .take(TOOL_GOVERNANCE_BREAKER_RENDER_LIMIT)
        .collect::<Vec<_>>();
    if active_breakers.is_empty() {
        out.push_str("- No active breakers.\n");
    } else {
        for breaker in active_breakers {
            out.push_str(&format!(
                "- {} | failures={} | tripped_until={} | reason={}\n",
                breaker.tool_name,
                breaker.consecutive_failures,
                breaker.tripped_until,
                if breaker.last_failure_reason.trim().is_empty() {
                    "<none>"
                } else {
                    breaker.last_failure_reason.as_str()
                }
            ));
        }
    }
    out.push_str("\n## Recent Records\n");
    if state.recent_records.is_empty() {
        out.push_str("- No tool governance records yet.\n");
    } else {
        for record in state
            .recent_records
            .iter()
            .rev()
            .take(TOOL_GOVERNANCE_RECORD_RENDER_LIMIT)
        {
            out.push_str(&format!(
                "- {} | {}:{} | {} | effect={} | risk={} | approval={} | reason={} | summary={}\n",
                record.recorded_at,
                record.tool_name,
                if record.operation.trim().is_empty() {
                    "<default>"
                } else {
                    record.operation.as_str()
                },
                record.status.label(),
                record.effect_class.label(),
                record.risk_level.label(),
                record.approval_mode.label(),
                if record.reason.trim().is_empty() {
                    "<none>"
                } else {
                    record.reason.as_str()
                },
                if record.summary.trim().is_empty() {
                    "<none>"
                } else {
                    record.summary.as_str()
                }
            ));
        }
    }
    out.trim_end().to_string()
}

fn load_state(fs: &dyn StateFs) -> ToolExecutionGovernanceState {
    match fs.read(REL_PATH_TOOL_EXECUTION_GOVERNANCE) {
        Ok(Some(raw)) if !raw.is_empty() => match parse_state_with_repair_hint(&raw) {
            Ok((state, None)) => state,
            Ok((mut state, Some(reason))) => {
                log::warn!(
                    "[tool_governance] repairing {}: {}",
                    REL_PATH_TOOL_EXECUTION_GOVERNANCE,
                    reason
                );
                backup_corrupt_state(fs, &raw);
                mark_file_repair(&mut state, reason);
                if let Err(repair_error) = persist_state(fs, &state) {
                    log::warn!(
                        "[tool_governance] failed to repair {}: {}",
                        REL_PATH_TOOL_EXECUTION_GOVERNANCE,
                        repair_error
                    );
                }
                state
            }
            Err(error) => {
                log::warn!(
                    "[tool_governance] failed to parse {}: {}",
                    REL_PATH_TOOL_EXECUTION_GOVERNANCE,
                    error
                );
                backup_corrupt_state(fs, &raw);
                let mut default_state = ToolExecutionGovernanceState::default();
                mark_file_repair(&mut default_state, "corrupt_json");
                if let Err(repair_error) = persist_state(fs, &default_state) {
                    log::warn!(
                        "[tool_governance] failed to repair {}: {}",
                        REL_PATH_TOOL_EXECUTION_GOVERNANCE,
                        repair_error
                    );
                }
                default_state
            }
        },
        _ => ToolExecutionGovernanceState::default(),
    }
}

fn parse_state_with_repair_hint(
    raw: &[u8],
) -> std::result::Result<(ToolExecutionGovernanceState, Option<&'static str>), serde_json::Error> {
    match serde_json::from_slice::<ToolExecutionGovernanceState>(raw) {
        Ok(state) => Ok((state, None)),
        Err(strict_error) => {
            let mut stream = serde_json::Deserializer::from_slice(raw)
                .into_iter::<ToolExecutionGovernanceState>();
            match stream.next() {
                Some(Ok(state)) => {
                    let trailing = &raw[stream.byte_offset()..];
                    if trailing
                        .iter()
                        .any(|b| !matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
                    {
                        Ok((state, Some("trailing_characters")))
                    } else {
                        Err(strict_error)
                    }
                }
                _ => Err(strict_error),
            }
        }
    }
}

fn mark_file_repair(state: &mut ToolExecutionGovernanceState, reason: &'static str) {
    state.file_state.repair_count = state.file_state.repair_count.saturating_add(1);
    state.file_state.last_repair_at = current_unix_secs();
    state.file_state.last_repair_reason = reason.to_string();
    state.file_state.last_backup_path =
        REL_PATH_TOOL_EXECUTION_GOVERNANCE_CORRUPT_BACKUP.to_string();
    state.updated_at = state.updated_at.max(state.file_state.last_repair_at);
}

fn backup_corrupt_state(fs: &dyn StateFs, raw: &[u8]) {
    if let Err(error) = fs.write(REL_PATH_TOOL_EXECUTION_GOVERNANCE_CORRUPT_BACKUP, raw) {
        log::warn!(
            "[tool_governance] failed to back up corrupt {} to {}: {}",
            REL_PATH_TOOL_EXECUTION_GOVERNANCE,
            REL_PATH_TOOL_EXECUTION_GOVERNANCE_CORRUPT_BACKUP,
            error
        );
    }
}

fn persist_state(fs: &dyn StateFs, state: &ToolExecutionGovernanceState) -> Result<()> {
    let data = serde_json::to_vec_pretty(state)
        .map_err(|error| Error::config("tool_governance_persist", error.to_string()))?;
    fs.write(REL_PATH_TOOL_EXECUTION_GOVERNANCE, &data)
}

fn push_record(state: &mut ToolExecutionGovernanceState, record: ToolExecutionRecord) {
    state.updated_at = state.updated_at.max(record.recorded_at);
    state.recent_records.push(record);
    if state.recent_records.len() > TOOL_GOVERNANCE_MAX_RECORDS {
        let drop_count = state.recent_records.len() - TOOL_GOVERNANCE_MAX_RECORDS;
        state.recent_records.drain(0..drop_count);
    }
}

fn breaker_for_tool<'a>(
    state: &'a ToolExecutionGovernanceState,
    tool_name: &str,
) -> Option<&'a ToolCircuitBreakerState> {
    state
        .breakers
        .iter()
        .find(|breaker| breaker.tool_name == tool_name)
}

fn breaker_for_tool_mut<'a>(
    state: &'a mut ToolExecutionGovernanceState,
    tool_name: &str,
) -> &'a mut ToolCircuitBreakerState {
    if let Some(index) = state
        .breakers
        .iter()
        .position(|breaker| breaker.tool_name == tool_name)
    {
        return &mut state.breakers[index];
    }
    state.breakers.push(ToolCircuitBreakerState {
        tool_name: tool_name.to_string(),
        ..ToolCircuitBreakerState::default()
    });
    let len = state.breakers.len();
    &mut state.breakers[len - 1]
}

fn clear_breaker(state: &mut ToolExecutionGovernanceState, tool_name: &str) {
    if let Some(index) = state
        .breakers
        .iter()
        .position(|breaker| breaker.tool_name == tool_name)
    {
        state.breakers[index].consecutive_failures = 0;
        state.breakers[index].tripped_until = 0;
        state.breakers[index].last_failure_reason.clear();
    }
    prune_breakers(state);
}

fn prune_breakers(state: &mut ToolExecutionGovernanceState) {
    if state.breakers.len() <= TOOL_GOVERNANCE_MAX_BREAKERS {
        return;
    }
    state.breakers.sort_by(|left, right| {
        right
            .tripped_until
            .cmp(&left.tripped_until)
            .then_with(|| right.last_failure_at.cmp(&left.last_failure_at))
    });
    state.breakers.truncate(TOOL_GOVERNANCE_MAX_BREAKERS);
}

fn breaker_policy_for_risk(risk: ToolRiskLevel) -> Option<(u8, u64)> {
    match risk {
        ToolRiskLevel::Low => None,
        ToolRiskLevel::Medium => Some((4, 5 * 60)),
        ToolRiskLevel::High => Some((3, 15 * 60)),
        ToolRiskLevel::Critical => Some((2, 30 * 60)),
    }
}

fn record_from_permit(
    permit: &ToolExecutionPermit,
    status: ToolExecutionRecordStatus,
    reason: &str,
    summary: &str,
) -> ToolExecutionRecord {
    ToolExecutionRecord {
        recorded_at: current_unix_secs(),
        tool_name: permit.tool_name.clone(),
        operation: sanitize_operation(permit.shape.operation.as_str()),
        ingress: ingress_label(permit.ingress).to_string(),
        channel: sanitize_text(permit.channel.as_str(), 48),
        status,
        effect_class: permit.shape.effect_class,
        risk_level: permit.shape.risk_level,
        approval_mode: permit.shape.approval_mode,
        rollback_kind: permit.shape.rollback_kind,
        requires_network: permit.requires_network,
        reason: sanitize_text(reason, TOOL_GOVERNANCE_REASON_MAX_CHARS),
        summary: sanitize_text(summary, TOOL_GOVERNANCE_SUMMARY_MAX_CHARS),
    }
}

fn build_success_summary(outcome: &ToolExecutionOutcome) -> String {
    let outbound_count = outcome.outbound_intents.len();
    let content = sanitize_text(outcome.content.as_str(), TOOL_GOVERNANCE_SUMMARY_MAX_CHARS);
    if outbound_count == 0 {
        return content;
    }
    if content.is_empty() {
        return format!("outbound_intents={outbound_count}");
    }
    format!("{} | outbound_intents={outbound_count}", content)
}

fn sanitize_operation(operation: &str) -> String {
    sanitize_text(operation, 64)
}

fn sanitize_text(value: &str, max_chars: usize) -> String {
    truncate_content_to_max(scrub_credentials(value).trim(), max_chars).to_string()
}

fn ingress_label(ingress: IngressKind) -> &'static str {
    match ingress {
        IngressKind::User => "user",
        IngressKind::System => "system",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MemoryStateFs {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl StateFs for MemoryStateFs {
        fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(rel_path)
                .cloned())
        }

        fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove(&self, rel_path: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(rel_path);
            Ok(())
        }

        fn list_dir(&self, _rel_path: &str) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn corrupt_governance_json_is_repaired_to_default_state() {
        let fs = Arc::new(MemoryStateFs::default());
        fs.write(REL_PATH_TOOL_EXECUTION_GOVERNANCE, br#"{"updated_at":"#)
            .unwrap();
        let governance =
            ToolExecutionGovernance::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let state = governance.inspect().unwrap();

        assert!(state.recent_records.is_empty());
        assert_eq!(state.file_state.repair_count, 1);
        let repaired = fs
            .read(REL_PATH_TOOL_EXECUTION_GOVERNANCE)
            .unwrap()
            .expect("corrupt state should be repaired");
        assert!(serde_json::from_slice::<ToolExecutionGovernanceState>(&repaired).is_ok());
    }

    #[test]
    fn trailing_governance_json_repair_preserves_valid_prefix_and_backup() {
        let fs = Arc::new(MemoryStateFs::default());
        let valid_prefix = br#"{"recent_records":[{"tool_name":"web_search","status":"succeeded","effect_class":"network_search"}],"updated_at":7}"#;
        let mut raw = valid_prefix.to_vec();
        raw.extend_from_slice(b"false");
        fs.write(REL_PATH_TOOL_EXECUTION_GOVERNANCE, &raw).unwrap();
        let governance =
            ToolExecutionGovernance::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let state = governance.inspect().unwrap();

        assert_eq!(state.recent_records.len(), 1);
        assert_eq!(state.recent_records[0].tool_name, "web_search");
        assert_eq!(state.file_state.repair_count, 1);
        assert_eq!(state.file_state.last_repair_reason, "trailing_characters");
        let repaired = fs
            .read(REL_PATH_TOOL_EXECUTION_GOVERNANCE)
            .unwrap()
            .expect("primary state should be rewritten");
        assert!(serde_json::from_slice::<ToolExecutionGovernanceState>(&repaired).is_ok());
        assert!(!String::from_utf8_lossy(&repaired).contains("}false"));
        let backup = fs
            .read(REL_PATH_TOOL_EXECUTION_GOVERNANCE_CORRUPT_BACKUP)
            .unwrap()
            .expect("corrupt raw state should be backed up");
        assert_eq!(backup, raw);
    }

    #[test]
    fn unrecoverable_governance_json_repair_records_file_state() {
        let fs = Arc::new(MemoryStateFs::default());
        let raw = br#"{"recent_records":[{"tool_name":"web_search""#.to_vec();
        fs.write(REL_PATH_TOOL_EXECUTION_GOVERNANCE, &raw).unwrap();
        let governance =
            ToolExecutionGovernance::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let state = governance.inspect().unwrap();

        assert!(state.recent_records.is_empty());
        assert_eq!(state.file_state.repair_count, 1);
        assert_eq!(state.file_state.last_repair_reason, "corrupt_json");
        let backup = fs
            .read(REL_PATH_TOOL_EXECUTION_GOVERNANCE_CORRUPT_BACKUP)
            .unwrap()
            .expect("unrecoverable raw state should be backed up");
        assert_eq!(backup, raw);
    }

    #[test]
    fn explicit_intent_gate_denies_without_approval() {
        let governance = ToolExecutionGovernance::new(Arc::new(MemoryStateFs::default()));
        let decision = governance
            .assess(ToolExecutionRequest {
                tool_name: "danger".to_string(),
                ingress: IngressKind::User,
                channel: "telegram".to_string(),
                metadata: ToolMetadata::stateful(),
                shape: ToolMetadata::stateful()
                    .default_execution_shape("danger")
                    .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                    .with_approval_granted(false),
                requires_network: false,
            })
            .unwrap();
        assert!(matches!(decision, ToolExecutionGateDecision::Deny { .. }));
    }

    #[test]
    fn breaker_trips_after_repeated_high_risk_failures() {
        let governance = ToolExecutionGovernance::new(Arc::new(MemoryStateFs::default()));
        let permit = ToolExecutionPermit {
            tool_name: "device_control".to_string(),
            ingress: IngressKind::User,
            channel: "telegram".to_string(),
            metadata: ToolMetadata::task(),
            shape: ToolMetadata::task()
                .default_execution_shape("gpio_out")
                .with_effect_class(ToolEffectClass::HardwareActuation)
                .with_risk_level(ToolRiskLevel::High),
            requires_network: false,
        };
        let error = Error::config("tool_device_control", "boom");
        governance.record_failure(&permit, &error).unwrap();
        governance.record_failure(&permit, &error).unwrap();
        governance.record_failure(&permit, &error).unwrap();

        let decision = governance
            .assess(ToolExecutionRequest {
                tool_name: "device_control".to_string(),
                ingress: IngressKind::User,
                channel: "telegram".to_string(),
                metadata: ToolMetadata::task(),
                shape: permit.shape.clone(),
                requires_network: false,
            })
            .unwrap();
        assert!(matches!(decision, ToolExecutionGateDecision::Deny { .. }));
    }
}
