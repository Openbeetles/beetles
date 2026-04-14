import React from "react";
import type { TFunction } from "i18next";
import Box from "@mui/material/Box";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";
import Typography from "@mui/material/Typography";
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
import { Os3dIcon } from "./Os3dIcon";
import { OS_ICON_DASHBOARD } from "../config/osIcons";
import { DashboardCard } from "../pages/DevicePage";
import {
  DASHBOARD_BLOCK_GAP,
  DASHBOARD_INSET_WELL_BG,
  DASHBOARD_SECTION_STACK_GAP,
  UI_LABEL_SECONDARY_SX,
} from "../theme/panelStyles";
import {
  buildFaultAndRecoveryMetrics,
  buildMemoryMetrics,
  buildRuntimeTelemetryFields,
  buildRuntimeStrategyView,
  type RuntimeStrategyBudgetField,
} from "../pages/deviceHomeViewModel";

// Non-component exports removed to fix Fast Refresh lint error.
// They are now defined in DevicePage.tsx or kept internal here.

/** 运行策略「行为列表」与「预算」图标列同宽，保证与正文左缘对齐 */
const STRATEGY_ALIGN_ICON_PX = 28;

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
            {Math.round(percent * 100)}<span style={{ fontSize: "0.5em", color: "var(--text-tertiary)" }}>%</span>
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
          <Typography variant="caption" sx={{ color: "var(--text-tertiary)", mt: 0.5, fontFamily: "var(--font-mono)", display: "block" }}>
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
            <Typography component="span" variant="caption" sx={{ color: "var(--text-tertiary)", fontFamily: "var(--font-mono)", fontWeight: 500, ml: 0.25 }}>
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
            <Typography variant="caption" sx={{ color: "var(--text-tertiary)", mt: label ? 0.25 : 0, fontFamily: "var(--font-mono)", display: "block", fontSize: "0.65rem" }}>
              {subLabel}
            </Typography>
          ) : null}
        </Box>
      )}
    </Box>
  );
}

function strategyAccentColor(intensity: 1 | 2 | 3): string {
  if (intensity === 3) return "var(--semantic-danger)";
  if (intensity === 2) return "var(--semantic-warning)";
  return "var(--semantic-success)";
}

/** 与 behaviorKeys 顺序一致：对话/工具/重连 */
const STRATEGY_BEHAVIOR_DIM_KEYS = [
  "device.systemStatusStrategyBehaviorDimReplies",
  "device.systemStatusStrategyBehaviorDimTools",
  "device.systemStatusStrategyBehaviorDimReconnect",
] as const;

const STRATEGY_BEHAVIOR_ICONS = [ChatBubbleOutlineRounded, ExtensionRounded, SyncRounded] as const;

/**
 * 行为说明：三行列表（无套层灰底，避免与卡底对比形成「描边」）；与下方预算参数表区分层级。
 */
