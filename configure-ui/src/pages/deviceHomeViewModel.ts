import type {
  HealthData,
  MetricsSnapshotData,
  ResourceBudgetData,
  ResourceSnapshotData,
  SystemInfoData,
} from "../api/endpoints/system";
import type { DeviceRuntimeKind } from "../store/deviceStatusStore";

export interface HomeSummaryField {
  id:
    | "product_name"
    | "board_id"
    | "hardware_model"
    | "os_type"
    | "kernel_version"
    | "cpu_model"
    | "cpu_cores"
    | "lan_ip"
    | "storage_media"
    | "storage_media_error"
    | "firmware_version"
    | "system_status"
    | "audio_duplex_profile"
    | "locale"
    | "current_time";
  labelKey: string;
  value: string | boolean;
  valueKind: "text" | "boolean" | "audio_profile" | "status_key";
}

export interface HomeMetricField {
  id: string;
  labelKey: string;
  value: number;
}

export interface HomeDetailField {
  id: string;
  labelKey: string;
  value: string | number | boolean | [number, number, number];
  valueKind:
    | "text"
    | "number"
    | "boolean"
    | "bytes"
    | "milliseconds"
    | "microseconds"
    | "load_average"
    | "translation_key";
  danger?: boolean;
}

export interface RuntimeStrategyBudgetField extends HomeMetricField {
  valueKind: "bytes" | "seconds";
}

export interface StorageMediaDetailView {
  id: string;
  title: string;
  fields: HomeDetailField[];
}

export interface RuntimeTelemetryField {
  id:
    | "active_http_count"
    | "active_wss_count"
    | "active_agent_tasks"
    | "session_count"
    | "messages_in"
    | "agent_messages_in"
    | "system_messages_in"
    | "messages_out"
    | "llm_calls"
    | "llm_last_ms"
    | "tool_calls"
    | "dispatch_send_ok"
    | "inbound_depth"
    | "outbound_depth"
    | "last_active_epoch_secs"
    | "cpu_usage_percent"
    | "process_memory_kb"
    | "load_average";
  labelKey: string;
  value: number | [number, number, number];
  valueKind: "number" | "epoch_seconds" | "float1" | "load_average";
  unit?: string;
  color?: string;
}

export interface RuntimeStrategyViewModel {
  headlineKey: string;
  summaryKey: string;
  behaviorKeys: [string, string, string];
  intensity: 1 | 2 | 3;
  level: string | null;
  budgetFields: RuntimeStrategyBudgetField[];
}

function summarizeStorageMedia(systemInfo: SystemInfoData | null): string | null {
  if (systemInfo?.storage_media_error) return systemInfo.storage_media_error;
  if (!systemInfo?.storage_media?.length) return null;
  return systemInfo.storage_media
    .map((item) => item.label || item.id)
    .filter(Boolean)
    .join(", ");
}

const FAULT_METRIC_DEFS = [
  { id: "errors_agent_chat", labelKey: "device.systemStatusChatErrors" },
  { id: "errors_agent_context", labelKey: "device.systemStatusErrContext" },
  { id: "errors_tool_execute", labelKey: "device.systemStatusErrToolExec" },
  { id: "errors_llm_request", labelKey: "device.systemStatusErrLlmReq" },
  { id: "errors_llm_parse", labelKey: "device.systemStatusErrLlmParse" },
  { id: "errors_channel_dispatch", labelKey: "device.systemStatusErrChDispatch" },
  { id: "llm_errors", labelKey: "device.systemStatusLlmErrors" },
  { id: "tool_errors", labelKey: "device.systemStatusToolErrors" },
  { id: "dispatch_send_fail", labelKey: "device.systemStatusDispatchFail" },
  { id: "errors_session_append", labelKey: "device.systemStatusErrSession" },
  { id: "errors_other", labelKey: "device.systemStatusErrOther" },
] as const;

const RECOVERY_METRIC_DEFS = [
  { id: "wifi_reconnect_total", labelKey: "device.systemStatusWifiReconnect" },
  { id: "wifi_ap_restart_total", labelKey: "device.systemStatusWifiApRestart" },
] as const;

