//! 企业微信通道：出站经 MessageSink 队列，由 main 用 HTTP 鉴权后发送应用消息；入站无。
//! 鉴权 GET gettoken，发送 POST message/send；text 按 2048 字节分片（官方限制）。Sink 统一为 dispatch::QueuedSink。

use crate::bus::{CanonicalMessageBody, CardBody, MediaLocatorKind, TextBody, TextFormat};
use crate::channels::send::{
    ensure_sender_http, record_outbound_http_failure, record_outbound_http_success,
    run_buffered_sender_loop,
};
use crate::channels::ChannelHttpClient;
use crate::config::AppConfig;
use serde_json::{Map, Value};

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
) -> super::super::connectivity::ChannelConnectivityItem {
    use super::super::connectivity;
    let agent_id_u32 = config.wecom_agent_id.trim().parse::<u32>().ok();
    let configured = !config.wecom_corp_id.trim().is_empty()
        && !config.wecom_corp_secret.trim().is_empty()
        && agent_id_u32.is_some();
    connectivity::probe_item("wecom", configured, || {
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
                return connectivity::ProbeStatus::CheckFailed;
            }
        };
        if status >= 400 {
            log::warn!("[wecom_connectivity] gettoken status {}", status);
            return connectivity::ProbeStatus::InvalidToken;
        }
        let r: WecomTokenResponse = match serde_json::from_slice(resp_body.as_ref()) {
            Ok(x) => x,
            Err(e) => {
                log::warn!("[wecom_connectivity] gettoken parse: {}", e);
                return connectivity::ProbeStatus::CheckFailed;
            }
        };
        if r.errcode != 0 {
            log::warn!(
                "[wecom_connectivity] gettoken errcode {} {}",
                r.errcode,
                r.errmsg
            );
            return connectivity::ProbeStatus::InvalidToken;
        }
        let token = match r.access_token {
            Some(t) if !t.is_empty() => t,
            _ => {
                log::warn!("[wecom_connectivity] no access_token in response");
                return connectivity::ProbeStatus::InvalidToken;
            }
        };
        let touser = config.wecom_default_touser.trim();
        if touser.is_empty() {
            return connectivity::ProbeStatus::Ok;
        }
        let agent_id_u32 = match agent_id_u32 {
            Some(agent_id_u32) => agent_id_u32,
            None => {
                log::warn!("[wecom_connectivity] invalid agent_id parse");
                return connectivity::ProbeStatus::InvalidToken;
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
                return connectivity::ProbeStatus::CheckFailed;
            }
        };
        let send_url = format!("{}?access_token={}", WECOM_SEND_BASE, token);
        let (status, resp_body) = match http.http_post(&send_url, &body_bytes) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[wecom_connectivity] send http: {}", e);
                return connectivity::ProbeStatus::CheckFailed;
            }
        };
        if status >= 400 {
            log::warn!("[wecom_connectivity] send status {}", status);
            return connectivity::ProbeStatus::CheckFailed;
        }
        let send_r: WecomSendResponse = match serde_json::from_slice(resp_body.as_ref()) {
            Ok(x) => x,
            Err(e) => {
                log::warn!("[wecom_connectivity] send parse: {}", e);
                return connectivity::ProbeStatus::CheckFailed;
            }
        };
        if send_r.errcode != 0 {
            log::warn!(
                "[wecom_connectivity] send errcode {} {}",
                send_r.errcode,
                send_r.errmsg
            );
            return connectivity::ProbeStatus::CheckFailed;
        }
        connectivity::ProbeStatus::Ok
    })
}

