//! WSS 网关协议驱动：取 URL、解析 Hello/鉴权、解析收包、构造心跳。
//! Protocol driver for WSS gateway: get URL, parse hello/identify, parse incoming frames, build heartbeat.

use crate::bus::PcMsg;
use crate::channels::ChannelHttpClient;
use crate::error::Result;

/// 建连后首包（如 QQ Hello）解析结果：心跳间隔与可选鉴权报文。
#[derive(Debug)]
pub struct WssSessionState {
    pub heartbeat_interval_ms: u64,
    pub identify_payload: Option<Vec<u8>>,
}

/// 每收到一条 Binary 后驱动返回的动作。
#[derive(Debug)]
pub enum WssRecvAction {
    /// 需入队消息（None 表示本帧不产生消息，如非 AT_MESSAGE_CREATE）
    Dispatch(Option<Box<PcMsg>>),
    /// 入队消息并立即回送 ACK 帧（飞书长连接需确认，否则服务端重复投递）
    #[cfg(feature = "feishu")]
    DispatchAndAck(Option<Box<PcMsg>>, Vec<u8>),
    /// 需按协议发送心跳，seq 为下次心跳的 d 字段（如 QQ 的 s）
    SendHeartbeat(u64),
    /// 忽略本帧
    Ignore,
    /// 服务端要求重连（如 QQ op=7 Reconnect）
    Disconnect,
}

impl WssRecvAction {
    /// 构造普通入站派发动作；消息较大，进入 enum 前装箱，避免放大所有变体。
    pub fn dispatch(msg: Option<PcMsg>) -> Self {
        Self::Dispatch(msg.map(Box::new))
    }

    /// 构造带 ACK 的入站派发动作；用于需要服务端确认的 WSS 协议。
    #[cfg(feature = "feishu")]
    pub fn dispatch_and_ack(msg: Option<PcMsg>, ack: Vec<u8>) -> Self {
        Self::DispatchAndAck(msg.map(Box::new), ack)
    }
}

/// WSS 网关协议驱动：由各通道（飞书、QQ）实现。
pub trait WssGatewayDriver {
    /// 通过 HTTP 获取 WSS 地址（如飞书 POST endpoint、QQ GET gateway）。
    fn get_url(&mut self, http: &mut dyn ChannelHttpClient) -> Result<String>;
    /// 是否期待建连后首条为 Hello（如 QQ op=10）；飞书为 false，不消费首包。
    fn expects_hello(&self) -> bool {
        false
    }
    /// 建连后首条消息（仅当 expects_hello() 为 true 时调用）：QQ 解析 op=10 返回 interval + Identify。
    fn on_hello(&mut self, first_message: &[u8]) -> Result<WssSessionState>;
    /// 每条 Binary 收包后的协议解析，返回入队/心跳/忽略/断开。
    fn on_recv(&mut self, data: &[u8]) -> Result<WssRecvAction>;
    /// 构造心跳帧；seq 为协议所需序号（QQ 为 s，飞书忽略）。
    fn build_heartbeat(&self, seq: Option<u64>) -> Result<Vec<u8>>;
    /// 会话已进入可用态（已建连，且需要的话已完成协议鉴权）。
    fn on_session_started(&mut self) {}
    /// 会话已退出。
    fn on_session_ended(&mut self) {}
}
