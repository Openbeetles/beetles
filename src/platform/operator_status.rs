//! Unified operator-facing status contract for HTTP and CLI.

use crate::capability_package::{
    build_capability_package_operator_snapshot, render_capability_package_operator_text,
    CapabilityPackageOperatorSnapshot, CapabilityPackageRuntimeCapabilities,
};
use crate::channel_capability::{
    build_channel_capability_snapshots_for_registry, ChannelCapabilityRegistry,
    ChannelCapabilitySnapshot,
};
use crate::memory::{
    board_subject_scope_id, compute_core_revision_governance_digest,
    inspect_personality_governance, load_recent_persona_evidence,
    select_personality_governance_targets, CoreRevisionLedgerStore, MemoryProfile,
    PersonalityGovernanceInspectionInput, RelationshipConstitutionStore,
    RelationshipPortfolioStore, RelationshipTopologyStore, SelfAuthoredCoreStore,
    SelfContinuityStore, TurnLedgerStore,
};
use crate::orchestrator;
use crate::runtime;
use crate::task_execution::{
    build_task_execution_operator_snapshot, render_task_execution_operator_text,
    TaskExecutionOperatorSnapshot,
};
use crate::tools::{ToolCatalogEntry, ToolExecutionGovernanceState, ToolRegistry};
use crate::util::{current_unix_secs, truncate_content_to_max};
use crate::Platform;
use serde::Serialize;

const REL_DIR_MANUAL_CONTINUITY_SNAPSHOTS: &str = "memory/continuity_snapshots/manual";
const OPERATOR_GOVERNANCE_RELATION_SUMMARY_MAX_CHARS: usize = 120;

