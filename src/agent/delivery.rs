use super::{TaskTerminalVisibilityStatus, TurnVisibilityFact};
use crate::bus::{
    CanonicalMessageBody, IngressKind, MessageTransport, OutboundKind, OutboundTx, PcMsg,
    PlatformNativeBody,
};
use crate::channel_capability::ChannelDeliveryOrderingModel;
use crate::error::Result;
use crate::i18n::Locale as UiLocale;
use crate::memory::MemorySystemKind;
use crate::metrics;
use crate::tools::{ToolOutboundDeliveryKind, ToolOutboundIntent, ToolOutboundTarget};
use crate::util::truncate_content_to_max;
use std::sync::{Arc, Mutex};

const EDIT_THROTTLE_MS: u64 = 500;
const MAX_EDIT_FAILURES: u8 = 3;
const MIN_PARTIAL_VISIBLE_CHARS: usize = 8;
const MAX_QUEUED_PROGRESS_CHARS: usize = 120;
const MAX_QUEUED_PARTIAL_CHARS: usize = 240;
const MAX_APPEND_ONLY_TOOL_NAME_CHARS: usize = 32;
const TELEGRAM_REACTION_ACCEPTED: &str = "👀";
const TELEGRAM_REACTION_WORKING: &str = "⏳";
const TELEGRAM_REACTION_SUCCEEDED: &str = "✅";
const TELEGRAM_REACTION_FAILED: &str = "⚠️";
#[cfg(test)]
const APPEND_ONLY_PRIVATE_ACK_DELAY_MS: u64 = 5;
#[cfg(not(test))]
const APPEND_ONLY_PRIVATE_ACK_DELAY_MS: u64 = 1500;
#[cfg(test)]
const APPEND_ONLY_GROUP_HEARTBEAT_DELAY_MS: u64 = 10;
#[cfg(not(test))]
const APPEND_ONLY_GROUP_HEARTBEAT_DELAY_MS: u64 = 8000;
const RELIABLE_OUTBOUND_ENQUEUE_RETRY_DELAY_MS: u64 = 50;
const RELIABLE_OUTBOUND_ENQUEUE_LOG_EVERY: u32 = 20;

/// 流式编辑器：LLM 流式输出期间，发送占位消息并逐步编辑内容。
/// 实现方内部自行创建/管理 HTTP 连接，不占用 agent 的 LLM HTTP 连接。
pub trait StreamEditor {
    /// 发送初始占位消息，返回 message_id（用于后续编辑）。
    fn send_initial(&self, chat_id: &str, content: &str) -> Result<Option<String>>;
    /// 编辑已发送的消息。
    fn edit(&self, chat_id: &str, message_id: &str, content: &str) -> Result<()>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DeliveryReport {
    pub edit_phase_header_updates_sent: u8,
    pub edit_planner_header_updates_sent: u8,
    pub edit_tool_header_updates_sent: u8,
    pub edit_action_header_updates_sent: u8,
    pub edit_terminal_header_updates_sent: u8,
    pub append_only_ack_sent: u8,
    pub append_only_heartbeat_sent: u8,
    pub append_only_first_tool_milestone_sent: u8,
    pub partial_updates_sent: u8,
    pub tool_outbound_intents_seen: u8,
    pub tool_visible_updates_sent: u8,
    pub explicit_outbound_sent: u8,
    pub tool_outbound_suppressed: u8,
    pub current_primary_delivered: bool,
    pub finalize_streamed: bool,
    pub visible_text_updates_sent: u8,
}

pub(crate) struct DeliverySession<'a> {
    mode: DeliveryMode<'a>,
    outbound_tx: &'a OutboundTx,
    req_id: &'a str,
    policy: DeliveryPolicy,
    visible_update_contract: VisibleUpdateContract,
    fact_state: DeliveryFactState,
}

enum DeliveryMode<'a> {
    Silent,
    Edit(EditDelivery<'a>),
    Queued(QueuedDelivery),
}

struct EditDelivery<'a> {
    chat_id: &'a str,
    editor: &'a (dyn StreamEditor + Send + Sync),
    message_id: Option<String>,
    last_edit_at: std::time::Instant,
    edit_disabled: bool,
    edit_failures: u8,
    lifecycle: DeliveryLifecycle,
    status_header: Option<EditStatusHeader>,
    last_body_text: String,
    last_sent_composed: String,
    pending_compose: String,
    report: DeliveryReport,
}

