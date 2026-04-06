//! QQ URL verification and Ed25519 signature verification helpers.
//! QQ 验址与 Ed25519 验签辅助逻辑。

use crate::error::{Error, Result};
use ed25519_dalek::{Signer, SigningKey};

fn secret_to_seed(secret: &str) -> [u8; 32] {
    let mut seed = [0u8; 32];
    let bytes = secret.as_bytes();
    if bytes.is_empty() {
        log::warn!("[qq_signature] secret_to_seed called with empty secret");
        return seed;
    }
    for (i, b) in seed.iter_mut().enumerate() {
        *b = bytes[i % bytes.len()];
    }
    seed
}

/// Signs `event_ts + plain_token` for QQ URL verification responses.
/// 对 `event_ts + plain_token` 做签名，用于 QQ 验址响应。
pub fn sign_qq_url_verify(secret: &str, event_ts: &str, plain_token: &str) -> Result<String> {
    let seed = secret_to_seed(secret);
    let signing_key = SigningKey::from_bytes(&seed);
    let message = format!("{}{}", event_ts, plain_token);
    let signature = signing_key.sign(message.as_bytes());
    Ok(hex::encode(signature.to_bytes()))
}

/// Verifies QQ callback Ed25519 signature against `timestamp + body`.
/// 校验 QQ 回调的 Ed25519 签名，消息为 `timestamp + body`。
pub fn verify_qq_signature(
    secret: &str,
    timestamp: &str,
    body: &[u8],
    signature_hex: &str,
) -> Result<()> {
    let seed = secret_to_seed(secret);
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();
    let sig_bytes: [u8; 64] = hex::decode(signature_hex)
        .map_err(|e| Error::config("qq_verify_hex", e.to_string()))?
        .try_into()
        .map_err(|_| Error::config("qq_verify", "signature length must be 64 bytes"))?;
    let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);
    let message: Vec<u8> = timestamp
        .as_bytes()
        .iter()
        .chain(body.iter())
        .copied()
        .collect();
    verifying_key
        .verify_strict(&message, &signature)
        .map_err(|_| Error::config("qq_verify", "signature verification failed"))?;
    Ok(())
}
