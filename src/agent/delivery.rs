use crate::bus::{IngressKind, OutboundTx, PcMsg};
use crate::error::Result;
use crate::i18n::{tr, Locale as UiLocale, Message as UiMessage};
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
    pub waiting_notice_sent: bool,
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
    req_id: &'a str,
    lifecycle: DeliveryLifecycle,
    last_visible_text: String,
    report: DeliveryReport,
    shared: Arc<QueuedDeliveryShared>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeliveryLifecycle {
    Open,
    CurrentPrimaryDelivered,
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
    CurrentPrimary,
}

struct QueuedDeliveryShared {
    visible_updates_sent: AtomicU8,
    waiting_notice_canceled: AtomicBool,
    waiting_notice_sent: AtomicBool,
}

struct WaitingNoticeJob {
    due_at: Instant,
    outbound_tx: OutboundTx,
    channel: Arc<str>,
    chat_id: Arc<str>,
    req_id: String,
    waiting_notice: String,
    shared: Weak<QueuedDeliveryShared>,
}

impl<'a> DeliverySession<'a> {
    pub(crate) fn new(
        msg: &'a PcMsg,
        req_id: &'a str,
        outbound_tx: &'a OutboundTx,
        editor: Option<&'a (dyn StreamEditor + Send + Sync)>,
        loc: UiLocale,
    ) -> Self {
        let mode = if msg.ingress != IngressKind::User || msg.channel.as_ref() == "voice" {
            DeliveryMode::Silent
        } else if let Some(editor) = editor {
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
                req_id,
                lifecycle: DeliveryLifecycle::Open,
                last_visible_text: String::new(),
                report: DeliveryReport::default(),
                shared: spawn_waiting_notice(
                    outbound_tx.clone(),
                    Arc::clone(&msg.channel),
                    Arc::clone(&msg.chat_id),
                    req_id,
                    tr(UiMessage::AgentStillWorking, loc),
                ),
            })
        };
        Self {
            mode,
            outbound_tx,
            req_id,
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

    pub(crate) fn emit_progress(&mut self, content: &str) {
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

    pub(crate) fn emit_partial(&mut self, content: &str) {
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
            // Keep queued delivery runtime-controlled: waiting notice + tool progress + final answer.
            DeliveryMode::Queued(_) => {}
            DeliveryMode::Silent => {}
        }
    }

    /// 返回 true 表示最终答复已经直接交付到通道，外层应跳过 outbound_tx。
    pub(crate) fn finalize(&mut self, _final_content: &str) -> bool {
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => delivery.finalize(_final_content),
            DeliveryMode::Queued(ref mut delivery) => {
                delivery.cancel_waiting_notice();
                delivery.finalize()
            }
            DeliveryMode::Silent => false,
        }
    }

    /// 直接向当前聊天交付主答复，并进入“本轮已交付”状态，后续 progress/finalize 不再重复发。
    pub(crate) fn deliver_current_primary(&mut self, content: &str) -> Result<bool> {
        let text = normalize_visible_update(content, crate::bus::MAX_CONTENT_LEN);
        if text.is_empty() {
            return Ok(false);
        }
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => delivery.deliver_current_primary(&text),
            DeliveryMode::Queued(ref mut delivery) => delivery.deliver_current_primary(&text),
            DeliveryMode::Silent => Ok(false),
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
                    if self.deliver_current_primary(&text)? {
                        self.bump_tool_visible_update(false);
                        Ok(ToolIntentDelivery::CurrentPrimary)
                    } else {
                        self.bump_tool_intent_suppressed();
                        Ok(ToolIntentDelivery::Suppressed)
                    }
                }
                ToolOutboundDeliveryKind::Supplemental => match self.mode {
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
                },
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
            delivery.cancel_waiting_notice();
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
        if self.lifecycle == DeliveryLifecycle::CurrentPrimaryDelivered {
            return true;
        }
        if self.lifecycle == DeliveryLifecycle::Finalized {
            return self.report.finalize_streamed;
        }
        if self.message_id.is_none() {
            if final_content.trim().is_empty() {
                return false;
            }
            self.send_initial(final_content);
        } else if !final_content.trim().is_empty() {
            self.edit_existing(final_content);
        }
        let streamed = self.message_id.is_some() && !self.edit_disabled;
        self.lifecycle = DeliveryLifecycle::Finalized;
        self.report.finalize_streamed = streamed;
        streamed
    }

    fn deliver_current_primary(&mut self, content: &str) -> Result<bool> {
        if self.lifecycle == DeliveryLifecycle::CurrentPrimaryDelivered {
            return Ok(true);
        }
        if self.lifecycle == DeliveryLifecycle::Finalized {
            return Ok(false);
        }
        if self.message_id.is_none() {
            self.send_initial(content);
        } else {
            self.edit_existing(content);
        }
        if self.edit_disabled || self.last_visible_text != content {
            return Err(crate::error::Error::config(
                "current_chat_delivery",
                "failed to deliver current-chat primary reply via stream editor",
            ));
        }
        self.lifecycle = DeliveryLifecycle::CurrentPrimaryDelivered;
        self.report.current_primary_delivered = true;
        Ok(true)
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
            if self.edit_disabled {
                return Err(crate::error::Error::config(
                    "current_chat_delivery",
                    "failed to deliver current-chat supplemental update via stream editor",
                ));
            }
            return Ok(false);
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
    fn cancel_waiting_notice(&self) {
        self.shared
            .waiting_notice_canceled
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
        self.cancel_waiting_notice();
        if !self.try_claim_visible_slot() {
            return;
        }
        match send_visible_update(
            self.outbound_tx,
            self.channel,
            self.chat_id,
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

    fn deliver_current_primary(&mut self, content: &str) -> Result<bool> {
        if self.lifecycle == DeliveryLifecycle::CurrentPrimaryDelivered {
            return Ok(true);
        }
        if self.lifecycle == DeliveryLifecycle::Finalized {
            return Ok(false);
        }
        self.cancel_waiting_notice();
        send_visible_update(
            self.outbound_tx,
            self.channel,
            self.chat_id,
            self.req_id,
            content,
        )
        .map_err(|()| {
            crate::error::Error::config(
                "current_chat_delivery",
                "failed to enqueue current-chat primary reply",
            )
        })?;
        self.last_visible_text = content.to_string();
        self.lifecycle = DeliveryLifecycle::CurrentPrimaryDelivered;
        self.report.current_primary_delivered = true;
        Ok(true)
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
            DeliveryLifecycle::CurrentPrimaryDelivered => true,
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
        report.waiting_notice_sent = self.shared.waiting_notice_sent.load(Ordering::Relaxed);
        report
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
    req_id: &str,
    content: &str,
) -> std::result::Result<(), ()> {
    let msg = PcMsg {
        channel: Arc::clone(channel),
        chat_id: Arc::clone(chat_id),
        content: content.to_string(),
        req_id: Some(req_id.to_string()),
        ingress: IngressKind::User,
        enqueue_ts_ms: current_unix_ms(),
        is_group: false,
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

fn spawn_waiting_notice(
    outbound_tx: OutboundTx,
    channel: Arc<str>,
    chat_id: Arc<str>,
    req_id: &str,
    waiting_notice: String,
) -> Arc<QueuedDeliveryShared> {
    let shared = Arc::new(QueuedDeliveryShared {
        visible_updates_sent: AtomicU8::new(0),
        waiting_notice_canceled: AtomicBool::new(false),
        waiting_notice_sent: AtomicBool::new(false),
    });
    let job = WaitingNoticeJob {
        due_at: Instant::now() + waiting_notice_delay(),
        outbound_tx,
        channel,
        chat_id,
        req_id: req_id.to_string(),
        waiting_notice,
        shared: Arc::downgrade(&shared),
    };
    if !crate::runtime::schedule_delayed_task(
        job.due_at,
        Box::new(move || fire_waiting_notice_job(job)),
    ) {
        shared
            .waiting_notice_canceled
            .store(true, Ordering::Relaxed);
        log::warn!("[agent_delivery] waiting notice skipped: delayed task queue full");
    }
    shared
}

fn fire_waiting_notice_job(job: WaitingNoticeJob) {
    let Some(shared) = job.shared.upgrade() else {
        return;
    };
    if shared.waiting_notice_canceled.load(Ordering::Relaxed) {
        return;
    }
    if !try_claim_shared_visible_slot(&shared) {
        return;
    }
    if !should_send_waiting_notice_after_claim(&shared) {
        return;
    }
    if send_visible_update(
        &job.outbound_tx,
        &job.channel,
        &job.chat_id,
        &job.req_id,
        &job.waiting_notice,
    )
    .is_err()
    {
        shared.visible_updates_sent.fetch_sub(1, Ordering::Relaxed);
    } else {
        shared.waiting_notice_sent.store(true, Ordering::Relaxed);
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

fn should_send_waiting_notice_after_claim(shared: &QueuedDeliveryShared) -> bool {
    if shared.waiting_notice_canceled.load(Ordering::Relaxed) {
        shared.visible_updates_sent.fetch_sub(1, Ordering::Relaxed);
        return false;
    }
    true
}

#[cfg(test)]
fn waiting_notice_delay() -> std::time::Duration {
    Duration::from_millis(20)
}

#[cfg(not(test))]
fn waiting_notice_delay() -> std::time::Duration {
    Duration::from_millis(3000)
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::new_inbound_channel;
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

    fn build_msg(channel: &str) -> PcMsg {
        PcMsg::new_inbound(channel, "chat-1", "hello", false).expect("pcmsg")
    }

    fn reset_delayed_tasks() {
        crate::runtime::delayed_task::reset_delayed_tasks_for_tests();
    }

    fn delayed_task_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn queued_delivery_emits_distinct_updates_with_cap() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(&msg, "req-1", &outbound_tx, None, UiLocale::Zh);

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
        let mut delivery = DeliverySession::new(&msg, "req-1", &outbound_tx, None, UiLocale::Zh);

        delivery.emit_partial("## 第2步：检查文件系统结构");

        assert!(outbound_rx.try_recv().is_err());
    }

    #[test]
    fn edit_delivery_finalizes_without_outbound_message() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, _outbound_rx, _) = new_inbound_channel(4);
        let msg = build_msg("telegram");
        let editor = StubEditor::default();
        let mut delivery =
            DeliverySession::new(&msg, "req-1", &outbound_tx, Some(&editor), UiLocale::Zh);

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
    fn queued_delivery_primary_current_suppresses_followup_finalize() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(&msg, "req-1", &outbound_tx, None, UiLocale::Zh);

        let delivered = delivery
            .deliver_current_primary("主答复")
            .expect("primary current");
        assert!(delivered);
        assert!(delivery.finalize("最终答案"));
        assert_eq!(
            delivery.report(),
            DeliveryReport {
                current_primary_delivered: true,
                ..DeliveryReport::default()
            }
        );

        let first = outbound_rx.try_recv().expect("primary reply");
        assert_eq!(first.content, "主答复");
        assert!(outbound_rx.try_recv().is_err());
    }

    #[test]
    fn queued_delivery_accepts_current_supplemental_tool_intent() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(&msg, "req-1", &outbound_tx, None, UiLocale::Zh);

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
    fn queued_delivery_suppresses_tool_intents_after_primary_close() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(&msg, "req-1", &outbound_tx, None, UiLocale::Zh);

        let first = delivery
            .deliver_tool_outbound_intent(&ToolOutboundIntent {
                target: ToolOutboundTarget::CurrentChat,
                delivery_kind: ToolOutboundDeliveryKind::Primary,
                content: "主答复".to_string(),
            })
            .expect("primary intent");
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

        assert_eq!(first, ToolIntentDelivery::CurrentPrimary);
        assert_eq!(second, ToolIntentDelivery::Suppressed);
        let outbound = outbound_rx.try_recv().expect("primary reply");
        assert_eq!(outbound.content, "主答复");
        assert!(outbound_rx.try_recv().is_err());
        assert_eq!(delivery.report().tool_outbound_intents_seen, 2);
        assert_eq!(delivery.report().tool_visible_updates_sent, 1);
        assert_eq!(delivery.report().tool_outbound_suppressed, 1);
    }

    #[test]
    fn queued_delivery_routes_explicit_tool_intent_through_runtime() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(&msg, "req-1", &outbound_tx, None, UiLocale::Zh);

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
    fn edit_delivery_primary_current_reuses_edit_lane() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, _outbound_rx, _) = new_inbound_channel(4);
        let msg = build_msg("telegram");
        let editor = StubEditor::default();
        let mut delivery =
            DeliverySession::new(&msg, "req-1", &outbound_tx, Some(&editor), UiLocale::Zh);

        delivery.emit_progress("处理中");
        let delivered = delivery
            .deliver_current_primary("主答复")
            .expect("primary current");
        assert!(delivered);
        assert!(delivery.finalize("不应重复"));

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
    fn queued_delivery_sends_waiting_notice_for_long_think() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let delivery = DeliverySession::new(&msg, "req-1", &outbound_tx, None, UiLocale::Zh);

        std::thread::sleep(waiting_notice_delay() + std::time::Duration::from_millis(20));
        crate::runtime::service_delayed_tasks();

        let first = outbound_rx.try_recv().expect("waiting notice");
        assert_eq!(first.content, "还在处理，请稍等 ⏳");
        assert!(delivery.report().waiting_notice_sent);
    }

    #[test]
    fn queued_delivery_finalize_cancels_waiting_notice() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(&msg, "req-1", &outbound_tx, None, UiLocale::Zh);

        let streamed = delivery.finalize("最终答案");
        assert!(!streamed);
        std::thread::sleep(waiting_notice_delay() + std::time::Duration::from_millis(20));
        crate::runtime::service_delayed_tasks();

        assert!(outbound_rx.try_recv().is_err());
    }

    #[test]
    fn queued_delivery_drop_cancels_waiting_notice() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        {
            let _delivery = DeliverySession::new(&msg, "req-1", &outbound_tx, None, UiLocale::Zh);
        }

        std::thread::sleep(waiting_notice_delay() + std::time::Duration::from_millis(20));
        crate::runtime::service_delayed_tasks();

        assert!(outbound_rx.try_recv().is_err());
    }

    #[test]
    fn waiting_notice_rechecks_cancel_after_claim() {
        let _guard = delayed_task_test_lock();
        reset_delayed_tasks();
        let shared = Arc::new(QueuedDeliveryShared {
            visible_updates_sent: AtomicU8::new(0),
            waiting_notice_canceled: AtomicBool::new(false),
            waiting_notice_sent: AtomicBool::new(false),
        });

        assert!(try_claim_shared_visible_slot(&shared));
        shared
            .waiting_notice_canceled
            .store(true, Ordering::Relaxed);

        assert!(!should_send_waiting_notice_after_claim(&shared));
        assert_eq!(shared.visible_updates_sent.load(Ordering::Relaxed), 0);
    }
}
