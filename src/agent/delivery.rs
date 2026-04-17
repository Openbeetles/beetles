use crate::bus::{IngressKind, OutboundTx, PcMsg};
use crate::error::Result;
use crate::i18n::Locale as UiLocale;
use crate::memory::MemorySystemKind;
use crate::metrics;
use crate::tools::{ToolOutboundIntent, ToolOutboundTarget};
use crate::util::truncate_content_to_max;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

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
    pub presence_pulses_sent: u8,
    pub progress_updates_sent: u8,
    pub planner_progress_updates_sent: u8,
    pub tool_progress_updates_sent: u8,
    pub action_progress_updates_sent: u8,
    pub terminal_progress_updates_sent: u8,
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
}

#[derive(Clone, Copy, Debug)]
struct VisibleUpdateContract {
    loc: UiLocale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VisibleUpdateKind {
    PlannerProgress,
    ToolProgress,
    ActionProgress,
    TerminalProgress,
    PartialDraft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskActionProgressKind {
    Started,
    Resumed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskTerminalProgressKind {
    Completed,
    PartialComplete,
    Blocked,
    Aborted,
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
                shared: Arc::new(QueuedDeliveryShared {
                    visible_updates_sent: AtomicU8::new(0),
                }),
            })
        };
        Self {
            mode,
            outbound_tx,
            req_id,
            policy,
            visible_update_contract,
        }
    }

    pub(crate) fn report(&self) -> DeliveryReport {
        match self.mode {
            DeliveryMode::Silent => DeliveryReport::default(),
            DeliveryMode::Edit(ref delivery) => delivery.report,
            DeliveryMode::Queued(ref delivery) => delivery.report,
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
                delivery.force_visible_update(&text, VisibleUpdateKind::PlannerProgress)
            }
            DeliveryMode::Queued(ref mut delivery) => {
                delivery.emit(&text, VisibleUpdateKind::PlannerProgress)
            }
            DeliveryMode::Silent => {}
        }
    }

    pub(crate) fn emit_task_planner_progress(&mut self) {
        if !self.policy.supports_current_supplemental {
            return;
        }
        let text = normalize_visible_update(
            &self.visible_update_contract.task_planner_progress(),
            MAX_QUEUED_PROGRESS_CHARS,
        );
        if text.is_empty() {
            return;
        }
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => {
                delivery.force_visible_update(&text, VisibleUpdateKind::PlannerProgress)
            }
            DeliveryMode::Queued(ref mut delivery) => {
                delivery.emit(&text, VisibleUpdateKind::PlannerProgress)
            }
            DeliveryMode::Silent => {}
        }
    }

    pub(crate) fn emit_tool_progress(&mut self, name: &str, index: usize, total: usize) {
        if !self.policy.supports_current_supplemental {
            return;
        }
        let text = normalize_visible_update(
            &self
                .visible_update_contract
                .tool_progress(name, index, total),
            MAX_QUEUED_PROGRESS_CHARS,
        );
        if text.is_empty() {
            return;
        }
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => {
                delivery.force_visible_update(&text, VisibleUpdateKind::ToolProgress)
            }
            DeliveryMode::Queued(ref mut delivery) => {
                delivery.emit(&text, VisibleUpdateKind::ToolProgress)
            }
            DeliveryMode::Silent => {}
        }
    }

    pub(crate) fn emit_task_action_progress(&mut self, kind: TaskActionProgressKind) {
        if !self.policy.supports_current_supplemental {
            return;
        }
        let text = normalize_visible_update(
            &self.visible_update_contract.task_action_progress(kind),
            MAX_QUEUED_PROGRESS_CHARS,
        );
        if text.is_empty() {
            return;
        }
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => {
                delivery.force_visible_update(&text, VisibleUpdateKind::ActionProgress)
            }
            DeliveryMode::Queued(ref mut delivery) => {
                delivery.emit(&text, VisibleUpdateKind::ActionProgress)
            }
            DeliveryMode::Silent => {}
        }
    }

    pub(crate) fn emit_foreground_work_resumed(&mut self) {
        self.emit_task_action_progress(TaskActionProgressKind::Resumed);
    }

    pub(crate) fn emit_task_terminal_progress(&mut self, kind: TaskTerminalProgressKind) {
        if !self.policy.supports_current_supplemental {
            return;
        }
        let text = normalize_visible_update(
            &self.visible_update_contract.task_terminal_progress(kind),
            MAX_QUEUED_PROGRESS_CHARS,
        );
        if text.is_empty() {
            return;
        }
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => {
                delivery.force_visible_update(&text, VisibleUpdateKind::TerminalProgress)
            }
            DeliveryMode::Queued(ref mut delivery) => {
                delivery.emit(&text, VisibleUpdateKind::TerminalProgress)
            }
            DeliveryMode::Silent => {}
        }
    }

    pub(crate) fn emit_foreground_work_blocked(&mut self) {
        self.emit_task_terminal_progress(TaskTerminalProgressKind::Blocked);
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
                delivery.force_visible_update(&text, VisibleUpdateKind::PartialDraft)
            }
            // Non-edit channels cannot revise previously sent text, so exposing ToolUse-time
            // assistant drafts here tends to leak unfinished step plans to the user.
            // Keep queued delivery runtime-controlled: typed progress updates + final answer.
            DeliveryMode::Queued(_) => {}
            DeliveryMode::Silent => {}
        }
    }

    /// 返回 true 表示最终答复已经直接交付到通道，外层应跳过 outbound_tx。
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
    fn drop(&mut self) {}
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

    fn force_visible_update(&mut self, content: &str, kind: VisibleUpdateKind) {
        if self.edit_disabled || self.lifecycle.is_closed() {
            return;
        }
        record_visible_update_kind(&mut self.report, kind);
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
    fn emit(&mut self, content: &str, kind: VisibleUpdateKind) {
        if self.lifecycle.is_closed() {
            return;
        }
        let normalized = normalize_visible_update(content, crate::bus::MAX_CONTENT_LEN);
        if normalized.is_empty() || normalized == self.last_visible_text {
            return;
        }
        record_visible_update_kind(&mut self.report, kind);
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
}

impl VisibleUpdateContract {
    fn new(loc: UiLocale) -> Self {
        Self { loc }
    }

    fn tool_progress(self, name: &str, index: usize, total: usize) -> String {
        match self.loc {
            UiLocale::Zh if total > 1 => {
                format!("正在执行 {}（{}/{}），继续推进 🪲", name, index + 1, total)
            }
            UiLocale::Zh => format!("正在执行 {}，继续推进 🪲", name),
            UiLocale::En if total > 1 => {
                format!(
                    "Running {} ({}/{}), still moving 🪲",
                    name,
                    index + 1,
                    total
                )
            }
            UiLocale::En => format!("Running {}, still moving 🪲", name),
        }
    }

    fn task_planner_progress(self) -> String {
        match self.loc {
            UiLocale::Zh => "正在判断当前动作路径，继续推进 🪲".to_string(),
            UiLocale::En => "Evaluating the current action path, still moving 🪲".to_string(),
        }
    }

    fn task_action_progress(self, kind: TaskActionProgressKind) -> String {
        match (self.loc, kind) {
            (UiLocale::Zh, TaskActionProgressKind::Started) => {
                "已进入任务执行，继续推进 🪲".to_string()
            }
            (UiLocale::Zh, TaskActionProgressKind::Resumed) => {
                "已恢复当前任务，继续推进 🪲".to_string()
            }
            (UiLocale::En, TaskActionProgressKind::Started) => {
                "Task execution started, still moving 🪲".to_string()
            }
            (UiLocale::En, TaskActionProgressKind::Resumed) => {
                "Current task resumed, still moving 🪲".to_string()
            }
        }
    }

    fn task_terminal_progress(self, kind: TaskTerminalProgressKind) -> String {
        match (self.loc, kind) {
            (UiLocale::Zh, TaskTerminalProgressKind::Completed) => {
                "当前任务已完成，正在整理答复 🪲".to_string()
            }
            (UiLocale::Zh, TaskTerminalProgressKind::PartialComplete) => {
                "当前任务已部分完成，正在整理结果 🪲".to_string()
            }
            (UiLocale::Zh, TaskTerminalProgressKind::Blocked) => {
                "当前任务已阻塞，正在整理结果 🪲".to_string()
            }
            (UiLocale::Zh, TaskTerminalProgressKind::Aborted) => {
                "当前任务已终止，正在整理结果 🪲".to_string()
            }
            (UiLocale::En, TaskTerminalProgressKind::Completed) => {
                "Task completed, preparing the final reply 🪲".to_string()
            }
            (UiLocale::En, TaskTerminalProgressKind::PartialComplete) => {
                "Task partially completed, preparing the result 🪲".to_string()
            }
            (UiLocale::En, TaskTerminalProgressKind::Blocked) => {
                "Task blocked, preparing the result 🪲".to_string()
            }
            (UiLocale::En, TaskTerminalProgressKind::Aborted) => {
                "Task aborted, preparing the result 🪲".to_string()
            }
        }
    }
}

fn record_visible_update_kind(report: &mut DeliveryReport, kind: VisibleUpdateKind) {
    match kind {
        VisibleUpdateKind::PlannerProgress => {
            report.progress_updates_sent = report.progress_updates_sent.saturating_add(1);
            report.planner_progress_updates_sent =
                report.planner_progress_updates_sent.saturating_add(1);
        }
        VisibleUpdateKind::ToolProgress => {
            report.progress_updates_sent = report.progress_updates_sent.saturating_add(1);
            report.tool_progress_updates_sent = report.tool_progress_updates_sent.saturating_add(1);
        }
        VisibleUpdateKind::ActionProgress => {
            report.progress_updates_sent = report.progress_updates_sent.saturating_add(1);
            report.action_progress_updates_sent =
                report.action_progress_updates_sent.saturating_add(1);
        }
        VisibleUpdateKind::TerminalProgress => {
            report.progress_updates_sent = report.progress_updates_sent.saturating_add(1);
            report.terminal_progress_updates_sent =
                report.terminal_progress_updates_sent.saturating_add(1);
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
    fn queued_delivery_without_supplemental_contract_suppresses_progress_updates() {
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
                planner_progress_updates_sent: 1,
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
            .deliver_tool_outbound_intent(&ToolOutboundIntent {
                target: ToolOutboundTarget::CurrentChat,
                delivery_kind: ToolOutboundDeliveryKind::Supplemental,
                content: "补充说明".to_string(),
            })
            .expect("supplemental intent");
        let primary = delivery
            .deliver_tool_outbound_intent(&ToolOutboundIntent {
                target: ToolOutboundTarget::CurrentChat,
                delivery_kind: ToolOutboundDeliveryKind::Primary,
                content: "主答复".to_string(),
            })
            .expect("suppressed primary intent");
        let explicit = delivery
            .deliver_tool_outbound_intent(&ToolOutboundIntent {
                target: ToolOutboundTarget::Explicit {
                    channel: "telegram".to_string(),
                    chat_id: "chat-2".to_string(),
                },
                delivery_kind: ToolOutboundDeliveryKind::Supplemental,
                content: "不应再发送".to_string(),
            })
            .expect("explicit intent");

        assert_eq!(supplemental, ToolIntentDelivery::Suppressed);
        assert_eq!(primary, ToolIntentDelivery::Suppressed);
        assert_eq!(explicit, ToolIntentDelivery::VisibleUpdate);
        let outbound = outbound_rx.try_recv().expect("explicit outbound");
        assert_eq!(outbound.channel.as_ref(), "telegram");
        assert_eq!(outbound.chat_id.as_ref(), "chat-2");
        assert_eq!(outbound.content, "不应再发送");
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
    fn queued_delivery_tool_progress_uses_typed_progress_copy() {
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
        assert_eq!(outbound.content, "正在执行 board_info，继续推进 🪲");
        assert_eq!(
            delivery.report(),
            DeliveryReport {
                progress_updates_sent: 1,
                tool_progress_updates_sent: 1,
                visible_text_updates_sent: 1,
                ..DeliveryReport::default()
            }
        );
    }

    #[test]
    fn queued_delivery_task_planner_progress_uses_typed_progress_contract() {
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

        delivery.emit_task_planner_progress();

        let outbound = outbound_rx.try_recv().expect("planner pulse");
        assert_eq!(outbound.content, "正在判断当前动作路径，继续推进 🪲");
        assert_eq!(
            delivery.report(),
            DeliveryReport {
                progress_updates_sent: 1,
                planner_progress_updates_sent: 1,
                visible_text_updates_sent: 1,
                ..DeliveryReport::default()
            }
        );
    }

    #[test]
    fn queued_delivery_task_action_progress_uses_typed_progress_contract() {
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

        delivery.emit_task_action_progress(TaskActionProgressKind::Resumed);

        let outbound = outbound_rx.try_recv().expect("action pulse");
        assert_eq!(outbound.content, "已恢复当前任务，继续推进 🪲");
        assert_eq!(
            delivery.report(),
            DeliveryReport {
                progress_updates_sent: 1,
                action_progress_updates_sent: 1,
                visible_text_updates_sent: 1,
                ..DeliveryReport::default()
            }
        );
    }

    #[test]
    fn queued_delivery_started_action_progress_uses_action_progress_contract() {
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

        delivery.emit_task_action_progress(TaskActionProgressKind::Started);

        let outbound = outbound_rx.try_recv().expect("action pulse");
        assert_eq!(outbound.content, "已进入任务执行，继续推进 🪲");
        assert_eq!(
            delivery.report(),
            DeliveryReport {
                progress_updates_sent: 1,
                action_progress_updates_sent: 1,
                visible_text_updates_sent: 1,
                ..DeliveryReport::default()
            }
        );
    }

    #[test]
    fn action_progress_reports_no_implicit_presence_pulses() {
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

        delivery.emit_foreground_work_resumed();

        let outbound = outbound_rx.try_recv().expect("action pulse");
        assert_eq!(outbound.content, "已恢复当前任务，继续推进 🪲");
        assert_eq!(delivery.report().presence_pulses_sent, 0);
        assert_eq!(delivery.report().action_progress_updates_sent, 1);
    }

    #[test]
    fn queued_delivery_task_terminal_progress_uses_typed_progress_contract() {
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

        delivery.emit_task_terminal_progress(TaskTerminalProgressKind::PartialComplete);

        let outbound = outbound_rx.try_recv().expect("terminal pulse");
        assert_eq!(outbound.content, "当前任务已部分完成，正在整理结果 🪲");
        assert_eq!(
            delivery.report(),
            DeliveryReport {
                progress_updates_sent: 1,
                terminal_progress_updates_sent: 1,
                visible_text_updates_sent: 1,
                ..DeliveryReport::default()
            }
        );
    }

    #[test]
    fn queued_delivery_foreground_work_blocked_uses_terminal_progress_contract() {
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

        delivery.emit_foreground_work_blocked();

        let outbound = outbound_rx.try_recv().expect("terminal pulse");
        assert_eq!(outbound.content, "当前任务已阻塞，正在整理结果 🪲");
        assert_eq!(
            delivery.report(),
            DeliveryReport {
                progress_updates_sent: 1,
                terminal_progress_updates_sent: 1,
                visible_text_updates_sent: 1,
                ..DeliveryReport::default()
            }
        );
    }
}
