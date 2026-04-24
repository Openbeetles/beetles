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

const DEFAULT_SESSION_MAX_MESSAGES = 32

const LEGACY_AVAILABLE_CHANNELS = [
  '',
  'telegram',
  'feishu',
  'dingtalk',
  'wecom',
  'qq_channel',
] as const

function objectRecord(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object'
    ? (value as Record<string, unknown>)
    : {}
}

function stringValue(value: unknown, fallback = ''): string {
  return typeof value === 'string' ? value : fallback
}

function finiteNumberValue(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback
}

function nullableStringValue(value: unknown): string | null {
  return typeof value === 'string' ? value : null
}

function normalizeLlmSource(value: unknown): LlmSource | null {
  const record = objectRecord(value)
  if (Object.keys(record).length === 0) return null
  return {
    provider: stringValue(record.provider),
    api_key: stringValue(record.api_key),
    model: stringValue(record.model),
    api_url: stringValue(record.api_url),
  }
}

function normalizeSourceIndex(value: unknown, sourceCount: number): number | null {
  if (typeof value !== 'number' || !Number.isInteger(value)) return null
  return value >= 0 && value < sourceCount ? value : null
}

function normalizeAvailableChannels(value: unknown): string[] {
  if (!Array.isArray(value)) return [...LEGACY_AVAILABLE_CHANNELS]

  const seen = new Set<string>()
  const channels: string[] = []
  for (const item of value) {
    if (typeof item !== 'string') continue
    const channel = item.trim()
    if (seen.has(channel)) continue
    seen.add(channel)
    channels.push(channel)
  }

  return channels.length > 0 ? channels : ['']
}

/** 合并 API 返回（旧固件或局部响应可能缺省字段）为完整 LlmConfigSegment。 */
export function normalizeLlmConfigFromDevice(
  raw: unknown,
): LlmConfigSegment {
  const record = objectRecord(raw)
  const sources = Array.isArray(record.llm_sources)
    ? record.llm_sources
        .map((source) => normalizeLlmSource(source))
        .filter((source): source is LlmSource => source !== null)
    : []

  return {
    llm_sources: sources,
    llm_router_source_index: normalizeSourceIndex(
      record.llm_router_source_index,
      sources.length,
    ),
    llm_worker_source_index: normalizeSourceIndex(
      record.llm_worker_source_index,
      sources.length,
    ),
  }
}

/** 合并 API 返回（旧固件或局部响应可能缺省字段）为完整 ChannelsConfigView。 */
export function normalizeChannelsConfigFromDevice(
  raw: Partial<ChannelsConfigView> | null | undefined,
): ChannelsConfigView {
  const tgGroupActivation =
    raw?.tg_group_activation === 'always' ? 'always' : 'mention'
  const unavailableEnabledChannel = stringValue(raw?.unavailable_enabled_channel).trim()

  return {
    enabled_channel: stringValue(raw?.enabled_channel),
    tg_token: stringValue(raw?.tg_token),
    tg_allowed_chat_ids: stringValue(raw?.tg_allowed_chat_ids),
    tg_group_activation: tgGroupActivation,
    feishu_app_id: stringValue(raw?.feishu_app_id),
    feishu_app_secret: stringValue(raw?.feishu_app_secret),
    feishu_allowed_chat_ids: stringValue(raw?.feishu_allowed_chat_ids),
    dingtalk_client_id: stringValue(raw?.dingtalk_client_id),
    dingtalk_client_secret: stringValue(raw?.dingtalk_client_secret),
    wecom_bot_id: stringValue(raw?.wecom_bot_id),
    wecom_bot_secret: stringValue(raw?.wecom_bot_secret),
    wecom_ws_url: stringValue(raw?.wecom_ws_url),
    qq_channel_app_id: stringValue(raw?.qq_channel_app_id),
    qq_channel_secret: stringValue(raw?.qq_channel_secret),
    webhook_enabled: raw?.webhook_enabled === true,
    webhook_token: stringValue(raw?.webhook_token),
    available_channels: normalizeAvailableChannels(raw?.available_channels),
    ...(unavailableEnabledChannel
      ? { unavailable_enabled_channel: unavailableEnabledChannel }
      : {}),
  }
}

/** 合并 API 返回（旧固件或局部响应可能缺省字段）为完整 SystemConfigSegment。 */
export function normalizeSystemConfigFromDevice(
  raw: unknown,
): SystemConfigSegment {
  const record = objectRecord(raw)
  return {
    wifi_ssid: stringValue(record.wifi_ssid),
    wifi_pass: stringValue(record.wifi_pass),
    proxy_url: stringValue(record.proxy_url),
    session_max_messages: finiteNumberValue(
      record.session_max_messages,
      DEFAULT_SESSION_MAX_MESSAGES,
    ),
    tg_group_activation:
      record.tg_group_activation === 'always' ? 'always' : 'mention',
    locale: nullableStringValue(record.locale),
  }
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
