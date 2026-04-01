use crate::bus::{IngressKind, OutboundTx, PcMsg};
use crate::error::Result;
use crate::i18n::{tr, Locale as UiLocale, Message as UiMessage};
use crate::metrics;
use crate::util::truncate_content_to_max;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
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

pub(crate) struct DeliverySession<'a> {
    mode: DeliveryMode<'a>,
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
    last_visible_text: String,
}

struct QueuedDelivery<'a> {
    outbound_tx: &'a OutboundTx,
    channel: &'a str,
    chat_id: &'a str,
    req_id: &'a str,
    last_visible_text: String,
    shared: Arc<QueuedDeliveryShared>,
}

struct QueuedDeliveryShared {
    visible_updates_sent: AtomicU8,
    waiting_notice_canceled: AtomicBool,
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
                last_visible_text: String::new(),
            })
        } else {
            DeliveryMode::Queued(QueuedDelivery {
                outbound_tx,
                channel: msg.channel.as_ref(),
                chat_id: msg.chat_id.as_ref(),
                req_id,
                last_visible_text: String::new(),
                shared: spawn_waiting_notice(
                    outbound_tx.clone(),
                    msg.channel.as_ref(),
                    msg.chat_id.as_ref(),
                    req_id,
                    tr(UiMessage::AgentStillWorking, loc),
                ),
            })
        };
        Self { mode }
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
            DeliveryMode::Edit(ref mut delivery) => delivery.force_visible_update(&text),
            DeliveryMode::Queued(ref mut delivery) => delivery.emit(&text),
            DeliveryMode::Silent => {}
        }
    }

    pub(crate) fn emit_partial(&mut self, content: &str) {
        let text = normalize_visible_update(content, MAX_QUEUED_PARTIAL_CHARS);
        if text.chars().count() < MIN_PARTIAL_VISIBLE_CHARS {
            return;
        }
        match self.mode {
            DeliveryMode::Edit(ref mut delivery) => delivery.force_visible_update(&text),
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
                false
            }
            DeliveryMode::Silent => false,
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
        if self.edit_disabled || accumulated.trim().is_empty() {
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

    fn force_visible_update(&mut self, content: &str) {
        if self.edit_disabled {
            return;
        }
        if self.message_id.is_none() {
            self.send_initial(content);
        } else {
            self.edit_existing(content);
        }
    }

    fn finalize(&mut self, final_content: &str) -> bool {
        if self.message_id.is_none() {
            if final_content.trim().is_empty() {
                return false;
            }
            self.send_initial(final_content);
        } else if !final_content.trim().is_empty() {
            self.edit_existing(final_content);
        }
        self.message_id.is_some() && !self.edit_disabled
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

    fn emit(&mut self, content: &str) {
        let normalized = normalize_visible_update(content, crate::bus::MAX_CONTENT_LEN);
        if normalized.is_empty() || normalized == self.last_visible_text {
            return;
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
            }
            Err(()) => {
                self.shared
                    .visible_updates_sent
                    .fetch_sub(1, Ordering::Relaxed);
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

fn normalize_visible_update(content: &str, max_chars: usize) -> String {
    truncate_content_to_max(content.trim(), max_chars)
        .trim()
        .to_string()
}

fn send_visible_update(
    outbound_tx: &OutboundTx,
    channel: &str,
    chat_id: &str,
    req_id: &str,
    content: &str,
) -> std::result::Result<(), ()> {
    let msg = PcMsg {
        channel: std::sync::Arc::from(channel),
        chat_id: std::sync::Arc::from(chat_id),
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

fn spawn_waiting_notice(
    outbound_tx: OutboundTx,
    channel: &str,
    chat_id: &str,
    req_id: &str,
    waiting_notice: String,
) -> Arc<QueuedDeliveryShared> {
    let shared = Arc::new(QueuedDeliveryShared {
        visible_updates_sent: AtomicU8::new(0),
        waiting_notice_canceled: AtomicBool::new(false),
    });
    let worker_shared = Arc::clone(&shared);
    let channel = channel.to_string();
    let chat_id = chat_id.to_string();
    let req_id = req_id.to_string();
    let spawn_res = std::thread::Builder::new()
        .name("agent_waiting_notice".to_string())
        .spawn(move || {
            std::thread::sleep(waiting_notice_delay());
            if worker_shared
                .waiting_notice_canceled
                .load(Ordering::Relaxed)
            {
                return;
            }
            if !try_claim_shared_visible_slot(&worker_shared) {
                return;
            }
            if send_visible_update(&outbound_tx, &channel, &chat_id, &req_id, &waiting_notice)
                .is_err()
            {
                worker_shared
                    .visible_updates_sent
                    .fetch_sub(1, Ordering::Relaxed);
            }
        });
    if let Err(e) = spawn_res {
        log::warn!("[agent_delivery] failed to spawn waiting notice: {}", e);
    }
    shared
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

#[cfg(test)]
fn waiting_notice_delay() -> std::time::Duration {
    std::time::Duration::from_millis(20)
}

#[cfg(not(test))]
fn waiting_notice_delay() -> std::time::Duration {
    std::time::Duration::from_millis(2500)
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

    #[test]
    fn queued_delivery_emits_distinct_updates_with_cap() {
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
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(&msg, "req-1", &outbound_tx, None, UiLocale::Zh);

        delivery.emit_partial("## 第2步：检查文件系统结构");

        assert!(outbound_rx.try_recv().is_err());
    }

    #[test]
    fn edit_delivery_finalizes_without_outbound_message() {
        let (outbound_tx, _outbound_rx, _) = new_inbound_channel(4);
        let msg = build_msg("telegram");
        let editor = StubEditor::default();
        let mut delivery =
            DeliverySession::new(&msg, "req-1", &outbound_tx, Some(&editor), UiLocale::Zh);

        delivery.emit_progress("正在执行 tools");
        let streamed = delivery.finalize("最终答案");

        assert!(streamed);
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
    fn queued_delivery_sends_waiting_notice_for_long_think() {
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let _delivery = DeliverySession::new(&msg, "req-1", &outbound_tx, None, UiLocale::Zh);

        std::thread::sleep(waiting_notice_delay() + std::time::Duration::from_millis(20));

        let first = outbound_rx.try_recv().expect("waiting notice");
        assert_eq!(first.content, "还在处理，请稍等 ⏳");
    }

    #[test]
    fn queued_delivery_finalize_cancels_waiting_notice() {
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        let mut delivery = DeliverySession::new(&msg, "req-1", &outbound_tx, None, UiLocale::Zh);

        let streamed = delivery.finalize("最终答案");
        assert!(!streamed);
        std::thread::sleep(waiting_notice_delay() + std::time::Duration::from_millis(20));

        assert!(outbound_rx.try_recv().is_err());
    }

    #[test]
    fn queued_delivery_drop_cancels_waiting_notice() {
        let (outbound_tx, outbound_rx, _) = new_inbound_channel(8);
        let msg = build_msg("qq_channel");
        {
            let _delivery = DeliverySession::new(&msg, "req-1", &outbound_tx, None, UiLocale::Zh);
        }

        std::thread::sleep(waiting_notice_delay() + std::time::Duration::from_millis(20));

        assert!(outbound_rx.try_recv().is_err());
    }
}
