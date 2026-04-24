/**
 * 配置面分段读写模型；与 Rust 侧 segment schema 对齐。
 * 约束见 CONFIG_API.md 与 src/config.rs。
 */

export interface LlmSource {
  provider: string
  api_key: string
  model: string
  api_url: string
}

/** GET/POST /api/config/llm 读写模型。 */
export interface LlmConfigSegment {
  llm_sources: LlmSource[]
  llm_router_source_index?: number | null
  llm_worker_source_index?: number | null
}

/** POST /api/config/channels 请求体。 */
export interface ChannelsConfigSegment {
  enabled_channel: string
  tg_token: string
  tg_allowed_chat_ids: string
  tg_group_activation: string
  feishu_app_id: string
  feishu_app_secret: string
  feishu_allowed_chat_ids: string
  dingtalk_client_id: string
  dingtalk_client_secret: string
  wecom_bot_id: string
  wecom_bot_secret: string
  wecom_ws_url: string
  qq_channel_app_id: string
  qq_channel_secret: string
  webhook_enabled: boolean
  webhook_token: string
}

export interface ChannelsConfigView extends ChannelsConfigSegment {
  available_channels: string[]
  unavailable_enabled_channel?: string
}

export function enabledChannelLabelKey(channelId: string): string {
  switch (channelId) {
    case "":
      return "config.enabledChannel_none"
    case "telegram":
      return "config.enabledChannel_telegram"
    case "feishu":
      return "config.enabledChannel_feishu"
    case "dingtalk":
      return "config.enabledChannel_dingtalk"
    case "wecom":
      return "config.enabledChannel_wecom"
    case "qq_channel":
      return "config.enabledChannel_qq_channel"
    default:
      return channelId
  }
}
/** GET/POST /api/config/system 读写系统段。 */
export interface SystemConfigSegment {
  wifi_ssid: string
  wifi_pass: string
  proxy_url: string
  session_max_messages: number
  tg_group_activation: string
  locale?: string | null
}
