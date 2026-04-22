//! 通道出站 HTTP 发送与统一失败日志，供各通道 flush 使用。
//! Shared POST + log-on-failure for channel outbound; reduces duplicate match/log code.

use super::ChannelHttpClient;
use crate::bus::{CanonicalMessageBody, OutboundKind};
use crate::error::{Error, Result};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

static NEXT_QUEUED_OUTBOUND_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedOutboundMessage {
    pub transport_send_id: u32,
    pub chat_id: String,
    pub content: String,
    pub body: CanonicalMessageBody,
    pub platform_thread_id: String,
    pub req_id: Option<String>,
    pub outbound_kind: OutboundKind,
}

pub(crate) const CHANNEL_SENDER_MAX_RETRIES: u8 = 3;
const CHANNEL_SENDER_RECV_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) enum SenderLoopEvent {
    Message(Box<QueuedOutboundMessage>),
    Timeout,
    Disconnected,
}

pub(crate) fn next_queued_outbound_id() -> u32 {
    NEXT_QUEUED_OUTBOUND_ID.fetch_add(1, Ordering::Relaxed)
}

/// 根据 POST 结果打一次 warn：Err 或 status >= 400。
pub fn log_send_failure(tag: &str, res: &Result<(u16, crate::platform::ResponseBody)>) {
    match res {
        Err(e) => log::warn!("[{}] send failed: {}", tag, e),
        Ok((status, _)) if *status >= 400 => log::warn!("[{}] send status={}", tag, status),
        _ => {}
    }
}

/// 执行 POST，失败时打日志，返回结果供需要解析 body 的调用方使用。
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
    if error.is_tls_admission() || error.is_connect_error() || error.is_retryable_upstream() {
        crate::orchestrator::observe_runtime_capability_failure(
            crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
            crate::orchestrator::RuntimeCapabilityReason::UpstreamUnavailable,
        );
    }
}

pub(crate) fn start_sender_loop(tag: &str) {
    log::info!("[{}] sender loop started", tag);
}

pub(crate) fn feed_sender_loop_wdt() {
    crate::platform::task_wdt::feed_current_task();
}

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

pub(crate) fn sleep_sender_retry_delay() {
    std::thread::sleep(Duration::from_secs(2));
    feed_sender_loop_wdt();
}

fn drain_sender_pending(
    rx: &Receiver<QueuedOutboundMessage>,
    pending: &mut VecDeque<QueuedOutboundMessage>,
) {
    while let Ok(message) = rx.try_recv() {
        pending.push_back(message);
    }
}

pub(crate) fn run_buffered_sender_loop<SendOne>(
    rx: Receiver<QueuedOutboundMessage>,
    tag: &'static str,
    mut send_one: SendOne,
) where
    SendOne: FnMut(&QueuedOutboundMessage, u8) -> crate::error::Result<()>,
{
    start_sender_loop(tag);

    let mut pending = VecDeque::with_capacity(4);
    loop {
        if pending.is_empty() {
            match recv_sender_loop_event(&rx, tag) {
                SenderLoopEvent::Message(message) => pending.push_back(*message),
                SenderLoopEvent::Timeout => continue,
                SenderLoopEvent::Disconnected => break,
            }
        }

        drain_sender_pending(&rx, &mut pending);
        let Some(message) = pending.pop_front() else {
            continue;
        };
        feed_sender_loop_wdt();

        let max_retries = if message.outbound_kind.is_supplemental() {
            1
        } else {
            CHANNEL_SENDER_MAX_RETRIES
        };
        let mut sent = false;
        for retry in 0..max_retries {
            let attempt = retry + 1;
            if retry > 0 {
                sleep_sender_retry_delay();
            }
            match send_one(&message, attempt) {
                Ok(()) => {
                    sent = true;
                    break;
                }
                Err(error) => {
                    if matches!(error, Error::Config { .. }) {
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
            } else {
                log_sender_drop(
                    tag,
                    message.req_id.as_deref(),
                    Some(message.chat_id.as_str()),
                    max_retries,
                );
            }
        }
    }
}

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

pub(crate) fn log_sender_drop(
    tag: &str,
    req_id: Option<&str>,
    chat_id: Option<&str>,
    max_retries: u8,
) {
    if let Some(chat_id) = chat_id {
        log::error!(
            "[{}] message dropped after {} retries, chat_id={}",
            tag,
            max_retries,
            chat_id
        );
    } else {
        log::error!("[{}] message dropped after {} retries", tag, max_retries);
    }
    log::error!(
        "[{}] req_id={} message dropped after retries",
        tag,
        req_id.unwrap_or("-")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use std::sync::Mutex;

    static TEST_GUARD: Mutex<()> = Mutex::new(());

    #[test]
    fn outbound_http_failure_records_tls_admission_and_marks_capability_offline() {
        let _guard = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
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
            crate::orchestrator::RuntimeCapabilityStatus::Offline
        );
        assert_eq!(
            capability.reason,
            crate::orchestrator::RuntimeCapabilityReason::UpstreamUnavailable
        );
    }

    #[test]
    fn outbound_http_success_restores_capability_online() {
        let _guard = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
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
        let _guard = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
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
        let _guard = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        tx.send(QueuedOutboundMessage {
            transport_send_id: next_queued_outbound_id(),
            chat_id: "chat-a".to_string(),
            content: "first".to_string(),
            body: CanonicalMessageBody::text("first"),
            platform_thread_id: String::new(),
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
        let _guard = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        tx.send(QueuedOutboundMessage {
            transport_send_id: next_queued_outbound_id(),
            chat_id: "chat-a".to_string(),
            content: "broken".to_string(),
            body: CanonicalMessageBody::text("broken"),
            platform_thread_id: String::new(),
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
    fn buffered_sender_loop_does_not_retry_supplemental_message() {
        let _guard = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        tx.send(QueuedOutboundMessage {
            transport_send_id: next_queued_outbound_id(),
            chat_id: "chat-a".to_string(),
            content: "supplemental".to_string(),
            body: CanonicalMessageBody::text("supplemental"),
            platform_thread_id: String::new(),
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
}
