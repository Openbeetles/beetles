//! Shared channel-side crypto helpers for webhook integrations.
//! 通道侧共享加解密辅助，供 webhook 协议实现复用。

use aes::Aes256;
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};

type Aes256CbcDec = cbc::Decryptor<Aes256>;

pub(crate) fn aes256_cbc_decrypt_pkcs7(
    key: &[u8; 32],
    iv: &[u8; 16],
    ciphertext: &[u8],
    stage: &'static str,
) -> crate::error::Result<Vec<u8>> {
    let mut buf = ciphertext.to_vec();
    let decrypted = Aes256CbcDec::new(key.into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| crate::error::Error::config(stage, "aes256-cbc decrypt failed"))?;
    Ok(decrypted.to_vec())
}

pub(crate) fn sha256_bytes(input: &[u8]) -> [u8; 32] {
    use sha2::Digest;

    let digest = sha2::Sha256::digest(input);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}
