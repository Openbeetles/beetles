//! 企业微信通道：出站 Sink/flush，连通性检查；入站 webhook。

mod send;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod webhook;
pub use send::{check_connectivity, flush_wecom_sends, run_wecom_sender_loop};