/// Returns `(access_token, expires_in_secs)` for sender-loop caching.
fn acquire_wecom_token_with_expiry<H: ChannelHttpClient>(
    http: &mut H,
    corp_id: &str,
    corp_secret: &str,
) -> crate::error::Result<(String, u64)> {
    const TAG: &str = "wecom_send";
    let url = format!(
        "{}?corpid={}&corpsecret={}",
        WECOM_GETTOKEN_BASE, corp_id, corp_secret
    );
    let (status, resp_body) = match http.http_get(&url) {
        Ok(r) => r,
        Err(e) => {
            let error = crate::error::Error::Other {
                source: Box::new(e),
                stage: "wecom_token",
            };
            record_outbound_http_failure(&error);
            return Err(error);
        }
    };
    if status >= 400 {
        let error = crate::error::Error::Http {
            status_code: status,
            stage: "wecom_token",
        };
        record_outbound_http_failure(&error);
        return Err(error);
    }
    record_outbound_http_success();
    let token_resp: WecomTokenResponse = match serde_json::from_slice(resp_body.as_ref()) {
        Ok(t) => t,
        Err(e) => {
            return Err(crate::error::Error::Other {
                source: Box::new(e),
                stage: "wecom_token",
            });
        }
    };
    if token_resp.errcode != 0 {
        log::warn!(
            "[{}] gettoken errcode={} errmsg={}",
            TAG,
            token_resp.errcode,
            token_resp.errmsg
        );
        return Err(crate::error::Error::config(
            "wecom_token",
            format!(
                "errcode={} errmsg={}",
                token_resp.errcode, token_resp.errmsg
            ),
        ));
    }
    match token_resp.access_token {
        Some(t) if !t.is_empty() => {
            let exp_secs = if token_resp.expires_in == 0 {
                7200
            } else {
                token_resp.expires_in
            };
            Ok((t, exp_secs.max(60)))
        }
        _ => {
            log::warn!("[{}] gettoken empty access_token", TAG);
            Err(crate::error::Error::config(
                "wecom_token",
                "empty access_token",
            ))
        }
    }
}

fn acquire_wecom_token<H: ChannelHttpClient>(
    http: &mut H,
    corp_id: &str,
    corp_secret: &str,
) -> Option<String> {
    acquire_wecom_token_with_expiry(http, corp_id, corp_secret)
        .ok()
        .map(|(t, _)| t)
}

fn resolve_touser<'a>(chat_id: &'a str, default_touser: &'a str) -> &'a str {
    if chat_id.trim().is_empty() {
        default_touser.trim()
    } else {
        chat_id.trim()
    }
}

fn send_url(token: &str) -> String {
    format!("{}?access_token={}", WECOM_SEND_BASE, token)
}

fn media_handle<'a>(
    asset: &'a crate::bus::MediaAssetRef,
    body_kind: &str,
) -> crate::error::Result<&'a str> {
    if asset.locator_kind != MediaLocatorKind::PlatformHandle {
        return Err(crate::error::Error::config(
            "wecom_send",
            format!("WeCom {body_kind} outbound requires a platform_handle media locator"),
        ));
    }
    let locator = asset.locator.trim();
    if locator.is_empty() {
        return Err(crate::error::Error::config(
            "wecom_send",
            format!("WeCom {body_kind} outbound requires a non-empty media handle"),
        ));
    }
    Ok(locator)
}

fn build_text_payloads(
    touser: &str,
    agent_id_u32: u32,
    text: &TextBody,
    fallback_content: &str,
) -> crate::error::Result<Vec<Value>> {
    let mut normalized = text.text.trim().to_string();
    if normalized.is_empty() {
        normalized = fallback_content.trim().to_string();
    }
    if normalized.is_empty() {
        return Err(crate::error::Error::config(
            "wecom_send",
            "refusing to send empty WeCom text body",
        ));
    }
    match text.format {
        TextFormat::Plain => Ok(crate::channels::chunk::chunk_text_by_utf8_bytes(
            &normalized,
            WECOM_MAX_TEXT_BYTES,
        )
        .into_iter()
        .map(|chunk| {
            serde_json::json!({
                "touser": touser,
                "msgtype": "text",
                "agentid": agent_id_u32,
                "text": { "content": chunk }
            })
        })
        .collect()),
        TextFormat::Markdown => Ok(crate::channels::chunk::chunk_text_by_utf8_bytes(
            &normalized,
            WECOM_MAX_TEXT_BYTES,
        )
        .into_iter()
        .map(|chunk| {
            serde_json::json!({
                "touser": touser,
                "msgtype": "markdown",
                "agentid": agent_id_u32,
                "markdown": { "content": chunk }
            })
        })
        .collect()),
        TextFormat::Html => Err(crate::error::Error::config(
            "wecom_send",
            "WeCom does not support HTML text bodies",
        )),
        TextFormat::RichText => Err(crate::error::Error::config(
            "wecom_send",
            "WeCom rich_text body requires a CardBody payload",
        )),
    }
}

