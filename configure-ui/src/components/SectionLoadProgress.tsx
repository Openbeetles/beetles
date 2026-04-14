import Box from "@mui/material/Box";
import LinearProgress from "@mui/material/LinearProgress";
import Typography from "@mui/material/Typography";

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
  /** 尚无缓存内容时显示；已有数据刷新时仅保留顶条 */
  idleHint?: string;
}

/**
 * 设置区块加载态：细条 indeterminate LinearProgress，可选一句说明。
 */
export function SectionLoadProgress({ loading, idleHint }: SectionLoadProgressProps) {
  if (!loading) return null;
  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
      <LinearProgress aria-busy sx={LINEAR_SX} />
      {idleHint?.trim() ? (
        <Typography variant="body2" sx={{ color: "var(--text-tertiary)", fontSize: "var(--font-size-caption)" }}>
          {idleHint}
        </Typography>
      ) : null}
    </Box>
  );
}