const RESOURCE_GOVERNANCE_FIELD_DEFS = [
  { id: "runtime_spawn_failure_total", labelKey: "device.systemStatusRuntimeSpawnFailure" },
  { id: "http_route_reject_total", labelKey: "device.systemStatusHttpRouteReject" },
  { id: "inbound_queue_full_total", labelKey: "device.systemStatusInboundQueueFull" },
  { id: "inbound_defer_total", labelKey: "device.systemStatusInboundDeferred" },
  { id: "inbound_drop_total", labelKey: "device.systemStatusInboundDropped" },
  { id: "event_ingress_enqueued_total", labelKey: "device.systemStatusEventIngressEnqueued" },
  { id: "event_ingress_rejected_total", labelKey: "device.systemStatusEventIngressRejected" },
  { id: "event_ingress_purged_total", labelKey: "device.systemStatusEventIngressPurged" },
  { id: "event_ingress_cancelled_total", labelKey: "device.systemStatusEventIngressCancelled" },
  { id: "event_ingress_stale_drop_total", labelKey: "device.systemStatusEventIngressStaleDropped" },
] as const;

const EXECUTION_TIMING_FIELD_DEFS = [
  { id: "llm_request_body_last_bytes", labelKey: "device.systemStatusLlmRequestBodyLast", valueKind: "bytes" },
  { id: "llm_request_body_max_bytes", labelKey: "device.systemStatusLlmRequestBodyMax", valueKind: "bytes" },
  { id: "request_semantics_last_ms", labelKey: "device.systemStatusRequestSemanticsMs", valueKind: "milliseconds" },
  { id: "tool_exec_last_ms", labelKey: "device.systemStatusToolExecMs", valueKind: "milliseconds" },
  { id: "mental_privacy_review_last_ms", labelKey: "device.systemStatusMentalPrivacyReviewMs", valueKind: "milliseconds" },
  { id: "ttft_last_ms", labelKey: "device.systemStatusTtftMs", valueKind: "milliseconds" },
  { id: "e2e_last_ms", labelKey: "device.systemStatusE2eMs", valueKind: "milliseconds" },
  { id: "post_reply_last_ms", labelKey: "device.systemStatusPostReplyMs", valueKind: "milliseconds" },
  { id: "user_queue_wait_last_ms", labelKey: "device.systemStatusUserQueueWaitMs", valueKind: "milliseconds" },
  { id: "system_queue_wait_last_ms", labelKey: "device.systemStatusSystemQueueWaitMs", valueKind: "milliseconds" },
  { id: "cron_e2e_last_ms", labelKey: "device.systemStatusCronE2eMs", valueKind: "milliseconds" },
] as const;

const TURN_PROTOCOL_FIELD_DEFS = [
  { id: "react_rounds_last", labelKey: "device.systemStatusReactRoundsLast" },
  { id: "tool_calls_last", labelKey: "device.systemStatusToolCallsLast" },
  { id: "user_messages_done", labelKey: "device.systemStatusUserMessagesDone" },
  { id: "system_messages_done", labelKey: "device.systemStatusSystemMessagesDone" },
  { id: "cron_messages_done", labelKey: "device.systemStatusCronMessagesDone" },
  { id: "tool_protocol_forced_rounds", labelKey: "device.systemStatusToolProtocolForced" },
  { id: "tool_protocol_violation", labelKey: "device.systemStatusToolProtocolViolation", danger: true },
  { id: "final_answer_calls", labelKey: "device.systemStatusFinalAnswerCalls" },
  { id: "outbound_enqueue_fail", labelKey: "device.systemStatusOutboundEnqueueFail", danger: true },
  { id: "tool_succeeded_final_drift_total", labelKey: "device.systemStatusToolSucceededFinalDrift", danger: true },
  { id: "empty_final_blocked_total", labelKey: "device.systemStatusEmptyFinalBlocked", danger: true },
  { id: "internal_error_copy_suppressed_total", labelKey: "device.systemStatusInternalErrorCopySuppressed" },
  { id: "channel_http_ok", labelKey: "device.systemStatusChannelHttpOk" },
  { id: "channel_http_fail", labelKey: "device.systemStatusChannelHttpFail", danger: true },
  { id: "errors_tls_admission", labelKey: "device.systemStatusErrTlsAdmission", danger: true },
] as const;

const HTTP_STORAGE_STREAM_FIELD_DEFS = [
  { id: "http_permit_wait_last_ms", labelKey: "device.systemStatusHttpPermitWaitMs", valueKind: "milliseconds" },
  { id: "http_route_queue_wait_last_ms", labelKey: "device.systemStatusHttpRouteQueueWaitMs", valueKind: "milliseconds" },
  { id: "http_route_handler_last_ms", labelKey: "device.systemStatusHttpRouteHandlerMs", valueKind: "milliseconds" },
  { id: "http_route_timeout_total", labelKey: "device.systemStatusHttpRouteTimeout", danger: true },
  { id: "storage_lock_ops_total", labelKey: "device.systemStatusStorageLockOps" },
  { id: "storage_lock_contention_total", labelKey: "device.systemStatusStorageLockContention", danger: true },
  { id: "storage_lock_wait_last_us", labelKey: "device.systemStatusStorageLockWaitLastUs", valueKind: "microseconds" },
  { id: "storage_lock_hold_last_us", labelKey: "device.systemStatusStorageLockHoldLastUs", valueKind: "microseconds" },
  { id: "storage_lock_hold_last_stage", labelKey: "device.systemStatusStorageLockHoldLastStage", valueKind: "text" },
  { id: "stream_http_reuse_hits", labelKey: "device.systemStatusStreamHttpReuse" },
  { id: "stream_http_creates", labelKey: "device.systemStatusStreamHttpCreates" },
  { id: "stream_http_resets", labelKey: "device.systemStatusStreamHttpResets" },
  { id: "stream_http_invalidates", labelKey: "device.systemStatusStreamHttpInvalidates", danger: true },
] as const;

