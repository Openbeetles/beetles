//! 出站分发：从 outbound_rx 取 PcMsg，按 channel 调用对应 MessageSink；按通道熔断，避免单通道拖垮全局。
//! Outbound dispatch: recv from outbound_rx, send via MessageSink; per-channel circuit breaker.

#[cfg(any(
    feature = "telegram",
    feature = "feishu",
    feature = "dingtalk",
    feature = "wecom",
    feature = "qq_channel"
))]
use crate::bus::CanonicalMessageBody;
use crate::bus::{OutboundKind, OutboundRx, PcMsg, MAX_CONTENT_LEN};
use crate::channel_capability::ChannelCapabilityRegistry;
use crate::config::AppConfig;
use crate::error::Result;
use crate::metrics;
use crate::orchestrator::AdmissionDecision;
use crate::platform::PlatformHttpClient;
#[cfg(not(any(
    feature = "telegram",
    feature = "feishu",
    feature = "dingtalk",
    feature = "wecom",
    feature = "qq_channel"
)))]
use crate::util::truncate_content_to_max;
#[cfg(any(
    feature = "telegram",
    feature = "feishu",
    feature = "dingtalk",
    feature = "wecom",
    feature = "qq_channel"
))]
use crate::util::truncate_content_to_max;
#[cfg(any(
    feature = "telegram",
    feature = "feishu",
    feature = "dingtalk",
    feature = "qq_channel"
))]
use crate::util::STACK_CHANNEL_SENDER;
use std::collections::HashMap;
use std::collections::VecDeque;
#[cfg(any(
    feature = "telegram",
    feature = "feishu",
    feature = "dingtalk",
    feature = "wecom",
    feature = "qq_channel"
))]
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 出站发送抽象；各通道实现此 trait，由 main 注册到 ChannelSinks。
pub trait MessageSink: Send + Sync {
    fn send(&self, chat_id: &str, content: &str) -> Result<()>;

    fn send_message(&self, msg: &PcMsg, content: &str) -> Result<()> {
        self.send_with_req(
            &msg.chat_id,
            content,
            msg.req_id.as_deref(),
            msg.outbound_kind,
        )
    }

    fn send_with_req(
        &self,
        chat_id: &str,
        content: &str,
        _req_id: Option<&str>,
        _outbound_kind: OutboundKind,
    ) -> Result<()> {
        self.send(chat_id, content)
    }

    /// 发送消息并返回平台侧 message_id（用于后续编辑）。默认回退到 send + None。
    fn send_and_get_id(&self, chat_id: &str, content: &str) -> Result<Option<String>> {
        self.send(chat_id, content)?;
        Ok(None)
    }

    /// 编辑已发送的消息。默认 no-op（不支持编辑的通道直接忽略）。
    fn edit(&self, _chat_id: &str, _message_id: &str, _content: &str) -> Result<()> {
        Ok(())
    }
}

/// 队列型 Sink：将 (chat_id, content) 送入 channel，由 main 的 flush_*_sends 消费。各通道仅 stage 不同。
#[cfg(any(
    feature = "telegram",
    feature = "feishu",
    feature = "dingtalk",
    feature = "wecom",
    feature = "qq_channel"
))]
pub struct QueuedSink {
    tx: std::sync::mpsc::SyncSender<super::send::QueuedOutboundMessage>,
    stage: &'static str,
}

#[cfg(any(
    feature = "telegram",
    feature = "feishu",
    feature = "dingtalk",
    feature = "wecom",
    feature = "qq_channel"
))]
impl QueuedSink {
    pub fn new(
        tx: std::sync::mpsc::SyncSender<super::send::QueuedOutboundMessage>,
        stage: &'static str,
    ) -> Self {
        Self { tx, stage }
    }
}

#[cfg(any(
    feature = "telegram",
    feature = "feishu",
    feature = "dingtalk",
    feature = "wecom",
    feature = "qq_channel"
))]
impl MessageSink for QueuedSink {
    fn send(&self, chat_id: &str, content: &str) -> Result<()> {
        self.send_with_req(chat_id, content, None, OutboundKind::Primary)
    }

    fn send_message(&self, msg: &PcMsg, content: &str) -> Result<()> {
        let content = truncate_content_to_max(content, MAX_CONTENT_LEN);
        self.tx
            .try_send(super::send::QueuedOutboundMessage {
                transport_send_id: super::send::next_queued_outbound_id(),
                chat_id: msg.chat_id.to_string(),
                content: content.into_owned(),
                body: msg.body.clone(),
                platform_thread_id: msg.platform_thread_id.clone(),
                platform_message_id: msg.platform_message_id.clone(),
                req_id: msg.req_id.clone(),
                outbound_kind: msg.outbound_kind,
            })
            .map_err(|e| crate::error::Error::Other {
                source: Box::new(e),
                stage: self.stage,
            })
    }

    fn send_with_req(
        &self,
        chat_id: &str,
        content: &str,
        req_id: Option<&str>,
        outbound_kind: OutboundKind,
    ) -> Result<()> {
        let content = truncate_content_to_max(content, MAX_CONTENT_LEN);
        let projection = content.as_ref().to_string();
        self.tx
            .try_send(super::send::QueuedOutboundMessage {
                transport_send_id: super::send::next_queued_outbound_id(),
                chat_id: chat_id.to_string(),
                content: projection.clone(),
                body: CanonicalMessageBody::text(projection),
                platform_thread_id: String::new(),
                platform_message_id: String::new(),
                req_id: req_id.map(str::to_string),
                outbound_kind,
            })
            .map_err(|e| crate::error::Error::Other {
                source: Box::new(e),
                stage: self.stage,
            })
    }
}

/// channel 名称 → sink 映射；由 main 构造并传入 run_dispatch。
pub struct ChannelSinks {
    map: HashMap<String, Box<dyn MessageSink>>,
}

impl ChannelSinks {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn register(&mut self, channel: impl Into<String>, sink: Box<dyn MessageSink>) {
        self.map.insert(channel.into(), sink);
    }

    fn get(&self, channel: &str) -> Option<&dyn MessageSink> {
        self.map.get(channel).map(|b| b.as_ref())
    }
}

impl Default for ChannelSinks {
    fn default() -> Self {
        Self::new()
    }
}

/// 可选重试次数（含首次）；重试间隔（毫秒），避免连续锤击失败通道。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const SEND_RETRY: u32 = 2;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const SEND_RETRY: u32 = 3;

const SEND_RETRY_DELAY_MS: u64 = 500;

fn is_channel_in_cooldown(channel: &str) -> bool {
    !crate::orchestrator::is_channel_healthy_pub(channel)
}

fn record_channel_fail(channel: &str) {
    crate::orchestrator::record_channel_result_pub(channel, false);
}

fn record_channel_ok(channel: &str) {
    crate::orchestrator::record_channel_result_pub(channel, true);
}

fn outbound_reject_reason(msg: &crate::bus::PcMsg) -> Option<&'static str> {
    match crate::orchestrator::should_accept_outbound_pub(&msg.channel) {
        AdmissionDecision::Reject { reason } => Some(reason),
        AdmissionDecision::Accept | AdmissionDecision::Defer { .. } => None,
    }
}

enum BufferPushResult {
    Buffered,
    Dropped,
    Full(Box<crate::bus::PcMsg>),
}

