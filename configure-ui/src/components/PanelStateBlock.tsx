import type { ReactNode } from "react";
import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import { useTranslation } from "react-i18next";
import { LAYOUT_TOKENS } from "../config/themeTokens";
import {
  PANEL_STATE_AREA_SX,
  TEXT_BODY_TERTIARY_SX,
} from "../theme/panelStyles";

/** 与 InlineAlert / 全屏状态蒙层对齐的语义色带。 */
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

const TONE_NOTICE_TINT: Record<PanelStateTone, string> = {
  neutral:
    "color-mix(in srgb, var(--foreground) 5%, transparent)",
  warning:
    "color-mix(in srgb, var(--semantic-warning) 7%, transparent)",
  danger:
    "color-mix(in srgb, var(--semantic-danger) 6%, transparent)",
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
 * 状态区标题行：左插画井 + 标题 + 说明（与全屏状态蒙层同源结构）。
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
        backgroundColor:
          layout === "stack" && tone === "neutral"
            ? "color-mix(in srgb, var(--surface) 74%, var(--card))"
            : TONE_WELL[tone],
        backgroundImage:
          layout === "stack"
            ? "linear-gradient(180deg, color-mix(in srgb, #fff 16%, transparent) 0%, transparent 100%)"
            : undefined,
        boxShadow:
          layout === "stack"
            ? "var(--os3d-control-soft-lift-stack)"
            : "var(--os3d-pedestal-lift-stack)",
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
            ...(layout === "stack" ? { maxWidth: "34ch", mx: "auto" } : {}),
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
  /**
   * `empty`：空列表/缺失内容的静态占位；`notice`：连接前/警告/错误类提示。
   * Omit to derive from tone + size.
   */
  presentation?: "empty" | "notice";
  /** 透传给外层，便于测试或 aria */
  id?: string;
}

/**
 * 区块内「空 / 警告 / 错误」统一版式：柔光体积 + 奶玻璃语义色，不再使用左侧强调线。
 * Use for empty lists, connect-first, and inline danger (not page-level `InlineAlert`).
 */
export function PanelStateBlock({
  tone = "neutral",
  icon,
  title,
  description,
  actions,
  size = "default",
  presentation,
  id,
}: PanelStateBlockProps) {
  const compact = size === "compact";
  const resolvedPresentation =
    presentation ?? (compact ? "notice" : tone === "neutral" ? "empty" : "notice");
  const emptyPresentation = resolvedPresentation === "empty";
  const noticeTint = TONE_NOTICE_TINT[tone];
  const noticeBorder =
    tone === "neutral"
      ? "color-mix(in srgb, var(--primary) 12%, var(--border))"
      : `color-mix(in srgb, ${TONE_BORDER[tone]} 16%, var(--border))`;

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
        px: emptyPresentation ? 2.5 : 2,
        borderRadius: "var(--radius-card)",
        border: emptyPresentation ? "none" : `1px solid ${noticeBorder}`,
        backgroundColor: emptyPresentation
          ? "color-mix(in srgb, var(--surface) 68%, var(--card))"
          : "color-mix(in srgb, var(--card) 78%, transparent)",
        backgroundImage: emptyPresentation
          ? [
              "linear-gradient(180deg, color-mix(in srgb, #fff 14%, transparent) 0%, transparent 48%)",
              "linear-gradient(180deg, color-mix(in srgb, var(--surface) 32%, transparent) 0%, transparent 100%)",
            ].join(", ")
          : [
              "linear-gradient(180deg, color-mix(in srgb, #fff 22%, transparent) 0%, transparent 52%, color-mix(in srgb, #fff 8%, transparent) 100%)",
              `linear-gradient(135deg, ${noticeTint} 0%, transparent 44%, color-mix(in srgb, var(--accent) 4%, transparent) 100%)`,
            ].join(", "),
        boxShadow: emptyPresentation
          ? "var(--os3d-section-module-stack)"
          : [
              "0 18px 36px -32px color-mix(in srgb, var(--foreground) 14%, transparent)",
              `0 12px 28px -28px ${noticeTint}`,
              "inset 0 1px 0 color-mix(in srgb, #fff 52%, transparent)",
            ].join(", "),
        backdropFilter: emptyPresentation
          ? undefined
          : "blur(calc(var(--glass-blur) * 0.55)) saturate(1.04)",
        WebkitBackdropFilter: emptyPresentation
          ? undefined
          : "blur(calc(var(--glass-blur) * 0.55)) saturate(1.04)",
        boxSizing: "border-box",
      }}
    >
      <PanelStateHeroRow
        tone={tone}
        icon={icon}
        title={title}
        description={description}
        size={size}
        layout={emptyPresentation ? "stack" : "row"}
      />
      {actions ? (
        <Box
          sx={{
            mt: 2,
            display: "flex",
            flexWrap: "wrap",
            gap: 1,
            justifyContent: emptyPresentation ? "center" : "flex-start",
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
