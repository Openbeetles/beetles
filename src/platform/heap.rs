//! ESP 堆查询与 PSRAM 大块分配：供 orchestrator、HTTP 响应体与可观测性复用。
//! Heap query and PSRAM allocation for ESP.

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::error::Error;
use crate::error::Result;

/// 返回当前内部堆空闲字节数。仅 ESP 目标有效；非 ESP 返回 u32::MAX（视为充足）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn heap_free_internal() -> usize {
    unsafe {
        esp_idf_svc::sys::heap_caps_get_free_size(esp_idf_svc::sys::MALLOC_CAP_INTERNAL) as usize
    }
}

/// 返回当前 PSRAM 空闲字节数。仅 ESP 目标有效；无 PSRAM 时返回 0。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn heap_free_spiram() -> usize {
    unsafe {
        esp_idf_svc::sys::heap_caps_get_free_size(esp_idf_svc::sys::MALLOC_CAP_SPIRAM) as usize
    }
}

/// 返回 internal 堆当前最大连续空闲块（字节）。仅 ESP 有效；非 ESP 返回 usize::MAX。供 TLS 准入碎片化判定。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn heap_largest_free_block_internal() -> usize {
    unsafe {
        esp_idf_svc::sys::heap_caps_get_largest_free_block(esp_idf_svc::sys::MALLOC_CAP_INTERNAL)
            as usize
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[allow(dead_code)]
pub fn heap_free_internal() -> usize {
    usize::MAX
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[allow(dead_code)]
pub fn heap_free_spiram() -> usize {
    0
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[allow(dead_code)]
pub fn heap_largest_free_block_internal() -> usize {
    usize::MAX
}

/// 返回内部堆历史最小空闲字节数。仅 ESP 目标有效；非 ESP 返回 usize::MAX。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn heap_min_free_internal() -> usize {
    unsafe {
        esp_idf_svc::sys::heap_caps_get_minimum_free_size(esp_idf_svc::sys::MALLOC_CAP_INTERNAL)
            as usize
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[allow(dead_code)]
pub fn heap_min_free_internal() -> usize {
    usize::MAX
}

/// S3 上从 PSRAM 分配大块缓冲区；无 PSRAM 或失败返回 None。调用方负责 heap_caps_free。
#[cfg(target_arch = "xtensa")]
pub fn alloc_spiram_buffer(size: usize) -> Option<*mut u8> {
    let ptr =
        unsafe { esp_idf_svc::sys::heap_caps_malloc(size, esp_idf_svc::sys::MALLOC_CAP_SPIRAM) };
    if ptr.is_null() {
        None
    } else {
        Some(ptr as *mut u8)
    }
}

/// # Safety
///
/// `ptr` must be a pointer previously returned by `alloc_spiram_buffer`, or null.
/// Calling this with any other pointer is undefined behavior.
#[cfg(target_arch = "xtensa")]
pub unsafe fn free_spiram_buffer(ptr: *mut u8) {
    if !ptr.is_null() {
        esp_idf_svc::sys::heap_caps_free(ptr as *mut core::ffi::c_void);
    }
}

#[cfg(not(target_arch = "xtensa"))]
#[allow(dead_code)]
pub fn alloc_spiram_buffer(_size: usize) -> Option<*mut u8> {
    None
}

#[cfg(not(target_arch = "xtensa"))]
#[allow(dead_code)]
pub unsafe fn free_spiram_buffer(_ptr: *mut u8) {}

/// 调试期堆完整性断点：用于把 heap 破坏收窄到具体启动阶段。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn debug_heap_checkpoint(stage: &'static str) -> Result<()> {
    let ok = unsafe { esp_idf_svc::sys::heap_caps_check_integrity_all(true) };
    if ok {
        Ok(())
    } else {
        Err(Error::config(stage, "heap integrity check failed"))
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn debug_heap_checkpoint(_stage: &'static str) -> Result<()> {
    Ok(())
}
