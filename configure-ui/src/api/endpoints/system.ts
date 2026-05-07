import { request, requestProtected, API_ERROR } from '../client.ts'
import type { ApiResult } from '../client.ts'

/** 与固件 `metrics::MetricsSnapshot` serde 字段一致。 */
export interface MetricsSnapshotData {
  messages_in?: number
  user_messages_in?: number
  agent_messages_in?: number
  system_messages_in?: number
  messages_out?: number
  llm_calls?: number
  llm_errors?: number
  llm_last_ms?: number
  llm_request_body_last_bytes?: number
  llm_request_body_max_bytes?: number
  request_semantics_last_ms?: number
  tool_exec_last_ms?: number
  mental_privacy_review_last_ms?: number
  ttft_last_ms?: number
  e2e_last_ms?: number
  post_reply_last_ms?: number
  user_queue_wait_last_ms?: number
  system_queue_wait_last_ms?: number
  cron_e2e_last_ms?: number
  react_rounds_last?: number
  tool_calls_last?: number
  user_messages_done?: number
  system_messages_done?: number
  cron_messages_done?: number
  tool_calls?: number
  tool_errors?: number
  tool_protocol_forced_rounds?: number
  tool_protocol_violation?: number
  final_answer_calls?: number
  dispatch_send_ok?: number
  dispatch_send_fail?: number
  outbound_enqueue_fail?: number
  inbound_queue_full_total?: number
  inbound_defer_total?: number
  inbound_drop_total?: number
  event_ingress_enqueued_total?: number
  event_ingress_rejected_total?: number
  event_ingress_purged_total?: number
  event_ingress_cancelled_total?: number
  event_ingress_stale_drop_total?: number
  runtime_spawn_failure_total?: number
  http_route_reject_total?: number
  tool_succeeded_final_drift_total?: number
  empty_final_blocked_total?: number
  internal_error_copy_suppressed_total?: number
  channel_http_ok?: number
  channel_http_fail?: number
  http_permit_wait_last_ms?: number
  http_route_queue_wait_last_ms?: number
  http_route_handler_last_ms?: number
  http_route_timeout_total?: number
  voice_input_capture_last_ms?: number
  voice_input_stt_http_last_ms?: number
  voice_output_tts_http_last_ms?: number
  voice_output_play_last_ms?: number
  voice_input_fail_total?: number
  voice_output_fail_total?: number
  voice_interrupt_request_total?: number
  voice_interrupt_accept_total?: number
  voice_cancel_sent_total?: number
  voice_interrupt_reference_suppress_total?: number
  voice_no_speech_timeout_total?: number
  voice_response_wait_timeout_total?: number
  voice_post_playback_timeout_total?: number
  wake_trigger_total?: number
  audio_worker_turns_total?: number
  audio_worker_idle_turns_total?: number
  audio_mic_poll_turns_total?: number
  audio_mic_frames_total?: number
  audio_mic_zero_read_total?: number
  audio_mic_read_last_us?: number
  audio_speaker_write_last_us?: number
  wake_feed_calls_total?: number
  wake_feed_skip_busy_total?: number
  wake_feed_skip_cooldown_total?: number
  wake_feed_detect_total?: number
  wake_feed_last_us?: number
  storage_lock_ops_total?: number
  storage_lock_contention_total?: number
  storage_lock_wait_last_us?: number
  storage_lock_hold_last_us?: number
  storage_lock_hold_last_stage?: string
  errors_agent_chat?: number
  errors_agent_context?: number
  errors_tool_execute?: number
  errors_llm_request?: number
  errors_llm_parse?: number
  errors_channel_dispatch?: number
  errors_session_append?: number
  errors_tls_admission?: number
  errors_other?: number
  last_active_epoch_secs?: number
  wifi_reconnect_total?: number
  wifi_ap_restart_total?: number
  wifi_last_failure_stage?: string
  stream_http_reuse_hits?: number
  stream_http_creates?: number
  stream_http_resets?: number
  stream_http_invalidates?: number
}

/** 与固件 `orchestrator::ResourceBudget` 一致（嵌套在 resource 内）。 */
export interface ResourceBudgetData {
  level?: string
  system_prompt_max?: number
  messages_max?: number
  response_body_max?: number
  reconnect_backoff_secs?: number
}

export interface ResourceGovernanceMetricsData {
  runtime_spawn_failure_total?: number
  http_route_reject_total?: number
  inbound_queue_full_total?: number
  inbound_defer_total?: number
  inbound_drop_total?: number
  event_ingress_enqueued_total?: number
  event_ingress_rejected_total?: number
  event_ingress_purged_total?: number
  event_ingress_cancelled_total?: number
  event_ingress_stale_drop_total?: number
}

