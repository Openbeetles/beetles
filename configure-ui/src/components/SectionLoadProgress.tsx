import Box from "@mui/material/Box";
import LinearProgress from "@mui/material/LinearProgress";

/** 与 ChannelConnectivityPanel 顶栏一致，供设备页多卡片复用 */
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
  /** 兼容旧调用方保留；当前仅显示进度条。 */
  idleHint?: string;
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
