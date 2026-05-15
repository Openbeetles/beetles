import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import { TEXT_SUBSECTION_TITLE_SX } from "../../theme/panelStyles";

export type SectionSubTitleAccentTone =
  | "primary"
  | "success"
  | "warning"
  | "danger"
  | "muted";

const ACCENT_TONE_COLOR: Record<SectionSubTitleAccentTone, string> = {
  primary: "var(--primary)",
  success: "var(--semantic-success)",
  warning: "var(--semantic-warning)",
  danger: "var(--semantic-danger)",
  muted: "var(--text-tertiary)",
};

interface SectionSubTitleRowProps {
  title: string;
  accentTone?: SectionSubTitleAccentTone;
  compact?: boolean;
  /**
   * 为 true 时竖线随交叉轴拉高（折叠头整行）；false 时固定高度（非折叠子标题一行）。
   * When true, accent bar stretches on cross-axis (collapsible header); false = one-line height.
   */
  accentStretch?: boolean;
}

/** 表单子区块标题：紧凑状态点 + 标签；折叠 / 非折叠共用，避免两套样式漂移。 */
export function SectionSubTitleRow({
  title,
  accentTone = "primary",
  accentStretch = false,
  compact = false,
}: SectionSubTitleRowProps) {
  const accentColor = ACCENT_TONE_COLOR[accentTone];
  return (
    <Box
      sx={{
        display: "flex",
        alignItems: "center",
        gap: 1.25,
        flex: 1,
        minWidth: 0,
      }}
    >
      <Box
        aria-hidden
        sx={{
          width: accentStretch ? 9 : 8,
          flexShrink: 0,
          height: accentStretch ? 9 : 8,
          borderRadius: "var(--radius-chip)",
          backgroundColor:
            `color-mix(in srgb, ${accentColor} 76%, var(--card))`,
          backgroundImage:
            "linear-gradient(180deg, color-mix(in srgb, var(--primary-fg) 30%, transparent) 0%, transparent 100%)",
          border:
            `1px solid color-mix(in srgb, ${accentColor} 28%, transparent)`,
          boxShadow: [
            "inset 0 1px 0 color-mix(in srgb, var(--primary-fg) 34%, transparent)",
            `0 6px 12px -10px color-mix(in srgb, ${accentColor} 52%, transparent)`,
          ].join(", "),
          alignSelf: "center",
        }}
      />
      <Typography
        variant="subtitle2"
        sx={{
          ...TEXT_SUBSECTION_TITLE_SX,
          ...(compact
            ? {
                fontSize: "var(--font-size-caption)",
                fontWeight: 600,
              }
            : null),
          minWidth: 0,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          letterSpacing: 0,
          textTransform: "none",
        }}
      >
        {title}
      </Typography>
    </Box>
  );
}