fn wrap_wecom_payload(
    touser: &str,
    agent_id_u32: u32,
    msgtype: &str,
    nested_key: &str,
    nested_value: Value,
) -> Value {
    let mut map = Map::new();
    map.insert("touser".to_string(), Value::String(touser.to_string()));
    map.insert("msgtype".to_string(), Value::String(msgtype.to_string()));
    map.insert("agentid".to_string(), Value::from(agent_id_u32));
    map.insert(nested_key.to_string(), nested_value);
    Value::Object(map)
}

fn payload_object(payload: &Value) -> crate::error::Result<&Map<String, Value>> {
    payload.as_object().ok_or_else(|| {
        crate::error::Error::config(
            "wecom_send",
            "WeCom CardBody payload_json must be a JSON object",
        )
    })
}

fn build_card_payload(
    touser: &str,
    agent_id_u32: u32,
    card: &CardBody,
) -> crate::error::Result<Value> {
    let payload = payload_object(&card.payload_json)?;
    if let Some(msgtype) = payload.get("msgtype").and_then(Value::as_str) {
        let nested_key = match msgtype {
            "textcard" => "textcard",
            "news" => "news",
            "mpnews" => "mpnews",
            "template_card" => "template_card",
            "taskcard" => "taskcard",
            other => {
                return Err(crate::error::Error::config(
                    "wecom_send",
                    format!("unsupported WeCom card msgtype={other}"),
                ))
            }
        };
        let nested = payload.get(nested_key).cloned().ok_or_else(|| {
            crate::error::Error::config(
                "wecom_send",
                format!("WeCom card payload missing nested object for msgtype={msgtype}"),
            )
        })?;
        let mut obj = payload.clone();
        obj.insert("touser".to_string(), Value::String(touser.to_string()));
        obj.insert("agentid".to_string(), Value::from(agent_id_u32));
        obj.insert(nested_key.to_string(), nested);
        return Ok(Value::Object(obj));
    }

    let title = payload
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            crate::error::Error::config("wecom_send", "WeCom textcard payload requires title")
        })?;
    let description = payload
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            crate::error::Error::config("wecom_send", "WeCom textcard payload requires description")
        })?;
    let url = payload
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            crate::error::Error::config("wecom_send", "WeCom textcard payload requires url")
        })?;
    let mut textcard = serde_json::json!({
        "title": title,
        "description": description,
        "url": url,
    });
    if let Some(btntxt) = payload
        .get("btntxt")
        .or_else(|| payload.get("btntext"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        textcard["btntxt"] = Value::String(btntxt.to_string());
    }
    Ok(wrap_wecom_payload(
        touser,
        agent_id_u32,
        "textcard",
        "textcard",
        textcard,
    ))
}

