//! HTTP 响应体句柄：PSRAM 路径不 to_vec，仅持 (ptr,len) 并在 Drop 时释放，消除双缓冲。
//! Response body handle: PSRAM path holds (ptr, len) and frees on Drop; no heap copy.

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::platform::heap::free_spiram_buffer;

/// 响应体：Heap 为堆上 Vec；PSRAM 仅 xtensa 存在，Drop 时释放 PSRAM。
#[derive(Debug)]
pub enum ResponseBody {
    Heap(Vec<u8>),
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    PSRAM {
        ptr: Option<*mut u8>,
        len: usize,
    },
}

impl ResponseBody {
    pub fn as_slice(&self) -> &[u8] {
        match self {
            ResponseBody::Heap(v) => v.as_ref(),
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
            ResponseBody::PSRAM { ptr, len } => match ptr {
                Some(p) if !p.is_null() && *len > 0 => unsafe {
                    std::slice::from_raw_parts(*p, *len)
                },
                _ => &[],
            },
        }
    }

    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn truncate(&mut self, max_len: usize) {
        match self {
            ResponseBody::Heap(v) => v.truncate(max_len),
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
            ResponseBody::PSRAM { len, .. } => {
                if *len > max_len {
                    *len = max_len;
                }
            }
        }
    }

    /// 调用方确需拥有 Vec 时再分配（如部分 channel/工具需保留字节）。
    pub fn into_vec(&mut self) -> Vec<u8> {
        match self {
            ResponseBody::Heap(v) => std::mem::take(v),
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
            ResponseBody::PSRAM { ptr, len } => {
                let p = ptr.take();
                let len = *len;
                let v = match p {
                    Some(p) if !p.is_null() && len > 0 => unsafe {
                        std::slice::from_raw_parts(p, len).to_vec()
                    },
                    _ => Vec::new(),
                };
                if let Some(p) = p {
                    if !p.is_null() {
                        unsafe { free_spiram_buffer(p) };
                    }
                }
                v
            }
        }
    }
}

impl AsRef<[u8]> for ResponseBody {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl Drop for ResponseBody {
    fn drop(&mut self) {
        if let ResponseBody::PSRAM { ptr, .. } = self {
            if let Some(p) = ptr.take() {
                if !p.is_null() {
                    unsafe { free_spiram_buffer(p) };
                }
            }
        }
    }
}
