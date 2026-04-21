//! 企业微信入站 Webhook：消息回调。
//! GET 校验支持明文/安全模式；POST 支持明文 XML 与加密 XML 解密后入队。

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use crate::bus::{
    AssetSourcePlatform, AudioBody, CanonicalMessageBody, CardBody, CardFormat, FileBody,
    ImageBody, InboundTx, MediaAssetRef, MessageTransport, PcMsg, TextBody, TextFormat, VideoBody,
};
use crate::channels::crypto::aes256_cbc_decrypt_pkcs7;
use crate::error::{Error, Result};
use serde_json::{Map, Value};

const TAG: &str = "wecom_webhook";

/// 企业微信回调处理模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackMode {
    Plaintext,
    Secure,
}

/// 企业微信回调配置视图。
#[derive(Clone, Copy, Debug)]
pub struct CallbackConfig<'a> {
    pub token: &'a str,
    pub encoding_aes_key: &'a str,
    pub corp_id: &'a str,
}

/// 验证企微回调签名。算法：SHA1(sort([token, timestamp, nonce, payload]))。
/// `payload` 在安全模式下是 `Encrypt` / `echostr`，明文模式下为空串时不纳入签名。
pub fn verify_signature(
    token: &str,
    timestamp: &str,
    nonce: &str,
    payload: &str,
    expected_sig: &str,
) -> bool {
    if token.trim().is_empty() || timestamp.trim().is_empty() || nonce.trim().is_empty() {
        return false;
    }
    if expected_sig.trim().is_empty() {
        return false;
    }
    let mut parts = vec![token.trim(), timestamp.trim(), nonce.trim()];
    if !payload.trim().is_empty() {
        parts.push(payload.trim());
    }
    parts.sort();
    let combined = parts.concat();
    let computed = crate::util::sha1_hex(combined.as_bytes());
    crate::util::constant_time_eq(&computed, expected_sig.trim())
}

/// 校验回调基础配置。token 不能为空；安全模式下 EncodingAESKey 与 corp_id 也必须存在。
pub fn validate_callback_config(cfg: &CallbackConfig<'_>) -> Result<CallbackMode> {
    if cfg.token.trim().is_empty() {
        return Err(Error::config(
            TAG,
            "wecom_token is required for WeCom callback verification",
        ));
    }
    let mode = if cfg.encoding_aes_key.trim().is_empty() {
        CallbackMode::Plaintext
    } else {
        if cfg.corp_id.trim().is_empty() {
            return Err(Error::config(
                TAG,
                "wecom_corp_id is required when EncodingAESKey is configured",
            ));
        }
        CallbackMode::Secure
    };
    Ok(mode)
}

/// GET URL 验证：明文模式直接返回 echostr；安全模式先验签再解密 echostr。
pub fn verify_get_echostr(
    cfg: &CallbackConfig<'_>,
    timestamp: &str,
    nonce: &str,
    msg_signature: &str,
    echostr: &str,
) -> Result<String> {
    let mode = validate_callback_config(cfg)?;
    match mode {
        CallbackMode::Plaintext => {
            if !verify_signature(cfg.token, timestamp, nonce, "", msg_signature) {
                return Err(Error::config(TAG, "invalid callback signature"));
            }
            Ok(echostr.to_string())
        }
        CallbackMode::Secure => {
            if !verify_signature(cfg.token, timestamp, nonce, echostr, msg_signature) {
                return Err(Error::config(TAG, "invalid callback signature"));
            }
            decrypt_wecom_message(cfg.encoding_aes_key, cfg.corp_id, echostr)
        }
    }
}

/// POST 消息回调：安全模式解析 Encrypt 并解密得到原始 XML；明文模式直接返回 body。
pub fn decode_post_xml(
    cfg: &CallbackConfig<'_>,
    timestamp: &str,
    nonce: &str,
    msg_signature: &str,
    body: &str,
) -> Result<String> {
    let mode = validate_callback_config(cfg)?;
    match mode {
        CallbackMode::Plaintext => {
            if extract_xml_field(body, "Encrypt").is_some() {
                return Err(Error::config(
                    TAG,
                    "encrypted callback received without EncodingAESKey",
                ));
            }
            if !verify_signature(cfg.token, timestamp, nonce, "", msg_signature) {
                return Err(Error::config(TAG, "invalid callback signature"));
            }
            Ok(body.to_string())
        }
        CallbackMode::Secure => {
            let encrypt = extract_xml_field(body, "Encrypt").ok_or_else(|| {
                Error::config(TAG, "missing Encrypt field in encrypted callback body")
            })?;
            if !verify_signature(cfg.token, timestamp, nonce, &encrypt, msg_signature) {
                return Err(Error::config(TAG, "invalid callback signature"));
            }
            decrypt_wecom_message(cfg.encoding_aes_key, cfg.corp_id, &encrypt)
        }
    }
}

