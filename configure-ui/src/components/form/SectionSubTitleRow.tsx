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
          width: "var(--accent-line-width)",
          flexShrink: 0,
          borderRadius: "var(--radius-chip)",
          bgcolor: "var(--primary)",
          ...(accentStretch
            ? {
                alignSelf: "stretch",
                minHeight: "var(--icon-container-md)",
              }
            : {
                height: "var(--icon-container-sm)",
                alignSelf: "center",
              }),
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
