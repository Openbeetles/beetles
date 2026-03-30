//! PSRAM 分配的 Vec 包装器，用于大块音频缓冲。
//! PSRAM-backed Vec wrapper for large audio buffers.

use super::heap::{alloc_spiram_buffer, free_spiram_buffer};

enum Backing {
    Spiram { ptr: *mut i16, len: usize },
    Heap(Vec<i16>),
}

/// PSRAM 分配的 i16 缓冲区。失败时回退到普通 Vec。
pub struct PsramVecI16 {
    backing: Backing,
}

impl PsramVecI16 {
    pub fn new(capacity: usize) -> Self {
        let byte_size = capacity * std::mem::size_of::<i16>();
        if let Some(ptr) = alloc_spiram_buffer(byte_size) {
            unsafe {
                std::ptr::write_bytes(ptr, 0, byte_size);
            }
            Self {
                backing: Backing::Spiram {
                    ptr: ptr as *mut i16,
                    len: capacity,
                },
            }
        } else {
            Self {
                backing: Backing::Heap(vec![0i16; capacity]),
            }
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [i16] {
        match &mut self.backing {
            Backing::Spiram { ptr, len } => unsafe { std::slice::from_raw_parts_mut(*ptr, *len) },
            Backing::Heap(v) => v.as_mut_slice(),
        }
    }

    pub fn len(&self) -> usize {
        match &self.backing {
            Backing::Spiram { len, .. } => *len,
            Backing::Heap(v) => v.len(),
        }
    }
}

impl Drop for PsramVecI16 {
    fn drop(&mut self) {
        match &mut self.backing {
            Backing::Spiram { ptr, .. } => unsafe { free_spiram_buffer(*ptr as *mut u8) },
            Backing::Heap(_) => {}
        }
    }
}

unsafe impl Send for PsramVecI16 {}