struct QueuedDelivery {
    lifecycle: DeliveryLifecycle,
    report: DeliveryReport,
    append_only_visibility: Option<Arc<AppendOnlyVisibilityShared>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DeliveryFactState {
    task_started_visible: bool,
    task_terminal_visible: bool,
}

#[derive(Clone)]
struct AppendOnlyVisibilityShared {
    delivery: AppendOnlyVisibilityDelivery,
    mode: AppendOnlyVisibilityMode,
    state: Arc<Mutex<AppendOnlyVisibilityState>>,
}

#[derive(Clone)]
struct AppendOnlyVisibilityDelivery {
    channel: Arc<str>,
    chat_id: Arc<str>,
    req_id: String,
    is_group: bool,
    source_transport: MessageTransport,
    platform_thread_id: String,
    platform_message_id: String,
    platform_event_id: String,
    inbound_dedup_key: String,
    outbound_tx: OutboundTx,
    contract: AppendOnlyVisibilityContract,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppendOnlyVisibilityMode {
    PrivateAck,
    GroupHeartbeat,
    AnchoredReaction,
}

#[derive(Clone, Copy, Debug)]
struct AppendOnlyVisibilityContract {
    loc: UiLocale,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AppendOnlyVisibilityState {
    finalized: bool,
    supplemental_emitted: u8,
    ack_sent: bool,
    heartbeat_sent: bool,
    first_tool_milestone_sent: bool,
    reaction_accepted_sent: bool,
    reaction_working_sent: bool,
    first_tool_snapshot: Option<FirstToolSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FirstToolSnapshot {
    tool_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AppendOnlyVisibilityProjection {
    text: String,
    marks_ack_sent: bool,
    marks_heartbeat_sent: bool,
    marks_first_tool_milestone_sent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeliveryLifecycle {
    Open,
    Finalized,
}

impl DeliveryLifecycle {
    fn is_closed(self) -> bool {
        !matches!(self, Self::Open)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolIntentDelivery {
    Suppressed,
    VisibleUpdate,
}

#[derive(Clone, Copy, Debug)]
struct VisibleUpdateContract {
    loc: UiLocale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VisibleUpdateKind {
    Acknowledged,
    Reasoning,
    PlannerProgress,
    ToolProgress,
    ActionProgress,
    TerminalProgress,
    Finalizing,
    PartialDraft,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EditStatusHeader {
    text: String,
    kind: EditStatusHeaderKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditStatusHeaderKind {
    Placeholder,
    StickyTerminal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EditFactProjection {
    header: EditStatusHeader,
    report_kind: VisibleUpdateKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DeliveryPolicy {
    supports_current_primary: bool,
    supports_current_supplemental: bool,
}

impl<'a> DeliverySession<'a> {
    pub(crate) fn new(
        msg: &'a PcMsg,
        req_id: &'a str,
        outbound_tx: &'a OutboundTx,
        editor: Option<&'a (dyn StreamEditor + Send + Sync)>,
        channel_capability: Option<crate::ChannelCapabilityEntry>,
        _memory_system_kind: MemorySystemKind,
        loc: UiLocale,
    ) -> Self {
        let visible_update_contract = VisibleUpdateContract::new(loc);
        let policy = channel_capability
            .filter(|entry| entry.enabled && msg.ingress == IngressKind::User)
            .map(|entry| DeliveryPolicy {
                supports_current_primary: entry.contract.supports_primary_reply,
                supports_current_supplemental: entry.contract.supports_supplemental_reply,
            })
            .unwrap_or_default();
        let append_only_visibility = channel_capability.and_then(|entry| {
            AppendOnlyVisibilityShared::new_if_enabled(msg, req_id, outbound_tx, entry, loc, policy)
        });
        let mode = if !policy.supports_current_primary && !policy.supports_current_supplemental {
            DeliveryMode::Silent
        } else if let Some(editor) = editor.filter(|_| {
            channel_capability
                .map(|entry| entry.enabled && entry.contract.supports_stream_edit)
                .unwrap_or(false)
        }) {
            DeliveryMode::Edit(EditDelivery {
                chat_id: msg.chat_id.as_ref(),
                editor,
                message_id: None,
                last_edit_at: std::time::Instant::now(),
                edit_disabled: false,
                edit_failures: 0,
                lifecycle: DeliveryLifecycle::Open,
                status_header: None,
                last_body_text: String::new(),
                last_sent_composed: String::new(),
                pending_compose: String::new(),
                report: DeliveryReport::default(),
            })
        } else {
            DeliveryMode::Queued(QueuedDelivery {
                lifecycle: DeliveryLifecycle::Open,
                report: DeliveryReport::default(),
                append_only_visibility,
            })
        };
        Self {
            mode,
            outbound_tx,
            req_id,
            policy,
            visible_update_contract,
            fact_state: DeliveryFactState::default(),
        }
    }

    pub(crate) fn report(&self) -> DeliveryReport {
        match self.mode {
            DeliveryMode::Silent => DeliveryReport::default(),
            DeliveryMode::Edit(ref delivery) => delivery.report,
            DeliveryMode::Queued(ref delivery) => delivery.report(),
        }
    }

    #[cfg(test)]
    pub(crate) fn on_stream_delta(&mut self, accumulated: &str) {
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => delivery.on_stream_delta(accumulated),
            DeliveryMode::Silent | DeliveryMode::Queued(_) => {}
        }
    }

    pub(crate) fn emit_fact(&mut self, fact: TurnVisibilityFact<'_>) {
        if !self.policy.supports_current_supplemental {
            return;
        }
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => {
                if delivery.emit_fact(self.visible_update_contract, fact) {
                    self.fact_state.record_visible_fact(fact);
                }
            }
            DeliveryMode::Queued(ref delivery) => delivery.observe_fact(fact),
            DeliveryMode::Silent => {}
        }
    }

    pub(crate) fn emit_partial(&mut self, content: &str) {
        if !self.policy.supports_current_supplemental {
            return;
        }
        let text = normalize_visible_update(content, MAX_QUEUED_PARTIAL_CHARS);
        if text.chars().count() < MIN_PARTIAL_VISIBLE_CHARS {
            return;
        }
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => delivery.emit_partial_body(&text),
            // Non-edit channels cannot revise previously sent text, so exposing ToolUse-time
            // drafts or mid-turn status copy here would leak unfinished plans. Reserve queued
            // delivery for the canonical final answer only.
            DeliveryMode::Queued(_) => {}
            DeliveryMode::Silent => {}
        }
    }

    /// 返回 true 表示最终答复已经直接交付到通道，外层应跳过 outbound_tx。
    #[cfg(test)]
    pub(crate) fn finalize(&mut self, _final_content: &str) -> bool {
        if !self.policy.supports_current_primary {
            if let DeliveryMode::Queued(ref mut delivery) = self.mode {
                delivery.finalize();
            }
            return false;
        }
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => delivery.finalize(_final_content),
            DeliveryMode::Queued(ref mut delivery) => delivery.finalize(),
            DeliveryMode::Silent => false,
        }
    }

    /// Close any progress delivery before ReplyFinalize produces canonical text.
    /// This never sends raw final content; the canonical reply is delivered later.
    pub(crate) fn close_before_canonical_reply(&mut self) -> bool {
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => {
                delivery.close_without_primary();
                false
            }
            DeliveryMode::Queued(ref mut delivery) => delivery.finalize(),
            DeliveryMode::Silent => false,
        }
    }

    pub(crate) fn deliver_tool_outbound_intent(
        &mut self,
        intent: &ToolOutboundIntent,
    ) -> Result<ToolIntentDelivery> {
        self.bump_tool_intent_seen();
        let body = intent
            .body
            .clone()
            .unwrap_or_else(|| CanonicalMessageBody::text(intent.content.clone()));
        let text = normalize_visible_update(&body.text_projection(), crate::bus::MAX_CONTENT_LEN);
        if text.is_empty() {
            self.bump_tool_intent_suppressed();
            return Ok(ToolIntentDelivery::Suppressed);
        }
        match &intent.target {
            ToolOutboundTarget::CurrentChat => {
                self.bump_tool_intent_suppressed();
                Ok(ToolIntentDelivery::Suppressed)
            }
            ToolOutboundTarget::Explicit { channel, chat_id } => {
                if self.is_closed() {
                    self.bump_tool_intent_suppressed();
                    return Ok(ToolIntentDelivery::Suppressed);
                }
                send_visible_update_explicit(
                    self.outbound_tx,
                    channel,
                    chat_id,
                    self.req_id,
                    map_tool_outbound_kind(intent.delivery_kind),
                    &body,
                    &text,
                )
                .map_err(|()| {
                    crate::error::Error::config(
                        "tool_outbound_message",
                        "failed to enqueue explicit outbound message",
                    )
                })?;
                self.bump_tool_visible_update(true);
                Ok(ToolIntentDelivery::VisibleUpdate)
            }
        }
    }

    fn is_closed(&self) -> bool {
        match self.mode {
            DeliveryMode::Silent => true,
            DeliveryMode::Edit(ref delivery) => delivery.lifecycle.is_closed(),
            DeliveryMode::Queued(ref delivery) => delivery.lifecycle.is_closed(),
        }
    }

    pub(crate) fn has_visible_task_started_fact(&self) -> bool {
        self.fact_state.task_started_visible
    }

    pub(crate) fn has_visible_task_terminal_fact(&self) -> bool {
        self.fact_state.task_terminal_visible
    }

    fn bump_tool_intent_seen(&mut self) {
        match self.mode {
            DeliveryMode::Silent => {}
            DeliveryMode::Edit(ref mut delivery) => {
                delivery.report.tool_outbound_intents_seen =
                    delivery.report.tool_outbound_intents_seen.saturating_add(1);
            }
            DeliveryMode::Queued(ref mut delivery) => {
                delivery.report.tool_outbound_intents_seen =
                    delivery.report.tool_outbound_intents_seen.saturating_add(1);
            }
        }
    }

    fn bump_tool_visible_update(&mut self, explicit: bool) {
        match self.mode {
            DeliveryMode::Silent => {}
            DeliveryMode::Edit(ref mut delivery) => {
                delivery.report.tool_visible_updates_sent =
                    delivery.report.tool_visible_updates_sent.saturating_add(1);
                if explicit {
                    delivery.report.explicit_outbound_sent =
                        delivery.report.explicit_outbound_sent.saturating_add(1);
                }
            }
            DeliveryMode::Queued(ref mut delivery) => {
                delivery.report.tool_visible_updates_sent =
                    delivery.report.tool_visible_updates_sent.saturating_add(1);
                if explicit {
                    delivery.report.explicit_outbound_sent =
                        delivery.report.explicit_outbound_sent.saturating_add(1);
                }
            }
        }
    }

    fn bump_tool_intent_suppressed(&mut self) {
        match self.mode {
            DeliveryMode::Silent => {}
            DeliveryMode::Edit(ref mut delivery) => {
                delivery.report.tool_outbound_suppressed =
                    delivery.report.tool_outbound_suppressed.saturating_add(1);
            }
            DeliveryMode::Queued(ref mut delivery) => {
                delivery.report.tool_outbound_suppressed =
                    delivery.report.tool_outbound_suppressed.saturating_add(1);
            }
        }
    }
}

impl Drop for DeliverySession<'_> {
    fn drop(&mut self) {}
}

impl DeliveryFactState {
    fn record_visible_fact(&mut self, fact: TurnVisibilityFact<'_>) {
        match fact {
            TurnVisibilityFact::TaskStarted { .. } => self.task_started_visible = true,
            TurnVisibilityFact::TaskTerminal { .. } => self.task_terminal_visible = true,
            TurnVisibilityFact::Acknowledged
            | TurnVisibilityFact::Reasoning { .. }
            | TurnVisibilityFact::RunningTool { .. }
            | TurnVisibilityFact::TaskPlanner
            | TurnVisibilityFact::Finalizing => {}
        }
    }
}

impl<'a> EditDelivery<'a> {
    #[cfg(test)]
    fn on_stream_delta(&mut self, accumulated: &str) {
        if self.edit_disabled || self.lifecycle.is_closed() || accumulated.trim().is_empty() {
            return;
        }
        let normalized = normalize_visible_update(accumulated, crate::bus::MAX_CONTENT_LEN);
        if normalized.is_empty() {
            return;
        }
        let had_no_body = self.last_body_text.is_empty();
        self.last_body_text = normalized;
        self.clear_placeholder_header_for_body();
        self.sync_composed(had_no_body);
    }

    fn emit_fact(&mut self, contract: VisibleUpdateContract, fact: TurnVisibilityFact<'_>) -> bool {
        if self.edit_disabled || self.lifecycle.is_closed() {
            return false;
        }
        let Some(projection) = contract.project_fact(fact) else {
            return false;
        };
        if matches!(
            self.status_header.as_ref().map(|header| header.kind),
            Some(EditStatusHeaderKind::StickyTerminal)
        ) && projection.header.kind == EditStatusHeaderKind::Placeholder
        {
            return false;
        }
        let header_kind = projection.header.kind;
        if self.last_body_text.is_empty() || header_kind == EditStatusHeaderKind::StickyTerminal {
            self.status_header = Some(projection.header);
        } else {
            return false;
        }
        record_visible_update_kind(&mut self.report, projection.report_kind);
        self.sync_composed(header_kind == EditStatusHeaderKind::StickyTerminal);
        true
    }

    fn emit_partial_body(&mut self, content: &str) {
        if self.edit_disabled || self.lifecycle.is_closed() {
            return;
        }
        let had_no_body = self.last_body_text.is_empty();
        self.last_body_text = content.to_string();
        self.clear_placeholder_header_for_body();
        record_visible_update_kind(&mut self.report, VisibleUpdateKind::PartialDraft);
        self.sync_composed(had_no_body);
    }

    #[cfg(test)]
    fn finalize(&mut self, final_content: &str) -> bool {
        if self.lifecycle == DeliveryLifecycle::Finalized {
            return self.report.finalize_streamed;
        }
        let normalized = normalize_visible_update(final_content, crate::bus::MAX_CONTENT_LEN);
        self.status_header = None;
        self.pending_compose.clear();
        let already_visible_final = !normalized.is_empty() && self.last_sent_composed == normalized;
        let visible_now = if self.message_id.is_none() {
            if normalized.is_empty() {
                false
            } else {
                self.send_initial(&normalized)
            }
        } else if normalized.is_empty() {
            false
        } else {
            self.edit_existing(&normalized)
        };
        let streamed = self.message_id.is_some() && (visible_now || already_visible_final);
        if streamed && !normalized.is_empty() {
            self.last_body_text = normalized.clone();
            self.last_sent_composed = normalized;
        }
        self.lifecycle = DeliveryLifecycle::Finalized;
        self.report.finalize_streamed = streamed;
        streamed
    }

    fn close_without_primary(&mut self) {
        if self.lifecycle == DeliveryLifecycle::Finalized {
            return;
        }
        self.pending_compose.clear();
        self.status_header = None;
        self.lifecycle = DeliveryLifecycle::Finalized;
        self.report.finalize_streamed = false;
    }

    fn clear_placeholder_header_for_body(&mut self) {
        if matches!(
            self.status_header.as_ref().map(|header| header.kind),
            Some(EditStatusHeaderKind::Placeholder)
        ) {
            self.status_header = None;
        }
    }

    fn sync_composed(&mut self, force_flush: bool) {
        let composed = self.compose_visible_text();
        if composed.is_empty() {
            return;
        }
        self.pending_compose = composed;
        if self.message_id.is_none() {
            let initial = self.pending_compose.clone();
            self.pending_compose.clear();
            self.send_initial(&initial);
            return;
        }
        if !force_flush
            && self.last_edit_at.elapsed() < std::time::Duration::from_millis(EDIT_THROTTLE_MS)
        {
            return;
        }
        self.flush_pending();
    }

    fn flush_pending(&mut self) {
        if self.pending_compose.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.pending_compose);
        self.edit_existing(&pending);
    }

    fn compose_visible_text(&self) -> String {
        let body = self.last_body_text.trim();
        let composed = match (self.status_header.as_ref(), body.is_empty()) {
            (Some(header), false) if header.kind == EditStatusHeaderKind::StickyTerminal => {
                format!("{}\n\n{}", header.text, body)
            }
            (_, false) => body.to_string(),
            (Some(header), true) => header.text.clone(),
            (None, true) => String::new(),
        };
        normalize_visible_update(&composed, crate::bus::MAX_CONTENT_LEN)
    }

    fn send_initial(&mut self, content: &str) -> bool {
        let normalized = normalize_visible_update(content, crate::bus::MAX_CONTENT_LEN);
        if normalized.is_empty() {
            return false;
        }
        match self.editor.send_initial(self.chat_id, &normalized) {
            Ok(Some(message_id)) => {
                self.message_id = Some(message_id);
                self.last_sent_composed = normalized;
                self.last_edit_at = std::time::Instant::now();
                self.edit_failures = 0;
                self.report.visible_text_updates_sent =
                    self.report.visible_text_updates_sent.saturating_add(1);
                true
            }
            Ok(None) => false,
            Err(e) => {
                log::warn!(
                    "[agent_delivery] send_initial failed, disabling edit delivery: {}",
                    e
                );
                self.edit_disabled = true;
                false
            }
        }
    }

    fn edit_existing(&mut self, content: &str) -> bool {
        let normalized = normalize_visible_update(content, crate::bus::MAX_CONTENT_LEN);
        if normalized.is_empty() {
            return false;
        }
        if normalized == self.last_sent_composed {
            return true;
        }
        let Some(ref message_id) = self.message_id else {
            return false;
        };
        match self.editor.edit(self.chat_id, message_id, &normalized) {
            Ok(()) => {
                self.last_sent_composed = normalized;
                self.last_edit_at = std::time::Instant::now();
                self.edit_failures = 0;
                true
            }
            Err(e) => {
                self.edit_failures = self.edit_failures.saturating_add(1);
                if self.edit_failures >= MAX_EDIT_FAILURES {
                    log::warn!(
                        "[agent_delivery] edit failed {} times, disabling edit delivery: {}",
                        MAX_EDIT_FAILURES,
                        e
                    );
                    self.edit_disabled = true;
                } else {
                    log::debug!(
                        "[agent_delivery] edit failed ({}/{}): {}",
                        self.edit_failures,
                        MAX_EDIT_FAILURES,
                        e
                    );
                }
                self.last_edit_at = std::time::Instant::now();
                false
            }
        }
    }
}

impl QueuedDelivery {
    fn finalize(&mut self) -> bool {
        if let Some(ref visibility) = self.append_only_visibility {
            visibility.mark_finalized();
        }
        match self.lifecycle {
            DeliveryLifecycle::Finalized => false,
            DeliveryLifecycle::Open => {
                self.lifecycle = DeliveryLifecycle::Finalized;
                false
            }
        }
    }

    fn observe_fact(&self, fact: TurnVisibilityFact<'_>) {
        if let Some(ref visibility) = self.append_only_visibility {
            visibility.observe_fact(fact);
        }
    }

    fn report(&self) -> DeliveryReport {
        let mut report = self.report;
        if let Some(ref visibility) = self.append_only_visibility {
            let snapshot = visibility.report_snapshot();
            report.append_only_ack_sent = snapshot.append_only_ack_sent;
            report.append_only_heartbeat_sent = snapshot.append_only_heartbeat_sent;
            report.append_only_first_tool_milestone_sent =
                snapshot.append_only_first_tool_milestone_sent;
        }
        report
    }
}

impl AppendOnlyVisibilityShared {
    fn new_if_enabled(
        msg: &PcMsg,
        req_id: &str,
        outbound_tx: &OutboundTx,
        entry: crate::ChannelCapabilityEntry,
        loc: UiLocale,
        policy: DeliveryPolicy,
    ) -> Option<Arc<Self>> {
        if msg.ingress != IngressKind::User
            || !policy.supports_current_primary
            || !policy.supports_current_supplemental
        {
            return None;
        }
        let mode = if reaction_visibility_enabled(msg, entry) {
            AppendOnlyVisibilityMode::AnchoredReaction
        } else {
            match entry.contract.delivery_ordering_model {
                ChannelDeliveryOrderingModel::AppendOnly
                | ChannelDeliveryOrderingModel::StatelessWebhook
                | ChannelDeliveryOrderingModel::SessionSocket => {
                    if msg.is_group {
                        AppendOnlyVisibilityMode::GroupHeartbeat
                    } else {
                        AppendOnlyVisibilityMode::PrivateAck
                    }
                }
                ChannelDeliveryOrderingModel::EditableSingleMessage
                | ChannelDeliveryOrderingModel::AudioPlayback => return None,
            }
        };
        let shared = Arc::new(Self {
            delivery: AppendOnlyVisibilityDelivery {
                channel: Arc::clone(&msg.channel),
                chat_id: Arc::clone(&msg.chat_id),
                req_id: req_id.to_string(),
                is_group: msg.is_group,
                source_transport: msg.source_transport,
                platform_thread_id: msg.platform_thread_id.clone(),
                platform_message_id: msg.platform_message_id.clone(),
                platform_event_id: msg.platform_event_id.clone(),
                inbound_dedup_key: msg.inbound_dedup_key.clone(),
                outbound_tx: outbound_tx.clone(),
                contract: AppendOnlyVisibilityContract { loc },
            },
            mode,
            state: Arc::new(Mutex::new(AppendOnlyVisibilityState::default())),
        });
        if !matches!(shared.mode, AppendOnlyVisibilityMode::AnchoredReaction) {
            shared.register_deadline();
        }
        Some(shared)
    }

    fn register_deadline(self: &Arc<Self>) {
        let due_at = std::time::Instant::now() + self.mode.deadline_delay();
        let shared = Arc::clone(self);
        if crate::runtime::schedule_critical_delayed_task(
            due_at,
            Box::new(move || {
                shared.fire_deadline();
            }),
        )
        .is_err()
        {
            log::warn!(
                "[agent_delivery] append-only visibility deadline registration dropped req_id={} channel={} chat_id={} mode={}",
                self.delivery.req_id,
                self.delivery.channel,
                self.delivery.chat_id,
                self.mode.log_label()
            );
        }
    }

    fn mark_finalized(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.finalized = true;
    }

    fn observe_fact(&self, fact: TurnVisibilityFact<'_>) {
        if matches!(self.mode, AppendOnlyVisibilityMode::AnchoredReaction) {
            self.observe_reaction_fact(fact);
            return;
        }
        match fact {
            TurnVisibilityFact::Acknowledged
                if matches!(self.mode, AppendOnlyVisibilityMode::PrivateAck) =>
            {
                self.emit_private_ack_now();
            }
            TurnVisibilityFact::RunningTool { .. }
                if matches!(self.mode, AppendOnlyVisibilityMode::PrivateAck) =>
            {
                self.observe_private_tool_fact(fact);
            }
            TurnVisibilityFact::Acknowledged
            | TurnVisibilityFact::Reasoning { .. }
            | TurnVisibilityFact::RunningTool { .. }
            | TurnVisibilityFact::TaskPlanner
            | TurnVisibilityFact::TaskStarted { .. }
            | TurnVisibilityFact::TaskTerminal { .. }
            | TurnVisibilityFact::Finalizing => {}
        }
    }

    fn emit_private_ack_now(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.finalized
            || state.ack_sent
            || state.supplemental_emitted >= self.mode.max_supplemental_messages()
        {
            return;
        }
        let Some(projection) = self.delivery.contract.private_ack() else {
            return;
        };
        if send_current_chat_supplemental(&self.delivery, &projection.text).is_ok() {
            state.ack_sent = projection.marks_ack_sent;
            state.supplemental_emitted = state.supplemental_emitted.saturating_add(1);
        }
    }

    fn observe_private_tool_fact(&self, fact: TurnVisibilityFact<'_>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.finalized || state.first_tool_milestone_sent {
            return;
        }
        if state.first_tool_snapshot.is_none() {
            let TurnVisibilityFact::RunningTool { tool, .. } = fact else {
                return;
            };
            state.first_tool_snapshot = Some(FirstToolSnapshot {
                tool_name: truncate_content_to_max(tool.trim(), MAX_APPEND_ONLY_TOOL_NAME_CHARS)
                    .to_string(),
            });
        }
        if !state.ack_sent || state.supplemental_emitted >= self.mode.max_supplemental_messages() {
            return;
        }
        let Some(projection) = self
            .delivery
            .contract
            .private_first_tool_milestone(state.first_tool_snapshot.as_ref())
        else {
            return;
        };
        if send_current_chat_supplemental(&self.delivery, &projection.text).is_ok() {
            state.first_tool_milestone_sent = projection.marks_first_tool_milestone_sent;
            state.supplemental_emitted = state.supplemental_emitted.saturating_add(1);
        }
    }

    fn observe_reaction_fact(&self, fact: TurnVisibilityFact<'_>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.finalized || state.supplemental_emitted >= self.mode.max_supplemental_messages() {
            return;
        }
        let (emoji, marks_accepted, marks_working) = match fact {
            TurnVisibilityFact::Acknowledged if !state.reaction_accepted_sent => {
                (TELEGRAM_REACTION_ACCEPTED, true, false)
            }
            TurnVisibilityFact::RunningTool { .. }
                if state.reaction_accepted_sent && !state.reaction_working_sent =>
            {
                (TELEGRAM_REACTION_WORKING, false, true)
            }
            TurnVisibilityFact::Acknowledged
            | TurnVisibilityFact::Reasoning { .. }
            | TurnVisibilityFact::RunningTool { .. }
            | TurnVisibilityFact::TaskPlanner
            | TurnVisibilityFact::TaskStarted { .. }
            | TurnVisibilityFact::TaskTerminal { .. }
            | TurnVisibilityFact::Finalizing => return,
        };
        if send_current_chat_reaction(&self.delivery, emoji).is_ok() {
            if marks_accepted {
                state.reaction_accepted_sent = true;
            }
            if marks_working {
                state.reaction_working_sent = true;
            }
            state.supplemental_emitted = state.supplemental_emitted.saturating_add(1);
        }
    }

    fn fire_deadline(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.finalized || state.supplemental_emitted >= self.mode.max_supplemental_messages() {
            return;
        }
        let projection = match self.mode {
            AppendOnlyVisibilityMode::PrivateAck => {
                if state.ack_sent {
                    return;
                }
                if state.first_tool_snapshot.is_some() {
                    self.delivery
                        .contract
                        .private_ack_with_first_tool(state.first_tool_snapshot.as_ref())
                } else {
                    self.delivery.contract.private_ack()
                }
            }
            AppendOnlyVisibilityMode::GroupHeartbeat => {
                if state.heartbeat_sent {
                    return;
                }
                self.delivery.contract.group_heartbeat()
            }
            AppendOnlyVisibilityMode::AnchoredReaction => return,
        };
        let Some(projection) = projection else {
            return;
        };
        if send_current_chat_supplemental(&self.delivery, &projection.text).is_ok() {
            if projection.marks_ack_sent {
                state.ack_sent = true;
            }
            if projection.marks_heartbeat_sent {
                state.heartbeat_sent = true;
            }
            if projection.marks_first_tool_milestone_sent {
                state.first_tool_milestone_sent = true;
            }
            state.supplemental_emitted = state.supplemental_emitted.saturating_add(1);
        }
    }

    fn report_snapshot(&self) -> AppendOnlyVisibilityReportSnapshot {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        AppendOnlyVisibilityReportSnapshot {
            append_only_ack_sent: u8::from(state.ack_sent),
            append_only_heartbeat_sent: u8::from(state.heartbeat_sent),
            append_only_first_tool_milestone_sent: u8::from(state.first_tool_milestone_sent),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AppendOnlyVisibilityReportSnapshot {
    append_only_ack_sent: u8,
    append_only_heartbeat_sent: u8,
    append_only_first_tool_milestone_sent: u8,
}

impl AppendOnlyVisibilityMode {
    fn deadline_delay(self) -> std::time::Duration {
        match self {
            Self::PrivateAck => std::time::Duration::from_millis(APPEND_ONLY_PRIVATE_ACK_DELAY_MS),
            Self::GroupHeartbeat => {
                std::time::Duration::from_millis(APPEND_ONLY_GROUP_HEARTBEAT_DELAY_MS)
            }
            Self::AnchoredReaction => std::time::Duration::from_millis(0),
        }
    }

    fn max_supplemental_messages(self) -> u8 {
        match self {
            Self::PrivateAck => 2,
            Self::GroupHeartbeat => 1,
            Self::AnchoredReaction => 3,
        }
    }

    fn log_label(self) -> &'static str {
        match self {
            Self::PrivateAck => "private_ack",
            Self::GroupHeartbeat => "group_heartbeat",
            Self::AnchoredReaction => "anchored_reaction",
        }
    }
}

impl VisibleUpdateContract {
    fn new(loc: UiLocale) -> Self {
        Self { loc }
    }

    fn project_fact(self, fact: TurnVisibilityFact<'_>) -> Option<EditFactProjection> {
        let (text, report_kind, header_kind) = match fact {
            TurnVisibilityFact::Acknowledged => (
                match self.loc {
                    UiLocale::Zh => "已收到，正在处理".to_string(),
                    UiLocale::En => "Received, processing".to_string(),
                },
                VisibleUpdateKind::Acknowledged,
                EditStatusHeaderKind::Placeholder,
            ),
            TurnVisibilityFact::Reasoning { round } => (
                match (self.loc, round) {
                    (UiLocale::Zh, 0 | 1) => "正在分析当前请求".to_string(),
                    (UiLocale::Zh, _) => "正在继续分析当前请求".to_string(),
                    (UiLocale::En, 0 | 1) => "Analyzing the current request".to_string(),
                    (UiLocale::En, _) => "Continuing to analyze the current request".to_string(),
                },
                VisibleUpdateKind::Reasoning,
                EditStatusHeaderKind::Placeholder,
            ),
            TurnVisibilityFact::RunningTool { tool, index, total } => (
                match self.loc {
                    UiLocale::Zh if total > 1 => {
                        format!("正在执行 {}（{}/{}）", tool, index + 1, total)
                    }
                    UiLocale::Zh => format!("正在执行 {}", tool),
                    UiLocale::En if total > 1 => {
                        format!("Running {} ({}/{})", tool, index + 1, total)
                    }
                    UiLocale::En => format!("Running {}", tool),
                },
                VisibleUpdateKind::ToolProgress,
                EditStatusHeaderKind::Placeholder,
            ),
            TurnVisibilityFact::TaskPlanner => (
                match self.loc {
                    UiLocale::Zh => "正在规划当前任务".to_string(),
                    UiLocale::En => "Planning the current task".to_string(),
                },
                VisibleUpdateKind::PlannerProgress,
                EditStatusHeaderKind::Placeholder,
            ),
            TurnVisibilityFact::TaskStarted { resumed } => (
                match (self.loc, resumed) {
                    (UiLocale::Zh, true) => "已恢复任务执行".to_string(),
                    (UiLocale::Zh, false) => "已进入任务执行".to_string(),
                    (UiLocale::En, true) => "Task execution resumed".to_string(),
                    (UiLocale::En, false) => "Task execution started".to_string(),
                },
                VisibleUpdateKind::ActionProgress,
                EditStatusHeaderKind::Placeholder,
            ),
            TurnVisibilityFact::TaskTerminal { status } => (
                match (self.loc, status) {
                    (UiLocale::Zh, TaskTerminalVisibilityStatus::Completed) => {
                        "当前任务已完成".to_string()
                    }
                    (UiLocale::Zh, TaskTerminalVisibilityStatus::PartialComplete) => {
                        "当前任务已部分完成".to_string()
                    }
                    (UiLocale::Zh, TaskTerminalVisibilityStatus::Blocked) => {
                        "当前任务已阻塞".to_string()
                    }
                    (UiLocale::Zh, TaskTerminalVisibilityStatus::Aborted) => {
                        "当前任务已终止".to_string()
                    }
                    (UiLocale::En, TaskTerminalVisibilityStatus::Completed) => {
                        "Task completed".to_string()
                    }
                    (UiLocale::En, TaskTerminalVisibilityStatus::PartialComplete) => {
                        "Task partially completed".to_string()
                    }
                    (UiLocale::En, TaskTerminalVisibilityStatus::Blocked) => {
                        "Task blocked".to_string()
                    }
                    (UiLocale::En, TaskTerminalVisibilityStatus::Aborted) => {
                        "Task aborted".to_string()
                    }
                },
                VisibleUpdateKind::TerminalProgress,
                EditStatusHeaderKind::StickyTerminal,
            ),
            TurnVisibilityFact::Finalizing => (
                match self.loc {
                    UiLocale::Zh => "正在整理最终答复".to_string(),
                    UiLocale::En => "Preparing the final reply".to_string(),
                },
                VisibleUpdateKind::Finalizing,
                EditStatusHeaderKind::Placeholder,
            ),
        };
        let text = normalize_visible_update(&text, MAX_QUEUED_PROGRESS_CHARS);
        if text.is_empty() {
            return None;
        }
        Some(EditFactProjection {
            header: EditStatusHeader {
                text,
                kind: header_kind,
            },
            report_kind,
        })
    }
}

impl AppendOnlyVisibilityContract {
    fn private_ack(self) -> Option<AppendOnlyVisibilityProjection> {
        Some(AppendOnlyVisibilityProjection {
            text: normalize_visible_update(
                match self.loc {
                    UiLocale::Zh => "已收到，正在处理",
                    UiLocale::En => "Received, processing",
                },
                MAX_QUEUED_PROGRESS_CHARS,
            ),
            marks_ack_sent: true,
            marks_heartbeat_sent: false,
            marks_first_tool_milestone_sent: false,
        })
    }

    fn group_heartbeat(self) -> Option<AppendOnlyVisibilityProjection> {
        Some(AppendOnlyVisibilityProjection {
            text: normalize_visible_update(
                match self.loc {
                    UiLocale::Zh => "仍在处理",
                    UiLocale::En => "Still processing",
                },
                MAX_QUEUED_PROGRESS_CHARS,
            ),
            marks_ack_sent: false,
            marks_heartbeat_sent: true,
            marks_first_tool_milestone_sent: false,
        })
    }

    fn private_first_tool_milestone(
        self,
        snapshot: Option<&FirstToolSnapshot>,
    ) -> Option<AppendOnlyVisibilityProjection> {
        let _has_tool_name = snapshot.is_some_and(|snapshot| !snapshot.tool_name.is_empty());
        Some(AppendOnlyVisibilityProjection {
            text: normalize_visible_update(
                match self.loc {
                    UiLocale::Zh => "已进入首个工具执行",
                    UiLocale::En => "Started the first tool execution",
                },
                MAX_QUEUED_PROGRESS_CHARS,
            ),
            marks_ack_sent: false,
            marks_heartbeat_sent: false,
            marks_first_tool_milestone_sent: true,
        })
    }

    fn private_ack_with_first_tool(
        self,
        snapshot: Option<&FirstToolSnapshot>,
    ) -> Option<AppendOnlyVisibilityProjection> {
        let ack = self.private_ack()?.text;
        let milestone = self.private_first_tool_milestone(snapshot)?.text;
        Some(AppendOnlyVisibilityProjection {
            text: normalize_visible_update(
                &format!("{ack}\n{milestone}"),
                MAX_QUEUED_PROGRESS_CHARS,
            ),
            marks_ack_sent: true,
            marks_heartbeat_sent: false,
            marks_first_tool_milestone_sent: true,
        })
    }
}

fn record_visible_update_kind(report: &mut DeliveryReport, kind: VisibleUpdateKind) {
    match kind {
        VisibleUpdateKind::Acknowledged
        | VisibleUpdateKind::Reasoning
        | VisibleUpdateKind::Finalizing => {
            report.edit_phase_header_updates_sent =
                report.edit_phase_header_updates_sent.saturating_add(1);
        }
        VisibleUpdateKind::PlannerProgress => {
            report.edit_phase_header_updates_sent =
                report.edit_phase_header_updates_sent.saturating_add(1);
            report.edit_planner_header_updates_sent =
                report.edit_planner_header_updates_sent.saturating_add(1);
        }
        VisibleUpdateKind::ToolProgress => {
            report.edit_phase_header_updates_sent =
                report.edit_phase_header_updates_sent.saturating_add(1);
            report.edit_tool_header_updates_sent =
                report.edit_tool_header_updates_sent.saturating_add(1);
        }
        VisibleUpdateKind::ActionProgress => {
            report.edit_phase_header_updates_sent =
                report.edit_phase_header_updates_sent.saturating_add(1);
            report.edit_action_header_updates_sent =
                report.edit_action_header_updates_sent.saturating_add(1);
        }
        VisibleUpdateKind::TerminalProgress => {
            report.edit_phase_header_updates_sent =
                report.edit_phase_header_updates_sent.saturating_add(1);
            report.edit_terminal_header_updates_sent =
                report.edit_terminal_header_updates_sent.saturating_add(1);
        }
        VisibleUpdateKind::PartialDraft => {
            report.partial_updates_sent = report.partial_updates_sent.saturating_add(1);
        }
    }
}

fn normalize_visible_update(content: &str, max_chars: usize) -> String {
    truncate_content_to_max(content.trim(), max_chars)
        .trim()
        .to_string()
}

fn send_reliable_or_best_effort_outbound(
    outbound_tx: &OutboundTx,
    msg: PcMsg,
    log_owner: &str,
) -> std::result::Result<(), ()> {
    let req_id = msg.req_id.clone().unwrap_or_default();
    let channel = msg.channel.clone();
    let chat_id = msg.chat_id.clone();
    let outbound_kind = msg.outbound_kind;
    let mut pending = msg;
    let mut full_attempts = 0u32;
    loop {
        match outbound_tx.try_send(pending) {
            Ok(()) => {
                metrics::record_message_out();
                if outbound_kind == OutboundKind::Visibility {
                    log::info!(
                        "[agent_delivery] foreground_ack event=visibility_enqueued before_llm=true owner={} req_id={} channel={} chat_id={}",
                        log_owner,
                        req_id,
                        channel,
                        chat_id
                    );
                } else if outbound_kind == OutboundKind::Primary {
                    log::info!(
                        "[agent_delivery] primary_delivery event=outbound_enqueued delivered=true owner={} req_id={} channel={} chat_id={}",
                        log_owner,
                        req_id,
                        channel,
                        chat_id
                    );
                }
                return Ok(());
            }
            Err(std::sync::mpsc::TrySendError::Full(msg))
                if !msg.outbound_kind.is_best_effort_delivery() =>
            {
                full_attempts = full_attempts.saturating_add(1);
                if full_attempts == 1
                    || full_attempts.is_multiple_of(RELIABLE_OUTBOUND_ENQUEUE_LOG_EVERY)
                {
                    log::warn!(
                        "[agent_delivery] reliable outbound queue full owner={} req_id={} channel={} chat_id={} outbound_kind={}, applying backpressure",
                        log_owner,
                        req_id,
                        channel,
                        chat_id,
                        msg.outbound_kind.as_str()
                    );
                }
                crate::platform::task_wdt::feed_current_task();
                std::thread::sleep(std::time::Duration::from_millis(
                    RELIABLE_OUTBOUND_ENQUEUE_RETRY_DELAY_MS,
                ));
                crate::platform::task_wdt::feed_current_task();
                pending = msg;
            }
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                metrics::record_outbound_enqueue_fail();
                log::warn!(
                    "[agent_delivery] best-effort outbound dropped: outbound queue full owner={} req_id={} channel={} chat_id={} outbound_kind={}",
                    log_owner,
                    req_id,
                    channel,
                    chat_id,
                    outbound_kind.as_str()
                );
                return Err(());
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                metrics::record_outbound_enqueue_fail();
                log::error!(
                    "[agent_delivery] outbound dropped: outbound disconnected owner={} req_id={} channel={} chat_id={} outbound_kind={}",
                    log_owner,
                    req_id,
                    channel,
                    chat_id,
                    outbound_kind.as_str()
                );
                return Err(());
            }
        }
    }
}

fn send_current_chat_supplemental(
    delivery: &AppendOnlyVisibilityDelivery,
    content: &str,
) -> std::result::Result<(), ()> {
    let mut msg = match PcMsg::new_outbound_for_chat(
        &delivery.channel,
        &delivery.chat_id,
        content,
        Some(delivery.req_id.clone()),
        delivery.is_group,
    ) {
        Ok(msg) => msg,
        Err(error) => {
            log::warn!(
                "[agent_delivery] current-chat supplemental rejected req_id={} channel={} chat_id={}: {}",
                delivery.req_id,
                delivery.channel,
                delivery.chat_id,
                error
            );
            return Err(());
        }
    };
    msg = msg
        .with_inbound_provenance(
            delivery.source_transport,
            delivery.platform_message_id.clone(),
            delivery.platform_event_id.clone(),
            delivery.inbound_dedup_key.clone(),
        )
        .with_platform_thread_id(delivery.platform_thread_id.clone());
    msg.outbound_kind = OutboundKind::Visibility;
    send_reliable_or_best_effort_outbound(&delivery.outbound_tx, msg, "current_chat_visibility")
}

fn reaction_visibility_enabled(msg: &PcMsg, entry: crate::ChannelCapabilityEntry) -> bool {
    msg.ingress == IngressKind::User
        && entry.enabled
        && entry.contract.supports_primary_reply
        && entry.contract.supports_supplemental_reply
        && entry.contract.supports_message_reaction
        && !msg.platform_message_id.trim().is_empty()
}

pub(crate) fn send_terminal_reaction_if_enabled(
    msg: &PcMsg,
    req_id: &str,
    outbound_tx: &OutboundTx,
    channel_capability: Option<crate::ChannelCapabilityEntry>,
    success: bool,
) -> bool {
    let Some(entry) = channel_capability else {
        return false;
    };
    if !reaction_visibility_enabled(msg, entry) {
        return false;
    }
    let delivery = AppendOnlyVisibilityDelivery {
        channel: Arc::clone(&msg.channel),
        chat_id: Arc::clone(&msg.chat_id),
        req_id: req_id.to_string(),
        is_group: msg.is_group,
        source_transport: msg.source_transport,
        platform_thread_id: msg.platform_thread_id.clone(),
        platform_message_id: msg.platform_message_id.clone(),
        platform_event_id: msg.platform_event_id.clone(),
        inbound_dedup_key: msg.inbound_dedup_key.clone(),
        outbound_tx: outbound_tx.clone(),
        contract: AppendOnlyVisibilityContract { loc: UiLocale::Zh },
    };
    let emoji = if success {
        TELEGRAM_REACTION_SUCCEEDED
    } else {
        TELEGRAM_REACTION_FAILED
    };
    send_current_chat_reaction(&delivery, emoji).is_ok()
}

fn send_current_chat_reaction(
    delivery: &AppendOnlyVisibilityDelivery,
    emoji: &str,
) -> std::result::Result<(), ()> {
    let body =
        CanonicalMessageBody::PlatformNative(PlatformNativeBody::telegram_message_reaction(emoji));
    let mut msg = match PcMsg::new_outbound_for_chat_with_body(
        &delivery.channel,
        &delivery.chat_id,
        body,
        emoji.to_string(),
        Some(delivery.req_id.clone()),
        delivery.is_group,
    ) {
        Ok(msg) => msg,
        Err(error) => {
            log::warn!(
                "[agent_delivery] current-chat reaction rejected req_id={} channel={} chat_id={}: {}",
                delivery.req_id,
                delivery.channel,
                delivery.chat_id,
                error
            );
            return Err(());
        }
    };
    msg = msg
        .with_inbound_provenance(
            delivery.source_transport,
            delivery.platform_message_id.clone(),
            delivery.platform_event_id.clone(),
            delivery.inbound_dedup_key.clone(),
        )
        .with_platform_thread_id(delivery.platform_thread_id.clone());
    msg.outbound_kind = OutboundKind::Visibility;
    send_reliable_or_best_effort_outbound(&delivery.outbound_tx, msg, "current_chat_reaction")
}

fn send_visible_update_explicit(
    outbound_tx: &OutboundTx,
    channel: &str,
    chat_id: &str,
    req_id: &str,
    outbound_kind: OutboundKind,
    body: &CanonicalMessageBody,
    content: &str,
) -> std::result::Result<(), ()> {
    let channel = std::sync::Arc::<str>::from(channel.to_string());
    let chat_id = std::sync::Arc::<str>::from(chat_id.to_string());
    let mut msg = match PcMsg::new_outbound_for_chat_with_body(
        &channel,
        &chat_id,
        body.clone(),
        content.to_string(),
        Some(req_id.to_string()),
        false,
    ) {
        Ok(msg) => msg,
        Err(error) => {
            log::warn!(
                "[agent_delivery] explicit outbound rejected channel={} chat_id={}: {}",
                channel,
                chat_id,
                error
            );
            return Err(());
        }
    };
    msg.outbound_kind = outbound_kind;
    msg.req_id = Some(req_id.to_string());
    send_reliable_or_best_effort_outbound(outbound_tx, msg, "explicit_outbound")
}

fn map_tool_outbound_kind(delivery_kind: ToolOutboundDeliveryKind) -> OutboundKind {
    match delivery_kind {
        ToolOutboundDeliveryKind::Supplemental => OutboundKind::Supplemental,
        ToolOutboundDeliveryKind::Primary => OutboundKind::Primary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{new_inbound_channel, OutboundKind, TextBody, TextFormat};
    use crate::channel_capability::{
        ChannelCapabilityContract, ChannelCapabilityEntry, ChannelDeliveryOrderingModel,
    };
    use crate::tools::{ToolOutboundDeliveryKind, ToolOutboundIntent, ToolOutboundTarget};
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubEditor {
        sends: Mutex<Vec<String>>,
        edits: Mutex<Vec<String>>,
    }

    impl StreamEditor for StubEditor {
        fn send_initial(&self, _chat_id: &str, content: &str) -> Result<Option<String>> {
            self.sends
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(content.to_string());
            Ok(Some("msg-1".to_string()))
        }

        fn edit(&self, _chat_id: &str, _message_id: &str, content: &str) -> Result<()> {
            self.edits
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(content.to_string());
            Ok(())
        }
    }

    #[derive(Default)]
    struct FailingEditEditor {
        sends: Mutex<Vec<String>>,
        edits: Mutex<Vec<String>>,
    }

    impl StreamEditor for FailingEditEditor {
        fn send_initial(&self, _chat_id: &str, content: &str) -> Result<Option<String>> {
            self.sends
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(content.to_string());
            Ok(Some("msg-1".to_string()))
        }

        fn edit(&self, _chat_id: &str, _message_id: &str, content: &str) -> Result<()> {
            self.edits
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(content.to_string());
            Err(crate::error::Error::config(
                "stream_edit",
                "synthetic edit failure",
            ))
        }
    }

    fn build_msg(channel: &str) -> PcMsg {
        build_msg_with_group(channel, false)
    }

    fn build_msg_with_group(channel: &str, is_group: bool) -> PcMsg {
        PcMsg::new_inbound(channel, "chat-1", "hello", is_group).expect("pcmsg")
    }

    fn capability_entry(
        id: &'static str,
        supports_primary_reply: bool,
        supports_supplemental_reply: bool,
        supports_stream_edit: bool,
    ) -> ChannelCapabilityEntry {
        ChannelCapabilityEntry {
            id,
            configured: true,
            enabled: true,
            contract: ChannelCapabilityContract {
                supports_primary_reply,
                supports_supplemental_reply,
                supports_edit: supports_stream_edit,
                supports_stream_edit,
                supports_explicit_target: true,
                supports_attachment: false,
                supports_typing_or_chat_action: false,
                supports_message_reaction: false,
                supported_body_kinds: &[crate::bus::MessageBodyKind::Text],
                supported_text_formats: &[crate::bus::TextFormat::Plain],
                requires_pre_upload_for_media: false,
                supports_platform_handle_reuse: false,
                supports_http_url_media: false,
                requires_passive_reply_anchor: false,
                max_text_chars: Some(4096),
                max_caption_chars: None,
                delivery_ordering_model: if supports_stream_edit {
                    ChannelDeliveryOrderingModel::EditableSingleMessage
                } else {
                    ChannelDeliveryOrderingModel::AppendOnly
                },
            },
        }
    }

    fn reaction_capability_entry(id: &'static str) -> ChannelCapabilityEntry {
        let mut entry = capability_entry(id, true, true, false);
        entry.contract.supports_message_reaction = true;
        entry
    }

    fn assert_reaction_message(outbound: &PcMsg, expected_emoji: &str) {
        assert_eq!(outbound.outbound_kind, OutboundKind::Visibility);
        assert_eq!(outbound.platform_message_id, "9");
        match &outbound.body {
            crate::bus::CanonicalMessageBody::PlatformNative(native) => {
                assert_eq!(native.platform_type, "telegram_message_reaction");
                assert_eq!(native.payload_json["emoji"], expected_emoji);
            }
            other => panic!("expected platform-native reaction body, got {other:?}"),
        }
    }

    fn reset_delayed_tasks() {
        // `delayed_task_test_lock` owns reset so tests take the state lock first.
    }

    fn delayed_task_test_lock() -> (
        std::sync::MutexGuard<'static, ()>,
        std::sync::MutexGuard<'static, ()>,
    ) {
        crate::runtime::delayed_task::delayed_task_test_scope()
    }

    fn service_due_delayed_tasks_after(wait_ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(wait_ms));
        crate::runtime::service_delayed_tasks();
    }

    #[test]
    fn queued_delivery_emits_ack_and_first_tool_only_for_current_chat() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_fact(TurnVisibilityFact::Acknowledged);
        delivery.emit_fact(TurnVisibilityFact::Reasoning { round: 1 });
        delivery.emit_partial("第二步：继续处理中");
        delivery.emit_fact(TurnVisibilityFact::TaskPlanner);
        delivery.emit_fact(TurnVisibilityFact::RunningTool {
            tool: "board_info",
            index: 0,
            total: 1,
        });
        delivery.emit_fact(TurnVisibilityFact::Finalizing);

        let ack = outbound_rx.try_recv().expect("current-chat ack");
        assert_eq!(ack.content, "已收到，正在处理");
        assert_eq!(ack.outbound_kind, OutboundKind::Visibility);
        let first_tool = outbound_rx.try_recv().expect("first tool milestone");
        assert_eq!(first_tool.content, "已进入首个工具执行");
        assert_eq!(first_tool.outbound_kind, OutboundKind::Visibility);
        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report().append_only_ack_sent, 1);
        assert_eq!(delivery.report().append_only_first_tool_milestone_sent, 1);
        assert_eq!(delivery.report().append_only_heartbeat_sent, 0);
        assert_eq!(delivery.report().edit_phase_header_updates_sent, 0);
        assert_eq!(delivery.report().partial_updates_sent, 0);
    }

    #[test]
    fn current_chat_visibility_waits_for_outbound_queue_space() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(1);
        outbound_tx
            .try_send(PcMsg::new("qq_channel", "chat-1", "queued").expect("queued"))
            .expect("fill outbound queue");
        let tx = outbound_tx.clone();
        let receiver = std::thread::spawn(move || {
            let first = outbound_rx.recv().expect("first queued message");
            let second = outbound_rx.recv().expect("visibility ack");
            (first.content, second.content, second.outbound_kind)
        });
        let delivery = AppendOnlyVisibilityDelivery {
            channel: Arc::from("qq_channel"),
            chat_id: Arc::from("chat-1"),
            req_id: "req-visible".to_string(),
            is_group: false,
            source_transport: MessageTransport::Internal,
            platform_thread_id: String::new(),
            platform_message_id: String::new(),
            platform_event_id: String::new(),
            inbound_dedup_key: String::new(),
            outbound_tx: tx,
            contract: AppendOnlyVisibilityContract { loc: UiLocale::Zh },
        };

        assert!(send_current_chat_supplemental(&delivery, "已收到，正在处理").is_ok());

        let (first, second, kind) = receiver.join().expect("receiver joins");
        assert_eq!(first, "queued");
        assert_eq!(second, "已收到，正在处理");
        assert_eq!(kind, OutboundKind::Visibility);
    }

