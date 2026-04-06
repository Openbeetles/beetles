//! 企业微信通道：出站经 MessageSink 队列，由 main 用 HTTP 鉴权后发送应用消息；入站无。
//! 鉴权 GET gettoken，发送 POST message/send；text 按 2048 字节分片（官方限制）。Sink 统一为 dispatch::QueuedSink。

use crate::channels::send::{
    ensure_sender_http, feed_sender_loop_wdt, log_sender_drop, recv_sender_loop_event,
    sleep_sender_retry_delay, start_sender_loop, SenderLoopEvent, CHANNEL_SENDER_MAX_RETRIES,
};
use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;

pub const WECOM_GETTOKEN_BASE: &str = "https://qyapi.weixin.qq.com/cgi-bin/gettoken";
pub const WECOM_SEND_BASE: &str = "https://qyapi.weixin.qq.com/cgi-bin/message/send";
/// 企业微信 text 消息 content 最大 2048 字节（官方文档）。
const WECOM_MAX_TEXT_BYTES: usize = 2048;

#[derive(serde::Deserialize)]
pub struct WecomTokenResponse {
    #[serde(default)]
    pub errcode: i32,
    #[serde(default)]
    pub errmsg: String,
    pub access_token: Option<String>,
    /// 秒；缺省为 0 时按 7200 处理（与官方 gettoken 一致）。
    #[serde(default)]
    pub expires_in: u64,
}

#[derive(serde::Deserialize)]
pub struct WecomSendResponse {
    #[serde(default)]
    pub errcode: i32,
    #[serde(default)]
    pub errmsg: String,
}

const CONNECTIVITY_MESSAGE: &str = "BOT, Hello";

/// 连通性检查：供 GET /api/channel_connectivity 使用。
pub fn check_connectivity<H: ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
    loc: crate::i18n::Locale,
) -> super::super::connectivity::ChannelConnectivityItem {
    use super::super::connectivity;
    use crate::i18n::{tr, Message};
    let configured = !config.wecom_corp_id.trim().is_empty()
        && !config.wecom_corp_secret.trim().is_empty()
        && config.wecom_agent_id.trim().parse::<u32>().is_ok();
    if !configured {
        return connectivity::item(
            "wecom",
            false,
            false,
            Some(tr(Message::ConnectivityNotConfigured, loc)),
        );
    }
    let url = format!(
        "{}?corpid={}&corpsecret={}",
        WECOM_GETTOKEN_BASE,
        config.wecom_corp_id.trim(),
        config.wecom_corp_secret.trim()
    );
    let (status, resp_body) = match http.http_get(&url) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[wecom_connectivity] gettoken http: {}", e);
            return connectivity::item(
                "wecom",
                configured,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            );
        }
    };
    if status >= 400 {
        log::warn!("[wecom_connectivity] gettoken status {}", status);
        return connectivity::item(
            "wecom",
            configured,
            false,
            Some(tr(Message::ConnectivityTokenInvalid, loc)),
        );
    }
    let r: WecomTokenResponse = match serde_json::from_slice(resp_body.as_ref()) {
        Ok(x) => x,
        Err(e) => {
            log::warn!("[wecom_connectivity] gettoken parse: {}", e);
            return connectivity::item(
                "wecom",
                configured,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            );
        }
    };
    if r.errcode != 0 {
        log::warn!(
            "[wecom_connectivity] gettoken errcode {} {}",
            r.errcode,
            r.errmsg
        );
        return connectivity::item(
            "wecom",
            configured,
            false,
            Some(tr(Message::ConnectivityTokenInvalid, loc)),
        );
    }
    let token = match r.access_token {
        Some(t) if !t.is_empty() => t,
        _ => {
            log::warn!("[wecom_connectivity] no access_token in response");
            return connectivity::item(
                "wecom",
                configured,
                false,
                Some(tr(Message::ConnectivityTokenInvalid, loc)),
            );
        }
    };
    let touser = config.wecom_default_touser.trim();
    if touser.is_empty() {
        return connectivity::item("wecom", configured, true, None);
    }
    let agent_id_u32: u32 = match config.wecom_agent_id.trim().parse() {
        Ok(n) => n,
        Err(_) => {
            log::warn!("[wecom_connectivity] invalid agent_id parse");
            return connectivity::item(
                "wecom",
                configured,
                false,
                Some(tr(Message::ConnectivityTokenInvalid, loc)),
            );
        }
    };
    let body = serde_json::json!({
        "touser": touser,
        "msgtype": "text",
        "agentid": agent_id_u32,
        "text": { "content": CONNECTIVITY_MESSAGE }
    });
    let body_bytes = match serde_json::to_vec(&body) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[wecom_connectivity] send json: {}", e);
            return connectivity::item(
                "wecom",
                configured,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            );
        }
    };
    let send_url = format!("{}?access_token={}", WECOM_SEND_BASE, token);
    let (status, resp_body) = match http.http_post(&send_url, &body_bytes) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[wecom_connectivity] send http: {}", e);
            return connectivity::item(
                "wecom",
                configured,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            );
        }
    };
    if status >= 400 {
        log::warn!("[wecom_connectivity] send status {}", status);
        return connectivity::item(
            "wecom",
            configured,
            false,
            Some(tr(Message::ConnectivityCheckFailed, loc)),
        );
    }
    let send_r: WecomSendResponse = match serde_json::from_slice(resp_body.as_ref()) {
        Ok(x) => x,
        Err(e) => {
            log::warn!("[wecom_connectivity] send parse: {}", e);
            return connectivity::item(
                "wecom",
                configured,
                false,
                Some(tr(Message::ConnectivityCheckFailed, loc)),
            );
        }
    };
    if send_r.errcode != 0 {
        log::warn!(
            "[wecom_connectivity] send errcode {} {}",
            send_r.errcode,
            send_r.errmsg
        );
        return connectivity::item(
            "wecom",
            configured,
            false,
            Some(tr(Message::ConnectivityCheckFailed, loc)),
        );
    }
    connectivity::item("wecom", configured, true, None)
}

