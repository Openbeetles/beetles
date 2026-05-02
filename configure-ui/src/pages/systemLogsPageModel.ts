import type { MetricsSnapshotData } from "../api/endpoints/system";

type MetricFieldId = keyof MetricsSnapshotData;

export interface SystemLogMetricItem {
  id: MetricFieldId;
  labelKey: string;
  value: number | string;
}

const SYSTEM_LOG_METRIC_FIELDS: ReadonlyArray<{
  id: MetricFieldId;
  labelKey: string;
}> = [
  { id: "messages_in", labelKey: "device.systemStatusMessagesIn" },
  { id: "user_messages_in", labelKey: "device.systemStatusMessagesIn" },
  { id: "agent_messages_in", labelKey: "device.systemStatusAgentMessagesIn" },
  { id: "system_messages_in", labelKey: "device.systemStatusSystemMessagesIn" },
  { id: "messages_out", labelKey: "device.systemStatusMessagesOut" },
  { id: "llm_calls", labelKey: "device.systemStatusLlmCalls" },
  { id: "llm_errors", labelKey: "device.systemStatusLlmErrors" },
  { id: "llm_last_ms", labelKey: "device.systemStatusLlmLastMs" },
  { id: "tool_calls", labelKey: "device.systemStatusToolCalls" },
  { id: "tool_errors", labelKey: "device.systemStatusToolErrors" },
  { id: "dispatch_send_ok", labelKey: "device.systemStatusDispatchOk" },
  { id: "dispatch_send_fail", labelKey: "device.systemStatusDispatchFail" },
  { id: "wifi_reconnect_total", labelKey: "device.systemStatusWifiReconnect" },
  { id: "wifi_ap_restart_total", labelKey: "device.systemStatusWifiApRestart" },
  { id: "wifi_last_failure_stage", labelKey: "device.systemStatusWifiLastFail" },
  { id: "last_active_epoch_secs", labelKey: "device.systemStatusLastActiveAt" },
];

export function buildSystemLogMetricItems(
  metrics: MetricsSnapshotData | null | undefined,
): SystemLogMetricItem[] {
  if (!metrics) return [];
  const items: SystemLogMetricItem[] = [];
  for (const field of SYSTEM_LOG_METRIC_FIELDS) {
    const value = metrics[field.id];
    if (value == null || value === "") continue;
    items.push({
      id: field.id,
      labelKey: field.labelKey,
      value,
    });
  }
  return items;
}