    #[test]
    fn queued_delivery_suppresses_model_partial_drafts() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_partial("## 第2步：检查文件系统结构");

        assert!(outbound_rx.try_recv().is_err());
    }

    #[test]
    fn queued_delivery_without_supplemental_contract_suppresses_fact_updates() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, false, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_fact(TurnVisibilityFact::Acknowledged);

        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report(), DeliveryReport::default());
    }

    #[test]
    fn queued_private_visibility_deadline_sends_ack() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel").with_inbound_provenance(
            crate::bus::MessageTransport::Wss,
            "msg-1",
            "event-1",
            "qq_message:msg-1",
        );
        let delivery = DeliverySession::new(
            &msg,
            "req-append-only-ack",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        service_due_delayed_tasks_after(APPEND_ONLY_PRIVATE_ACK_DELAY_MS + 5);

        let outbound = outbound_rx.try_recv().expect("append-only ack");
        assert_eq!(outbound.content, "已收到，正在处理");
        assert_eq!(outbound.outbound_kind, OutboundKind::Visibility);
        assert_eq!(outbound.source_transport, crate::bus::MessageTransport::Wss);
        assert_eq!(outbound.platform_message_id, "msg-1");
        assert_eq!(outbound.platform_event_id, "event-1");
        assert_eq!(outbound.inbound_dedup_key, "qq_message:msg-1");
        assert_eq!(delivery.report().append_only_ack_sent, 1);
        assert_eq!(delivery.report().append_only_first_tool_milestone_sent, 0);
    }

    #[test]
    fn queued_private_ack_fact_is_visible_immediately_before_llm() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel").with_inbound_provenance(
            crate::bus::MessageTransport::Wss,
            "msg-1",
            "event-1",
            "qq_message:msg-1",
        );
        let mut delivery = DeliverySession::new(
            &msg,
            "req-pre-llm-ack",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_fact(TurnVisibilityFact::Acknowledged);

        let outbound = outbound_rx.try_recv().expect("immediate pre-LLM ack");
        assert_eq!(outbound.content, "已收到，正在处理");
        assert_eq!(outbound.outbound_kind, OutboundKind::Visibility);
        assert_eq!(outbound.source_transport, crate::bus::MessageTransport::Wss);
        assert_eq!(outbound.platform_message_id, "msg-1");
        assert_eq!(outbound.platform_event_id, "event-1");
        assert_eq!(outbound.inbound_dedup_key, "qq_message:msg-1");
        assert_eq!(delivery.report().append_only_ack_sent, 1);

        service_due_delayed_tasks_after(APPEND_ONLY_PRIVATE_ACK_DELAY_MS + 5);
        assert!(
            outbound_rx.try_recv().is_err(),
            "deadline must not duplicate pre-LLM ack"
        );
    }

    #[test]
    fn queued_private_visibility_deadline_combines_ack_with_first_tool_milestone() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(
            &msg,
            "req-append-only-combined",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_fact(TurnVisibilityFact::RunningTool {
            tool: "board_info",
            index: 0,
            total: 1,
        });
        service_due_delayed_tasks_after(APPEND_ONLY_PRIVATE_ACK_DELAY_MS + 5);

        let outbound = outbound_rx.try_recv().expect("combined visibility");
        assert_eq!(outbound.content, "已收到，正在处理\n已进入首个工具执行");
        assert_eq!(outbound.outbound_kind, OutboundKind::Visibility);
        assert_eq!(delivery.report().append_only_ack_sent, 1);
        assert_eq!(delivery.report().append_only_first_tool_milestone_sent, 1);
        assert!(outbound_rx.try_recv().is_err());
    }

    #[test]
    fn queued_private_visibility_sends_late_first_tool_milestone_after_ack() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(
            &msg,
            "req-append-only-late-tool",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        service_due_delayed_tasks_after(APPEND_ONLY_PRIVATE_ACK_DELAY_MS + 5);
        let ack = outbound_rx.try_recv().expect("ack");
        assert_eq!(ack.content, "已收到，正在处理");

        delivery.emit_fact(TurnVisibilityFact::RunningTool {
            tool: "board_info",
            index: 0,
            total: 1,
        });

        let milestone = outbound_rx.try_recv().expect("first-tool milestone");
        assert_eq!(milestone.content, "已进入首个工具执行");
        assert_eq!(milestone.outbound_kind, OutboundKind::Visibility);
        assert_eq!(delivery.report().append_only_ack_sent, 1);
        assert_eq!(delivery.report().append_only_first_tool_milestone_sent, 1);
    }

    #[test]
    fn queued_group_visibility_deadline_sends_heartbeat() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg_with_group("qq_channel", true);
        let delivery = DeliverySession::new(
            &msg,
            "req-group-heartbeat",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        service_due_delayed_tasks_after(APPEND_ONLY_GROUP_HEARTBEAT_DELAY_MS + 5);

        let outbound = outbound_rx.try_recv().expect("group heartbeat");
        assert_eq!(outbound.content, "仍在处理");
        assert_eq!(outbound.outbound_kind, OutboundKind::Visibility);
        assert_eq!(delivery.report().append_only_heartbeat_sent, 1);
        assert_eq!(delivery.report().append_only_ack_sent, 0);
    }

    #[test]
    fn telegram_anchored_reaction_visibility_uses_message_anchor_and_suppresses_text() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("telegram").with_inbound_provenance(
            crate::bus::MessageTransport::Poll,
            "9",
            "",
            "telegram_message:9",
        );
        let mut delivery = DeliverySession::new(
            &msg,
            "req-reaction",
            &outbound_tx,
            None,
            Some(reaction_capability_entry("telegram")),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_fact(TurnVisibilityFact::Acknowledged);
        delivery.emit_fact(TurnVisibilityFact::RunningTool {
            tool: "board_info",
            index: 0,
            total: 1,
        });
        service_due_delayed_tasks_after(APPEND_ONLY_PRIVATE_ACK_DELAY_MS + 5);

        let accepted = outbound_rx.try_recv().expect("accepted reaction");
        assert_reaction_message(&accepted, "👀");
        let working = outbound_rx.try_recv().expect("working reaction");
        assert_reaction_message(&working, "⏳");
        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report().append_only_ack_sent, 0);
        assert_eq!(delivery.report().append_only_first_tool_milestone_sent, 0);
    }

    #[test]
    fn telegram_without_message_anchor_keeps_append_only_text_visibility() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("telegram");
        let delivery = DeliverySession::new(
            &msg,
            "req-reaction-no-anchor",
            &outbound_tx,
            None,
            Some(reaction_capability_entry("telegram")),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        service_due_delayed_tasks_after(APPEND_ONLY_PRIVATE_ACK_DELAY_MS + 5);

        let outbound = outbound_rx.try_recv().expect("text fallback");
        assert_eq!(outbound.content, "已收到，正在处理");
        assert!(matches!(
            outbound.body,
            crate::bus::CanonicalMessageBody::Text(_)
        ));
        assert_eq!(delivery.report().append_only_ack_sent, 1);
    }

    #[test]
    fn terminal_reaction_helper_uses_anchor_and_supports_failure_state() {
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(4);
        let msg = build_msg("telegram").with_inbound_provenance(
            crate::bus::MessageTransport::Poll,
            "9",
            "",
            "telegram_message:9",
        );

        let sent = send_terminal_reaction_if_enabled(
            &msg,
            "req-terminal",
            &outbound_tx,
            Some(reaction_capability_entry("telegram")),
            false,
        );

        assert!(sent);
        let outbound = outbound_rx.try_recv().expect("terminal reaction");
        assert_reaction_message(&outbound, "⚠️");
        assert_eq!(outbound.req_id.as_deref(), Some("req-terminal"));
    }

    #[test]
    fn queued_visibility_finalize_before_deadline_turns_deadline_into_noop() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(
            &msg,
            "req-append-only-finalize-first",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        assert!(!delivery.finalize("最终答复"));
        service_due_delayed_tasks_after(APPEND_ONLY_PRIVATE_ACK_DELAY_MS + 5);

        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report().append_only_ack_sent, 0);
        assert_eq!(delivery.report().append_only_heartbeat_sent, 0);
    }

    #[test]
    fn queued_system_turn_never_registers_append_only_deadline() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = PcMsg::new_inbound_with_ingress(
            "qq_channel",
            "chat-system",
            "system step",
            false,
            IngressKind::System,
        )
        .expect("system msg");
        let delivery = DeliverySession::new(
            &msg,
            "req-system-turn",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        service_due_delayed_tasks_after(APPEND_ONLY_PRIVATE_ACK_DELAY_MS + 5);

        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report().append_only_ack_sent, 0);
        assert_eq!(delivery.report().append_only_heartbeat_sent, 0);
    }

    #[test]
    fn queued_visibility_degrades_when_critical_delayed_task_slots_are_exhausted() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let hold_until = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while crate::runtime::schedule_critical_delayed_task(hold_until, Box::new(|| {})).is_ok() {}

        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let delivery = DeliverySession::new(
            &msg,
            "req-critical-slots-full",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        service_due_delayed_tasks_after(APPEND_ONLY_PRIVATE_ACK_DELAY_MS + 5);

        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report().append_only_ack_sent, 0);
        assert_eq!(delivery.report().append_only_heartbeat_sent, 0);
    }

    #[test]
    fn edit_delivery_finalizes_without_outbound_message() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, _outbound_rx, _) = new_inbound_channel(4);
        let msg = build_msg("telegram");
        let editor = StubEditor::default();
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            Some(&editor),
            Some(capability_entry("telegram", true, true, true)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_fact(TurnVisibilityFact::TaskPlanner);
        let streamed = delivery.finalize("最终答案");

        assert!(streamed);
        assert_eq!(
            delivery.report(),
            DeliveryReport {
                edit_phase_header_updates_sent: 1,
                edit_planner_header_updates_sent: 1,
                finalize_streamed: true,
                visible_text_updates_sent: 1,
                ..DeliveryReport::default()
            }
        );
        assert_eq!(
            editor
                .sends
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_slice(),
            ["正在规划当前任务"]
        );
        assert_eq!(
            editor
                .edits
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_slice(),
            ["最终答案"]
        );
    }

    #[test]
    fn edit_delivery_close_before_canonical_reply_does_not_send_raw_final() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, _outbound_rx, _) = new_inbound_channel(4);
        let msg = build_msg("telegram");
        let editor = StubEditor::default();
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            Some(&editor),
            Some(capability_entry("telegram", true, true, true)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_fact(TurnVisibilityFact::TaskPlanner);
        let streamed = delivery.close_before_canonical_reply();

        assert!(!streamed);
        assert!(!delivery.report().finalize_streamed);
        assert_eq!(
            editor
                .sends
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_slice(),
            ["正在规划当前任务"]
        );
        assert!(
            editor
                .edits
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty(),
            "raw final content must not be edited before ReplyFinalize"
        );
    }

    #[test]
    fn queued_delivery_suppresses_current_chat_tool_intents() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        let supplemental = delivery
            .deliver_tool_outbound_intent(&ToolOutboundIntent::text(
                ToolOutboundTarget::CurrentChat,
                ToolOutboundDeliveryKind::Supplemental,
                "补充说明",
            ))
            .expect("supplemental intent");
        let primary = delivery
            .deliver_tool_outbound_intent(&ToolOutboundIntent::text(
                ToolOutboundTarget::CurrentChat,
                ToolOutboundDeliveryKind::Primary,
                "主答复",
            ))
            .expect("suppressed primary intent");
        let explicit = delivery
            .deliver_tool_outbound_intent(&ToolOutboundIntent::text(
                ToolOutboundTarget::Explicit {
                    channel: "telegram".to_string(),
                    chat_id: "chat-2".to_string(),
                },
                ToolOutboundDeliveryKind::Supplemental,
                "不应再发送",
            ))
            .expect("explicit intent");

        assert_eq!(supplemental, ToolIntentDelivery::Suppressed);
        assert_eq!(primary, ToolIntentDelivery::Suppressed);
        assert_eq!(explicit, ToolIntentDelivery::VisibleUpdate);
        let outbound = outbound_rx.try_recv().expect("explicit outbound");
        assert_eq!(outbound.channel.as_ref(), "telegram");
        assert_eq!(outbound.chat_id.as_ref(), "chat-2");
        assert_eq!(outbound.content, "不应再发送");
        assert_eq!(outbound.outbound_kind, OutboundKind::Supplemental);
        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report().tool_outbound_intents_seen, 3);
        assert_eq!(delivery.report().tool_visible_updates_sent, 1);
        assert_eq!(delivery.report().tool_outbound_suppressed, 2);
        assert!(!delivery.report().current_primary_delivered);
    }

    #[test]
    fn queued_delivery_routes_explicit_tool_intent_through_runtime() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        let outcome = delivery
            .deliver_tool_outbound_intent(&ToolOutboundIntent {
                target: ToolOutboundTarget::Explicit {
                    channel: "telegram".to_string(),
                    chat_id: "chat-2".to_string(),
                },
                delivery_kind: ToolOutboundDeliveryKind::Primary,
                content: "显式主答复".to_string(),
                body: Some(CanonicalMessageBody::Text(TextBody {
                    text: "显式主答复".to_string(),
                    format: TextFormat::Markdown,
                })),
            })
            .expect("explicit intent");

        assert_eq!(outcome, ToolIntentDelivery::VisibleUpdate);
        let outbound = outbound_rx.try_recv().expect("outbound");
        assert_eq!(outbound.channel.as_ref(), "telegram");
        assert_eq!(outbound.chat_id.as_ref(), "chat-2");
        assert_eq!(outbound.content, "显式主答复");
        assert_eq!(outbound.outbound_kind, OutboundKind::Primary);
        assert!(matches!(
            outbound.body,
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Markdown,
                ..
            })
        ));
        assert_eq!(delivery.report().tool_outbound_intents_seen, 1);
        assert_eq!(delivery.report().tool_visible_updates_sent, 1);
        assert_eq!(delivery.report().explicit_outbound_sent, 1);
    }

    #[test]
    fn edit_delivery_finalize_reuses_edit_lane_after_header_projection() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, _outbound_rx, _) = new_inbound_channel(4);
        let msg = build_msg("telegram");
        let editor = StubEditor::default();
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            Some(&editor),
            Some(capability_entry("telegram", true, true, true)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_fact(TurnVisibilityFact::Acknowledged);
        assert!(delivery.finalize("主答复"));

        assert_eq!(
            editor
                .sends
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_slice(),
            ["已收到，正在处理"]
        );
        assert_eq!(
            editor
                .edits
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_slice(),
            ["主答复"]
        );
    }

    #[test]
    fn edit_delivery_placeholder_header_does_not_overwrite_visible_body() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, _outbound_rx, _) = new_inbound_channel(4);
        let msg = build_msg("telegram");
        let editor = StubEditor::default();
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            Some(&editor),
            Some(capability_entry("telegram", true, true, true)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_fact(TurnVisibilityFact::TaskPlanner);
        delivery.on_stream_delta("这是已经可见的正文");
        delivery.emit_fact(TurnVisibilityFact::RunningTool {
            tool: "board_info",
            index: 0,
            total: 1,
        });

        assert_eq!(
            editor
                .sends
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_slice(),
            ["正在规划当前任务"]
        );
        assert_eq!(
            editor
                .edits
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_slice(),
            ["这是已经可见的正文"]
        );
    }

    #[test]
    fn edit_delivery_finalize_treats_already_visible_final_as_delivered_after_edit_disable() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, _outbound_rx, _) = new_inbound_channel(4);
        let msg = build_msg("telegram");
        let editor = FailingEditEditor::default();
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            Some(&editor),
            Some(capability_entry("telegram", true, true, true)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_partial("这是已可见最终文本");
        delivery.emit_fact(TurnVisibilityFact::TaskPlanner);
        delivery.emit_fact(TurnVisibilityFact::RunningTool {
            tool: "board_info",
            index: 0,
            total: 1,
        });
        delivery.emit_fact(TurnVisibilityFact::Finalizing);

        let streamed = delivery.finalize("这是已可见最终文本");

        assert!(streamed);
        assert!(delivery.report().finalize_streamed);
        assert_eq!(
            editor
                .sends
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_slice(),
            ["这是已可见最终文本"]
        );
    }

    #[test]
    fn edit_delivery_terminal_header_stays_sticky_against_later_placeholder_fact() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, _outbound_rx, _) = new_inbound_channel(4);
        let msg = build_msg("telegram");
        let editor = StubEditor::default();
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            Some(&editor),
            Some(capability_entry("telegram", true, true, true)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_fact(TurnVisibilityFact::TaskTerminal {
            status: TaskTerminalVisibilityStatus::PartialComplete,
        });
        delivery.emit_fact(TurnVisibilityFact::Finalizing);

        assert_eq!(
            editor
                .sends
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_slice(),
            ["当前任务已部分完成"]
        );
        assert!(editor
            .edits
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty());
    }

    #[test]
    fn edit_delivery_terminal_header_stays_above_body_until_finalize() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, _outbound_rx, _) = new_inbound_channel(4);
        let msg = build_msg("telegram");
        let editor = StubEditor::default();
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            Some(&editor),
            Some(capability_entry("telegram", true, true, true)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_fact(TurnVisibilityFact::TaskTerminal {
            status: TaskTerminalVisibilityStatus::PartialComplete,
        });
        delivery.on_stream_delta("已取得部分结果");
        delivery.emit_fact(TurnVisibilityFact::Finalizing);
        assert!(delivery.finalize("最终答复"));

        assert_eq!(
            editor
                .sends
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_slice(),
            ["当前任务已部分完成"]
        );
        assert_eq!(
            editor
                .edits
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_slice(),
            ["当前任务已部分完成\n\n已取得部分结果", "最终答复"]
        );
    }

    #[test]
    fn queued_delivery_suppresses_structured_visibility_contracts_for_current_chat() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_fact(TurnVisibilityFact::TaskPlanner);
        delivery.emit_fact(TurnVisibilityFact::RunningTool {
            tool: "board_info",
            index: 0,
            total: 1,
        });
        delivery.emit_fact(TurnVisibilityFact::TaskStarted { resumed: false });
        delivery.emit_fact(TurnVisibilityFact::TaskTerminal {
            status: TaskTerminalVisibilityStatus::PartialComplete,
        });

        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report(), DeliveryReport::default());
    }

    #[test]
    fn queued_delivery_action_facts_remain_telemetry_only_without_append_only_visibility() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        delivery.emit_fact(TurnVisibilityFact::TaskStarted { resumed: true });
        delivery.emit_fact(TurnVisibilityFact::TaskTerminal {
            status: TaskTerminalVisibilityStatus::Blocked,
        });

        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report(), DeliveryReport::default());
    }
}