#[derive(Debug, Serialize)]
pub struct OperatorPersonalityGovernanceRelation {
    pub scope_id: String,
    pub channel: String,
    pub chat_id: String,
    pub closure_ready: bool,
    pub repair_needed: bool,
    pub primary_action: String,
    pub repair_summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outstanding: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drift_flags: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct OperatorPersonalityGovernanceSnapshot {
    pub board_core_present: bool,
    pub board_revision: u64,
    pub core_review_due: bool,
    pub core_conservative_mode: bool,
    pub observation_active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_chat_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_channel: Option<String>,
    pub active_relations: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<OperatorPersonalityGovernanceRelation>,
}

pub struct OperatorStatusInput<'a> {
    pub platform: &'a dyn Platform,
    pub tool_registry: &'a ToolRegistry,
    pub channel_capability_registry: &'a ChannelCapabilityRegistry,
    pub capability_package_runtime_capabilities: &'a CapabilityPackageRuntimeCapabilities,
    pub current_channel: &'a str,
    pub inbound_depth: usize,
    pub outbound_depth: usize,
    pub version: &'a str,
    pub board_id: &'a str,
    pub llm_stream_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct OperatorPlatformContract {
    pub board_id: String,
    pub firmware_version: String,
    pub memory_profile: String,
    pub wifi_connected: bool,
    pub config_plane_active: bool,
    pub display_available: bool,
    pub wifi_scan_available: bool,
    pub hardware_discovery_available: bool,
    pub ota_supported: bool,
    pub audio_duplex_profile: String,
    pub storage_media_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_media_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OperatorContinuityTooling {
    pub export_supported: bool,
    pub import_supported: bool,
    pub inspect_governance_supported: bool,
    pub inspect_recall_supported: bool,
    pub inspect_hygiene_supported: bool,
    pub inspect_tool_governance_supported: bool,
    pub saved_snapshot_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub saved_snapshots: Vec<String>,
}

#[derive(Serialize)]
pub struct OperatorStatusSnapshot {
    pub platform_contract: OperatorPlatformContract,
    pub inbound_depth: usize,
    pub outbound_depth: usize,
    pub last_error: String,
    pub metrics: crate::metrics::MetricsSnapshot,
    pub resource: orchestrator::ResourceSnapshot,
    pub threads: runtime::ThreadRegistrySnapshot,
    pub runtime_mode: runtime::thread_registry::RuntimeModeSnapshot,
    pub continuity_tooling: OperatorContinuityTooling,
    pub task_execution: TaskExecutionOperatorSnapshot,
    pub capability_packages: CapabilityPackageOperatorSnapshot,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<ChannelCapabilitySnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolCatalogEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_governance: Option<ToolExecutionGovernanceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub personality_governance: Option<OperatorPersonalityGovernanceSnapshot>,
}

pub fn build_operator_status(
    input: OperatorStatusInput<'_>,
) -> crate::error::Result<OperatorStatusSnapshot> {
    let continuity_tool_available = input.tool_registry.get("continuity_snapshot").is_some();
    let tool_governance = input.tool_registry.inspect_execution_governance()?;
    let storage_media = input.platform.storage_media();
    let (storage_media_count, storage_media_error) = match storage_media {
        Ok(items) => (items.len(), None),
        Err(error) => (0, Some(error.to_string())),
    };
    let saved_snapshots = input
        .platform
        .state_fs()
        .list_dir(REL_DIR_MANUAL_CONTINUITY_SNAPSHOTS)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| {
            name.strip_suffix(".json")
                .map(str::to_string)
                .filter(|value| !value.trim().is_empty())
        })
        .collect::<Vec<_>>();
    let audio_caps = input.platform.audio_duplex_capabilities();
    let task_execution = build_task_execution_operator_snapshot(
        input.platform.task_run_store().as_ref(),
        input.platform.task_artifact_store().as_ref(),
        input.platform.task_learning_store().as_ref(),
    )?;
    let capability_packages = build_capability_package_operator_snapshot(
        input.platform.state_fs().as_ref(),
        input.capability_package_runtime_capabilities,
        input.current_channel,
    )?;
    let channels = build_channel_capability_snapshots_for_registry(
        input.channel_capability_registry,
        input.llm_stream_enabled,
    );
    let personality_governance = inspect_operator_personality_governance(
        input.platform.self_authored_core_store().as_ref(),
        input.platform.core_revision_ledger_store().as_ref(),
        input.platform.relationship_constitution_store().as_ref(),
        input.platform.relationship_portfolio_store().as_ref(),
        input.platform.relationship_topology_store().as_ref(),
        input.platform.self_continuity_store().as_ref(),
        input.platform.turn_ledger_store().as_ref(),
        input.platform.memory_profile(),
        current_unix_secs(),
    )?;
    Ok(OperatorStatusSnapshot {
        platform_contract: OperatorPlatformContract {
            board_id: input.board_id.to_string(),
            firmware_version: input.version.to_string(),
            memory_profile: memory_profile_label(input.platform.memory_profile()).to_string(),
            wifi_connected: crate::state::wifi_sta_connected(),
            config_plane_active: crate::state::config_plane_active(),
            display_available: input.platform.display_available(),
            wifi_scan_available: input.platform.wifi_scan().is_some(),
            hardware_discovery_available: input.platform.hardware_discovery().is_some(),
            ota_supported: cfg!(feature = "ota"),
            audio_duplex_profile: audio_caps.profile().as_str().to_string(),
            storage_media_count,
            storage_media_error,
        },
        inbound_depth: input.inbound_depth,
        outbound_depth: input.outbound_depth,
        last_error: crate::state::get_current_error().unwrap_or_else(|| "none".to_string()),
        metrics: crate::metrics::snapshot(),
        resource: orchestrator::snapshot(),
        threads: runtime::thread_registry::snapshot(),
        runtime_mode: runtime::thread_registry::runtime_mode_snapshot(),
        continuity_tooling: OperatorContinuityTooling {
            export_supported: continuity_tool_available,
            import_supported: continuity_tool_available,
            inspect_governance_supported: continuity_tool_available,
            inspect_recall_supported: continuity_tool_available,
            inspect_hygiene_supported: continuity_tool_available,
            inspect_tool_governance_supported: continuity_tool_available
                && tool_governance.is_some(),
            saved_snapshot_count: saved_snapshots.len(),
            saved_snapshots,
        },
        task_execution,
        capability_packages,
        channels,
        tools: input.tool_registry.tool_catalog()?,
        tool_governance,
        personality_governance,
    })
}

pub fn render_operator_status_text(snapshot: &OperatorStatusSnapshot) -> String {
    let mut out = String::from("operator_status:\n");
    out.push_str(&format!(
        "  board_id: {}\n  firmware_version: {}\n  memory_profile: {}\n  wifi_connected: {}\n  config_plane_active: {}\n  display_available: {}\n  wifi_scan_available: {}\n  hardware_discovery_available: {}\n  ota_supported: {}\n  audio_duplex_profile: {}\n  storage_media_count: {}\n",
        snapshot.platform_contract.board_id,
        snapshot.platform_contract.firmware_version,
        snapshot.platform_contract.memory_profile,
        snapshot.platform_contract.wifi_connected,
        snapshot.platform_contract.config_plane_active,
        snapshot.platform_contract.display_available,
        snapshot.platform_contract.wifi_scan_available,
        snapshot.platform_contract.hardware_discovery_available,
        snapshot.platform_contract.ota_supported,
        snapshot.platform_contract.audio_duplex_profile,
        snapshot.platform_contract.storage_media_count,
    ));
    if let Some(error) = snapshot.platform_contract.storage_media_error.as_deref() {
        out.push_str(&format!("  storage_media_error: {}\n", error));
    }
    out.push_str(&format!(
        "  inbound_depth: {}\n  outbound_depth: {}\n  last_error: {}\n  pressure: {:?}\n  continuity_saved_snapshots: {}\n",
        snapshot.inbound_depth,
        snapshot.outbound_depth,
        snapshot.last_error,
        snapshot.resource.pressure,
        snapshot.continuity_tooling.saved_snapshot_count,
    ));
    if let Some(governance) = snapshot.tool_governance.as_ref() {
        out.push_str(&format!(
            "  tool_emergency_stop: {}\n  tool_breakers: {}\n  tool_records: {}\n",
            governance.emergency_stop.active,
            governance.breakers.len(),
            governance.recent_records.len(),
        ));
    }
    if let Some(governance) = snapshot.personality_governance.as_ref() {
        out.push_str(&format!(
            "  personality_governance: board_revision={} board_core_present={} review_due={} conservative={} observation_active={} active_relations={}\n",
            governance.board_revision,
            governance.board_core_present,
            governance.core_review_due,
            governance.core_conservative_mode,
            governance.observation_active,
            governance.active_relations,
        ));
        if let Some(chat_id) = governance.anchor_chat_id.as_deref() {
            out.push_str(&format!("  personality_anchor_chat: {}\n", chat_id));
        }
        if let Some(channel) = governance.anchor_channel.as_deref() {
            out.push_str(&format!("  personality_anchor_channel: {}\n", channel));
        }
        for relation in &governance.relations {
            out.push_str(&format!(
                "    - relation {}:{} ready={} repair={} summary={}\n",
                relation.channel,
                relation.chat_id,
                relation.closure_ready,
                relation.primary_action,
                relation.repair_summary,
            ));
        }
    }
    out.push_str(&render_task_execution_operator_text(
        &snapshot.task_execution,
    ));
    out.push_str(&render_capability_package_operator_text(
        &snapshot.capability_packages,
    ));
    out.push_str("  channels:\n");
    for channel in &snapshot.channels {
        out.push_str(&format!(
            "    - {} | configured={} enabled={} primary={} supplemental={} edit={} stream_edit={} explicit_target={} typing={} stream_edit_active={} degraded={}\n",
            channel.id,
            channel.configured,
            channel.enabled,
            channel.supports_primary_reply,
            channel.supports_supplemental_reply,
            channel.supports_edit,
            channel.supports_stream_edit,
            channel.supports_explicit_target,
            channel.supports_typing_or_chat_action,
            channel.stream_edit_active,
            if channel.degraded_reasons.is_empty() {
                "none".to_string()
            } else {
                channel.degraded_reasons.join(",")
            }
        ));
    }
    out.push_str("  tools:\n");
    for tool in snapshot.tools.iter().take(12) {
        out.push_str(&format!(
            "    - {} | exposure={} effect={} risk={} approval={} net={} user_llm={} breaker={}\n",
            tool.name,
            tool.exposure,
            tool.effect_class,
            tool.risk_level,
            tool.approval_mode,
            tool.requires_network,
            tool.llm_visible_user,
            tool.governance_breaker_tripped,
        ));
    }
    if snapshot.tools.len() > 12 {
        out.push_str(&format!(
            "    - ... {} more tools\n",
            snapshot.tools.len() - 12
        ));
    }
    out
}

fn memory_profile_label(profile: MemoryProfile) -> &'static str {
    match profile {
        MemoryProfile::Embedded => "embedded",
        MemoryProfile::Standard => "standard",
    }
}

