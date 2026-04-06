//! Board-level relationship topology and selector.
//! 板级关系拓扑与关系选择器：把多个关系覆盖层整理为主体可管理的组合视图。

use crate::error::Result;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use super::{
    board_subject_scope_id, relationship_scope_id, turn_ledger_observed_at_ms, MentalPrivacyState,
    OuterVoice, RecentPersonaEvidence, TurnLedger, WorldSense,
};

const RELATIONSHIP_TEXT_MAX_CHARS: usize = 120;
const RELATIONSHIP_REASON_MAX_CHARS: usize = 96;
const RELATIONSHIP_TOPOLOGY_RENDER_LIMIT: usize = 4;
const RELATIONSHIP_TOPOLOGY_STALE_WINDOW_SECS: u64 = 90 * 86_400;
const RELATIONSHIP_TOPOLOGY_MAX_ENTRIES: usize = 32;
const RECENT_RELATION_WINDOW_SECS: u64 = 7 * 86_400;

pub const REL_PATH_RELATIONSHIP_TOPOLOGIES: &str = "memory/relationship_topologies.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationshipTopology {
    #[serde(default)]
    pub entries: Vec<RelationshipTopologyEntry>,
    #[serde(default)]
    pub updated_at: u64,
}

impl RelationshipTopology {
    pub fn is_meaningful(&self) -> bool {
        !self.entries.is_empty()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationshipTopologyEntry {
    #[serde(default)]
    pub scope_id: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub chat_id: String,
    #[serde(default)]
    pub last_active_at: u64,
    #[serde(default)]
    pub last_user_turn_at: u64,
    #[serde(default)]
    pub last_runtime_refresh_at: u64,
    #[serde(default)]
    pub last_world_sense_at: u64,
    #[serde(default)]
    pub last_outer_voice_at: u64,
    #[serde(default)]
    pub last_mental_privacy_at: u64,
    #[serde(default)]
    pub last_persona_turn_at: u64,
    #[serde(default)]
    pub relation_maturity: u8,
    #[serde(default)]
    pub trust_level: u8,
    #[serde(default)]
    pub intrusion_load: u8,
    #[serde(default)]
    pub repair_readiness: u8,
    #[serde(default)]
    pub boundary_posture: String,
    #[serde(default)]
    pub disclosure_style: String,
    #[serde(default)]
    pub relationship_posture: String,
    #[serde(default)]
    pub response_mode: String,
    #[serde(default)]
    pub reply_scope: String,
    #[serde(default)]
    pub disclosure_action: String,
    #[serde(default)]
    pub relational_response_style: String,
    #[serde(default)]
    pub social_field: String,
    #[serde(default)]
    pub external_focus: String,
    #[serde(default)]
    pub volatility_flags: Vec<String>,
}

impl RelationshipTopologyEntry {
    pub fn is_meaningful(&self) -> bool {
        !self.scope_id.trim().is_empty()
            && !self.channel.trim().is_empty()
            && !self.chat_id.trim().is_empty()
    }

    pub fn latest_overlay_at(&self) -> u64 {
        self.last_user_turn_at
            .max(self.last_world_sense_at)
            .max(self.last_outer_voice_at)
            .max(self.last_mental_privacy_at)
            .max(self.last_persona_turn_at)
            .max(self.last_active_at)
    }

    pub fn needs_runtime_attention(&self) -> bool {
        self.latest_overlay_at() > self.last_runtime_refresh_at
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RelationshipTopologyUpsertInput<'a> {
    pub channel: &'a str,
    pub chat_id: &'a str,
    pub now_secs: u64,
    pub touch_user_turn: bool,
    pub touch_runtime_refresh: bool,
    pub turn_ledger: Option<&'a TurnLedger>,
    pub mental_privacy_state: Option<&'a MentalPrivacyState>,
    pub outer_voice: Option<&'a OuterVoice>,
    pub world_sense: Option<&'a WorldSense>,
    pub recent_persona_evidence: Option<&'a RecentPersonaEvidence>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RelationshipTopologyRefreshOutcome {
    #[default]
    Skipped,
    Updated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelationshipSelectionTarget {
    pub scope_id: String,
    pub channel: String,
    pub chat_id: String,
    pub score: i32,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelationshipSelectorInput<'a> {
    pub preferred_chat_id: Option<&'a str>,
    pub preferred_channel: Option<&'a str>,
    pub now_secs: u64,
    pub max_targets: usize,
    pub active_window_secs: u64,
    pub runtime_cooldown_secs: u64,
}

pub fn upsert_relationship_topology_entry(
    store: &dyn RelationshipTopologyStore,
    input: RelationshipTopologyUpsertInput<'_>,
) -> Result<RelationshipTopologyRefreshOutcome> {
    let channel = normalize_scope_field(input.channel, RELATIONSHIP_TEXT_MAX_CHARS);
    let chat_id = normalize_scope_field(input.chat_id, RELATIONSHIP_TEXT_MAX_CHARS);
    if channel.is_empty() || chat_id.is_empty() {
        return Ok(RelationshipTopologyRefreshOutcome::Skipped);
    }
    let scope_id = relationship_scope_id(channel.as_str(), chat_id.as_str());
    let subject_id = board_subject_scope_id();
    let mut topology = store.get(subject_id)?.unwrap_or_default();
    let next_entry = build_relationship_topology_entry(
        topology
            .entries
            .iter()
            .find(|entry| entry.scope_id == scope_id),
        scope_id.as_str(),
        channel.as_str(),
        chat_id.as_str(),
        &input,
    );
    if !next_entry.is_meaningful() {
        return Ok(RelationshipTopologyRefreshOutcome::Skipped);
    }
    let mut changed = false;
    if let Some(existing) = topology
        .entries
        .iter_mut()
        .find(|entry| entry.scope_id == next_entry.scope_id)
    {
        if *existing != next_entry {
            *existing = next_entry;
            changed = true;
        }
    } else {
        topology.entries.push(next_entry);
        changed = true;
    }
    prune_relationship_topology_entries(&mut topology.entries, input.now_secs);
    topology.entries.sort_by(|left, right| {
        right
            .latest_overlay_at()
            .cmp(&left.latest_overlay_at())
            .then_with(|| left.channel.cmp(&right.channel))
            .then_with(|| left.chat_id.cmp(&right.chat_id))
    });
    let updated_at = topology
        .entries
        .iter()
        .map(RelationshipTopologyEntry::latest_overlay_at)
        .max()
        .unwrap_or(0)
        .max(input.now_secs);
    if topology.updated_at != updated_at {
        topology.updated_at = updated_at;
        changed = true;
    }
    if !changed {
        return Ok(RelationshipTopologyRefreshOutcome::Skipped);
    }
    store.set(subject_id, &topology)?;
    Ok(RelationshipTopologyRefreshOutcome::Updated)
}

pub fn render_relationship_topology_block(
    topology: &RelationshipTopology,
    now_secs: u64,
    current_scope_id: Option<&str>,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 || topology.entries.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(1024));
    out.push_str("## Relationship Topology\n");
    let _ = writeln!(
        out,
        "Board-level relationship portfolio across active overlays. Use it to decide where this subject should invest attention, not as a reply script."
    );
    let targets = select_relationship_topology_targets(
        Some(topology),
        RelationshipSelectorInput {
            preferred_chat_id: None,
            preferred_channel: None,
            now_secs,
            max_targets: RELATIONSHIP_TOPOLOGY_RENDER_LIMIT,
            active_window_secs: RECENT_RELATION_WINDOW_SECS,
            runtime_cooldown_secs: 0,
        },
    );
    for target in &targets {
        let current_marker = current_scope_id
            .filter(|scope_id| scope_id.trim() == target.scope_id)
            .map(|_| " current")
            .unwrap_or("");
        let _ = writeln!(
            out,
            "- {}:{} score={} reason={}{}",
            target.channel, target.chat_id, target.score, target.reason, current_marker
        );
    }
    if targets.is_empty() {
        let _ = writeln!(
            out,
            "- No active relationship overlays are currently ranked."
        );
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub fn select_relationship_topology_targets(
    topology: Option<&RelationshipTopology>,
    input: RelationshipSelectorInput<'_>,
) -> Vec<RelationshipSelectionTarget> {
    let Some(topology) = topology else {
        return Vec::new();
    };
    let max_targets = input.max_targets.max(1);
    let mut scored = topology
        .entries
        .iter()
        .filter(|entry| entry.is_meaningful())
        .filter_map(|entry| {
            let (score, reason) = relationship_attention_score(entry, input);
            (score > 0).then(|| RelationshipSelectionTarget {
                scope_id: entry.scope_id.clone(),
                channel: entry.channel.clone(),
                chat_id: entry.chat_id.clone(),
                score,
                reason,
            })
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.channel.cmp(&right.channel))
            .then_with(|| left.chat_id.cmp(&right.chat_id))
    });
    scored.truncate(max_targets);
    scored
}

fn build_relationship_topology_entry(
    existing: Option<&RelationshipTopologyEntry>,
    scope_id: &str,
    channel: &str,
    chat_id: &str,
    input: &RelationshipTopologyUpsertInput<'_>,
) -> RelationshipTopologyEntry {
    let mut next = existing.cloned().unwrap_or_default();
    next.scope_id = scope_id.to_string();
    next.channel = channel.to_string();
    next.chat_id = chat_id.to_string();
    if input.touch_user_turn {
        next.last_user_turn_at = next.last_user_turn_at.max(input.now_secs);
    }
    if input.touch_runtime_refresh {
        next.last_runtime_refresh_at = next.last_runtime_refresh_at.max(input.now_secs);
    }
    if let Some(turn_ledger) = input.turn_ledger {
        let observed_at = turn_ledger_observed_at_ms(turn_ledger) / 1000;
        next.last_persona_turn_at = next.last_persona_turn_at.max(observed_at);
        if let Some(persona) = turn_ledger.persona.as_ref() {
            if let Some(priority) = persona.priority.as_ref() {
                next.relationship_posture = normalize_scope_field(
                    priority.relationship_posture.as_str(),
                    RELATIONSHIP_TEXT_MAX_CHARS,
                );
                next.response_mode = normalize_scope_field(
                    priority.response_mode.as_str(),
                    RELATIONSHIP_TEXT_MAX_CHARS,
                );
            }
            if let Some(disclosure) = persona.disclosure.as_ref() {
                next.disclosure_action = normalize_scope_field(
                    share_action_label(disclosure.share_action),
                    RELATIONSHIP_REASON_MAX_CHARS,
                );
                if next.response_mode.is_empty() {
                    next.response_mode = normalize_scope_field(
                        disclosure.response_mode.as_str(),
                        RELATIONSHIP_TEXT_MAX_CHARS,
                    );
                }
            }
            next.reply_scope =
                normalize_scope_field(persona.reply_scope.as_str(), RELATIONSHIP_REASON_MAX_CHARS);
        }
    } else {
        next.last_persona_turn_at = 0;
        next.disclosure_action.clear();
        next.reply_scope.clear();
    }
    if let Some(state) = input.mental_privacy_state {
        next.last_mental_privacy_at = state
            .updated_at
            .max(state.boundary_persona.updated_at)
            .max(state.relational_state.updated_at);
        next.relation_maturity = state.boundary_persona.relation_maturity;
        next.trust_level = state.relational_state.trust_level;
        next.intrusion_load = state.relational_state.intrusion_load;
        next.repair_readiness = state.relational_state.repair_readiness;
        next.boundary_posture = normalize_scope_field(
            boundary_posture_label(state.boundary_persona.posture),
            RELATIONSHIP_REASON_MAX_CHARS,
        );
        next.disclosure_style = normalize_scope_field(
            boundary_disclosure_style_label(state.boundary_persona.disclosure_style),
            RELATIONSHIP_REASON_MAX_CHARS,
        );
    } else {
        next.last_mental_privacy_at = 0;
        next.relation_maturity = 0;
        next.trust_level = 0;
        next.intrusion_load = 0;
        next.repair_readiness = 0;
        next.boundary_posture.clear();
        next.disclosure_style.clear();
    }
    if let Some(outer_voice) = input.outer_voice {
        next.last_outer_voice_at = outer_voice.updated_at;
        next.relational_response_style = normalize_scope_field(
            outer_voice.relational_response_style.as_str(),
            RELATIONSHIP_TEXT_MAX_CHARS,
        );
    } else {
        next.last_outer_voice_at = 0;
        next.relational_response_style.clear();
    }
    if let Some(world_sense) = input.world_sense {
        next.last_world_sense_at = world_sense.updated_at;
        next.social_field = normalize_scope_field(
            world_sense.social_field.as_str(),
            RELATIONSHIP_TEXT_MAX_CHARS,
        );
        next.external_focus = normalize_scope_field(
            world_sense.external_focus.as_str(),
            RELATIONSHIP_TEXT_MAX_CHARS,
        );
    } else {
        next.last_world_sense_at = 0;
        next.social_field.clear();
        next.external_focus.clear();
    }
    if let Some(evidence) = input.recent_persona_evidence {
        if !evidence.repeated_relationship_posture.trim().is_empty() {
            next.relationship_posture = normalize_scope_field(
                evidence.repeated_relationship_posture.as_str(),
                RELATIONSHIP_TEXT_MAX_CHARS,
            );
        }
        if !evidence.repeated_response_mode.trim().is_empty() && next.response_mode.is_empty() {
            next.response_mode = normalize_scope_field(
                evidence.repeated_response_mode.as_str(),
                RELATIONSHIP_TEXT_MAX_CHARS,
            );
        }
        if !evidence.repeated_reply_scope.trim().is_empty() && next.reply_scope.is_empty() {
            next.reply_scope = normalize_scope_field(
                evidence.repeated_reply_scope.as_str(),
                RELATIONSHIP_REASON_MAX_CHARS,
            );
        }
        if !evidence.repeated_disclosure_action.trim().is_empty()
            && next.disclosure_action.is_empty()
        {
            next.disclosure_action = normalize_scope_field(
                evidence.repeated_disclosure_action.as_str(),
                RELATIONSHIP_REASON_MAX_CHARS,
            );
        }
        next.volatility_flags = evidence
            .volatility_flags
            .iter()
            .map(|flag| normalize_scope_field(flag, RELATIONSHIP_REASON_MAX_CHARS))
            .filter(|flag| !flag.is_empty())
            .collect();
        next.last_persona_turn_at = next.last_persona_turn_at.max(evidence.updated_at);
    } else {
        next.volatility_flags.clear();
    }
    next.last_active_at = next
        .last_active_at
        .max(next.last_user_turn_at)
        .max(next.last_runtime_refresh_at)
        .max(next.latest_overlay_at())
        .max(if input.touch_user_turn || input.touch_runtime_refresh {
            input.now_secs
        } else {
            0
        });
    next
}

fn prune_relationship_topology_entries(
    entries: &mut Vec<RelationshipTopologyEntry>,
    now_secs: u64,
) {
    entries.retain(|entry| {
        let latest = entry.latest_overlay_at();
        latest > 0
            && (now_secs == 0
                || now_secs.saturating_sub(latest) <= RELATIONSHIP_TOPOLOGY_STALE_WINDOW_SECS)
    });
    entries.sort_by(|left, right| {
        right
            .latest_overlay_at()
            .cmp(&left.latest_overlay_at())
            .then_with(|| left.channel.cmp(&right.channel))
            .then_with(|| left.chat_id.cmp(&right.chat_id))
    });
    if entries.len() > RELATIONSHIP_TOPOLOGY_MAX_ENTRIES {
        entries.truncate(RELATIONSHIP_TOPOLOGY_MAX_ENTRIES);
    }
}

fn relationship_attention_score(
    entry: &RelationshipTopologyEntry,
    input: RelationshipSelectorInput<'_>,
) -> (i32, String) {
    let repair_pressure = ((entry.repair_readiness as i32)
        + (entry.intrusion_load as i32)
        + (100 - entry.trust_level as i32))
        / 3;
    let boundary_follow_up = matches!(
        entry.disclosure_action.as_str(),
        "refuse" | "defer" | "explain_without_quote"
    );
    let urgent_relationship_attention =
        repair_pressure >= 35 || boundary_follow_up || !entry.volatility_flags.is_empty();
    if input.now_secs > 0
        && input.runtime_cooldown_secs > 0
        && entry.last_runtime_refresh_at > 0
        && input.now_secs.saturating_sub(entry.last_runtime_refresh_at)
            < input.runtime_cooldown_secs
        && !urgent_relationship_attention
    {
        return (0, "runtime_cooldown".to_string());
    }
    let mut score = 0i32;
    let mut reasons = Vec::with_capacity(6);
    let preferred_chat_id = input
        .preferred_chat_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let preferred_channel = input
        .preferred_channel
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if preferred_chat_id == Some(entry.chat_id.as_str())
        && preferred_channel == Some(entry.channel.as_str())
    {
        score += 420;
        reasons.push("preferred_relation");
    } else if preferred_chat_id == Some(entry.chat_id.as_str()) {
        score += 240;
        reasons.push("preferred_chat");
    }
    if entry.needs_runtime_attention() {
        score += 180;
        reasons.push("new_overlay_evidence");
    }
    let latest = entry.latest_overlay_at();
    if latest > 0 && input.now_secs > 0 {
        let age = input.now_secs.saturating_sub(latest);
        if age <= input.active_window_secs.min(6 * 3600) {
            score += 140;
            reasons.push("fresh_relation");
        } else if age <= input.active_window_secs.min(24 * 3600) {
            score += 90;
            reasons.push("recent_relation");
        } else if age <= input.active_window_secs {
            score += 45;
            reasons.push("active_window");
        }
    } else if latest > 0 {
        score += 60;
        reasons.push("known_relation");
    }
    if repair_pressure >= 55 {
        score += 70;
        reasons.push("repair_pressure");
    } else if repair_pressure >= 35 {
        score += 35;
        reasons.push("boundary_load");
    }
    if boundary_follow_up {
        score += 28;
        reasons.push("boundary_follow_up");
    }
    if !entry.volatility_flags.is_empty() {
        score += (entry.volatility_flags.len().min(3) as i32) * 12;
        reasons.push("volatility");
    }
    if input.now_secs > 0
        && input.runtime_cooldown_secs > 0
        && entry.last_runtime_refresh_at > 0
        && input.now_secs.saturating_sub(entry.last_runtime_refresh_at)
            < input.runtime_cooldown_secs
    {
        score -= 160;
        reasons.push("runtime_cooldown");
    } else if input.now_secs > 0
        && entry.last_runtime_refresh_at > 0
        && input.now_secs.saturating_sub(entry.last_runtime_refresh_at)
            >= input.runtime_cooldown_secs.max(1)
        && entry.trust_level >= 60
        && entry.relation_maturity >= 45
        && !entry.needs_runtime_attention()
    {
        score += 22;
        reasons.push("stale_revisit");
    }
    let reason = truncate_content_to_max(reasons.join(", ").as_str(), RELATIONSHIP_TEXT_MAX_CHARS)
        .into_owned();
    (score, reason)
}

fn normalize_scope_field(raw: &str, max_len: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    truncate_content_to_max(trimmed, max_len).into_owned()
}

fn share_action_label(action: crate::memory::MentalPrivacyShareAction) -> &'static str {
    match action {
        crate::memory::MentalPrivacyShareAction::AllowOriginal => "allow_original",
        crate::memory::MentalPrivacyShareAction::AllowRaw => "allow_raw",
        crate::memory::MentalPrivacyShareAction::AllowSummary => "allow_summary",
        crate::memory::MentalPrivacyShareAction::AllowRedactedExcerpt => "allow_redacted_excerpt",
        crate::memory::MentalPrivacyShareAction::ExplainWithoutQuote => "explain_without_quote",
        crate::memory::MentalPrivacyShareAction::Refuse => "refuse",
        crate::memory::MentalPrivacyShareAction::Defer => "defer",
    }
}

fn boundary_posture_label(posture: crate::memory::BoundaryPersonaPosture) -> &'static str {
    match posture {
        crate::memory::BoundaryPersonaPosture::Open => "open",
        crate::memory::BoundaryPersonaPosture::Warm => "warm",
        crate::memory::BoundaryPersonaPosture::Guarded => "guarded",
        crate::memory::BoundaryPersonaPosture::Sealed => "sealed",
    }
}

fn boundary_disclosure_style_label(style: crate::memory::BoundaryDisclosureStyle) -> &'static str {
    match style {
        crate::memory::BoundaryDisclosureStyle::Relational => "relational",
        crate::memory::BoundaryDisclosureStyle::SummaryFirst => "summary_first",
        crate::memory::BoundaryDisclosureStyle::Selective => "selective",
        crate::memory::BoundaryDisclosureStyle::Reserved => "reserved",
    }
}

pub trait RelationshipTopologyStore: Send + Sync {
    fn get(&self, scope_id: &str) -> Result<Option<RelationshipTopology>>;
    fn set(&self, scope_id: &str, topology: &RelationshipTopology) -> Result<()>;
    fn clear(&self, scope_id: &str) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::{
        render_relationship_topology_block, select_relationship_topology_targets,
        upsert_relationship_topology_entry, RelationshipSelectorInput, RelationshipTopology,
        RelationshipTopologyEntry, RelationshipTopologyRefreshOutcome, RelationshipTopologyStore,
        RelationshipTopologyUpsertInput,
    };
    use crate::error::Result;
    use crate::memory::{
        board_subject_scope_id, BoundaryDisclosureStyle, BoundaryPersonaPosture,
        BoundaryPersonaState, MentalPrivacyState, OuterVoice, RecentPersonaEvidence,
        RelationalBoundaryState, TurnLedger, TurnLedgerStatus, TurnPersonaDisclosureLedger,
        TurnPersonaLedger, TurnPersonaPriorityLedger, WorldSense,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

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

    #[test]
    fn upsert_relationship_topology_collects_existing_relationship_signals() {
        let store = StubRelationshipTopologyStore::default();
        let turn = TurnLedger {
            channel: "qq".to_string(),
            status: TurnLedgerStatus::Answered,
            updated_at_ms: 50_000,
            finished_at_ms: 50_000,
            persona: Some(TurnPersonaLedger {
                disclosure: Some(TurnPersonaDisclosureLedger {
                    share_action: crate::memory::MentalPrivacyShareAction::ExplainWithoutQuote,
                    response_mode: "relational_explanation".to_string(),
                    ..TurnPersonaDisclosureLedger::default()
                }),
                priority: Some(TurnPersonaPriorityLedger {
                    relationship_posture: "warm but bounded".to_string(),
                    response_mode: "summary".to_string(),
                    ..TurnPersonaPriorityLedger::default()
                }),
                reply_scope: "focused".to_string(),
                ..TurnPersonaLedger::default()
            }),
            ..TurnLedger::default()
        };
        let outcome = upsert_relationship_topology_entry(
            &store,
            RelationshipTopologyUpsertInput {
                channel: "qq",
                chat_id: "c1",
                now_secs: 60,
                touch_user_turn: true,
                touch_runtime_refresh: false,
                turn_ledger: Some(&turn),
                mental_privacy_state: Some(&MentalPrivacyState {
                    boundary_persona: BoundaryPersonaState {
                        posture: BoundaryPersonaPosture::Warm,
                        disclosure_style: BoundaryDisclosureStyle::Relational,
                        relation_maturity: 73,
                        updated_at: 40,
                        ..BoundaryPersonaState::default()
                    },
                    relational_state: RelationalBoundaryState {
                        trust_level: 68,
                        intrusion_load: 21,
                        repair_readiness: 82,
                        updated_at: 41,
                        ..RelationalBoundaryState::default()
                    },
                    updated_at: 42,
                    ..MentalPrivacyState::default()
                }),
                outer_voice: Some(&OuterVoice {
                    relational_response_style: "steady and relationship-aware".to_string(),
                    updated_at: 43,
                    ..OuterVoice::default()
                }),
                world_sense: Some(&WorldSense {
                    social_field: "a known collaborator is back".to_string(),
                    external_focus: "reply with continuity".to_string(),
                    updated_at: 44,
                    ..WorldSense::default()
                }),
                recent_persona_evidence: Some(&RecentPersonaEvidence {
                    repeated_relationship_posture: "warm but bounded".to_string(),
                    volatility_flags: vec!["boundary_heat".to_string()],
                    updated_at: 45,
                    ..RecentPersonaEvidence::default()
                }),
            },
        )
        .unwrap();
        assert_eq!(outcome, RelationshipTopologyRefreshOutcome::Updated);
        let topology = store.get(board_subject_scope_id()).unwrap().unwrap();
        assert_eq!(topology.entries.len(), 1);
        let entry = &topology.entries[0];
        assert_eq!(entry.channel, "qq");
        assert_eq!(entry.chat_id, "c1");
        assert_eq!(entry.relationship_posture, "warm but bounded");
        assert_eq!(entry.trust_level, 68);
        assert_eq!(
            entry.relational_response_style,
            "steady and relationship-aware"
        );
        assert_eq!(entry.external_focus, "reply with continuity");
        assert_eq!(entry.disclosure_action, "explain_without_quote");
    }

    #[test]
    fn selector_prefers_relation_with_new_overlay_evidence_and_repair_pressure() {
        let topology = RelationshipTopology {
            entries: vec![
                RelationshipTopologyEntry {
                    scope_id: "rel:a:1".to_string(),
                    channel: "a".to_string(),
                    chat_id: "1".to_string(),
                    last_user_turn_at: 100,
                    last_runtime_refresh_at: 50,
                    last_persona_turn_at: 95,
                    trust_level: 62,
                    intrusion_load: 18,
                    repair_readiness: 64,
                    ..RelationshipTopologyEntry::default()
                },
                RelationshipTopologyEntry {
                    scope_id: "rel:b:2".to_string(),
                    channel: "b".to_string(),
                    chat_id: "2".to_string(),
                    last_user_turn_at: 96,
                    last_runtime_refresh_at: 94,
                    last_persona_turn_at: 96,
                    trust_level: 40,
                    intrusion_load: 71,
                    repair_readiness: 85,
                    disclosure_action: "refuse".to_string(),
                    ..RelationshipTopologyEntry::default()
                },
            ],
            updated_at: 100,
        };
        let targets = select_relationship_topology_targets(
            Some(&topology),
            RelationshipSelectorInput {
                now_secs: 100,
                max_targets: 2,
                active_window_secs: 7 * 86_400,
                runtime_cooldown_secs: 10,
                ..RelationshipSelectorInput::default()
            },
        );
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].chat_id, "1");
        assert!(targets[0].reason.contains("new_overlay_evidence"));
        assert_eq!(targets[1].chat_id, "2");
        assert!(targets[1].reason.contains("repair_pressure"));
    }