fn try_push_buffered_msg(
    tag: &str,
    cooldown_buffer: &mut VecDeque<crate::bus::PcMsg>,
    msg: crate::bus::PcMsg,
) -> BufferPushResult {
    if !msg.outbound_kind.is_supplemental() {
        if let Some(req_id) = msg
            .req_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            if let Some(existing) = cooldown_buffer.iter_mut().find(|buffered| {
                !buffered.outbound_kind.is_supplemental()
                    && buffered.req_id.as_deref() == Some(req_id)
                    && buffered.channel == msg.channel
                    && buffered.chat_id == msg.chat_id
            }) {
                log::debug!(
                    "[{}] req_id={} channel={} coalesced duplicate deferred primary",
                    tag,
                    req_id,
                    msg.channel
                );
                *existing = msg;
                return BufferPushResult::Buffered;
            }
        }
    }
    if cooldown_buffer.len() < COOLDOWN_BUFFER_MAX {
        cooldown_buffer.push_back(msg);
        return BufferPushResult::Buffered;
    }
    if msg.outbound_kind.is_supplemental() {
        log::warn!(
            "[{}] req_id={} channel={} supplemental deferred buffer full, dropping new message",
            tag,
            msg.req_id.as_deref().unwrap_or("-"),
            msg.channel
        );
        return BufferPushResult::Dropped;
    }
    if let Some(pos) = cooldown_buffer
        .iter()
        .position(|buffered| buffered.outbound_kind.is_supplemental())
    {
        cooldown_buffer.remove(pos);
        cooldown_buffer.push_back(msg);
        log::warn!(
            "[{}] deferred buffer full, evicted supplemental message to preserve primary reply",
            tag
        );
        return BufferPushResult::Buffered;
    }
    log::warn!(
        "[{}] req_id={} channel={} primary deferred buffer full, holding until replay frees space",
        tag,
        msg.req_id.as_deref().unwrap_or("-"),
        msg.channel
    );
    BufferPushResult::Full(Box::new(msg))
}

fn buffer_deferred_msg_with_replay<F>(
    tag: &str,
    cooldown_buffer: &mut VecDeque<crate::bus::PcMsg>,
    msg: crate::bus::PcMsg,
    mut replay_ready: F,
) where
    F: FnMut(&mut VecDeque<crate::bus::PcMsg>),
{
    let pending = match try_push_buffered_msg(tag, cooldown_buffer, msg) {
        BufferPushResult::Buffered | BufferPushResult::Dropped => return,
        BufferPushResult::Full(msg) => msg,
    };

    replay_ready(cooldown_buffer);
    match try_push_buffered_msg(tag, cooldown_buffer, *pending) {
        BufferPushResult::Buffered | BufferPushResult::Dropped => {}
        BufferPushResult::Full(msg) => {
            metrics::record_error_by_stage("channel_deferred_buffer_full");
            log::error!(
                "[{}] req_id={} channel={} deferred primary buffer full after bounded replay",
                tag,
                msg.req_id.as_deref().unwrap_or("-"),
                msg.channel
            );
        }
    }
}

fn buffer_deferred_msg_without_replay(
    tag: &str,
    cooldown_buffer: &mut VecDeque<crate::bus::PcMsg>,
    msg: crate::bus::PcMsg,
) {
    match try_push_buffered_msg(tag, cooldown_buffer, msg) {
        BufferPushResult::Buffered | BufferPushResult::Dropped => {}
        BufferPushResult::Full(msg) => {
            metrics::record_error_by_stage("channel_deferred_buffer_full");
            log::error!(
                "[{}] req_id={} channel={} deferred primary buffer full under local pressure",
                tag,
                msg.req_id.as_deref().unwrap_or("-"),
                msg.channel
            );
        }
    }
}

fn replay_cooldown_buffer_with<FH, FS>(
    cooldown_buffer: &mut VecDeque<crate::bus::PcMsg>,
    mut is_in_cooldown: FH,
    mut send: FS,
) where
    FH: FnMut(&str) -> bool,
    FS: FnMut(&crate::bus::PcMsg) -> bool,
{
    let mut i = 0;
    while i < cooldown_buffer.len() {
        let Some(buffered) = cooldown_buffer.get(i) else {
            break;
        };
        if is_in_cooldown(buffered.channel.as_ref()) {
            i += 1;
            continue;
        }
        let Some(buffered) = cooldown_buffer.remove(i) else {
            break;
        };
        if send(&buffered) {
            continue;
        }
        cooldown_buffer.insert(i, buffered);
        break;
    }
}

fn replay_ready_messages_for_tick<FH, FS>(
    cooldown_buffer: &mut VecDeque<crate::bus::PcMsg>,
    is_in_cooldown: FH,
    send: FS,
) where
    FH: FnMut(&str) -> bool,
    FS: FnMut(&crate::bus::PcMsg) -> bool,
{
    replay_cooldown_buffer_with(cooldown_buffer, is_in_cooldown, send);
}

#[derive(Default)]
struct DeferredReplayGate {
    next_replay_at: Option<Instant>,
}

impl DeferredReplayGate {
    fn ready(&self) -> bool {
        self.ready_at(Instant::now())
    }

    fn ready_at(&self, now: Instant) -> bool {
        self.next_replay_at
            .is_none_or(|next_replay_at| now >= next_replay_at)
    }

    fn defer(&mut self, delay: Duration) {
        self.defer_until_after(Instant::now(), delay);
    }

    fn defer_until_after(&mut self, now: Instant, delay: Duration) {
        self.next_replay_at = Some(now.checked_add(delay).unwrap_or(now));
    }

    fn clear(&mut self) {
        self.next_replay_at = None;
    }
}

