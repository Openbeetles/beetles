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
    | "ota_available"
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

export interface RuntimeStrategyBudgetField extends HomeMetricField {
  valueKind: "bytes" | "seconds";
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
  { id: "errors_agent_router", labelKey: "device.systemStatusErrRouter" },
  { id: "errors_agent_context", labelKey: "device.systemStatusErrContext" },
  { id: "errors_tool_execute", labelKey: "device.systemStatusErrToolExec" },
  { id: "errors_llm_request", labelKey: "device.systemStatusErrLlmReq" },
  { id: "errors_llm_parse", labelKey: "device.systemStatusErrLlmParse" },
  { id: "errors_channel_dispatch", labelKey: "device.systemStatusErrChDispatch" },
  { id: "llm_errors", labelKey: "device.systemStatusLlmErrors" },
  { id: "tool_errors", labelKey: "device.systemStatusToolErrors" },
  { id: "dispatch_send_fail", labelKey: "device.systemStatusDispatchFail" },
  { id: "errors_session_append", labelKey: "device.systemStatusErrSession" },
  { id: "errors_agent_chat", labelKey: "device.systemStatusChatErrors" },
  { id: "errors_other", labelKey: "device.systemStatusErrOther" },
] as const;

const RECOVERY_METRIC_DEFS = [
  { id: "wifi_reconnect_total", labelKey: "device.systemStatusWifiReconnect" },
  { id: "wifi_ap_restart_total", labelKey: "device.systemStatusWifiApRestart" },
] as const;

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
  const heapFreeSpiram = resource?.heap_free_spiram;
  if (runtimeKind === "esp" && (heapFreeSpiram ?? 0) > 0) {
    items.push({
      id: "heap_spiram",
      labelKey: "device.systemStatusHeapSpiram",
      value: heapFreeSpiram ?? 0,
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
  if (typeof systemInfo?.ota_available === "boolean") {
    items.push({
      id: "ota_available",
      labelKey: "device.deviceInfoOtaAvailable",
      value: systemInfo.ota_available,
      valueKind: "boolean",
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
  if (health?.wifi === "disconnected") return "device.runtimeSummaryWifiDisconnected";
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
