import React from "react";
import type { TFunction } from "i18next";
import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import type {
  HealthData,
  MetricsSnapshotData,
  ResourceSnapshotData,
  SystemInfoData,
} from "../api/endpoints/system";
import type { DeviceRuntimeKind } from "../store/deviceStatusStore";
import { Os3dIcon } from "./Os3dIcon";
import { OS_ICON_DASHBOARD } from "../config/osIcons";
import { DashboardCard } from "../pages/DevicePage";
import {
  DASHBOARD_BLOCK_GAP,
  DASHBOARD_INSET_WELL_BG,
  UI_LABEL_SECONDARY_SX,
} from "../theme/panelStyles";
import {
  buildFaultAndRecoveryMetrics,
  buildExecutionTimingFields,
  buildHealthDetailFields,
  buildHttpStorageStreamFields,
  buildMemoryMetrics,
  buildProgrammableReasoningFields,
  buildResourceGovernanceFields,
  buildResourceRiskFields,
  buildRuntimeTelemetryFields,
  buildRuntimeStrategyView,
  buildStorageMediaDetailFields,
  buildTurnProtocolFields,
  buildVoiceAudioTelemetryFields,
  type HomeDetailField,
  type HomeMetricField,
  type RuntimeStrategyBudgetField,
  type RuntimeStrategyViewModel,
  type StorageMediaDetailView,
} from "../pages/deviceHomeViewModel";

// Non-component exports removed to fix Fast Refresh lint error.
// They are now defined in DevicePage.tsx or kept internal here.

