import React from "react";
import type { TFunction } from "i18next";
import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import StorageRounded from "@mui/icons-material/StorageRounded";
import MemoryRounded from "@mui/icons-material/MemoryRounded";
import SwapVertRounded from "@mui/icons-material/SwapVertRounded";
import WarningRounded from "@mui/icons-material/WarningRounded";
import TuneRounded from "@mui/icons-material/TuneRounded";
import ChatBubbleOutlineRounded from "@mui/icons-material/ChatBubbleOutlineRounded";
import ExtensionRounded from "@mui/icons-material/ExtensionRounded";
import SyncRounded from "@mui/icons-material/SyncRounded";
import NotesRounded from "@mui/icons-material/NotesRounded";
import ForumRounded from "@mui/icons-material/ForumRounded";
import ArticleRounded from "@mui/icons-material/ArticleRounded";
import TimerOutlined from "@mui/icons-material/TimerOutlined";
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
  buildRuntimeTelemetryFields,
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
            stroke="var(--border-subtle)"
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
        <Typography
          variant="subtitle2"
          sx={{
            fontWeight: 600,
            color: "var(--foreground-soft)",
            letterSpacing: "0.04em",
            textTransform: "none",
            fontSize: "0.7rem",
          }}
        >
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

/** 运行策略压力等级：1–3 档，中心显示档位而非百分比，与存储/内存表盘风格一致 */
function IntensityLevelRing({
  intensity,
  label,
  subLabel,
  color,
  size = 100,
  strokeWidth = 9,
}: {
  intensity: 1 | 2 | 3;
  /** 省略时不展示表盘下文案（与标题/正文/细条档位避免重复） */
  label?: string;
  subLabel?: string;
  color: string;
  size?: number;
  strokeWidth?: number;
}) {
  const max = 3;
  const radius = (size - strokeWidth) / 2;
  const circumference = radius * 2 * Math.PI;
  const percent = max > 0 ? Math.min(Math.max(intensity / max, 0), 1) : 0;
  const offset = circumference - percent * circumference;

  return (
    <Box sx={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 1, flexShrink: 0 }}>
      <Box sx={{ position: "relative", width: size, height: size }}>
        <svg width={size} height={size} style={{ transform: "rotate(-90deg)", overflow: "visible" }}>
          <circle
            cx={size / 2}
            cy={size / 2}
            r={radius}
            stroke="var(--border-subtle)"
            strokeWidth={strokeWidth}
            fill="none"
          />
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
              transition: "stroke-dashoffset 0.8s cubic-bezier(0.4, 0, 0.2, 1)",
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
            {intensity}
            <Typography component="span" variant="caption" sx={{ color: "var(--muted)", fontFamily: "var(--font-mono)", fontWeight: 500, ml: 0.25 }}>
              /3
            </Typography>
          </Typography>
        </Box>
      </Box>
      {(label || subLabel) && (
        <Box sx={{ textAlign: "center", maxWidth: size + 24 }}>
          {label ? (
            <Typography
              variant="subtitle2"
              sx={{
                fontWeight: 600,
                color: "var(--foreground-soft)",
                letterSpacing: "0.03em",
                textTransform: "none",
                fontSize: "0.7rem",
              }}
            >
              {label}
            </Typography>
          ) : null}
          {subLabel ? (
            <Typography variant="caption" sx={{ color: "var(--muted)", mt: label ? 0.25 : 0, fontFamily: "var(--font-mono)", display: "block", fontSize: "0.65rem" }}>
              {subLabel}
            </Typography>
          ) : null}
        </Box>
      )}
    </Box>
  );
}