fn wecom_platform_handle(media_id: Option<String>) -> Option<MediaAssetRef> {
    media_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| MediaAssetRef::platform_handle(AssetSourcePlatform::WeCom, value))
}

fn card_fallback_text(title: Option<String>, description: Option<String>) -> String {
    let mut lines = Vec::new();
    if let Some(title) = title
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        lines.push(title);
    }
    if let Some(description) = description
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        lines.push(description);
    }
    lines.join("\n")
}

fn build_card_payload(msg_type: &str, body: &str) -> Value {
    let mut nested = Map::new();
    nested.insert(
        "title".to_string(),
        Value::String(extract_xml_field(body, "Title").unwrap_or_default()),
    );
    nested.insert(
        "description".to_string(),
        Value::String(extract_xml_field(body, "Description").unwrap_or_default()),
    );
    nested.insert(
        "url".to_string(),
        Value::String(extract_xml_field(body, "Url").unwrap_or_default()),
    );
    nested.insert(
        "btntxt".to_string(),
        Value::String(extract_xml_field(body, "Btntxt").unwrap_or_default()),
    );
    nested.insert(
        "media_id".to_string(),
        Value::String(extract_xml_field(body, "MediaId").unwrap_or_default()),
    );
    nested.insert(
        "picurl".to_string(),
        Value::String(extract_xml_field(body, "PicUrl").unwrap_or_default()),
    );

    let mut payload = Map::new();
    payload.insert("msgtype".to_string(), Value::String(msg_type.to_string()));
    payload.insert(msg_type.to_string(), Value::Object(nested));
    Value::Object(payload)
}

fn build_inbound_body(body: &str) -> Result<Option<CanonicalMessageBody>> {
    let msg_type = extract_xml_field(body, "MsgType")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if msg_type.is_empty() || msg_type == "event" {
        return Ok(None);
    }
    let canonical = match msg_type.as_str() {
        "text" => {
            let content = extract_xml_field(body, "Content").unwrap_or_default();
            let trimmed = content.trim().to_string();
            if trimmed.is_empty() {
                return Ok(None);
            }
            CanonicalMessageBody::Text(TextBody::plain(trimmed))
        }
        "markdown" => {
            let content = extract_xml_field(body, "Content").unwrap_or_default();
            let trimmed = content.trim().to_string();
            if trimmed.is_empty() {
                return Ok(None);
            }
            CanonicalMessageBody::Text(TextBody {
                text: trimmed,
                format: TextFormat::Markdown,
            })
        }
        "image" => {
            let Some(asset) = wecom_platform_handle(extract_xml_field(body, "MediaId")) else {
                return Ok(None);
            };
            CanonicalMessageBody::Image(ImageBody {
                asset,
                caption: None,
            })
        }
        "voice" => {
            let Some(asset) = wecom_platform_handle(extract_xml_field(body, "MediaId")) else {
                return Ok(None);
            };
            CanonicalMessageBody::Audio(AudioBody {
                asset,
                caption: None,
                transcript_text: extract_xml_field(body, "Recognition"),
            })
        }
        "video" => {
            let Some(asset) = wecom_platform_handle(extract_xml_field(body, "MediaId")) else {
                return Ok(None);
            };
            let title = extract_xml_field(body, "Title");
            let description = extract_xml_field(body, "Description");
            CanonicalMessageBody::Video(VideoBody {
                asset,
                caption: None,
                title,
                description,
            })
        }
        "file" => {
            let Some(asset) = wecom_platform_handle(extract_xml_field(body, "MediaId")) else {
                return Ok(None);
            };
            CanonicalMessageBody::File(FileBody {
                asset,
                caption: None,
            })
        }
        "textcard" | "news" | "taskcard" | "template_card" => {
            let title = extract_xml_field(body, "Title");
            let description = extract_xml_field(body, "Description");
            CanonicalMessageBody::Card(CardBody {
                format: CardFormat::TemplateCard,
                payload_json: build_card_payload(&msg_type, body),
                fallback_text: card_fallback_text(title, description),
            })
        }
        _ => {
            log::debug!("[{}] unsupported MsgType={}, skip", TAG, msg_type);
            return Ok(None);
        }
    };
    Ok(Some(canonical))
}

