//! 企业微信通道：智能机器人长连接出入站。

mod aibot;
pub use aibot::{
    check_connectivity, new_wecom_aibot_route_store, run_wecom_aibot_loop, WecomAibotRouteStore,
    WECOM_AIBOT_WS_URL,
};