fn deferred_buffer_replay_delay_ms() -> u64 {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        crate::orchestrator::pressure::budget_for_level(crate::orchestrator::refresh_heap_if_stale())
            .reconnect_backoff_secs
            .saturating_mul(1000)
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        crate::constants::OUTBOUND_DEFER_DELAY_MS
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DispatchOutcome {
    Sent,
    Deferred,
    Failed,
}

impl DispatchOutcome {
    fn sent(self) -> bool {
        matches!(self, Self::Sent)
    }
}

fn should_defer_primary_send_error(error: &crate::error::Error) -> bool {
    should_defer_primary_send_error_immediately(error) || error.is_retryable_upstream()
}

fn should_defer_primary_send_error_immediately(error: &crate::error::Error) -> bool {
    error.is_tls_admission()
        || error.is_connect_error()
        || is_local_sender_queue_backpressure(error)
}

fn is_local_sender_queue_backpressure(error: &crate::error::Error) -> bool {
    matches!(
        error.stage(),
        "telegram_send_queue"
            | "feishu_send_queue"
            | "dingtalk_send_queue"
            | "wecom_send_queue"
            | "qq_channel_send_queue"
    )
}

fn dispatch_via_sink(
    tag: &str,
    sinks: &ChannelSinks,
    capability_registry: &ChannelCapabilityRegistry,
    msg: &crate::bus::PcMsg,
) -> DispatchOutcome {
    let Some(sink) = sinks.get(&msg.channel) else {
        log::warn!("[{}] no sink for channel={}", tag, msg.channel);
        return DispatchOutcome::Failed;
    };
    let prepared = super::outbound_text::prepare_outbound_message_for_channel(
        msg,
        capability_registry.get(msg.channel.as_ref()),
    );

    crate::platform::task_wdt::feed_current_task();

    if msg.outbound_kind.is_supplemental() {
        match sink.send_message(&prepared.msg, &prepared.content) {
            Ok(()) => {
                metrics::record_dispatch_send(true);
                log::debug!(
                    "[latency][dispatch] req_id={} channel={} outbound_kind=supplemental attempt=1 status=ok",
                    msg.req_id.as_deref().unwrap_or("-"),
                    msg.channel
                );
                return DispatchOutcome::Sent;
            }
            Err(error) => {
                metrics::record_dispatch_send(false);
                log::warn!(
                    "[{}] req_id={} channel={} outbound_kind=supplemental send failed: {}",
                    tag,
                    msg.req_id.as_deref().unwrap_or("-"),
                    msg.channel,
                    error
                );
                return DispatchOutcome::Failed;
            }
        }
    }

    match crate::orchestrator::should_accept_outbound_pub(&msg.channel) {
        AdmissionDecision::Accept => {}
        AdmissionDecision::Defer { delay_ms } => {
            log::info!("[{}] outbound deferred {}ms (pressure)", tag, delay_ms);
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            crate::platform::task_wdt::feed_current_task();
        }
        AdmissionDecision::Reject { reason } => {
            log::info!(
                "[{}] req_id={} channel={} outbound rejected by admission reason={}",
                tag,
                msg.req_id.as_deref().unwrap_or("-"),
                msg.channel,
                reason
            );
            return DispatchOutcome::Deferred;
        }
    }
    let background_yield = crate::orchestrator::background_outbound_yield_ms_pub();
    if background_yield > 0 {
        std::thread::sleep(std::time::Duration::from_millis(background_yield));
        crate::platform::task_wdt::feed_current_task();
    }

    let mut last_err = None;
    for attempt in 0..SEND_RETRY {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(SEND_RETRY_DELAY_MS));
            crate::platform::task_wdt::feed_current_task();
        }
        match sink.send_message(&prepared.msg, &prepared.content) {
            Ok(()) => {
                log::debug!(
                    "[latency][dispatch] req_id={} channel={} attempt={} status=ok",
                    msg.req_id.as_deref().unwrap_or("-"),
                    msg.channel,
                    attempt + 1
                );
                record_channel_ok(&msg.channel);
                metrics::record_dispatch_send(true);
                return DispatchOutcome::Sent;
            }
            Err(e) => {
                if should_defer_primary_send_error_immediately(&e) {
                    last_err = Some(e);
                    break;
                }
                last_err = Some(e);
            }
        }
    }
    if let Some(e) = last_err {
        if should_defer_primary_send_error(&e) {
            metrics::record_dispatch_send(false);
            log::warn!(
                "[{}] req_id={} channel={} deferred after retryable send failure: {}",
                tag,
                msg.req_id.as_deref().unwrap_or("-"),
                msg.channel,
                e
            );
            return DispatchOutcome::Deferred;
        }
        record_channel_fail(&msg.channel);
        metrics::record_dispatch_send(false);
        metrics::record_error_by_stage("channel_dispatch");
        log::warn!(
            "[{}] req_id={} channel={} send failed after retries: {}",
            tag,
            msg.req_id.as_deref().unwrap_or("-"),
            msg.channel,
            e
        );
    }
    DispatchOutcome::Failed
}

fn dispatch_or_buffer_via_sink(
    tag: &str,
    cooldown_buffer: &mut VecDeque<crate::bus::PcMsg>,
    sinks: &ChannelSinks,
    capability_registry: &ChannelCapabilityRegistry,
    msg: crate::bus::PcMsg,
) {
    if let Some(reason) = outbound_reject_reason(&msg) {
        if msg.outbound_kind.is_supplemental() {
            log::warn!(
                "[{}] req_id={} channel={} outbound_kind=supplemental dropped by outbound admission reason={}",
                tag,
                msg.req_id.as_deref().unwrap_or("-"),
                msg.channel,
                reason
            );
        } else {
            log::info!(
                "[{}] req_id={} channel={} deferred by outbound admission reason={}",
                tag,
                msg.req_id.as_deref().unwrap_or("-"),
                msg.channel,
                reason
            );
            buffer_deferred_msg_with_replay(tag, cooldown_buffer, msg, |buffer| {
                replay_cooldown_buffer_with(buffer, is_channel_in_cooldown, |buffered| {
                    dispatch_via_sink(tag, sinks, capability_registry, buffered).sent()
                });
            });
        }
        return;
    }

    if is_channel_in_cooldown(&msg.channel) {
        if msg.outbound_kind.is_supplemental() {
            log::warn!(
                "[{}] req_id={} channel={} outbound_kind=supplemental dropped while channel is in cooldown",
                tag,
                msg.req_id.as_deref().unwrap_or("-"),
                msg.channel
            );
        } else {
            buffer_deferred_msg_with_replay(tag, cooldown_buffer, msg, |buffer| {
                replay_cooldown_buffer_with(buffer, is_channel_in_cooldown, |buffered| {
                    dispatch_via_sink(tag, sinks, capability_registry, buffered).sent()
                });
            });
        }
        return;
    }

    if dispatch_via_sink(tag, sinks, capability_registry, &msg) == DispatchOutcome::Deferred {
        buffer_deferred_msg_with_replay(tag, cooldown_buffer, msg, |buffer| {
            replay_cooldown_buffer_with(buffer, is_channel_in_cooldown, |buffered| {
                dispatch_via_sink(tag, sinks, capability_registry, buffered).sent()
            });
        });
        std::thread::sleep(Duration::from_millis(
            crate::constants::OUTBOUND_DEFER_DELAY_MS,
        ));
        crate::platform::task_wdt::feed_current_task();
    }
}

#[cfg(any(feature = "telegram", feature = "feishu", feature = "qq_channel", test))]
pub struct ActiveOutboundDriverConfig {
    channel: String,
    driver_builder: Box<dyn FnOnce() -> Box<dyn super::send::ActiveChannelSender> + Send>,
}

#[cfg(any(feature = "telegram", feature = "feishu", feature = "qq_channel", test))]
fn queued_from_prepared(
    prepared: super::outbound_text::PreparedOutboundMessage,
) -> super::send::QueuedOutboundMessage {
    let content = truncate_content_to_max(&prepared.content, MAX_CONTENT_LEN);
    super::send::QueuedOutboundMessage {
        transport_send_id: super::send::next_queued_outbound_id(),
        chat_id: prepared.msg.chat_id.to_string(),
        content: content.into_owned(),
        body: prepared.msg.body,
        platform_thread_id: prepared.msg.platform_thread_id,
        platform_message_id: prepared.msg.platform_message_id,
        req_id: prepared.msg.req_id,
        outbound_kind: prepared.msg.outbound_kind,
    }
}

