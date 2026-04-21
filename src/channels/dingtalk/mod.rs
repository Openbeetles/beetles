//! 钉钉通道：出站 Sink/flush，连通性检查；入站 webhook。

mod send;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod session;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod webhook;
pub use send::{check_connectivity, flush_dingtalk_sends, run_dingtalk_sender_loop};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use session::{active_session_webhook, store_session_webhook, DingtalkSessionStore};
