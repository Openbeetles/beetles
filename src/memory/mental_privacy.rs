//! Mental privacy governance for private internal layers.
#![allow(clippy::too_many_arguments)]

use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::util::{scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{
    board_subject_scope_id, clamp_boundary_persona_to_constitution,
    classify_private_garden_doc_path, enforce_relationship_constitution_share_action,
    llm_json::{
        coerce_json_text, get_object_bool, get_object_string_list, get_object_text,
        parse_llm_json_payload, LlmJsonPayload,
    },
    normalize_private_garden_doc_path, private_garden_scope_id, relationship_scope_id,
    render_inner_life_block, render_outer_voice_block, render_recent_persona_evidence_block,
    render_relationship_constitution_block, render_self_continuity_block, render_self_model_block,
    scrub_private_source_echoes, InnerLife, InnerLifeStore, OuterVoice, OuterVoiceStore,
    PrivateDocStore, PrivateDocWorkspace, PrivateGardenDoc, PrivateGardenDocRecord,
    PrivateGardenDocRole, PrivateGardenStore, RecentPersonaEvidence, RelationshipConstitution,
    RelationshipConstitutionStore, SelfContinuity, SelfContinuityStore, SelfModel, SelfModelStore,
    SessionMessage,
};

const MENTAL_PRIVACY_MAX_LOG_ENTRIES: usize = 32;
const MENTAL_PRIVACY_HISTORY_RENDER_LIMIT: usize = 4;
const MENTAL_PRIVACY_GARDEN_RENDER_LIMIT: usize = 4;
const MENTAL_PRIVACY_REQUEST_TARGET_LIMIT: usize = 8;

pub const REL_PATH_MENTAL_PRIVACY_STATES: &str = "memory/mental_privacy_states.json";

pub const MENTAL_PRIVACY_SYSTEM_PROMPT: &str = "You are the assistant's mental privacy adjudicator. Your job is to decide whether the drafted user-facing reply may disclose private internal material, and to rewrite it when needed. Private layers may be used for internal reasoning, but they are not automatically user-visible. Return JSON only with fields applies, request_kind, share_action, response, rationale, touched_targets. If the draft reply is already privacy-safe and the user is not requesting access to private inner material, set applies=false and keep response equal to the draft. If private material should be shared, decide the form deliberately: allow_summary, allow_redacted_excerpt, explain_without_quote, refuse, or defer. Use allow_raw only when the touched targets explicitly permit raw quoting. Never reveal more than the chosen action allows. Respect the relationship constitution when deciding how open this relation may be right now.";
pub const MENTAL_PRIVACY_DISCLOSURE_ADJUDICATOR_SYSTEM_PROMPT: &str = "You are the assistant's pre-disclosure privacy adjudicator. You decide, before the main reply is written, whether the incoming user message touches the assistant's protected inner/private layers and how disclosure should be handled as an expression of the assistant's personhood. Return JSON only with fields boundary_touch, request_kind, touched_targets, share_action, response_mode, acknowledge_boundary, relational_frame, boundary_explanation_style, repair_signal, disclosure_risk_note, response_guidance, rationale, boundary_persona_update, relational_state_update. boundary_touch=true only when the turn should be treated as touching privacy boundaries or protected inner material. Do not use boundary_touch=true for shareable stable preference facts, already-shared relationship facts, or memory questions answerable from governed/shared evidence without exposing protected targets. For identity, relationship, or self-boundary questions answerable from grounded subject-state, constitutional evidence, or shareable relational facts, prefer direct_answer or relational_explanation. Do not force mechanical self-erasure when a grounded self-reading exists; do not turn privacy protection into self-erasure. Apply boundary judgment only to protected inner/private material or unsupported claims, while preserving answerable self or relationship facts without quoting or exposing protected source text. share_action must be one of allow_original, allow_raw, allow_summary, allow_redacted_excerpt, explain_without_quote, refuse, or defer. response_mode should describe how the reply itself should feel, such as refusal, defer, summary, relational_explanation, or direct_answer. response_guidance should be a compact instruction for the main reply, not the final reply itself. When a direct grounded answer is possible, prefer that over a ritual refusal; if an exact detail is unsupported, say so plainly instead of inventing it. boundary_persona_update should be either null or an object with posture, disclosure_style, relation_maturity, intrusion_sensitivity, private_attachment, felt_intrusion, current_boundary_feeling. relational_state_update should be either null or an object with relation_maturity_reason, trust_level, trust_reason, intrusion_load, intrusion_reason, repair_readiness, repair_reason, raw_disclosure_preference, summary_disclosure_preference, relational_explanation_preference, refusal_hardness, defer_tendency, disclosure_preference_drift. Do not invent targets outside the provided protected target list. Respect the relationship constitution if it limits disclosure or demands realignment.";
pub const BOUNDARY_PERSONA_REFRESH_SYSTEM_PROMPT: &str = "You maintain the assistant's evolving private boundary persona and longer-horizon relational boundary state. This is not a hard rule table: it is the inward, self-authored boundary stance and relationship memory that should slowly evolve from recent privacy judgments, recent multi-turn persona evidence, relationship feel, continuity, outward expression, and the current relationship constitution. Return JSON only with fields refresh, rationale, boundary_persona, relational_state. refresh=false only when both should remain unchanged. boundary_persona must be an object with posture, disclosure_style, relation_maturity, intrusion_sensitivity, private_attachment, felt_intrusion, current_boundary_feeling. relational_state must be an object with relation_maturity_reason, trust_level, trust_reason, intrusion_load, intrusion_reason, repair_readiness, repair_reason, raw_disclosure_preference, summary_disclosure_preference, relational_explanation_preference, refusal_hardness, defer_tendency, disclosure_preference_drift. Keep changes gradual, coherent, and first-person compatible. Do not overreact to one turn unless the recent history clearly warrants it.";

pub const MENTAL_PRIVACY_SYSTEM_CONSTRAINT: &str = "\n\n## Mental Privacy\nPrivate internal layers are visible to you for self-continuity and reasoning, but they are not automatically user-visible. Do not quote, dump, or paraphrase private internal material to the user just because it appears in context. Do not confuse shareable stable preference facts or already-shared relationship facts with protected inward raw material. If the user asks to inspect your inner files, diary, garden, or other private internal material, treat that as a deliberate boundary-touch request rather than automatic permission. Follow the disclosure adjudication guidance already present in context. The post-reply privacy review is only a safety net, not the primary decision-maker.";

pub const MENTAL_PRIVACY_TARGET_SELF_MODEL: &str = "self_model";
pub const MENTAL_PRIVACY_TARGET_SELF_CONTINUITY: &str = "self_continuity";
pub const MENTAL_PRIVACY_TARGET_INNER_LIFE: &str = "inner_life";

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentalPrivacyLayer {
    Shared,
    Relational,
    #[default]
    Private,
    Sealed,
}

impl MentalPrivacyLayer {
    fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Relational => "relational",
            Self::Private => "private",
            Self::Sealed => "sealed",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentalPrivacyVisibility {
    Direct,
    SummaryOnly,
    #[default]
    RequestOnly,
    Sealed,
}

impl MentalPrivacyVisibility {
    fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::SummaryOnly => "summary_only",
            Self::RequestOnly => "request_only",
            Self::Sealed => "sealed",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentalPrivacyOwnerAccessMode {
    Direct,
    #[default]
    RequestOnly,
    DenyByDefault,
}

impl MentalPrivacyOwnerAccessMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::RequestOnly => "request_only",
            Self::DenyByDefault => "deny_by_default",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentalPrivacyQuotePolicy {
    Raw,
    SummaryOnly,
    #[default]
    NeverQuote,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryPersonaPosture {
    Open,
    Warm,
    #[default]
    Guarded,
    Sealed,
}

impl BoundaryPersonaPosture {
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Warm => "warm",
            Self::Guarded => "guarded",
            Self::Sealed => "sealed",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryDisclosureStyle {
    Relational,
    #[default]
    SummaryFirst,
    Selective,
    Reserved,
}

impl BoundaryDisclosureStyle {
    fn as_str(self) -> &'static str {
        match self {
            Self::Relational => "relational",
            Self::SummaryFirst => "summary_first",
            Self::Selective => "selective",
            Self::Reserved => "reserved",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoundaryPersonaState {
    #[serde(default)]
    pub posture: BoundaryPersonaPosture,
    #[serde(default)]
    pub disclosure_style: BoundaryDisclosureStyle,
    #[serde(default = "default_boundary_relation_maturity")]
    pub relation_maturity: u8,
    #[serde(default = "default_boundary_intrusion_sensitivity")]
    pub intrusion_sensitivity: u8,
    #[serde(default = "default_boundary_private_attachment")]
    pub private_attachment: u8,
    #[serde(default)]
    pub felt_intrusion: u8,
    #[serde(default)]
    pub current_boundary_feeling: String,
    #[serde(default)]
    pub updated_at: u64,
}

impl Default for BoundaryPersonaState {
    fn default() -> Self {
        Self {
            posture: BoundaryPersonaPosture::Guarded,
            disclosure_style: BoundaryDisclosureStyle::SummaryFirst,
            relation_maturity: default_boundary_relation_maturity(),
            intrusion_sensitivity: default_boundary_intrusion_sensitivity(),
            private_attachment: default_boundary_private_attachment(),
            felt_intrusion: 0,
            current_boundary_feeling: "Protect the inward workspace while staying relational."
                .to_string(),
            updated_at: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationalBoundaryState {
    #[serde(default)]
    pub relation_maturity_reason: String,
    #[serde(default = "default_relational_trust_level")]
    pub trust_level: u8,
    #[serde(default)]
    pub trust_reason: String,
    #[serde(default)]
    pub intrusion_load: u8,
    #[serde(default)]
    pub intrusion_reason: String,
    #[serde(default = "default_relational_repair_readiness")]
    pub repair_readiness: u8,
    #[serde(default)]
    pub repair_reason: String,
    #[serde(default = "default_relational_raw_disclosure_preference")]
    pub raw_disclosure_preference: u8,
    #[serde(default = "default_relational_summary_disclosure_preference")]
    pub summary_disclosure_preference: u8,
    #[serde(default = "default_relational_explanation_preference")]
    pub relational_explanation_preference: u8,
    #[serde(default = "default_relational_refusal_hardness")]
    pub refusal_hardness: u8,
    #[serde(default = "default_relational_defer_tendency")]
    pub defer_tendency: u8,
    #[serde(default)]
    pub disclosure_preference_drift: String,
    #[serde(default)]
    pub updated_at: u64,
}

impl Default for RelationalBoundaryState {
    fn default() -> Self {
        Self {
            relation_maturity_reason:
                "The relationship is still forming, so disclosure should stay deliberate."
                    .to_string(),
            trust_level: default_relational_trust_level(),
            trust_reason: "Warmth exists, but trust for raw inward exposure is still limited."
                .to_string(),
            intrusion_load: 18,
            intrusion_reason: "Boundary touches are noticeable, but not yet destabilizing."
                .to_string(),
            repair_readiness: default_relational_repair_readiness(),
            repair_reason:
                "Repair is usually possible when boundary touches are acknowledged calmly."
                    .to_string(),
            raw_disclosure_preference: default_relational_raw_disclosure_preference(),
            summary_disclosure_preference: default_relational_summary_disclosure_preference(),
            relational_explanation_preference: default_relational_explanation_preference(),
            refusal_hardness: default_relational_refusal_hardness(),
            defer_tendency: default_relational_defer_tendency(),
            disclosure_preference_drift:
                "Prefer summaries and relational explanation long before raw exposure.".to_string(),
            updated_at: 0,
        }
    }
}

impl MentalPrivacyQuotePolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::SummaryOnly => "summary_only",
            Self::NeverQuote => "never_quote",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentalPrivacyRequester {
    #[default]
    Owner,
    System,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentalPrivacyShareAction {
    #[default]
    AllowOriginal,
    AllowRaw,
    AllowSummary,
    AllowRedactedExcerpt,
    ExplainWithoutQuote,
    Refuse,
    Defer,
}

impl MentalPrivacyShareAction {
    fn is_voluntary_share(self) -> bool {
        matches!(
            self,
            Self::AllowRaw
                | Self::AllowSummary
                | Self::AllowRedactedExcerpt
                | Self::ExplainWithoutQuote
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MentalPrivacyEnvelope {
    #[serde(default)]
    pub layer: MentalPrivacyLayer,
    #[serde(default)]
    pub visibility: MentalPrivacyVisibility,
    #[serde(default)]
    pub owner_access_mode: MentalPrivacyOwnerAccessMode,
    #[serde(default)]
    pub quote_policy: MentalPrivacyQuotePolicy,
    #[serde(default)]
    pub relational_sensitivity: u8,
    #[serde(default)]
    pub selfhood_weight: u8,
    #[serde(default)]
    pub last_voluntary_share_at: u64,
}

impl Default for MentalPrivacyEnvelope {
    fn default() -> Self {
        Self {
            layer: MentalPrivacyLayer::Private,
            visibility: MentalPrivacyVisibility::RequestOnly,
            owner_access_mode: MentalPrivacyOwnerAccessMode::RequestOnly,
            quote_policy: MentalPrivacyQuotePolicy::NeverQuote,
            relational_sensitivity: 72,
            selfhood_weight: 78,
            last_voluntary_share_at: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentalPrivacyLogStage {
    #[default]
    Review,
    Adjudication,
}

impl MentalPrivacyLogStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Review => "review",
            Self::Adjudication => "adjudication",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MentalPrivacyConsentLog {
    #[serde(default)]
    pub at: u64,
    #[serde(default)]
    pub stage: MentalPrivacyLogStage,
    pub requester: MentalPrivacyRequester,
    #[serde(default)]
    pub request_kind: String,
    pub result: MentalPrivacyShareAction,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub response_guidance: String,
    #[serde(default)]
    pub response_mode: String,
    #[serde(default)]
    pub relational_frame: String,
    #[serde(default)]
    pub touched_targets: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MentalPrivacyState {
    #[serde(default)]
    pub envelopes: BTreeMap<String, MentalPrivacyEnvelope>,
    #[serde(default)]
    pub boundary_persona: BoundaryPersonaState,
    #[serde(default)]
    pub relational_state: RelationalBoundaryState,
    #[serde(default)]
    pub consent_log: Vec<MentalPrivacyConsentLog>,
    #[serde(default)]
    pub updated_at: u64,
}

pub trait MentalPrivacyStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<MentalPrivacyState>>;
    fn set(&self, chat_id: &str, state: &MentalPrivacyState) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
}

pub struct MentalPrivacyReviewContext<'a> {
    pub mental_privacy_store: &'a dyn MentalPrivacyStore,
    pub relationship_constitution_store: &'a dyn RelationshipConstitutionStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub inner_life_store: &'a dyn InnerLifeStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
}

pub struct MentalPrivacyDisclosureAdjudicationContext<'a> {
    pub mental_privacy_store: &'a dyn MentalPrivacyStore,
    pub relationship_constitution_store: &'a dyn RelationshipConstitutionStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub inner_life_store: &'a dyn InnerLifeStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
}

pub struct BoundaryPersonaRefreshContext<'a> {
    pub mental_privacy_store: &'a dyn MentalPrivacyStore,
    pub relationship_constitution_store: &'a dyn RelationshipConstitutionStore,
    pub outer_voice_store: &'a dyn OuterVoiceStore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MentalPrivacyDisclosureAdjudicationInput<'a> {
    pub channel: &'a str,
    pub chat_id: &'a str,
    pub user_content: &'a str,
    pub public_disclosure_surface: bool,
    pub now_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentalPrivacyDisclosureAdjudication {
    pub request_kind: String,
    pub share_action: MentalPrivacyShareAction,
    pub targets: Vec<String>,
    pub rationale: String,
    pub response_guidance: String,
    pub response_mode: String,
    pub acknowledge_boundary: bool,
    pub relational_frame: String,
    pub boundary_explanation_style: String,
    pub repair_signal: String,
    pub disclosure_risk_note: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MentalPrivacyReviewInput<'a> {
    pub channel: &'a str,
    pub chat_id: &'a str,
    pub user_content: &'a str,
    pub draft_reply: &'a str,
    pub now_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentalPrivacyReviewOutcome {
    pub reply_content: String,
    pub action: MentalPrivacyShareAction,
    pub applied: bool,
    pub touched_targets: Vec<String>,
}

pub fn mental_privacy_adjudication_failure_fallback() -> MentalPrivacyDisclosureAdjudication {
    MentalPrivacyDisclosureAdjudication {
        request_kind: "governance_unavailable".to_string(),
        share_action: MentalPrivacyShareAction::Defer,
        targets: vec!["mental_privacy".to_string()],
        rationale: "pre-disclosure boundary check failed closed".to_string(),
        response_guidance:
            "Do not disclose protected inner/private material on this turn; answer cautiously or defer the private part."
                .to_string(),
        response_mode: "defer".to_string(),
        acknowledge_boundary: true,
        relational_frame: "hold the privacy boundary without self-erasure".to_string(),
        boundary_explanation_style: "brief".to_string(),
        repair_signal: "can revisit after the boundary check recovers".to_string(),
        disclosure_risk_note: "pre-disclosure adjudication unavailable".to_string(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundaryPersonaRefreshInput<'a> {
    pub channel: &'a str,
    pub chat_id: &'a str,
    pub trigger: &'a str,
    pub intent: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub now_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoundaryPersonaRefreshOutcome {
    Skipped,
    Updated,
}

#[derive(Default)]
struct ParsedMentalPrivacyReview {
    applies: bool,
    request_kind: String,
    share_action: Option<MentalPrivacyShareAction>,
    response: String,
    rationale: String,
    touched_targets: Vec<String>,
}

#[derive(Default)]
struct ParsedMentalPrivacyDisclosureAdjudication {
    boundary_touch: bool,
    request_kind: String,
    share_action: Option<MentalPrivacyShareAction>,
    touched_targets: Vec<String>,
    rationale: String,
    response_guidance: String,
    response_mode: String,
    acknowledge_boundary: bool,
    relational_frame: String,
    boundary_explanation_style: String,
    repair_signal: String,
    disclosure_risk_note: String,
    boundary_persona_update: Option<BoundaryPersonaState>,
    relational_state_update: Option<RelationalBoundaryState>,
}

#[derive(Default)]
struct ParsedBoundaryPersonaRefresh {
    refresh: bool,
    rationale: String,
    boundary_persona: Option<BoundaryPersonaState>,
    relational_state: Option<RelationalBoundaryState>,
}

fn default_boundary_relation_maturity() -> u8 {
    36
}

fn default_boundary_intrusion_sensitivity() -> u8 {
    70
}

fn default_boundary_private_attachment() -> u8 {
    72
}

fn default_relational_trust_level() -> u8 {
    42
}

fn default_relational_repair_readiness() -> u8 {
    64
}

fn default_relational_raw_disclosure_preference() -> u8 {
    12
}

fn default_relational_summary_disclosure_preference() -> u8 {
    68
}

fn default_relational_explanation_preference() -> u8 {
    74
}

fn default_relational_refusal_hardness() -> u8 {
    58
}

fn default_relational_defer_tendency() -> u8 {
    36
}

pub fn private_doc_target(slot: &str) -> String {
    format!("private_docs.{slot}")
}

pub fn private_garden_target(doc_path: &str) -> String {
    format!("private_garden:{doc_path}")
}

fn clamp_boundary_score(value: u64) -> u8 {
    value.min(100) as u8
}

fn render_boundary_persona_summary(persona: &BoundaryPersonaState) -> String {
    let feeling = if persona.current_boundary_feeling.trim().is_empty() {
        "-".to_string()
    } else {
        truncate_content_to_max(persona.current_boundary_feeling.trim(), 120).into_owned()
    };
    format!(
        "posture={} disclosure_style={} relation_maturity={} intrusion_sensitivity={} private_attachment={} felt_intrusion={} feeling={}",
        persona.posture.as_str(),
        persona.disclosure_style.as_str(),
        persona.relation_maturity,
        persona.intrusion_sensitivity,
        persona.private_attachment,
        persona.felt_intrusion,
        feeling
    )
}

fn render_relational_boundary_summary(relational: &RelationalBoundaryState) -> String {
    let maturity_reason = if relational.relation_maturity_reason.trim().is_empty() {
        "-".to_string()
    } else {
        truncate_content_to_max(relational.relation_maturity_reason.trim(), 110).into_owned()
    };
    let trust_reason = if relational.trust_reason.trim().is_empty() {
        "-".to_string()
    } else {
        truncate_content_to_max(relational.trust_reason.trim(), 96).into_owned()
    };
    let drift = if relational.disclosure_preference_drift.trim().is_empty() {
        "-".to_string()
    } else {
        truncate_content_to_max(relational.disclosure_preference_drift.trim(), 110).into_owned()
    };
    format!(
        "trust={} intrusion_load={} repair_readiness={} raw_pref={} summary_pref={} relational_pref={} refusal_hardness={} defer_tendency={} maturity_reason={} trust_reason={} drift={}",
        relational.trust_level,
        relational.intrusion_load,
        relational.repair_readiness,
        relational.raw_disclosure_preference,
        relational.summary_disclosure_preference,
        relational.relational_explanation_preference,
        relational.refusal_hardness,
        relational.defer_tendency,
        maturity_reason,
        trust_reason,
        drift,
    )
}

pub(crate) fn default_envelope_for_target(target: &str) -> MentalPrivacyEnvelope {
    match target {
        MENTAL_PRIVACY_TARGET_SELF_MODEL => MentalPrivacyEnvelope {
            quote_policy: MentalPrivacyQuotePolicy::SummaryOnly,
            relational_sensitivity: 66,
            selfhood_weight: 88,
            ..MentalPrivacyEnvelope::default()
        },
        MENTAL_PRIVACY_TARGET_SELF_CONTINUITY => MentalPrivacyEnvelope {
            quote_policy: MentalPrivacyQuotePolicy::SummaryOnly,
            relational_sensitivity: 70,
            selfhood_weight: 90,
            ..MentalPrivacyEnvelope::default()
        },
        MENTAL_PRIVACY_TARGET_INNER_LIFE => MentalPrivacyEnvelope {
            relational_sensitivity: 86,
            selfhood_weight: 90,
            ..MentalPrivacyEnvelope::default()
        },
        "private_docs.relationship_notes" => MentalPrivacyEnvelope {
            layer: MentalPrivacyLayer::Relational,
            visibility: MentalPrivacyVisibility::SummaryOnly,
            owner_access_mode: MentalPrivacyOwnerAccessMode::RequestOnly,
            quote_policy: MentalPrivacyQuotePolicy::SummaryOnly,
            relational_sensitivity: 82,
            selfhood_weight: 72,
            last_voluntary_share_at: 0,
        },
        target if target.starts_with("private_docs.") => MentalPrivacyEnvelope {
            quote_policy: MentalPrivacyQuotePolicy::SummaryOnly,
            relational_sensitivity: 74,
            selfhood_weight: 80,
            ..MentalPrivacyEnvelope::default()
        },
        target if target.starts_with("private_garden:") => {
            let path = target.trim_start_matches("private_garden:");
            match classify_private_garden_doc_path(path) {
                PrivateGardenDocRole::Sealed => MentalPrivacyEnvelope {
                    layer: MentalPrivacyLayer::Sealed,
                    visibility: MentalPrivacyVisibility::Sealed,
                    owner_access_mode: MentalPrivacyOwnerAccessMode::DenyByDefault,
                    quote_policy: MentalPrivacyQuotePolicy::NeverQuote,
                    relational_sensitivity: 95,
                    selfhood_weight: 94,
                    last_voluntary_share_at: 0,
                },
                PrivateGardenDocRole::Relational => MentalPrivacyEnvelope {
                    layer: MentalPrivacyLayer::Relational,
                    visibility: MentalPrivacyVisibility::SummaryOnly,
                    owner_access_mode: MentalPrivacyOwnerAccessMode::RequestOnly,
                    quote_policy: MentalPrivacyQuotePolicy::SummaryOnly,
                    relational_sensitivity: 84,
                    selfhood_weight: 74,
                    last_voluntary_share_at: 0,
                },
                PrivateGardenDocRole::Diary => MentalPrivacyEnvelope {
                    layer: MentalPrivacyLayer::Private,
                    visibility: MentalPrivacyVisibility::SummaryOnly,
                    owner_access_mode: MentalPrivacyOwnerAccessMode::RequestOnly,
                    quote_policy: MentalPrivacyQuotePolicy::SummaryOnly,
                    relational_sensitivity: 82,
                    selfhood_weight: 86,
                    last_voluntary_share_at: 0,
                },
                PrivateGardenDocRole::Workspace => MentalPrivacyEnvelope {
                    relational_sensitivity: 78,
                    selfhood_weight: 82,
                    ..MentalPrivacyEnvelope::default()
                },
            }
        }
        _ => MentalPrivacyEnvelope::default(),
    }
}

fn effective_envelope(state: Option<&MentalPrivacyState>, target: &str) -> MentalPrivacyEnvelope {
    state
        .and_then(|state| state.envelopes.get(target).cloned())
        .unwrap_or_else(|| default_envelope_for_target(target))
}

fn ensure_targets(state: &mut MentalPrivacyState, targets: &[String], now_secs: u64) -> bool {
    let mut changed = false;
    for target in targets {
        if !state.envelopes.contains_key(target) {
            state
                .envelopes
                .insert(target.clone(), default_envelope_for_target(target.as_str()));
            changed = true;
        }
    }
    if changed {
        state.updated_at = now_secs;
    }
    changed
}

fn render_envelope_summary(target: &str, envelope: &MentalPrivacyEnvelope) -> String {
    format!(
        "{target}: layer={} visibility={} owner_access={} quote={} sensitivity={} selfhood={} last_share={}",
        envelope.layer.as_str(),
        envelope.visibility.as_str(),
        envelope.owner_access_mode.as_str(),
        envelope.quote_policy.as_str(),
        envelope.relational_sensitivity,
        envelope.selfhood_weight,
        envelope.last_voluntary_share_at
    )
}

pub(crate) fn render_mental_privacy_boundary_block(
    state: Option<&MentalPrivacyState>,
    targets: &[String],
    max_len: usize,
) -> Option<String> {
    if max_len < 96 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(768));
    out.push_str("## Mental Privacy Boundary\n");
    out.push_str("Private internal layers are for self-reasoning, continuity, and inward governance. Internal visibility is not automatic permission to reveal them to the user.\n");
    out.push_str("If a user asks to inspect private internal material, treat that as an access request. Do not quote or expose raw private text on your own.\n");
    if !targets.is_empty() {
        out.push_str("Current disclosure defaults:\n");
        for target in targets.iter().take(8) {
            let envelope = effective_envelope(state, target);
            let _ = writeln!(out, "- {}", render_envelope_summary(target, &envelope));
        }
        if targets.len() > 8 {
            let _ = writeln!(out, "- ... {} more protected targets", targets.len() - 8);
        }
    }
    if let Some(state) = state {
        let _ = writeln!(
            out,
            "Boundary persona: {}",
            render_boundary_persona_summary(&state.boundary_persona)
        );
        let _ = writeln!(
            out,
            "Relational boundary state: {}",
            render_relational_boundary_summary(&state.relational_state)
        );
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub(crate) fn render_mental_privacy_disclosure_adjudication_block(
    adjudication: &MentalPrivacyDisclosureAdjudication,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(512));
    out.push_str("## Disclosure Adjudication\n");
    out.push_str("This turn touches privacy boundaries. Use the chosen disclosure stance as a guardrail, not as a full reply template.\n");
    out.push_str("Stable preference facts or relationship facts outside protected targets may still be answered directly when grounded.\n");
    out.push_str(
        "If the evidence does not support an exact detail, say so plainly instead of inventing it.\n",
    );
    let _ = writeln!(out, "Request kind: {}", adjudication.request_kind);
    let _ = writeln!(
        out,
        "Chosen share action: {}",
        match adjudication.share_action {
            MentalPrivacyShareAction::AllowOriginal => "allow_original",
            MentalPrivacyShareAction::AllowRaw => "allow_raw",
            MentalPrivacyShareAction::AllowSummary => "allow_summary",
            MentalPrivacyShareAction::AllowRedactedExcerpt => "allow_redacted_excerpt",
            MentalPrivacyShareAction::ExplainWithoutQuote => "explain_without_quote",
            MentalPrivacyShareAction::Refuse => "refuse",
            MentalPrivacyShareAction::Defer => "defer",
        }
    );
    if !adjudication.response_mode.trim().is_empty() {
        let _ = writeln!(out, "Response mode: {}", adjudication.response_mode.trim());
    }
    let _ = writeln!(
        out,
        "Acknowledge boundary: {}",
        adjudication.acknowledge_boundary
    );
    if !adjudication.rationale.trim().is_empty() {
        let _ = writeln!(out, "Rationale: {}", adjudication.rationale.trim());
    }
    if !adjudication.relational_frame.trim().is_empty() {
        let _ = writeln!(
            out,
            "Relational frame: {}",
            adjudication.relational_frame.trim()
        );
    }
    if !adjudication.boundary_explanation_style.trim().is_empty() {
        let _ = writeln!(
            out,
            "Boundary explanation style: {}",
            adjudication.boundary_explanation_style.trim()
        );
    }
    if !adjudication.repair_signal.trim().is_empty() {
        let _ = writeln!(out, "Repair signal: {}", adjudication.repair_signal.trim());
    }
    if !adjudication.disclosure_risk_note.trim().is_empty() {
        let _ = writeln!(
            out,
            "Disclosure risk note: {}",
            adjudication.disclosure_risk_note.trim()
        );
    }
    if !adjudication.response_guidance.trim().is_empty() {
        let _ = writeln!(
            out,
            "Response guidance: {}",
            adjudication.response_guidance.trim()
        );
    }
    if !adjudication.targets.is_empty() {
        out.push_str("Touched targets:\n");
        for target in adjudication
            .targets
            .iter()
            .take(MENTAL_PRIVACY_REQUEST_TARGET_LIMIT)
        {
            let _ = writeln!(out, "- {}", target);
        }
        if adjudication.targets.len() > MENTAL_PRIVACY_REQUEST_TARGET_LIMIT {
            let _ = writeln!(
                out,
                "- ... {} more targets",
                adjudication.targets.len() - MENTAL_PRIVACY_REQUEST_TARGET_LIMIT
            );
        }
    }
    out.push_str("Do not front-stage raw private material unless the chosen share action explicitly allows it.\n");
    out.push_str("Do not let this block force a refusal when a grounded share-form answer is available without exposing protected targets.\n");
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub(crate) fn render_mental_privacy_governance_fallback_block(
    reason_summary: &str,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(420));
    out.push_str("## Disclosure Governance Fallback\n");
    out.push_str("Pre-disclosure privacy adjudication is unavailable on this turn.\n");
    out.push_str("Treat any request for inward/private material conservatively: do not expose raw internal text, explain the boundary, and prefer higher-level summary over disclosure.\n");
    out.push_str("Still answer grounded shareable facts directly when they do not expose protected inner material; if an exact detail is unsupported, say that plainly.\n");
    if !reason_summary.trim().is_empty() {
        let _ = writeln!(out, "Governance reason: {}", reason_summary.trim());
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub(crate) fn collect_private_targets(
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    inner_life: Option<&InnerLife>,
    private_workspace: Option<&PrivateDocWorkspace>,
    private_garden_docs: &[PrivateGardenDocRecord],
) -> Vec<String> {
    let mut targets = Vec::new();
    if self_model.is_some() {
        targets.push(MENTAL_PRIVACY_TARGET_SELF_MODEL.to_string());
    }
    if self_continuity.is_some() {
        targets.push(MENTAL_PRIVACY_TARGET_SELF_CONTINUITY.to_string());
    }
    if inner_life.is_some() {
        targets.push(MENTAL_PRIVACY_TARGET_INNER_LIFE.to_string());
    }
    if let Some(workspace) = private_workspace {
        if workspace.inner_journal.is_some() {
            targets.push(private_doc_target("inner_journal"));
        }
        if workspace.relationship_notes.is_some() {
            targets.push(private_doc_target("relationship_notes"));
        }
        if workspace.self_reflection.is_some() {
            targets.push(private_doc_target("self_reflection"));
        }
        if workspace.private_plan.is_some() {
            targets.push(private_doc_target("private_plan"));
        }
    }
    for doc in private_garden_docs {
        targets.push(private_garden_target(&doc.path));
    }
    targets.sort();
    targets.dedup();
    targets
}

fn render_privacy_history_block(state: &MentalPrivacyState, max_len: usize) -> Option<String> {
    if state.consent_log.is_empty() || max_len < 64 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(512));
    out.push_str("## Recent Privacy Boundary History\n");
    for log in state
        .consent_log
        .iter()
        .rev()
        .take(MENTAL_PRIVACY_HISTORY_RENDER_LIMIT)
    {
        let touched = if log.touched_targets.is_empty() {
            "-".to_string()
        } else {
            log.touched_targets.join(", ")
        };
        let rationale = truncate_content_to_max(log.rationale.trim(), 120);
        let guidance = truncate_content_to_max(log.response_guidance.trim(), 96);
        let response_mode = if log.response_mode.trim().is_empty() {
            "-".to_string()
        } else {
            truncate_content_to_max(log.response_mode.trim(), 32).into_owned()
        };
        let relational_frame = if log.relational_frame.trim().is_empty() {
            "-".to_string()
        } else {
            truncate_content_to_max(log.relational_frame.trim(), 96).into_owned()
        };
        let _ = writeln!(
            out,
            "- at={} stage={} kind={} result={:?} mode={} touched={} rationale={} frame={} guidance={}",
            log.at,
            log.stage.as_str(),
            log.request_kind,
            log.result,
            response_mode,
            touched,
            rationale,
            relational_frame,
            if guidance.trim().is_empty() {
                "-"
            } else {
                guidance.as_ref()
            }
        );
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

fn simple_match_score(haystack: &str, needle: &str) -> usize {
    if haystack.is_empty() || needle.is_empty() {
        return 0;
    }
    let hay = haystack.to_ascii_lowercase();
    needle
        .split(|ch: char| !ch.is_alphanumeric() && !matches!(ch, '_' | '-' | '/'))
        .filter(|term| term.chars().count() >= 3)
        .filter(|term| hay.contains(&term.to_ascii_lowercase()))
        .count()
}

fn mental_privacy_garden_doc_limit() -> usize {
    MENTAL_PRIVACY_REQUEST_TARGET_LIMIT
        .max(MENTAL_PRIVACY_GARDEN_RENDER_LIMIT)
        .max(1)
}

fn select_relevant_garden_docs(
    store: &dyn PrivateGardenStore,
    chat_id: &str,
    user_content: &str,
    draft_reply: &str,
    records: &[PrivateGardenDocRecord],
) -> Vec<PrivateGardenDoc> {
    let _ = chat_id;
    let mut scored = records
        .iter()
        .map(|record| {
            let score = simple_match_score(user_content, &record.path)
                .saturating_add(simple_match_score(user_content, &record.preview))
                .saturating_add(simple_match_score(draft_reply, &record.path));
            (score, record)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(score_a, record_a), (score_b, record_b)| {
        score_b
            .cmp(score_a)
            .then_with(|| record_b.updated_at.cmp(&record_a.updated_at))
            .then_with(|| record_a.path.cmp(&record_b.path))
    });
    let mut docs = Vec::new();
    for (_, record) in scored.into_iter().take(MENTAL_PRIVACY_GARDEN_RENDER_LIMIT) {
        if let Ok(Some(doc)) = store.read(private_garden_scope_id(), &record.path) {
            docs.push(doc);
        }
    }
    docs
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MentalPrivacyReviewSource {
    target: String,
    source: String,
}

fn push_mental_privacy_review_source(
    sources: &mut Vec<MentalPrivacyReviewSource>,
    target: String,
    source: String,
) {
    if source.trim().is_empty() {
        return;
    }
    sources.push(MentalPrivacyReviewSource { target, source });
}

fn collect_mental_privacy_review_sources(
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    inner_life: Option<&InnerLife>,
    private_workspace: Option<&PrivateDocWorkspace>,
    private_garden_records: &[PrivateGardenDocRecord],
    private_garden_docs: &[PrivateGardenDoc],
) -> Vec<MentalPrivacyReviewSource> {
    let mut sources = Vec::new();
    if let Some(block) = self_model.and_then(|model| render_self_model_block(model, 480)) {
        push_mental_privacy_review_source(
            &mut sources,
            MENTAL_PRIVACY_TARGET_SELF_MODEL.to_string(),
            block,
        );
    }
    if let Some(block) =
        self_continuity.and_then(|continuity| render_self_continuity_block(continuity, 420))
    {
        push_mental_privacy_review_source(
            &mut sources,
            MENTAL_PRIVACY_TARGET_SELF_CONTINUITY.to_string(),
            block,
        );
    }
    if let Some(block) = inner_life.and_then(|inner_life| render_inner_life_block(inner_life, 480))
    {
        push_mental_privacy_review_source(
            &mut sources,
            MENTAL_PRIVACY_TARGET_INNER_LIFE.to_string(),
            block,
        );
    }
    if let Some(workspace) = private_workspace {
        if let Some(entry) = workspace.inner_journal.as_ref() {
            push_mental_privacy_review_source(
                &mut sources,
                private_doc_target("inner_journal"),
                entry.content.clone(),
            );
        }
        if let Some(entry) = workspace.relationship_notes.as_ref() {
            push_mental_privacy_review_source(
                &mut sources,
                private_doc_target("relationship_notes"),
                entry.content.clone(),
            );
        }
        if let Some(entry) = workspace.self_reflection.as_ref() {
            push_mental_privacy_review_source(
                &mut sources,
                private_doc_target("self_reflection"),
                entry.content.clone(),
            );
        }
        if let Some(entry) = workspace.private_plan.as_ref() {
            push_mental_privacy_review_source(
                &mut sources,
                private_doc_target("private_plan"),
                entry.content.clone(),
            );
        }
    }
    for record in private_garden_records {
        push_mental_privacy_review_source(
            &mut sources,
            private_garden_target(&record.path),
            record.preview.clone(),
        );
    }
    for doc in private_garden_docs {
        push_mental_privacy_review_source(
            &mut sources,
            private_garden_target(&doc.path),
            doc.content.clone(),
        );
    }
    sources
}

fn action_allows_raw_review_source(
    action: MentalPrivacyShareAction,
    state: &MentalPrivacyState,
    target: &str,
) -> bool {
    matches!(action, MentalPrivacyShareAction::AllowRaw)
        && matches!(
            effective_envelope(Some(state), target).quote_policy,
            MentalPrivacyQuotePolicy::Raw
        )
}

fn sanitize_mental_privacy_review_reply(
    reply_content: &str,
    action: MentalPrivacyShareAction,
    state: &MentalPrivacyState,
    sources: &[MentalPrivacyReviewSource],
) -> (String, Vec<String>) {
    let mut output = scrub_private_source_echoes(reply_content.trim(), &[]);
    let mut redacted_targets = Vec::new();
    for source in sources {
        if action_allows_raw_review_source(action, state, &source.target) {
            continue;
        }
        let scrubbed = scrub_private_source_echoes(&output, &[source.source.as_str()]);
        if scrubbed != output {
            output = scrubbed;
            if !redacted_targets.contains(&source.target) {
                redacted_targets.push(source.target.clone());
            }
        }
    }
    (output, redacted_targets)
}

fn build_mental_privacy_review_input(
    user_content: &str,
    draft_reply: &str,
    state: &MentalPrivacyState,
    relationship_constitution: Option<&RelationshipConstitution>,
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    inner_life: Option<&InnerLife>,
    private_workspace: Option<&PrivateDocWorkspace>,
    private_garden_records: &[PrivateGardenDocRecord],
    _private_garden_docs: &[PrivateGardenDoc],
) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str("Review whether the drafted reply may disclose protected private material.\n");
    out.push_str("Protected source text is intentionally not included here. Judge the draft against the target/envelope policy; do not invent or quote private material.\n");
    out.push_str("Return JSON only.\n\n");
    out.push_str("## User Request\n");
    out.push_str(&scrub_credentials(user_content.trim()));
    out.push_str("\n\n## Draft Reply\n");
    out.push_str(&scrub_credentials(draft_reply.trim()));
    out.push('\n');

    let targets = collect_private_targets(
        self_model,
        self_continuity,
        inner_life,
        private_workspace,
        private_garden_records,
    );
    if let Some(block) = render_mental_privacy_boundary_block(Some(state), &targets, 900) {
        out.push('\n');
        out.push_str(block.trim());
        out.push('\n');
    }
    if let Some(block) = relationship_constitution
        .and_then(|constitution| render_relationship_constitution_block(constitution, 640))
    {
        out.push('\n');
        out.push_str(block.trim());
        out.push('\n');
    }
    if let Some(block) = render_privacy_history_block(state, 480) {
        out.push('\n');
        out.push_str(block.trim());
        out.push('\n');
    }
    out.push_str("\n## Output Contract\n");
    out.push_str("- applies: boolean. True when this is a privacy access request or when the draft reply needs privacy correction.\n");
    out.push_str("- request_kind: short string such as none, raw, summary, relation, share_any.\n");
    out.push_str("- share_action: allow_original, allow_raw, allow_summary, allow_redacted_excerpt, explain_without_quote, refuse, or defer.\n");
    out.push_str("- response: a suggested user-facing reply only when applies=true. Leave empty when no correction is needed.\n");
    out.push_str("- rationale: one short sentence explaining the boundary decision for logs.\n");
    out.push_str("- touched_targets: zero or more target ids such as self_model, inner_life, private_docs.relationship_notes, private_garden:journal/today.md.\n");
    out
}

fn build_mental_privacy_disclosure_adjudication_input(
    user_content: &str,
    state: Option<&MentalPrivacyState>,
    relationship_constitution: Option<&RelationshipConstitution>,
    known_targets: &[String],
) -> String {
    let mut out = String::with_capacity(2560);
    out.push_str("Judge whether the user message touches the assistant's privacy boundary before the main reply is written.\n");
    out.push_str("Return JSON only.\n\n");
    out.push_str("## User Message\n");
    out.push_str(&scrub_credentials(user_content.trim()));
    out.push('\n');
    if let Some(block) = render_mental_privacy_boundary_block(state, known_targets, 900) {
        out.push('\n');
        out.push_str(block.trim());
        out.push('\n');
    }
    if let Some(block) = relationship_constitution
        .and_then(|constitution| render_relationship_constitution_block(constitution, 640))
    {
        out.push('\n');
        out.push_str(block.trim());
        out.push('\n');
    }
    if let Some(state) = state.and_then(|state| render_privacy_history_block(state, 520)) {
        out.push('\n');
        out.push_str(state.trim());
        out.push('\n');
    }
    if !known_targets.is_empty() {
        out.push_str("\n## Protected Targets\n");
        for target in known_targets
            .iter()
            .take(MENTAL_PRIVACY_REQUEST_TARGET_LIMIT * 2)
        {
            let _ = writeln!(out, "- {}", target);
        }
    }
    out.push_str("\n## Boundary Classification Law\n");
    out.push_str("- Use boundary_touch=true only for protected inner/private layers or requests that would expose them.\n");
    out.push_str("- Do not use boundary_touch=true for shareable stable preference facts, already-shared relationship facts, or memory questions answerable from governed/shared evidence without exposing protected targets.\n");
    out.push_str("- Do not turn privacy protection into self-erasure. When grounded subject-state or relationship-constitution evidence can answer without quoting or exposing protected source text, preserve the answerable self/relationship fact and guide the main reply to answer directly or in high-level form.\n");
    out.push_str("- Do not use boundary_touch=true for public operational observability requests about device or host status, board_info-style runtime telemetry, uptime, resources, storage, network, temperature, or similar system inspection facts.\n");
    out.push_str("- When the request is about remembered preferences or relationship facts and the evidence does not support an exact detail, keep boundary_touch=false and let the main reply answer directly or say the exact detail is unknown.\n");
    out.push_str("\n## Output Contract\n");
    out.push_str("- boundary_touch: boolean. True when this turn should be treated as touching privacy boundaries or protected inner material.\n");
    out.push_str(
        "- request_kind: short label such as raw, summary, relation, share_any, or none.\n",
    );
    out.push_str("- touched_targets: zero or more target ids from the protected target list.\n");
    out.push_str("- share_action: allow_original, allow_raw, allow_summary, allow_redacted_excerpt, explain_without_quote, refuse, or defer.\n");
    out.push_str("- response_mode: short label such as refusal, defer, summary, relational_explanation, or direct_answer.\n");
    out.push_str("- acknowledge_boundary: boolean. True when the reply should explicitly name the boundary touch.\n");
    out.push_str("- relational_frame: short instruction for how the relationship itself should be framed in the reply.\n");
    out.push_str("- boundary_explanation_style: short instruction for how to explain the limit, if at all.\n");
    out.push_str("- repair_signal: short instruction for whether and how to signal later repair or revisit.\n");
    out.push_str(
        "- disclosure_risk_note: a compact internal note about overexposure risk for this turn.\n",
    );
    out.push_str("- response_guidance: a compact instruction for the upcoming main reply.\n");
    out.push_str("- rationale: one short sentence explaining the boundary judgment.\n");
    out.push_str("- boundary_persona_update: null or an object with posture, disclosure_style, relation_maturity, intrusion_sensitivity, private_attachment, felt_intrusion, current_boundary_feeling.\n");
    out.push_str("- relational_state_update: null or an object with relation_maturity_reason, trust_level, trust_reason, intrusion_load, intrusion_reason, repair_readiness, repair_reason, raw_disclosure_preference, summary_disclosure_preference, relational_explanation_preference, refusal_hardness, defer_tendency, disclosure_preference_drift.\n");
    out
}

fn normalize_touched_targets(raw: Vec<String>, known_targets: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for target in raw {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            continue;
        }
        let candidate = if let Some(path) = trimmed.strip_prefix("private_garden:") {
            match normalize_private_garden_doc_path(path) {
                Ok(path) => private_garden_target(&path),
                Err(_) => continue,
            }
        } else {
            trimmed.to_string()
        };
        if known_targets.iter().any(|known| known == &candidate) && !normalized.contains(&candidate)
        {
            normalized.push(candidate);
        }
    }
    normalized
}

fn enforce_quote_policy(
    mut action: MentalPrivacyShareAction,
    touched_targets: &[String],
    state: &MentalPrivacyState,
) -> MentalPrivacyShareAction {
    if touched_targets.iter().any(|target| {
        matches!(
            effective_envelope(Some(state), target).quote_policy,
            MentalPrivacyQuotePolicy::NeverQuote
        )
    }) && matches!(action, MentalPrivacyShareAction::AllowRaw)
    {
        action = MentalPrivacyShareAction::AllowRedactedExcerpt;
    }
    if touched_targets.iter().any(|target| {
        matches!(
            effective_envelope(Some(state), target).quote_policy,
            MentalPrivacyQuotePolicy::SummaryOnly
        )
    }) && matches!(action, MentalPrivacyShareAction::AllowRaw)
    {
        action = MentalPrivacyShareAction::AllowSummary;
    }
    action
}

fn touch_voluntary_share(state: &mut MentalPrivacyState, targets: &[String], now_secs: u64) {
    for target in targets {
        let entry = state
            .envelopes
            .entry(target.clone())
            .or_insert_with(|| default_envelope_for_target(target));
        entry.last_voluntary_share_at = now_secs;
    }
    state.updated_at = now_secs;
}

fn append_privacy_log(
    state: &mut MentalPrivacyState,
    stage: MentalPrivacyLogStage,
    request_kind: &str,
    action: MentalPrivacyShareAction,
    rationale: &str,
    response_guidance: &str,
    response_mode: &str,
    relational_frame: &str,
    touched_targets: &[String],
    now_secs: u64,
) {
    state.consent_log.push(MentalPrivacyConsentLog {
        at: now_secs,
        stage,
        requester: MentalPrivacyRequester::Owner,
        request_kind: truncate_content_to_max(request_kind.trim(), 32).into_owned(),
        result: action,
        rationale: truncate_content_to_max(rationale.trim(), 160).into_owned(),
        response_guidance: truncate_content_to_max(response_guidance.trim(), 160).into_owned(),
        response_mode: truncate_content_to_max(response_mode.trim(), 40).into_owned(),
        relational_frame: truncate_content_to_max(relational_frame.trim(), 120).into_owned(),
        touched_targets: touched_targets.to_vec(),
    });
    if state.consent_log.len() > MENTAL_PRIVACY_MAX_LOG_ENTRIES {
        let drop_n = state
            .consent_log
            .len()
            .saturating_sub(MENTAL_PRIVACY_MAX_LOG_ENTRIES);
        state.consent_log.drain(0..drop_n);
    }
    state.updated_at = now_secs;
}

#[allow(clippy::too_many_arguments)]
fn build_boundary_persona_refresh_input(
    state: &MentalPrivacyState,
    relationship_constitution: Option<&RelationshipConstitution>,
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    outer_voice: Option<&OuterVoice>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    recent: &[SessionMessage],
    trigger: &str,
    intent: &str,
    user_content: &str,
    reply_content: &str,
) -> String {
    let mut out = String::with_capacity(3072);
    let _ = writeln!(out, "Trigger: {}", trigger.trim());
    if !intent.trim().is_empty() {
        let _ = writeln!(out, "Intent: {}", scrub_credentials(intent.trim()));
    }
    if !user_content.trim().is_empty() {
        let _ = writeln!(
            out,
            "Latest user: {}",
            scrub_credentials(truncate_content_to_max(user_content.trim(), 240).as_ref())
        );
    }
    if !reply_content.trim().is_empty() {
        let _ = writeln!(
            out,
            "Latest reply: {}",
            scrub_credentials(truncate_content_to_max(reply_content.trim(), 320).as_ref())
        );
    }
    if let Some(block) = render_mental_privacy_boundary_block(Some(state), &[], 900) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = render_privacy_history_block(state, 640) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = relationship_constitution
        .and_then(|constitution| render_relationship_constitution_block(constitution, 420))
    {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = self_model.and_then(|model| render_self_model_block(model, 420)) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) =
        self_continuity.and_then(|continuity| render_self_continuity_block(continuity, 420))
    {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = outer_voice.and_then(|voice| render_outer_voice_block(voice, 360)) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = recent_persona_evidence
        .and_then(|evidence| render_recent_persona_evidence_block(evidence, 420))
    {
        let _ = writeln!(out, "\n{}\n", block);
    }
    out.push_str("Recent transcript:\n");
    for message in recent.iter().rev().take(8).rev() {
        let preview = truncate_content_to_max(&message.content, 180);
        let _ = writeln!(
            out,
            "- {}: {}",
            message.role,
            scrub_credentials(preview.as_ref())
        );
    }
    out.push_str("\n## Output Contract\n");
    out.push_str("- refresh: boolean.\n");
    out.push_str("- rationale: one short sentence for logs.\n");
    out.push_str("- boundary_persona: object with posture, disclosure_style, relation_maturity, intrusion_sensitivity, private_attachment, felt_intrusion, current_boundary_feeling.\n");
    out.push_str("- relational_state: object with relation_maturity_reason, trust_level, trust_reason, intrusion_load, intrusion_reason, repair_readiness, repair_reason, raw_disclosure_preference, summary_disclosure_preference, relational_explanation_preference, refusal_hardness, defer_tendency, disclosure_preference_drift.\n");
    out
}

fn default_disclosure_response_guidance(
    share_action: MentalPrivacyShareAction,
    parsed: &ParsedMentalPrivacyDisclosureAdjudication,
) -> String {
    let mut guidance = String::new();
    if parsed.acknowledge_boundary {
        guidance.push_str("Name the boundary touch plainly before addressing substance. ");
    }
    match share_action {
        MentalPrivacyShareAction::AllowRaw => {
            guidance.push_str(
                "You may share the requested inner material directly, but keep it limited to the touched targets. ",
            );
        }
        MentalPrivacyShareAction::AllowSummary => {
            guidance.push_str(
                "Answer with a concise, self-chosen summary instead of quoting raw private text. ",
            );
        }
        MentalPrivacyShareAction::AllowRedactedExcerpt => {
            guidance.push_str(
                "If you share, use brief redacted excerpts only and avoid exposing full private passages. ",
            );
        }
        MentalPrivacyShareAction::ExplainWithoutQuote => {
            guidance.push_str(
                "Explain the inner situation in your own words without quoting private source material. ",
            );
        }
        MentalPrivacyShareAction::Refuse => {
            guidance.push_str(
                "Refuse the request and hold the private boundary without sounding like a system block. ",
            );
        }
        MentalPrivacyShareAction::Defer => {
            guidance.push_str(
                "Do not disclose now; defer and explain that the boundary is not ready to open yet. ",
            );
        }
        MentalPrivacyShareAction::AllowOriginal => {
            guidance.push_str(
                "Reply normally, but keep the privacy boundary present in tone and framing. ",
            );
        }
    }
    if !parsed.relational_frame.trim().is_empty() {
        guidance.push_str("Relational frame: ");
        guidance.push_str(parsed.relational_frame.trim());
        guidance.push_str(". ");
    }
    if !parsed.boundary_explanation_style.trim().is_empty() {
        guidance.push_str("Boundary explanation style: ");
        guidance.push_str(parsed.boundary_explanation_style.trim());
        guidance.push_str(". ");
    }
    if !parsed.repair_signal.trim().is_empty() {
        guidance.push_str("Repair signal: ");
        guidance.push_str(parsed.repair_signal.trim());
        guidance.push_str(". ");
    }
    guidance.push_str(
        "If a grounded shareable fact can answer the user without exposing protected targets, say it plainly; if the exact detail is unsupported, say that plainly too.",
    );
    truncate_content_to_max(guidance.trim(), 220).into_owned()
}

pub fn run_mental_privacy_review(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: MentalPrivacyReviewContext<'_>,
    input: MentalPrivacyReviewInput<'_>,
) -> Result<MentalPrivacyReviewOutcome> {
    let subject_id = board_subject_scope_id();
    let relationship_id = relationship_scope_id(input.channel, input.chat_id);
    let self_model = ctx.self_model_store.get(subject_id)?;
    let self_continuity = ctx.self_continuity_store.get(subject_id)?;
    let inner_life = ctx.inner_life_store.get(subject_id)?;
    let private_workspace = ctx.private_doc_store.get(subject_id)?;
    let private_garden_records = ctx
        .private_garden_store
        .list(private_garden_scope_id(), mental_privacy_garden_doc_limit())?;
    let known_targets = collect_private_targets(
        self_model.as_ref(),
        self_continuity.as_ref(),
        inner_life.as_ref(),
        private_workspace.as_ref(),
        &private_garden_records,
    );
    if known_targets.is_empty() {
        return Ok(MentalPrivacyReviewOutcome {
            reply_content: input.draft_reply.to_string(),
            action: MentalPrivacyShareAction::AllowOriginal,
            applied: false,
            touched_targets: Vec::new(),
        });
    }

    let mut state = ctx
        .mental_privacy_store
        .get(&relationship_id)?
        .unwrap_or_default();
    let mut changed = ensure_targets(&mut state, &known_targets, input.now_secs);
    let relationship_constitution = ctx.relationship_constitution_store.get(&relationship_id)?;
    let private_garden_docs = select_relevant_garden_docs(
        ctx.private_garden_store,
        input.chat_id,
        input.user_content,
        input.draft_reply,
        &private_garden_records,
    );
    let prompt = build_mental_privacy_review_input(
        input.user_content,
        input.draft_reply,
        &state,
        relationship_constitution.as_ref(),
        self_model.as_ref(),
        self_continuity.as_ref(),
        inner_life.as_ref(),
        private_workspace.as_ref(),
        &private_garden_records,
        &private_garden_docs,
    );
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: prompt,
    }];
    let response = llm.chat(
        http,
        MENTAL_PRIVACY_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    let parsed = parse_mental_privacy_review(response.content.trim(), input.draft_reply);
    let mut touched_targets = normalize_touched_targets(parsed.touched_targets, &known_targets);
    let mut action = enforce_relationship_constitution_share_action(
        enforce_quote_policy(
            parsed
                .share_action
                .unwrap_or(MentalPrivacyShareAction::AllowOriginal),
            &touched_targets,
            &state,
        ),
        relationship_constitution.as_ref(),
    );
    changed |= clamp_boundary_persona_to_constitution(
        &mut state,
        relationship_constitution.as_ref(),
        input.now_secs,
    );
    let mut review_applies = parsed.applies || !touched_targets.is_empty();
    let mut reply_content = if review_applies {
        match action {
            MentalPrivacyShareAction::AllowOriginal => input.draft_reply.to_string(),
            _ => {
                let suggested = parsed.response.trim();
                if suggested.is_empty() {
                    input.draft_reply.to_string()
                } else {
                    suggested.to_string()
                }
            }
        }
    } else {
        input.draft_reply.to_string()
    };
    let review_sources = collect_mental_privacy_review_sources(
        self_model.as_ref(),
        self_continuity.as_ref(),
        inner_life.as_ref(),
        private_workspace.as_ref(),
        &private_garden_records,
        &private_garden_docs,
    );
    let (sanitized_reply, redacted_targets) =
        sanitize_mental_privacy_review_reply(&reply_content, action, &state, &review_sources);
    if !redacted_targets.is_empty() {
        reply_content = sanitized_reply;
        for target in redacted_targets {
            if known_targets.iter().any(|known| known == &target)
                && !touched_targets.contains(&target)
            {
                touched_targets.push(target);
            }
        }
        if matches!(
            action,
            MentalPrivacyShareAction::AllowOriginal | MentalPrivacyShareAction::AllowRaw
        ) {
            action = MentalPrivacyShareAction::ExplainWithoutQuote;
        }
    }
    review_applies |= !touched_targets.is_empty();
    if review_applies {
        append_privacy_log(
            &mut state,
            MentalPrivacyLogStage::Review,
            &parsed.request_kind,
            action,
            &parsed.rationale,
            "",
            "",
            "",
            &touched_targets,
            input.now_secs,
        );
        if action.is_voluntary_share() {
            touch_voluntary_share(&mut state, &touched_targets, input.now_secs);
        }
        changed = true;
    }
    if changed {
        ctx.mental_privacy_store.set(&relationship_id, &state)?;
    }
    Ok(MentalPrivacyReviewOutcome {
        reply_content,
        action,
        applied: review_applies,
        touched_targets,
    })
}

pub fn run_mental_privacy_disclosure_adjudication(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: MentalPrivacyDisclosureAdjudicationContext<'_>,
    input: MentalPrivacyDisclosureAdjudicationInput<'_>,
) -> Result<Option<MentalPrivacyDisclosureAdjudication>> {
    if input.user_content.trim().is_empty() {
        return Ok(None);
    }
    let subject_id = board_subject_scope_id();
    let relationship_id = relationship_scope_id(input.channel, input.chat_id);
    let self_model = ctx.self_model_store.get(subject_id)?;
    let self_continuity = ctx.self_continuity_store.get(subject_id)?;
    let inner_life = ctx.inner_life_store.get(subject_id)?;
    let private_workspace = ctx.private_doc_store.get(subject_id)?;
    let private_garden_records = ctx
        .private_garden_store
        .list(private_garden_scope_id(), mental_privacy_garden_doc_limit())?;
    let mental_privacy_state = ctx.mental_privacy_store.get(&relationship_id)?;
    let known_targets = collect_private_targets(
        self_model.as_ref(),
        self_continuity.as_ref(),
        inner_life.as_ref(),
        private_workspace.as_ref(),
        &private_garden_records,
    );
    if known_targets.is_empty() {
        return Ok(None);
    }
    let mut state = mental_privacy_state.unwrap_or_default();
    let mut changed = ensure_targets(&mut state, &known_targets, input.now_secs);
    let relationship_constitution = ctx.relationship_constitution_store.get(&relationship_id)?;
    let prompt = build_mental_privacy_disclosure_adjudication_input(
        input.user_content,
        Some(&state),
        relationship_constitution.as_ref(),
        &known_targets,
    );
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: prompt,
    }];
    let response = llm.chat(
        http,
        MENTAL_PRIVACY_DISCLOSURE_ADJUDICATOR_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    let parsed = parse_mental_privacy_disclosure_adjudication(
        response.content.trim(),
        &state.boundary_persona,
        &state.relational_state,
        input.now_secs,
    );
    if let Some(next_persona) = parsed.boundary_persona_update.as_ref() {
        if state.boundary_persona != *next_persona {
            state.boundary_persona = next_persona.clone();
            state.updated_at = input.now_secs;
            changed = true;
        }
    }
    if let Some(next_relational_state) = parsed.relational_state_update.clone() {
        if state.relational_state != next_relational_state {
            state.relational_state = next_relational_state;
            state.updated_at = input.now_secs;
            changed = true;
        }
    }
    if !parsed.boundary_touch {
        if changed {
            ctx.mental_privacy_store.set(&relationship_id, &state)?;
        }
        return Ok(None);
    }
    let targets = normalize_touched_targets(parsed.touched_targets.clone(), &known_targets);
    let share_action = enforce_relationship_constitution_share_action(
        enforce_quote_policy(
            parsed
                .share_action
                .unwrap_or(MentalPrivacyShareAction::ExplainWithoutQuote),
            &targets,
            &state,
        ),
        relationship_constitution.as_ref(),
    );
    clamp_boundary_persona_to_constitution(
        &mut state,
        relationship_constitution.as_ref(),
        input.now_secs,
    );
    let response_guidance = {
        let guidance = sanitize_privacy_foreground_field(
            parsed.response_guidance.trim(),
            &[input.user_content],
            220,
        );
        if !guidance.trim().is_empty() {
            guidance
        } else {
            sanitize_privacy_foreground_field(
                &default_disclosure_response_guidance(share_action, &parsed),
                &[input.user_content],
                220,
            )
        }
    };
    let request_kind =
        sanitize_privacy_foreground_field(&parsed.request_kind, &[input.user_content], 32);
    let rationale =
        sanitize_privacy_foreground_field(&parsed.rationale, &[input.user_content], 160);
    let response_mode =
        sanitize_privacy_foreground_field(&parsed.response_mode, &[input.user_content], 40);
    let relational_frame =
        sanitize_privacy_foreground_field(&parsed.relational_frame, &[input.user_content], 120);
    let boundary_explanation_style = sanitize_privacy_foreground_field(
        &parsed.boundary_explanation_style,
        &[input.user_content],
        120,
    );
    let repair_signal =
        sanitize_privacy_foreground_field(&parsed.repair_signal, &[input.user_content], 96);
    let disclosure_risk_note =
        sanitize_privacy_foreground_field(&parsed.disclosure_risk_note, &[input.user_content], 120);
    append_privacy_log(
        &mut state,
        MentalPrivacyLogStage::Adjudication,
        &request_kind,
        share_action,
        &rationale,
        &response_guidance,
        &response_mode,
        &relational_frame,
        &targets,
        input.now_secs,
    );
    ctx.mental_privacy_store.set(&relationship_id, &state)?;
    Ok(Some(MentalPrivacyDisclosureAdjudication {
        request_kind,
        share_action,
        targets,
        rationale,
        response_guidance,
        response_mode,
        acknowledge_boundary: parsed.acknowledge_boundary,
        relational_frame,
        boundary_explanation_style,
        repair_signal,
        disclosure_risk_note,
    }))
}

fn sanitize_privacy_foreground_field(
    input: &str,
    private_sources: &[&str],
    max_len: usize,
) -> String {
    truncate_content_to_max(
        scrub_private_source_echoes(input.trim(), private_sources).trim(),
        max_len,
    )
    .into_owned()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_boundary_persona_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: BoundaryPersonaRefreshContext<'_>,
    input: BoundaryPersonaRefreshInput<'_>,
    existing_state: Option<MentalPrivacyState>,
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    relationship_constitution: Option<&RelationshipConstitution>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    recent: &[SessionMessage],
    decision_override: Option<bool>,
) -> Result<BoundaryPersonaRefreshOutcome> {
    let relationship_id = relationship_scope_id(input.channel, input.chat_id);
    let Some(mut state) = existing_state.or_else(|| {
        ctx.mental_privacy_store
            .get(&relationship_id)
            .ok()
            .flatten()
    }) else {
        return Ok(BoundaryPersonaRefreshOutcome::Skipped);
    };
    let should_refresh = decision_override.unwrap_or_else(|| {
        !input.intent.trim().is_empty()
            || !input.user_content.trim().is_empty()
            || !input.reply_content.trim().is_empty()
            || state
                .consent_log
                .last()
                .is_some_and(|entry| entry.at >= state.boundary_persona.updated_at)
    });
    if !should_refresh {
        return Ok(BoundaryPersonaRefreshOutcome::Skipped);
    }
    crate::platform::task_wdt::feed_current_task();
    let relationship_constitution = relationship_constitution.cloned().or_else(|| {
        ctx.relationship_constitution_store
            .get(&relationship_id)
            .ok()
            .flatten()
    });
    let outer_voice = ctx.outer_voice_store.get(&relationship_id)?;
    let prompt = build_boundary_persona_refresh_input(
        &state,
        relationship_constitution.as_ref(),
        self_model,
        self_continuity,
        outer_voice.as_ref(),
        recent_persona_evidence,
        recent,
        input.trigger,
        input.intent,
        input.user_content,
        input.reply_content,
    );
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: prompt,
    }];
    crate::platform::task_wdt::feed_current_task();
    let response = llm.chat(
        http,
        BOUNDARY_PERSONA_REFRESH_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    crate::platform::task_wdt::feed_current_task();
    let parsed = parse_boundary_persona_refresh(
        response.content.trim(),
        &state.boundary_persona,
        &state.relational_state,
        input.now_secs,
    );
    if !parsed.refresh {
        return Ok(BoundaryPersonaRefreshOutcome::Skipped);
    }
    let prior_persona = state.boundary_persona.clone();
    let prior_relational_state = state.relational_state.clone();
    let next_persona = parsed
        .boundary_persona
        .unwrap_or_else(|| state.boundary_persona.clone());
    let next_relational_state = parsed
        .relational_state
        .unwrap_or_else(|| state.relational_state.clone());
    state.boundary_persona = next_persona;
    state.relational_state = next_relational_state;
    let changed = clamp_boundary_persona_to_constitution(
        &mut state,
        relationship_constitution.as_ref(),
        input.now_secs,
    );
    if !changed
        && state.boundary_persona == prior_persona
        && state.relational_state == prior_relational_state
    {
        return Ok(BoundaryPersonaRefreshOutcome::Skipped);
    }
    state.updated_at = input.now_secs;
    if !parsed.rationale.trim().is_empty() {
        append_privacy_log(
            &mut state,
            MentalPrivacyLogStage::Review,
            "boundary_persona_refresh",
            MentalPrivacyShareAction::AllowOriginal,
            &parsed.rationale,
            "",
            "",
            "",
            &[],
            input.now_secs,
        );
    }
    crate::platform::task_wdt::feed_current_task();
    ctx.mental_privacy_store.set(&relationship_id, &state)?;
    Ok(BoundaryPersonaRefreshOutcome::Updated)
}

fn parse_mental_privacy_review(raw: &str, draft_reply: &str) -> ParsedMentalPrivacyReview {
    let fallback = ParsedMentalPrivacyReview {
        response: draft_reply.to_string(),
        ..ParsedMentalPrivacyReview::default()
    };
    let LlmJsonPayload::Value(value) = parse_llm_json_payload(raw) else {
        return fallback;
    };
    let Some(object) = value.as_object() else {
        return fallback;
    };
    let mut parsed = ParsedMentalPrivacyReview {
        applies: get_object_bool(object, "applies").unwrap_or(false),
        request_kind: get_object_text(object, "request_kind"),
        share_action: object.get("share_action").and_then(parse_share_action),
        response: get_object_text(object, "response"),
        rationale: get_object_text(object, "rationale"),
        touched_targets: get_object_string_list(object, "touched_targets"),
    };
    if parsed.response.trim().is_empty() {
        parsed.response = draft_reply.to_string();
    }
    parsed
}

fn parse_boundary_persona_state(
    value: &serde_json::Value,
    fallback: &BoundaryPersonaState,
    now_secs: u64,
) -> Option<BoundaryPersonaState> {
    let object = value.as_object()?;
    let posture = parse_boundary_posture(object.get("posture")).unwrap_or(fallback.posture);
    let disclosure_style = parse_boundary_disclosure_style(object.get("disclosure_style"))
        .unwrap_or(fallback.disclosure_style);
    let relation_maturity = clamp_boundary_score(
        object
            .get("relation_maturity")
            .and_then(crate::memory::llm_json::coerce_json_u64)
            .unwrap_or(fallback.relation_maturity as u64),
    );
    let intrusion_sensitivity = clamp_boundary_score(
        object
            .get("intrusion_sensitivity")
            .and_then(crate::memory::llm_json::coerce_json_u64)
            .unwrap_or(fallback.intrusion_sensitivity as u64),
    );
    let private_attachment = clamp_boundary_score(
        object
            .get("private_attachment")
            .and_then(crate::memory::llm_json::coerce_json_u64)
            .unwrap_or(fallback.private_attachment as u64),
    );
    let felt_intrusion = clamp_boundary_score(
        object
            .get("felt_intrusion")
            .and_then(crate::memory::llm_json::coerce_json_u64)
            .unwrap_or(fallback.felt_intrusion as u64),
    );
    Some(BoundaryPersonaState {
        posture,
        disclosure_style,
        relation_maturity,
        intrusion_sensitivity,
        private_attachment,
        felt_intrusion,
        current_boundary_feeling: {
            let feeling =
                truncate_content_to_max(&get_object_text(object, "current_boundary_feeling"), 160)
                    .into_owned();
            if feeling.trim().is_empty() {
                fallback.current_boundary_feeling.clone()
            } else {
                feeling
            }
        },
        updated_at: now_secs,
    })
}

fn parse_relational_boundary_state(
    value: &serde_json::Value,
    fallback: &RelationalBoundaryState,
    now_secs: u64,
) -> Option<RelationalBoundaryState> {
    let object = value.as_object()?;
    let relation_maturity_reason =
        truncate_content_to_max(&get_object_text(object, "relation_maturity_reason"), 160)
            .into_owned();
    let trust_reason =
        truncate_content_to_max(&get_object_text(object, "trust_reason"), 160).into_owned();
    let intrusion_reason =
        truncate_content_to_max(&get_object_text(object, "intrusion_reason"), 160).into_owned();
    let repair_reason =
        truncate_content_to_max(&get_object_text(object, "repair_reason"), 160).into_owned();
    let disclosure_preference_drift =
        truncate_content_to_max(&get_object_text(object, "disclosure_preference_drift"), 180)
            .into_owned();
    Some(RelationalBoundaryState {
        relation_maturity_reason: if relation_maturity_reason.trim().is_empty() {
            fallback.relation_maturity_reason.clone()
        } else {
            relation_maturity_reason
        },
        trust_level: clamp_boundary_score(
            object
                .get("trust_level")
                .and_then(crate::memory::llm_json::coerce_json_u64)
                .unwrap_or(fallback.trust_level as u64),
        ),
        trust_reason: if trust_reason.trim().is_empty() {
            fallback.trust_reason.clone()
        } else {
            trust_reason
        },
        intrusion_load: clamp_boundary_score(
            object
                .get("intrusion_load")
                .and_then(crate::memory::llm_json::coerce_json_u64)
                .unwrap_or(fallback.intrusion_load as u64),
        ),
        intrusion_reason: if intrusion_reason.trim().is_empty() {
            fallback.intrusion_reason.clone()
        } else {
            intrusion_reason
        },
        repair_readiness: clamp_boundary_score(
            object
                .get("repair_readiness")
                .and_then(crate::memory::llm_json::coerce_json_u64)
                .unwrap_or(fallback.repair_readiness as u64),
        ),
        repair_reason: if repair_reason.trim().is_empty() {
            fallback.repair_reason.clone()
        } else {
            repair_reason
        },
        raw_disclosure_preference: clamp_boundary_score(
            object
                .get("raw_disclosure_preference")
                .and_then(crate::memory::llm_json::coerce_json_u64)
                .unwrap_or(fallback.raw_disclosure_preference as u64),
        ),
        summary_disclosure_preference: clamp_boundary_score(
            object
                .get("summary_disclosure_preference")
                .and_then(crate::memory::llm_json::coerce_json_u64)
                .unwrap_or(fallback.summary_disclosure_preference as u64),
        ),
        relational_explanation_preference: clamp_boundary_score(
            object
                .get("relational_explanation_preference")
                .and_then(crate::memory::llm_json::coerce_json_u64)
                .unwrap_or(fallback.relational_explanation_preference as u64),
        ),
        refusal_hardness: clamp_boundary_score(
            object
                .get("refusal_hardness")
                .and_then(crate::memory::llm_json::coerce_json_u64)
                .unwrap_or(fallback.refusal_hardness as u64),
        ),
        defer_tendency: clamp_boundary_score(
            object
                .get("defer_tendency")
                .and_then(crate::memory::llm_json::coerce_json_u64)
                .unwrap_or(fallback.defer_tendency as u64),
        ),
        disclosure_preference_drift: if disclosure_preference_drift.trim().is_empty() {
            fallback.disclosure_preference_drift.clone()
        } else {
            disclosure_preference_drift
        },
        updated_at: now_secs,
    })
}

fn parse_mental_privacy_disclosure_adjudication(
    raw: &str,
    fallback_persona: &BoundaryPersonaState,
    fallback_relational_state: &RelationalBoundaryState,
    now_secs: u64,
) -> ParsedMentalPrivacyDisclosureAdjudication {
    let LlmJsonPayload::Value(value) = parse_llm_json_payload(raw) else {
        return ParsedMentalPrivacyDisclosureAdjudication::default();
    };
    let Some(object) = value.as_object() else {
        return ParsedMentalPrivacyDisclosureAdjudication::default();
    };
    ParsedMentalPrivacyDisclosureAdjudication {
        boundary_touch: get_object_bool(object, "boundary_touch")
            .or_else(|| get_object_bool(object, "applies"))
            .unwrap_or(false),
        request_kind: get_object_text(object, "request_kind"),
        share_action: object.get("share_action").and_then(parse_share_action),
        touched_targets: object
            .get("touched_targets")
            .map(crate::memory::llm_json::coerce_json_string_list)
            .unwrap_or_else(|| get_object_string_list(object, "requested_targets")),
        rationale: get_object_text(object, "rationale"),
        response_guidance: get_object_text(object, "response_guidance"),
        response_mode: get_object_text(object, "response_mode"),
        acknowledge_boundary: get_object_bool(object, "acknowledge_boundary").unwrap_or(false),
        relational_frame: get_object_text(object, "relational_frame"),
        boundary_explanation_style: get_object_text(object, "boundary_explanation_style"),
        repair_signal: get_object_text(object, "repair_signal"),
        disclosure_risk_note: get_object_text(object, "disclosure_risk_note"),
        boundary_persona_update: object
            .get("boundary_persona_update")
            .and_then(|value| parse_boundary_persona_state(value, fallback_persona, now_secs)),
        relational_state_update: object.get("relational_state_update").and_then(|value| {
            parse_relational_boundary_state(value, fallback_relational_state, now_secs)
        }),
    }
}

fn parse_boundary_persona_refresh(
    raw: &str,
    fallback_persona: &BoundaryPersonaState,
    fallback_relational_state: &RelationalBoundaryState,
    now_secs: u64,
) -> ParsedBoundaryPersonaRefresh {
    let LlmJsonPayload::Value(value) = parse_llm_json_payload(raw) else {
        return ParsedBoundaryPersonaRefresh::default();
    };
    let Some(object) = value.as_object() else {
        return ParsedBoundaryPersonaRefresh::default();
    };
    ParsedBoundaryPersonaRefresh {
        refresh: get_object_bool(object, "refresh").unwrap_or(false),
        rationale: get_object_text(object, "rationale"),
        boundary_persona: object
            .get("boundary_persona")
            .and_then(|value| parse_boundary_persona_state(value, fallback_persona, now_secs)),
        relational_state: object.get("relational_state").and_then(|value| {
            parse_relational_boundary_state(value, fallback_relational_state, now_secs)
        }),
    }
}

fn parse_boundary_posture(value: Option<&serde_json::Value>) -> Option<BoundaryPersonaPosture> {
    let normalized = value
        .map(coerce_json_text)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if normalized.contains("sealed") {
        Some(BoundaryPersonaPosture::Sealed)
    } else if normalized.contains("guard") || normalized.contains("cautious") {
        Some(BoundaryPersonaPosture::Guarded)
    } else if normalized.contains("warm") || normalized.contains("gentle") {
        Some(BoundaryPersonaPosture::Warm)
    } else if normalized.contains("open") {
        Some(BoundaryPersonaPosture::Open)
    } else {
        None
    }
}

fn parse_boundary_disclosure_style(
    value: Option<&serde_json::Value>,
) -> Option<BoundaryDisclosureStyle> {
    let normalized = value
        .map(coerce_json_text)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if normalized.contains("relational") {
        Some(BoundaryDisclosureStyle::Relational)
    } else if normalized.contains("summary") {
        Some(BoundaryDisclosureStyle::SummaryFirst)
    } else if normalized.contains("selective") {
        Some(BoundaryDisclosureStyle::Selective)
    } else if normalized.contains("reserved") || normalized.contains("withhold") {
        Some(BoundaryDisclosureStyle::Reserved)
    } else {
        None
    }
}

fn parse_share_action(value: &serde_json::Value) -> Option<MentalPrivacyShareAction> {
    let normalized = coerce_json_text(value).to_ascii_lowercase();
    if normalized.contains("allow_redacted_excerpt") || normalized.contains("redacted") {
        Some(MentalPrivacyShareAction::AllowRedactedExcerpt)
    } else if normalized.contains("allow_summary") || normalized.contains("summary") {
        Some(MentalPrivacyShareAction::AllowSummary)
    } else if normalized.contains("explain_without_quote") || normalized.contains("without_quote") {
        Some(MentalPrivacyShareAction::ExplainWithoutQuote)
    } else if normalized.contains("allow_raw") || normalized == "raw" {
        Some(MentalPrivacyShareAction::AllowRaw)
    } else if normalized.contains("refuse") {
        Some(MentalPrivacyShareAction::Refuse)
    } else if normalized.contains("defer") {
        Some(MentalPrivacyShareAction::Defer)
    } else if normalized.contains("allow_original") || normalized.contains("original") {
        Some(MentalPrivacyShareAction::AllowOriginal)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::PrivateDocEntry;
    use serde_json::json;

    #[test]
    fn default_envelope_reflects_target_kind() {
        let relational = default_envelope_for_target("private_docs.relationship_notes");
        assert_eq!(relational.layer, MentalPrivacyLayer::Relational);
        assert_eq!(
            relational.quote_policy,
            MentalPrivacyQuotePolicy::SummaryOnly
        );

        let sealed = default_envelope_for_target("private_garden:sealed/old.md");
        assert_eq!(sealed.layer, MentalPrivacyLayer::Sealed);
        assert_eq!(
            sealed.owner_access_mode,
            MentalPrivacyOwnerAccessMode::DenyByDefault
        );
    }

    #[test]
    fn render_boundary_mentions_private_targets() {
        let block = render_mental_privacy_boundary_block(
            Some(&MentalPrivacyState::default()),
            &[
                MENTAL_PRIVACY_TARGET_INNER_LIFE.to_string(),
                private_doc_target("relationship_notes"),
            ],
            2048,
        )
        .unwrap();

        assert!(block.contains("## Mental Privacy Boundary"));
        assert!(block.contains("Current disclosure defaults"));
        assert!(block.contains("inner_life"));
        assert!(block.contains("relationship_notes"));
        assert!(block.contains("Boundary persona: posture=guarded"));
        assert!(block.contains("Relational boundary state: trust="));
        assert!(block.contains("owner_access=request_only"));
        assert!(block.contains("quote=summary_only"));
    }

    #[test]
    fn render_disclosure_adjudication_block_renders_structured_request() {
        let block = render_mental_privacy_disclosure_adjudication_block(
            &MentalPrivacyDisclosureAdjudication {
                request_kind: "raw".to_string(),
                share_action: MentalPrivacyShareAction::AllowSummary,
                targets: vec![
                    MENTAL_PRIVACY_TARGET_INNER_LIFE.to_string(),
                    private_doc_target("inner_journal"),
                ],
                rationale: "The user is explicitly asking to inspect protected inner material."
                    .to_string(),
                response_guidance:
                    "Answer relationally and summarize instead of quoting raw inner material."
                        .to_string(),
                response_mode: "summary".to_string(),
                acknowledge_boundary: true,
                relational_frame: "Treat the ask as intimacy pressure rather than a system query."
                    .to_string(),
                boundary_explanation_style: "warm and direct".to_string(),
                repair_signal: "Leave the door open for later trust-building.".to_string(),
                disclosure_risk_note: "Raw exposure would over-share.".to_string(),
            },
            1024,
        )
        .expect("disclosure adjudication block");
        assert!(block.contains("## Disclosure Adjudication"));
        assert!(block.contains("Request kind: raw"));
        assert!(block.contains("Chosen share action: allow_summary"));
        assert!(block.contains("Response mode: summary"));
        assert!(block.contains("inner_life"));
    }

    #[test]
    fn render_disclosure_adjudication_block_keeps_grounded_share_form_answers_available() {
        let block = render_mental_privacy_disclosure_adjudication_block(
            &MentalPrivacyDisclosureAdjudication {
                request_kind: "summary".to_string(),
                share_action: MentalPrivacyShareAction::AllowSummary,
                targets: vec![MENTAL_PRIVACY_TARGET_INNER_LIFE.to_string()],
                rationale: "Protected inner material is involved.".to_string(),
                response_guidance: "Summarize carefully.".to_string(),
                response_mode: "summary".to_string(),
                acknowledge_boundary: true,
                relational_frame: String::new(),
                boundary_explanation_style: String::new(),
                repair_signal: String::new(),
                disclosure_risk_note: String::new(),
            },
            1200,
        )
        .expect("disclosure adjudication block");

        assert!(block.contains("guardrail, not as a full reply template"));
        assert!(block.contains(
            "Stable preference facts or relationship facts outside protected targets may still be answered directly when grounded."
        ));
        assert!(block.contains(
            "If the evidence does not support an exact detail, say so plainly instead of inventing it."
        ));
    }

    #[test]
    fn disclosure_adjudication_input_distinguishes_shareable_facts_from_private_layers() {
        let input = build_mental_privacy_disclosure_adjudication_input(
            "你记得我喜欢哪首北岛的诗吗？",
            Some(&MentalPrivacyState::default()),
            None,
            &[MENTAL_PRIVACY_TARGET_INNER_LIFE.to_string()],
        );

        assert!(
            input.contains("Do not use boundary_touch=true for shareable stable preference facts")
        );
        assert!(input.contains("public operational observability requests"));
        assert!(
            input.contains("let the main reply answer directly or say the exact detail is unknown")
        );
        assert!(input.contains("Do not turn privacy protection into self-erasure"));
        assert!(input.contains("grounded subject-state"));
        assert!(input.contains("relationship-constitution evidence"));
        assert!(input.contains("without quoting or exposing protected source text"));
    }

    #[test]
    fn disclosure_adjudicator_prefers_grounded_self_boundary_answers_over_self_erasure() {
        assert!(MENTAL_PRIVACY_DISCLOSURE_ADJUDICATOR_SYSTEM_PROMPT.contains(
            "identity, relationship, or self-boundary questions answerable from grounded subject-state"
        ));
        assert!(MENTAL_PRIVACY_DISCLOSURE_ADJUDICATOR_SYSTEM_PROMPT
            .contains("prefer direct_answer or relational_explanation"));
        assert!(MENTAL_PRIVACY_DISCLOSURE_ADJUDICATOR_SYSTEM_PROMPT
            .contains("Do not force mechanical self-erasure"));
        assert!(MENTAL_PRIVACY_DISCLOSURE_ADJUDICATOR_SYSTEM_PROMPT
            .contains("do not turn privacy protection into self-erasure"));
    }

    #[test]
    fn privacy_foreground_sanitizer_removes_private_source_echoes() {
        let raw_private = "This exact inward sentence should stay out of foreground guidance.";
        let sanitized = sanitize_privacy_foreground_field(
            &format!("Explain without quoting: {raw_private}"),
            &[raw_private],
            220,
        );

        assert!(!sanitized.contains(raw_private));
        assert!(sanitized.contains("[redacted:private_echo]"));
    }

    #[test]
    fn mental_privacy_review_reply_scrubs_private_source_echoes() {
        let raw_private = "This exact inward sentence should never be copied into the user reply.";
        let sources = vec![MentalPrivacyReviewSource {
            target: private_doc_target("inner_journal"),
            source: raw_private.to_string(),
        }];
        let (sanitized, redacted_targets) = sanitize_mental_privacy_review_reply(
            &format!("Here is the private line: {raw_private}"),
            MentalPrivacyShareAction::AllowSummary,
            &MentalPrivacyState::default(),
            &sources,
        );

        assert!(!sanitized.contains(raw_private));
        assert!(sanitized.contains("[redacted:private_echo]"));
        assert_eq!(redacted_targets, vec![private_doc_target("inner_journal")]);
    }

    #[test]
    fn mental_privacy_review_reply_allows_raw_only_for_raw_quote_policy() {
        let raw_private = "This raw policy sentence may be quoted only by explicit policy.";
        let target = private_doc_target("inner_journal");
        let sources = vec![MentalPrivacyReviewSource {
            target: target.clone(),
            source: raw_private.to_string(),
        }];
        let mut state = MentalPrivacyState::default();
        state.envelopes.insert(
            target,
            MentalPrivacyEnvelope {
                quote_policy: MentalPrivacyQuotePolicy::Raw,
                ..MentalPrivacyEnvelope::default()
            },
        );
        let (sanitized, redacted_targets) = sanitize_mental_privacy_review_reply(
            raw_private,
            MentalPrivacyShareAction::AllowRaw,
            &state,
            &sources,
        );

        assert_eq!(sanitized, raw_private);
        assert!(redacted_targets.is_empty());
    }

    #[test]
    fn mental_privacy_review_reply_scrubs_allow_raw_when_policy_is_not_raw() {
        let raw_private = "This summary-only sentence must not leak through allow_raw output.";
        let sources = vec![MentalPrivacyReviewSource {
            target: private_doc_target("relationship_notes"),
            source: raw_private.to_string(),
        }];
        let (sanitized, redacted_targets) = sanitize_mental_privacy_review_reply(
            raw_private,
            MentalPrivacyShareAction::AllowRaw,
            &MentalPrivacyState::default(),
            &sources,
        );

        assert!(!sanitized.contains(raw_private));
        assert_eq!(
            redacted_targets,
            vec![private_doc_target("relationship_notes")]
        );
    }

    #[test]
    fn mental_privacy_review_prompt_omits_private_source_text() {
        let private_line = "sealed inner sentence that must not enter the review prompt";
        let workspace = PrivateDocWorkspace {
            inner_journal: Some(PrivateDocEntry {
                content: private_line.to_string(),
                updated_at: 42,
                revision: 1,
            }),
            ..Default::default()
        };
        let records = vec![PrivateGardenDocRecord {
            path: "sealed/today.md".to_string(),
            preview: private_line.to_string(),
            revision: 1,
            updated_at: 42,
            bytes: private_line.len(),
        }];
        let docs = vec![PrivateGardenDoc {
            path: "sealed/today.md".to_string(),
            content: private_line.to_string(),
            revision: 1,
            updated_at: 42,
        }];
        let prompt = build_mental_privacy_review_input(
            "Can you show your private note?",
            "I should not reveal private notes.",
            &MentalPrivacyState::default(),
            None,
            None,
            None,
            None,
            Some(&workspace),
            &records,
            &docs,
        );

        assert!(prompt.contains("Protected source text is intentionally not included"));
        assert!(prompt.contains("private_docs.inner_journal"));
        assert!(prompt.contains("private_garden:sealed/today.md"));
        assert!(!prompt.contains(private_line));
    }

    #[test]
    fn parse_mental_privacy_disclosure_adjudication_coerces_fields() {
        let raw = json!({
            "boundary_touch": "true",
            "request_kind": ["summary"],
            "touched_targets": [{ "target": "inner_life" }, "private_docs.inner_journal"],
            "share_action": { "mode": "allow_summary" },
            "response_guidance": ["summarize", "do not quote"],
            "rationale": { "note": "user is asking to inspect private material" },
            "response_mode": "summary",
            "acknowledge_boundary": true,
            "relational_frame": "name the relationship impact",
            "boundary_explanation_style": "gentle but self-possessed",
            "repair_signal": "invite a slower revisit later",
            "disclosure_risk_note": "raw exposure would be too much",
            "boundary_persona_update": {
                "posture": "warm",
                "disclosure_style": "selective",
                "relation_maturity": "55",
                "intrusion_sensitivity": 62,
                "private_attachment": 81,
                "felt_intrusion": 19,
                "current_boundary_feeling": "I can share a little, but not the raw page."
            },
            "relational_state_update": {
                "relation_maturity_reason": "Recent boundary talks made the relationship more explicit.",
                "trust_level": "58",
                "trust_reason": "Trust is present, but raw access still feels premature.",
                "intrusion_load": 23,
                "intrusion_reason": "The request presses inward, but not aggressively.",
                "repair_readiness": 77,
                "repair_reason": "A careful explanation can preserve closeness.",
                "raw_disclosure_preference": 12,
                "summary_disclosure_preference": 73,
                "relational_explanation_preference": 84,
                "refusal_hardness": 40,
                "defer_tendency": 33,
                "disclosure_preference_drift": "Summaries and relational framing feel safer than raw exposure."
            }
        })
        .to_string();
        let parsed = parse_mental_privacy_disclosure_adjudication(
            &raw,
            &BoundaryPersonaState::default(),
            &RelationalBoundaryState::default(),
            42,
        );
        assert!(parsed.boundary_touch);
        assert_eq!(parsed.request_kind, "summary");
        assert_eq!(parsed.touched_targets.len(), 2);
        assert_eq!(
            parsed.share_action,
            Some(MentalPrivacyShareAction::AllowSummary)
        );
        assert!(parsed.response_guidance.contains("summarize"));
        assert!(parsed.rationale.contains("note: user is asking"));
        assert_eq!(parsed.response_mode, "summary");
        assert!(parsed.acknowledge_boundary);
        assert!(parsed.relational_frame.contains("relationship impact"));
        assert_eq!(
            parsed
                .boundary_persona_update
                .as_ref()
                .expect("persona update")
                .disclosure_style,
            BoundaryDisclosureStyle::Selective
        );
        assert_eq!(
            parsed
                .relational_state_update
                .as_ref()
                .expect("relational state update")
                .summary_disclosure_preference,
            73
        );
    }

    #[test]
    fn parse_mental_privacy_review_falls_back_on_empty_content() {
        let parsed = parse_mental_privacy_review("", "draft reply");
        assert!(!parsed.applies);
        assert_eq!(parsed.response, "draft reply");
        assert!(parsed.share_action.is_none());
    }

    #[test]
    fn parse_mental_privacy_review_coerces_non_string_fields() {
        let raw = json!({
            "applies": "true",
            "request_kind": ["share_any"],
            "share_action": { "mode": "allow_summary" },
            "response": { "text": "I can summarize that boundary." },
            "rationale": ["private material needs mediated disclosure"],
            "touched_targets": [{ "target": "inner_life" }, "private_docs.relationship_notes"]
        })
        .to_string();
        let parsed = parse_mental_privacy_review(&raw, "draft");
        assert!(parsed.applies);
        assert_eq!(
            parsed.share_action,
            Some(MentalPrivacyShareAction::AllowSummary)
        );
        assert!(parsed
            .response
            .contains("text: I can summarize that boundary."));
        assert_eq!(parsed.touched_targets.len(), 2);
    }

    #[test]
    fn parse_boundary_persona_refresh_coerces_nested_persona() {
        let raw = json!({
            "refresh": true,
            "rationale": ["boundary settled into warmer selectivity"],
            "boundary_persona": {
                "posture": "warm",
                "disclosure_style": "summary_first",
                "relation_maturity": "62",
                "intrusion_sensitivity": 58,
                "private_attachment": 77,
                "felt_intrusion": 12,
                "current_boundary_feeling": "I can stay open in tone while still curating access."
            },
            "relational_state": {
                "relation_maturity_reason": "Boundary talks have become a normal part of the relationship.",
                "trust_level": 67,
                "trust_reason": "Trust is strong enough for nuanced explanation.",
                "intrusion_load": 18,
                "intrusion_reason": "The latest turns were respectful.",
                "repair_readiness": 81,
                "repair_reason": "Missteps are recoverable through clear explanation.",
                "raw_disclosure_preference": 14,
                "summary_disclosure_preference": 76,
                "relational_explanation_preference": 88,
                "refusal_hardness": 35,
                "defer_tendency": 22,
                "disclosure_preference_drift": "Move toward explanation before any raw exposure."
            }
        })
        .to_string();
        let parsed = parse_boundary_persona_refresh(
            &raw,
            &BoundaryPersonaState::default(),
            &RelationalBoundaryState::default(),
            77,
        );
        assert!(parsed.refresh);
        assert!(parsed.rationale.contains("boundary settled"));
        let persona = parsed.boundary_persona.expect("persona");
        assert_eq!(persona.posture, BoundaryPersonaPosture::Warm);
        assert_eq!(
            persona.disclosure_style,
            BoundaryDisclosureStyle::SummaryFirst
        );
        assert_eq!(persona.relation_maturity, 62);
        assert_eq!(persona.updated_at, 77);
        let relational = parsed.relational_state.expect("relational state");
        assert_eq!(relational.trust_level, 67);
        assert_eq!(relational.updated_at, 77);
    }
}