fn inspect_operator_personality_governance(
    self_authored_core_store: &dyn SelfAuthoredCoreStore,
    core_revision_ledger_store: &dyn CoreRevisionLedgerStore,
    relationship_constitution_store: &dyn RelationshipConstitutionStore,
    relationship_portfolio_store: &dyn RelationshipPortfolioStore,
    relationship_topology_store: &dyn RelationshipTopologyStore,
    self_continuity_store: &dyn SelfContinuityStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    profile: MemoryProfile,
    now_secs: u64,
) -> crate::error::Result<Option<OperatorPersonalityGovernanceSnapshot>> {
    let subject_id = board_subject_scope_id();
    let self_authored_core = self_authored_core_store.get(subject_id)?;
    let core_revision_ledger = core_revision_ledger_store.get(subject_id)?;
    let self_continuity = self_continuity_store.get(subject_id)?;
    let relationship_portfolio = relationship_portfolio_store.get(subject_id)?;
    let relationship_topology = relationship_topology_store.get(subject_id)?;
    let has_governance_state = self_authored_core.is_some()
        || core_revision_ledger.is_some()
        || relationship_portfolio.is_some()
        || relationship_topology.is_some();
    if !has_governance_state {
        return Ok(None);
    }

    let governance = compute_core_revision_governance_digest(
        core_revision_ledger.as_ref(),
        self_authored_core
            .as_ref()
            .map(|core| core.last_reviewed_at)
            .unwrap_or(0),
        self_authored_core
            .as_ref()
            .map(|core| core.stability_score)
            .unwrap_or(0),
        now_secs,
    );
    let targets = select_operator_personality_governance_targets(
        self_continuity.as_ref(),
        relationship_portfolio.as_ref(),
        relationship_topology.as_ref(),
        now_secs,
        profile,
    );
    let mut relations = Vec::with_capacity(targets.len());
    for target in targets {
        let relationship_constitution = relationship_constitution_store.get(&target.scope_id)?;
        let recent_persona_evidence =
            load_recent_persona_evidence(turn_ledger_store, &target.scope_id)?;
        let inspection = inspect_personality_governance(PersonalityGovernanceInspectionInput {
            channel: &target.channel,
            chat_id: &target.chat_id,
            now_secs,
            self_authored_core: self_authored_core.as_ref(),
            core_revision_ledger: core_revision_ledger.as_ref(),
            relationship_constitution: relationship_constitution.as_ref(),
            relationship_topology: relationship_topology.as_ref(),
            recent_persona_evidence: recent_persona_evidence.as_ref(),
        });
        relations.push(OperatorPersonalityGovernanceRelation {
            scope_id: target.scope_id,
            channel: target.channel,
            chat_id: target.chat_id,
            closure_ready: inspection.closure.ready,
            repair_needed: inspection.repair_plan.repair_needed,
            primary_action: inspection.repair_plan.primary_action.label().to_string(),
            repair_summary: truncate_content_to_max(
                inspection.repair_plan.summary.trim(),
                OPERATOR_GOVERNANCE_RELATION_SUMMARY_MAX_CHARS,
            )
            .into_owned(),
            outstanding: inspection.closure.outstanding,
            drift_flags: inspection
                .relationship_audit
                .map(|audit| audit.drift_flags)
                .unwrap_or_default(),
        });
    }

    Ok(Some(OperatorPersonalityGovernanceSnapshot {
        board_core_present: self_authored_core.is_some(),
        board_revision: self_authored_core
            .as_ref()
            .map(|core| core.revision)
            .unwrap_or(0),
        core_review_due: governance.review_due,
        core_conservative_mode: governance.conservative_mode,
        observation_active: governance.observation_active,
        anchor_chat_id: self_continuity
            .as_ref()
            .map(|continuity| continuity.last_user_chat_id.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        anchor_channel: self_continuity
            .as_ref()
            .map(|continuity| continuity.last_user_channel.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        active_relations: relations.len(),
        relations,
    }))
}

fn select_operator_personality_governance_targets(
    self_continuity: Option<&crate::memory::SelfContinuity>,
    relationship_portfolio: Option<&crate::memory::RelationshipPortfolio>,
    relationship_topology: Option<&crate::memory::RelationshipTopology>,
    now_secs: u64,
    profile: MemoryProfile,
) -> Vec<crate::memory::RelationshipSelectionTarget> {
    select_personality_governance_targets(
        self_continuity,
        relationship_portfolio,
        relationship_topology,
        now_secs,
        operator_governance_relation_limit(profile),
    )
}

fn operator_governance_relation_limit(profile: MemoryProfile) -> usize {
    match profile {
        MemoryProfile::Embedded => 1,
        MemoryProfile::Standard => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{
        board_subject_scope_id, CoreRevisionLedger, CoreRevisionRecord,
        RelationshipGovernanceState, RelationshipInheritanceMode, RelationshipPortfolio,
        RelationshipPortfolioEntry, RelationshipTopology, RelationshipTopologyEntry,
        SelfAuthoredCore, SelfContinuity, TurnLedger,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSelfAuthoredCoreStore {
        values: Mutex<HashMap<String, SelfAuthoredCore>>,
    }

    impl SelfAuthoredCoreStore for StubSelfAuthoredCoreStore {
        fn get(&self, scope_id: &str) -> Result<Option<SelfAuthoredCore>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(scope_id)
                .cloned())
        }

        fn set(&self, scope_id: &str, core: &SelfAuthoredCore) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(scope_id.to_string(), core.clone());
            Ok(())
        }

        fn clear(&self, scope_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(scope_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubCoreRevisionLedgerStore {
        values: Mutex<HashMap<String, CoreRevisionLedger>>,
    }

    impl CoreRevisionLedgerStore for StubCoreRevisionLedgerStore {
        fn get(&self, scope_id: &str) -> Result<Option<CoreRevisionLedger>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(scope_id)
                .cloned())
        }

        fn set(&self, scope_id: &str, ledger: &CoreRevisionLedger) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(scope_id.to_string(), ledger.clone());
            Ok(())
        }

        fn clear(&self, scope_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(scope_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipConstitutionStore;

    impl RelationshipConstitutionStore for StubRelationshipConstitutionStore {
        fn get(&self, _scope_id: &str) -> Result<Option<crate::memory::RelationshipConstitution>> {
            Ok(None)
        }

        fn set(
            &self,
            _scope_id: &str,
            _constitution: &crate::memory::RelationshipConstitution,
        ) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipPortfolioStore {
        values: Mutex<HashMap<String, RelationshipPortfolio>>,
    }

    impl RelationshipPortfolioStore for StubRelationshipPortfolioStore {
        fn get(&self, scope_id: &str) -> Result<Option<RelationshipPortfolio>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(scope_id)
                .cloned())
        }

        fn set(&self, scope_id: &str, portfolio: &RelationshipPortfolio) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(scope_id.to_string(), portfolio.clone());
            Ok(())
        }

        fn clear(&self, scope_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(scope_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipTopologyStore {
        values: Mutex<HashMap<String, RelationshipTopology>>,
    }

    impl RelationshipTopologyStore for StubRelationshipTopologyStore {
        fn get(&self, scope_id: &str) -> Result<Option<RelationshipTopology>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(scope_id)
                .cloned())
        }

        fn set(&self, scope_id: &str, topology: &RelationshipTopology) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(scope_id.to_string(), topology.clone());
            Ok(())
        }

        fn clear(&self, scope_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(scope_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfContinuityStore {
        values: Mutex<HashMap<String, SelfContinuity>>,
    }

    impl SelfContinuityStore for StubSelfContinuityStore {
        fn get(&self, chat_id: &str) -> Result<Option<SelfContinuity>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, continuity: &SelfContinuity) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), continuity.clone());
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
    struct StubTurnLedgerStore;

    impl TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn operator_personality_governance_summary_uses_active_relation_and_repair_plan() {
        let subject_id = board_subject_scope_id().to_string();
        let self_authored_core_store = StubSelfAuthoredCoreStore::default();
        let core_revision_ledger_store = StubCoreRevisionLedgerStore::default();
        let relationship_constitution_store = StubRelationshipConstitutionStore;
        let relationship_portfolio_store = StubRelationshipPortfolioStore::default();
        let relationship_topology_store = StubRelationshipTopologyStore::default();
        let self_continuity_store = StubSelfContinuityStore::default();
        let turn_ledger_store = StubTurnLedgerStore;

        self_authored_core_store
            .set(
                &subject_id,
                &SelfAuthoredCore {
                    revision: 3,
                    identity_anchor: "board".to_string(),
                    stability_score: 78,
                    last_reviewed_at: 900,
                    ..SelfAuthoredCore::default()
                },
            )
            .unwrap();
        core_revision_ledger_store
            .set(
                &subject_id,
                &CoreRevisionLedger {
                    entries: vec![CoreRevisionRecord {
                        reviewed_at: 900,
                        ..CoreRevisionRecord::default()
                    }],
                    updated_at: 900,
                },
            )
            .unwrap();
        self_continuity_store
            .set(
                &subject_id,
                &SelfContinuity {
                    last_user_chat_id: "chat-a".to_string(),
                    last_user_channel: "qq".to_string(),
                    updated_at: 950,
                    ..SelfContinuity::default()
                },
            )
            .unwrap();
        relationship_portfolio_store
            .set(
                &subject_id,
                &RelationshipPortfolio {
                    entries: vec![RelationshipPortfolioEntry {
                        scope_id: "rel:qq:chat-a".to_string(),
                        channel: "qq".to_string(),
                        chat_id: "chat-a".to_string(),
                        governance_state: RelationshipGovernanceState::Repair,
                        inheritance_mode: RelationshipInheritanceMode::Guarded,
                        priority_score: 120,
                        reason: "needs_attention".to_string(),
                        needs_runtime_attention: true,
                        next_review_at: 800,
                        last_active_at: 940,
                        ..RelationshipPortfolioEntry::default()
                    }],
                    updated_at: 940,
                },
            )
            .unwrap();
        relationship_topology_store
            .set(
                &subject_id,
                &RelationshipTopology {
                    entries: vec![RelationshipTopologyEntry {
                        scope_id: "rel:qq:chat-a".to_string(),
                        channel: "qq".to_string(),
                        chat_id: "chat-a".to_string(),
                        last_active_at: 940,
                        last_user_turn_at: 940,
                        last_runtime_refresh_at: 900,
                        ..RelationshipTopologyEntry::default()
                    }],
                    updated_at: 940,
                },
            )
            .unwrap();

        let snapshot = inspect_operator_personality_governance(
            &self_authored_core_store,
            &core_revision_ledger_store,
            &relationship_constitution_store,
            &relationship_portfolio_store,
            &relationship_topology_store,
            &self_continuity_store,
            &turn_ledger_store,
            MemoryProfile::Embedded,
            1_000,
        )
        .unwrap()
        .expect("governance snapshot");

        assert!(snapshot.board_core_present);
        assert_eq!(snapshot.board_revision, 3);
        assert_eq!(snapshot.active_relations, 1);
        assert_eq!(snapshot.anchor_chat_id.as_deref(), Some("chat-a"));
        assert_eq!(
            snapshot.relations[0].primary_action,
            "repair_relationship_constitution"
        );
        assert!(snapshot.relations[0].repair_needed);
        assert_eq!(snapshot.relations[0].chat_id, "chat-a");
    }
}