const VOICE_AUDIO_FIELD_DEFS = [
  { id: "voice_input_capture_last_ms", labelKey: "device.systemStatusVoiceInputCaptureMs", valueKind: "milliseconds" },
  { id: "voice_input_stt_http_last_ms", labelKey: "device.systemStatusVoiceInputSttHttpMs", valueKind: "milliseconds" },
  { id: "voice_output_tts_http_last_ms", labelKey: "device.systemStatusVoiceOutputTtsHttpMs", valueKind: "milliseconds" },
  { id: "voice_output_play_last_ms", labelKey: "device.systemStatusVoiceOutputPlayMs", valueKind: "milliseconds" },
  { id: "voice_input_fail_total", labelKey: "device.systemStatusVoiceInputFail", danger: true },
  { id: "voice_output_fail_total", labelKey: "device.systemStatusVoiceOutputFail", danger: true },
  { id: "voice_interrupt_request_total", labelKey: "device.systemStatusVoiceInterruptRequest" },
  { id: "voice_interrupt_accept_total", labelKey: "device.systemStatusVoiceInterruptAccept" },
  { id: "voice_cancel_sent_total", labelKey: "device.systemStatusVoiceCancelSent" },
  { id: "voice_interrupt_reference_suppress_total", labelKey: "device.systemStatusVoiceInterruptReferenceSuppress" },
  { id: "voice_no_speech_timeout_total", labelKey: "device.systemStatusVoiceNoSpeechTimeout", danger: true },
  { id: "voice_response_wait_timeout_total", labelKey: "device.systemStatusVoiceResponseWaitTimeout", danger: true },
  { id: "voice_post_playback_timeout_total", labelKey: "device.systemStatusVoicePostPlaybackTimeout", danger: true },
  { id: "wake_trigger_total", labelKey: "device.systemStatusWakeTrigger" },
  { id: "audio_worker_turns_total", labelKey: "device.systemStatusAudioWorkerTurns" },
  { id: "audio_worker_idle_turns_total", labelKey: "device.systemStatusAudioWorkerIdleTurns" },
  { id: "audio_mic_poll_turns_total", labelKey: "device.systemStatusAudioMicPollTurns" },
  { id: "audio_mic_frames_total", labelKey: "device.systemStatusAudioMicFrames" },
  { id: "audio_mic_zero_read_total", labelKey: "device.systemStatusAudioMicZeroRead", danger: true },
  { id: "audio_mic_read_last_us", labelKey: "device.systemStatusAudioMicReadLastUs", valueKind: "microseconds" },
  { id: "audio_speaker_write_last_us", labelKey: "device.systemStatusAudioSpeakerWriteLastUs", valueKind: "microseconds" },
  { id: "wake_feed_calls_total", labelKey: "device.systemStatusWakeFeedCalls" },
  { id: "wake_feed_skip_busy_total", labelKey: "device.systemStatusWakeFeedSkipBusy" },
  { id: "wake_feed_skip_cooldown_total", labelKey: "device.systemStatusWakeFeedSkipCooldown" },
  { id: "wake_feed_detect_total", labelKey: "device.systemStatusWakeFeedDetect" },
  { id: "wake_feed_last_us", labelKey: "device.systemStatusWakeFeedLastUs", valueKind: "microseconds" },
] as const;

function metricFieldFromDef(
  metrics: MetricsSnapshotData | null,
  def: {
    id: string;
    labelKey: string;
    valueKind?: HomeDetailField["valueKind"];
    danger?: boolean;
  },
): HomeDetailField | null {
  const value = metrics?.[def.id as keyof MetricsSnapshotData];
  if (value == null) return null;
  if (typeof value !== "number" && typeof value !== "string") return null;
  return {
    id: def.id,
    labelKey: def.labelKey,
    value,
    valueKind: def.valueKind ?? (typeof value === "string" ? "text" : "number"),
    danger: def.danger,
  };
}

