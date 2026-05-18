//! WSS 网关入站抽象：协议驱动 trait + 传输 trait + 统一循环。
//! 仅依赖 bus、error、ChannelHttpClient；不依赖 platform/esp_idf。
//! 扩展新通道：实现 WssGatewayDriver，在 ESP 上提供 WssConnection 实现并调用 run_wss_gateway_loop。

mod connection;
#[cfg(any(feature = "feishu", feature = "wecom", feature = "qq_channel", test))]
mod driver;
#[cfg(any(feature = "feishu", feature = "wecom", feature = "qq_channel", test))]
mod r#loop;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
mod esp_conn;

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod linux_conn;

#[allow(unused_imports)]
pub use connection::{WssCloseInfo, WssConnectProfile, WssConnection, WssEvent};
#[cfg(any(feature = "feishu", feature = "wecom", feature = "qq_channel", test))]
#[allow(unused_imports)]
pub use driver::{WssGatewayDriver, WssRecvAction, WssSessionState};
#[cfg(any(feature = "feishu", feature = "wecom", feature = "qq_channel", test))]
#[allow(unused_imports)]
pub use r#loop::run_wss_gateway_loop;
#[cfg(any(feature = "feishu", feature = "wecom", feature = "qq_channel", test))]
#[allow(unused_imports)]
pub(crate) use r#loop::{
    external_wss_connect_gate, external_wss_session_stop_reason,
    external_wss_worker_should_exit_for_evict,
};

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
#[allow(unused_imports)]
pub use esp_conn::{
    connect_esp_wss, connect_esp_wss_with_headers_and_profile, connect_esp_wss_with_profile,
    EspWssConnection,
};

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[allow(unused_imports)]
pub use linux_conn::{
    connect_linux_wss, connect_linux_wss_with_headers_and_profile, connect_linux_wss_with_profile,
    LinuxWssConnection,
};

/// 平台 WSS 建连：ESP 用 `esp-idf` websocket；Linux 用 `tungstenite`+rustls。供 `run_*_ws_loop` 注入。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) fn connect_wss(url: &str) -> crate::error::Result<esp_conn::EspWssConnection> {
    esp_conn::connect_esp_wss_with_profile(url, WssConnectProfile::Gateway)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) fn connect_wss_with_headers_and_profile(
    url: &str,
    headers: &[(&str, &str)],
    profile: WssConnectProfile,
) -> crate::error::Result<esp_conn::EspWssConnection> {
    esp_conn::connect_esp_wss_with_headers_and_profile(url, headers, profile)
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(crate) fn connect_wss(url: &str) -> crate::error::Result<linux_conn::LinuxWssConnection> {
    linux_conn::connect_linux_wss_with_profile(url, WssConnectProfile::Gateway)
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(crate) fn connect_wss_with_headers_and_profile(
    url: &str,
    headers: &[(&str, &str)],
    profile: WssConnectProfile,
) -> crate::error::Result<linux_conn::LinuxWssConnection> {
    linux_conn::connect_linux_wss_with_headers_and_profile(url, headers, profile)
}
