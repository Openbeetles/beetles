//! HTTP(S) 客户端：ESP 用 esp-idf-svc；host/Linux 用 `ureq`（rustls）。
//! HTTP(S) client: esp-idf-svc on ESP; `ureq` (rustls) on host/Linux.

pub(crate) mod response_buffer;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
mod esp;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub use esp::EspHttpClient;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod host;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use host::EspHttpClient;