fn build_wecom_payloads(
    touser: &str,
    agent_id_u32: u32,
    message: &crate::channels::send::QueuedOutboundMessage,
) -> crate::error::Result<Vec<Value>> {
    match &message.body {
        CanonicalMessageBody::Text(text) => {
            build_text_payloads(touser, agent_id_u32, text, &message.content)
        }
        CanonicalMessageBody::Image(image) => Ok(vec![wrap_wecom_payload(
            touser,
            agent_id_u32,
            "image",
            "image",
            serde_json::json!({ "media_id": media_handle(&image.asset, "image")? }),
        )]),
        CanonicalMessageBody::Audio(audio) => Ok(vec![wrap_wecom_payload(
            touser,
            agent_id_u32,
            "voice",
            "voice",
            serde_json::json!({ "media_id": media_handle(&audio.asset, "voice")? }),
        )]),
        CanonicalMessageBody::Video(video) => {
            let mut payload = serde_json::json!({
                "media_id": media_handle(&video.asset, "video")?,
            });
            if let Some(title) = video
                .title
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                payload["title"] = Value::String(title.to_string());
            }
            if let Some(description) = video
                .description
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                payload["description"] = Value::String(description.to_string());
            }
            Ok(vec![wrap_wecom_payload(
                touser,
                agent_id_u32,
                "video",
                "video",
                payload,
            )])
        }
        CanonicalMessageBody::File(file) => Ok(vec![wrap_wecom_payload(
            touser,
            agent_id_u32,
            "file",
            "file",
            serde_json::json!({ "media_id": media_handle(&file.asset, "file")? }),
        )]),
        CanonicalMessageBody::Card(card) => {
            Ok(vec![build_card_payload(touser, agent_id_u32, card)?])
        }
        CanonicalMessageBody::PlatformNative(native) => {
            if native.payload_json.is_object() {
                Ok(vec![native.payload_json.clone()])
            } else {
                Err(crate::error::Error::config(
                    "wecom_send",
                    "WeCom PlatformNativeBody payload_json must be a JSON object",
                ))
            }
        }
    }
}

fn send_one_wecom<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    agent_id_u32: u32,
    message: &crate::channels::send::QueuedOutboundMessage,
    default_touser: &str,
) -> crate::error::Result<()> {
    const TAG: &str = "wecom_send";
    let touser = resolve_touser(&message.chat_id, default_touser);
    if touser.is_empty() {
        return Ok(());
    }
    let send_url = send_url(token);
    for body in build_wecom_payloads(touser, agent_id_u32, message)? {
        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| crate::error::Error::config("wecom_send", e.to_string()))?;
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
    rx: &std::sync::mpsc::Receiver<crate::channels::send::QueuedOutboundMessage>,
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
    while let Ok(message) = rx.try_recv() {
        if let Err(error) = send_one_wecom(http, &token, agent_id_u32, &message, default_touser) {
            record_outbound_http_failure(&error);
            log::warn!(
                "[wecom_flush] send failed for chat_id={}: {}",
                message.chat_id,
                error
            );
        } else {
            record_outbound_http_success();
        }
    }
}

/// access_token 缓存提前刷新余量（秒）。
const WECOM_TOKEN_CACHE_MARGIN_SECS: u64 = 120;