/// 解析企微消息回调 XML body，提取真实 MsgType 与 FromUserName。
pub fn handle_message(body: &str, inbound_tx: &InboundTx) -> Result<()> {
    let from_user = extract_xml_field(body, "FromUserName").unwrap_or("wecom_default".to_string());
    let Some(canonical_body) = build_inbound_body(body)? else {
        log::debug!("[{}] empty or unsupported inbound body, skip", TAG);
        return Ok(());
    };

    log::info!(
        "[{}] received from user={} kind={:?} len={}",
        TAG,
        from_user,
        canonical_body.kind(),
        canonical_body.text_projection().len()
    );

    let msg_id = extract_xml_field(body, "MsgId").unwrap_or_default();
    let inbound_dedup_key = if msg_id.trim().is_empty() {
        String::new()
    } else {
        format!("wecom_message:{}", msg_id.trim())
    };
    let msg = PcMsg::new_inbound_with_body("wecom", &from_user, canonical_body, false)?
        .with_inbound_provenance(MessageTransport::Webhook, msg_id, "", inbound_dedup_key);
    if inbound_tx.send(msg).is_err() {
        log::warn!("[{}] inbound_tx send failed (queue full?)", TAG);
    }
    Ok(())
}

fn decrypt_wecom_message(encoding_aes_key: &str, corp_id: &str, payload: &str) -> Result<String> {
    let key = decode_encoding_aes_key(encoding_aes_key)?;
    let ciphertext = STANDARD
        .decode(payload.as_bytes())
        .map_err(|_| Error::config(TAG, "failed to decode base64 payload"))?;
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&key[..16]);
    let plain = aes256_cbc_decrypt_pkcs7(&key, &iv, &ciphertext, TAG)?;
    parse_wecom_plaintext(&plain, corp_id)
}

