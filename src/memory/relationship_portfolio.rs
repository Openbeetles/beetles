//! Board-level relationship governance portfolio.
//! 板级关系治理账本：在关系事实层与主体调度/晋升层之间增加正式治理决策。

use crate::error::Result;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use super::{
    board_subject_scope_id, RelationshipSelectionTarget, RelationshipTopology, SelfAuthoredCore,
};

const RELATIONSHIP_PORTFOLIO_REASON_MAX_CHARS: usize = 120;
const RELATIONSHIP_PORTFOLIO_RENDER_LIMIT: usize = 4;
const RELATIONSHIP_PORTFOLIO_STALE_WINDOW_SECS: u64 = 90 * 86_400;
const RELATIONSHIP_PORTFOLIO_MAX_ENTRIES: usize = 32;
const RELATIONSHIP_PORTFOLIO_REPAIR_REVIEW_SECS: u64 = 30 * 60;
const RELATIONSHIP_PORTFOLIO_MAINTAIN_REVIEW_SECS: u64 = 4 * 3600;
const RELATIONSHIP_PORTFOLIO_COOLDOWN_REVIEW_SECS: u64 = 6 * 3600;
const RELATIONSHIP_PORTFOLIO_REVISIT_REVIEW_SECS: u64 = 12 * 3600;
const RELATIONSHIP_PORTFOLIO_DEPRIORITIZE_REVIEW_SECS: u64 = 24 * 3600;

pub const REL_PATH_RELATIONSHIP_PORTFOLIOS: &str = "memory/relationship_portfolios.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationshipPortfolio {
    #[serde(default)]
    pub entries: Vec<RelationshipPortfolioEntry>,
    #[serde(default)]
    pub updated_at: u64,
}

impl RelationshipPortfolio {
    pub fn is_meaningful(&self) -> bool {
        !self.entries.is_empty()
    }