/// 持续运行的企业微信发送循环：sender 线程内**复用** HTTP，并按 `expires_in` **缓存** token。
pub fn run_wecom_sender_loop<H, F>(
    rx: std::sync::mpsc::Receiver<crate::channels::send::QueuedOutboundMessage>,
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
    let mut http: Option<H> = None;
    let mut token_cache: Option<(String, std::time::Instant)> = None;
    run_buffered_sender_loop(rx, TAG, |message, attempt| {
        if !ensure_sender_http(&mut http, &mut create_http, TAG, attempt) {
            return Err(crate::error::Error::config(TAG, "create http failed"));
        }
        let now = std::time::Instant::now();
        let mut token = token_cache
            .as_ref()
            .filter(|(_, exp)| now < *exp)
            .map(|(token, _)| token.clone());
        if token.is_none() {
            token_cache = None;
            let Some(h) = http.as_mut() else {
                return Err(crate::error::Error::config(
                    TAG,
                    "sender http missing after ensure",
                ));
            };
            match acquire_wecom_token_with_expiry(h, corp_id, corp_secret) {
                Ok((fresh_token, exp_secs)) => {
                    let keep = exp_secs
                        .saturating_sub(WECOM_TOKEN_CACHE_MARGIN_SECS)
                        .max(30);
                    token_cache = Some((
                        fresh_token.clone(),
                        now + std::time::Duration::from_secs(keep),
                    ));
                    token = Some(fresh_token);
                }
                Err(error) => {
                    log::warn!(
                        "[{}] acquire token failed (attempt {}): {}",
                        TAG,
                        attempt,
                        error
                    );
                    http = None;
                    return Err(error);
                }
            }
        }

        let Some(token) = token else {
            return Err(crate::error::Error::config(
                TAG,
                "sender token missing after refresh",
            ));
        };
        let Some(h) = http.as_mut() else {
            return Err(crate::error::Error::config(
                TAG,
                "sender http missing after token refresh",
            ));
        };
        match send_one_wecom(h, &token, agent_id_u32, message, default_touser) {
            Ok(()) => {
                record_outbound_http_success();
                Ok(())
            }
            Err(error) => {
                record_outbound_http_failure(&error);
                log::warn!(
                    "[{}] send failed (attempt {}), chat_id={}: {}",
                    TAG,
                    attempt,
                    message.chat_id,
                    error
                );
                token_cache = None;
                http = None;
                Err(error)
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::send_one_wecom;
    use crate::bus::{
        AssetSourcePlatform, CanonicalMessageBody, CardBody, ImageBody, MediaAssetRef, OutboundKind,
    };
    use crate::channels::send::QueuedOutboundMessage;
    use crate::channels::ChannelHttpClient;
    use crate::platform::ResponseBody;

    #[derive(Default)]
    struct FakeHttp {
        posts: Vec<(String, Vec<u8>)>,
    }

    impl ChannelHttpClient for FakeHttp {
        fn http_get(&mut self, _url: &str) -> crate::error::Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(b"{}".to_vec())))
        }

        fn http_get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> crate::error::Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(b"{}".to_vec())))
        }

        fn http_post(
            &mut self,
            url: &str,
            body: &[u8],
        ) -> crate::error::Result<(u16, ResponseBody)> {
            self.posts.push((url.to_string(), body.to_vec()));
            Ok((200, ResponseBody::Heap(b"{}".to_vec())))
        }

        fn http_post_with_headers(
            &mut self,
            url: &str,
            _headers: &[(&str, &str)],
            body: &[u8],
        ) -> crate::error::Result<(u16, ResponseBody)> {
            self.http_post(url, body)
        }
    }

    fn queued_message(body: CanonicalMessageBody) -> QueuedOutboundMessage {
        QueuedOutboundMessage {
            transport_send_id: 1,
            chat_id: "user-1".to_string(),
            content: body.text_projection(),
            body,
            platform_thread_id: String::new(),
            req_id: Some("req-1".to_string()),
            outbound_kind: OutboundKind::Primary,
        }
    }

    #[test]
    fn send_one_wecom_renders_image_media_id_payload() {
        let message = queued_message(CanonicalMessageBody::Image(ImageBody {
            asset: MediaAssetRef::platform_handle(AssetSourcePlatform::WeCom, "MEDIA123"),
            caption: None,
        }));
        let mut http = FakeHttp::default();

        send_one_wecom(&mut http, "token-1", 100, &message, "").expect("send image");

        assert_eq!(http.posts.len(), 1);
        let posted: serde_json::Value = serde_json::from_slice(&http.posts[0].1).expect("json");
        assert_eq!(posted["msgtype"], "image");
        assert_eq!(posted["image"]["media_id"], "MEDIA123");
    }

    #[test]
    fn send_one_wecom_builds_textcard_from_card_payload() {
        let message = queued_message(CanonicalMessageBody::Card(CardBody {
            format: crate::bus::CardFormat::TemplateCard,
            payload_json: serde_json::json!({
                "title": "Alert",
                "description": "Line 1",
                "url": "https://example.invalid",
                "btntxt": "More"
            }),
            fallback_text: "Alert".to_string(),
        }));
        let mut http = FakeHttp::default();

        send_one_wecom(&mut http, "token-1", 100, &message, "").expect("send card");

        let posted: serde_json::Value = serde_json::from_slice(&http.posts[0].1).expect("json");
        assert_eq!(posted["msgtype"], "textcard");
        assert_eq!(posted["textcard"]["title"], "Alert");
        assert_eq!(posted["textcard"]["btntxt"], "More");
    }
}
