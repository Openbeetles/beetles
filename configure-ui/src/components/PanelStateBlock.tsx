import type { ReactNode } from "react";
import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import { useTranslation } from "react-i18next";
import { LAYOUT_TOKENS } from "../config/themeTokens";
import { PANEL_STATE_AREA_SX, TEXT_BODY_TERTIARY_SX } from "../theme/panelStyles";

/** 与 InlineAlert / DisconnectedCacheOverlay 对齐的语义色带。 */
export type PanelStateTone = "neutral" | "warning" | "danger";

const TONE_WELL: Record<PanelStateTone, string> = {
  neutral:
    "color-mix(in srgb, var(--foreground) 6%, transparent)",
  warning:
    "color-mix(in srgb, var(--semantic-warning) 16%, transparent)",
  danger:
    "color-mix(in srgb, var(--semantic-danger) 14%, transparent)",
};

const TONE_BORDER: Record<PanelStateTone, string> = {
  neutral:
    "color-mix(in srgb, var(--primary) 40%, transparent)",
  warning: "var(--semantic-warning)",
  danger: "var(--semantic-danger)",
};

/** `row`：左图右文（面板空态）；`stack`：图标置顶居中 + 文案居中（与 `ConfirmDialog` 一致）。 */
export type PanelStateHeroLayout = "row" | "stack";

export interface PanelStateHeroRowProps {
  tone: PanelStateTone;
  /** 通常为 `Os3dIcon`，与导航/仪表盘同权重 */
  icon: ReactNode;
  title: string;
  description?: string;
  /** 默认插画井字略大；紧凑用于条带提示 */
  size?: "default" | "compact";
  layout?: PanelStateHeroLayout;
}

/**
 * 状态区标题行：左插画井 + 标题 + 说明（与 `DisconnectedCacheOverlay` 同源结构）。
 * Shared hero row for section empty states and full-screen overlays.
 */
export function PanelStateHeroRow({
  tone,
  icon,
  title,
  description,
  size = "default",
  layout = "row",
}: PanelStateHeroRowProps) {
  const well =
    size === "compact"
      ? LAYOUT_TOKENS.panelStateIconWellCompactPx
      : LAYOUT_TOKENS.panelStateIconWellPx;
  const inner =
    size === "compact"
      ? LAYOUT_TOKENS.panelStateIconInnerCompactPx
      : LAYOUT_TOKENS.panelStateIconInnerPx;

  const iconWell = (
    <Box
      sx={{
        width: well,
        height: well,
        borderRadius: "var(--radius-chip)",
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        backgroundColor: TONE_WELL[tone],
        boxShadow: "var(--os3d-pedestal-lift-stack)",
      }}
    >
      <Box sx={{ width: inner, height: inner }}>{icon}</Box>
    </Box>
  );

  const titleDescGap = layout === "stack" ? 1.75 : 0.75;

  const textBlock = (
    <Box
      sx={{
        minWidth: 0,
        ...(layout === "row"
          ? { flex: 1, pt: 0.25 }
          : { width: "100%", textAlign: "center" }),
      }}
    >
      <Typography
        component="h2"
        sx={{
          fontFamily: "var(--font-sans)",
          fontSize: "var(--font-size-body-lg)",
          fontWeight: 700,
          letterSpacing: "var(--letter-spacing-tight)",
          lineHeight: layout === "stack" ? 1.45 : "var(--line-height-snug)",
          color: "var(--foreground)",
          mb: description?.trim() ? titleDescGap : 0,
        }}
      >
        {title}
      </Typography>
      {description?.trim() ? (
        <Typography
          sx={{
            fontSize: "var(--font-size-body-sm)",
            lineHeight: "var(--line-height-relaxed)",
            color: "var(--text-tertiary)",
            whiteSpace: "pre-line",
          }}
        >
          {description}
        </Typography>
      ) : null}
    </Box>
  );

  if (layout === "stack") {
    return (
      <Stack direction="column" alignItems="center" spacing={2.5}>
        {iconWell}
        {textBlock}
      </Stack>
    );
  }

  return (
    <Stack direction="row" alignItems="flex-start" spacing={2}>
      {iconWell}
      {textBlock}
    </Stack>
  );
}

export interface PanelStateBlockProps {
  tone?: PanelStateTone;
  icon: ReactNode;
  title: string;
  description?: string;
  /** 次要操作（空列表引导按钮等） */
  actions?: ReactNode;
  size?: "default" | "compact";
  /** 透传给外层，便于测试或 aria */
  id?: string;
}

/**
 * 区块内「空 / 警告 / 错误」统一版式：细描边卡 + 左侧语义线 + 3D 插画与标题层级。
 * Use for empty lists, connect-first, and inline danger (not page-level `InlineAlert`).
 */
export function PanelStateBlock({
  tone = "neutral",
  icon,
  title,
  description,
  actions,
  size = "default",
  id,
}: PanelStateBlockProps) {
  const compact = size === "compact";
  return (
    <Box
      id={id}
      role="status"
      aria-live="polite"
      sx={{
        ...PANEL_STATE_AREA_SX,
        ...(compact
          ? { minHeight: "auto", py: 2 }
          : { py: 2.5 }),
        px: 2,
        borderRadius: "var(--radius-card)",
        border: "1px solid var(--form-outline-rest)",
        borderLeft: `${LAYOUT_TOKENS.accentLineWidth}px solid ${TONE_BORDER[tone]}`,
        backgroundColor: "var(--input-idle-well)",
        boxSizing: "border-box",
      }}
    >
      <PanelStateHeroRow
        tone={tone}
        icon={icon}
        title={title}
        description={description}
        size={size}
      />
      {actions ? (
        <Box
          sx={{
            mt: 2,
            display: "flex",
            flexWrap: "wrap",
            gap: 1,
            justifyContent: "flex-start",
          }}
        >
          {actions}
        </Box>
      ) : null}
    </Box>
  );
}

/**
 * 骨架屏外统一加一句「加载中」与 `aria-busy`，与空态标题同级降噪。
 * Wrap skeletons so loading reads consistently across config pages.
 */
export function PanelStateLoading({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  return (
    <Box
      aria-busy="true"
      aria-live="polite"
      role="status"
      sx={{ display: "flex", flexDirection: "column", gap: 2, width: "100%" }}
    >
      <Typography
        variant="body2"
        component="p"
        sx={{
          ...TEXT_BODY_TERTIARY_SX,
          m: 0,
        }}
      >
        {t("common.loading")}
      </Typography>
      {children}
    </Box>
  );
}