    pub fn entry_for_scope(&self, scope_id: &str) -> Option<&RelationshipPortfolioEntry> {
        let scope_id = scope_id.trim();
        (!scope_id.is_empty())
            .then_some(scope_id)
            .and_then(|scope_id| self.entries.iter().find(|entry| entry.scope_id == scope_id))
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipGovernanceState {
    #[default]
    Maintain,
    Repair,
    CoolDown,
    Deprioritize,
    Revisit,
}

impl RelationshipGovernanceState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Maintain => "maintain",
            Self::Repair => "repair",
            Self::CoolDown => "cool_down",
            Self::Deprioritize => "deprioritize",
            Self::Revisit => "revisit",
        }
    }

    fn review_interval_secs(self) -> u64 {
        match self {
            Self::Maintain => RELATIONSHIP_PORTFOLIO_MAINTAIN_REVIEW_SECS,
            Self::Repair => RELATIONSHIP_PORTFOLIO_REPAIR_REVIEW_SECS,
            Self::CoolDown => RELATIONSHIP_PORTFOLIO_COOLDOWN_REVIEW_SECS,
            Self::Deprioritize => RELATIONSHIP_PORTFOLIO_DEPRIORITIZE_REVIEW_SECS,
            Self::Revisit => RELATIONSHIP_PORTFOLIO_REVISIT_REVIEW_SECS,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipInheritanceMode {
    Full,
    #[default]
    Guarded,
    Limited,
    Quarantined,
}

impl RelationshipInheritanceMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Guarded => "guarded",
            Self::Limited => "limited",
            Self::Quarantined => "quarantined",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationshipPortfolioEntry {
    #[serde(default)]
    pub scope_id: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub chat_id: String,
    #[serde(default)]
    pub governance_state: RelationshipGovernanceState,
    #[serde(default)]
    pub inheritance_mode: RelationshipInheritanceMode,
    #[serde(default)]
    pub priority_score: i32,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub source_updated_at: u64,
    #[serde(default)]
    pub last_active_at: u64,
    #[serde(default)]
    pub needs_runtime_attention: bool,
    #[serde(default)]
    pub last_selected_at: u64,
    #[serde(default)]
    pub next_review_at: u64,
}

impl RelationshipPortfolioEntry {
    pub fn is_meaningful(&self) -> bool {
        !self.scope_id.trim().is_empty()
            && !self.channel.trim().is_empty()
            && !self.chat_id.trim().is_empty()
    }

    pub fn permits_board_level_promotion(&self) -> bool {
        matches!(
            (self.governance_state, self.inheritance_mode),
            (
                RelationshipGovernanceState::Maintain | RelationshipGovernanceState::Revisit,
                RelationshipInheritanceMode::Full | RelationshipInheritanceMode::Guarded
            )
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelationshipPortfolioSelectorInput<'a> {
    pub preferred_chat_id: Option<&'a str>,
    pub preferred_channel: Option<&'a str>,
    pub now_secs: u64,
    pub max_targets: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RelationshipPortfolioSyncOutcome {
    #[default]
    Skipped,
    Updated,
}

pub fn sync_relationship_portfolio(
    store: &dyn RelationshipPortfolioStore,
    topology: Option<&RelationshipTopology>,
    self_authored_core: Option<&SelfAuthoredCore>,
    now_secs: u64,
) -> Result<Option<RelationshipPortfolio>> {
    let subject_id = board_subject_scope_id();
    let existing = store.get(subject_id)?.unwrap_or_default();
    let mut next =
        derive_relationship_portfolio(topology, self_authored_core, now_secs, Some(&existing));
    if next.entries.is_empty() {
        if existing.is_meaningful() {
            store.clear(subject_id)?;
        }
        return Ok(None);
    }
    if next != existing {
        store.set(subject_id, &next)?;
    }
    Ok(Some(std::mem::take(&mut next)))
}

pub fn touch_relationship_portfolio_selection(
    store: &dyn RelationshipPortfolioStore,
    scope_id: &str,
    now_secs: u64,
) -> Result<()> {
    let subject_id = board_subject_scope_id();
    let mut portfolio = match store.get(subject_id)? {
        Some(portfolio) => portfolio,
        None => return Ok(()),
    };
    let Some(entry) = portfolio
        .entries
        .iter_mut()
        .find(|entry| entry.scope_id == scope_id.trim())
    else {
        return Ok(());
    };
    let next_review_at = now_secs.saturating_add(entry.governance_state.review_interval_secs());
    if entry.last_selected_at == now_secs && entry.next_review_at == next_review_at {
        return Ok(());
    }
    entry.last_selected_at = now_secs;
    entry.next_review_at = next_review_at;
    portfolio.updated_at = portfolio.updated_at.max(now_secs);
    store.set(subject_id, &portfolio)
}

pub fn select_relationship_portfolio_targets(
    portfolio: Option<&RelationshipPortfolio>,
    input: RelationshipPortfolioSelectorInput<'_>,
) -> Vec<RelationshipSelectionTarget> {
    let Some(portfolio) = portfolio else {
        return Vec::new();
    };
    let max_targets = input.max_targets.max(1);
    let preferred_chat_id = input
        .preferred_chat_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let preferred_channel = input
        .preferred_channel
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut scored = portfolio
        .entries
        .iter()
        .filter(|entry| entry.is_meaningful())
        .filter_map(|entry| {
            let preferred_exact = preferred_chat_id == Some(entry.chat_id.as_str())
                && preferred_channel == Some(entry.channel.as_str());
            let preferred_chat = preferred_chat_id == Some(entry.chat_id.as_str());
            let review_due = input.now_secs == 0
                || entry.next_review_at == 0
                || input.now_secs >= entry.next_review_at;
            if !review_due && !entry.needs_runtime_attention && !preferred_exact {
                return None;
            }
            let mut score = entry.priority_score;
            let mut reasons = vec![entry.governance_state.label().to_string()];
            if preferred_exact {
                score += 240;
                reasons.push("preferred_relation".to_string());
            } else if preferred_chat {
                score += 140;
                reasons.push("preferred_chat".to_string());
            }
            if entry.needs_runtime_attention {
                score += 120;
                reasons.push("new_overlay_evidence".to_string());
            }
            if review_due {
                score += 35;
                reasons.push("review_due".to_string());
            }
            if input.now_secs > 0
                && entry.last_selected_at > 0
                && input.now_secs.saturating_sub(entry.last_selected_at) < 10 * 60
                && !entry.needs_runtime_attention
            {
                score -= 160;
                reasons.push("recently_selected".to_string());
            }
            if input.now_secs > 0 && entry.last_active_at > 0 {
                let age = input.now_secs.saturating_sub(entry.last_active_at);
                if age <= 6 * 3600 {
                    score += 60;
                    reasons.push("fresh_relation".to_string());
                } else if age <= 24 * 3600 {
                    score += 35;
                    reasons.push("recent_relation".to_string());
                }
            }
            (score > 0).then(|| RelationshipSelectionTarget {
                scope_id: entry.scope_id.clone(),
                channel: entry.channel.clone(),
                chat_id: entry.chat_id.clone(),
                score,
                reason: truncate_content_to_max(
                    reasons.join(", ").as_str(),
                    RELATIONSHIP_PORTFOLIO_REASON_MAX_CHARS,
                )
                .into_owned(),
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

pub fn render_relationship_portfolio_block(
    portfolio: &RelationshipPortfolio,
    now_secs: u64,
    current_scope_id: Option<&str>,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 || portfolio.entries.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(1024));
    out.push_str("## Relationship Portfolio\n");
    out.push_str(
        "Board-level governance over relationship overlays. Use it to decide how much authority, inheritance, and runtime attention each relation should receive.\n",
    );
    let mut rendered_scope_ids = Vec::with_capacity(RELATIONSHIP_PORTFOLIO_RENDER_LIMIT + 1);
    for target in select_relationship_portfolio_targets(
        Some(portfolio),
        RelationshipPortfolioSelectorInput {
            preferred_chat_id: None,
            preferred_channel: None,
            now_secs,
            max_targets: RELATIONSHIP_PORTFOLIO_RENDER_LIMIT,
        },
    ) {
        if let Some(entry) = portfolio.entry_for_scope(&target.scope_id) {
            rendered_scope_ids.push(entry.scope_id.clone());
            let current_marker = current_scope_id
                .filter(|scope_id| scope_id.trim() == target.scope_id)
                .map(|_| " current")
                .unwrap_or("");
            let _ = writeln!(
                out,
                "- {}:{} state={} inheritance={} score={} reason={}{}",
                entry.channel,
                entry.chat_id,
                entry.governance_state.label(),
                entry.inheritance_mode.label(),
                target.score,
                entry.reason,
                current_marker
            );
        }
    }
    let current_scope_id = current_scope_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(current_scope_id) = current_scope_id {
        if let Some(entry) = portfolio.entry_for_scope(current_scope_id) {
            if !rendered_scope_ids
                .iter()
                .any(|scope_id| scope_id == current_scope_id)
            {
                let _ = writeln!(
                    out,
                    "- {}:{} state={} inheritance={} score={} reason={} current",
                    entry.channel,
                    entry.chat_id,
                    entry.governance_state.label(),
                    entry.inheritance_mode.label(),
                    entry.priority_score,
                    entry.reason
                );
            }
        } else {
            let _ = writeln!(
                out,
                "- Current relationship has no board-level governance entry yet."
            );
        }
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

fn derive_relationship_portfolio(
    topology: Option<&RelationshipTopology>,
    self_authored_core: Option<&SelfAuthoredCore>,
    now_secs: u64,
    existing: Option<&RelationshipPortfolio>,
) -> RelationshipPortfolio {
    let Some(topology) = topology else {
        return RelationshipPortfolio::default();
    };
    let mut entries = topology
        .entries
        .iter()
        .filter(|entry| entry.is_meaningful())
        .map(|entry| {
            derive_relationship_portfolio_entry(entry, self_authored_core, now_secs, existing)
        })
        .collect::<Vec<_>>();
    entries.retain(|entry| {
        entry.last_active_at > 0
            && (now_secs == 0
                || now_secs.saturating_sub(entry.last_active_at)
                    <= RELATIONSHIP_PORTFOLIO_STALE_WINDOW_SECS)
    });
    entries.sort_by(|left, right| {
        right
            .priority_score
            .cmp(&left.priority_score)
            .then_with(|| right.last_active_at.cmp(&left.last_active_at))
            .then_with(|| left.channel.cmp(&right.channel))
            .then_with(|| left.chat_id.cmp(&right.chat_id))
    });
    if entries.len() > RELATIONSHIP_PORTFOLIO_MAX_ENTRIES {
        entries.truncate(RELATIONSHIP_PORTFOLIO_MAX_ENTRIES);
    }
    RelationshipPortfolio {
        updated_at: entries
            .iter()
            .map(|entry| entry.source_updated_at.max(entry.last_selected_at))
            .max()
            .unwrap_or(0)
            .max(now_secs.min(topology.updated_at.max(now_secs))),
        entries,
    }
}

fn derive_relationship_portfolio_entry(
    entry: &super::RelationshipTopologyEntry,
    self_authored_core: Option<&SelfAuthoredCore>,
    now_secs: u64,
    existing: Option<&RelationshipPortfolio>,
) -> RelationshipPortfolioEntry {
    let prior = existing.and_then(|portfolio| portfolio.entry_for_scope(&entry.scope_id));
    let trust = entry.trust_level as i32;
    let maturity = entry.relation_maturity as i32;
    let intrusion = entry.intrusion_load as i32;
    let repair = entry.repair_readiness as i32;
    let volatility = (entry.volatility_flags.len().min(3) as i32) * 12;
    let low_trust = 100 - trust;
    let boundary_severity = match entry.boundary_posture.as_str() {
        "sealed" => 42,
        "guarded" => 20,
        _ => 0,
    };
    let disclosure_severity = match entry.disclosure_action.as_str() {
        "refuse" => 36,
        "defer" => 28,
        "explain_without_quote" => 18,
        _ => 0,
    };
    let strain =
        ((intrusion + low_trust + boundary_severity + disclosure_severity) / 4) + volatility;
    let review_due =
        prior.is_none_or(|prior| prior.next_review_at == 0 || now_secs >= prior.next_review_at);
    let stale_for_revisit = entry.last_runtime_refresh_at > 0
        && now_secs > 0
        && now_secs.saturating_sub(entry.last_runtime_refresh_at)
            >= RELATIONSHIP_PORTFOLIO_REVISIT_REVIEW_SECS;
    let governance_state = if entry.boundary_posture == "sealed"
        || trust <= 20
        || intrusion >= 75
        || matches!(entry.disclosure_action.as_str(), "refuse" | "defer")
    {
        RelationshipGovernanceState::CoolDown
    } else if repair >= 50 && (strain >= 35 || !entry.volatility_flags.is_empty()) {
        RelationshipGovernanceState::Repair
    } else if stale_for_revisit && trust >= 55 && maturity >= 45 {
        RelationshipGovernanceState::Revisit
    } else if trust < 40 || maturity < 25 || strain >= 50 {
        RelationshipGovernanceState::Deprioritize
    } else {
        RelationshipGovernanceState::Maintain
    };
    let inheritance_mode = if matches!(governance_state, RelationshipGovernanceState::CoolDown) {
        RelationshipInheritanceMode::Quarantined
    } else if strain >= 55 || matches!(governance_state, RelationshipGovernanceState::Deprioritize)
    {
        RelationshipInheritanceMode::Limited
    } else if self_authored_core.is_some()
        && trust >= 70
        && maturity >= 60
        && entry.volatility_flags.is_empty()
        && entry.boundary_posture != "guarded"
    {
        RelationshipInheritanceMode::Full
    } else {
        RelationshipInheritanceMode::Guarded
    };
    let mut reasons = Vec::with_capacity(5);
    reasons.push(governance_state.label());
    if entry.needs_runtime_attention() {
        reasons.push("new_overlay_evidence");
    }
    if matches!(
        inheritance_mode,
        RelationshipInheritanceMode::Quarantined | RelationshipInheritanceMode::Limited
    ) {
        reasons.push(inheritance_mode.label());
    }
    if !entry.volatility_flags.is_empty() {
        reasons.push("volatile");
    }
    if review_due {
        reasons.push("review_due");
    }
    let base_priority = match governance_state {
        RelationshipGovernanceState::Repair => 320,
        RelationshipGovernanceState::Maintain => 280,
        RelationshipGovernanceState::Revisit => 230,
        RelationshipGovernanceState::CoolDown => 170,
        RelationshipGovernanceState::Deprioritize => 110,
    };
    let priority_score = base_priority
        + (maturity / 3)
        + (trust / 2)
        + if entry.needs_runtime_attention() {
            90
        } else {
            0
        }
        + if review_due { 20 } else { 0 }
        - intrusion
        - volatility;
    let default_next_review_at = now_secs.saturating_add(governance_state.review_interval_secs());
    RelationshipPortfolioEntry {
        scope_id: entry.scope_id.clone(),
        channel: entry.channel.clone(),
        chat_id: entry.chat_id.clone(),
        governance_state,
        inheritance_mode,
        priority_score,
        reason: truncate_content_to_max(
            reasons.join(", ").as_str(),
            RELATIONSHIP_PORTFOLIO_REASON_MAX_CHARS,
        )
        .into_owned(),
        source_updated_at: entry.latest_overlay_at().max(entry.last_runtime_refresh_at),
        last_active_at: entry.last_active_at.max(entry.latest_overlay_at()),
        needs_runtime_attention: entry.needs_runtime_attention(),
        last_selected_at: prior.map(|entry| entry.last_selected_at).unwrap_or(0),
        next_review_at: prior
            .map(|entry| entry.next_review_at)
            .filter(|next_review_at| *next_review_at > now_secs && !entry.needs_runtime_attention())
            .unwrap_or(default_next_review_at),
    }
}

pub trait RelationshipPortfolioStore: Send + Sync {
    fn get(&self, scope_id: &str) -> Result<Option<RelationshipPortfolio>>;
    fn set(&self, scope_id: &str, portfolio: &RelationshipPortfolio) -> Result<()>;
    fn clear(&self, scope_id: &str) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::{
        render_relationship_portfolio_block, select_relationship_portfolio_targets,
        sync_relationship_portfolio, touch_relationship_portfolio_selection,
        RelationshipGovernanceState, RelationshipInheritanceMode, RelationshipPortfolio,
        RelationshipPortfolioSelectorInput, RelationshipPortfolioStore,
    };
    use crate::error::Result;
    use crate::memory::{
        board_subject_scope_id, RelationshipTopology, RelationshipTopologyEntry, SelfAuthoredCore,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

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

    fn stable_topology() -> RelationshipTopology {
        RelationshipTopology {
            entries: vec![RelationshipTopologyEntry {
                scope_id: "rel:qq:stable".to_string(),
                channel: "qq".to_string(),
                chat_id: "stable".to_string(),
                relation_maturity: 80,
                trust_level: 82,
                intrusion_load: 8,
                repair_readiness: 15,
                boundary_posture: "warm".to_string(),
                last_active_at: 500,
                last_user_turn_at: 500,
                last_world_sense_at: 500,
                last_outer_voice_at: 500,
                last_mental_privacy_at: 500,
                last_persona_turn_at: 500,
                ..RelationshipTopologyEntry::default()
            }],
            updated_at: 500,
        }
    }

    #[test]
    fn stable_relationship_becomes_full_maintain_with_board_core() {
        let store = StubRelationshipPortfolioStore::default();
        let portfolio = sync_relationship_portfolio(
            &store,
            Some(&stable_topology()),
            Some(&SelfAuthoredCore {
                identity_anchor: "board".to_string(),
                updated_at: 10,
                ..SelfAuthoredCore::default()
            }),
            600,
        )
        .unwrap()
        .unwrap();
        let entry = portfolio.entry_for_scope("rel:qq:stable").unwrap();
        assert_eq!(
            entry.governance_state,
            RelationshipGovernanceState::Maintain
        );
        assert_eq!(entry.inheritance_mode, RelationshipInheritanceMode::Full);
        assert!(entry.permits_board_level_promotion());
    }

    #[test]
    fn strained_relationship_is_quarantined_from_board_promotion() {
        let store = StubRelationshipPortfolioStore::default();
        let topology = RelationshipTopology {
            entries: vec![RelationshipTopologyEntry {
                scope_id: "rel:qq:strained".to_string(),
                channel: "qq".to_string(),
                chat_id: "strained".to_string(),
                trust_level: 18,
                intrusion_load: 88,
                repair_readiness: 70,
                boundary_posture: "sealed".to_string(),
                disclosure_action: "refuse".to_string(),
                last_active_at: 600,
                last_user_turn_at: 600,
                ..RelationshipTopologyEntry::default()
            }],
            updated_at: 600,
        };
        let portfolio = sync_relationship_portfolio(&store, Some(&topology), None, 600)
            .unwrap()
            .unwrap();
        let entry = portfolio.entry_for_scope("rel:qq:strained").unwrap();
        assert_eq!(
            entry.governance_state,
            RelationshipGovernanceState::CoolDown
        );
        assert_eq!(
            entry.inheritance_mode,
            RelationshipInheritanceMode::Quarantined
        );
        assert!(!entry.permits_board_level_promotion());
    }

    #[test]
    fn selector_prefers_due_or_attention_relations() {
        let portfolio = RelationshipPortfolio {
            entries: vec![
                super::RelationshipPortfolioEntry {
                    scope_id: "rel:qq:a".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "a".to_string(),
                    governance_state: RelationshipGovernanceState::Maintain,
                    inheritance_mode: RelationshipInheritanceMode::Guarded,
                    priority_score: 220,
                    reason: "maintain".to_string(),
                    source_updated_at: 10,
                    last_active_at: 10,
                    needs_runtime_attention: false,
                    last_selected_at: 90,
                    next_review_at: 10_000,
                },
                super::RelationshipPortfolioEntry {
                    scope_id: "rel:qq:b".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "b".to_string(),
                    governance_state: RelationshipGovernanceState::Repair,
                    inheritance_mode: RelationshipInheritanceMode::Limited,
                    priority_score: 260,
                    reason: "repair".to_string(),
                    source_updated_at: 200,
                    last_active_at: 200,
                    needs_runtime_attention: true,
                    last_selected_at: 0,
                    next_review_at: 10_000,
                },
            ],
            updated_at: 200,
        };
        let selected = select_relationship_portfolio_targets(
            Some(&portfolio),
            RelationshipPortfolioSelectorInput {
                preferred_chat_id: None,
                preferred_channel: None,
                now_secs: 300,
                max_targets: 2,
            },
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].chat_id, "b");
    }

    #[test]
    fn touch_selection_updates_review_window() {
        let store = StubRelationshipPortfolioStore::default();
        store
            .set(
                board_subject_scope_id(),
                &RelationshipPortfolio {
                    entries: vec![super::RelationshipPortfolioEntry {
                        scope_id: "rel:qq:a".to_string(),
                        channel: "qq".to_string(),
                        chat_id: "a".to_string(),
                        governance_state: RelationshipGovernanceState::Repair,
                        inheritance_mode: RelationshipInheritanceMode::Limited,
                        priority_score: 200,
                        reason: "repair".to_string(),
                        source_updated_at: 1,
                        last_active_at: 1,
                        needs_runtime_attention: true,
                        last_selected_at: 0,
                        next_review_at: 0,
                    }],
                    updated_at: 1,
                },
            )
            .unwrap();
        touch_relationship_portfolio_selection(&store, "rel:qq:a", 100).unwrap();
        let portfolio = store.get(board_subject_scope_id()).unwrap().unwrap();
        let entry = portfolio.entry_for_scope("rel:qq:a").unwrap();
        assert_eq!(entry.last_selected_at, 100);
        assert!(entry.next_review_at > 100);
    }

    #[test]
    fn render_block_marks_current_entry() {
        let block = render_relationship_portfolio_block(
            &RelationshipPortfolio {
                entries: vec![super::RelationshipPortfolioEntry {
                    scope_id: "rel:qq:a".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "a".to_string(),
                    governance_state: RelationshipGovernanceState::Maintain,
                    inheritance_mode: RelationshipInheritanceMode::Guarded,
                    priority_score: 220,
                    reason: "maintain".to_string(),
                    source_updated_at: 10,
                    last_active_at: 10,
                    needs_runtime_attention: true,
                    last_selected_at: 0,
                    next_review_at: 0,
                }],
                updated_at: 10,
            },
            20,
            Some("rel:qq:a"),
            512,
        )
        .unwrap();
        assert!(block.contains("current"));
        assert!(block.contains("inheritance=guarded"));
    }
}