function numberField(
  id: string,
  labelKey: string,
  value: number | undefined,
  options?: Partial<Pick<HomeDetailField, "valueKind" | "danger">>,
): HomeDetailField | null {
  if (value == null) return null;
  return {
    id,
    labelKey,
    value,
    valueKind: options?.valueKind ?? "number",
    danger: options?.danger,
  };
}

function textField(
  id: string,
  labelKey: string,
  value: string | undefined | null,
): HomeDetailField | null {
  if (!value) return null;
  return { id, labelKey, value, valueKind: "text" };
}

function booleanField(
  id: string,
  labelKey: string,
  value: boolean | undefined | null,
): HomeDetailField | null {
  if (value == null) return null;
  return { id, labelKey, value, valueKind: "boolean" };
}

function translationField(
  id: string,
  labelKey: string,
  value: string | undefined | null,
): HomeDetailField | null {
  if (!value) return null;
  return { id, labelKey, value, valueKind: "translation_key" };
}

export function buildMemoryMetrics(
  runtimeKind: DeviceRuntimeKind,
  resource: ResourceSnapshotData | null,
): HomeMetricField[] {
  const items: HomeMetricField[] = [];
  if (resource?.heap_free_internal != null) {
    items.push({
      id: "heap_internal",
      labelKey: "device.systemStatusHeapInternal",
      value: resource.heap_free_internal,
    });
  }
  if (runtimeKind === "esp" && resource?.heap_min_free_internal != null) {
    items.push({
      id: "heap_internal_min",
      labelKey: "device.systemStatusHeapInternalMin",
      value: resource.heap_min_free_internal,
    });
  }
  const heapFreeSpiram = resource?.heap_free_spiram;
  if (runtimeKind === "esp" && (heapFreeSpiram ?? 0) > 0) {
    items.push({
      id: "heap_spiram_free",
      labelKey: "device.systemStatusHeapSpiram",
      value: heapFreeSpiram ?? 0,
    });
  }
  if (runtimeKind === "esp" && resource?.heap_used_spiram_est != null) {
    items.push({
      id: "heap_spiram_used_est",
      labelKey: "device.systemStatusHeapSpiramUsedEst",
      value: resource.heap_used_spiram_est,
    });
  }
  if (runtimeKind === "esp" && resource?.heap_total_spiram != null) {
    items.push({
      id: "heap_spiram_total",
      labelKey: "device.systemStatusHeapSpiramTotal",
      value: resource.heap_total_spiram,
    });
  }
  if (runtimeKind === "esp" && resource?.heap_min_free_spiram != null) {
    items.push({
      id: "heap_spiram_min",
      labelKey: "device.systemStatusHeapSpiramMin",
      value: resource.heap_min_free_spiram,
    });
  }
  if (runtimeKind === "esp" && resource?.heap_largest_block_spiram != null) {
    items.push({
      id: "heap_spiram_largest",
      labelKey: "device.systemStatusHeapSpiramLargest",
      value: resource.heap_largest_block_spiram,
    });
  }
  if (
    resource?.heap_largest_block_internal != null &&
    !(runtimeKind === "linux" && resource.heap_largest_block_internal <= 0)
  ) {
    items.push({
      id: "heap_largest",
      labelKey: "device.systemStatusHeapLargest",
      value: resource.heap_largest_block_internal,
    });
  }
  return items;
}

