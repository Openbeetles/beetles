import type { TFunction } from "i18next";
import Box from "@mui/material/Box";
import LinearProgress from "@mui/material/LinearProgress";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import MonitorHeartOutlined from "@mui/icons-material/MonitorHeartOutlined";
import RouterOutlined from "@mui/icons-material/RouterOutlined";
import SpeedOutlined from "@mui/icons-material/SpeedOutlined";
import type { HealthData } from "../api/endpoints/system";

/** 两列指标网格：与各小节内布局一致。 */
const STAT_GRID_SX = {
  display: "grid",
  gridTemplateColumns: { xs: "1fr", sm: "repeat(2, minmax(0, 1fr))" },
  columnGap: 1.75,
  rowGap: 1.125,
  width: "100%",
  alignItems: "stretch",
} as const;

const SECTION_PANEL_SX = {
  borderRadius: "var(--radius-control)",
  border: "none",
  bgcolor: "var(--card)",
  boxShadow: "none",
  px: { xs: 1.75, sm: 2.25 },
  py: 1.5,
  overflow: "hidden",
} as const;

function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value < 0) return String(value);
  if (value < 1024) return `${value} B`;
  const kb = value / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KB`;
  const mb = kb / 1024;
  return `${mb.toFixed(1)} MB`;
}

function formatLastActiveAt(
  epochSecs: number | undefined,
  t: TFunction,
): string {
  if (epochSecs == null || epochSecs === 0) return t("common.na");
  try {
    return new Date(epochSecs * 1000).toLocaleString();
  } catch {
    return String(epochSecs);
  }
}

function wifiStaLabel(wifi: string | undefined, t: TFunction): string {
  if (wifi === "connected") return t("device.wifiStaConnected");
  if (wifi === "disconnected") return t("device.wifiStaDisconnected");
  return wifi ?? t("common.na");
}

function yesNo(v: boolean | undefined, t: TFunction): string {
  if (v === undefined) return t("common.na");
  return v ? t("common.yes") : t("common.no");
}

function lastErrorDisplay(s: string | undefined, t: TFunction): string {
  if (s == null || s === "" || s === "none") return t("common.na");
  return s;
}

function pressureColor(pressure?: string): string {
  switch (pressure) {
    case "Normal":
      return "var(--semantic-success)";
    case "Cautious":
      return "var(--semantic-warning)";
    case "Critical":
      return "var(--semantic-danger)";
    default:
      return "var(--text-primary)";
  }
}

/** 标签与数值：格内微卡片，CSS Grid 对齐。 */
function StatRow({
  label,
  value,
  valueColor = "var(--text-primary)",
  wide,
  mono = true,
}: {
  label: string;
  value: string;
  valueColor?: string;
  wide?: boolean;
  mono?: boolean;
}) {
  return (
    <Box
      sx={{
        gridColumn: wide ? "1 / -1" : undefined,
        display: "grid",
        gridTemplateColumns: {
          xs: "1fr",
          sm: "minmax(7.5rem, 36%) minmax(0, 1fr)",
        },
        columnGap: { xs: 0.75, sm: 1.5 },
        rowGap: { xs: 0.5, sm: 0 },
        alignItems: { xs: "start", sm: "center" },
        borderRadius: "var(--radius-chip)",
        border: "none",
        bgcolor: "color-mix(in srgb, var(--foreground) 2%, transparent)",
        px: { xs: 1.125, sm: 1.35 },
        py: { xs: 0.875, sm: 1 },
        minWidth: 0,
        transition:
          "border-color var(--transition-duration) ease, background-color var(--transition-duration) ease, box-shadow var(--transition-duration) var(--ease-out-smooth)",
        "&:hover": {
          borderColor:
            "color-mix(in srgb, var(--primary) 14%, var(--border-subtle))",
          boxShadow: "none",
        },
      }}
    >
      <Typography
        variant="body2"
        component="span"
        sx={{
          color: "var(--muted)",
          opacity: 0.92,
          pt: { xs: 0, sm: "0.0625rem" },
          fontSize: "var(--font-size-caption)",
          lineHeight: 1.45,
          letterSpacing: "0.02em",
        }}
      >
        {label}
      </Typography>
      <Typography
        variant="body2"
        component="span"
        sx={{
          fontFamily: mono ? "var(--font-mono)" : "inherit",
          fontSize: mono
            ? "var(--font-size-data-value)"
            : "var(--font-size-body-sm)",
          color:
            valueColor === "var(--text-primary)"
              ? valueColor
              : `color-mix(in srgb, ${valueColor} 87%, var(--foreground))`,
          wordBreak: "break-word",
          lineHeight: 1.5,
          textAlign: "right",
          width: "100%",
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {value}
      </Typography>
    </Box>
  );
}

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <Stack spacing={1.5} sx={{ width: "100%" }}>
      <Box
        sx={{
          display: "flex",
          alignItems: "center",
          gap: 1.125,
          pl: 0.125,
          minHeight: 20,
        }}
      >
        <Box
          aria-hidden
          sx={{
            width: "var(--accent-line-width)",
            height: 16,
            alignSelf: "center",
            borderRadius: 1,
            bgcolor: "var(--primary)",
            opacity: 0.28,
            flexShrink: 0,
          }}
        />
        <Typography
          variant="overline"
          component="h3"
          sx={{
            color: "var(--muted)",
            letterSpacing: "0.1em",
            lineHeight: 1.25,
            fontSize: "var(--font-size-overline)",
            opacity: 0.9,
          }}
        >
          {title}
        </Typography>
      </Box>
      <Box sx={SECTION_PANEL_SX}>
        <Stack spacing={1.5} sx={{ width: "100%" }}>
          {children}
        </Stack>
      </Box>
    </Stack>
  );
}

function SummaryCard({
  icon,
  label,
  value,
  valueColor,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  valueColor?: string;
}) {
  return (
    <Box
      sx={{
        borderRadius: "var(--radius-control)",
        border: "none",
        bgcolor: "var(--card)",
        p: 1.5,
        minHeight: 88,
        display: "grid",
        gridTemplateRows: "auto 1fr",
        gap: 0,
        alignContent: "stretch",
        boxShadow: "none",
        transition:
          "border-color var(--transition-duration) ease, box-shadow var(--transition-duration) var(--ease-out-smooth)",
        "&:hover": {
          borderColor:
            "color-mix(in srgb, var(--primary) 12%, var(--border-subtle))",
          boxShadow: "none",
        },
      }}
    >
      <Stack
        direction="row"
        alignItems="center"
        spacing={1}
        sx={{ minHeight: 36 }}
      >
        <Box
          sx={{
            color: "var(--primary)",
            display: "grid",
            placeItems: "center",
            width: 36,
            height: 36,
            borderRadius: "var(--radius-chip)",
            bgcolor: "color-mix(in srgb, var(--primary) 6%, transparent)",
            flexShrink: 0,
          }}
        >
          {icon}
        </Box>
        <Typography
          variant="caption"
          sx={{
            color: "var(--muted)",
            letterSpacing: "0.04em",
            fontSize: "var(--font-size-caption)",
            lineHeight: 1.35,
            opacity: 0.92,
          }}
        >
          {label}
        </Typography>
      </Stack>
      <Box
        sx={{
          mt: 1.25,
          pt: 1.25,
          borderTop: "none",
        }}
      >
        <Typography
          sx={{
            color:
              valueColor != null
                ? `color-mix(in srgb, ${String(valueColor)} 88%, var(--foreground))`
                : "var(--foreground)",
            fontFamily: "var(--font-mono)",
            lineHeight: 1.4,
            fontSize: "var(--font-size-data-value)",
            textAlign: "right",
            width: "100%",
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {value}
        </Typography>
      </Box>
    </Box>
  );
}

function SystemStatusLastError({
  lastError,
  t,
}: {
  lastError: string | undefined;
  t: TFunction;
}) {
  const text = lastErrorDisplay(lastError, t);
  const isEmpty = text === t("common.na");
  return (
    <Box
      sx={{
        display: "block",
        width: "100%",
        mt: 0.5,
        p: 1.75,
        borderRadius: "var(--radius-control)",
        border: "none",
        borderLeftWidth: "var(--accent-line-width)",
        borderLeftColor: isEmpty
          ? "color-mix(in srgb, var(--border-subtle) 85%, transparent)"
          : "color-mix(in srgb, var(--semantic-danger) 72%, var(--border-subtle))",
        bgcolor: isEmpty
          ? "color-mix(in srgb, var(--border) 2.5%, var(--card))"
          : "color-mix(in srgb, var(--semantic-danger) 4%, var(--card))",
        boxShadow: "none",
      }}
    >
      <Typography
        variant="caption"
        component="div"
        sx={{
          color: "var(--muted)",
          letterSpacing: "0.08em",
          fontSize: "var(--font-size-caption)",
          opacity: 0.9,
        }}
      >
        {t("device.systemStatusLastError")}
      </Typography>
      <Typography
        variant="body2"
        component="div"
        sx={{
          mt: 1.25,
          pt: 1.25,
          borderTop: "none",
          fontFamily: "var(--font-mono)",
          fontSize: "var(--font-size-data-value)",
          color: isEmpty
            ? "var(--muted)"
            : "color-mix(in srgb, var(--semantic-danger) 88%, var(--foreground))",
          wordBreak: "break-word",
          lineHeight: 1.55,
          textAlign: "right",
        }}
      >
        {text}
      </Typography>
    </Box>
  );
}

export interface SystemStatusPanelProps {
  healthData: HealthData;
  t: TFunction;
}

export function SystemStatusPanel({ healthData, t }: SystemStatusPanelProps) {
  const res = healthData.resource;
  const met = healthData.metrics;
  const pressure = res?.pressure;
  const wifiOk = healthData.wifi === "connected";

  const storageUsed = res?.storage_used_kb;
  const storageTotal = res?.storage_total_kb;
  const storagePct =
    storageUsed != null && storageTotal != null && storageTotal > 0
      ? Math.min(100, (storageUsed / storageTotal) * 100)
      : null;

  const hasDetailedErrors =
    (met?.errors_agent_router ?? 0) > 0 ||
    (met?.errors_agent_context ?? 0) > 0 ||
    (met?.errors_tool_execute ?? 0) > 0 ||
    (met?.errors_llm_request ?? 0) > 0 ||
    (met?.errors_llm_parse ?? 0) > 0 ||
    (met?.errors_channel_dispatch ?? 0) > 0 ||
    (met?.errors_session_append ?? 0) > 0 ||
    (met?.errors_other ?? 0) > 0;

  return (
    <Stack spacing={3} sx={{ width: "100%" }}>
      {/* 概览：三卡 */}
      <Box
        sx={{
          display: "grid",
          gridTemplateColumns: { xs: "1fr", sm: "repeat(3, minmax(0, 1fr))" },
          gap: { xs: 1.5, sm: 1.75 },
        }}
      >
        <SummaryCard
          icon={<RouterOutlined sx={{ fontSize: "var(--icon-size-md)" }} />}
          label={t("device.systemStatusWifiSta")}
          value={wifiStaLabel(healthData.wifi, t)}
          valueColor={
            wifiOk ? "var(--semantic-success)" : "var(--semantic-warning)"
          }
        />
        <SummaryCard
          icon={
            <MonitorHeartOutlined sx={{ fontSize: "var(--icon-size-md)" }} />
          }
          label={t("device.systemStatusDisplayAvailable")}
          value={yesNo(healthData.display?.available, t)}
          valueColor={
            healthData.display?.available === true
              ? "var(--semantic-success)"
              : healthData.display?.available === false
                ? "var(--semantic-warning)"
                : "var(--muted)"
          }
        />
        <SummaryCard
          icon={<SpeedOutlined sx={{ fontSize: "var(--icon-size-md)" }} />}
          label={t("device.systemStatusPressure")}
          value={pressure ?? t("common.na")}
          valueColor={pressureColor(pressure)}
        />
      </Box>

      <Section title={t("device.systemStatusGroupResource")}>
        <Box sx={STAT_GRID_SX}>
          {res?.heap_free_internal != null && (
            <StatRow
              label={t("device.systemStatusHeapInternal")}
              value={formatBytes(res.heap_free_internal)}
            />
          )}
          {res?.heap_free_spiram != null && (
            <StatRow
              label={t("device.systemStatusHeapSpiram")}
              value={formatBytes(res.heap_free_spiram)}
            />
          )}
          {res?.heap_largest_block_internal != null && (
            <StatRow
              label={t("device.systemStatusHeapLargest")}
              value={formatBytes(res.heap_largest_block_internal)}
            />
          )}
          {res?.active_http_count != null && (
            <StatRow
              label={t("device.systemStatusActiveHttp")}
              value={String(res.active_http_count)}
            />
          )}
          {res?.inbound_depth != null && (
            <StatRow
              label={t("device.systemStatusInboundDepth")}
              value={String(res.inbound_depth)}
            />
          )}
          {res?.outbound_depth != null && (
            <StatRow
              label={t("device.systemStatusOutboundDepth")}
              value={String(res.outbound_depth)}
            />
          )}
          {res?.session_count != null && (
            <StatRow
              label={t("device.systemStatusSessionCount")}
              value={String(res.session_count)}
            />
          )}
        </Box>
        {storageUsed != null && storageTotal != null && (
          <Box
            sx={{
              width: "100%",
              mt: 1.25,
              pt: 1.5,
              borderTop:
                "1px solid color-mix(in srgb, var(--border-subtle) 62%, transparent)",
            }}
          >
            <StatRow
              wide
              label={t("device.systemStatusStorage")}
              value={`${storageUsed} / ${storageTotal} KB`}
            />
            {storagePct != null && (
              <Box sx={{ width: "100%", pt: 0.5, pb: 0.25 }}>
                <LinearProgress
                  variant="determinate"
                  value={storagePct}
                  aria-label={t("device.systemStatusStorage")}
                  sx={{
                    height: 9,
                    borderRadius: "var(--radius-chip)",
                    bgcolor:
                      "color-mix(in srgb, var(--border) 22%, transparent)",
                    overflow: "hidden",
                    "& .MuiLinearProgress-bar": {
                      borderRadius: "var(--radius-chip)",
                      bgcolor:
                        "color-mix(in srgb, var(--primary) 78%, var(--muted))",
                      transition: "transform 0.45s var(--ease-out-smooth)",
                    },
                  }}
                />
                <Typography
                  variant="caption"
                  sx={{
                    color: "var(--muted)",
                    mt: 0.75,
                    display: "block",
                    textAlign: "right",
                  }}
                >
                  {t("device.systemStatusStorageBarHint", {
                    pct: storagePct.toFixed(0),
                  })}
                </Typography>
              </Box>
            )}
          </Box>
        )}
      </Section>

      <Section title={t("device.systemStatusGroupTraffic")}>
        <Box sx={STAT_GRID_SX}>
          {met?.messages_in != null && (
            <StatRow
              label={t("device.systemStatusMessagesIn")}
              value={String(met.messages_in)}
            />
          )}
          {met?.messages_out != null && (
            <StatRow
              label={t("device.systemStatusMessagesOut")}
              value={String(met.messages_out)}
            />
          )}
          {met?.llm_last_ms != null && (
            <StatRow
              label={t("device.systemStatusLlmLastMs")}
              value={String(met.llm_last_ms)}
            />
          )}
          {met?.last_active_epoch_secs != null && (
            <StatRow
              label={t("device.systemStatusLastActiveAt")}
              value={formatLastActiveAt(met.last_active_epoch_secs, t)}
            />
          )}
        </Box>
      </Section>

      <Section title={t("device.systemStatusGroupOps")}>
        <Box sx={STAT_GRID_SX}>
          {met?.llm_calls != null && (
            <StatRow
              label={t("device.systemStatusLlmCalls")}
              value={String(met.llm_calls)}
            />
          )}
          {met?.tool_calls != null && (
            <StatRow
              label={t("device.systemStatusToolCalls")}
              value={String(met.tool_calls)}
            />
          )}
          {met?.tool_errors != null && (
            <StatRow
              label={t("device.systemStatusToolErrors")}
              value={String(met.tool_errors)}
              valueColor={
                met.tool_errors > 0
                  ? "var(--semantic-warning)"
                  : "var(--text-primary)"
              }
            />
          )}
          {met?.dispatch_send_ok != null && (
            <StatRow
              label={t("device.systemStatusDispatchOk")}
              value={String(met.dispatch_send_ok)}
            />
          )}
          {met?.dispatch_send_fail != null && (
            <StatRow
              label={t("device.systemStatusDispatchFail")}
              value={String(met.dispatch_send_fail)}
              valueColor={
                met.dispatch_send_fail > 0
                  ? "var(--semantic-danger)"
                  : "var(--text-primary)"
              }
            />
          )}
          {met?.llm_errors != null && (
            <StatRow
              label={t("device.systemStatusLlmErrors")}
              value={String(met.llm_errors)}
              valueColor={
                met.llm_errors > 0
                  ? "var(--semantic-warning)"
                  : "var(--text-primary)"
              }
            />
          )}
          {met?.errors_agent_chat != null && (
            <StatRow
              label={t("device.systemStatusChatErrors")}
              value={String(met.errors_agent_chat)}
              valueColor={
                met.errors_agent_chat > 0
                  ? "var(--semantic-danger)"
                  : "var(--text-primary)"
              }
            />
          )}
          {met?.wdt_feeds != null && (
            <StatRow
              label={t("device.systemStatusWdtFeeds")}
              value={String(met.wdt_feeds)}
            />
          )}
        </Box>
      </Section>

      <Section title={t("device.systemStatusGroupWifi")}>
        <Box sx={STAT_GRID_SX}>
          {met?.wifi_reconnect_total != null && (
            <StatRow
              label={t("device.systemStatusWifiReconnect")}
              value={String(met.wifi_reconnect_total)}
            />
          )}
          {met?.wifi_ap_restart_total != null && (
            <StatRow
              label={t("device.systemStatusWifiApRestart")}
              value={String(met.wifi_ap_restart_total)}
            />
          )}
          {met?.wifi_last_failure_stage != null &&
            met.wifi_last_failure_stage.trim() !== "" && (
              <StatRow
                label={t("device.systemStatusWifiLastFail")}
                value={met.wifi_last_failure_stage}
              />
            )}
        </Box>
      </Section>

      {hasDetailedErrors ? (
        <Section title={t("device.systemStatusGroupErrors")}>
          <Box sx={STAT_GRID_SX}>
            {met?.errors_agent_router != null &&
              met.errors_agent_router > 0 && (
                <StatRow
                  label={t("device.systemStatusErrRouter")}
                  value={String(met.errors_agent_router)}
                  valueColor="var(--semantic-danger)"
                />
              )}
            {met?.errors_agent_context != null &&
              met.errors_agent_context > 0 && (
                <StatRow
                  label={t("device.systemStatusErrContext")}
                  value={String(met.errors_agent_context)}
                  valueColor="var(--semantic-danger)"
                />
              )}
            {met?.errors_tool_execute != null &&
              met.errors_tool_execute > 0 && (
                <StatRow
                  label={t("device.systemStatusErrToolExec")}
                  value={String(met.errors_tool_execute)}
                  valueColor="var(--semantic-danger)"
                />
              )}
            {met?.errors_llm_request != null && met.errors_llm_request > 0 && (
              <StatRow
                label={t("device.systemStatusErrLlmReq")}
                value={String(met.errors_llm_request)}
                valueColor="var(--semantic-danger)"
              />
            )}
            {met?.errors_llm_parse != null && met.errors_llm_parse > 0 && (
              <StatRow
                label={t("device.systemStatusErrLlmParse")}
                value={String(met.errors_llm_parse)}
                valueColor="var(--semantic-danger)"
              />
            )}
            {met?.errors_channel_dispatch != null &&
              met.errors_channel_dispatch > 0 && (
                <StatRow
                  label={t("device.systemStatusErrChDispatch")}
                  value={String(met.errors_channel_dispatch)}
                  valueColor="var(--semantic-danger)"
                />
              )}
            {met?.errors_session_append != null &&
              met.errors_session_append > 0 && (
                <StatRow
                  label={t("device.systemStatusErrSession")}
                  value={String(met.errors_session_append)}
                  valueColor="var(--semantic-danger)"
                />
              )}
            {met?.errors_other != null && met.errors_other > 0 && (
              <StatRow
                label={t("device.systemStatusErrOther")}
                value={String(met.errors_other)}
                valueColor="var(--semantic-danger)"
              />
            )}
          </Box>
        </Section>
      ) : null}

      <SystemStatusLastError lastError={healthData.last_error} t={t} />
    </Stack>
  );
}