function buildStatusNoticeSx(accent: string) {
  return {
    mt: 2,
    p: 2,
    borderRadius: "var(--radius-chip)",
    border: `1px solid color-mix(in srgb, ${accent} 16%, var(--border))`,
    bgcolor: "color-mix(in srgb, var(--card) 78%, transparent)",
    backgroundImage: [
      "linear-gradient(180deg, color-mix(in srgb, #fff 18%, transparent) 0%, transparent 62%)",
      `linear-gradient(135deg, color-mix(in srgb, ${accent} 6%, transparent) 0%, transparent 46%, color-mix(in srgb, var(--accent) 4%, transparent) 100%)`,
    ].join(", "),
    boxShadow: [
      `0 18px 32px -30px color-mix(in srgb, ${accent} 20%, transparent)`,
      "inset 0 1px 0 color-mix(in srgb, #fff 52%, transparent)",
    ].join(", "),
    backdropFilter: "blur(calc(var(--glass-blur) * 0.45)) saturate(1.03)",
    WebkitBackdropFilter:
      "blur(calc(var(--glass-blur) * 0.45)) saturate(1.03)",
  } as const;
}

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
  centerLabel,
  color = "var(--primary)",
  size = 140,
  strokeWidth = 12,
}: {
  value: number;
  max: number;
  label: string;
  subLabel?: string;
  centerLabel?: string;
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
            {centerLabel ?? (
              <>
                {Math.round(percent * 100)}<span style={{ fontSize: "0.5em", color: "var(--text-tertiary)" }}>%</span>
              </>
            )}
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

function StrategyBehaviorRow({
  behaviorKeys,
  t,
}: {
  behaviorKeys: [string, string, string];
  t: TFunction;
}) {
  return (
    <Box
      sx={{
        display: "grid",
        gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
        gap: 1,
        mt: 1.5,
      }}
    >
      {behaviorKeys.map((key, index) => (
        <Box
          key={key}
          sx={{
            minWidth: 0,
            p: 1.25,
            borderRadius: "var(--radius-chip)",
            bgcolor: DASHBOARD_INSET_WELL_BG,
            border: "1px solid color-mix(in srgb, var(--border) 16%, transparent)",
          }}
        >
          <Typography
            variant="caption"
            component="div"
            sx={{
              color: "var(--text-tertiary)",
              fontWeight: 500,
              fontSize: "var(--font-size-label)",
              letterSpacing: "0.04em",
              lineHeight: 1.2,
              mb: 0.5,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {t(STRATEGY_BEHAVIOR_DIM_KEYS[index])}
          </Typography>
          <Typography
            component="div"
            sx={{
              color: "var(--text-primary)",
              fontSize: "var(--font-size-caption)",
              fontWeight: 600,
              lineHeight: 1.3,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {t(key)}
          </Typography>
        </Box>
      ))}
    </Box>
  );
}

/** 预算表左栏简称（完整说明见 `labelKey` + `title`） */
const STRATEGY_BUDGET_ABBR_KEY: Record<string, string> = {
  messages_max: "device.systemStatusStrategyBudgetAbbrMessages",
  system_prompt_max: "device.systemStatusStrategyBudgetAbbrSystemPrompt",
  response_body_max: "device.systemStatusStrategyBudgetAbbrResponseBody",
  reconnect_backoff_secs: "device.systemStatusStrategyBudgetAbbrReconnect",
};

function StrategyBudgetRow({
  fields,
  t,
}: {
  fields: RuntimeStrategyBudgetField[];
  t: TFunction;
}) {
  if (fields.length === 0) return null;
  return (
    <Box
      sx={{
        display: "grid",
        gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
        gap: 1,
        mt: 1,
      }}
    >
      {fields.map((item) => {
        const formatted = formatStrategyBudgetValue(item.value, item.valueKind);
        const abbrKey = STRATEGY_BUDGET_ABBR_KEY[item.id];
        const shortLabel = abbrKey ? t(abbrKey) : t(item.labelKey);
        const fullLabel = t(item.labelKey);
        return (
          <Box
            key={item.id}
            title={fullLabel}
            sx={{
              minWidth: 0,
              p: 1.25,
              borderRadius: "var(--radius-chip)",
              bgcolor: DASHBOARD_INSET_WELL_BG,
              border: "1px solid color-mix(in srgb, var(--border) 16%, transparent)",
            }}
          >
            <Typography
              variant="caption"
              component="div"
              sx={{
                color: "var(--text-tertiary)",
                fontWeight: 500,
                fontSize: "var(--font-size-label)",
                letterSpacing: "0.04em",
                lineHeight: 1.2,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                mb: 0.5,
              }}
            >
              {shortLabel}
            </Typography>
            <Box
              sx={{
                display: "flex",
                alignItems: "baseline",
                gap: 0.4,
                minWidth: 0,
              }}
            >
              <Typography
                component="span"
                sx={{
                  color: "var(--text-primary)",
                  fontFamily: "var(--font-mono)",
                  fontWeight: 700,
                  fontSize: "var(--font-size-data-value)",
                  lineHeight: 1.2,
                  minWidth: 0,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {formatted.value}
              </Typography>
              {formatted.unit ? (
                <Typography
                  component="span"
                  variant="caption"
                  sx={{
                    color: "var(--text-tertiary)",
                    fontFamily: "var(--font-mono)",
                    fontWeight: 500,
                    flexShrink: 0,
                    fontSize: "var(--font-size-label)",
                  }}
                >
                  {formatted.unit}
                </Typography>
              ) : null}
            </Box>
          </Box>
        );
      })}
    </Box>
  );
}

function StrategyStatusPanel({
  strategy,
  t,
}: {
  strategy: RuntimeStrategyViewModel;
  t: TFunction;
}) {
  const accent = strategyAccentColor(strategy.intensity);

  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        gap: 1.5,
        alignItems: "stretch",
        height: "100%",
      }}
    >
      {/* Left: pressure gauge */}
      <Box
        sx={{
          flexShrink: 0,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          gap: 1.5,
          p: 2,
          borderRadius: "var(--radius-card)",
          bgcolor: DASHBOARD_INSET_WELL_BG,
          border: `1px solid color-mix(in srgb, ${accent} 18%, var(--border) 14%)`,
          minWidth: 0,
          width: "100%",
          alignSelf: "auto",
        }}
      >
        <CircularGauge
          value={strategy.intensity}
          max={3}
          label={t(strategy.headlineKey)}
          color={accent}
          size={88}
          strokeWidth={8}
        />
        {/* Intensity badge under gauge */}
        <Box
          sx={{
            display: "inline-flex",
            alignItems: "center",
            gap: 0.5,
            px: 1,
            py: 0.4,
            borderRadius: "var(--radius-full)",
            bgcolor: `color-mix(in srgb, ${accent} 12%, transparent)`,
            border: `1px solid color-mix(in srgb, ${accent} 26%, transparent)`,
          }}
        >
          <Box
            sx={{
              width: 6,
              height: 6,
              borderRadius: "50%",
              bgcolor: accent,
              boxShadow: `0 0 5px ${accent}`,
              flexShrink: 0,
            }}
          />
          <Typography
            component="span"
            sx={{
              color: accent,
              fontFamily: "var(--font-mono)",
              fontWeight: 700,
              fontSize: "var(--font-size-label)",
              lineHeight: 1,
              letterSpacing: "0.06em",
            }}
          >
            {strategy.intensity}/3
          </Typography>
        </Box>
      </Box>

      {/* Right: behavior + budget fields */}
      <Box sx={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 0, justifyContent: "center" }}>
        <StrategyBehaviorRow behaviorKeys={strategy.behaviorKeys} t={t} />
        <StrategyBudgetRow fields={strategy.budgetFields} t={t} />
      </Box>
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
          : "1px solid color-mix(in srgb, var(--border) 22%, transparent)",
        boxShadow: "var(--os3d-chip-lift-stack)",
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

function memoryMetricColor(id: string): string {
  if (id === "heap_internal") return "var(--semantic-warning)";
  if (id === "heap_spiram_free") return "var(--semantic-success)";
  return "var(--foreground)";
}

function MemoryMetricGrid({
  metrics,
  t,
}: {
  metrics: HomeMetricField[];
  t: TFunction;
}) {
  return (
    <Box
      sx={{
        display: "grid",
        gridTemplateColumns: { xs: "1fr", sm: "repeat(2, minmax(0, 1fr))" },
        gap: { xs: 0.85, sm: 1 },
        height: "100%",
        alignContent: "start",
      }}
    >
      {metrics.map((item) => (
        <Box
          key={item.id}
          title={t(item.labelKey)}
          sx={{
            minWidth: 0,
            p: { xs: 1.05, sm: 1.1 },
            borderRadius: "var(--radius-chip)",
            bgcolor: DASHBOARD_INSET_WELL_BG,
            border:
              "1px solid color-mix(in srgb, var(--border) 18%, transparent)",
            boxShadow: "var(--os3d-chip-lift-stack)",
          }}
        >
          <Typography
            variant="caption"
            component="div"
            sx={{
              color: "var(--foreground-soft)",
              fontWeight: 600,
              fontSize: "0.65rem",
              lineHeight: 1.15,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              mb: 0.45,
            }}
          >
            {t(item.labelKey)}
          </Typography>
          <Typography
            component="div"
            sx={{
              color: memoryMetricColor(item.id),
              fontFamily: "var(--font-mono)",
              fontWeight: 750,
              fontSize: { xs: "1rem", sm: "1.08rem" },
              lineHeight: 1.1,
              letterSpacing: 0,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {formatBytes(item.value)}
          </Typography>
        </Box>
      ))}
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

function formatDetailFieldValue(field: HomeDetailField, t: TFunction): string | number {
  switch (field.valueKind) {
    case "boolean":
      return field.value === true ? t("common.yes") : t("common.no");
    case "bytes":
      return typeof field.value === "number" ? formatBytes(field.value) : String(field.value);
    case "milliseconds":
      return `${field.value} ms`;
    case "microseconds":
      return `${field.value} us`;
    case "load_average":
      return formatLoadAverage(field.value as [number, number, number]);
    case "translation_key":
      return t(String(field.value));
    case "text":
    case "number":
    default:
      return field.value as string | number;
  }
}

function formatEpochSeconds(value: number | undefined): string {
  if (!value || value <= 0) return "—";
  return new Date(value * 1000).toLocaleString();
}

function DetailFieldGrid({
  fields,
  t,
}: {
  fields: HomeDetailField[];
  t: TFunction;
}) {
  if (fields.length === 0) return null;
  return (
    <Box
      sx={{
        display: "grid",
        gridTemplateColumns: {
          xs: "repeat(2, minmax(0, 1fr))",
          sm: "repeat(auto-fit, minmax(128px, 1fr))",
        },
        gap: DASHBOARD_BLOCK_GAP,
        alignContent: "start",
      }}
    >
      {fields.map((item) => (
        <DigitalCounter
          key={item.id}
          label={t(item.labelKey)}
          value={formatDetailFieldValue(item, t)}
          danger={item.danger}
        />
      ))}
    </Box>
  );
}

function StorageMediaDetailList({
  items,
  t,
}: {
  items: StorageMediaDetailView[];
  t: TFunction;
}) {
  if (items.length === 0) return null;
  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1.5 }}>
      {items.map((item) => (
        <Box
          key={item.id}
          sx={{
            p: 1.5,
            borderRadius: "var(--radius-chip)",
            bgcolor: DASHBOARD_INSET_WELL_BG,
            border: "1px solid color-mix(in srgb, var(--border) 18%, transparent)",
          }}
        >
          <Typography
            variant="subtitle2"
            sx={{
              color: "var(--text-primary)",
              fontWeight: 700,
              mb: 1,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {item.title}
          </Typography>
          <DetailFieldGrid fields={item.fields} t={t} />
        </Box>
      ))}
    </Box>
  );
}

export interface SystemStatusPanelProps {
  healthData: HealthData;
  resourceData: ResourceSnapshotData | null;
  metricsData: MetricsSnapshotData | null;
  systemInfo: SystemInfoData | null;
  runtimeKind: DeviceRuntimeKind;
  t: TFunction;
}

export function SystemStatusPanel({
  healthData,
  resourceData,
  metricsData,
  systemInfo,
  runtimeKind,
  t,
}: SystemStatusPanelProps) {
  const res = resourceData;
  const met = metricsData;
  const memoryMetrics = buildMemoryMetrics(runtimeKind, res);
  const groupedFaults = buildFaultAndRecoveryMetrics(met);
  const strategy = buildRuntimeStrategyView(res);
  const runtimeTelemetry = buildRuntimeTelemetryFields(runtimeKind, res, met);
  const healthDetailFields = [
    ...buildHealthDetailFields(healthData),
    ...buildResourceRiskFields(res),
  ];
  const resourceGovernanceFields = buildResourceGovernanceFields(res);
  const executionTimingFields = buildExecutionTimingFields(met);
  const turnProtocolFields = buildTurnProtocolFields(met);
  const httpStorageStreamFields = buildHttpStorageStreamFields(met);
  const voiceAudioTelemetryFields = buildVoiceAudioTelemetryFields(met);
  const programmableReasoningFields = buildProgrammableReasoningFields(systemInfo);
  const storageMediaDetails = buildStorageMediaDetailFields(systemInfo);

  const storageUsed = res?.storage_used_kb;
  const storageTotal = res?.storage_total_kb;
  const hasStorageUsage =
    typeof storageUsed === "number" &&
    typeof storageTotal === "number" &&
    Number.isFinite(storageUsed) &&
    Number.isFinite(storageTotal) &&
    storageTotal > 0;
  const storageGaugeValue = hasStorageUsage ? Math.max(storageUsed, 0) : 0;
  const storageGaugeMax = hasStorageUsage ? storageTotal : 1;
  const storageSubLabel = hasStorageUsage
    ? `${Math.max(storageUsed, 0)} / ${storageTotal} KB`
    : t("common.na");

  const hasErrors = 
    groupedFaults.faults.some((item) => item.value > 0) ||
    groupedFaults.recovery.some((item) => item.value > 0) ||
    (healthData.last_error && healthData.last_error !== "none");

  return (
    <React.Fragment>
      {/* Storage Gauge (Span 4 cols, 2 rows) */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 4", lg: "span 4" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard title={t("device.systemStatusStorage")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.storage} variant="tile" />}>
          <CircularGauge
            value={storageGaugeValue}
            max={storageGaugeMax}
            label={t("device.systemStatusStorage")}
            subLabel={storageSubLabel}
            centerLabel={hasStorageUsage ? undefined : t("common.na")}
            color="var(--primary)"
          />
        </DashboardCard>
      </Box>

      {/* RAM & Memory (Span 4 cols, 2 rows) */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 4", lg: "span 4" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard title={t("device.systemStatusGroupMemory")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.memory} variant="tile" />}>
          <MemoryMetricGrid metrics={memoryMetrics} t={t} />
        </DashboardCard>
      </Box>

      {healthDetailFields.length > 0 ? (
        <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 4" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
          <DashboardCard title={t("device.systemStatusGroupHealthDetails")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.healthDetails} variant="tile" />}>
            <DetailFieldGrid fields={healthDetailFields} t={t} />
          </DashboardCard>
        </Box>
      ) : null}

      {/* Runtime Strategy */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 4" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard
          title={t("device.systemStatusStrategy")}
          icon={<Os3dIcon src={OS_ICON_DASHBOARD.strategy} variant="tile" />}
        >
          {strategy ? (
            <StrategyStatusPanel strategy={strategy} t={t} />
          ) : (
            <Typography variant="body2" sx={{ color: "var(--text-tertiary)" }}>
              {t("common.na")}
            </Typography>
          )}
        </DashboardCard>
      </Box>

      {resourceGovernanceFields.length > 0 ? (
        <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 4" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
          <DashboardCard title={t("device.systemStatusGroupGovernance")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.governance} variant="tile" />}>
            <DetailFieldGrid fields={resourceGovernanceFields} t={t} />
          </DashboardCard>
        </Box>
      ) : null}

      {/* Traffic & Ops */}
      <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 12" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
        <DashboardCard title={t("device.systemStatusGroupRuntime")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.runtime} variant="tile" />}>
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
          icon={<Os3dIcon src={OS_ICON_DASHBOARD.faults} variant="tile" />}
          sx={
            hasErrors
              ? {
                  border:
                    "1px solid color-mix(in srgb, var(--semantic-danger) 32%, transparent)",
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
            <Box sx={buildStatusNoticeSx("var(--semantic-warning)")}>
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
            <Box sx={buildStatusNoticeSx("var(--semantic-danger)")}>
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

      {executionTimingFields.length > 0 ? (
        <Box sx={{ gridColumn: { xs: "span 4", sm: "span 2", lg: "span 3" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
          <DashboardCard title={t("device.systemStatusGroupExecutionTiming")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.executionTiming} variant="tile" />}>
            <DetailFieldGrid fields={executionTimingFields} t={t} />
          </DashboardCard>
        </Box>
      ) : null}

      {turnProtocolFields.length > 0 ? (
        <Box sx={{ gridColumn: { xs: "span 4", sm: "span 2", lg: "span 3" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
          <DashboardCard title={t("device.systemStatusGroupTurnProtocol")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.turnProtocol} variant="tile" />}>
            <DetailFieldGrid fields={turnProtocolFields} t={t} />
          </DashboardCard>
        </Box>
      ) : null}

      {httpStorageStreamFields.length > 0 ? (
        <Box sx={{ gridColumn: { xs: "span 4", sm: "span 2", lg: "span 3" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
          <DashboardCard title={t("device.systemStatusGroupHttpStorageStream")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.httpStorageStream} variant="tile" />}>
            <DetailFieldGrid fields={httpStorageStreamFields} t={t} />
          </DashboardCard>
        </Box>
      ) : null}

      {voiceAudioTelemetryFields.length > 0 ? (
        <Box sx={{ gridColumn: { xs: "span 4", sm: "span 2", lg: "span 3" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
          <DashboardCard title={t("device.systemStatusGroupVoiceAudio")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.voiceAudio} variant="tile" />}>
            <DetailFieldGrid fields={voiceAudioTelemetryFields} t={t} />
          </DashboardCard>
        </Box>
      ) : null}

      {programmableReasoningFields.length > 0 ? (
        <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 6" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
          <DashboardCard title={t("device.systemStatusGroupProgrammableReasoning")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.programmableReasoning} variant="tile" />}>
            <DetailFieldGrid fields={programmableReasoningFields} t={t} />
          </DashboardCard>
        </Box>
      ) : null}

      {storageMediaDetails.length > 0 ? (
        <Box sx={{ gridColumn: { xs: "span 4", sm: "span 8", lg: "span 6" }, gridRow: { xs: "span 2", lg: "span 2" } }}>
          <DashboardCard title={t("device.systemStatusGroupStorageMediaDetails")} icon={<Os3dIcon src={OS_ICON_DASHBOARD.storageMedia} variant="tile" />}>
            <StorageMediaDetailList items={storageMediaDetails} t={t} />
          </DashboardCard>
        </Box>
      ) : null}
    </React.Fragment>
  );
}