export function buildDeviceSummaryFields(
  systemInfo: SystemInfoData | null,
  health: HealthData | null,
  runtimeStatusKey?: string | null,
): HomeSummaryField[] {
  const items: HomeSummaryField[] = [];
  if (systemInfo?.product_name) {
    items.push({
      id: "product_name",
      labelKey: "device.deviceInfoProduct",
      value: systemInfo.product_name,
      valueKind: "text",
    });
  }
  if (systemInfo?.board_id) {
    items.push({
      id: "board_id",
      labelKey: "device.deviceInfoBoardId",
      value: systemInfo.board_id,
      valueKind: "text",
    });
  }
  if (systemInfo?.hardware_model) {
    items.push({
      id: "hardware_model",
      labelKey: "device.deviceInfoHardwareModel",
      value: systemInfo.hardware_model,
      valueKind: "text",
    });
  }
  if (systemInfo?.os_type) {
    items.push({
      id: "os_type",
      labelKey: "device.deviceInfoOsType",
      value: systemInfo.os_type,
      valueKind: "text",
    });
  }
  if (systemInfo?.kernel_version) {
    items.push({
      id: "kernel_version",
      labelKey: "device.deviceInfoKernelVersion",
      value: systemInfo.kernel_version,
      valueKind: "text",
    });
  }
  if (systemInfo?.cpu_model) {
    items.push({
      id: "cpu_model",
      labelKey: "device.deviceInfoCpuModel",
      value: systemInfo.cpu_model,
      valueKind: "text",
    });
  }
  if (systemInfo?.cpu_cores != null) {
    items.push({
      id: "cpu_cores",
      labelKey: "device.deviceInfoCpuCores",
      value: String(systemInfo.cpu_cores),
      valueKind: "text",
    });
  }
  if (systemInfo?.lan_ip) {
    items.push({
      id: "lan_ip",
      labelKey: "device.deviceInfoLanIp",
      value: systemInfo.lan_ip,
      valueKind: "text",
    });
  }
  const storageMediaSummary = summarizeStorageMedia(systemInfo);
  if (storageMediaSummary) {
    items.push({
      id: systemInfo?.storage_media_error ? "storage_media_error" : "storage_media",
      labelKey: systemInfo?.storage_media_error
        ? "device.deviceInfoStorageMediaError"
        : "device.deviceInfoStorageMedia",
      value: storageMediaSummary,
      valueKind: "text",
    });
  }
  if (systemInfo?.firmware_version) {
    items.push({
      id: "firmware_version",
      labelKey: "device.deviceInfoFirmware",
      value: systemInfo.firmware_version,
      valueKind: "text",
    });
  }
  if (runtimeStatusKey) {
    items.push({
      id: "system_status",
      labelKey: "device.deviceInfoStatus",
      value: runtimeStatusKey,
      valueKind: "status_key",
    });
  }
  if (health?.audio?.duplex_profile) {
    items.push({
      id: "audio_duplex_profile",
      labelKey: "device.deviceInfoAudio",
      value: health.audio.duplex_profile,
      valueKind: "audio_profile",
    });
  }
  if (systemInfo?.locale) {
    items.push({
      id: "locale",
      labelKey: "device.deviceInfoLocale",
      value: systemInfo.locale,
      valueKind: "text",
    });
  }
  if (systemInfo?.current_time) {
    items.push({
      id: "current_time",
      labelKey: "device.deviceInfoCurrentTime",
      value: systemInfo.current_time,
      valueKind: "text",
    });
  }
  return items;
}

export function buildFaultAndRecoveryMetrics(
  metrics: MetricsSnapshotData | null,
): { faults: HomeMetricField[]; recovery: HomeMetricField[] } {
  const faults = FAULT_METRIC_DEFS.filter(
    ({ id }) => metrics?.[id as keyof MetricsSnapshotData] != null,
  ).map(({ id, labelKey }) => ({
    id,
    labelKey,
    value: Number(metrics?.[id as keyof MetricsSnapshotData] ?? 0),
  }));
  const recovery = RECOVERY_METRIC_DEFS.filter(
    ({ id }) => metrics?.[id as keyof MetricsSnapshotData] != null,
  ).map(({ id, labelKey }) => ({
    id,
    labelKey,
    value: Number(metrics?.[id as keyof MetricsSnapshotData] ?? 0),
  }));
  return { faults, recovery };
}

export function buildDeviceOperationalStatusKey(
  health: HealthData | null,
  resource: ResourceSnapshotData | null,
): string | null {
  if (!health && !resource) return null;
  if (health?.network_status?.sta_connected === false) {
    return "device.runtimeSummaryWifiDisconnected";
  }
  if (health?.last_error && health.last_error !== "none") {
    return "device.runtimeSummaryError";
  }
  switch (resource?.pressure) {
    case "Critical":
      return "device.runtimeSummaryCritical";
    case "Cautious":
      return "device.runtimeSummaryCautious";
    case "Normal":
      return "device.runtimeSummaryHealthy";
    default:
      return "device.runtimeSummaryHealthy";
  }
}

function healthStatusValueKey(status: HealthData["status"] | undefined): string | null {
  switch (status) {
    case "ok":
      return "device.healthStatusOk";
    case "degraded":
      return "device.healthStatusDegraded";
    default:
      return null;
  }
}

function networkStageValueKey(stage: string | undefined): string | null {
  switch (stage) {
    case "ap_only":
      return "device.networkStageApOnly";
    case "sta_connecting":
      return "device.networkStageStaConnecting";
    case "sta_auth_failed":
      return "device.networkStageStaAuthFailed";
    case "sta_ap_not_found":
      return "device.networkStageStaApNotFound";
    case "sta_l2_connected":
      return "device.networkStageStaL2Connected";
    case "sta_waiting_dhcp":
      return "device.networkStageStaWaitingDhcp";
    case "sta_ip_ready":
      return "device.networkStageStaIpReady";
    case "sta_recovering":
      return "device.networkStageStaRecovering";
    case "sta_fallback_ap":
      return "device.networkStageStaFallbackAp";
    default:
      return stage ?? null;
  }
}

