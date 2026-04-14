import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import Stack from "@mui/material/Stack";
import { Os3dIcon } from "./Os3dIcon";
import { PanelStateHeroRow } from "./PanelStateBlock";
import { OS_ICON_DASHBOARD } from "../config/osIcons";

const BACKDROP_SX = {
  position: "fixed" as const,
  inset: 0,
  zIndex: 1100,
  pointerEvents: "auto" as const,
  backgroundColor: "color-mix(in srgb, var(--foreground) 10%, transparent)",
  backdropFilter: "blur(var(--overlay-backdrop-blur))",
  WebkitBackdropFilter: "blur(var(--overlay-backdrop-blur))",
};

/** 与 `ConfirmDialog` 同档内边距；细描边 + 轻投影 */
const CARD_SX = {
  position: "fixed" as const,
  top: "50%",
  left: "50%",
  transform: "translate(-50%, -50%)",
  zIndex: 1101,
  pointerEvents: "auto" as const,
  width:
    "min(var(--status-overlay-card-max), calc(100vw - var(--status-overlay-card-inset)))",
  maxWidth: "100%",
  boxSizing: "border-box" as const,
  display: "flex",
  flexDirection: "column",
  alignItems: "stretch",
  gap: 2.5,
  px: 3,
  pt: 3.5,
  pb: 3,
  borderRadius: "var(--radius-card)",
  border: "1px solid var(--form-outline-rest)",
  backgroundColor: "var(--card)",
  boxShadow:
    "0 8px 32px color-mix(in srgb, var(--foreground) 10%, transparent)",
};

export interface DisconnectedCacheOverlayProps {
  /** 主标题（短、醒目） */
  title: string;
  /** 说明正文 */
  subtitle: string;
  editConnectionLabel: string;
  retryLabel: string;
  onEditConnection: () => void;
  onRetry: () => void;
  retryDisabled: boolean;
}

/**
 * 设备不可达且存在本地缓存时的全屏提示：图标置顶居中 + 文案居中，与 `ConfirmDialog` 一致。
 * 「编辑设备连接」由调用方在导航前清空本地缓存（见 `Layout`）。
 * Full-screen overlay when the device is unreachable but cached config exists.
 */
export function DisconnectedCacheOverlay({
  title,
  subtitle,
  editConnectionLabel,
  retryLabel,
  onEditConnection,
  onRetry,
  retryDisabled,
}: DisconnectedCacheOverlayProps) {
  return (
    <>
      <Box aria-hidden sx={BACKDROP_SX} />
      <Box role="status" aria-live="polite" sx={CARD_SX}>
        <PanelStateHeroRow
          tone="danger"
          layout="stack"
          icon={<Os3dIcon src={OS_ICON_DASHBOARD.deviceUnreachable} />}
          title={title}
          description={subtitle}
        />

        <Stack
          direction={{ xs: "column", sm: "row" }}
          spacing={1}
          useFlexGap
          sx={{
            width: "100%",
            alignItems: "stretch",
            justifyContent: "center",
          }}
        >
          <Button
            size="medium"
            variant="contained"
            onClick={onEditConnection}
            sx={{
              borderRadius: "var(--radius-control)",
              textTransform: "none",
              fontWeight: 600,
              boxShadow: "none",
              "&:hover": { boxShadow: "none" },
            }}
          >
            {editConnectionLabel}
          </Button>
          <Button
            size="medium"
            variant="outlined"
            onClick={onRetry}
            disabled={retryDisabled}
            startIcon={
              retryDisabled ? (
                <CircularProgress size={16} color="inherit" />
              ) : undefined
            }
            sx={{
              borderRadius: "var(--radius-control)",
              textTransform: "none",
              fontWeight: 600,
              borderColor: "var(--primary)",
              color: "var(--primary)",
            }}
          >
            {retryLabel}
          </Button>
        </Stack>
      </Box>
    </>
  );
}
