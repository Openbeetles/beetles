/**
 * 与 Rust AppConfig / LlmSource 一一对应，供 GET /api/config 及按段保存接口使用。
 * 约束见 CONFIG_API.md 与 pocket_crayfish src/config.rs。
 */

export interface LlmSource {
  provider: string
  api_key: string
  model: string
  api_url: string
}

export interface AppConfig {
  wifi_ssid: string
  wifi_pass: string
  tg_token: string
  tg_allowed_chat_ids: string
  feishu_app_id: string
  feishu_app_secret: string
  feishu_verification_token: string
  feishu_encrypt_key: string
  feishu_allowed_chat_ids: string
  dingtalk_webhook_url: string
  wecom_corp_id: string
  wecom_corp_secret: string
  wecom_agent_id: string
  wecom_default_touser: string
  wecom_token: string
  wecom_encoding_aes_key: string
  dingtalk_app_secret: string
  qq_channel_app_id: string
  qq_channel_secret: string
  api_key: string
  model: string
  model_provider: string
  api_url: string
  /** 代理 URL，如 http://proxy.example.com:8080；留空直连。 */
  proxy_url: string
  search_key: string
  tavily_key: string
  tg_group_activation: string
  session_max_messages: number
  webhook_enabled: boolean
  webhook_token: string
  /** 当前启用的通道（仅一个；可选值受当前固件编译产物约束）。 */
  enabled_channel: string
  llm_sources: LlmSource[]
  llm_router_source_index: number | null
  llm_worker_source_index: number | null
}

/** GET/POST /api/config/llm 读写模型。 */
export interface LlmConfigSegment {
  llm_sources: LlmSource[]
  llm_router_source_index?: number | null
  llm_worker_source_index?: number | null
}

export function llmConfigSegmentFromAppConfig(config: AppConfig): LlmConfigSegment {
  const sources =
    config.llm_sources?.length > 0
      ? config.llm_sources
      : [
          {
            provider: config.model_provider || "",
            api_key: config.api_key || "",
            model: config.model || "",
            api_url: config.api_url || "",
          },
        ]
  return {
    llm_sources: sources.map((source) => ({ ...source })),
    llm_router_source_index: config.llm_router_source_index ?? null,
    llm_worker_source_index: config.llm_worker_source_index ?? null,
  }
}

/** POST /api/config/channels 请求体。 */
export interface ChannelsConfigSegment {
  enabled_channel: string
  tg_token: string
  tg_allowed_chat_ids: string
  tg_group_activation: string
  feishu_app_id: string
  feishu_app_secret: string
  feishu_verification_token: string
  feishu_encrypt_key: string
  feishu_allowed_chat_ids: string
  dingtalk_webhook_url: string
  wecom_corp_id: string
  wecom_corp_secret: string
  wecom_agent_id: string
  wecom_default_touser: string
  wecom_token: string
  wecom_encoding_aes_key: string
  dingtalk_app_secret: string
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
/** POST /api/config/system 请求体。 */
export interface SystemConfigSegment {
  wifi_ssid: string
  wifi_pass: string
  proxy_url: string
  session_max_messages: number
  tg_group_activation: string
  locale?: string | null
}