/** 细进度条 + 三档文案，避免大块色面与强阴影 */
function StrategyPressureTrack({
  intensity,
  t,
}: {
  intensity: 1 | 2 | 3;
  t: TFunction;
}) {
  const steps: { key: "Normal" | "Cautious" | "Critical"; level: 1 | 2 | 3 }[] = [
    { key: "Normal", level: 1 },
    { key: "Cautious", level: 2 },
    { key: "Critical", level: 3 },
  ];
  const fillPct = (intensity / 3) * 100;
  const accent = strategyAccentColor(intensity);

  return (
    <Box sx={{ width: "100%" }}>
      <Box
        sx={{
          height: 3,
          borderRadius: 999,
          bgcolor: "var(--border-subtle)",
          overflow: "hidden",
        }}
      >
        <Box
          sx={{
            height: "100%",
            width: `${fillPct}%`,
            bgcolor: accent,
            opacity: 0.38,
            transition: "width 0.45s cubic-bezier(0.4, 0, 0.2, 1)",
          }}
        />
      </Box>
      <Box sx={{ display: "flex", justifyContent: "space-between", gap: 1, mt: 0.85 }}>
        {steps.map((step) => {
          const labelKey = pressureLabelKey(step.key) as string;
          const isCurrent = intensity === step.level;
          return (
            <Typography
              key={step.key}
              variant="caption"
              sx={{
                flex: 1,
                textAlign: "center",
                fontSize: "0.68rem",
                fontWeight: isCurrent ? 600 : 400,
                color: isCurrent ? "var(--foreground)" : "var(--muted)",
                letterSpacing: "0.02em",
                textTransform: "none",
                lineHeight: 1.35,
              }}
            >
              {t(labelKey)}
            </Typography>
          );
        })}
      </Box>
    </Box>
  );
}

function strategyAccentColor(intensity: 1 | 2 | 3): string {
  if (intensity === 3) return "var(--semantic-danger)";
  if (intensity === 2) return "var(--semantic-warning)";
  return "var(--semantic-success)";
}

function strategyBehaviorIcon(index: number): React.ReactNode {
  const sx = { fontSize: "0.88rem" };
  switch (index) {
    case 0:
      return <ChatBubbleOutlineRounded sx={sx} />;
    case 1:
      return <ExtensionRounded sx={sx} />;
    case 2:
      return <SyncRounded sx={sx} />;
    default:
      return null;
  }
}

function strategyBudgetIcon(id: string): React.ReactNode {
  const sx = { fontSize: "0.82rem", opacity: 0.75 };
  switch (id) {
    case "messages_max":
      return <ForumRounded sx={sx} />;
    case "system_prompt_max":
      return <NotesRounded sx={sx} />;
    case "response_body_max":
      return <ArticleRounded sx={sx} />;
    case "reconnect_backoff_secs":
      return <TimerOutlined sx={sx} />;
    default:
      return null;
  }
}

