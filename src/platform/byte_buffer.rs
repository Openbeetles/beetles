//! Owned byte buffer with ESP large-buffer external-allocation preference.
//! 统一的 owned 字节缓冲；ESP 大对象优先走外部内存，避免各调用链复制 PSRAM 逻辑。

use crate::platform::psram_vec::PsramVec;
use std::io::{self, Write};

enum ByteBufferInner {
    Heap(Vec<u8>),
    ExternalPreferred(PsramVec<u8>),
}

/// Owned bytes for request bodies and state files.
///
/// Small buffers stay in a normal `Vec<u8>`. Buffers whose initial capacity is
/// above `EXTERNAL_PREFERRED_THRESHOLD` use the shared `PsramVec` path, which is
/// PSRAM-backed on ESP when available and falls back to heap elsewhere.
pub struct ByteBuffer {
    inner: ByteBufferInner,
}

impl std::fmt::Debug for ByteBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ByteBuffer")
            .field("len", &self.len())
            .field("external_preferred", &self.is_external_preferred())
            .finish()
    }
}

impl ByteBuffer {
    /// Large buffers above this threshold prefer external allocation.
    pub const EXTERNAL_PREFERRED_THRESHOLD: usize = 8 * 1024;

    pub fn empty() -> Self {
        Self::from_vec(Vec::new())
    }

    pub fn with_capacity(capacity: usize) -> Self {
        if capacity >= Self::EXTERNAL_PREFERRED_THRESHOLD {
            Self {
                inner: ByteBufferInner::ExternalPreferred(PsramVec::with_max_capacity(capacity)),
            }
        } else {
            Self {
                inner: ByteBufferInner::Heap(Vec::with_capacity(capacity)),
            }
        }
    }

    pub fn zeroed(len: usize) -> Self {
        if len >= Self::EXTERNAL_PREFERRED_THRESHOLD {
            Self {
                inner: ByteBufferInner::ExternalPreferred(PsramVec::new(len)),
            }
        } else {
            Self {
                inner: ByteBufferInner::Heap(vec![0; len]),
            }
        }
    }

    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self {
            inner: ByteBufferInner::Heap(bytes),
        }
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    pub(crate) fn from_psram_vec(bytes: PsramVec<u8>) -> Self {
        Self {
            inner: ByteBufferInner::ExternalPreferred(bytes),
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        match &self.inner {
            ByteBufferInner::Heap(bytes) => bytes.as_slice(),
            ByteBufferInner::ExternalPreferred(bytes) => bytes.as_ref(),
        }
    }

    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_external_preferred(&self) -> bool {
        matches!(self.inner, ByteBufferInner::ExternalPreferred(_))
    }

    pub fn into_vec(self) -> Vec<u8> {
        match self.inner {
            ByteBufferInner::Heap(bytes) => bytes,
            ByteBufferInner::ExternalPreferred(bytes) => bytes.into_vec(),
        }
    }

    fn ensure_capacity_for(&mut self, additional: usize) -> io::Result<()> {
        if additional == 0 {
            return Ok(());
        }
        let needed = self
            .len()
            .checked_add(additional)
            .ok_or_else(|| io::Error::new(io::ErrorKind::OutOfMemory, "byte buffer too large"))?;
        match &mut self.inner {
            ByteBufferInner::Heap(bytes) => {
                if needed >= Self::EXTERNAL_PREFERRED_THRESHOLD {
                    let capacity = Self::grown_capacity(bytes.capacity(), needed);
                    let mut external = PsramVec::with_max_capacity(capacity);
                    external.write_all(bytes.as_slice())?;
                    self.inner = ByteBufferInner::ExternalPreferred(external);
                }
            }
            ByteBufferInner::ExternalPreferred(bytes) => {
                if needed > bytes.capacity() {
                    let capacity = Self::grown_capacity(bytes.capacity(), needed);
                    let mut external = PsramVec::with_max_capacity(capacity);
                    external.write_all(bytes.as_ref())?;
                    *bytes = external;
                }
            }
        }
        Ok(())
    }

    fn grown_capacity(current: usize, needed: usize) -> usize {
        let base = current.max(Self::EXTERNAL_PREFERRED_THRESHOLD);
        base.saturating_mul(2).max(needed)
    }
}

impl Default for ByteBuffer {
    fn default() -> Self {
        Self::empty()
    }
}

impl From<Vec<u8>> for ByteBuffer {
    fn from(value: Vec<u8>) -> Self {
        Self::from_vec(value)
    }
}

impl AsRef<[u8]> for ByteBuffer {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl std::ops::Deref for ByteBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl Write for ByteBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.ensure_capacity_for(buf.len())?;
        match &mut self.inner {
            ByteBufferInner::Heap(bytes) => {
                bytes.extend_from_slice(buf);
                Ok(buf.len())
            }
            ByteBufferInner::ExternalPreferred(bytes) => {
                bytes.write_all(buf)?;
                Ok(buf.len())
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ByteBuffer;
    use std::io::Write;

    #[test]
    fn small_buffer_stays_heap_backed() {
        let mut buffer = ByteBuffer::with_capacity(ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD - 1);
        buffer.write_all(b"abc").expect("write bytes");

        assert!(!buffer.is_external_preferred());
        assert_eq!(buffer.as_ref(), b"abc");
    }

    #[test]
    fn large_buffer_uses_external_preferred_variant() {
        let mut buffer = ByteBuffer::with_capacity(ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD);
        buffer.write_all(b"abc").expect("write bytes");

        assert!(buffer.is_external_preferred());
        assert_eq!(buffer.as_ref(), b"abc");
    }

    #[test]
    fn zeroed_large_buffer_uses_external_preferred_variant() {
        let buffer = ByteBuffer::zeroed(ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD);

        assert!(buffer.is_external_preferred());
        assert_eq!(buffer.len(), ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD);
        assert!(buffer.as_ref().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn into_vec_preserves_bytes() {
        let mut buffer = ByteBuffer::with_capacity(ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD);
        buffer.write_all(b"abc").expect("write bytes");

        assert_eq!(buffer.into_vec(), b"abc");
    }

    #[test]
    fn heap_buffer_promotes_when_write_crosses_threshold() {
        let mut buffer = ByteBuffer::with_capacity(16);
        let bytes = vec![b'a'; ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD + 1];

        buffer.write_all(&bytes).expect("write bytes");

        assert!(buffer.is_external_preferred());
        assert_eq!(buffer.as_ref(), bytes.as_slice());
    }

    #[test]
    fn external_buffer_grows_without_truncation() {
        let mut buffer = ByteBuffer::with_capacity(ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD);
        let bytes = vec![b'a'; ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD * 3];

        buffer.write_all(&bytes).expect("write bytes");

        assert!(buffer.is_external_preferred());
        assert_eq!(buffer.len(), bytes.len());
        assert_eq!(buffer.as_ref(), bytes.as_slice());
    }
}
