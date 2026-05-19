//! 通道出站 HTTP 发送与统一失败日志，供各通道 flush 使用。
//! Shared POST + log-on-failure for channel outbound; reduces duplicate match/log code.

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
use super::ChannelHttpClient;
use crate::bus::{CanonicalMessageBody, OutboundKind};
#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
use crate::error::{Error, Result};
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
use std::sync::mpsc::{Receiver, RecvTimeoutError};
#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
use std::time::Duration;

static NEXT_QUEUED_OUTBOUND_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedOutboundMessage {
    pub transport_send_id: u32,
    pub chat_id: String,
    pub content: String,
    pub body: CanonicalMessageBody,
    pub platform_thread_id: String,
    pub platform_message_id: String,
    pub req_id: Option<String>,
    pub outbound_kind: OutboundKind,
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
pub(crate) const CHANNEL_SENDER_MAX_RETRIES: u8 = 3;
#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
const CHANNEL_SENDER_RECV_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(any(feature = "telegram", feature = "feishu", feature = "qq_channel", test))]
pub(crate) trait ActiveChannelSender {
    fn tag(&self) -> &'static str;
    fn send_attempt(
        &mut self,
        message: &QueuedOutboundMessage,
        attempt: u8,
    ) -> crate::error::Result<()>;
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
pub(crate) fn max_retries_for_message(message: &QueuedOutboundMessage) -> u8 {
    if message.outbound_kind == OutboundKind::Supplemental {
        1
    } else {
        CHANNEL_SENDER_MAX_RETRIES
    }
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
pub(crate) fn reply_http_priority_for_message_kind(
    kind: OutboundKind,
) -> crate::orchestrator::Priority {
    if kind == OutboundKind::Primary || kind == OutboundKind::Visibility {
        crate::orchestrator::Priority::Critical
    } else {
        crate::orchestrator::Priority::Normal
    }
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
pub(crate) fn begin_reply_http_priority_scope(
    kind: OutboundKind,
) -> Option<crate::orchestrator::HttpPriorityOverrideGuard> {
    if reply_http_priority_for_message_kind(kind) == crate::orchestrator::Priority::Critical {
        Some(crate::orchestrator::begin_reply_critical_http_scope())
    } else {
        None
    }
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
fn should_defer_primary_send_error(error: &Error) -> bool {
    should_defer_primary_send_error_immediately(error) || error.is_retryable_upstream()
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
fn should_defer_primary_send_error_immediately(error: &Error) -> bool {
    error.is_tls_admission() || error.is_connect_error()
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
pub(crate) enum SenderLoopEvent {
    Message(Box<QueuedOutboundMessage>),
    Timeout,
    Disconnected,
}

pub(crate) fn next_queued_outbound_id() -> u32 {
    NEXT_QUEUED_OUTBOUND_ID.fetch_add(1, Ordering::Relaxed)
}

/// 根据 POST 结果打一次 warn：Err 或 status >= 400。
#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
))]
pub fn log_send_failure(tag: &str, res: &Result<(u16, crate::platform::ResponseBody)>) {
    match res {
        Err(e) => log::warn!("[{}] send failed: {}", tag, e),
        Ok((status, _)) if *status >= 400 => log::warn!("[{}] send status={}", tag, status),
        _ => {}
    }
}

/// 执行 POST，失败时打日志，返回结果供需要解析 body 的调用方使用。
#[cfg(any(feature = "telegram", feature = "dingtalk"))]
pub fn send_post<H: ChannelHttpClient>(
    tag: &str,
    http: &mut H,
    url: &str,
    body: &[u8],
) -> Result<(u16, crate::platform::ResponseBody)> {
    let res = http.http_post(url, body);
    log_send_failure(tag, &res);
    res
}

/// 执行带 headers 的 POST，失败时打日志，返回结果。
#[cfg(any(feature = "feishu", feature = "qq_channel"))]
pub fn send_post_with_headers<H: ChannelHttpClient>(
    tag: &str,
    http: &mut H,
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<(u16, crate::platform::ResponseBody)> {
    let res = http.http_post_with_headers(url, headers, body);
    log_send_failure(tag, &res);
    res
}

pub(crate) fn record_outbound_http_success() {
    crate::metrics::record_channel_http_result(true);
    crate::orchestrator::observe_runtime_capability_success(&[
        crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
    ]);
}

pub(crate) fn record_outbound_http_failure(error: &crate::error::Error) {
    crate::metrics::record_channel_http_result(false);
    crate::metrics::record_error_by_stage(error.metrics_stage());
    if let Some(reason) = outbound_http_failure_capability_reason(error) {
        crate::orchestrator::observe_runtime_capability_failure(
            crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            reason,
        );
    }
}

fn outbound_http_failure_capability_reason(
    error: &crate::error::Error,
) -> Option<crate::orchestrator::RuntimeCapabilityReason> {
    if error.is_tls_admission() {
        Some(crate::orchestrator::RuntimeCapabilityReason::RecoveryStabilizing)
    } else if error.is_connect_error() || error.is_retryable_upstream() {
        Some(crate::orchestrator::RuntimeCapabilityReason::UpstreamUnavailable)
    } else {
        None
    }
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
pub(crate) fn start_sender_loop(tag: &str) {
    log::info!("[{}] sender loop started", tag);
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
pub(crate) fn feed_sender_loop_wdt() {
    crate::platform::task_wdt::feed_current_task();
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
pub(crate) fn recv_sender_loop_event(
    rx: &Receiver<QueuedOutboundMessage>,
    tag: &str,
) -> SenderLoopEvent {
    match rx.recv_timeout(CHANNEL_SENDER_RECV_TIMEOUT) {
        Ok(message) => SenderLoopEvent::Message(Box::new(message)),
        Err(RecvTimeoutError::Timeout) => {
            feed_sender_loop_wdt();
            SenderLoopEvent::Timeout
        }
        Err(RecvTimeoutError::Disconnected) => {
            log::info!("[{}] rx disconnected, exiting", tag);
            SenderLoopEvent::Disconnected
        }
    }
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
pub(crate) fn sleep_sender_retry_delay() {
    std::thread::sleep(Duration::from_secs(2));
    feed_sender_loop_wdt();
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
pub(crate) fn run_buffered_sender_loop<SendOne>(
    rx: Receiver<QueuedOutboundMessage>,
    tag: &'static str,
    mut send_one: SendOne,
) where
    SendOne: FnMut(&QueuedOutboundMessage, u8) -> crate::error::Result<()>,
{
    start_sender_loop(tag);

    let mut pending: Option<QueuedOutboundMessage> = None;
    loop {
        let message = if let Some(message) = pending.take() {
            message
        } else {
            match recv_sender_loop_event(&rx, tag) {
                SenderLoopEvent::Message(message) => *message,
                SenderLoopEvent::Timeout => continue,
                SenderLoopEvent::Disconnected => break,
            }
        };
        feed_sender_loop_wdt();

        let max_retries = max_retries_for_message(&message);
        let mut sent = false;
        let mut last_err = None;
        let mut attempts = 0u8;
        for retry in 0..max_retries {
            let attempt = retry + 1;
            attempts = attempt;
            if retry > 0 {
                sleep_sender_retry_delay();
            }
            let _reply_priority = begin_reply_http_priority_scope(message.outbound_kind);
            match send_one(&message, attempt) {
                Ok(()) => {
                    sent = true;
                    break;
                }
                Err(error) => {
                    let should_defer = should_defer_primary_send_error_immediately(&error);
                    last_err = Some(error);
                    if should_defer || matches!(last_err.as_ref(), Some(Error::Config { .. })) {
                        break;
                    }
                }
            }
        }
        if !sent {
            if message.outbound_kind.is_supplemental() {
                log::warn!(
                    "[{}] supplemental dropped after send failure req_id={} chat_id={}",
                    tag,
                    message.req_id.as_deref().unwrap_or("-"),
                    message.chat_id
                );
            } else if last_err
                .as_ref()
                .is_some_and(should_defer_primary_send_error)
            {
                log::warn!(
                    "[{}] primary deferred after retryable send failure req_id={} chat_id={}",
                    tag,
                    message.req_id.as_deref().unwrap_or("-"),
                    message.chat_id
                );
                pending = Some(message);
                sleep_sender_retry_delay();
            } else {
                log_sender_drop(
                    tag,
                    message.req_id.as_deref(),
                    Some(message.chat_id.as_str()),
                    attempts,
                );
            }
        }
    }
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
pub(crate) fn ensure_sender_http<H, F>(
    http: &mut Option<H>,
    create_http: &mut F,
    tag: &str,
    attempt: u8,
) -> bool
where
    H: ChannelHttpClient,
    F: FnMut() -> crate::error::Result<H>,
{
    if http.is_some() {
        return true;
    }
    match create_http() {
        Ok(client) => {
            *http = Some(client);
            true
        }
        Err(error) => {
            record_outbound_http_failure(&error);
            log::warn!(
                "[{}] create http failed (attempt {}): {}",
                tag,
                attempt,
                error
            );
            false
        }
    }
}

#[cfg(any(
    feature = "telegram",
    feature = "dingtalk",
    feature = "feishu",
    feature = "qq_channel",
    test
))]
pub(crate) fn log_sender_drop(
    tag: &str,
    req_id: Option<&str>,
    chat_id: Option<&str>,
    attempts: u8,
) {
    if let Some(chat_id) = chat_id {
        log::error!(
            "[{}] message dropped after {} send attempts, chat_id={}",
            tag,
            attempts,
            chat_id
        );
    } else {
        log::error!("[{}] message dropped after {} send attempts", tag, attempts);
    }
    log::error!(
        "[{}] req_id={} message dropped after send attempts",
        tag,
        req_id.unwrap_or("-")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use std::sync::Mutex;

    #[test]
    fn outbound_http_failure_records_tls_admission_as_local_recovery() {
        let _guard = crate::orchestrator::runtime_capability::RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::orchestrator::reset_runtime_capabilities_for_tests();
        let before = crate::metrics::snapshot();
        let err = Error::config("tls_admission", "permit timeout");

        record_outbound_http_failure(&err);

        let after = crate::metrics::snapshot();
        let capability = crate::orchestrator::get_runtime_capability(
            crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
        )
        .expect("capability");
        assert!(after.channel_http_fail > before.channel_http_fail);
        assert!(after.errors_tls_admission > before.errors_tls_admission);
        assert_eq!(
            capability.status,
            crate::orchestrator::RuntimeCapabilityStatus::Degraded
        );
        assert_eq!(
            capability.reason,
            crate::orchestrator::RuntimeCapabilityReason::RecoveryStabilizing
        );
    }

    #[test]
    fn outbound_http_success_restores_capability_online() {
        let _guard = crate::orchestrator::runtime_capability::RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::orchestrator::reset_runtime_capabilities_for_tests();
        crate::orchestrator::observe_runtime_capability_failure(
            crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            crate::orchestrator::RuntimeCapabilityReason::UpstreamUnavailable,
        );
        let before = crate::metrics::snapshot();

        record_outbound_http_success();

        let after = crate::metrics::snapshot();
        let capability = crate::orchestrator::get_runtime_capability(
            crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
        )
        .expect("capability");
        assert!(after.channel_http_ok > before.channel_http_ok);
        assert_eq!(
            capability.status,
            crate::orchestrator::RuntimeCapabilityStatus::Online
        );
        assert_eq!(
            capability.reason,
            crate::orchestrator::RuntimeCapabilityReason::Nominal
        );
    }

    #[test]
    fn wrapped_tls_admission_failure_records_root_stage() {
        let _guard = crate::orchestrator::runtime_capability::RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::orchestrator::reset_runtime_capabilities_for_tests();
        let before = crate::metrics::snapshot();
        let err = Error::Other {
            source: Box::new(Error::config("tls_admission", "permit timeout")),
            stage: "telegram_send",
        };

        record_outbound_http_failure(&err);

        let after = crate::metrics::snapshot();
        assert!(after.channel_http_fail > before.channel_http_fail);
        assert!(after.errors_tls_admission > before.errors_tls_admission);
    }

    #[test]
    fn buffered_sender_loop_retries_failed_drained_message_instead_of_losing_it() {
        let _guard = crate::orchestrator::runtime_capability::RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        tx.send(QueuedOutboundMessage {
            transport_send_id: next_queued_outbound_id(),
            chat_id: "chat-a".to_string(),
            content: "first".to_string(),
            body: CanonicalMessageBody::text("first"),
            platform_thread_id: String::new(),
            platform_message_id: String::new(),
            req_id: None,
            outbound_kind: OutboundKind::Primary,
        })
        .expect("send first");
        tx.send(QueuedOutboundMessage {
            transport_send_id: next_queued_outbound_id(),
            chat_id: "chat-b".to_string(),
            content: "second".to_string(),
            body: CanonicalMessageBody::text("second"),
            platform_thread_id: String::new(),
            platform_message_id: String::new(),
            req_id: None,
            outbound_kind: OutboundKind::Primary,
        })
        .expect("send second");
        tx.send(QueuedOutboundMessage {
            transport_send_id: next_queued_outbound_id(),
            chat_id: "chat-c".to_string(),
            content: "third".to_string(),
            body: CanonicalMessageBody::text("third"),
            platform_thread_id: String::new(),
            platform_message_id: String::new(),
            req_id: None,
            outbound_kind: OutboundKind::Primary,
        })
        .expect("send third");
        drop(tx);

        let seen = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let seen_clone = std::sync::Arc::clone(&seen);

        run_buffered_sender_loop(rx, "test_sender", move |message, attempt| {
            let mut guard = seen_clone.lock().unwrap_or_else(|e| e.into_inner());
            guard.push(format!("{}:{}", message.content, attempt));
            if message.content == "second" && attempt == 1 {
                return Err(Error::http("test_sender", 500));
            }
            Ok(())
        });

        let guard = seen.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            guard.as_slice(),
            ["first:1", "second:1", "second:2", "third:1"]
        );
    }

    #[test]
    fn buffered_sender_loop_does_not_retry_config_error() {
        let _guard = crate::orchestrator::runtime_capability::RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        tx.send(QueuedOutboundMessage {
            transport_send_id: next_queued_outbound_id(),
            chat_id: "chat-a".to_string(),
            content: "broken".to_string(),
            body: CanonicalMessageBody::text("broken"),
            platform_thread_id: String::new(),
            platform_message_id: String::new(),
            req_id: None,
            outbound_kind: OutboundKind::Primary,
        })
        .expect("send broken");
        drop(tx);

        let attempts = std::sync::Arc::new(Mutex::new(Vec::<u8>::new()));
        let attempts_clone = std::sync::Arc::clone(&attempts);

        run_buffered_sender_loop(rx, "test_sender", move |_message, attempt| {
            attempts_clone
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(attempt);
            Err(Error::config("test_sender", "deterministic config failure"))
        });

        let attempts = attempts.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(attempts.as_slice(), &[1]);
    }

    #[test]
    fn buffered_sender_loop_defers_primary_tls_admission_failure() {
        let _guard = crate::orchestrator::runtime_capability::RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        tx.send(QueuedOutboundMessage {
            transport_send_id: next_queued_outbound_id(),
            chat_id: "chat-a".to_string(),
            content: "reply".to_string(),
            body: CanonicalMessageBody::text("reply"),
            platform_thread_id: String::new(),
            platform_message_id: String::new(),
            req_id: Some("req-1".to_string()),
            outbound_kind: OutboundKind::Primary,
        })
        .expect("send reply");
        drop(tx);

        let seen = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let seen_clone = std::sync::Arc::clone(&seen);

        run_buffered_sender_loop(rx, "test_sender", move |message, attempt| {
            let mut guard = seen_clone.lock().unwrap_or_else(|e| e.into_inner());
            guard.push(format!("{}:{attempt}", message.content));
            if guard.len() == 1 {
                return Err(Error::config("tls_admission", "largest block too small"));
            }
            Ok(())
        });

        let seen = seen.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(seen.as_slice(), ["reply:1", "reply:1"]);
    }

    #[test]
    fn buffered_sender_loop_keeps_retryable_primary_bounded_in_sync_channel() {
        let _guard = crate::orchestrator::runtime_capability::RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        tx.send(QueuedOutboundMessage {
            transport_send_id: next_queued_outbound_id(),
            chat_id: "chat-a".to_string(),
            content: "first".to_string(),
            body: CanonicalMessageBody::text("first"),
            platform_thread_id: String::new(),
            platform_message_id: String::new(),
            req_id: Some("req-1".to_string()),
            outbound_kind: OutboundKind::Primary,
        })
        .expect("send first");

        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let release = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let attempts_clone = std::sync::Arc::clone(&attempts);
        let release_clone = std::sync::Arc::clone(&release);
        let worker = std::thread::spawn(move || {
            run_buffered_sender_loop(rx, "test_sender", move |_message, _attempt| {
                attempts_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if release_clone.load(std::sync::atomic::Ordering::SeqCst) {
                    Err(Error::config("test_sender", "release retry loop"))
                } else {
                    Err(Error::config("tls_admission", "largest block too small"))
                }
            });
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while attempts.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "sender should attempt the first message"
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        tx.send(QueuedOutboundMessage {
            transport_send_id: next_queued_outbound_id(),
            chat_id: "chat-b".to_string(),
            content: "second".to_string(),
            body: CanonicalMessageBody::text("second"),
            platform_thread_id: String::new(),
            platform_message_id: String::new(),
            req_id: Some("req-2".to_string()),
            outbound_kind: OutboundKind::Primary,
        })
        .expect("second should fit in the bounded sync channel");

        std::thread::sleep(Duration::from_millis(100));
        let third = QueuedOutboundMessage {
            transport_send_id: next_queued_outbound_id(),
            chat_id: "chat-c".to_string(),
            content: "third".to_string(),
            body: CanonicalMessageBody::text("third"),
            platform_thread_id: String::new(),
            platform_message_id: String::new(),
            req_id: Some("req-3".to_string()),
            outbound_kind: OutboundKind::Primary,
        };
        assert!(
            matches!(
                tx.try_send(third),
                Err(std::sync::mpsc::TrySendError::Full(_))
            ),
            "retryable primary failure must not drain the bounded sender queue into heap"
        );

        release.store(true, std::sync::atomic::Ordering::SeqCst);
        drop(tx);
        worker
            .join()
            .expect("sender loop should exit after release");
    }

    #[test]
    fn buffered_sender_loop_does_not_retry_supplemental_message() {
        let _guard = crate::orchestrator::runtime_capability::RUNTIME_CAPABILITY_TEST_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        tx.send(QueuedOutboundMessage {
            transport_send_id: next_queued_outbound_id(),
            chat_id: "chat-a".to_string(),
            content: "supplemental".to_string(),
            body: CanonicalMessageBody::text("supplemental"),
            platform_thread_id: String::new(),
            platform_message_id: String::new(),
            req_id: Some("req-1".to_string()),
            outbound_kind: OutboundKind::Supplemental,
        })
        .expect("send supplemental");
        drop(tx);

        let attempts = std::sync::Arc::new(Mutex::new(Vec::<u8>::new()));
        let attempts_clone = std::sync::Arc::clone(&attempts);

        run_buffered_sender_loop(rx, "test_sender", move |_message, attempt| {
            attempts_clone
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(attempt);
            Err(Error::config("test_sender", "synthetic failure"))
        });

        let attempts = attempts.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(attempts.as_slice(), [1]);
    }

    #[test]
    fn visibility_message_uses_primary_retry_budget_and_critical_priority() {
        let message = QueuedOutboundMessage {
            transport_send_id: next_queued_outbound_id(),
            chat_id: "chat-a".to_string(),
            content: "已收到，正在处理".to_string(),
            body: CanonicalMessageBody::text("已收到，正在处理"),
            platform_thread_id: String::new(),
            platform_message_id: String::new(),
            req_id: Some("req-visibility".to_string()),
            outbound_kind: OutboundKind::Visibility,
        };

        assert_eq!(
            max_retries_for_message(&message),
            CHANNEL_SENDER_MAX_RETRIES
        );
        assert_eq!(
            reply_http_priority_for_message_kind(message.outbound_kind),
            crate::orchestrator::Priority::Critical
        );
    }
}
