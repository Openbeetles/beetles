use crate::bus::{IngressKind, OutboundTx, PcMsg};
use crate::error::Result;
use crate::i18n::Locale as UiLocale;
use crate::memory::MemorySystemKind;
use crate::metrics;
use crate::tools::{ToolOutboundDeliveryKind, ToolOutboundIntent, ToolOutboundTarget};
use crate::util::truncate_content_to_max;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

const EDIT_THROTTLE_MS: u64 = 500;
const MAX_EDIT_FAILURES: u8 = 3;
const MAX_QUEUED_VISIBLE_UPDATES: u8 = 4;
const MIN_PARTIAL_VISIBLE_CHARS: usize = 8;
const MAX_QUEUED_PROGRESS_CHARS: usize = 120;
const MAX_QUEUED_PARTIAL_CHARS: usize = 240;

#[cfg(test)]
const COMPACT_PRESENCE_PULSE_SCHEDULE_MS: [u64; 1] = [20];
#[cfg(test)]
const RICH_PRESENCE_PULSE_SCHEDULE_MS: [u64; 2] = [20, 60];
#[cfg(not(test))]
const COMPACT_PRESENCE_PULSE_SCHEDULE_MS: [u64; 1] = [3000];
#[cfg(not(test))]
const RICH_PRESENCE_PULSE_SCHEDULE_MS: [u64; 2] = [3000, 9000];

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
    pub presence_pulses_sent: u8,
    pub progress_updates_sent: u8,
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
    presence_contract: PresencePulseContract,
}

enum DeliveryMode<'a> {
    Silent,
    Edit(EditDelivery<'a>),
    Queued(QueuedDelivery<'a>),
}

struct EditDelivery<'a> {
    chat_id: &'a str,
    editor: &'a (dyn StreamEditor + Send + Sync),
    message_id: Option<String>,
    last_edit_at: std::time::Instant,
    edit_disabled: bool,
    edit_failures: u8,
    lifecycle: DeliveryLifecycle,
    last_visible_text: String,
    report: DeliveryReport,
}

struct QueuedDelivery<'a> {
    outbound_tx: &'a OutboundTx,
    channel: &'a std::sync::Arc<str>,
    chat_id: &'a std::sync::Arc<str>,
    is_group: bool,
    req_id: &'a str,
    lifecycle: DeliveryLifecycle,
    last_visible_text: String,
    report: DeliveryReport,
    shared: Arc<QueuedDeliveryShared>,
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

struct QueuedDeliveryShared {
    visible_updates_sent: AtomicU8,
    presence_pulses_canceled: AtomicBool,
    presence_pulses_sent: AtomicU8,
}

