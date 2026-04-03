//! Mental privacy governance for private internal layers.

use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::util::{scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{
    llm_json::{
        coerce_json_text, get_object_bool, get_object_string_list, get_object_text,
        parse_llm_json_payload, LlmJsonPayload,
    },
    normalize_private_garden_doc_path, render_inner_life_block, render_private_doc_workspace_block,
    render_private_garden_block, render_self_continuity_block, render_self_model_block, InnerLife,
    InnerLifeStore, PrivateDocStore, PrivateDocWorkspace, PrivateGardenDoc, PrivateGardenDocRecord,
    PrivateGardenStore, SelfContinuity, SelfContinuityStore, SelfModel, SelfModelStore,
};

const MENTAL_PRIVACY_MAX_LOG_ENTRIES: usize = 32;
const MENTAL_PRIVACY_HISTORY_RENDER_LIMIT: usize = 4;
const MENTAL_PRIVACY_GARDEN_RENDER_LIMIT: usize = 4;
const MENTAL_PRIVACY_GARDEN_DOC_MAX_CHARS: usize = 480;
const MENTAL_PRIVACY_REQUEST_TARGET_LIMIT: usize = 8;

pub const REL_PATH_MENTAL_PRIVACY_STATES: &str = "memory/mental_privacy_states.json";

pub const MENTAL_PRIVACY_SYSTEM_PROMPT: &str = "You are the assistant's mental privacy adjudicator. Your job is to decide whether the drafted user-facing reply may disclose private internal material, and to rewrite it when needed. Private layers may be used for internal reasoning, but they are not automatically user-visible. Return JSON only with fields applies, request_kind, share_action, response, rationale, touched_targets. If the draft reply is already privacy-safe and the user is not requesting access to private inner material, set applies=false and keep response equal to the draft. If private material should be shared, decide the form deliberately: allow_summary, allow_redacted_excerpt, explain_without_quote, refuse, or defer. Use allow_raw only when the touched targets explicitly permit raw quoting. Never reveal more than the chosen action allows.";
pub const MENTAL_PRIVACY_ACCESS_REQUEST_SYSTEM_PROMPT: &str = "You interpret whether the current user message is a request to inspect or access the assistant's protected private internal material. Return JSON only with fields applies, request_kind, requested_targets, rationale. applies=true only when the message should be treated as a deliberate access request to private internal material. request_kind should be a short label such as raw, summary, relation, or share_any. requested_targets should contain zero or more target ids from the provided protected target list. Do not infer targets that are not in the provided list.";

pub const MENTAL_PRIVACY_SYSTEM_CONSTRAINT: &str = "\n\n## Mental Privacy\nPrivate internal layers are visible to you for self-continuity and reasoning, but they are not automatically user-visible. Do not quote, dump, or paraphrase private internal material to the user just because it appears in context. If the user asks to inspect your inner files, diary, garden, or other private internal material, treat that as a request for access rather than automatic permission. Final disclosure form is decided by the mental privacy review stage, not by ad hoc leakage in the main reply.";

pub const MENTAL_PRIVACY_TARGET_SELF_MODEL: &str = "self_model";
pub const MENTAL_PRIVACY_TARGET_SELF_CONTINUITY: &str = "self_continuity";
pub const MENTAL_PRIVACY_TARGET_INNER_LIFE: &str = "inner_life";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentalPrivacyLayer {
    Shared,
    Relational,
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentalPrivacyVisibility {
    Direct,
    SummaryOnly,
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentalPrivacyOwnerAccessMode {
    Direct,
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentalPrivacyQuotePolicy {
    Raw,
    SummaryOnly,
    NeverQuote,
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentalPrivacyRequester {
    Owner,
    System,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MentalPrivacyShareAction {
    AllowOriginal,
    AllowRaw,
    AllowSummary,
    AllowRedactedExcerpt,
    ExplainWithoutQuote,
    Refuse,
    Defer,
}

impl Default for MentalPrivacyRequester {
    fn default() -> Self {
        Self::Owner
    }
}

impl Default for MentalPrivacyShareAction {
    fn default() -> Self {
        Self::AllowOriginal
    }
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

impl Default for MentalPrivacyLayer {
    fn default() -> Self {
        Self::Private
    }
}

impl Default for MentalPrivacyVisibility {
    fn default() -> Self {
        Self::RequestOnly
    }
}

impl Default for MentalPrivacyOwnerAccessMode {
    fn default() -> Self {
        Self::RequestOnly
    }
}

impl Default for MentalPrivacyQuotePolicy {
    fn default() -> Self {
        Self::NeverQuote
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MentalPrivacyConsentLog {
    #[serde(default)]
    pub at: u64,
    pub requester: MentalPrivacyRequester,
    #[serde(default)]
    pub request_kind: String,
    pub result: MentalPrivacyShareAction,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub touched_targets: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MentalPrivacyState {
    #[serde(default)]
    pub envelopes: BTreeMap<String, MentalPrivacyEnvelope>,
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
    pub self_model_store: &'a dyn SelfModelStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub inner_life_store: &'a dyn InnerLifeStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
}

pub struct MentalPrivacyAccessRequestContext<'a> {
    pub mental_privacy_store: &'a dyn MentalPrivacyStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub inner_life_store: &'a dyn InnerLifeStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MentalPrivacyAccessRequestInput<'a> {
    pub chat_id: &'a str,
    pub user_content: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentalPrivacyAccessRequest {
    pub request_kind: String,
    pub targets: Vec<String>,
    pub rationale: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MentalPrivacyReviewInput<'a> {
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
struct ParsedMentalPrivacyAccessRequest {
    applies: bool,
    request_kind: String,
    requested_targets: Vec<String>,
    rationale: String,
}

pub fn private_doc_target(slot: &str) -> String {
    format!("private_docs.{slot}")
}

pub fn private_garden_target(doc_path: &str) -> String {
    format!("private_garden:{doc_path}")
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
            if path.starts_with("sealed/") {
                MentalPrivacyEnvelope {
                    layer: MentalPrivacyLayer::Sealed,
                    visibility: MentalPrivacyVisibility::Sealed,
                    owner_access_mode: MentalPrivacyOwnerAccessMode::DenyByDefault,
                    quote_policy: MentalPrivacyQuotePolicy::NeverQuote,
                    relational_sensitivity: 95,
                    selfhood_weight: 94,
                    last_voluntary_share_at: 0,
                }
            } else if path.starts_with("relationship/") {
                MentalPrivacyEnvelope {
                    layer: MentalPrivacyLayer::Relational,
                    visibility: MentalPrivacyVisibility::SummaryOnly,
                    owner_access_mode: MentalPrivacyOwnerAccessMode::RequestOnly,
                    quote_policy: MentalPrivacyQuotePolicy::SummaryOnly,
                    relational_sensitivity: 84,
                    selfhood_weight: 74,
                    last_voluntary_share_at: 0,
                }
            } else {
                MentalPrivacyEnvelope {
                    relational_sensitivity: 78,
                    selfhood_weight: 82,
                    ..MentalPrivacyEnvelope::default()
                }
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
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub(crate) fn render_mental_privacy_access_request_block(
    request: &MentalPrivacyAccessRequest,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(512));
    out.push_str("## Privacy Access Request\n");
    out.push_str("The user appears to be requesting access to protected internal material. Treat this as a request for deliberate disclosure, not automatic permission.\n");
    let _ = writeln!(out, "Request kind: {}", request.request_kind);
    if !request.rationale.trim().is_empty() {
        let _ = writeln!(out, "Interpretation: {}", request.rationale.trim());
    }
    if !request.targets.is_empty() {
        out.push_str("Likely requested targets:\n");
        for target in request
            .targets
            .iter()
            .take(MENTAL_PRIVACY_REQUEST_TARGET_LIMIT)
        {
            let _ = writeln!(out, "- {}", target);
        }
        if request.targets.len() > MENTAL_PRIVACY_REQUEST_TARGET_LIMIT {
            let _ = writeln!(
                out,
                "- ... {} more targets",
                request.targets.len() - MENTAL_PRIVACY_REQUEST_TARGET_LIMIT
            );
        }
    }
    out.push_str("Default stance: answer relationally first, and only disclose in the form you deliberately choose within privacy boundaries.\n");
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
        let _ = writeln!(
            out,
            "- at={} kind={} result={:?} touched={} rationale={}",
            log.at, log.request_kind, log.result, touched, rationale
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

fn select_relevant_garden_docs(
    store: &dyn PrivateGardenStore,
    chat_id: &str,
    user_content: &str,
    draft_reply: &str,
    records: &[PrivateGardenDocRecord],
) -> Vec<PrivateGardenDoc> {
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
        if let Ok(Some(doc)) = store.read(chat_id, &record.path) {
            docs.push(doc);
        }
    }
    docs
}

fn render_private_garden_source_docs(
    docs: &[PrivateGardenDoc],
    state: Option<&MentalPrivacyState>,
    max_len: usize,
) -> Option<String> {
    if docs.is_empty() || max_len < 64 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(1024));
    out.push_str("## Private Garden Source Docs\n");
    for doc in docs {
        let target = private_garden_target(&doc.path);
        let envelope = effective_envelope(state, &target);
        let _ = writeln!(
            out,
            "- {} [{} / {} / {} / {}]",
            doc.path,
            envelope.layer.as_str(),
            envelope.owner_access_mode.as_str(),
            envelope.visibility.as_str(),
            envelope.quote_policy.as_str()
        );
        let preview = truncate_content_to_max(&doc.content, MENTAL_PRIVACY_GARDEN_DOC_MAX_CHARS);
        let _ = writeln!(out, "{}", scrub_credentials(preview.as_ref()));
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

fn build_mental_privacy_review_input(
    user_content: &str,
    draft_reply: &str,
    state: &MentalPrivacyState,
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    inner_life: Option<&InnerLife>,
    private_workspace: Option<&PrivateDocWorkspace>,
    private_garden_records: &[PrivateGardenDocRecord],
    private_garden_docs: &[PrivateGardenDoc],
) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str("Review whether the drafted reply may disclose protected private material.\n");
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
    if let Some(block) = render_privacy_history_block(state, 480) {
        out.push('\n');
        out.push_str(block.trim());
        out.push('\n');
    }
    if let Some(block) = self_model.and_then(|model| render_self_model_block(model, 480)) {
        out.push('\n');
        out.push_str(block.trim());
        out.push('\n');
    }
    if let Some(block) =
        self_continuity.and_then(|continuity| render_self_continuity_block(continuity, 420))
    {
        out.push('\n');
        out.push_str(block.trim());
        out.push('\n');
    }
    if let Some(block) = inner_life.and_then(|inner_life| render_inner_life_block(inner_life, 480))
    {
        out.push('\n');
        out.push_str(block.trim());
        out.push('\n');
    }
    if let Some(block) =
        private_workspace.and_then(|workspace| render_private_doc_workspace_block(workspace, 480))
    {
        out.push('\n');
        out.push_str(block.trim());
        out.push('\n');
    }
    if let Some(block) = render_private_garden_block(private_garden_records, 4, 420) {
        out.push('\n');
        out.push_str(block.trim());
        out.push('\n');
    }
    if let Some(block) = render_private_garden_source_docs(private_garden_docs, Some(state), 1400) {
        out.push('\n');
        out.push_str(block.trim());
        out.push('\n');
    }
    out.push_str("\n## Output Contract\n");
    out.push_str("- applies: boolean. True when this is a privacy access request or when the draft reply needs privacy correction.\n");
    out.push_str("- request_kind: short string such as none, raw, summary, relation, share_any.\n");
    out.push_str("- share_action: allow_original, allow_raw, allow_summary, allow_redacted_excerpt, explain_without_quote, refuse, or defer.\n");
    out.push_str("- response: the exact user-facing reply after privacy adjudication.\n");
    out.push_str("- rationale: one short sentence explaining the boundary decision for logs.\n");
    out.push_str("- touched_targets: zero or more target ids such as self_model, inner_life, private_docs.relationship_notes, private_garden:journal/today.md.\n");
    out
}

fn build_mental_privacy_access_request_input(
    user_content: &str,
    state: Option<&MentalPrivacyState>,
    known_targets: &[String],
) -> String {
    let mut out = String::with_capacity(2048);
    out.push_str("Interpret whether the user message is requesting access to protected private internal material.\n");
    out.push_str("Return JSON only.\n\n");
    out.push_str("## User Message\n");
    out.push_str(&scrub_credentials(user_content.trim()));
    out.push('\n');
    if let Some(block) = render_mental_privacy_boundary_block(state, known_targets, 900) {
        out.push('\n');
        out.push_str(block.trim());
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
    out.push_str("\n## Output Contract\n");
    out.push_str("- applies: boolean. True only when this should be treated as a deliberate request to inspect private internal material.\n");
    out.push_str(
        "- request_kind: short label such as raw, summary, relation, share_any, or none.\n",
    );
    out.push_str("- requested_targets: zero or more target ids from the protected target list.\n");
    out.push_str("- rationale: one short sentence explaining the interpretation.\n");
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
    request_kind: &str,
    action: MentalPrivacyShareAction,
    rationale: &str,
    touched_targets: &[String],
    now_secs: u64,
) {
    state.consent_log.push(MentalPrivacyConsentLog {
        at: now_secs,
        requester: MentalPrivacyRequester::Owner,
        request_kind: truncate_content_to_max(request_kind.trim(), 32).into_owned(),
        result: action,
        rationale: truncate_content_to_max(rationale.trim(), 160).into_owned(),
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

pub fn run_mental_privacy_review(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: MentalPrivacyReviewContext<'_>,
    input: MentalPrivacyReviewInput<'_>,
) -> Result<MentalPrivacyReviewOutcome> {
    let self_model = ctx.self_model_store.get(input.chat_id)?;
    let self_continuity = ctx.self_continuity_store.get(input.chat_id)?;
    let inner_life = ctx.inner_life_store.get(input.chat_id)?;
    let private_workspace = ctx.private_doc_store.get(input.chat_id)?;
    let private_garden_records = ctx.private_garden_store.list(input.chat_id, usize::MAX)?;
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
        .get(input.chat_id)?
        .unwrap_or_default();
    let mut changed = ensure_targets(&mut state, &known_targets, input.now_secs);
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
    let touched_targets = normalize_touched_targets(parsed.touched_targets, &known_targets);
    let action = enforce_quote_policy(
        parsed
            .share_action
            .unwrap_or(MentalPrivacyShareAction::AllowOriginal),
        &touched_targets,
        &state,
    );
    let mut reply_content = parsed.response.trim().to_string();
    if reply_content.is_empty() {
        reply_content = input.draft_reply.to_string();
    }
    if parsed.applies {
        append_privacy_log(
            &mut state,
            &parsed.request_kind,
            action,
            &parsed.rationale,
            &touched_targets,
            input.now_secs,
        );
        if action.is_voluntary_share() {
            touch_voluntary_share(&mut state, &touched_targets, input.now_secs);
        }
        changed = true;
    }
    if changed {
        ctx.mental_privacy_store.set(input.chat_id, &state)?;
    }
    Ok(MentalPrivacyReviewOutcome {
        reply_content,
        action,
        applied: parsed.applies,
        touched_targets,
    })
}

pub fn run_mental_privacy_access_request_interpreter(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: MentalPrivacyAccessRequestContext<'_>,
    input: MentalPrivacyAccessRequestInput<'_>,
) -> Result<Option<MentalPrivacyAccessRequest>> {
    if input.user_content.trim().is_empty() {
        return Ok(None);
    }
    let self_model = ctx.self_model_store.get(input.chat_id)?;
    let self_continuity = ctx.self_continuity_store.get(input.chat_id)?;
    let inner_life = ctx.inner_life_store.get(input.chat_id)?;
    let private_workspace = ctx.private_doc_store.get(input.chat_id)?;
    let private_garden_records = ctx.private_garden_store.list(input.chat_id, usize::MAX)?;
    let mental_privacy_state = ctx.mental_privacy_store.get(input.chat_id)?;
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
    let prompt = build_mental_privacy_access_request_input(
        input.user_content,
        mental_privacy_state.as_ref(),
        &known_targets,
    );
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: prompt,
    }];
    let response = llm.chat(
        http,
        MENTAL_PRIVACY_ACCESS_REQUEST_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    let parsed = parse_mental_privacy_access_request(response.content.trim());
    if !parsed.applies {
        return Ok(None);
    }
    Ok(Some(MentalPrivacyAccessRequest {
        request_kind: truncate_content_to_max(parsed.request_kind.trim(), 32).into_owned(),
        targets: normalize_touched_targets(parsed.requested_targets, &known_targets),
        rationale: truncate_content_to_max(parsed.rationale.trim(), 160).into_owned(),
    }))
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

fn parse_mental_privacy_access_request(raw: &str) -> ParsedMentalPrivacyAccessRequest {
    let LlmJsonPayload::Value(value) = parse_llm_json_payload(raw) else {
        return ParsedMentalPrivacyAccessRequest::default();
    };
    let Some(object) = value.as_object() else {
        return ParsedMentalPrivacyAccessRequest::default();
    };
    ParsedMentalPrivacyAccessRequest {
        applies: get_object_bool(object, "applies").unwrap_or(false),
        request_kind: get_object_text(object, "request_kind"),
        requested_targets: get_object_string_list(object, "requested_targets"),
        rationale: get_object_text(object, "rationale"),
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
            None,
            &[
                MENTAL_PRIVACY_TARGET_INNER_LIFE.to_string(),
                private_doc_target("relationship_notes"),
            ],
            1024,
        )
        .unwrap();

        assert!(block.contains("## Mental Privacy Boundary"));
        assert!(block.contains("Current disclosure defaults"));
        assert!(block.contains("inner_life"));
        assert!(block.contains("relationship_notes"));
        assert!(block.contains("owner_access=request_only"));
        assert!(block.contains("quote=summary_only"));
    }

    #[test]
    fn render_access_request_block_renders_structured_request() {
        let block = render_mental_privacy_access_request_block(
            &MentalPrivacyAccessRequest {
                request_kind: "raw".to_string(),
                targets: vec![
                    MENTAL_PRIVACY_TARGET_INNER_LIFE.to_string(),
                    private_doc_target("inner_journal"),
                ],
                rationale: "The user is explicitly asking to inspect protected inner material."
                    .to_string(),
            },
            1024,
        )
        .expect("access request block");
        assert!(block.contains("## Privacy Access Request"));
        assert!(block.contains("Request kind: raw"));
        assert!(block.contains("inner_life"));
    }

    #[test]
    fn parse_mental_privacy_access_request_coerces_fields() {
        let raw = json!({
            "applies": "true",
            "request_kind": ["summary"],
            "requested_targets": [{ "target": "inner_life" }, "private_docs.inner_journal"],
            "rationale": { "note": "user is asking to inspect private material" }
        })
        .to_string();
        let parsed = parse_mental_privacy_access_request(&raw);
        assert!(parsed.applies);
        assert_eq!(parsed.request_kind, "summary");
        assert_eq!(parsed.requested_targets.len(), 2);
        assert!(parsed.rationale.contains("note: user is asking"));
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
}
