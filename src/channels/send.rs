//! 通道出站 HTTP 发送与统一失败日志，供各通道 flush 使用。
//! Shared POST + log-on-failure for channel outbound; reduces duplicate match/log code.

use super::ChannelHttpClient;
use crate::error::Result;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

pub(crate) type QueuedOutboundMessage = (String, String, Option<String>);

pub(crate) const CHANNEL_SENDER_MAX_RETRIES: u8 = 3;
const CHANNEL_SENDER_RECV_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) enum SenderLoopEvent {
    Message(QueuedOutboundMessage),
    Timeout,
    Disconnected,
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

pub(crate) fn start_sender_loop(tag: &str) {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    crate::platform::task_wdt::register_current_task_to_task_wdt();
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
        Ok(message) => SenderLoopEvent::Message(message),
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
