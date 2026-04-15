#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod caldav;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod feishu;
