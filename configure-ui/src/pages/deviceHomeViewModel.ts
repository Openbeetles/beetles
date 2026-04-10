import type {
  HealthData,
  MetricsSnapshotData,
  ResourceSnapshotData,
  SystemInfoData,
} from "../api/endpoints/system";
import type { DeviceRuntimeKind } from "../store/deviceStatusStore";

export interface HomeSummaryField {
  id:
    | "board_id"
    | "hardware_model"
    | "lan_ip"
    | "firmware_version"
    | "system_status"
    | "audio_duplex_profile"
    | "locale"
    | "ota_available"
    | "current_time";
  labelKey: string;
  value: string | boolean;
  valueKind: "text" | "boolean" | "audio_profile";
}

export interface HomeMetricField {
  id: string;
  labelKey: string;
  value: number;
}

const FAULT_METRIC_DEFS = [
  { id: "errors_agent_router", labelKey: "device.systemStatusErrRouter" },
  { id: "errors_agent_context", labelKey: "device.systemStatusErrContext" },
  { id: "errors_tool_execute", labelKey: "device.systemStatusErrToolExec" },
  { id: "errors_llm_request", labelKey: "device.systemStatusErrLlmReq" },
  { id: "errors_llm_parse", labelKey: "device.systemStatusErrLlmParse" },
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
): HomeSummaryField[] {
  const items: HomeSummaryField[] = [];
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
  if (systemInfo?.lan_ip) {
    items.push({
      id: "lan_ip",
      labelKey: "device.deviceInfoLanIp",
      value: systemInfo.lan_ip,
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
  if (systemInfo?.system_status) {
    items.push({
      id: "system_status",
      labelKey: "device.deviceInfoStatus",
      value: systemInfo.system_status,
      valueKind: "text",
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
