//! Shared LLM JSON request body builder.
//! 供应商 schema 留在各 provider，字节分配策略统一收口在这里。

use crate::error::{Error, Result};
use crate::platform::ByteBuffer;
use std::io::Write;

/// Owned LLM request body.
pub(crate) struct LlmRequestBody {
    bytes: ByteBuffer,
    max_len: usize,
}

impl LlmRequestBody {
    pub(crate) fn with_estimated_capacity(estimated_capacity: usize, max_len: usize) -> Self {
        Self {
            bytes: ByteBuffer::with_capacity(estimated_capacity.min(max_len)),
            max_len,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        self.bytes.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn is_external_preferred(&self) -> bool {
        self.bytes.is_external_preferred()
    }

    pub(crate) fn push_str(&mut self, value: &str) -> Result<()> {
        self.write_all(value.as_bytes())
    }

    pub(crate) fn push_byte(&mut self, value: u8) -> Result<()> {
        self.write_all(&[value])
    }

    pub(crate) fn push_json_string(&mut self, value: &str) -> Result<()> {
        self.push_byte(b'"')?;
        for ch in value.chars() {
            match ch {
                '"' => self.push_str("\\\"")?,
                '\\' => self.push_str("\\\\")?,
                '\n' => self.push_str("\\n")?,
                '\r' => self.push_str("\\r")?,
                '\t' => self.push_str("\\t")?,
                '\u{08}' => self.push_str("\\b")?,
                '\u{0c}' => self.push_str("\\f")?,
                c if c <= '\u{1f}' => {
                    let code = c as u32 as u8;
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    self.push_str("\\u00")?;
                    self.push_byte(HEX[(code >> 4) as usize])?;
                    self.push_byte(HEX[(code & 0x0f) as usize])?;
                }
                c => {
                    let mut buf = [0u8; 4];
                    self.push_str(c.encode_utf8(&mut buf))?;
                }
            }
        }
        self.push_byte(b'"')
    }

    pub(crate) fn push_json_string_field(&mut self, key: &str, value: &str) -> Result<()> {
        self.push_byte(b'"')?;
        self.push_str(key)?;
        self.push_str("\":")?;
        self.push_json_string(value)
    }

    pub(crate) fn finish(self, max_len: usize) -> Result<Self> {
        if self.len() > max_len {
            return Err(Self::too_large_error(max_len));
        }
        Ok(self)
    }

    fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        let next_len = self
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| Self::too_large_error(self.max_len))?;
        if next_len > self.max_len {
            return Err(Self::too_large_error(self.max_len));
        }
        self.bytes
            .write_all(bytes)
            .map_err(|e| Error::io("llm_request", e))
    }

    fn too_large_error(max_len: usize) -> Error {
        Error::config(
            "llm_request",
            format!("request body exceeds {} bytes", max_len),
        )
    }
}

impl AsRef<[u8]> for LlmRequestBody {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl std::ops::Deref for LlmRequestBody {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::LlmRequestBody;
    use crate::platform::ByteBuffer;

    #[test]
    fn json_string_escaping_matches_existing_string_helper() {
        let input = "a\"\n\t\\\u{01}中";
        let mut body = LlmRequestBody::with_estimated_capacity(64, 1024);
        body.push_json_string(input).expect("write escaped");

        let mut expected = String::new();
        crate::util::push_json_string_escaped(&mut expected, input);
        assert_eq!(body.as_ref(), expected.as_bytes());
    }

    #[test]
    fn write_enforces_max_len_as_config_error() {
        let mut body = LlmRequestBody::with_estimated_capacity(8, 8);
        let err = body.push_str("123456789").expect_err("oversize body");

        assert!(matches!(err, crate::error::Error::Config { .. }));
    }

    #[test]
    fn escaped_content_can_promote_from_small_estimate() {
        let input = "\"".repeat(ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD - 1);
        let mut body =
            LlmRequestBody::with_estimated_capacity(input.len(), input.len().saturating_mul(3));

        body.push_json_string(&input).expect("write escaped");

        assert!(body.is_external_preferred());
    }

    #[test]
    fn initial_allocation_is_capped_at_max_len() {
        let body = LlmRequestBody::with_estimated_capacity(
            ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD * 128,
            32,
        );

        assert!(!body.is_external_preferred());
    }
}