function riskValueKey(value: string | undefined): string | null {
  switch (value) {
    case "Healthy":
    case "healthy":
      return "device.systemStatusRiskHealthy";
    case "Normal":
    case "normal":
      return "device.systemStatusPressureNormal";
    case "Cautious":
    case "cautious":
      return "device.systemStatusPressureCautious";
    case "Critical":
    case "critical":
      return "device.systemStatusPressureCritical";
    default:
      return value ?? null;
  }
}

export function buildHealthDetailFields(
  health: HealthData | null,
): HomeDetailField[] {
  return [
    translationField(
      "health_status",
      "device.systemStatusHealthStatus",
      healthStatusValueKey(health?.status),
    ),
    translationField(
      "network_stage",
      "device.systemStatusNetworkStage",
      networkStageValueKey(health?.network_status?.stage),
    ),
    booleanField(
      "wall_clock_trusted",
      "device.systemStatusWallClockTrusted",
      health?.network_status?.wall_clock_trusted,
    ),
  ].filter((item): item is HomeDetailField => item !== null);
}

export function buildResourceRiskFields(
  resource: ResourceSnapshotData | null,
): HomeDetailField[] {
  return [
    translationField(
      "tls_fragmentation_risk",
      "device.systemStatusTlsFragmentationRisk",
      riskValueKey(resource?.tls_fragmentation_risk),
    ),
    translationField(
      "storage_contention_risk",
      "device.systemStatusStorageContentionRisk",
      riskValueKey(resource?.storage_contention_risk),
    ),
  ].filter((item): item is HomeDetailField => item !== null);
}

export function buildResourceGovernanceFields(
  resource: ResourceSnapshotData | null,
): HomeDetailField[] {
  return RESOURCE_GOVERNANCE_FIELD_DEFS.map((def) => {
    const value =
      resource?.governance_metrics?.[
        def.id as keyof NonNullable<ResourceSnapshotData["governance_metrics"]>
      ];
    return numberField(def.id, def.labelKey, value, {
      danger: def.id.includes("reject") || def.id.includes("failure") || def.id.includes("drop"),
    });
  }).filter((item): item is HomeDetailField => item !== null);
}

export function buildExecutionTimingFields(
  metrics: MetricsSnapshotData | null,
): HomeDetailField[] {
  return EXECUTION_TIMING_FIELD_DEFS.map((def) =>
    metricFieldFromDef(metrics, def),
  ).filter((item): item is HomeDetailField => item !== null);
}

export function buildTurnProtocolFields(
  metrics: MetricsSnapshotData | null,
): HomeDetailField[] {
  return TURN_PROTOCOL_FIELD_DEFS.map((def) =>
    metricFieldFromDef(metrics, def),
  ).filter((item): item is HomeDetailField => item !== null);
}

export function buildHttpStorageStreamFields(
  metrics: MetricsSnapshotData | null,
): HomeDetailField[] {
  return HTTP_STORAGE_STREAM_FIELD_DEFS.map((def) =>
    metricFieldFromDef(metrics, def),
  ).filter((item): item is HomeDetailField => item !== null);
}

export function buildVoiceAudioTelemetryFields(
  metrics: MetricsSnapshotData | null,
): HomeDetailField[] {
  return VOICE_AUDIO_FIELD_DEFS.map((def) =>
    metricFieldFromDef(metrics, def),
  ).filter((item): item is HomeDetailField => item !== null);
}

export function buildProgrammableReasoningFields(
  systemInfo: SystemInfoData | null,
): HomeDetailField[] {
  const value = systemInfo?.programmable_reasoning;
  if (!value) return [];
  return [
    textField("pr_stage", "device.programmableReasoningStage", value.stage),
    booleanField(
      "pr_execution_enabled",
      "device.programmableReasoningExecutionEnabled",
      value.execution_enabled,
    ),
    textField("pr_backend", "device.programmableReasoningBackend", value.backend),
    booleanField("pr_linux_only", "device.programmableReasoningLinuxOnly", value.linux_only),
    booleanField(
      "pr_proposal_only_persistence",
      "device.programmableReasoningProposalOnlyPersistence",
      value.proposal_only_persistence,
    ),
    textField(
      "pr_product_headline",
      "device.programmableReasoningProductHeadline",
      value.product_headline,
    ),
    numberField(
      "pr_demo_scenario_count",
      "device.programmableReasoningDemoScenarioCount",
      value.demo_scenario_count,
    ),
    booleanField(
      "pr_inspection_ready",
      "device.programmableReasoningInspectionReady",
      value.inspection_ready,
    ),
    booleanField("pr_replay_ready", "device.programmableReasoningReplayReady", value.replay_ready),
  ].filter((item): item is HomeDetailField => item !== null);
}