fn decode_encoding_aes_key(encoding_aes_key: &str) -> Result<[u8; 32]> {
    let raw = encoding_aes_key.trim();
    if raw.len() != 43 && raw.len() != 44 {
        return Err(Error::config(
            TAG,
            "EncodingAESKey must be 43 or 44 characters",
        ));
    }
    let normalized = if raw.ends_with('=') {
        raw.to_string()
    } else if raw.len() == 43 {
        format!("{}=", raw)
    } else {
        raw.to_string()
    };
    let decoded = STANDARD
        .decode(normalized.as_bytes())
        .map_err(|_| Error::config(TAG, "invalid EncodingAESKey"))?;
    if decoded.len() != 32 {
        return Err(Error::config(TAG, "invalid EncodingAESKey length"));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
}

fn parse_wecom_plaintext(plain: &[u8], corp_id: &str) -> Result<String> {
    if plain.len() < 20 {
        return Err(Error::config(TAG, "decrypted payload too short"));
    }
    let msg_len = u32::from_be_bytes(
        plain[16..20]
            .try_into()
            .map_err(|_| Error::config(TAG, "failed to read decrypted payload length"))?,
    ) as usize;
    let end = 20usize
        .checked_add(msg_len)
        .ok_or_else(|| Error::config(TAG, "decrypted payload length overflow"))?;
    if plain.len() < end {
        return Err(Error::config(TAG, "decrypted payload truncated"));
    }
    let msg = std::str::from_utf8(&plain[20..end])
        .map_err(|_| Error::config(TAG, "decrypted payload is not valid utf-8"))?;
    let receive_id = std::str::from_utf8(&plain[end..])
        .map_err(|_| Error::config(TAG, "decrypted receive_id is not valid utf-8"))?;
    if !corp_id.trim().is_empty() && receive_id != corp_id.trim() {
        return Err(Error::config(
            TAG,
            "decrypted receive_id does not match corp_id",
        ));
    }
    Ok(msg.to_string())
}

/// 从 XML 字符串中提取指定标签的内容。支持 CDATA 和纯文本。
fn extract_xml_field(xml: &str, field: &str) -> Option<String> {
    let open = format!("<{}>", field);
    let close = format!("</{}>", field);
    let start = xml.find(&open).map(|i| i + open.len())?;
    let end = xml[start..].find(&close).map(|i| start + i)?;
    let value = &xml[start..end];
    let value = value.trim();
    if value.starts_with("<![CDATA[") && value.ends_with("]]>") {
        Some(value[9..value.len() - 3].to_string())
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{new_inbound_channel, CanonicalMessageBody};
    use aes::Aes256;
    use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};

    type Aes256CbcEnc = cbc::Encryptor<Aes256>;

    fn encode_aes_key(raw: &[u8; 32]) -> String {
        let encoded = STANDARD.encode(raw);
        encoded.trim_end_matches('=').to_string()
    }

    fn build_signature(token: &str, timestamp: &str, nonce: &str, payload: &str) -> String {
        let mut parts = [token, timestamp, nonce, payload];
        parts.sort();
        crate::util::sha1_hex(parts.concat().as_bytes())
    }

    fn encrypt_callback_xml(encoding_aes_key: &str, corp_id: &str, xml: &str) -> String {
        let key = decode_encoding_aes_key(encoding_aes_key).expect("key");
        let mut plain = Vec::new();
        plain.extend_from_slice(&[0u8; 16]);
        plain.extend_from_slice(&(xml.len() as u32).to_be_bytes());
        plain.extend_from_slice(xml.as_bytes());
        plain.extend_from_slice(corp_id.as_bytes());
        let msg_len = plain.len();
        plain.resize(msg_len + 32, 0);
        let encrypted = Aes256CbcEnc::new((&key).into(), (&key[..16]).into())
            .encrypt_padded_mut::<Pkcs7>(&mut plain, msg_len)
            .expect("encrypt");
        STANDARD.encode(encrypted)
    }

    #[test]
    fn validate_callback_config_rejects_empty_token() {
        let error = validate_callback_config(&CallbackConfig {
            token: "",
            encoding_aes_key: "",
            corp_id: "",
        })
        .expect_err("empty token must fail");
        assert_eq!(error.stage(), TAG);
    }

    #[test]
    fn verify_signature_rejects_empty_token() {
        assert!(!verify_signature("", "1", "2", "", "abc"));
    }

    #[test]
    fn handle_message_maps_markdown_msgtype_to_markdown_text_body() {
        let body = r#"
            <xml>
                <FromUserName><![CDATA[user-1]]></FromUserName>
                <MsgType><![CDATA[markdown]]></MsgType>
                <Content><![CDATA[# Title]]></Content>
                <MsgId>100</MsgId>
            </xml>
        "#;
        let (inbound_tx, inbound_rx, _) = new_inbound_channel(4);

        handle_message(body, &inbound_tx).expect("handle");

        let msg = inbound_rx.try_recv().expect("message");
        match msg.body {
            CanonicalMessageBody::Text(text) => {
                assert_eq!(text.format, TextFormat::Markdown);
                assert_eq!(text.text, "# Title");
            }
            other => panic!("unexpected body: {:?}", other),
        }
        assert_eq!(msg.platform_message_id, "100");
    }

    #[test]
    fn secure_callback_image_message_decodes_to_image_body() {
        let corp_id = "wx123456";
        let mut key = [0u8; 32];
        for (index, byte) in key.iter_mut().enumerate() {
            *byte = (index as u8).saturating_add(1);
        }
        let encoding_aes_key = encode_aes_key(&key);
        let cfg = CallbackConfig {
            token: "token-1",
            encoding_aes_key: &encoding_aes_key,
            corp_id,
        };
        let inner_xml = r#"
            <xml>
                <FromUserName><![CDATA[user-2]]></FromUserName>
                <MsgType><![CDATA[image]]></MsgType>
                <MediaId><![CDATA[MEDIA_123]]></MediaId>
                <MsgId>200</MsgId>
            </xml>
        "#;
        let encrypt = encrypt_callback_xml(&encoding_aes_key, corp_id, inner_xml);
        let timestamp = "1710000000";
        let nonce = "nonce-1";
        let signature = build_signature(cfg.token, timestamp, nonce, &encrypt);
        let callback_xml = format!("<xml><Encrypt><![CDATA[{encrypt}]]></Encrypt></xml>");
        let decoded =
            decode_post_xml(&cfg, timestamp, nonce, &signature, &callback_xml).expect("decode");
        let (inbound_tx, inbound_rx, _) = new_inbound_channel(4);

        handle_message(&decoded, &inbound_tx).expect("handle");

        let msg = inbound_rx.try_recv().expect("message");
        match msg.body {
            CanonicalMessageBody::Image(image) => {
                assert_eq!(image.asset.locator, "MEDIA_123");
            }
            other => panic!("unexpected body: {:?}", other),
        }
        assert_eq!(msg.platform_message_id, "200");
    }
}