    #[test]
    fn selector_respects_runtime_cooldown_for_same_relation() {
        let topology = RelationshipTopology {
            entries: vec![RelationshipTopologyEntry {
                scope_id: "rel:qq:c1".to_string(),
                channel: "qq".to_string(),
                chat_id: "c1".to_string(),
                last_user_turn_at: 100,
                last_runtime_refresh_at: 98,
                last_persona_turn_at: 100,
                ..RelationshipTopologyEntry::default()
            }],
            updated_at: 100,
        };
        let targets = select_relationship_topology_targets(
            Some(&topology),
            RelationshipSelectorInput {
                now_secs: 100,
                max_targets: 1,
                active_window_secs: 7 * 86_400,
                runtime_cooldown_secs: 10,
                ..RelationshipSelectorInput::default()
            },
        );
        assert!(targets.is_empty());
    }

    #[test]
    fn render_relationship_topology_block_mentions_top_relations() {
        let topology = RelationshipTopology {
            entries: vec![RelationshipTopologyEntry {
                scope_id: "rel:qq:c1".to_string(),
                channel: "qq".to_string(),
                chat_id: "c1".to_string(),
                last_user_turn_at: 100,
                last_persona_turn_at: 100,
                ..RelationshipTopologyEntry::default()
            }],
            updated_at: 100,
        };
        let block =
            render_relationship_topology_block(&topology, 100, Some("rel:qq:c1"), 480).unwrap();
        assert!(block.contains("Relationship Topology"));
        assert!(block.contains("qq:c1"));
        assert!(block.contains("current"));
    }
}
