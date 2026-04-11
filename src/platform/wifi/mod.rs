//! WiFi：ESP 上 SoftAP+STA；Linux 嵌入式接入系统 WiFi 栈；其余 host 为桩。
//! WiFi: SoftAP+STA on ESP; system WiFi stack on Linux embedded; stubs on other hosts.

mod linux_startup_policy;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
mod esp;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub use esp::*;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[cfg(target_os = "linux")]
mod linux_ctrl;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[cfg(target_os = "linux")]
mod linux_embedded;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[cfg(target_os = "linux")]
pub use linux_embedded::*;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[cfg(not(target_os = "linux"))]
mod host;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[cfg(not(target_os = "linux"))]
pub use host::*;