export function buildStorageMediaDetailFields(
  systemInfo: SystemInfoData | null,
): StorageMediaDetailView[] {
  return (systemInfo?.storage_media ?? []).map((media, index) => ({
    id: media.id || `storage-${index}`,
    title: media.label || media.id || `Storage ${index + 1}`,
    fields: [
      textField("id", "device.storageMediaId", media.id),
      textField("kind", "device.storageMediaKind", media.kind),
      booleanField("present", "device.storageMediaPresent", media.present),
      booleanField("mounted", "device.storageMediaMounted", media.mounted),
      textField("mount_path", "device.storageMediaMountPath", media.mount_path),
      textField("filesystem", "device.storageMediaFilesystem", media.filesystem),
      textField("source", "device.storageMediaSource", media.source),
      booleanField("removable", "device.storageMediaRemovable", media.removable),
      booleanField("is_system_root", "device.storageMediaSystemRoot", media.is_system_root),
      booleanField("is_state_root", "device.storageMediaStateRoot", media.is_state_root),
      numberField("capacity_bytes", "device.storageMediaCapacity", media.capacity_bytes ?? undefined, {
        valueKind: "bytes",
      }),
      numberField("free_bytes", "device.storageMediaFree", media.free_bytes ?? undefined, {
        valueKind: "bytes",
      }),
    ].filter((item): item is HomeDetailField => item !== null),
  }));
}

export function buildRuntimeTelemetryFields(
  runtimeKind: DeviceRuntimeKind,
  resource: ResourceSnapshotData | null,
  metrics: MetricsSnapshotData | null,
): RuntimeTelemetryField[] {
  const fields: RuntimeTelemetryField[] = [];
  const pushNumber = (
    id: RuntimeTelemetryField["id"],
    labelKey: string,
    value: number | undefined,
    options?: Partial<Pick<RuntimeTelemetryField, "unit" | "color" | "valueKind">>,
  ) => {
    if (value == null) return;
    fields.push({
      id,
      labelKey,
      value,
      valueKind: options?.valueKind ?? "number",
      unit: options?.unit,
      color: options?.color,
    });
  };

  pushNumber("active_http_count", "device.systemStatusActiveHttp", resource?.active_http_count, {
    color: "var(--primary)",
  });
  pushNumber("active_wss_count", "device.systemStatusActiveWss", resource?.active_wss_count, {
    color: "var(--primary)",
  });
  pushNumber(
    "active_agent_tasks",
    "device.systemStatusActiveAgentTasks",
    resource?.active_agent_tasks,
    { color: "var(--primary)" },
  );
  pushNumber("session_count", "device.systemStatusSessionCount", resource?.session_count, {
    color: "var(--primary)",
  });
  pushNumber(
    "messages_in",
    "device.systemStatusMessagesIn",
    metrics?.user_messages_in ?? metrics?.messages_in,
  );
  pushNumber(
    "agent_messages_in",
    "device.systemStatusAgentMessagesIn",
    metrics?.agent_messages_in,
  );
  pushNumber(
    "system_messages_in",
    "device.systemStatusSystemMessagesIn",
    metrics?.system_messages_in,
  );
  pushNumber("messages_out", "device.systemStatusMessagesOut", metrics?.messages_out);
  pushNumber("llm_calls", "device.systemStatusLlmCalls", metrics?.llm_calls);
  pushNumber("llm_last_ms", "device.systemStatusLlmLastMs", metrics?.llm_last_ms);
  pushNumber("tool_calls", "device.systemStatusToolCalls", metrics?.tool_calls);
  pushNumber("dispatch_send_ok", "device.systemStatusDispatchOk", metrics?.dispatch_send_ok);
  pushNumber("inbound_depth", "device.systemStatusInboundDepth", resource?.inbound_depth);
  pushNumber("outbound_depth", "device.systemStatusOutboundDepth", resource?.outbound_depth);
  pushNumber(
    "last_active_epoch_secs",
    "device.systemStatusLastActiveAt",
    metrics?.last_active_epoch_secs,
    {
      valueKind: "epoch_seconds",
      color: "var(--semantic-warning)",
    },
  );

  if (runtimeKind === "linux") {
    pushNumber(
      "cpu_usage_percent",
      "device.systemStatusCpuUsage",
      resource?.cpu_usage_percent,
      {
        valueKind: "float1",
        unit: "%",
        color: "var(--semantic-warning)",
      },
    );
    pushNumber(
      "process_memory_kb",
      "device.systemStatusProcessMemory",
      resource?.process_memory_kb,
      {
        unit: "KB",
        color: "var(--semantic-warning)",
      },
    );
    if (resource?.load_average) {
      fields.push({
        id: "load_average",
        labelKey: "device.systemStatusLoadAverage",
        value: resource.load_average,
        valueKind: "load_average",
        color: "var(--semantic-warning)",
      });
    }
  }

  return fields;
}