#[cfg(any(feature = "telegram", feature = "feishu", feature = "qq_channel", test))]
fn dispatch_via_active_driver(
    tag: &str,
    capability_registry: &ChannelCapabilityRegistry,
    driver: &mut dyn super::send::ActiveChannelSender,
    msg: &crate::bus::PcMsg,
) -> DispatchOutcome {
    let prepared = super::outbound_text::prepare_outbound_message_for_channel(
        msg,
        capability_registry.get(msg.channel.as_ref()),
    );
    let queued = queued_from_prepared(prepared);

    crate::platform::task_wdt::feed_current_task();
    if queued.outbound_kind.is_supplemental() {
        match driver.send_attempt(&queued, 1) {
            Ok(()) => {
                metrics::record_dispatch_send(true);
                log::debug!(
                    "[latency][{}] req_id={} channel={} outbound_kind=supplemental attempt=1 status=ok",
                    tag,
                    queued.req_id.as_deref().unwrap_or("-"),
                    msg.channel
                );
                return DispatchOutcome::Sent;
            }
            Err(error) => {
                metrics::record_dispatch_send(false);
                log::warn!(
                    "[{}] req_id={} channel={} outbound_kind=supplemental send failed: {}",
                    tag,
                    queued.req_id.as_deref().unwrap_or("-"),
                    msg.channel,
                    error
                );
                return DispatchOutcome::Failed;
            }
        }
    }

    match crate::orchestrator::should_accept_outbound_pub(&msg.channel) {
        AdmissionDecision::Accept => {}
        AdmissionDecision::Defer { delay_ms } => {
            log::info!("[{}] outbound deferred {}ms (pressure)", tag, delay_ms);
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            crate::platform::task_wdt::feed_current_task();
        }
        AdmissionDecision::Reject { reason } => {
            log::info!(
                "[{}] req_id={} channel={} outbound rejected by admission reason={}",
                tag,
                queued.req_id.as_deref().unwrap_or("-"),
                msg.channel,
                reason
            );
            return DispatchOutcome::Deferred;
        }
    }
    let background_yield = crate::orchestrator::background_outbound_yield_ms_pub();
    if background_yield > 0 {
        std::thread::sleep(std::time::Duration::from_millis(background_yield));
        crate::platform::task_wdt::feed_current_task();
    }

    let max_retries = super::send::max_retries_for_message(&queued);
    let mut last_err = None;
    for retry in 0..max_retries {
        let attempt = retry + 1;
        if retry > 0 {
            super::send::sleep_sender_retry_delay();
        }
        match driver.send_attempt(&queued, attempt) {
            Ok(()) => {
                log::debug!(
                    "[latency][{}] req_id={} channel={} driver={} attempt={} status=ok",
                    tag,
                    queued.req_id.as_deref().unwrap_or("-"),
                    msg.channel,
                    driver.tag(),
                    attempt
                );
                record_channel_ok(&msg.channel);
                metrics::record_dispatch_send(true);
                return DispatchOutcome::Sent;
            }
            Err(error) => {
                if should_defer_primary_send_error_immediately(&error)
                    || matches!(error, crate::error::Error::Config { .. })
                {
                    last_err = Some(error);
                    break;
                }
                last_err = Some(error);
            }
        }
    }
    if last_err
        .as_ref()
        .is_some_and(should_defer_primary_send_error)
    {
        metrics::record_dispatch_send(false);
        if let Some(error) = last_err {
            log::warn!(
                "[{}] req_id={} channel={} driver={} deferred after retryable send failure: {}",
                tag,
                queued.req_id.as_deref().unwrap_or("-"),
                msg.channel,
                driver.tag(),
                error
            );
        }
        return DispatchOutcome::Deferred;
    }
    record_channel_fail(&msg.channel);
    metrics::record_dispatch_send(false);
    metrics::record_error_by_stage("channel_dispatch");
    super::send::log_sender_drop(
        driver.tag(),
        queued.req_id.as_deref(),
        Some(queued.chat_id.as_str()),
        max_retries,
    );
    if let Some(error) = last_err {
        log::warn!(
            "[{}] req_id={} channel={} send failed after retries: {}",
            tag,
            queued.req_id.as_deref().unwrap_or("-"),
            msg.channel,
            error
        );
    }
    DispatchOutcome::Failed
}

/// 熔断冷却期暂存的消息上限，防止无限积累。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const COOLDOWN_BUFFER_MAX: usize = 16;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const COOLDOWN_BUFFER_MAX: usize = 64;
const DISPATCH_POLL_MAX_WAIT_MS: u64 = 200;

/// 循环接收出站消息，按 msg.channel 查找 sink 并调用 send；失败打日志并重试；
/// 单通道熔断冷却期内暂存消息，冷却结束后重放。
pub fn run_dispatch(
    outbound_rx: OutboundRx,
    sinks: Arc<ChannelSinks>,
    capability_registry: Arc<ChannelCapabilityRegistry>,
) {
    const TAG: &str = "channel_dispatch";
    let mut cooldown_buffer: VecDeque<crate::bus::PcMsg> = VecDeque::new();

    loop {
        crate::platform::task_wdt::feed_current_task();
        replay_ready_messages_for_tick(&mut cooldown_buffer, is_channel_in_cooldown, |buffered| {
            if outbound_reject_reason(buffered).is_some() {
                return false;
            }
            dispatch_via_sink(TAG, sinks.as_ref(), capability_registry.as_ref(), buffered).sent()
        });
        let msg = match outbound_rx.recv_timeout(Duration::from_millis(DISPATCH_POLL_MAX_WAIT_MS)) {
            Ok(m) => m,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(e) => {
                log::warn!("[{}] outbound disconnected, dispatch exiting: {:?}", TAG, e);
                break;
            }
        };

        let content = truncate_content_to_max(&msg.content, MAX_CONTENT_LEN);
        if content.trim() == "SILENT" || msg.channel.as_ref() == "cron" {
            continue;
        }

        dispatch_or_buffer_via_sink(
            TAG,
            &mut cooldown_buffer,
            sinks.as_ref(),
            capability_registry.as_ref(),
            msg,
        );
    }
}