/// Returns `(access_token, expires_in_secs)` for sender-loop caching.
fn acquire_wecom_token_with_expiry<H: ChannelHttpClient>(
    http: &mut H,
    corp_id: &str,
    corp_secret: &str,
) -> Option<(String, u64)> {
    const TAG: &str = "wecom_send";
    let url = format!(
        "{}?corpid={}&corpsecret={}",
        WECOM_GETTOKEN_BASE, corp_id, corp_secret
    );
    let (status, resp_body) = match http.http_get(&url) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[{}] gettoken failed: {}", TAG, e);
            return None;
        }
    };
    if status >= 400 {
        log::warn!("[{}] gettoken status={}", TAG, status);
        return None;
    }
    let token_resp: WecomTokenResponse = match serde_json::from_slice(resp_body.as_ref()) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("[{}] gettoken parse: {}", TAG, e);
            return None;
        }
    };
    if token_resp.errcode != 0 {
        log::warn!(
            "[{}] gettoken errcode={} errmsg={}",
            TAG,
            token_resp.errcode,
            token_resp.errmsg
        );
        return None;
    }
    match token_resp.access_token {
        Some(t) if !t.is_empty() => {
            let exp_secs = if token_resp.expires_in == 0 {
                7200
            } else {
                token_resp.expires_in
            };
            Some((t, exp_secs.max(60)))
        }
        _ => {
            log::warn!("[{}] gettoken empty access_token", TAG);
            None
        }
    }
}

fn acquire_wecom_token<H: ChannelHttpClient>(
    http: &mut H,
    corp_id: &str,
    corp_secret: &str,
) -> Option<String> {
    acquire_wecom_token_with_expiry(http, corp_id, corp_secret).map(|(t, _)| t)
}

fn send_one_wecom<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    agent_id_u32: u32,
    chat_id: &str,
    default_touser: &str,
    content: &str,
) -> crate::error::Result<()> {
    const TAG: &str = "wecom_send";
    if content.trim().is_empty() {
        return Err(crate::error::Error::config(
            "wecom_send",
            "refusing to send empty WeCom message",
        ));
    }
    let touser = if chat_id.trim().is_empty() {
        default_touser
    } else {
        chat_id.trim()
    };
    if touser.is_empty() {
        return Ok(());
    }
    for chunk in crate::channels::chunk::chunk_text_by_utf8_bytes(content, WECOM_MAX_TEXT_BYTES) {
        let body = serde_json::json!({
            "touser": touser,
            "msgtype": "text",
            "agentid": agent_id_u32,
            "text": { "content": chunk }
        });
        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| crate::error::Error::config("wecom_send", e.to_string()))?;
        let send_url = format!("{}?access_token={}", WECOM_SEND_BASE, token);
        let (status, resp_body) =
            crate::channels::send::send_post(TAG, http, &send_url, &body_bytes)?;
        if status >= 400 {
            return Err(crate::error::Error::http("wecom_send", status));
        }
        if let Ok(resp) = serde_json::from_slice::<WecomSendResponse>(resp_body.as_ref()) {
            if resp.errcode != 0 {
                log::warn!(
                    "[{}] send errcode={} errmsg={}",
                    TAG,
                    resp.errcode,
                    resp.errmsg
                );
                return Err(crate::error::Error::config(
                    "wecom_send",
                    format!("errcode={} errmsg={}", resp.errcode, resp.errmsg),
                ));
            }
        }
    }
    Ok(())
}

