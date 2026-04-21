//! 出站分发：从 outbound_rx 取 PcMsg，按 channel 调用对应 MessageSink；按通道熔断，避免单通道拖垮全局。
//! Outbound dispatch: recv from outbound_rx, send via MessageSink; per-channel circuit breaker.

use crate::bus::{OutboundKind, OutboundRx, MAX_CONTENT_LEN};
use crate::config::AppConfig;
use crate::constants::VOICE_CHANNEL_NAME;
use crate::error::Result;
use crate::metrics;
use crate::orchestrator::AdmissionDecision;
use crate::platform::PlatformHttpClient;
use crate::util::{truncate_content_to_max, STACK_CHANNEL_SENDER};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

/// 出站发送抽象；各通道实现此 trait，由 main 注册到 ChannelSinks。
pub trait MessageSink: Send + Sync {
    fn send(&self, chat_id: &str, content: &str) -> Result<()>;

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
pub struct QueuedSink {
    tx: std::sync::mpsc::SyncSender<super::send::QueuedOutboundMessage>,
    stage: &'static str,
}

impl QueuedSink {
    pub fn new(
        tx: std::sync::mpsc::SyncSender<super::send::QueuedOutboundMessage>,
        stage: &'static str,
    ) -> Self {
        Self { tx, stage }
    }
}

impl MessageSink for QueuedSink {
    fn send(&self, chat_id: &str, content: &str) -> Result<()> {
        self.send_with_req(chat_id, content, None, OutboundKind::Primary)
    }