#[cfg(any(feature = "telegram", feature = "feishu", feature = "qq_channel", test))]
/// 单 active-channel OS 出站 worker：ESP 用它替代 `dispatch + *_sender` 常驻线程组合。
pub fn run_os_outbound_worker(
    outbound_rx: OutboundRx,
    active: ActiveOutboundDriverConfig,
    local_sinks: Arc<ChannelSinks>,
    capability_registry: Arc<ChannelCapabilityRegistry>,
) {
    const TAG: &str = "os_outbound";
    let active_channel: Arc<str> = Arc::from(active.channel.as_str());
    let mut active_driver = (active.driver_builder)();
    let mut cooldown_buffer: VecDeque<crate::bus::PcMsg> = VecDeque::new();
    let mut replay_gate = DeferredReplayGate::default();

    loop {
        crate::platform::task_wdt::feed_current_task();
        if replay_gate.ready() {
            replay_ready_messages_for_tick(
                &mut cooldown_buffer,
                is_channel_in_cooldown,
                |buffered| {
                    if buffered.channel != active_channel {
                        if outbound_reject_reason(buffered).is_some() {
                            return false;
                        }
                        return dispatch_via_sink(
                            TAG,
                            local_sinks.as_ref(),
                            capability_registry.as_ref(),
                            buffered,
                        )
                        .sent();
                    }
                    if outbound_reject_reason(buffered).is_some() {
                        return false;
                    }
                    dispatch_via_active_driver(
                        TAG,
                        capability_registry.as_ref(),
                        active_driver.as_mut(),
                        buffered,
                    )
                    .sent()
                },
            );
            if cooldown_buffer.is_empty() {
                replay_gate.clear();
            } else {
                replay_gate.defer(Duration::from_millis(deferred_buffer_replay_delay_ms()));
            }
        }

        let msg = match outbound_rx.recv_timeout(Duration::from_millis(DISPATCH_POLL_MAX_WAIT_MS)) {
            Ok(m) => m,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(error) => {
                log::warn!(
                    "[{}] outbound disconnected, worker exiting: {:?}",
                    TAG,
                    error
                );
                break;
            }
        };

        let content = truncate_content_to_max(&msg.content, MAX_CONTENT_LEN);
        if content.trim() == "SILENT" || msg.channel.as_ref() == "cron" {
            continue;
        }

        if msg.channel != active_channel {
            dispatch_or_buffer_via_sink(
                TAG,
                &mut cooldown_buffer,
                local_sinks.as_ref(),
                capability_registry.as_ref(),
                msg,
            );
            continue;
        }

        if let Some(reason) = outbound_reject_reason(&msg) {
            if msg.outbound_kind.is_supplemental() {
                log::warn!(
                    "[{}] req_id={} channel={} outbound_kind=supplemental dropped by outbound admission reason={}",
                    TAG,
                    msg.req_id.as_deref().unwrap_or("-"),
                    msg.channel,
                    reason
                );
            } else {
                log::info!(
                    "[{}] req_id={} channel={} deferred by outbound admission reason={}",
                    TAG,
                    msg.req_id.as_deref().unwrap_or("-"),
                    msg.channel,
                    reason
                );
                buffer_deferred_msg_without_replay(TAG, &mut cooldown_buffer, msg);
                replay_gate.defer(Duration::from_millis(deferred_buffer_replay_delay_ms()));
            }
            continue;
        }

        if is_channel_in_cooldown(&msg.channel) {
            if msg.outbound_kind.is_supplemental() {
                log::warn!(
                    "[{}] req_id={} channel={} outbound_kind=supplemental dropped while channel is in cooldown",
                    TAG,
                    msg.req_id.as_deref().unwrap_or("-"),
                    msg.channel
                );
            } else {
                buffer_deferred_msg_without_replay(TAG, &mut cooldown_buffer, msg);
                replay_gate.defer(Duration::from_millis(deferred_buffer_replay_delay_ms()));
            }
            continue;
        }

        match dispatch_via_active_driver(
            TAG,
            capability_registry.as_ref(),
            active_driver.as_mut(),
            &msg,
        ) {
            DispatchOutcome::Sent | DispatchOutcome::Failed => {}
            DispatchOutcome::Deferred => {
                buffer_deferred_msg_without_replay(TAG, &mut cooldown_buffer, msg);
                replay_gate.defer(Duration::from_millis(deferred_buffer_replay_delay_ms()));
            }
        }
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
#[cfg(any(feature = "telegram", feature = "feishu", feature = "qq_channel"))]
pub fn build_esp_active_outbound_driver(
    config: &AppConfig,
    #[cfg(feature = "qq_channel")] qq_msg_id_cache: &super::QqMsgIdCache,
    #[cfg(feature = "qq_channel")] qq_token_cache: &super::SharedQqTokenCache,
    create_http: Arc<dyn Fn() -> crate::Result<Box<dyn PlatformHttpClient>> + Send + Sync>,
) -> Option<ActiveOutboundDriverConfig> {
    let enabled = crate::normalize_compiled_enabled_channel(&config.enabled_channel);
    match enabled {
        #[cfg(feature = "telegram")]
        "telegram" if !config.tg_token.trim().is_empty() => Some(ActiveOutboundDriverConfig {
            channel: "telegram".to_string(),
            driver_builder: {
                let token = config.tg_token.clone();
                let create_http = Arc::clone(&create_http);
                Box::new(move || super::telegram::telegram_outbound_driver(token, create_http))
            },
        }),
        #[cfg(feature = "feishu")]
        "feishu"
            if !config.feishu_app_id.trim().is_empty()
                && !config.feishu_app_secret.trim().is_empty() =>
        {
            Some(ActiveOutboundDriverConfig {
                channel: "feishu".to_string(),
                driver_builder: {
                    let app_id = config.feishu_app_id.clone();
                    let app_secret = config.feishu_app_secret.clone();
                    let create_http = Arc::clone(&create_http);
                    Box::new(move || {
                        super::feishu::feishu_outbound_driver(app_id, app_secret, create_http)
                    })
                },
            })
        }
        #[cfg(feature = "qq_channel")]
        "qq_channel"
            if !config.qq_channel_app_id.trim().is_empty()
                && !config.qq_channel_secret.trim().is_empty() =>
        {
            Some(ActiveOutboundDriverConfig {
                channel: "qq_channel".to_string(),
                driver_builder: {
                    let app_id = config.qq_channel_app_id.clone();
                    let secret = config.qq_channel_secret.clone();
                    let msg_id_cache = Arc::clone(qq_msg_id_cache);
                    let token_cache = qq_token_cache.clone();
                    let create_http = Arc::clone(&create_http);
                    Box::new(move || {
                        super::qq::qq_outbound_driver(
                            app_id,
                            secret,
                            msg_id_cache,
                            token_cache,
                            create_http,
                        )
                    })
                },
            })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Channel sink construction & sender thread spawning (extracted from main.rs)
// ---------------------------------------------------------------------------

/// 各通道的 rx 及 flush 所需凭证，由 build_channel_sinks 填充；未启用通道为 None。
pub struct ChannelRxSet {
    #[cfg(feature = "telegram")]
    pub telegram: Option<mpsc::Receiver<super::send::QueuedOutboundMessage>>,
    #[cfg(feature = "feishu")]
    pub feishu: Option<FeishuRxConfig>,
    #[cfg(feature = "dingtalk")]
    pub dingtalk: Option<DingtalkRxConfig>,
    #[cfg(feature = "wecom")]
    pub wecom: Option<WecomRxConfig>,
    #[cfg(feature = "qq_channel")]
    pub qq_channel: Option<QqChannelRxConfig>,
}

#[cfg(feature = "feishu")]
pub struct FeishuRxConfig {
    pub rx: mpsc::Receiver<super::send::QueuedOutboundMessage>,
    pub app_id: String,
    pub app_secret: String,
}

#[cfg(feature = "dingtalk")]
pub struct DingtalkRxConfig {
    pub rx: mpsc::Receiver<super::send::QueuedOutboundMessage>,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    pub session_store: super::DingtalkSessionStore,
}

#[cfg(feature = "wecom")]
pub struct WecomRxConfig {
    pub rx: mpsc::Receiver<super::send::QueuedOutboundMessage>,
    pub bot_id: String,
    pub bot_secret: String,
    pub websocket_url: String,
    pub route_store: super::WecomAibotRouteStore,
}

#[cfg(feature = "qq_channel")]
pub struct QqChannelRxConfig {
    pub rx: mpsc::Receiver<super::send::QueuedOutboundMessage>,
    pub app_id: String,
    pub app_secret: String,
    pub msg_id_cache: super::QqMsgIdCache,
    pub token_cache: super::SharedQqTokenCache,
}

/// Sender 二级队列深度。ESP 受内存限制为 8，Linux 有充足内存用 32。
#[cfg(any(
    feature = "telegram",
    feature = "feishu",
    feature = "dingtalk",
    feature = "wecom",
    feature = "qq_channel"
))]
const SENDER_QUEUE_DEPTH: usize = crate::constants::CHANNEL_SENDER_QUEUE_DEPTH;

/// 根据 config.enabled_channel 与凭证创建 ChannelSinks 并注册，返回 sinks 与各通道 rx 集合。
pub fn build_channel_sinks(
    config: &AppConfig,
    #[cfg(feature = "qq_channel")] qq_msg_id_cache: &super::QqMsgIdCache,
    #[cfg(feature = "qq_channel")] qq_token_cache: &super::SharedQqTokenCache,
    #[cfg(all(
        feature = "dingtalk",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    dingtalk_session_store: &super::DingtalkSessionStore,
    #[cfg(feature = "wecom")] wecom_aibot_route_store: &super::WecomAibotRouteStore,
) -> (ChannelSinks, ChannelRxSet) {
    #[cfg(not(any(
        feature = "telegram",
        feature = "feishu",
        feature = "dingtalk",
        feature = "wecom",
        feature = "qq_channel"
    )))]
    let _ = config;
    #[cfg(any(
        feature = "telegram",
        feature = "feishu",
        feature = "dingtalk",
        feature = "wecom",
        feature = "qq_channel",
        feature = "websocket"
    ))]
    let mut sinks = ChannelSinks::new();
    #[cfg(not(any(
        feature = "telegram",
        feature = "feishu",
        feature = "dingtalk",
        feature = "wecom",
        feature = "qq_channel",
        feature = "websocket"
    )))]
    let sinks = ChannelSinks::new();
    #[cfg(any(
        feature = "telegram",
        feature = "feishu",
        feature = "dingtalk",
        feature = "wecom",
        feature = "qq_channel"
    ))]
    let enabled = crate::normalize_compiled_enabled_channel(&config.enabled_channel);

    #[cfg(feature = "telegram")]
    let telegram = if enabled == "telegram" && !config.tg_token.trim().is_empty() {
        let (tx, rx) = mpsc::sync_channel::<super::send::QueuedOutboundMessage>(SENDER_QUEUE_DEPTH);
        sinks.register(
            "telegram",
            Box::new(QueuedSink::new(tx, "telegram_send_queue")),
        );
        Some(rx)
    } else {
        None
    };
    #[cfg(feature = "feishu")]
    let feishu = if enabled == "feishu"
        && !config.feishu_app_id.trim().is_empty()
        && !config.feishu_app_secret.trim().is_empty()
    {
        let (tx, rx) = mpsc::sync_channel::<super::send::QueuedOutboundMessage>(SENDER_QUEUE_DEPTH);
        sinks.register("feishu", Box::new(QueuedSink::new(tx, "feishu_send_queue")));
        Some(FeishuRxConfig {
            rx,
            app_id: config.feishu_app_id.clone(),
            app_secret: config.feishu_app_secret.clone(),
        })
    } else {
        None
    };

    #[cfg(feature = "dingtalk")]
    let dingtalk = if enabled == "dingtalk" {
        let (tx, rx) = mpsc::sync_channel::<super::send::QueuedOutboundMessage>(SENDER_QUEUE_DEPTH);
        sinks.register(
            "dingtalk",
            Box::new(QueuedSink::new(tx, "dingtalk_send_queue")),
        );
        Some(DingtalkRxConfig {
            rx,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            session_store: Arc::clone(dingtalk_session_store),
        })
    } else {
        None
    };

    #[cfg(feature = "wecom")]
    let wecom = if enabled == "wecom"
        && !config.wecom_bot_id.trim().is_empty()
        && !config.wecom_bot_secret.trim().is_empty()
    {
        let (tx, rx) = mpsc::sync_channel::<super::send::QueuedOutboundMessage>(SENDER_QUEUE_DEPTH);
        sinks.register("wecom", Box::new(QueuedSink::new(tx, "wecom_send_queue")));
        Some(WecomRxConfig {
            rx,
            bot_id: config.wecom_bot_id.clone(),
            bot_secret: config.wecom_bot_secret.clone(),
            websocket_url: config.wecom_ws_url.clone(),
            route_store: Arc::clone(wecom_aibot_route_store),
        })
    } else {
        None
    };

    #[cfg(feature = "qq_channel")]
    let qq_channel = if enabled == "qq_channel"
        && !config.qq_channel_app_id.trim().is_empty()
        && !config.qq_channel_secret.trim().is_empty()
    {
        let (tx, rx) = mpsc::sync_channel::<super::send::QueuedOutboundMessage>(SENDER_QUEUE_DEPTH);
        sinks.register(
            "qq_channel",
            Box::new(QueuedSink::new(tx, "qq_channel_send_queue")),
        );
        Some(QqChannelRxConfig {
            rx,
            app_id: config.qq_channel_app_id.clone(),
            app_secret: config.qq_channel_secret.clone(),
            msg_id_cache: Arc::clone(qq_msg_id_cache),
            token_cache: qq_token_cache.clone(),
        })
    } else {
        None
    };

    #[cfg(feature = "websocket")]
    sinks.register("websocket", Box::new(super::WebSocketSink::new("ws")));

    let rx_set = ChannelRxSet {
        #[cfg(feature = "telegram")]
        telegram,
        #[cfg(feature = "feishu")]
        feishu,
        #[cfg(feature = "dingtalk")]
        dingtalk,
        #[cfg(feature = "wecom")]
        wecom,
        #[cfg(feature = "qq_channel")]
        qq_channel,
    };
    (sinks, rx_set)
}

