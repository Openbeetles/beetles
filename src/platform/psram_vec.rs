//! PSRAM 分配的 Vec 包装器，用于大块缓冲（音频、编码输出等）。
//! PSRAM-backed Vec wrapper for large buffers (audio, encoding output, etc.).

use super::heap::{alloc_spiram_buffer, free_spiram_buffer};

enum Backing<T> {
    Spiram {
        ptr: *mut T,
        capacity: usize,
        len: usize,
    },
    Heap(Vec<T>),
}

/// PSRAM 分配的泛型缓冲区。分配失败时回退到普通 Vec。
/// 支持两种模式：
/// - 固定容量（`new`）：全量初始化为零值，`len == capacity`
/// - 可增长（`with_max_capacity`）：预分配 capacity，`len` 从 0 增长
pub struct PsramVec<T> {
    backing: Backing<T>,
}

impl<T: Copy + Default> PsramVec<T> {
    /// 分配并零初始化 `capacity` 个元素。`len() == capacity`。
    /// 分配并零初始化，`len() == capacity`。
    pub fn new(capacity: usize) -> Self {
        let byte_size = capacity * std::mem::size_of::<T>();
        if let Some(ptr) = alloc_spiram_buffer(byte_size) {
            unsafe { std::ptr::write_bytes(ptr, 0, byte_size) };
            Self {
                backing: Backing::Spiram {
                    ptr: ptr as *mut T,
                    capacity,
                    len: capacity,
                },
            }
        } else {
            Self {
                backing: Backing::Heap(vec![T::default(); capacity]),
            }
        }
    }

    /// 预分配 `max_capacity` 但 `len` 从 0 开始，可通过 `extend_from_slice` 追加。
    /// PSRAM 上不支持 realloc，因此一次性分配最大容量。
    pub fn with_max_capacity(max_capacity: usize) -> Self {
        let byte_size = max_capacity * std::mem::size_of::<T>();
        if let Some(ptr) = alloc_spiram_buffer(byte_size) {
            Self {
                backing: Backing::Spiram {
                    ptr: ptr as *mut T,
                    capacity: max_capacity,
                    len: 0,
                },
            }
        } else {
            Self {
                backing: Backing::Heap(Vec::with_capacity(max_capacity)),
            }
        }
    }
}

impl<T> PsramVec<T> {
    pub fn from_heap_vec(vec: Vec<T>) -> Self {
        Self {
            backing: Backing::Heap(vec),
        }
    }

    pub fn as_slice(&self) -> &[T] {
        match &self.backing {
            Backing::Spiram { ptr, len, .. } => unsafe { std::slice::from_raw_parts(*ptr, *len) },
            Backing::Heap(v) => v.as_slice(),
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        match &mut self.backing {
            Backing::Spiram { ptr, len, .. } => unsafe {
                std::slice::from_raw_parts_mut(*ptr, *len)
            },
            Backing::Heap(v) => v.as_mut_slice(),
        }
    }

    pub fn len(&self) -> usize {
        match &self.backing {
            Backing::Spiram { len, .. } => *len,
            Backing::Heap(v) => v.len(),
        }
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[allow(dead_code)]
    pub fn capacity(&self) -> usize {
        match &self.backing {
            Backing::Spiram { capacity, .. } => *capacity,
            Backing::Heap(v) => v.capacity(),
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        match &mut self.backing {
            Backing::Spiram { len, .. } => *len = 0,
            Backing::Heap(v) => v.clear(),
        }
    }

    pub fn into_vec(self) -> Vec<T>
    where
        T: Copy,
    {
        let mut this = std::mem::ManuallyDrop::new(self);
        match &mut this.backing {
            Backing::Spiram { ptr, len, .. } => {
                let out = if !ptr.is_null() && *len > 0 {
                    unsafe { std::slice::from_raw_parts(*ptr, *len).to_vec() }
                } else {
                    Vec::new()
                };
                unsafe { free_spiram_buffer(*ptr as *mut u8) };
                out
            }
            Backing::Heap(v) => std::mem::take(v),
        }
    }
}

impl<T: Copy> PsramVec<T> {
    /// 追加切片。PSRAM 路径在容量耗尽时静默截断；Heap 路径可正常增长。
    pub fn extend_from_slice(&mut self, data: &[T]) {
        match &mut self.backing {
            Backing::Spiram { ptr, capacity, len } => {
                let avail = *capacity - *len;
                let take = data.len().min(avail);
                if take > 0 {
                    unsafe {
                        std::ptr::copy_nonoverlapping(data.as_ptr(), (*ptr).add(*len), take);
                    }
                    *len += take;
                }
            }
            Backing::Heap(v) => v.extend_from_slice(data),
        }
    }
}

impl std::io::Write for PsramVec<u8> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &mut self.backing {
            Backing::Spiram { ptr, capacity, len } => {
                let avail = *capacity - *len;
                let take = buf.len().min(avail);
                if take > 0 {
                    unsafe {
                        std::ptr::copy_nonoverlapping(buf.as_ptr(), (*ptr).add(*len), take);
                    }
                    *len += take;
                }
                Ok(take)
            }
            Backing::Heap(v) => {
                v.extend_from_slice(buf);
                Ok(buf.len())
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<T> Drop for PsramVec<T> {
    fn drop(&mut self) {
        match &mut self.backing {
            Backing::Spiram { ptr, .. } => unsafe { free_spiram_buffer(*ptr as *mut u8) },
            Backing::Heap(_) => {}
        }
    }
}

// The wrapper owns the backing allocation and only exposes mutable access
// through `&mut self`; sharing immutable slices across threads is safe when T
// is Sync, and moving ownership is safe when T is Send.
unsafe impl<T: Send> Send for PsramVec<T> {}
unsafe impl<T: Sync> Sync for PsramVec<T> {}

impl<T> std::ops::Deref for PsramVec<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T> AsRef<[T]> for PsramVec<T> {
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T> From<Vec<T>> for PsramVec<T> {
    fn from(value: Vec<T>) -> Self {
        Self::from_heap_vec(value)
    }
}

#[cfg(test)]
mod tests {
    use super::PsramVec;

    #[test]
    fn heap_backed_into_vec_round_trips_bytes() {
        let mut buf = PsramVec::from(vec![1u8, 2, 3]);
        buf.extend_from_slice(&[4, 5]);

        assert_eq!(buf.as_ref(), &[1, 2, 3, 4, 5]);
        assert_eq!(buf.into_vec(), vec![1, 2, 3, 4, 5]);
    }
}
