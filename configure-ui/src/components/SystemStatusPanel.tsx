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
import type { DeviceRuntimeKind } from "../store/deviceStatusStore";
import { DashboardCard } from "../pages/DevicePage";
import {
  buildFaultAndRecoveryMetrics,
  buildMemoryMetrics,
  pressureLabelKey,
  buildRuntimeStrategyView,
} from "../pages/deviceHomeViewModel";

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

function formatStrategyBudgetValue(
  value: number,
  kind: "bytes" | "seconds",
): { value: string; unit?: string } {
  if (kind === "seconds") return { value: String(value), unit: "s" };
  return { value: formatBytes(value) };
}

function formatLoadAverage(value: [number, number, number] | undefined): string {
  if (!value) return "—";
  return value.map((item) => item.toFixed(2)).join(" / ");
}

function formatEpochSeconds(value: number | undefined): string {
  if (!value || value <= 0) return "—";
  return new Date(value * 1000).toLocaleString();
}

export interface SystemStatusPanelProps {
  healthData: HealthData;
  resourceData: ResourceSnapshotData | null;
  metricsData: MetricsSnapshotData | null;
  runtimeKind: DeviceRuntimeKind;
  t: TFunction;
}

export function SystemStatusPanel({
  healthData,
  resourceData,
  metricsData,
  runtimeKind,
  t,
}: SystemStatusPanelProps) {
  const res = resourceData;
  const met = metricsData;
  const memoryMetrics = buildMemoryMetrics(runtimeKind, res);
  const groupedFaults = buildFaultAndRecoveryMetrics(met);
  const strategy = buildRuntimeStrategyView(res);

  const storageUsed = res?.storage_used_kb || 0;
  const storageTotal = res?.storage_total_kb || 0;

  const hasErrors = 
    groupedFaults.faults.some((item) => item.value > 0) ||
    groupedFaults.recovery.some((item) => item.value > 0) ||
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
        <DashboardCard title={t("device.systemStatusGroupMemory")} icon={<MemoryRounded />}>
          <Box sx={{ display: "grid", gridTemplateColumns: "1fr", gap: 1.5, height: "100%", alignContent: "start" }}>
            {memoryMetrics.map((item) => (
              <DigitalCounter
                key={item.id}
                label={t(item.labelKey)}
                value={formatBytes(item.value)}
                color={item.id === "heap_internal" ? "var(--semantic-warning)" : item.id === "heap_spiram" ? "var(--semantic-success)" : "var(--foreground)"}
              />
            ))}
          </Box>
        </DashboardCard>
      </Box>

      {/* Runtime Strategy (Span 6 cols, 2 rows) */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 6" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard title={t("device.systemStatusStrategy")} icon={<SwapVertRounded />}>
          {strategy ? (
            <Box sx={{ display: "flex", flexDirection: "column", gap: 2, height: "100%" }}>
              {/* Header: Pressure & Level Indicator */}
              <Box sx={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 2 }}>
                <Box sx={{ minWidth: 0 }}>
                  <Typography variant="caption" sx={{ color: "var(--muted)", textTransform: "uppercase", letterSpacing: "0.08em", display: "block", mb: 0.75 }}>
                    {t("device.systemStatusPressure")}
                  </Typography>
                  <Typography variant="h5" sx={{ color: "var(--foreground)", fontWeight: 700, letterSpacing: "-0.03em", lineHeight: 1.05 }}>
                    {t(strategy.headlineKey)}
                  </Typography>
                  <Typography variant="body2" sx={{ color: "var(--muted)", mt: 1, lineHeight: 1.6, maxWidth: 360 }}>
                    {t(strategy.summaryKey)}
                  </Typography>
                </Box>
                <Box
                  sx={{
                    display: "flex",
                    alignItems: "center",
                    gap: 0.5,
                    pt: 0.5,
                    px: 1.5,
                    py: 0.5,
                    borderRadius: "var(--radius-sm)",
                    bgcolor: "color-mix(in srgb, var(--foreground) 2%, transparent)",
                    border: "1px solid color-mix(in srgb, var(--border) 20%, transparent)",
                  }}
                >
                  {[1, 2, 3].map((level) => {
                    const active = level <= strategy.intensity;
                    const dangerLevel = strategy.intensity === 3;
                    const color = dangerLevel
                      ? "var(--semantic-danger)"
                      : strategy.intensity === 2
                        ? "var(--semantic-warning)"
                        : "var(--semantic-success)";
                    return (
                      <Box
                        key={level}
                        sx={{
                          width: 8,
                          height: 12,
                          borderRadius: "2px",
                          bgcolor: active ? color : "color-mix(in srgb, var(--border) 25%, transparent)",
                          transition: "background-color 0.25s ease",
                        }}
                      />
                    );
                  })}
                  <Typography
                    variant="caption"
                    sx={{
                      ml: 0.5,
                      fontFamily: "var(--font-mono)",
                      fontWeight: 600,
                      color: "var(--muted)",
                      letterSpacing: "0.05em",
                    }}
                  >
                    {strategy.level && pressureLabelKey(strategy.level)
                      ? t(pressureLabelKey(strategy.level) as string)
                      : `LVL.${strategy.intensity}`}
                  </Typography>
                </Box>
              </Box>

              {/* Behavior List */}
              <Box
                sx={{
                  display: "flex",
                  flexDirection: "column",
                  gap: 0,
                  bgcolor: "color-mix(in srgb, var(--foreground) 1%, transparent)",
                  border: "1px solid color-mix(in srgb, var(--border) 15%, transparent)",
                  borderRadius: "var(--radius-chip)",
                  p: 1.5,
                  fontFamily: "var(--font-mono)",
                }}
              >
                {strategy.behaviorKeys.map((key) => (
                  <Box
                    key={key}
                    sx={{
                      display: "flex",
                      alignItems: "flex-start",
                      gap: 1.5,
                      py: 0.75,
                      borderBottom: "1px dashed color-mix(in srgb, var(--border) 15%, transparent)",
                      "&:last-child": { borderBottom: "none" },
                    }}
                  >
                    <Typography
                      component="span"
                      sx={{
                        color:
                          strategy.intensity === 3
                            ? "var(--semantic-danger)"
                            : strategy.intensity === 2
                              ? "var(--semantic-warning)"
                              : "var(--semantic-success)",
                        fontSize: "0.75rem",
                        lineHeight: 1.6,
                        userSelect: "none",
                      }}
                    >
                      ▶
                    </Typography>
                    <Typography
                      variant="body2"
                      sx={{
                        color: "var(--foreground)",
                        lineHeight: 1.6,
                        fontSize: "0.8rem",
                        fontFamily: "inherit",
                      }}
                    >
                      {t(key)}
                    </Typography>
                  </Box>
                ))}
              </Box>

              <Box sx={{ display: "grid", gridTemplateColumns: { xs: "1fr", sm: "repeat(3, 1fr)" }, gap: 1.5, mt: "auto" }}>
                {strategy.budgetFields.map((item) => {
                  const formatted = formatStrategyBudgetValue(item.value, item.valueKind);
                  return (
                    <DigitalCounter
                      key={item.id}
                      label={t(item.labelKey)}
                      value={formatted.value}
                      unit={formatted.unit}
                      color="var(--primary)"
                    />
                  );
                })}
              </Box>
            </Box>
          ) : (
            <Typography variant="body2" sx={{ color: "var(--muted)" }}>
              {t("common.na")}
            </Typography>
          )}
        </DashboardCard>
      </Box>

      {/* Traffic & Ops (Span 12 cols, 2 rows) */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 12" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard title={t("device.systemStatusGroupRuntime")} icon={<SwapVertRounded />}>
          <Box sx={{ display: "grid", gridTemplateColumns: { xs: "1fr 1fr", sm: "repeat(3, 1fr)", lg: "repeat(6, 1fr)" }, gap: 1.5, height: "100%", alignContent: "start" }}>
            <DigitalCounter label={t("device.systemStatusActiveHttp")} value={res?.active_http_count ?? "—"} color="var(--primary)" />
            <DigitalCounter label={t("device.systemStatusActiveWss")} value={res?.active_wss_count ?? "—"} color="var(--primary)" />
            <DigitalCounter label={t("device.systemStatusActiveAgentTasks")} value={res?.active_agent_tasks ?? "—"} color="var(--primary)" />
            <DigitalCounter label={t("device.systemStatusSessionCount")} value={res?.session_count ?? "—"} color="var(--primary)" />
            <DigitalCounter label={t("device.systemStatusMessagesIn")} value={met?.messages_in ?? "—"} />
            <DigitalCounter label={t("device.systemStatusMessagesOut")} value={met?.messages_out ?? "—"} />
            <DigitalCounter label={t("device.systemStatusLlmCalls")} value={met?.llm_calls ?? "—"} />
            <DigitalCounter label={t("device.systemStatusLlmLastMs")} value={met?.llm_last_ms ?? "—"} />
            <DigitalCounter label={t("device.systemStatusToolCalls")} value={met?.tool_calls ?? "—"} />
            <DigitalCounter label={t("device.systemStatusDispatchOk")} value={met?.dispatch_send_ok ?? "—"} />
            <DigitalCounter label={t("device.systemStatusInboundDepth")} value={res?.inbound_depth ?? "—"} />
            <DigitalCounter label={t("device.systemStatusOutboundDepth")} value={res?.outbound_depth ?? "—"} />
            <DigitalCounter label={t("device.systemStatusWdtFeeds")} value={met?.wdt_feeds ?? "—"} />
            <DigitalCounter label={t("device.systemStatusLastActiveAt")} value={formatEpochSeconds(met?.last_active_epoch_secs)} color="var(--semantic-warning)" />
            <DigitalCounter label={t("device.systemStatusCpuUsage")} value={res?.cpu_usage_percent != null ? res.cpu_usage_percent.toFixed(1) : "—"} unit="%" color="var(--semantic-warning)" />
            <DigitalCounter label={t("device.systemStatusProcessMemory")} value={res?.process_memory_kb ?? "—"} unit="KB" color="var(--semantic-warning)" />
            <DigitalCounter label={t("device.systemStatusLoadAverage")} value={formatLoadAverage(res?.load_average)} color="var(--semantic-warning)" />
          </Box>
        </DashboardCard>
      </Box>

      {/* Error Telemetry (Span 12 cols, 2 rows) */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 12" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard 
          title={t("device.systemStatusGroupFaults")} 
          icon={<WarningRounded />}
          sx={{
            borderColor: hasErrors ? "color-mix(in srgb, var(--semantic-danger) 40%, transparent)" : undefined,
            boxShadow: hasErrors ? "0 0 20px color-mix(in srgb, var(--semantic-danger) 10%, transparent)" : undefined,
          }}
        >
          <Typography variant="caption" sx={{ color: "var(--muted)", textTransform: "uppercase", letterSpacing: "0.08em", mb: 1, display: "block" }}>
            {t("device.systemStatusSubsectionFaults")}
          </Typography>
          <Box sx={{ display: "grid", gridTemplateColumns: { xs: "1fr 1fr", sm: "repeat(3, 1fr)", lg: "repeat(6, 1fr)" }, gap: 1.5 }}>
            {groupedFaults.faults.map((item) => (
              <DigitalCounter key={item.id} label={t(item.labelKey)} value={item.value} danger />
            ))}
          </Box>
          <Typography variant="caption" sx={{ color: "var(--muted)", textTransform: "uppercase", letterSpacing: "0.08em", mt: 2, mb: 1, display: "block" }}>
            {t("device.systemStatusSubsectionRecovery")}
          </Typography>
          <Box sx={{ display: "grid", gridTemplateColumns: { xs: "1fr 1fr", sm: "repeat(3, 1fr)", lg: "repeat(6, 1fr)" }, gap: 1.5 }}>
            {groupedFaults.recovery.map((item) => (
              <DigitalCounter
                key={item.id}
                label={t(item.labelKey)}
                value={item.value}
                color="var(--semantic-warning)"
              />
            ))}
          </Box>
          {met?.wifi_last_failure_stage && met.wifi_last_failure_stage !== "none" && (
            <Box sx={{ mt: 2, p: 2, bgcolor: "color-mix(in srgb, var(--semantic-warning) 10%, transparent)", borderRadius: "var(--radius-chip)", borderLeft: "4px solid var(--semantic-warning)" }}>
              <Typography variant="caption" sx={{ color: "var(--semantic-warning)", fontWeight: 600, letterSpacing: "0.1em", textTransform: "uppercase" }}>
                {t("device.systemStatusWifiLastFail")}
              </Typography>
              <Typography variant="body2" sx={{ fontFamily: "var(--font-mono)", color: "var(--foreground)", mt: 1, wordBreak: "break-all" }}>
                {met.wifi_last_failure_stage}
              </Typography>
            </Box>
          )}
          
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
