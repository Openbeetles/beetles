//! 飞书通道：长连接事件入站、OpenAPI 出站与连通性检查。
//! 一通道一目录，所有飞书相关逻辑集中于此。

pub(crate) mod send;
#[allow(unused_imports)]
pub use send::{
    acquire_tenant_token, check_connectivity, edit_message as feishu_edit_message,
    event_body_to_pcmsg, flush_feishu_sends, run_feishu_sender_loop,
    send_and_get_id as feishu_send_and_get_id, FeishuTokenCache, FeishuTokenRequest,
    FeishuTokenResponse, FEISHU_TOKEN_URL,
};

mod frame;
// pbbp2 仅 ws 使用，不对外 re-export

#[cfg(feature = "feishu")]
mod ws;
#[cfg(feature = "feishu")]
pub use ws::run_feishu_ws_loop;
