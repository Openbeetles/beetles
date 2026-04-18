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

/** 表单子区块标题：主色竖线 + 标签；折叠 / 非折叠共用，避免两套样式漂移。 */
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
          width: accentStretch ? 8 : 7,
          flexShrink: 0,
          height: accentStretch ? 18 : 14,
          borderRadius: 999,
          backgroundColor:
            "color-mix(in srgb, var(--primary) 16%, var(--surface))",
          backgroundImage:
            "linear-gradient(180deg, color-mix(in srgb, #fff 42%, transparent) 0%, transparent 100%)",
          border:
            "1px solid color-mix(in srgb, var(--primary) 14%, var(--border))",
          boxShadow: [
            "inset 0 1px 0 color-mix(in srgb, #fff 72%, transparent)",
            "0 8px 14px -12px color-mix(in srgb, var(--primary) 42%, transparent)",
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
