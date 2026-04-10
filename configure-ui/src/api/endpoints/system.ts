import { request, API_ERROR } from '../client'
import type { ApiResult } from '../client'

/** 与固件 `metrics::MetricsSnapshot` serde 字段一致。 */
export interface MetricsSnapshotData {
  messages_in?: number
  messages_out?: number
  llm_calls?: number
  llm_errors?: number
  llm_last_ms?: number
  tool_calls?: number
  tool_errors?: number
  wdt_feeds?: number
  dispatch_send_ok?: number
  dispatch_send_fail?: number
  errors_agent_router?: number
  errors_agent_chat?: number
  errors_agent_context?: number
  errors_tool_execute?: number
  errors_llm_request?: number
  errors_llm_parse?: number
  errors_channel_dispatch?: number
  errors_session_append?: number
  errors_other?: number
  last_active_epoch_secs?: number
  wifi_reconnect_total?: number
  wifi_ap_restart_total?: number
  wifi_last_failure_stage?: string
  audio_speaker_queue_depth_last_samples?: number
  audio_speaker_queue_depth_min_samples?: number
  audio_speaker_underrun_total?: number
}

/** 与固件 `orchestrator::ChannelHealthSnapshot` 一致。 */
export interface ChannelHealthSnapshotData {
  consecutive_failures?: number
  total_failures?: number
  total_successes?: number
  healthy?: boolean
}

/** 与固件 `orchestrator::ChannelsHealthSnapshot` 一致（具名通道）。 */
export interface ChannelsHealthSnapshotData {
  telegram?: ChannelHealthSnapshotData
  feishu?: ChannelHealthSnapshotData
  dingtalk?: ChannelHealthSnapshotData
  wecom?: ChannelHealthSnapshotData
  qq_channel?: ChannelHealthSnapshotData
}

/** 与固件 `orchestrator::ResourceBudget` 一致（嵌套在 resource 内）。 */
export interface ResourceBudgetData {
  level?: string
  system_prompt_max?: number
  messages_max?: number
  response_body_max?: number
  reconnect_backoff_secs?: number
  llm_hint?: string
}

/** 与固件 `orchestrator::ResourceSnapshot` 一致。 */
export interface ResourceSnapshotData {
  pressure?: string
  heap_free_internal?: number
  heap_free_spiram?: number
  heap_largest_block_internal?: number
  active_http_count?: number
  inbound_depth?: number
  outbound_depth?: number
  budget?: ResourceBudgetData
  channels?: ChannelsHealthSnapshotData
  session_count?: number
  storage_used_kb?: number
  storage_total_kb?: number
}

/** 与固件 `handlers/health.rs` 中 `DisplayHealth` 一致。 */
export interface HealthDisplayData {
  available?: boolean
}

export interface HealthAudioCapabilitiesData {
  microphone_input?: boolean
  speaker_output?: boolean
  concurrent_capture_playback?: boolean
  barge_in?: boolean
  reference_capture?: string
  echo_cancellation?: string
}

export interface HealthAudioData {
  duplex_profile?: string
  duplex_capabilities?: HealthAudioCapabilitiesData
}

export interface HealthData {
  wifi?: string
  last_error?: string
  display?: HealthDisplayData
  audio?: HealthAudioData
}

export interface DiagnoseItem {
  severity: string
  category: string
  message: string
}

export async function getHealth(baseUrl: string): Promise<ApiResult<HealthData>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const res = await request<HealthData>(baseUrl, '/api/health')
  return res
}

export async function getResource(baseUrl: string): Promise<ApiResult<ResourceSnapshotData>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  return request<ResourceSnapshotData>(baseUrl, '/api/resource')
}

export async function getMetrics(baseUrl: string): Promise<ApiResult<MetricsSnapshotData>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  return request<MetricsSnapshotData>(baseUrl, '/api/metrics')
}

export async function getDiagnose(baseUrl: string): Promise<ApiResult<DiagnoseItem[]>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const res = await request<DiagnoseItem[]>(baseUrl, '/api/diagnose')
  if (res.ok && Array.isArray(res.data)) return res
  return { ok: false, error: res.error ?? 'Invalid response', data: [] }
}

/** GET /api/wifi/scan 返回项；设备扫描周边 WiFi，按 rssi 降序。 */
export interface WifiApEntry {
  ssid: string
  rssi: number
}

export async function getWifiScan(baseUrl: string): Promise<ApiResult<WifiApEntry[]>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const res = await request<WifiApEntry[]>(baseUrl, '/api/wifi/scan')
  if (res.ok && Array.isArray(res.data)) return res
  return { ok: false, error: res.error ?? 'Scan failed', data: [] }
}

/** GET /api/system_info 返回；需已激活（配对码已设置）。 */
export interface SystemInfoData {
  product_name: string
  system_status: string
  current_time?: string
  firmware_version: string
  /** 运行期板型键（ESP：片型+Flash 档；Linux：`linux`），与 OTA manifest `boards` 键对齐。 */
  board_id?: string
  /** 设备摘要：ESP 为芯片/Flash/核数等一句；Linux 为设备树/DMI 等（若有）。 */
  hardware_model?: string
  /** STA 下路由器分配的 IPv4；未连接时设备返回 "—"。 */
  lan_ip?: string
  ota_available?: boolean
  locale?: string
}

export async function getSystemInfo(
  baseUrl: string,
  pairingCode?: string,
): Promise<ApiResult<SystemInfoData>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  return request<SystemInfoData>(baseUrl, '/api/system_info', { pairingCode })
}

/** GET /api/channel_connectivity 单通道项 */
export interface ChannelConnectivityItem {
  id: string
  configured: boolean
  ok: boolean
  message: string | null
}

/** GET /api/channel_connectivity 响应 */
export interface ChannelConnectivityResponse {
  channels: ChannelConnectivityItem[]
}

export async function getChannelConnectivity(
  baseUrl: string,
  pairingCode?: string,
): Promise<ApiResult<ChannelConnectivityResponse>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  return request<ChannelConnectivityResponse>(baseUrl, '/api/channel_connectivity', {
    pairingCode,
  })
}

/** POST /api/restart：配对码必填，设备将重启。 */
export async function postRestart(
  baseUrl: string,
  pairingCode: string,
): Promise<ApiResult<{ ok: boolean }>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return request<{ ok: boolean }>(baseUrl, '/api/restart', {
    method: 'POST',
    pairingCode: pairingCode.trim(),
  })
}
