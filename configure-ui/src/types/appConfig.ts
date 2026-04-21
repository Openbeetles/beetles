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
  feishu_allowed_chat_ids: string
  dingtalk_webhook_url: string
  wecom_corp_id: string
  wecom_corp_secret: string
  wecom_agent_id: string
  wecom_default_touser: string
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
  /** 当前启用的通道（仅一个）："" | "telegram" | "feishu" | "dingtalk" | "wecom" | "qq_channel" */
  enabled_channel: string
  llm_sources: LlmSource[]
  llm_router_source_index: number | null
  llm_worker_source_index: number | null
}

/** POST /api/config/llm 请求体。 */
export interface LlmConfigSegment {
  llm_sources: LlmSource[]
  llm_router_source_index?: number | null
  llm_worker_source_index?: number | null
}

/** 可选启用通道值，与后端 ALLOWED_ENABLED_CHANNELS 一致。 */
export const ENABLED_CHANNEL_OPTIONS = [
  { value: "", labelKey: "config.enabledChannel_none" },
  { value: "telegram", labelKey: "config.enabledChannel_telegram" },
  { value: "feishu", labelKey: "config.enabledChannel_feishu" },
  { value: "dingtalk", labelKey: "config.enabledChannel_dingtalk" },
  { value: "wecom", labelKey: "config.enabledChannel_wecom" },
  { value: "qq_channel", labelKey: "config.enabledChannel_qq_channel" },
] as const

/** POST /api/config/channels 请求体。 */
export interface ChannelsConfigSegment {
  enabled_channel: string
  tg_token: string
  tg_allowed_chat_ids: string
  feishu_app_id: string
  feishu_app_secret: string
  feishu_allowed_chat_ids: string
  dingtalk_webhook_url: string
  wecom_corp_id: string
  wecom_corp_secret: string
  wecom_agent_id: string
  wecom_default_touser: string
  qq_channel_app_id: string
  qq_channel_secret: string
  webhook_enabled: boolean
  webhook_token: string
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
