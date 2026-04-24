import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import { TEXT_SUBSECTION_TITLE_SX } from "../../theme/panelStyles";

interface SectionSubTitleRowProps {
  title: string;
  /**
   * 为 true 时竖线随交叉轴拉高（折叠头整行）；false 时固定高度（非折叠子标题一行）。
   * When true, accent bar stretches on cross-axis (collapsible header); false = one-line height.
   */
  accentStretch?: boolean;
}

/** 表单子区块标题：紧凑状态点 + 标签；折叠 / 非折叠共用，避免两套样式漂移。 */
export function SectionSubTitleRow({
  title,
  accentStretch = false,
}: SectionSubTitleRowProps) {
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
            "color-mix(in srgb, var(--primary) 68%, var(--accent))",
          backgroundImage:
            "linear-gradient(180deg, color-mix(in srgb, var(--primary-fg) 28%, transparent) 0%, transparent 100%)",
          border:
            "1px solid color-mix(in srgb, var(--primary) 18%, transparent)",
          boxShadow: [
            "inset 0 1px 0 color-mix(in srgb, var(--primary-fg) 34%, transparent)",
            "0 6px 12px -10px color-mix(in srgb, var(--primary) 48%, transparent)",
          ].join(", "),
          alignSelf: "center",
        }}
      />
      <Typography
        variant="subtitle2"
        sx={{
          ...TEXT_SUBSECTION_TITLE_SX,
          letterSpacing: "0.01em",
          textTransform: "none",
        }}
      >
        {title}
      </Typography>
    </Box>
  );
}