function buildRuntimeBudgetFields(
  budget: ResourceBudgetData | undefined,
): RuntimeStrategyBudgetField[] {
  if (!budget) return [];
  const items: RuntimeStrategyBudgetField[] = [];
  if (budget.system_prompt_max != null) {
    items.push({
      id: "system_prompt_max",
      labelKey: "device.systemStatusStrategyBudgetSystemPrompt",
      value: budget.system_prompt_max,
      valueKind: "bytes",
    });
  }
  if (budget.messages_max != null) {
    items.push({
      id: "messages_max",
      labelKey: "device.systemStatusStrategyBudgetMessages",
      value: budget.messages_max,
      valueKind: "bytes",
    });
  }
  if (budget.response_body_max != null) {
    items.push({
      id: "response_body_max",
      labelKey: "device.systemStatusStrategyBudgetResponseBody",
      value: budget.response_body_max,
      valueKind: "bytes",
    });
  }
  if (budget.reconnect_backoff_secs != null) {
    items.push({
      id: "reconnect_backoff_secs",
      labelKey: "device.systemStatusStrategyBudgetReconnect",
      value: budget.reconnect_backoff_secs,
      valueKind: "seconds",
    });
  }
  return items;
}

export function buildRuntimeStrategyView(
  resource: ResourceSnapshotData | null,
): RuntimeStrategyViewModel | null {
  if (!resource?.pressure || !resource.budget) return null;
  switch (resource.pressure) {
    case "Critical":
      return {
        headlineKey: "device.systemStatusStrategyCritical",
        summaryKey: "device.systemStatusStrategyHintCritical",
        behaviorKeys: [
          "device.systemStatusStrategyBehaviorCriticalReplies",
          "device.systemStatusStrategyBehaviorCriticalTools",
          "device.systemStatusStrategyBehaviorCriticalReconnect",
        ],
        intensity: 3,
        level: resource.budget.level ?? resource.pressure,
        budgetFields: buildRuntimeBudgetFields(resource.budget),
      };
    case "Cautious":
      return {
        headlineKey: "device.systemStatusStrategyCautious",
        summaryKey: "device.systemStatusStrategyHintCautious",
        behaviorKeys: [
          "device.systemStatusStrategyBehaviorCautiousReplies",
          "device.systemStatusStrategyBehaviorCautiousTools",
          "device.systemStatusStrategyBehaviorCautiousReconnect",
        ],
        intensity: 2,
        level: resource.budget.level ?? resource.pressure,
        budgetFields: buildRuntimeBudgetFields(resource.budget),
      };
    case "Normal":
    default:
      return {
        headlineKey: "device.systemStatusStrategyNormal",
        summaryKey: "device.systemStatusStrategyHintNormal",
        behaviorKeys: [
          "device.systemStatusStrategyBehaviorNormalReplies",
          "device.systemStatusStrategyBehaviorNormalTools",
          "device.systemStatusStrategyBehaviorNormalReconnect",
        ],
        intensity: 1,
        level: resource.budget.level ?? resource.pressure,
        budgetFields: buildRuntimeBudgetFields(resource.budget),
      };
  }
}

export function pressureLabelKey(pressure: string | undefined): string | null {
  switch (pressure) {
    case "Normal":
      return "device.systemStatusPressureNormal";
    case "Cautious":
      return "device.systemStatusPressureCautious";
    case "Critical":
      return "device.systemStatusPressureCritical";
    default:
      return null;
  }
}

export function audioProfileLabelKey(profile: string | undefined): string | null {
  switch (profile) {
    case "unavailable":
      return "device.audioProfileUnavailable";
    case "speaker_only":
      return "device.audioProfileSpeakerOnly";
    case "microphone_only":
      return "device.audioProfileMicrophoneOnly";
    case "duplex_no_reference":
      return "device.audioProfileDuplexNoReference";
    case "duplex_playback_reference":
      return "device.audioProfileDuplexPlaybackReference";
    case "duplex_input_reference":
      return "device.audioProfileDuplexInputReference";
    case "duplex_platform_aec":
      return "device.audioProfileDuplexPlatformAec";
    default:
      return null;
  }
}