struct PresencePulseJob {
    stage_idx: u8,
    due_at: Instant,
    outbound_tx: OutboundTx,
    channel: Arc<str>,
    chat_id: Arc<str>,
    is_group: bool,
    req_id: String,
    content: String,
    shared: Weak<QueuedDeliveryShared>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PresencePulseProfile {
    Compact,
    Rich,
}

#[derive(Clone, Copy, Debug)]
struct PresencePulseContract {
    loc: UiLocale,
    profile: PresencePulseProfile,
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
        memory_system_kind: MemorySystemKind,
        loc: UiLocale,
    ) -> Self {
        let presence_contract = PresencePulseContract::new(memory_system_kind, loc);
        let policy = channel_capability
            .filter(|entry| entry.enabled && msg.ingress == IngressKind::User)
            .map(|entry| DeliveryPolicy {
                supports_current_primary: entry.contract.supports_primary_reply,
                supports_current_supplemental: entry.contract.supports_supplemental_reply,
            })
            .unwrap_or_default();
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
                last_visible_text: String::new(),
                report: DeliveryReport::default(),
            })
        } else {
            DeliveryMode::Queued(QueuedDelivery {
                outbound_tx,
                channel: &msg.channel,
                chat_id: &msg.chat_id,
                is_group: msg.is_group,
                req_id,
                lifecycle: DeliveryLifecycle::Open,
                last_visible_text: String::new(),
                report: DeliveryReport::default(),
                shared: if policy.supports_current_supplemental {
                    spawn_presence_pulses(
                        outbound_tx.clone(),
                        Arc::clone(&msg.channel),
                        Arc::clone(&msg.chat_id),
                        msg.is_group,
                        req_id,
                        presence_contract,
                    )
                } else {
                    Arc::new(QueuedDeliveryShared {
                        visible_updates_sent: AtomicU8::new(0),
                        presence_pulses_canceled: AtomicBool::new(true),
                        presence_pulses_sent: AtomicU8::new(0),
                    })
                },
            })
        };
        Self {
            mode,
            outbound_tx,
            req_id,
            policy,
            presence_contract,
        }
    }

    pub(crate) fn report(&self) -> DeliveryReport {
        match self.mode {
            DeliveryMode::Silent => DeliveryReport::default(),
            DeliveryMode::Edit(ref delivery) => delivery.report,
            DeliveryMode::Queued(ref delivery) => delivery.report(),
        }
    }

    pub(crate) fn on_stream_delta(&mut self, accumulated: &str) {
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => delivery.on_stream_delta(accumulated),
            DeliveryMode::Silent | DeliveryMode::Queued(_) => {}
        }
    }

    #[cfg(test)]
    pub(crate) fn emit_progress(&mut self, content: &str) {
        if !self.policy.supports_current_supplemental {
            return;
        }
        let text = normalize_visible_update(content, MAX_QUEUED_PROGRESS_CHARS);
        if text.is_empty() {
            return;
        }
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => {
                delivery.force_visible_update(&text, true, false)
            }
            DeliveryMode::Queued(ref mut delivery) => delivery.emit(&text, true, false),
            DeliveryMode::Silent => {}
        }
    }

    pub(crate) fn emit_tool_progress(&mut self, name: &str, index: usize, total: usize) {
        if !self.policy.supports_current_supplemental {
            return;
        }
        let text = normalize_visible_update(
            &self.presence_contract.tool_progress(name, index, total),
            MAX_QUEUED_PROGRESS_CHARS,
        );
        if text.is_empty() {
            return;
        }
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => {
                delivery.force_visible_update(&text, true, false)
            }
            DeliveryMode::Queued(ref mut delivery) => delivery.emit(&text, true, false),
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
            DeliveryMode::Edit(ref mut delivery) => {
                delivery.force_visible_update(&text, false, true)
            }
            // Non-edit channels cannot revise previously sent text, so exposing ToolUse-time
            // assistant drafts here tends to leak unfinished step plans to the user.
            // Keep queued delivery runtime-controlled: presence pulses + tool progress + final answer.
            DeliveryMode::Queued(_) => {}
            DeliveryMode::Silent => {}
        }
    }

    /// 返回 true 表示最终答复已经直接交付到通道，外层应跳过 outbound_tx。
    pub(crate) fn finalize(&mut self, _final_content: &str) -> bool {
        if !self.policy.supports_current_primary {
            if let DeliveryMode::Queued(ref mut delivery) = self.mode {
                delivery.cancel_presence_pulses();
                delivery.finalize();
            }
            return false;
        }
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => delivery.finalize(_final_content),
            DeliveryMode::Queued(ref mut delivery) => {
                delivery.cancel_presence_pulses();
                delivery.finalize()
            }
            DeliveryMode::Silent => false,
        }
    }

    pub(crate) fn deliver_tool_outbound_intent(
        &mut self,
        intent: &ToolOutboundIntent,
    ) -> Result<ToolIntentDelivery> {
        self.bump_tool_intent_seen();
        let text = normalize_visible_update(&intent.content, crate::bus::MAX_CONTENT_LEN);
        if text.is_empty() {
            self.bump_tool_intent_suppressed();
            return Ok(ToolIntentDelivery::Suppressed);
        }
        match &intent.target {
            ToolOutboundTarget::CurrentChat => match intent.delivery_kind {
                ToolOutboundDeliveryKind::Primary => {
                    self.bump_tool_intent_suppressed();
                    Ok(ToolIntentDelivery::Suppressed)
                }
                ToolOutboundDeliveryKind::Supplemental => {
                    if !self.policy.supports_current_supplemental {
                        self.bump_tool_intent_suppressed();
                        return Ok(ToolIntentDelivery::Suppressed);
                    }
                    match self.mode {
                        DeliveryMode::Edit(ref mut delivery) => {
                            if delivery.deliver_current_supplemental(&text)? {
                                self.bump_tool_visible_update(false);
                                Ok(ToolIntentDelivery::VisibleUpdate)
                            } else {
                                self.bump_tool_intent_suppressed();
                                Ok(ToolIntentDelivery::Suppressed)
                            }
                        }
                        DeliveryMode::Queued(ref mut delivery) => {
                            if delivery.deliver_current_supplemental(&text) {
                                self.bump_tool_visible_update(false);
                                Ok(ToolIntentDelivery::VisibleUpdate)
                            } else {
                                self.bump_tool_intent_suppressed();
                                Ok(ToolIntentDelivery::Suppressed)
                            }
                        }
                        DeliveryMode::Silent => {
                            self.bump_tool_intent_suppressed();
                            Ok(ToolIntentDelivery::Suppressed)
                        }
                    }
                }
            },
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
    fn drop(&mut self) {
        if let DeliveryMode::Queued(ref delivery) = self.mode {
            delivery.cancel_presence_pulses();
        }
    }
}

impl<'a> EditDelivery<'a> {
    fn on_stream_delta(&mut self, accumulated: &str) {
        if self.edit_disabled || self.lifecycle.is_closed() || accumulated.trim().is_empty() {
            return;
        }
        if self.message_id.is_none() {
            self.send_initial(accumulated);
            return;
        }
        if self.last_edit_at.elapsed() < std::time::Duration::from_millis(EDIT_THROTTLE_MS) {
            return;
        }
        self.edit_existing(accumulated);
    }

    fn force_visible_update(&mut self, content: &str, is_progress: bool, is_partial: bool) {
        if self.edit_disabled || self.lifecycle.is_closed() {
            return;
        }
        if is_progress {
            self.report.progress_updates_sent = self.report.progress_updates_sent.saturating_add(1);
        }
        if is_partial {
            self.report.partial_updates_sent = self.report.partial_updates_sent.saturating_add(1);
        }
        if self.message_id.is_none() {
            self.send_initial(content);
        } else {
            self.edit_existing(content);
        }
    }

    fn finalize(&mut self, final_content: &str) -> bool {
        if self.lifecycle == DeliveryLifecycle::Finalized {
            return self.report.finalize_streamed;
        }
        let normalized = normalize_visible_update(final_content, crate::bus::MAX_CONTENT_LEN);
        if self.message_id.is_none() {
            if normalized.is_empty() {
                return false;
            }
            self.send_initial(&normalized);
        } else if !normalized.is_empty() {
            self.edit_existing(&normalized);
        }
        let streamed = self.message_id.is_some()
            && (!self.edit_disabled
                || (!normalized.is_empty() && self.last_visible_text == normalized));
        self.lifecycle = DeliveryLifecycle::Finalized;
        self.report.finalize_streamed = streamed;
        streamed
    }

    fn deliver_current_supplemental(&mut self, content: &str) -> Result<bool> {
        if self.lifecycle.is_closed() {
            return Ok(false);
        }
        let before = self.last_visible_text.clone();
        if self.message_id.is_none() {
            self.send_initial(content);
        } else {
            self.edit_existing(content);
        }
        if self.last_visible_text == before {
            return Ok(false);
        }
        if self.last_visible_text != content {
            return Err(crate::error::Error::config(
                "current_chat_delivery",
                "failed to deliver current-chat supplemental update via stream editor",
            ));
        }
        Ok(true)
    }

    fn send_initial(&mut self, content: &str) {
        let normalized = normalize_visible_update(content, crate::bus::MAX_CONTENT_LEN);
        if normalized.is_empty() {
            return;
        }
        match self.editor.send_initial(self.chat_id, &normalized) {
            Ok(Some(message_id)) => {
                self.message_id = Some(message_id);
                self.last_visible_text = normalized;
                self.last_edit_at = std::time::Instant::now();
                self.edit_failures = 0;
                self.report.visible_text_updates_sent =
                    self.report.visible_text_updates_sent.saturating_add(1);
            }
            Ok(None) => {}
            Err(e) => {
                log::warn!(
                    "[agent_delivery] send_initial failed, disabling edit delivery: {}",
                    e
                );
                self.edit_disabled = true;
            }
        }
    }

    fn edit_existing(&mut self, content: &str) {
        let normalized = normalize_visible_update(content, crate::bus::MAX_CONTENT_LEN);
        if normalized.is_empty() || normalized == self.last_visible_text {
            return;
        }
        let Some(ref message_id) = self.message_id else {
            return;
        };
        match self.editor.edit(self.chat_id, message_id, &normalized) {
            Ok(()) => {
                self.last_visible_text = normalized;
                self.last_edit_at = std::time::Instant::now();
                self.edit_failures = 0;
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
            }
        }
    }
}

impl<'a> QueuedDelivery<'a> {
    fn cancel_presence_pulses(&self) {
        self.shared
            .presence_pulses_canceled
            .store(true, Ordering::Relaxed);
    }

    fn emit(&mut self, content: &str, is_progress: bool, is_partial: bool) {
        if self.lifecycle.is_closed() {
            return;
        }
        let normalized = normalize_visible_update(content, crate::bus::MAX_CONTENT_LEN);
        if normalized.is_empty() || normalized == self.last_visible_text {
            return;
        }
        if is_progress {
            self.report.progress_updates_sent = self.report.progress_updates_sent.saturating_add(1);
        }
        if is_partial {
            self.report.partial_updates_sent = self.report.partial_updates_sent.saturating_add(1);
        }
        if !self.try_claim_visible_slot() {
            return;
        }
        match send_visible_update(
            self.outbound_tx,
            self.channel,
            self.chat_id,
            self.is_group,
            self.req_id,
            &normalized,
        ) {
            Ok(()) => {
                self.last_visible_text = normalized;
                self.report.visible_text_updates_sent =
                    self.report.visible_text_updates_sent.saturating_add(1);
            }
            Err(()) => {
                self.shared
                    .visible_updates_sent
                    .fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    fn deliver_current_supplemental(&mut self, content: &str) -> bool {
        if self.lifecycle.is_closed() {
            return false;
        }
        let before = self.last_visible_text.clone();
        self.emit(content, false, false);
        self.last_visible_text != before
    }

    fn finalize(&mut self) -> bool {
        match self.lifecycle {
            DeliveryLifecycle::Finalized => false,
            DeliveryLifecycle::Open => {
                self.lifecycle = DeliveryLifecycle::Finalized;
                false
            }
        }
    }

    fn try_claim_visible_slot(&self) -> bool {
        loop {
            let current = self.shared.visible_updates_sent.load(Ordering::Relaxed);
            if current >= MAX_QUEUED_VISIBLE_UPDATES {
                return false;
            }
            if self
                .shared
                .visible_updates_sent
                .compare_exchange(
                    current,
                    current.saturating_add(1),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return true;
            }
        }
    }

    fn report(&self) -> DeliveryReport {
        let mut report = self.report;
        report.presence_pulses_sent = self.shared.presence_pulses_sent.load(Ordering::Relaxed);
        report
    }
}

impl PresencePulseProfile {
    fn for_memory_system_kind(memory_system_kind: MemorySystemKind) -> Self {
        match memory_system_kind {
            MemorySystemKind::EspCompact => Self::Compact,
            MemorySystemKind::LinuxFull => Self::Rich,
        }
    }
}

impl PresencePulseContract {
    fn new(memory_system_kind: MemorySystemKind, loc: UiLocale) -> Self {
        Self {
            loc,
            profile: PresencePulseProfile::for_memory_system_kind(memory_system_kind),
        }
    }

    fn scheduled_text(self, stage_idx: u8) -> Option<String> {
        match (self.loc, self.profile, stage_idx) {
            (UiLocale::Zh, PresencePulseProfile::Rich, 0) => {
                Some("〔甲壳虫〕已接到，继续处理中 🪲".to_string())
            }
            (UiLocale::Zh, PresencePulseProfile::Rich, 1) => {
                Some("〔甲壳虫〕这轮还在整理，结果马上接上 (｀･ω･´)ゞ".to_string())
            }
            (UiLocale::Zh, PresencePulseProfile::Compact, 0) => {
                Some("〔甲壳虫〕继续处理中，马上接上 🪲".to_string())
            }
            (UiLocale::En, PresencePulseProfile::Rich, 0) => {
                Some("[Beetle] Turn received, still working 🪲".to_string())
            }
            (UiLocale::En, PresencePulseProfile::Rich, 1) => {
                Some("[Beetle] Still organizing this turn, reply coming up (｀･ω･´)ゞ".to_string())
            }
            (UiLocale::En, PresencePulseProfile::Compact, 0) => {
                Some("[Beetle] Still working, reply coming up 🪲".to_string())
            }
            _ => None,
        }
    }

    fn tool_progress(self, name: &str, index: usize, total: usize) -> String {
        match self.loc {
            UiLocale::Zh if total > 1 => {
                format!(
                    "〔甲壳虫〕正在执行 {}（{}/{}），继续推进 🪲",
                    name,
                    index + 1,
                    total
                )
            }
            UiLocale::Zh => format!("〔甲壳虫〕正在执行 {}，继续推进 🪲", name),
            UiLocale::En if total > 1 => {
                format!(
                    "[Beetle] Running {} ({}/{}), still moving 🪲",
                    name,
                    index + 1,
                    total
                )
            }
            UiLocale::En => format!("[Beetle] Running {}, still moving 🪲", name),
        }
    }
}

fn normalize_visible_update(content: &str, max_chars: usize) -> String {
    truncate_content_to_max(content.trim(), max_chars)
        .trim()
        .to_string()
}

fn send_visible_update(
    outbound_tx: &OutboundTx,
    channel: &std::sync::Arc<str>,
    chat_id: &std::sync::Arc<str>,
    is_group: bool,
    req_id: &str,
    content: &str,
) -> std::result::Result<(), ()> {
    let msg = match PcMsg::new_outbound_for_chat(
        channel,
        chat_id,
        content,
        Some(req_id.to_string()),
        is_group,
    ) {
        Ok(msg) => msg,
        Err(error) => {
            log::error!(
                "[agent_delivery] visible update rejected channel={} chat_id={}: {}",
                channel,
                chat_id,
                error
            );
            return Err(());
        }
    };
    match outbound_tx.try_send(msg) {
        Ok(()) => {
            metrics::record_message_out();
            Ok(())
        }
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            metrics::record_outbound_enqueue_fail();
            log::warn!(
                "[agent_delivery] visible update dropped: outbound queue full channel={} chat_id={}",
                channel,
                chat_id
            );
            Err(())
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            metrics::record_outbound_enqueue_fail();
            log::error!(
                "[agent_delivery] visible update dropped: outbound disconnected channel={} chat_id={}",
                channel,
                chat_id
            );
            Err(())
        }
    }
}

fn send_visible_update_explicit(
    outbound_tx: &OutboundTx,
    channel: &str,
    chat_id: &str,
    req_id: &str,
    content: &str,
) -> std::result::Result<(), ()> {
    let mut msg = match PcMsg::new(channel, chat_id, content) {
        Ok(msg) => msg,
        Err(error) => {
            log::warn!(
                "[agent_delivery] explicit visible update rejected channel={} chat_id={}: {}",
                channel,
                chat_id,
                error
            );
            return Err(());
        }
    };
    msg.req_id = Some(req_id.to_string());
    match outbound_tx.try_send(msg) {
        Ok(()) => {
            metrics::record_message_out();
            Ok(())
        }
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            metrics::record_outbound_enqueue_fail();
            log::warn!(
                "[agent_delivery] explicit visible update dropped: outbound queue full channel={} chat_id={}",
                channel,
                chat_id
            );
            Err(())
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            metrics::record_outbound_enqueue_fail();
            log::error!(
                "[agent_delivery] explicit visible update dropped: outbound disconnected channel={} chat_id={}",
                channel,
                chat_id
            );
            Err(())
        }
    }
}

fn spawn_presence_pulses(
    outbound_tx: OutboundTx,
    channel: Arc<str>,
    chat_id: Arc<str>,
    is_group: bool,
    req_id: &str,
    presence_contract: PresencePulseContract,
) -> Arc<QueuedDeliveryShared> {
    let shared = Arc::new(QueuedDeliveryShared {
        visible_updates_sent: AtomicU8::new(0),
        presence_pulses_canceled: AtomicBool::new(false),
        presence_pulses_sent: AtomicU8::new(0),
    });
    for (stage_idx, delay_ms) in presence_pulse_schedule_ms(presence_contract.profile)
        .iter()
        .copied()
        .enumerate()
    {
        let Some(content) = presence_contract.scheduled_text(stage_idx as u8) else {
            continue;
        };
        let job = PresencePulseJob {
            stage_idx: stage_idx as u8,
            due_at: Instant::now() + Duration::from_millis(delay_ms),
            outbound_tx: outbound_tx.clone(),
            channel: Arc::clone(&channel),
            chat_id: Arc::clone(&chat_id),
            is_group,
            req_id: req_id.to_string(),
            content,
            shared: Arc::downgrade(&shared),
        };
        if !crate::runtime::schedule_delayed_task(
            job.due_at,
            Box::new(move || fire_presence_pulse_job(job)),
        ) {
            log::warn!(
                "[agent_delivery] presence pulse skipped stage={} queue full",
                stage_idx
            );
            break;
        }
    }
    shared
}

fn fire_presence_pulse_job(job: PresencePulseJob) {
    let Some(shared) = job.shared.upgrade() else {
        return;
    };
    if shared.presence_pulses_canceled.load(Ordering::Relaxed) {
        return;
    }
    if job.stage_idx == 0 && shared.visible_updates_sent.load(Ordering::Relaxed) > 0 {
        return;
    }
    if !try_claim_shared_visible_slot(&shared) {
        return;
    }
    if !should_send_presence_pulse_after_claim(&shared) {
        return;
    }
    if send_visible_update(
        &job.outbound_tx,
        &job.channel,
        &job.chat_id,
        job.is_group,
        &job.req_id,
        &job.content,
    )
    .is_err()
    {
        shared.visible_updates_sent.fetch_sub(1, Ordering::Relaxed);
    } else {
        shared.presence_pulses_sent.fetch_add(1, Ordering::Relaxed);
    }
}

fn try_claim_shared_visible_slot(shared: &QueuedDeliveryShared) -> bool {
    loop {
        let current = shared.visible_updates_sent.load(Ordering::Relaxed);
        if current >= MAX_QUEUED_VISIBLE_UPDATES {
            return false;
        }
        if shared
            .visible_updates_sent
            .compare_exchange(
                current,
                current.saturating_add(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            return true;
        }
    }
}

fn should_send_presence_pulse_after_claim(shared: &QueuedDeliveryShared) -> bool {
    if shared.presence_pulses_canceled.load(Ordering::Relaxed) {
        shared.visible_updates_sent.fetch_sub(1, Ordering::Relaxed);
        return false;
    }
    true
}

#[cfg(test)]
fn presence_pulse_initial_delay() -> std::time::Duration {
    Duration::from_millis(RICH_PRESENCE_PULSE_SCHEDULE_MS[0])
}

#[cfg(test)]
fn presence_pulse_followup_delay() -> std::time::Duration {
    Duration::from_millis(RICH_PRESENCE_PULSE_SCHEDULE_MS[1])
}

fn presence_pulse_schedule_ms(profile: PresencePulseProfile) -> &'static [u64] {
    match profile {
        PresencePulseProfile::Compact => &COMPACT_PRESENCE_PULSE_SCHEDULE_MS,
        PresencePulseProfile::Rich => &RICH_PRESENCE_PULSE_SCHEDULE_MS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::new_inbound_channel;
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
                max_text_bytes: 4096,
                delivery_ordering_model: if supports_stream_edit {
                    ChannelDeliveryOrderingModel::EditableSingleMessage
                } else {
                    ChannelDeliveryOrderingModel::AppendOnly
                },
            },
        }
    }

    fn reset_delayed_tasks() {
        crate::runtime::delayed_task::reset_delayed_tasks_for_tests();
    }

    fn delayed_task_test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::runtime::delayed_task::delayed_task_test_guard()
    }

    fn service_delayed_tasks_in_normal_mode() {
        crate::state::set_voice_exclusive_active(false);
        crate::state::set_background_maintenance_active(false);
        crate::state::set_config_plane_active(false);
        crate::state::set_boot_phase_active(false);
        crate::state::set_pairing_state_known(false);
        crate::state::set_pairing_required(false);
        crate::state::set_recovery_safe_mode_active(false);
        crate::runtime::service_delayed_tasks();
    }

    #[test]
    fn queued_delivery_emits_distinct_updates_with_cap() {
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

        delivery.emit_progress("第一步");
        delivery.emit_progress("第一步");
        delivery.emit_partial("第二步：继续处理中");
        delivery.emit_progress("第三步");
        delivery.emit_progress("第四步");
        delivery.emit_progress("第五步");

        let mut contents = Vec::new();
        while let Ok(msg) = outbound_rx.try_recv() {
            contents.push(msg.content);
        }
        assert_eq!(contents, vec!["第一步", "第三步", "第四步", "第五步"]);
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
    fn queued_delivery_without_supplemental_contract_suppresses_progress_and_presence_pulse() {
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

        delivery.emit_progress("处理中");
        std::thread::sleep(presence_pulse_initial_delay() + std::time::Duration::from_millis(20));
        service_delayed_tasks_in_normal_mode();

        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report().presence_pulses_sent, 0);
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

        delivery.emit_progress("正在执行 tools");
        let streamed = delivery.finalize("最终答案");

        assert!(streamed);
        assert_eq!(
            delivery.report(),
            DeliveryReport {
                progress_updates_sent: 1,
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
            ["正在执行 tools"]
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
    fn queued_delivery_accepts_current_supplemental_tool_intent() {
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
                target: ToolOutboundTarget::CurrentChat,
                delivery_kind: ToolOutboundDeliveryKind::Supplemental,
                content: "补充说明".to_string(),
            })
            .expect("supplemental intent");

        assert_eq!(outcome, ToolIntentDelivery::VisibleUpdate);
        let outbound = outbound_rx.try_recv().expect("outbound");
        assert_eq!(outbound.content, "补充说明");
        assert_eq!(delivery.report().tool_outbound_intents_seen, 1);
        assert_eq!(delivery.report().tool_visible_updates_sent, 1);
        assert_eq!(delivery.report().tool_outbound_suppressed, 0);
    }

    #[test]
    fn queued_delivery_suppresses_current_primary_tool_intent() {
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

        let first = delivery
            .deliver_tool_outbound_intent(&ToolOutboundIntent {
                target: ToolOutboundTarget::CurrentChat,
                delivery_kind: ToolOutboundDeliveryKind::Primary,
                content: "主答复".to_string(),
            })
            .expect("suppressed primary intent");
        let second = delivery
            .deliver_tool_outbound_intent(&ToolOutboundIntent {
                target: ToolOutboundTarget::Explicit {
                    channel: "telegram".to_string(),
                    chat_id: "chat-2".to_string(),
                },
                delivery_kind: ToolOutboundDeliveryKind::Supplemental,
                content: "不应再发送".to_string(),
            })
            .expect("suppressed explicit intent");

        assert_eq!(first, ToolIntentDelivery::Suppressed);
        assert_eq!(second, ToolIntentDelivery::VisibleUpdate);
        let outbound = outbound_rx.try_recv().expect("explicit outbound");
        assert_eq!(outbound.channel.as_ref(), "telegram");
        assert_eq!(outbound.chat_id.as_ref(), "chat-2");
        assert_eq!(outbound.content, "不应再发送");
        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report().tool_outbound_intents_seen, 2);
        assert_eq!(delivery.report().tool_visible_updates_sent, 1);
        assert_eq!(delivery.report().tool_outbound_suppressed, 1);
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
                delivery_kind: ToolOutboundDeliveryKind::Supplemental,
                content: "显式外发".to_string(),
            })
            .expect("explicit intent");

        assert_eq!(outcome, ToolIntentDelivery::VisibleUpdate);
        let outbound = outbound_rx.try_recv().expect("outbound");
        assert_eq!(outbound.channel.as_ref(), "telegram");
        assert_eq!(outbound.chat_id.as_ref(), "chat-2");
        assert_eq!(outbound.content, "显式外发");
        assert_eq!(delivery.report().tool_outbound_intents_seen, 1);
        assert_eq!(delivery.report().tool_visible_updates_sent, 1);
        assert_eq!(delivery.report().explicit_outbound_sent, 1);
    }

    #[test]
    fn edit_delivery_finalize_reuses_edit_lane_after_progress() {
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

        delivery.emit_progress("处理中");
        assert!(delivery.finalize("主答复"));

        assert_eq!(
            editor
                .sends
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_slice(),
            ["处理中"]
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

        delivery.emit_progress("已可见最终文本");
        delivery.emit_progress("第一次失败");
        delivery.emit_progress("第二次失败");
        delivery.emit_progress("第三次失败");

        let streamed = delivery.finalize("已可见最终文本");

        assert!(streamed);
        assert!(delivery.report().finalize_streamed);
        assert_eq!(
            editor
                .sends
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_slice(),
            ["已可见最终文本"]
        );
    }

    #[test]
    fn queued_delivery_sends_staged_presence_pulses_for_long_turn() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg_with_group("qq_channel", true);
        let delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::LinuxFull,
            UiLocale::Zh,
        );

        std::thread::sleep(presence_pulse_initial_delay() + std::time::Duration::from_millis(20));
        service_delayed_tasks_in_normal_mode();
        std::thread::sleep(presence_pulse_followup_delay() + std::time::Duration::from_millis(20));
        service_delayed_tasks_in_normal_mode();

        let first = outbound_rx.try_recv().expect("first pulse");
        let second = outbound_rx.try_recv().expect("second pulse");
        assert_eq!(first.content, "〔甲壳虫〕已接到，继续处理中 🪲");
        assert_eq!(
            second.content,
            "〔甲壳虫〕这轮还在整理，结果马上接上 (｀･ω･´)ゞ"
        );
        assert!(first.is_group);
        assert!(second.is_group);
        assert_eq!(delivery.report().presence_pulses_sent, 2);
    }

    #[test]
    fn queued_delivery_tool_progress_uses_presence_copy() {
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

        delivery.emit_tool_progress("board_info", 0, 1);

        let outbound = outbound_rx.try_recv().expect("tool pulse");
        assert_eq!(
            outbound.content,
            "〔甲壳虫〕正在执行 board_info，继续推进 🪲"
        );
    }

    #[test]
    fn compact_profile_sends_single_presence_pulse() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let delivery = DeliverySession::new(
            &msg,
            "req-1",
            &outbound_tx,
            None,
            Some(capability_entry("qq_channel", true, true, false)),
            MemorySystemKind::EspCompact,
            UiLocale::Zh,
        );

        std::thread::sleep(presence_pulse_initial_delay() + std::time::Duration::from_millis(20));
        service_delayed_tasks_in_normal_mode();
        std::thread::sleep(presence_pulse_followup_delay() + std::time::Duration::from_millis(20));
        service_delayed_tasks_in_normal_mode();

        let first = outbound_rx.try_recv().expect("compact pulse");
        assert_eq!(first.content, "〔甲壳虫〕继续处理中，马上接上 🪲");
        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report().presence_pulses_sent, 1);
    }

    #[test]
    fn tool_progress_suppresses_initial_presence_pulse_but_keeps_followup() {
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

        delivery.emit_tool_progress("board_info", 0, 1);
        std::thread::sleep(presence_pulse_initial_delay() + std::time::Duration::from_millis(20));
        service_delayed_tasks_in_normal_mode();
        std::thread::sleep(presence_pulse_followup_delay() + std::time::Duration::from_millis(20));
        service_delayed_tasks_in_normal_mode();

        let first = outbound_rx.try_recv().expect("tool progress");
        let second = outbound_rx.try_recv().expect("followup pulse");
        assert_eq!(first.content, "〔甲壳虫〕正在执行 board_info，继续推进 🪲");
        assert_eq!(
            second.content,
            "〔甲壳虫〕这轮还在整理，结果马上接上 (｀･ω･´)ゞ"
        );
        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report().presence_pulses_sent, 1);
    }

    #[test]
    fn queued_delivery_finalize_cancels_presence_pulses() {
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

        let streamed = delivery.finalize("最终答案");
        assert!(!streamed);
        std::thread::sleep(presence_pulse_initial_delay() + std::time::Duration::from_millis(20));
        service_delayed_tasks_in_normal_mode();

        assert!(outbound_rx.try_recv().is_err());
    }

    #[test]
    fn queued_delivery_drop_cancels_presence_pulses() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        {
            let _delivery = DeliverySession::new(
                &msg,
                "req-1",
                &outbound_tx,
                None,
                Some(capability_entry("qq_channel", true, true, false)),
                MemorySystemKind::LinuxFull,
                UiLocale::Zh,
            );
        }

        std::thread::sleep(presence_pulse_initial_delay() + std::time::Duration::from_millis(20));
        service_delayed_tasks_in_normal_mode();

        assert!(outbound_rx.try_recv().is_err());
    }

    #[test]
    fn presence_pulse_rechecks_cancel_after_claim() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let shared = Arc::new(QueuedDeliveryShared {
            visible_updates_sent: AtomicU8::new(0),
            presence_pulses_canceled: AtomicBool::new(false),
            presence_pulses_sent: AtomicU8::new(0),
        });

        assert!(try_claim_shared_visible_slot(&shared));
        shared
            .presence_pulses_canceled
            .store(true, Ordering::Relaxed);

        assert!(!should_send_presence_pulse_after_claim(&shared));
        assert_eq!(shared.visible_updates_sent.load(Ordering::Relaxed), 0);
    }
}
