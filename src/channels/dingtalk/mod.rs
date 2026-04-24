//! 钉钉通道：Stream Mode 入站与 sessionWebhook 出站。

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod inbound;
mod send;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod session;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod stream;
pub use send::{check_connectivity, flush_dingtalk_sends, run_dingtalk_sender_loop};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use session::{active_session_webhook, store_session_webhook, DingtalkSessionStore};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use stream::run_dingtalk_stream_loop;
