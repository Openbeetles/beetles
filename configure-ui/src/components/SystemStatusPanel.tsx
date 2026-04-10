import React from "react";
import type { TFunction } from "i18next";
import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import StorageRounded from "@mui/icons-material/StorageRounded";
import MemoryRounded from "@mui/icons-material/MemoryRounded";
import SwapVertRounded from "@mui/icons-material/SwapVertRounded";
import WarningRounded from "@mui/icons-material/WarningRounded";
import type {
  HealthData,
  MetricsSnapshotData,
  ResourceSnapshotData,
} from "../api/endpoints/system";
import { DashboardCard } from "../pages/DevicePage";

// Non-component exports removed to fix Fast Refresh lint error.
// They are now defined in DevicePage.tsx or kept internal here.

function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value < 0) return String(value);
  if (value < 1024) return `${value} B`;
  const kb = value / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KB`;
  const mb = kb / 1024;
  return `${mb.toFixed(1)} MB`;
}

function CircularGauge({
  value,
  max,
  label,
  subLabel,
  color = "var(--primary)",
  size = 140,
  strokeWidth = 12,
}: {
  value: number;
  max: number;
  label: string;
  subLabel?: string;
  color?: string;
  size?: number;
  strokeWidth?: number;
}) {
  const radius = (size - strokeWidth) / 2;
  const circumference = radius * 2 * Math.PI;
  const percent = max > 0 ? Math.min(Math.max(value / max, 0), 1) : 0;
  const offset = circumference - percent * circumference;

  return (
    <Box sx={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 1.5, height: "100%", justifyContent: "center" }}>
      <Box sx={{ position: "relative", width: size, height: size }}>
        <svg width={size} height={size} style={{ transform: "rotate(-90deg)", overflow: "visible" }}>
          {/* Track */}
          <circle
            cx={size / 2}
            cy={size / 2}
            r={radius}
            stroke="color-mix(in srgb, var(--border) 40%, transparent)"
            strokeWidth={strokeWidth}
            fill="none"
          />
          {/* Progress */}
          <circle
            cx={size / 2}
            cy={size / 2}
            r={radius}
            stroke={color}
            strokeWidth={strokeWidth}
            fill="none"
            strokeDasharray={circumference}
            strokeDashoffset={offset}
            strokeLinecap="round"
            style={{
              transition: "stroke-dashoffset 1s cubic-bezier(0.4, 0, 0.2, 1)",
              filter: `drop-shadow(0 0 6px color-mix(in srgb, ${color} 40%, transparent))`
            }}
          />
        </svg>
        <Box
          sx={{
            position: "absolute",
            inset: 0,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <Typography variant="h4" sx={{ fontFamily: "var(--font-mono)", fontWeight: 700, lineHeight: 1, color: "var(--foreground)" }}>
            {Math.round(percent * 100)}<span style={{ fontSize: "0.5em", color: "var(--muted)" }}>%</span>
          </Typography>
        </Box>
      </Box>
      <Box sx={{ textAlign: "center" }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 600, color: "var(--muted)", letterSpacing: "0.1em", textTransform: "uppercase", fontSize: "0.7rem" }}>
          {label}
        </Typography>
        {subLabel && (
          <Typography variant="caption" sx={{ color: "var(--muted)", mt: 0.5, fontFamily: "var(--font-mono)", display: "block" }}>
            {subLabel}
          </Typography>
        )}
      </Box>
    </Box>
  );
}

function DigitalCounter({ label, value, unit, color = "var(--foreground)", danger = false }: { label: string; value: string | number; unit?: string; color?: string; danger?: boolean }) {
  const isDanger = danger && Number(value) > 0;
  const finalColor = isDanger ? "var(--semantic-danger)" : color;
  
  return (
    <Box sx={{
      p: 1.5,
      bgcolor: "color-mix(in srgb, var(--foreground) 2%, transparent)",
      borderRadius: "var(--radius-chip)",
      border: "1px solid",
      borderColor: isDanger ? "color-mix(in srgb, var(--semantic-danger) 30%, transparent)" : "color-mix(in srgb, var(--border) 30%, transparent)",
      display: "flex",
      flexDirection: "column",
      gap: 0.5,
      boxShadow: isDanger ? "0 0 12px color-mix(in srgb, var(--semantic-danger) 20%, transparent)" : "none",
      minWidth: 0,
    }}>
      <Typography variant="caption" sx={{ color: isDanger ? "var(--semantic-danger)" : "var(--muted)", textTransform: "uppercase", letterSpacing: "0.05em", fontWeight: 600, fontSize: "0.65rem", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
        {label}
      </Typography>
      <Box sx={{ display: "flex", alignItems: "baseline", gap: 0.5, overflow: "hidden" }}>
        <Typography variant="h6" sx={{ fontFamily: "var(--font-mono)", fontWeight: 700, color: finalColor, textShadow: isDanger ? `0 0 8px ${finalColor}` : "none", lineHeight: 1.2, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
          {value}
        </Typography>
        {unit && <Typography variant="caption" sx={{ color: "var(--muted)", fontFamily: "var(--font-mono)" }}>{unit}</Typography>}
      </Box>
    </Box>
  );
}

export interface SystemStatusPanelProps {
  healthData: HealthData;
  resourceData: ResourceSnapshotData | null;
  metricsData: MetricsSnapshotData | null;
  t: TFunction;
}

export function SystemStatusPanel({
  healthData,
  resourceData,
  metricsData,
  t,
}: SystemStatusPanelProps) {
  const res = resourceData;
  const met = metricsData;

  const storageUsed = res?.storage_used_kb || 0;
  const storageTotal = res?.storage_total_kb || 0;

  const hasErrors = 
    (met?.errors_agent_router ?? 0) > 0 ||
    (met?.errors_agent_context ?? 0) > 0 ||
    (met?.errors_tool_execute ?? 0) > 0 ||
    (met?.errors_llm_request ?? 0) > 0 ||
    (met?.errors_llm_parse ?? 0) > 0 ||
    (met?.errors_channel_dispatch ?? 0) > 0 ||
    (met?.errors_session_append ?? 0) > 0 ||
    (met?.errors_agent_chat ?? 0) > 0 ||
    (met?.wifi_reconnect_total ?? 0) > 0 ||
    (met?.wifi_ap_restart_total ?? 0) > 0 ||
    (healthData.last_error && healthData.last_error !== "none");

  return (
    <React.Fragment>
      {/* Storage Gauge (Span 3 cols, 2 rows) */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 4", lg: "span 3" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard title={t("device.systemStatusStorage")} icon={<StorageRounded />}>
          <CircularGauge 
            value={storageUsed} 
            max={storageTotal} 
            label={t("device.systemStatusStorage")} 
            subLabel={`${storageUsed} / ${storageTotal} KB`} 
            color="var(--primary)" 
          />
        </DashboardCard>
      </Box>

      {/* RAM & Memory (Span 3 cols, 2 rows) */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 4", lg: "span 3" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard title={t("device.systemStatusGroupResource")} icon={<MemoryRounded />}>
          <Box sx={{ display: "grid", gridTemplateColumns: "1fr", gap: 1.5, height: "100%", alignContent: "start" }}>
            <DigitalCounter label={t("device.systemStatusHeapInternal")} value={res?.heap_free_internal != null ? formatBytes(res.heap_free_internal) : "—"} color="var(--semantic-warning)" />
            <DigitalCounter label={t("device.systemStatusHeapSpiram")} value={res?.heap_free_spiram != null ? formatBytes(res.heap_free_spiram) : "—"} color="var(--semantic-success)" />
            <DigitalCounter label={t("device.systemStatusHeapLargest")} value={res?.heap_largest_block_internal != null ? formatBytes(res.heap_largest_block_internal) : "—"} />
          </Box>
        </DashboardCard>
      </Box>

      {/* Traffic & Ops (Span 6 cols, 2 rows) */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 6" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard title={t("device.systemStatusGroupTraffic")} icon={<SwapVertRounded />}>
          <Box sx={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 1.5, height: "100%", alignContent: "start" }}>
            <DigitalCounter label={t("device.systemStatusActiveHttp")} value={res?.active_http_count ?? "—"} color="var(--primary)" />
            <DigitalCounter label={t("device.systemStatusSessionCount")} value={res?.session_count ?? "—"} color="var(--primary)" />
            <DigitalCounter label={t("device.systemStatusMessagesIn")} value={met?.messages_in ?? "—"} />
            <DigitalCounter label={t("device.systemStatusMessagesOut")} value={met?.messages_out ?? "—"} />
            <DigitalCounter label={t("device.systemStatusLlmCalls")} value={met?.llm_calls ?? "—"} />
            <DigitalCounter label={t("device.systemStatusToolCalls")} value={met?.tool_calls ?? "—"} />
            <DigitalCounter label={t("device.systemStatusInboundDepth")} value={res?.inbound_depth ?? "—"} />
            <DigitalCounter label={t("device.systemStatusOutboundDepth")} value={res?.outbound_depth ?? "—"} />
            <DigitalCounter label={t("device.systemStatusWdtFeeds")} value={met?.wdt_feeds ?? "—"} />
          </Box>
        </DashboardCard>
      </Box>

      {/* Error Telemetry (Span 12 cols, 1 or 2 rows depending on content) */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 12" }, gridRow: "auto" }}>
        <DashboardCard 
          title={t("device.systemStatusGroupErrors")} 
          icon={<WarningRounded />}
          sx={{
            borderColor: hasErrors ? "color-mix(in srgb, var(--semantic-danger) 40%, transparent)" : undefined,
            boxShadow: hasErrors ? "0 0 20px color-mix(in srgb, var(--semantic-danger) 10%, transparent)" : undefined,
          }}
        >
          <Box sx={{ display: "grid", gridTemplateColumns: { xs: "1fr 1fr", sm: "repeat(3, 1fr)", lg: "repeat(5, 1fr)" }, gap: 1.5 }}>
            <DigitalCounter label={t("device.systemStatusErrRouter")} value={met?.errors_agent_router ?? 0} danger />
            <DigitalCounter label={t("device.systemStatusErrContext")} value={met?.errors_agent_context ?? 0} danger />
            <DigitalCounter label={t("device.systemStatusErrToolExec")} value={met?.errors_tool_execute ?? 0} danger />
            <DigitalCounter label={t("device.systemStatusErrLlmReq")} value={met?.errors_llm_request ?? 0} danger />
            <DigitalCounter label={t("device.systemStatusErrLlmParse")} value={met?.errors_llm_parse ?? 0} danger />
            <DigitalCounter label={t("device.systemStatusDispatchFail")} value={met?.dispatch_send_fail ?? 0} danger />
            <DigitalCounter label={t("device.systemStatusErrSession")} value={met?.errors_session_append ?? 0} danger />
            <DigitalCounter label={t("device.systemStatusChatErrors")} value={met?.errors_agent_chat ?? 0} danger />
            <DigitalCounter label={t("device.systemStatusWifiReconnect")} value={met?.wifi_reconnect_total ?? 0} danger />
            <DigitalCounter label={t("device.systemStatusWifiApRestart")} value={met?.wifi_ap_restart_total ?? 0} danger />
          </Box>
          
          {/* Last Error Log */}
          {healthData.last_error && healthData.last_error !== "none" && (
            <Box sx={{ mt: 2, p: 2, bgcolor: "color-mix(in srgb, var(--semantic-danger) 10%, transparent)", borderRadius: "var(--radius-chip)", borderLeft: "4px solid var(--semantic-danger)" }}>
              <Typography variant="caption" sx={{ color: "var(--semantic-danger)", fontWeight: 600, letterSpacing: "0.1em", textTransform: "uppercase" }}>
                {t("device.systemStatusLastError")}
              </Typography>
              <Typography variant="body2" sx={{ fontFamily: "var(--font-mono)", color: "var(--foreground)", mt: 1, wordBreak: "break-all" }}>
                {healthData.last_error}
              </Typography>
            </Box>
          )}
        </DashboardCard>
      </Box>
    </React.Fragment>
  );
}
