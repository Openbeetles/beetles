#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod feishu;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod webdav;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod wecom;
