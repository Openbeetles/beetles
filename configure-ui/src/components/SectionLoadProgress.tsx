import Box from "@mui/material/Box";
import LinearProgress from "@mui/material/LinearProgress";

/** 设备页区块加载态共用进度条。 */
const LINEAR_SX = {
  height: 3,
  borderRadius: "var(--radius-chip)",
  backgroundColor: "var(--border-subtle)",
  "& .MuiLinearProgress-bar": {
    borderRadius: "var(--radius-chip)",
  },
} as const;

export interface SectionLoadProgressProps {
  loading: boolean;
}

/**
 * 设置区块加载态：仅保留细条 indeterminate LinearProgress。
 */
export function SectionLoadProgress({ loading }: SectionLoadProgressProps) {
  if (!loading) return null;
  return (
    <Box aria-busy="true" role="status">
      <LinearProgress sx={LINEAR_SX} />
    </Box>
  );
}