/** 与固件 `handlers/resource.rs` 的轻量资源压力快照契约一致。 */
export interface ResourceSnapshotData {
  pressure?: string
  tls_fragmentation_risk?: string
  storage_contention_risk?: string
  heap_free_internal?: number
  heap_min_free_internal?: number
  heap_free_spiram?: number
  heap_total_spiram?: number
  heap_min_free_spiram?: number
  heap_largest_block_spiram?: number
  heap_used_spiram_est?: number
  heap_largest_block_internal?: number
  active_http_count?: number
  active_wss_count?: number
  active_agent_tasks?: number
  inbound_depth?: number
  outbound_depth?: number
  budget?: ResourceBudgetData
  governance_metrics?: ResourceGovernanceMetricsData
  session_count?: number
  storage_used_kb?: number
  storage_total_kb?: number
  cpu_usage_percent?: number
  load_average?: [number, number, number]
  process_memory_kb?: number
}

/** 与固件 `handlers/health.rs` 中 `DisplayHealth` 一致。 */
export interface HealthDisplayData {
  available?: boolean
}

export interface HealthAudioCapabilitiesData {
  microphone_input?: boolean
  speaker_output?: boolean
}

export interface HealthAudioData {
  duplex_profile?: string
  duplex_capabilities?: HealthAudioCapabilitiesData
}

export interface HealthNetworkStatusData {
  stage?: string
  sta_connected?: boolean
  wall_clock_trusted?: boolean
}

export interface HealthCurrentChannelData {
  id?: string
}

/** 与固件 `handlers/health.rs` 的轻量生命体征契约一致。 */
export interface HealthData {
  status?: "ok" | "degraded"
  network_status?: HealthNetworkStatusData
  last_error?: string
  current_channel?: HealthCurrentChannelData
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
  return { ok: false, error: res.error ?? 'common.invalid_response', data: [] }
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
  return { ok: false, error: res.error ?? 'common.invalid_response', data: [] }
}

/** GET /api/system_info 返回；需已激活（配对码已设置）。 */
export interface SystemInfoProgrammableReasoningData {
  stage?: string
  execution_enabled?: boolean
  backend?: string
  linux_only?: boolean
  proposal_only_persistence?: boolean
  product_headline?: string
  demo_scenario_count?: number
  inspection_ready?: boolean
  replay_ready?: boolean
}

export interface SystemInfoData {
  product_name: string
  current_time?: string
  firmware_version: string
  /** 运行期板型键（ESP：片型+Flash 档；Linux：`linux`），与官方发布包版型键对齐。 */
  board_id?: string
  /** 设备摘要：ESP 为芯片/Flash/核数等一句；Linux 为设备树/DMI 等（若有）。 */
  hardware_model?: string
  /** STA 下路由器分配的 IPv4；未连接时设备返回 "—"。 */
  lan_ip?: string
  locale?: string
  os_type?: string
  kernel_version?: string
  cpu_model?: string
  cpu_cores?: number
  storage_media?: Array<{
    id: string
    kind: string
    label: string
    present: boolean
    mounted: boolean
    mount_path?: string | null
    filesystem?: string | null
    source?: string | null
    removable: boolean
    is_system_root: boolean
    is_state_root: boolean
    capacity_bytes?: number | null
    free_bytes?: number | null
  }>
  storage_media_error?: string
  programmable_reasoning?: SystemInfoProgrammableReasoningData
}

export async function getSystemInfo(
  baseUrl: string,
  pairingCode?: string,
): Promise<ApiResult<SystemInfoData>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  return request<SystemInfoData>(baseUrl, '/api/system_info', { pairingCode })
}

/** GET /api/channel_connectivity?channel=... 单通道项 */
export interface ChannelConnectivityItem {
  id: string
  configured: boolean
  ok: boolean
  message_key: string | null
  runtime_status?:
    | 'disabled'
    | 'configured'
    | 'worker_started'
    | 'waiting_network'
    | 'waiting_wall_clock'
    | 'suspended_by_mode'
    | 'connecting'
    | 'connected'
    | 'cooling_down'
    | 'failed'
  runtime_reason?: string | null
}

/** GET /api/channel_connectivity?channel=... 响应 */
export interface ChannelConnectivityResponse {
  channel: ChannelConnectivityItem
  checked_at_unix_secs?: number | null
}

export async function getChannelConnectivity(
  baseUrl: string,
  channel: string,
  pairingCode?: string,
): Promise<ApiResult<ChannelConnectivityResponse>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  const normalizedChannel = channel.trim()
  if (!normalizedChannel) {
    return {
      ok: false,
      status: 400,
      error: 'common.missing_query_param',
      errorKey: 'common.missing_query_param',
    }
  }
  return request<ChannelConnectivityResponse>(
    baseUrl,
    `/api/channel_connectivity?channel=${encodeURIComponent(normalizedChannel)}`,
    {
      pairingCode,
    },
  )
}

/** POST /api/restart：配对码必填，设备将重启。 */
export async function postRestart(
  baseUrl: string,
  pairingCode: string,
): Promise<ApiResult<{ ok: boolean }>> {
  if (!baseUrl?.trim()) return { ok: false, error: API_ERROR.NO_BASE_URL }
  if (!pairingCode?.trim()) return { ok: false, error: API_ERROR.PAIRING_REQUIRED }
  return requestProtected<{ ok: boolean }>(baseUrl, '/api/restart', {
    method: 'POST',
    pairingCode: pairingCode.trim(),
  })
}