#[cfg(any(
    feature = "telegram",
    feature = "feishu",
    feature = "dingtalk",
    feature = "qq_channel"
))]
fn spawn_sender_thread<F>(
    tag: &str,
    started_label: &str,
    stage: &'static str,
    spawn: F,
) -> Result<()>
where
    F: FnOnce() -> std::io::Result<crate::util::TaskHandle>,
{
    spawn().map_err(|error| crate::error::Error::io(stage, error))?;
    log::info!("[{}] {}", tag, started_label);
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    crate::orchestrator::log_startup_memory_checkpoint(stage);
    Ok(())
}

/// 启动各通道的 sender 线程。rx_set 中有值的通道 `.take()` 后 spawn 线程。
/// `create_http` 在每个线程内调用以创建独立 HTTP 客户端；使用 `Arc` 共享工厂，避免闭包需实现 `Clone`。
pub fn spawn_sender_threads(
    rx_set: &mut ChannelRxSet,
    tg_token: &str,
    create_http: Arc<dyn Fn() -> crate::Result<Box<dyn PlatformHttpClient>> + Send + Sync>,
) -> Result<()> {
    #[cfg(any(
        feature = "telegram",
        feature = "feishu",
        feature = "dingtalk",
        feature = "qq_channel"
    ))]
    const TAG: &str = "beetle";
    #[cfg(not(any(
        feature = "telegram",
        feature = "feishu",
        feature = "dingtalk",
        feature = "qq_channel"
    )))]
    let _ = (&*rx_set, tg_token, &create_http);
    #[cfg(all(
        not(feature = "telegram"),
        any(
            feature = "telegram",
            feature = "feishu",
            feature = "dingtalk",
            feature = "qq_channel"
        )
    ))]
    let _ = tg_token;

    #[cfg(feature = "telegram")]
    if let Some(tg_rx) = rx_set.telegram.take() {
        let f = Arc::clone(&create_http);
        let tg_send_token = tg_token.to_string();
        spawn_sender_thread(
            TAG,
            "Telegram sender thread started",
            "telegram_sender_spawn",
            move || {
                crate::util::spawn_guarded_with_profile_handle(
                    "tg_sender",
                    STACK_CHANNEL_SENDER,
                    Some(crate::util::SpawnCore::Core0),
                    crate::util::HttpThreadRole::Io,
                    move || {
                        super::run_telegram_sender_loop(tg_rx, &tg_send_token, move || f());
                    },
                )
            },
        )?;
    }

    #[cfg(feature = "feishu")]
    if let Some(c) = rx_set.feishu.take() {
        let f = Arc::clone(&create_http);
        let fs_rx = c.rx;
        let fs_id = c.app_id;
        let fs_sec = c.app_secret;
        spawn_sender_thread(
            TAG,
            "Feishu sender thread started",
            "feishu_sender_spawn",
            move || {
                crate::util::spawn_guarded_with_profile_handle(
                    "fs_sender",
                    STACK_CHANNEL_SENDER,
                    Some(crate::util::SpawnCore::Core0),
                    crate::util::HttpThreadRole::Io,
                    move || {
                        super::run_feishu_sender_loop(fs_rx, &fs_id, &fs_sec, move || f());
                    },
                )
            },
        )?;
    }

    #[cfg(feature = "dingtalk")]
    if let Some(c) = rx_set.dingtalk.take() {
        let f = Arc::clone(&create_http);
        let dt_rx = c.rx;
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        let dt_session_store = c.session_store;
        spawn_sender_thread(
            TAG,
            "DingTalk sender thread started",
            "dingtalk_sender_spawn",
            move || {
                crate::util::spawn_guarded_with_profile_handle(
                    "dt_sender",
                    STACK_CHANNEL_SENDER,
                    Some(crate::util::SpawnCore::Core0),
                    crate::util::HttpThreadRole::Io,
                    move || {
                        super::run_dingtalk_sender_loop(
                            dt_rx,
                            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
                            &dt_session_store,
                            move || f(),
                        );
                    },
                )
            },
        )?;
    }
    #[cfg(feature = "qq_channel")]
    if let Some(c) = rx_set.qq_channel.take() {
        let f = Arc::clone(&create_http);
        let qq_rx = c.rx;
        let qq_id = c.app_id;
        let qq_sec = c.app_secret;
        let qq_cache = c.msg_id_cache;
        let qq_token_cache = c.token_cache;
        spawn_sender_thread(
            TAG,
            "QQ Channel sender thread started",
            "qq_sender_spawn",
            move || {
                crate::util::spawn_guarded_with_profile_handle(
                    "qq_sender",
                    STACK_CHANNEL_SENDER,
                    Some(crate::util::SpawnCore::Core0),
                    crate::util::HttpThreadRole::Io,
                    move || {
                        super::run_qq_sender_loop(
                            qq_rx,
                            &qq_id,
                            &qq_sec,
                            qq_cache,
                            qq_token_cache,
                            move || f(),
                        );
                    },
                )
            },
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::replay_cooldown_buffer_with;
    use super::replay_ready_messages_for_tick;
    #[cfg(any(
        feature = "telegram",
        feature = "feishu",
        feature = "dingtalk",
        feature = "qq_channel"
    ))]
    use super::spawn_sender_thread;
    use crate::bus::{OutboundKind, PcMsg};
    use crate::error::{Error, Result};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn build_msg(channel: &str, chat_id: &str, content: &str) -> PcMsg {
        PcMsg::new(channel, chat_id, content).expect("pcmsg")
    }

    #[test]
    fn queued_from_prepared_preserves_platform_message_id() {
        let mut msg =
            PcMsg::new_inbound("qq_channel", "c2c:chat-1", "hello", false).expect("pcmsg");
        msg.platform_message_id = "msg-1".to_string();
        let prepared = crate::channels::outbound_text::PreparedOutboundMessage {
            msg,
            content: "reply".to_string(),
        };

        let queued = super::queued_from_prepared(prepared);

        assert_eq!(queued.platform_message_id, "msg-1");
    }

    struct FailingSink {
        attempts: Arc<AtomicUsize>,
    }

    struct FailingActiveDriver {
        attempts: Arc<AtomicUsize>,
    }

    impl crate::channels::send::ActiveChannelSender for FailingActiveDriver {
        fn tag(&self) -> &'static str {
            "failing_active_driver"
        }

        fn send_attempt(
            &mut self,
            _message: &crate::channels::send::QueuedOutboundMessage,
            _attempt: u8,
        ) -> Result<()> {
            self.attempts.fetch_add(1, Ordering::Relaxed);
            Err(Error::config("failing_active_driver", "synthetic failure"))
        }
    }

    impl super::MessageSink for FailingSink {
        fn send(&self, _chat_id: &str, _content: &str) -> Result<()> {
            self.attempts.fetch_add(1, Ordering::Relaxed);
            Err(Error::config("failing_sink", "synthetic failure"))
        }
    }

    #[test]
    fn sender_queue_backpressure_defers_without_channel_failure() {
        let error = Error::Other {
            source: Box::new(std::io::Error::other("full")),
            stage: "qq_channel_send_queue",
        };

        assert!(super::should_defer_primary_send_error_immediately(&error));
    }

    struct TlsAdmissionFailingActiveDriver {
        attempts: Arc<AtomicUsize>,
    }

    impl crate::channels::send::ActiveChannelSender for TlsAdmissionFailingActiveDriver {
        fn tag(&self) -> &'static str {
            "tls_admission_active_driver"
        }

        fn send_attempt(
            &mut self,
            _message: &crate::channels::send::QueuedOutboundMessage,
            _attempt: u8,
        ) -> Result<()> {
            self.attempts.fetch_add(1, Ordering::Relaxed);
            Err(Error::config("tls_admission", "synthetic fragmentation"))
        }
    }

    #[test]
    fn replay_cooldown_buffer_preserves_fifo_for_ready_messages() {
        let mut buffer = VecDeque::from(vec![
            build_msg("blocked", "chat-1", "first-blocked"),
            build_msg("ready", "chat-2", "first-ready"),
            build_msg("ready", "chat-2", "second-ready"),
        ]);
        let mut replayed = Vec::new();

        replay_cooldown_buffer_with(
            &mut buffer,
            |channel| channel == "blocked",
            |msg| {
                replayed.push(msg.content.clone());
                true
            },
        );

        assert_eq!(replayed, vec!["first-ready", "second-ready"]);
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer[0].content, "first-blocked");
    }

    #[test]
    fn replay_cooldown_buffer_reinserts_failed_message_in_place() {
        let mut buffer = VecDeque::from(vec![
            build_msg("ready", "chat-1", "first-ready"),
            build_msg("ready", "chat-1", "second-ready"),
        ]);
        let mut attempts = 0usize;

        replay_cooldown_buffer_with(
            &mut buffer,
            |_channel| false,
            |_msg| {
                attempts += 1;
                false
            },
        );

        assert_eq!(attempts, 1);
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer[0].content, "first-ready");
        assert_eq!(buffer[1].content, "second-ready");
    }

    #[test]
    fn deferred_buffer_evicts_supplemental_before_primary_reply() {
        let mut buffer = VecDeque::new();
        for index in 0..super::COOLDOWN_BUFFER_MAX {
            let mut msg = build_msg("ready", "chat-1", format!("primary-{index}").as_str());
            if index == 0 {
                msg.outbound_kind = OutboundKind::Supplemental;
                msg.content = "supplemental-0".to_string();
            }
            buffer.push_back(msg);
        }
        let new_primary = build_msg("ready", "chat-1", "new-primary");

        assert!(matches!(
            super::try_push_buffered_msg("test", &mut buffer, new_primary),
            super::BufferPushResult::Buffered
        ));

        assert_eq!(buffer.len(), super::COOLDOWN_BUFFER_MAX);
        assert!(!buffer.iter().any(|msg| msg.content == "supplemental-0"));
        assert_eq!(
            buffer.back().map(|msg| msg.content.as_str()),
            Some("new-primary")
        );
    }

    #[test]
    fn deferred_buffer_rejects_unrelated_primary_when_full() {
        let mut buffer = VecDeque::new();
        for index in 0..super::COOLDOWN_BUFFER_MAX {
            buffer.push_back(build_msg(
                "ready",
                "chat-1",
                format!("primary-{index}").as_str(),
            ));
        }
        let new_primary = build_msg("ready", "chat-1", "new-primary");

        let result = super::try_push_buffered_msg("test", &mut buffer, new_primary);

        match result {
            super::BufferPushResult::Full(msg) => {
                assert_eq!(msg.content, "new-primary");
            }
            _ => panic!("full primary-only buffer must reject unrelated primary"),
        }

        assert_eq!(buffer.len(), super::COOLDOWN_BUFFER_MAX);
        assert!(buffer.iter().any(|msg| msg.content == "primary-0"));
        assert!(!buffer.iter().any(|msg| msg.content == "new-primary"));
    }

    #[test]
    fn deferred_buffer_does_not_coalesce_different_req_primary_when_full() {
        let mut buffer = VecDeque::new();
        for index in 0..super::COOLDOWN_BUFFER_MAX {
            let mut msg = build_msg("ready", "chat-1", format!("primary-{index}").as_str());
            msg.req_id = Some(format!("req-{index}"));
            buffer.push_back(msg);
        }
        let mut new_primary = build_msg("ready", "chat-1", "new-primary");
        new_primary.req_id = Some("req-new".to_string());

        assert!(matches!(
            super::try_push_buffered_msg("test", &mut buffer, new_primary),
            super::BufferPushResult::Full(_)
        ));

        assert_eq!(buffer.len(), super::COOLDOWN_BUFFER_MAX);
        assert!(buffer.iter().any(|msg| msg.content == "primary-0"));
        assert!(!buffer.iter().any(|msg| msg.content == "new-primary"));
    }

    #[test]
    fn deferred_buffer_coalesces_same_req_primary_retry() {
        let mut buffer = VecDeque::new();
        let mut first = build_msg("qq_channel", "chat-1", "old-primary");
        first.req_id = Some("req-1".to_string());
        first.outbound_kind = OutboundKind::Primary;
        buffer.push_back(first);
        let mut second = build_msg("qq_channel", "chat-1", "new-primary");
        second.req_id = Some("req-1".to_string());
        second.outbound_kind = OutboundKind::Primary;

        assert!(matches!(
            super::try_push_buffered_msg("test", &mut buffer, second),
            super::BufferPushResult::Buffered
        ));

        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer[0].content, "new-primary");
        assert_eq!(buffer[0].req_id.as_deref(), Some("req-1"));
    }

    #[test]
    fn deferred_buffer_coalesces_same_req_primary_retry_when_full() {
        let mut buffer = VecDeque::new();
        for index in 0..super::COOLDOWN_BUFFER_MAX {
            let mut msg = build_msg("qq_channel", "chat-1", format!("primary-{index}").as_str());
            msg.req_id = Some(format!("req-{index}"));
            msg.outbound_kind = OutboundKind::Primary;
            buffer.push_back(msg);
        }
        let mut retry = build_msg("qq_channel", "chat-1", "new-primary");
        retry.req_id = Some("req-0".to_string());
        retry.outbound_kind = OutboundKind::Primary;

        assert!(matches!(
            super::try_push_buffered_msg("test", &mut buffer, retry),
            super::BufferPushResult::Buffered
        ));

        assert_eq!(buffer.len(), super::COOLDOWN_BUFFER_MAX);
        assert_eq!(buffer[0].content, "new-primary");
        assert_eq!(buffer[0].req_id.as_deref(), Some("req-0"));
        assert!(!buffer.iter().any(|msg| msg.content == "primary-0"));
    }

    #[test]
    fn deferred_buffer_full_primary_uses_replay_before_recording_backpressure() {
        let mut buffer = VecDeque::new();
        for index in 0..super::COOLDOWN_BUFFER_MAX {
            buffer.push_back(build_msg(
                "blocked",
                "chat-1",
                format!("primary-{index}").as_str(),
            ));
        }
        let mut replay_attempts = 0usize;

        super::buffer_deferred_msg_with_replay(
            "test",
            &mut buffer,
            build_msg("blocked", "chat-1", "new-primary"),
            |_buffer| {
                replay_attempts += 1;
            },
        );

        assert_eq!(
            replay_attempts, 1,
            "full primary-only buffer should try bounded replay before reporting backpressure"
        );
        assert_eq!(buffer.len(), super::COOLDOWN_BUFFER_MAX);
        assert!(buffer.iter().any(|msg| msg.content == "primary-0"));
        assert!(!buffer.iter().any(|msg| msg.content == "new-primary"));
    }

    #[test]
    fn idle_tick_replays_ready_messages_without_new_inbound() {
        let mut buffer = VecDeque::from(vec![build_msg("ready", "chat-1", "deferred")]);
        let mut replayed = Vec::new();

        replay_ready_messages_for_tick(
            &mut buffer,
            |_channel| false,
            |msg| {
                replayed.push(msg.content.clone());
                true
            },
        );

        assert_eq!(replayed, vec!["deferred"]);
        assert!(buffer.is_empty());
    }

    #[test]
    fn deferred_replay_gate_holds_until_delay_expires() {
        use std::time::{Duration, Instant};

        let mut gate = super::DeferredReplayGate::default();
        let start = Instant::now();

        assert!(gate.ready_at(start));
        gate.defer_until_after(start, Duration::from_secs(5));

        assert!(!gate.ready_at(start + Duration::from_millis(4_999)));
        assert!(gate.ready_at(start + Duration::from_secs(5)));
        gate.clear();
        assert!(gate.ready_at(start + Duration::from_secs(6)));
    }

    #[cfg(any(
        feature = "telegram",
        feature = "feishu",
        feature = "dingtalk",
        feature = "qq_channel"
    ))]
    #[test]
    fn sender_thread_spawn_failure_is_propagated_with_stage() {
        let error = spawn_sender_thread("beetle", "unused", "telegram_sender_spawn", || {
            Err(std::io::Error::other("synthetic spawn failure"))
        })
        .expect_err("spawn should fail");

        assert_eq!(error.stage(), "telegram_sender_spawn");
    }

    #[test]
    fn supplemental_dispatch_fails_fast_without_retries() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let mut msg = build_msg("ready", "chat-1", "supplemental");
        msg.outbound_kind = OutboundKind::Supplemental;
        let capability_registry = crate::channel_capability::ChannelCapabilityRegistry::default();
        let mut sinks = super::ChannelSinks::new();
        sinks.register(
            "ready",
            Box::new(FailingSink {
                attempts: Arc::clone(&attempts),
            }),
        );

        assert_eq!(
            super::dispatch_via_sink("channel_dispatch", &sinks, &capability_registry, &msg),
            super::DispatchOutcome::Failed
        );
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn supplemental_active_outbound_driver_fails_fast_without_retries() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let mut driver = FailingActiveDriver {
            attempts: Arc::clone(&attempts),
        };
        let mut msg = build_msg("ready", "chat-1", "supplemental");
        msg.outbound_kind = OutboundKind::Supplemental;
        let capability_registry = crate::channel_capability::ChannelCapabilityRegistry::default();

        assert_eq!(
            super::dispatch_via_active_driver(
                "os_outbound",
                &capability_registry,
                &mut driver,
                &msg,
            ),
            super::DispatchOutcome::Failed
        );
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn primary_active_outbound_defers_tls_admission_failures() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let mut driver = TlsAdmissionFailingActiveDriver {
            attempts: Arc::clone(&attempts),
        };
        let msg = build_msg("ready", "chat-1", "primary");
        let capability_registry = crate::channel_capability::ChannelCapabilityRegistry::default();

        assert_eq!(
            super::dispatch_via_active_driver(
                "os_outbound",
                &capability_registry,
                &mut driver,
                &msg,
            ),
            super::DispatchOutcome::Deferred
        );
        assert_eq!(
            attempts.load(Ordering::Relaxed),
            1,
            "resource-pressure TLS failures should defer immediately instead of hammering retries"
        );
    }
}