function DigitalCounter({
  label,
  value,
  unit,
  color = "var(--foreground)",
  danger = false,
  leadingIcon,
  compact,
}: {
  label: string;
  value: string | number;
  unit?: string;
  color?: string;
  danger?: boolean;
  leadingIcon?: React.ReactNode;
  /** 运行策略预算等：更轻边框与图标槽，避免与其它仪表盘数字块抢戏 */
  compact?: boolean;
}) {
  const isDanger = danger && Number(value) > 0;
  const finalColor = isDanger ? "var(--semantic-danger)" : color;

  return (
    <Box
      sx={{
        p: compact ? 1.15 : 1.5,
        bgcolor: "color-mix(in srgb, var(--foreground) 2%, transparent)",
        borderRadius: "var(--radius-chip)",
        border: "1px solid",
        borderColor: isDanger
          ? "color-mix(in srgb, var(--semantic-danger) 22%, var(--border-subtle))"
          : "var(--border-subtle)",
        display: "flex",
        flexDirection: "column",
        gap: compact ? 0.35 : 0.5,
        boxShadow: isDanger ? "0 0 12px color-mix(in srgb, var(--semantic-danger) 20%, transparent)" : "none",
        minWidth: 0,
      }}
    >
      <Box sx={{ display: "flex", alignItems: "center", gap: compact ? 0.65 : 0.75, minWidth: 0 }}>
        {leadingIcon ? (
          <Box
            sx={{
              flexShrink: 0,
              width: compact ? 26 : undefined,
              height: compact ? 26 : undefined,
              borderRadius: compact ? "var(--radius-sm)" : 0,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              bgcolor: compact ? "color-mix(in srgb, var(--foreground) 5%, transparent)" : undefined,
              color: "var(--foreground-soft)",
            }}
          >
            {leadingIcon}
          </Box>
        ) : null}
        <Typography
          variant="caption"
          component="span"
          sx={{
            color: isDanger ? "var(--semantic-danger)" : "var(--foreground-soft)",
            textTransform: "none",
            letterSpacing: "0.02em",
            fontWeight: compact ? 500 : 600,
            fontSize: compact ? "0.64rem" : "0.68rem",
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {label}
        </Typography>
      </Box>
      <Box
        sx={{
          display: "flex",
          alignItems: "baseline",
          gap: 0.45,
          overflow: "hidden",
          pl: leadingIcon && compact ? 0.25 : 0,
        }}
      >
        <Typography
          variant={compact ? "subtitle1" : "h6"}
          sx={{
            fontFamily: "var(--font-mono)",
            fontWeight: 700,
            fontSize: compact ? "0.95rem" : undefined,
            color: finalColor,
            textShadow: isDanger ? `0 0 8px ${finalColor}` : "none",
            lineHeight: 1.2,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {value}
        </Typography>
        {unit && (
          <Typography variant="caption" sx={{ color: "var(--muted)", fontFamily: "var(--font-mono)", fontSize: compact ? "0.65rem" : undefined }}>
            {unit}
          </Typography>
        )}
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
  const runtimeTelemetry = buildRuntimeTelemetryFields(runtimeKind, res, met);

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
        <DashboardCard title={t("device.systemStatusStrategy")} icon={<TuneRounded />}>
          {strategy ? (
            <Box sx={{ display: "flex", flexDirection: "column", gap: 1.5, height: "100%" }}>
              <Box
                sx={{
                  display: "flex",
                  flexDirection: { xs: "column", sm: "row" },
                  alignItems: { xs: "stretch", sm: "center" },
                  gap: 1.75,
                }}
              >
                <IntensityLevelRing intensity={strategy.intensity} color={strategyAccentColor(strategy.intensity)} />
                <Box sx={{ minWidth: 0, flex: 1 }}>
                  <Typography variant="h5" sx={{ color: "var(--foreground)", fontWeight: 600, letterSpacing: "-0.02em", lineHeight: 1.15, fontSize: { xs: "1.15rem", sm: "1.25rem" } }}>
                    {t(strategy.headlineKey)}
                  </Typography>
                  <Typography variant="body2" sx={{ color: "var(--muted)", mt: 0.75, lineHeight: 1.55, fontSize: "0.8125rem" }}>
                    {t(strategy.summaryKey)}
                  </Typography>
                </Box>
              </Box>

              <StrategyPressureTrack intensity={strategy.intensity} t={t} />

              <Box component="ul" sx={{ listStyle: "none", m: 0, p: 0, display: "flex", flexDirection: "column", gap: 0 }}>
                {strategy.behaviorKeys.map((key, index) => (
                  <Box
                    component="li"
                    key={key}
                    sx={{
                      display: "flex",
                      alignItems: "flex-start",
                      gap: 1.1,
                      py: 0.7,
                      borderTop:
                        index === 0
                          ? "none"
                          : "1px solid var(--border-subtle)",
                    }}
                  >
                    <Box
                      aria-hidden
                      sx={{
                        width: 30,
                        height: 30,
                        flexShrink: 0,
                        borderRadius: "var(--radius-sm)",
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        bgcolor: "color-mix(in srgb, var(--foreground) 4%, transparent)",
                        color: "var(--foreground-soft)",
                      }}
                    >
                      {strategyBehaviorIcon(index)}
                    </Box>
                    <Typography
                      variant="body2"
                      component="p"
                      sx={{
                        color: "var(--foreground)",
                        lineHeight: 1.5,
                        fontSize: "0.8125rem",
                        m: 0,
                        pt: 0.2,
                      }}
                    >
                      {t(key)}
                    </Typography>
                  </Box>
                ))}
              </Box>

              <Box sx={{ display: "grid", gridTemplateColumns: { xs: "1fr", sm: "repeat(3, 1fr)" }, gap: 1.15, mt: "auto" }}>
                {strategy.budgetFields.map((item) => {
                  const formatted = formatStrategyBudgetValue(item.value, item.valueKind);
                  return (
                    <DigitalCounter
                      key={item.id}
                      label={t(item.labelKey)}
                      value={formatted.value}
                      unit={formatted.unit}
                      color="var(--primary)"
                      leadingIcon={strategyBudgetIcon(item.id)}
                      compact
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
            {runtimeTelemetry.map((item) => {
              let value: string | number;
              switch (item.valueKind) {
                case "epoch_seconds":
                  value = formatEpochSeconds(item.value as number);
                  break;
                case "float1":
                  value = (item.value as number).toFixed(1);
                  break;
                case "load_average":
                  value = formatLoadAverage(item.value as [number, number, number]);
                  break;
                case "number":
                default:
                  value = item.value as number;
                  break;
              }
              return (
                <DigitalCounter
                  key={item.id}
                  label={t(item.labelKey)}
                  value={value}
                  unit={item.unit}
                  color={item.color}
                />
              );
            })}
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
