import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Typography from "@mui/material/Typography";
import CheckCircleOutlined from "@mui/icons-material/CheckCircleOutlined";
import ErrorOutlined from "@mui/icons-material/ErrorOutlined";
import HubOutlined from "@mui/icons-material/HubOutlined";
import LinkOffOutlined from "@mui/icons-material/LinkOffOutlined";
import RefreshRounded from "@mui/icons-material/RefreshRounded";
import WarningAmberRounded from "@mui/icons-material/WarningAmberRounded";
import type { TFunction } from "i18next";
import type { ChannelConnectivityItem } from "../api/endpoints/system";
import { PanelStateBlock } from "./PanelStateBlock";
import { SectionLoadProgress } from "./SectionLoadProgress";
import { DASHBOARD_INSET_WELL_BG } from "../theme/panelStyles";

/** 与 SystemStatusPanel 小节面板一致：中性底，不用 --surface 色块 */
const SECTION_PANEL_SX = {
  borderRadius: "var(--radius-control)",
  border: "none",
  bgcolor: "var(--card)",
  boxShadow: "var(--os3d-inset-panel-stack)",
} as const;

/** 与 SystemStatusPanel StatRow 微卡片一致 */
const MICRO_CELL_SX = {
  borderRadius: "var(--radius-chip)",
  border: "none",
  bgcolor: DASHBOARD_INSET_WELL_BG,
  boxShadow: "var(--os3d-chip-lift-stack)",
} as const;

const ROW_DIVIDER = "none";

const CHANNEL_UNAVAIL = "channel connectivity unavailable";

function isI18nKey(msg: string): boolean {
  return /^[a-z]+\.[a-zA-Z0-9.]+$/.test(msg.trim());
}

/** 将 API / 前端错误码转为展示文案 */
function resolveChannelConnectivityError(error: string, t: TFunction): string {
  const trimmed = error.trim();
  if (!trimmed) return t("device.channelConnectivityLoadFailedHint");
  if (trimmed === CHANNEL_UNAVAIL)
    return t("device.channelConnectivityUnavailable");
  if (isI18nKey(trimmed)) return t(trimmed);
  return trimmed;
}

function StatPill({
  label,
  value,
  accent = "var(--foreground)",
}: {
  label: string;
  value: number | string;
  accent?: string;
}) {
  return (
    <Box
      sx={{
        display: "inline-flex",
        alignItems: "baseline",
        gap: 0.65,
        px: 1.125,
        py: 0.5,
        ...MICRO_CELL_SX,
      }}
    >
      <Typography
        component="span"
        sx={{
          fontSize: "var(--font-size-overline)",
          color: "var(--text-tertiary)",
          fontWeight: 400,
          letterSpacing: "var(--letter-spacing-label)",
          lineHeight: 1,
        }}
      >
        {label}
      </Typography>
      <Typography
        component="span"
        sx={{
          fontSize: "var(--font-size-body-sm)",
          fontWeight: 400,
          color: accent,
          fontVariantNumeric: "tabular-nums",
          lineHeight: 1,
        }}
      >
        {value}
      </Typography>
    </Box>
  );
}