function StrategyBehaviorList({
  behaviorKeys,
  accent,
  t,
}: {
  behaviorKeys: [string, string, string];
  accent: string;
  t: TFunction;
}) {
  return (
    <Box
      component="ul"
      sx={{ listStyle: "none", m: 0, p: 0, display: "flex", flexDirection: "column", gap: 1.15 }}
    >
      {behaviorKeys.map((key, index) => {
        const Icon = STRATEGY_BEHAVIOR_ICONS[index];
        return (
          <Box
            component="li"
            key={key}
            sx={{
              display: "flex",
              alignItems: "flex-start",
              gap: 1,
              minWidth: 0,
            }}
          >
            <Box
              sx={{
                width: STRATEGY_ALIGN_ICON_PX,
                minWidth: STRATEGY_ALIGN_ICON_PX,
                flexShrink: 0,
                color: accent,
                mt: 0.1,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                opacity: 0.92,
              }}
              aria-hidden
            >
              <Icon sx={{ fontSize: "1.05rem" }} />
            </Box>
            <Typography
              component="p"
              sx={{
                m: 0,
                fontSize: "0.8125rem",
                lineHeight: 1.5,
                color: "var(--foreground)",
                minWidth: 0,
              }}
            >
              <Box component="span" sx={{ fontWeight: 600, color: "var(--foreground-soft)" }}>
                {t(STRATEGY_BEHAVIOR_DIM_KEYS[index])}
              </Box>
              <Box component="span" sx={{ color: "var(--text-tertiary)", px: 0.45 }}>
                ·
              </Box>
              {t(key)}
            </Typography>
          </Box>
        );
      })}
    </Box>
  );
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

/** 预算表左栏简称（完整说明见 `labelKey` + `title`） */
const STRATEGY_BUDGET_ABBR_KEY: Record<string, string> = {
  messages_max: "device.systemStatusStrategyBudgetAbbrMessages",
  system_prompt_max: "device.systemStatusStrategyBudgetAbbrSystemPrompt",
  response_body_max: "device.systemStatusStrategyBudgetAbbrResponseBody",
  reconnect_backoff_secs: "device.systemStatusStrategyBudgetAbbrReconnect",
};

/**
 * 运行策略预算：紧凑参数表（左图标 + 简称，右等宽数值）。
 */
function StrategyBudgetTable({
  fields,
  accent,
  t,
}: {
  fields: RuntimeStrategyBudgetField[];
  accent: string;
  t: TFunction;
}) {
  if (fields.length === 0) return null;
  return (
    <Box sx={{ width: "100%", pt: 0.25 }}>
      <Table
        size="small"
        sx={{
          width: "100%",
          borderCollapse: "separate",
          borderSpacing: 0,
          "& .MuiTableCell-root": {
            borderBottom: "var(--divider-row)",
            py: 0.65,
            px: 0,
            verticalAlign: "middle",
          },
          "& .MuiTableRow:last-of-type .MuiTableCell-root": {
            borderBottom: "none",
          },
        }}
      >
        <TableBody>
          {fields.map((item) => {
            const formatted = formatStrategyBudgetValue(item.value, item.valueKind);
            const abbrKey = STRATEGY_BUDGET_ABBR_KEY[item.id];
            const shortLabel = abbrKey ? t(abbrKey) : t(item.labelKey);
            const fullLabel = t(item.labelKey);
            return (
              <TableRow key={item.id}>
                <TableCell
                  component="th"
                  scope="row"
                  title={fullLabel}
                  sx={{
                    width: "58%",
                    minWidth: 0,
                    pr: 1,
                    borderColor: "transparent",
                    fontWeight: "inherit",
                  }}
                >
                  <Box
                    sx={{
                      display: "flex",
                      alignItems: "center",
                      gap: 1,
                      minWidth: 0,
                    }}
                  >
                    <Box
                      sx={{
                        width: 24,
                        minWidth: 24,
                        height: 24,
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        color: accent,
                        flexShrink: 0,
                        opacity: 0.92,
                      }}
                      aria-hidden
                    >
                      {strategyBudgetIcon(item.id)}
                    </Box>
                    <Typography
                      variant="caption"
                      component="span"
                      sx={{
                        fontWeight: 600,
                        fontSize: "var(--font-size-caption)",
                        color: "var(--foreground-soft)",
                        lineHeight: 1.35,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {shortLabel}
                    </Typography>
                  </Box>
                </TableCell>
                <TableCell
                  align="right"
                  title={fullLabel}
                  sx={{
                    borderColor: "transparent",
                    fontFamily: "var(--font-mono)",
                    fontWeight: 700,
                    fontSize: "0.9375rem",
                    color: "var(--primary)",
                    whiteSpace: "nowrap",
                  }}
                >
                  <Box
                    component="span"
                    sx={{ display: "inline-flex", alignItems: "baseline", gap: 0.4, justifyContent: "flex-end" }}
                  >
                    {formatted.value}
                    {formatted.unit ? (
                      <Typography
                        component="span"
                        variant="caption"
                        sx={{
                          color: "var(--text-tertiary)",
                          fontFamily: "var(--font-mono)",
                          fontSize: "0.7rem",
                          fontWeight: 500,
                        }}
                      >
                        {formatted.unit}
                      </Typography>
                    ) : null}
                  </Box>
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </Box>
  );
}

function DigitalCounter({
  label,
  value,
  unit,
  color = "var(--foreground)",
  danger = false,
}: {
  label: string;
  value: string | number;
  unit?: string;
  color?: string;
  danger?: boolean;
}) {
  const isDanger = danger && Number(value) > 0;
  const finalColor = isDanger ? "var(--semantic-danger)" : color;

  return (
    <Box
      sx={{
        p: 1.5,
        bgcolor: DASHBOARD_INSET_WELL_BG,
        borderRadius: "var(--radius-chip)",
        display: "flex",
        flexDirection: "column",
        gap: 0.5,
        border: isDanger
          ? "1px solid color-mix(in srgb, var(--semantic-danger) 38%, transparent)"
          : "none",
        boxShadow: "none",
        minWidth: 0,
      }}
    >
      <Typography
        variant="caption"
        component="span"
        sx={{
          color: isDanger ? "var(--semantic-danger)" : "var(--foreground-soft)",
          textTransform: "none",
          letterSpacing: "0.02em",
          fontWeight: 600,
          fontSize: "0.68rem",
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "ellipsis",
        }}
      >
        {label}
      </Typography>
      <Box
        sx={{
          display: "flex",
          alignItems: "baseline",
          gap: 0.45,
          overflow: "hidden",
        }}
      >
        <Typography
          variant="h6"
          sx={{
            fontFamily: "var(--font-mono)",
            fontWeight: 700,
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
          <Typography variant="caption" sx={{ color: "var(--text-tertiary)", fontFamily: "var(--font-mono)" }}>
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
      {/* Storage Gauge (Span 4 cols, 2 rows) */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 4", lg: "span 4" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard title={t("device.systemStatusStorage")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.storage} />}>
          <CircularGauge 
            value={storageUsed} 
            max={storageTotal} 
            label={t("device.systemStatusStorage")} 
            subLabel={`${storageUsed} / ${storageTotal} KB`} 
            color="var(--primary)" 
          />
        </DashboardCard>
      </Box>

      {/* RAM & Memory (Span 4 cols, 2 rows) */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 4", lg: "span 4" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard title={t("device.systemStatusGroupMemory")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.memory} />}>
          <Box sx={{ display: "grid", gridTemplateColumns: "1fr", gap: DASHBOARD_BLOCK_GAP, height: "100%", alignContent: "start" }}>
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

      {/* Runtime Strategy (Span 4 cols, 2 rows) */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 4" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard title={t("device.systemStatusStrategy")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.strategy} />}>
          {strategy ? (
            <Box sx={{ display: "flex", flexDirection: "column", gap: DASHBOARD_SECTION_STACK_GAP, height: "100%" }}>
              <Box
                sx={{
                  display: "flex",
                  flexDirection: { xs: "column", sm: "row" },
                  alignItems: { xs: "center", sm: "center" },
                  gap: DASHBOARD_SECTION_STACK_GAP,
                }}
              >
                <IntensityLevelRing intensity={strategy.intensity} size={92} strokeWidth={8} color={strategyAccentColor(strategy.intensity)} />
                <Box sx={{ minWidth: 0, flex: 1, textAlign: { xs: "center", sm: "left" } }}>
                  <Typography
                    variant="h5"
                    sx={{
                      color: "var(--foreground)",
                      fontWeight: 600,
                      letterSpacing: "-0.02em",
                      lineHeight: 1.2,
                      fontSize: { xs: "1.1rem", sm: "1.2rem" },
                    }}
                  >
                    {t(strategy.headlineKey)}
                  </Typography>
                  <Typography variant="body2" sx={{ color: "var(--text-tertiary)", mt: 0.75, lineHeight: 1.55, fontSize: "0.8125rem" }}>
                    {t(strategy.summaryKey)}
                  </Typography>
                </Box>
              </Box>

              <StrategyBehaviorList
                behaviorKeys={strategy.behaviorKeys}
                accent={strategyAccentColor(strategy.intensity)}
                t={t}
              />

              <StrategyBudgetTable
                fields={strategy.budgetFields}
                accent={strategyAccentColor(strategy.intensity)}
                t={t}
              />
            </Box>
          ) : (
            <Typography variant="body2" sx={{ color: "var(--text-tertiary)" }}>
              {t("common.na")}
            </Typography>
          )}
        </DashboardCard>
      </Box>

      {/* Traffic & Ops (Span 12 cols, 2 rows) */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 12" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard title={t("device.systemStatusGroupRuntime")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.runtime} />}>
          <Box sx={{ display: "grid", gridTemplateColumns: { xs: "1fr 1fr", sm: "repeat(3, 1fr)", lg: "repeat(6, 1fr)" }, gap: DASHBOARD_BLOCK_GAP, height: "100%", alignContent: "start" }}>
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
          icon={<Os3dIcon src={OS_ICON_DASHBOARD.faults} />}
          sx={
            hasErrors
              ? {
                  border:
                    "1px solid color-mix(in srgb, var(--semantic-danger) 32%, transparent)",
                  boxShadow: "none",
                }
              : undefined
          }
        >
          <Typography
            variant="caption"
            component="div"
            sx={{
              ...UI_LABEL_SECONDARY_SX,
              textTransform: "uppercase",
              letterSpacing: "0.08em",
              color: "var(--text-tertiary)",
              mb: 1,
            }}
          >
            {t("device.systemStatusSubsectionFaults")}
          </Typography>
          <Box sx={{ display: "grid", gridTemplateColumns: { xs: "1fr 1fr", sm: "repeat(3, 1fr)", lg: "repeat(6, 1fr)" }, gap: DASHBOARD_BLOCK_GAP }}>
            {groupedFaults.faults.map((item) => (
              <DigitalCounter key={item.id} label={t(item.labelKey)} value={item.value} danger />
            ))}
          </Box>
          <Typography
            variant="caption"
            component="div"
            sx={{
              ...UI_LABEL_SECONDARY_SX,
              textTransform: "uppercase",
              letterSpacing: "0.08em",
              color: "var(--text-tertiary)",
              mt: 2,
              mb: 1,
            }}
          >
            {t("device.systemStatusSubsectionRecovery")}
          </Typography>
          <Box sx={{ display: "grid", gridTemplateColumns: { xs: "1fr 1fr", sm: "repeat(3, 1fr)", lg: "repeat(6, 1fr)" }, gap: DASHBOARD_BLOCK_GAP }}>
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