    fn send_with_req(
        &self,
        chat_id: &str,
        content: &str,
        req_id: Option<&str>,
        outbound_kind: OutboundKind,
    ) -> Result<()> {
        let content = truncate_content_to_max(content, MAX_CONTENT_LEN);
        self.tx
            .try_send(super::send::QueuedOutboundMessage {
                chat_id: chat_id.to_string(),
                content: content.into_owned(),
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

fn outbound_blocked(msg: &crate::bus::PcMsg) -> bool {
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    !runtime_mode.action_budget.allow_non_voice_outbound
        && msg.channel.as_ref() != VOICE_CHANNEL_NAME
}

fn push_buffered_msg(
    tag: &str,
    cooldown_buffer: &mut VecDeque<crate::bus::PcMsg>,
    msg: crate::bus::PcMsg,
) {
    if cooldown_buffer.len() < COOLDOWN_BUFFER_MAX {
        cooldown_buffer.push_back(msg);
        return;
    }
    log::warn!(
        "[{}] req_id={} channel={} deferred buffer full, dropping oldest",
        tag,
        msg.req_id.as_deref().unwrap_or("-"),
        msg.channel
    );
    cooldown_buffer.pop_front();
    cooldown_buffer.push_back(msg);
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

fn dispatch_via_sink(
    tag: &str,
    sinks: &ChannelSinks,
    msg: &crate::bus::PcMsg,
    content: &str,
) -> bool {
    let Some(sink) = sinks.get(&msg.channel) else {
        log::warn!("[{}] no sink for channel={}", tag, msg.channel);
        return false;
    };

    crate::platform::task_wdt::feed_current_task();

    if msg.outbound_kind.is_supplemental() {
        match sink.send_with_req(
            &msg.chat_id,
            content,
            msg.req_id.as_deref(),
            msg.outbound_kind,
        ) {
            Ok(()) => {
                metrics::record_dispatch_send(true);
                log::debug!(
                    "[latency][dispatch] req_id={} channel={} outbound_kind=supplemental attempt=1 status=ok",
                    msg.req_id.as_deref().unwrap_or("-"),
                    msg.channel
                );
                return true;
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
                return false;
            }
        }
    }

    if let AdmissionDecision::Defer { delay_ms } =
        crate::orchestrator::should_accept_outbound_pub(&msg.channel)
    {
        log::info!("[{}] outbound deferred {}ms (pressure)", tag, delay_ms);
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        crate::platform::task_wdt::feed_current_task();
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
        match sink.send_with_req(
            &msg.chat_id,
            content,
            msg.req_id.as_deref(),
            msg.outbound_kind,
        ) {
            Ok(()) => {
                log::debug!(
                    "[latency][dispatch] req_id={} channel={} attempt={} status=ok",
                    msg.req_id.as_deref().unwrap_or("-"),
                    msg.channel,
                    attempt + 1
                );
                record_channel_ok(&msg.channel);
                metrics::record_dispatch_send(true);
                return true;
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }
    if let Some(e) = last_err {
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
    false
}

/// 熔断冷却期暂存的消息上限，防止无限积累。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const COOLDOWN_BUFFER_MAX: usize = 16;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const COOLDOWN_BUFFER_MAX: usize = 64;
const DISPATCH_POLL_MAX_WAIT_MS: u64 = 200;

/// 循环接收出站消息，按 msg.channel 查找 sink 并调用 send；失败打日志并重试；
/// 单通道熔断冷却期内暂存消息，冷却结束后重放。
pub fn run_dispatch(outbound_rx: OutboundRx, sinks: Arc<ChannelSinks>) {
    const TAG: &str = "channel_dispatch";
    let mut cooldown_buffer: VecDeque<crate::bus::PcMsg> = VecDeque::new();

    loop {
        crate::platform::task_wdt::feed_current_task();
        replay_ready_messages_for_tick(&mut cooldown_buffer, is_channel_in_cooldown, |buffered| {
            if outbound_blocked(buffered) {
                return false;
            }
            let buffered_content = truncate_content_to_max(&buffered.content, MAX_CONTENT_LEN);
            dispatch_via_sink(TAG, sinks.as_ref(), buffered, &buffered_content)
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

        if outbound_blocked(&msg) {
            if msg.outbound_kind.is_supplemental() {
                log::warn!(
                    "[{}] req_id={} channel={} outbound_kind=supplemental dropped while voice-exclusive is active",
                    TAG,
                    msg.req_id.as_deref().unwrap_or("-"),
                    msg.channel
                );
            } else {
                log::info!(
                    "[{}] req_id={} channel={} deferred while voice-exclusive is active",
                    TAG,
                    msg.req_id.as_deref().unwrap_or("-"),
                    msg.channel
                );
                push_buffered_msg(TAG, &mut cooldown_buffer, msg);
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
                push_buffered_msg(TAG, &mut cooldown_buffer, msg);
            }
            continue;
        }
        let _ = dispatch_via_sink(TAG, sinks.as_ref(), &msg, &content);
    }
}

// ---------------------------------------------------------------------------
// Channel sink construction & sender thread spawning (extracted from main.rs)
// ---------------------------------------------------------------------------

/// 各通道的 rx 及 flush 所需凭证，由 build_channel_sinks 填充；未启用通道为 None。
pub struct ChannelRxSet {
    pub telegram: Option<mpsc::Receiver<super::send::QueuedOutboundMessage>>,
    pub feishu: Option<FeishuRxConfig>,
    pub dingtalk: Option<DingtalkRxConfig>,
    pub wecom: Option<WecomRxConfig>,
    pub qq_channel: Option<QqChannelRxConfig>,
}

pub struct FeishuRxConfig {
    pub rx: mpsc::Receiver<super::send::QueuedOutboundMessage>,
    pub app_id: String,
    pub app_secret: String,
}

pub struct DingtalkRxConfig {
    pub rx: mpsc::Receiver<super::send::QueuedOutboundMessage>,
    pub webhook_url: String,
}

pub struct WecomRxConfig {
    pub rx: mpsc::Receiver<super::send::QueuedOutboundMessage>,
    pub corp_id: String,
    pub corp_secret: String,
    pub agent_id: String,
    pub default_touser: String,
}

pub struct QqChannelRxConfig {
    pub rx: mpsc::Receiver<super::send::QueuedOutboundMessage>,
    pub app_id: String,
    pub app_secret: String,
    pub msg_id_cache: super::QqMsgIdCache,
    pub token_cache: super::SharedQqTokenCache,
}

/// Sender 二级队列深度。ESP 受内存限制为 8，Linux 有充足内存用 32。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const SENDER_QUEUE_DEPTH: usize = 8;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const SENDER_QUEUE_DEPTH: usize = 32;

/// 根据 config.enabled_channel 与凭证创建 ChannelSinks 并注册，返回 sinks 与各通道 rx 集合。
pub fn build_channel_sinks(
    config: &AppConfig,
    qq_msg_id_cache: &super::QqMsgIdCache,
    qq_token_cache: &super::SharedQqTokenCache,
) -> (ChannelSinks, ChannelRxSet) {
    let mut sinks = ChannelSinks::new();
    let enabled = config.enabled_channel.as_str();

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

    let dingtalk = if enabled == "dingtalk" && !config.dingtalk_webhook_url.trim().is_empty() {
        let (tx, rx) = mpsc::sync_channel::<super::send::QueuedOutboundMessage>(SENDER_QUEUE_DEPTH);
        sinks.register(
            "dingtalk",
            Box::new(QueuedSink::new(tx, "dingtalk_send_queue")),
        );
        Some(DingtalkRxConfig {
            rx,
            webhook_url: config.dingtalk_webhook_url.clone(),
        })
    } else {
        None
    };

    let wecom = if enabled == "wecom"
        && !config.wecom_corp_id.trim().is_empty()
        && !config.wecom_corp_secret.trim().is_empty()
        && config.wecom_agent_id.trim().parse::<u32>().is_ok()
    {
        let (tx, rx) = mpsc::sync_channel::<super::send::QueuedOutboundMessage>(SENDER_QUEUE_DEPTH);
        sinks.register("wecom", Box::new(QueuedSink::new(tx, "wecom_send_queue")));
        Some(WecomRxConfig {
            rx,
            corp_id: config.wecom_corp_id.clone(),
            corp_secret: config.wecom_corp_secret.clone(),
            agent_id: config.wecom_agent_id.clone(),
            default_touser: config.wecom_default_touser.clone(),
        })
    } else {
        None
    };

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

    sinks.register("websocket", Box::new(super::WebSocketSink::new("ws")));

    let rx_set = ChannelRxSet {
        telegram,
        feishu,
        dingtalk,
        wecom,
        qq_channel,
    };
    (sinks, rx_set)
}

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
    const TAG: &str = "beetle";

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
    if let Some(c) = rx_set.dingtalk.take() {
        let f = Arc::clone(&create_http);
        let dt_rx = c.rx;
        let dt_url = c.webhook_url;
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
                        super::run_dingtalk_sender_loop(dt_rx, &dt_url, move || f());
                    },
                )
            },
        )?;
    }
    if let Some(c) = rx_set.wecom.take() {
        let f = Arc::clone(&create_http);
        let wc_rx = c.rx;
        let wc_cid = c.corp_id;
        let wc_sec = c.corp_secret;
        let wc_aid = c.agent_id;
        let wc_usr = c.default_touser;
        spawn_sender_thread(
            TAG,
            "WeCom sender thread started",
            "wecom_sender_spawn",
            move || {
                crate::util::spawn_guarded_with_profile_handle(
                    "wc_sender",
                    STACK_CHANNEL_SENDER,
                    Some(crate::util::SpawnCore::Core0),
                    crate::util::HttpThreadRole::Io,
                    move || {
                        super::run_wecom_sender_loop(
                            wc_rx,
                            &wc_cid,
                            &wc_sec,
                            &wc_aid,
                            &wc_usr,
                            move || f(),
                        );
                    },
                )
            },
        )?;
    }
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
    use super::spawn_sender_thread;
    use crate::bus::{OutboundKind, PcMsg};
    use crate::error::{Error, Result};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn build_msg(channel: &str, chat_id: &str, content: &str) -> PcMsg {
        PcMsg::new(channel, chat_id, content).expect("pcmsg")
    }

    struct FailingSink {
        attempts: Arc<AtomicUsize>,
    }

    impl super::MessageSink for FailingSink {
        fn send(&self, _chat_id: &str, _content: &str) -> Result<()> {
            self.attempts.fetch_add(1, Ordering::Relaxed);
            Err(Error::config("failing_sink", "synthetic failure"))
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
        let mut sinks = super::ChannelSinks::new();
        sinks.register(
            "ready",
            Box::new(FailingSink {
                attempts: Arc::clone(&attempts),
            }),
        );

        assert!(!super::dispatch_via_sink(
            "channel_dispatch",
            &sinks,
            &msg,
            "supplemental"
        ));
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
    }
}
