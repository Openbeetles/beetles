//! 企业微信入站 Webhook：消息回调。
//! GET 校验支持明文/安全模式；POST 支持明文 XML 与加密 XML 解密后入队。

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use crate::bus::{InboundTx, PcMsg};
use crate::channels::crypto::aes256_cbc_decrypt_pkcs7;
use crate::error::{Error, Result};

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

/// 解析企微消息回调 XML body，提取 Content 和 FromUserName。
/// 企微回调 body 格式为 XML：
/// ```xml
/// <xml><ToUserName>...</ToUserName><FromUserName>...</FromUserName><Content>...</Content>...</xml>
/// ```
pub fn handle_message(body: &str, inbound_tx: &InboundTx) -> Result<()> {
    let content = extract_xml_field(body, "Content").unwrap_or_default();
    let from_user = extract_xml_field(body, "FromUserName").unwrap_or("wecom_default".to_string());

    if content.trim().is_empty() {
        log::debug!("[{}] empty content, skip", TAG);
        return Ok(());
    }

    log::info!(
        "[{}] received from user={} len={}",
        TAG,
        from_user,
        content.len()
    );

    let msg = PcMsg::new("wecom", &from_user, content)?;
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
}