function ChannelStatusPill({
  configured,
  ok,
  statusLabel,
}: {
  configured: boolean;
  ok: boolean;
  statusLabel: string;
}) {
  const Icon = configured
    ? ok
      ? CheckCircleOutlined
      : ErrorOutlined
    : LinkOffOutlined;
  const accent = configured
    ? ok
      ? "var(--semantic-success)"
      : "var(--semantic-danger)"
    : "var(--text-tertiary)";

  return (
    <Box
      component="span"
      sx={{
        display: "inline-flex",
        alignItems: "center",
        gap: 0.5,
        flexShrink: 0,
        pl: 0.75,
        pr: 1,
        py: 0.35,
        maxWidth: "min(52%, 240px)",
        ...MICRO_CELL_SX,
        color: accent,
      }}
    >
      <Icon
        sx={{ fontSize: "var(--icon-size-sm)", flexShrink: 0 }}
        aria-hidden
      />
      <Typography
        component="span"
        sx={{
          fontSize: "var(--font-size-caption)",
          fontWeight: 400,
          lineHeight: "var(--line-height-snug)",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {statusLabel}
      </Typography>
    </Box>
  );
}

function ChannelRow({
  label,
  configured,
  ok,
  message,
  t,
  isLast,
}: {
  label: string;
  configured: boolean;
  ok: boolean;
  message?: string | null;
  t: TFunction;
  isLast: boolean;
}) {
  const statusText = configured
    ? ok
      ? t("device.channelOk")
      : t("device.channelFail")
    : t("device.channelNotConfigured");

  return (
    <Box
      sx={{
        px: 1.5,
        py: 1.25,
        borderBottom: isLast ? "none" : ROW_DIVIDER,
      }}
    >
      <Box
        sx={{
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "space-between",
          gap: 1.5,
          minWidth: 0,
        }}
      >
        <Typography
          variant="body2"
          component="span"
          sx={{
            color: "var(--text-tertiary)",
            opacity: 0.92,
            pt: { xs: 0, sm: "0.0625rem" },
            fontSize: "var(--font-size-caption)",
            lineHeight: 1.45,
            letterSpacing: "0.02em",
            minWidth: 0,
            flex: "1 1 auto",
          }}
        >
          {label}
        </Typography>
        <ChannelStatusPill
          configured={configured}
          ok={ok}
          statusLabel={statusText}
        />
      </Box>
      {configured && message?.trim() ? (
        <Box
          sx={{
            mt: 0.875,
            display: "flex",
            gap: 0.85,
            alignItems: "flex-start",
            ...(!ok
              ? {
                  px: 1.125,
                  py: 0.85,
                  borderRadius: "var(--radius-chip)",
                  border:
                    "1px solid color-mix(in srgb, var(--semantic-danger) 14%, var(--border))",
                  bgcolor:
                    "color-mix(in srgb, var(--card) 78%, transparent)",
                  backgroundImage: [
                    "linear-gradient(180deg, color-mix(in srgb, #fff 18%, transparent) 0%, transparent 64%)",
                    "linear-gradient(135deg, color-mix(in srgb, var(--semantic-danger) 5%, transparent) 0%, transparent 42%, color-mix(in srgb, var(--accent) 4%, transparent) 100%)",
                  ].join(", "),
                  boxShadow: [
                    "0 14px 26px -24px color-mix(in srgb, var(--semantic-danger) 18%, transparent)",
                    "inset 0 1px 0 color-mix(in srgb, #fff 52%, transparent)",
                  ].join(", "),
                  backdropFilter:
                    "blur(calc(var(--glass-blur) * 0.45)) saturate(1.03)",
                  WebkitBackdropFilter:
                    "blur(calc(var(--glass-blur) * 0.45)) saturate(1.03)",
                }
              : {
                  py: 0.125,
                }),
          }}
        >
          {!ok ? (
            <ErrorOutlined
              sx={{
                fontSize: "1.05rem",
                color: "var(--semantic-danger)",
                mt: "0.1rem",
                flexShrink: 0,
              }}
              aria-hidden
            />
          ) : null}
          <Typography
            variant="caption"
            component="span"
            sx={{
              flex: 1,
              minWidth: 0,
              color: !ok
                ? "color-mix(in srgb, var(--semantic-danger) 22%, var(--foreground))"
                : "var(--foreground-soft)",
              fontFamily: "var(--font-mono)",
              fontWeight: !ok ? 500 : 400,
              fontSize: "var(--font-size-caption)",
              lineHeight: 1.55,
              wordBreak: "break-word",
            }}
          >
            {isI18nKey(message) ? t(message) : message}
          </Typography>
        </Box>
      ) : null}
    </Box>
  );
}

function LoadFailedState({
  title,
  detail,
  onRetry,
  retryLabel,
}: {
  title: string;
  detail: string;
  onRetry: () => void;
  retryLabel: string;
}) {
  return (
    <PanelStateBlock
      tone="danger"
      presentation="notice"
      size="compact"
      icon={<HubOutlined sx={{ fontSize: "var(--icon-size-lg)" }} aria-hidden />}
      title={title}
      description={detail}
      actions={
        <Button
          size="small"
          variant="outlined"
          startIcon={<RefreshRounded sx={{ fontSize: "var(--icon-size-sm)" }} />}
          onClick={onRetry}
          sx={{
            alignSelf: "flex-start",
            borderRadius: "var(--radius-control)",
            fontWeight: 400,
          }}
        >
          {retryLabel}
        </Button>
      }
    />
  );
}

export interface ChannelConnectivityPanelProps {
  channels: ChannelConnectivityItem[];
  loading: boolean;
  error: string;
  onRetry: () => void;
  channelLabel: (id: string) => string;
  t: TFunction;
}

/**
 * 设备页「通道连通性」内容区：摘要统计、列表卡片、加载与错误态。
 */
export function ChannelConnectivityPanel({
  channels,
  loading,
  error,
  onRetry,
  channelLabel,
  t,
}: ChannelConnectivityPanelProps) {
  const hasList = channels.length > 0;
  const showBlockingError = Boolean(error?.trim() && !hasList && !loading);
  /** 仅有历史列表时的刷新失败：用警告条 + 重试，不遮挡列表 */
  const showStaleHint = Boolean(error?.trim() && hasList && !loading);

  const configuredCount = channels.filter((c) => c.configured).length;
  const okCount = channels.filter((c) => c.configured && c.ok).length;
  const issueCount = channels.filter((c) => c.configured && !c.ok).length;

  const errorDisplay = error.trim()
    ? resolveChannelConnectivityError(error, t)
    : "";

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1.75 }}>
      <SectionLoadProgress
        loading={loading}
        idleHint={!hasList ? t("device.channelConnectivityLoading") : undefined}
      />

      {showBlockingError && (
        <LoadFailedState
          title={t("device.channelConnectivityLoadFailedTitle")}
          detail={errorDisplay}
          onRetry={onRetry}
          retryLabel={t("device.channelRefresh")}
        />
      )}

      {hasList && (
        <>
          <Box
            sx={{
              display: "flex",
              flexWrap: "wrap",
              gap: 1,
              alignItems: "center",
            }}
          >
            <StatPill
              label={t("device.channelStatTotal")}
              value={channels.length}
            />
            <StatPill
              label={t("device.channelStatConfigured")}
              value={configuredCount}
            />
            <StatPill
              label={t("device.channelStatOk")}
              value={okCount}
              accent="var(--semantic-success)"
            />
            {issueCount > 0 && (
              <StatPill
                label={t("device.channelStatIssues")}
                value={issueCount}
                accent="var(--semantic-danger)"
              />
            )}
          </Box>

          {showStaleHint && (
            <PanelStateBlock
              tone="warning"
              presentation="notice"
              size="compact"
              icon={
                <WarningAmberRounded
                  sx={{ fontSize: "var(--icon-size-md)" }}
                  aria-hidden
                />
              }
              title={t("device.channelConnectivityStale")}
              actions={
                <Button
                  size="small"
                  variant="outlined"
                  color="warning"
                  onClick={onRetry}
                  sx={{
                    minWidth: 0,
                    borderRadius: "var(--radius-control)",
                    fontWeight: 600,
                  }}
                >
                  {t("common.retry")}
                </Button>
              }
            />
          )}

          <Box
            sx={{
              overflow: "hidden",
              opacity: loading ? 0.72 : 1,
              transition:
                "opacity var(--transition-duration) var(--ease-emphasized)",
              pointerEvents: loading ? "none" : "auto",
              ...SECTION_PANEL_SX,
            }}
          >
            {channels.map((ch, i) => (
              <ChannelRow
                key={ch.id}
                label={channelLabel(ch.id)}
                configured={ch.configured}
                ok={ch.ok}
                message={ch.message}
                t={t}
                isLast={i === channels.length - 1}
              />
            ))}
          </Box>

          {t("device.channelConnectivityNote")?.trim() ? (
            <Typography
              variant="caption"
              sx={{
                color: "var(--text-tertiary)",
                display: "block",
                lineHeight: "var(--line-height-relaxed)",
              }}
            >
              {t("device.channelConnectivityNote")}
            </Typography>
          ) : null}
        </>
      )}
    </Box>
  );
}