/// 从 rx 取出待发送（一次性 drain）。
pub fn flush_wecom_sends<H: ChannelHttpClient>(
    rx: &std::sync::mpsc::Receiver<(String, String, Option<String>)>,
    corp_id: &str,
    corp_secret: &str,
    agent_id: &str,
    default_touser: &str,
    http: &mut H,
) {
    if corp_id.is_empty() || corp_secret.is_empty() {
        return;
    }
    let agent_id_u32: u32 = match agent_id.parse() {
        Ok(n) => n,
        Err(_) => {
            log::warn!("[wecom_send] invalid agent_id");
            return;
        }
    };
    let token = match acquire_wecom_token(http, corp_id, corp_secret) {
        Some(t) => t,
        None => return,
    };
    while let Ok((chat_id, content, _req_id)) = rx.try_recv() {
        if let Err(error) = send_one_wecom(
            http,
            &token,
            agent_id_u32,
            &chat_id,
            default_touser,
            &content,
        ) {
            log::warn!(
                "[wecom_flush] send failed for chat_id={}: {}",
                chat_id,
                error
            );
        }
    }
}

/// access_token 缓存提前刷新余量（秒）。
const WECOM_TOKEN_CACHE_MARGIN_SECS: u64 = 120;

/// 持续运行的企业微信发送循环：sender 线程内**复用** HTTP，并按 `expires_in` **缓存** token。
pub fn run_wecom_sender_loop<H, F>(
    rx: std::sync::mpsc::Receiver<(String, String, Option<String>)>,
    corp_id: &str,
    corp_secret: &str,
    agent_id: &str,
    default_touser: &str,
    mut create_http: F,
) where
    H: ChannelHttpClient,
    F: FnMut() -> crate::error::Result<H>,
{
    const TAG: &str = "wecom_sender";
    if corp_id.is_empty() || corp_secret.is_empty() {
        return;
    }
    let agent_id_u32: u32 = match agent_id.parse() {
        Ok(n) => n,
        Err(_) => {
            log::warn!("[{}] invalid agent_id", TAG);
            return;
        }
    };
    start_sender_loop(TAG);

    let mut http: Option<H> = None;
    let mut token_cache: Option<(String, std::time::Instant)> = None;
    loop {
        let (chat_id, content, req_id) = match recv_sender_loop_event(&rx, TAG) {
            SenderLoopEvent::Message(item) => item,
            SenderLoopEvent::Timeout => continue,
            SenderLoopEvent::Disconnected => break,
        };
        feed_sender_loop_wdt();
        let mut sent = false;
        for retry in 0..CHANNEL_SENDER_MAX_RETRIES {
            if retry > 0 {
                sleep_sender_retry_delay();
            }

            let now = std::time::Instant::now();
            let mut token_opt: Option<String> = token_cache
                .as_ref()
                .filter(|(_, exp)| now < *exp)
                .map(|(t, _)| t.clone());
            if token_opt.is_none() {
                token_cache = None;
                if !ensure_sender_http(&mut http, &mut create_http, TAG, retry + 1) {
                    continue;
                }
                let Some(h) = http.as_mut() else {
                    continue;
                };
                match acquire_wecom_token_with_expiry(h, corp_id, corp_secret) {
                    Some((t, exp_secs)) => {
                        let keep = exp_secs
                            .saturating_sub(WECOM_TOKEN_CACHE_MARGIN_SECS)
                            .max(30);
                        token_cache = Some((t.clone(), now + std::time::Duration::from_secs(keep)));
                        token_opt = Some(t);
                    }
                    None => {
                        http = None;
                        continue;
                    }
                }
            }

            let token = match token_opt {
                Some(t) => t,
                None => continue,
            };

            if !ensure_sender_http(&mut http, &mut create_http, TAG, retry + 1) {
                continue;
            }
            let Some(h) = http.as_mut() else {
                continue;
            };
            match send_one_wecom(h, &token, agent_id_u32, &chat_id, default_touser, &content) {
                Ok(()) => crate::metrics::record_channel_http_result(true),
                Err(error) => {
                    crate::metrics::record_channel_http_result(false);
                    log::warn!(
                        "[{}] send failed (attempt {}), chat_id={}: {}",
                        TAG,
                        retry + 1,
                        chat_id,
                        error
                    );
                    token_cache = None;
                    http = None;
                    continue;
                }
            }
            while let Ok((cid, cnt, _)) = rx.try_recv() {
                if let Err(error) =
                    send_one_wecom(h, &token, agent_id_u32, &cid, default_touser, &cnt)
                {
                    crate::metrics::record_channel_http_result(false);
                    log::warn!("[{}] drain send failed for chat_id={}: {}", TAG, cid, error);
                    break;
                }
                crate::metrics::record_channel_http_result(true);
            }
            sent = true;
            break;
        }
        if !sent {
            log_sender_drop(
                TAG,
                req_id.as_deref(),
                Some(chat_id.as_str()),
                CHANNEL_SENDER_MAX_RETRIES,
            );
        }
    }
}
