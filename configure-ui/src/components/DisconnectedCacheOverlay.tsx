import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import { Os3dIcon } from "./Os3dIcon";
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

/** 与 `ConfirmDialog` / 状态卡一致的浮层卡片（细描边 + 轻投影） */
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
  gap: 2,
  p: 2.5,
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
  clearCacheLabel: string;
  onEditConnection: () => void;
  onRetry: () => void;
  onClearCache: () => void;
  retryDisabled: boolean;
}

/**
 * 设备不可达且存在本地缓存时的全屏提示：层次清晰的 CTA（编辑连接 / 重试 / 清空缓存）。
 * Full-screen overlay when the device is unreachable but cached config exists.
 */
export function DisconnectedCacheOverlay({
  title,
  subtitle,
  editConnectionLabel,
  retryLabel,
  clearCacheLabel,
  onEditConnection,
  onRetry,
  onClearCache,
  retryDisabled,
}: DisconnectedCacheOverlayProps) {
  return (
    <>
      <Box aria-hidden sx={BACKDROP_SX} />
      <Box
        role="status"
        aria-live="polite"
        sx={{
          ...CARD_SX,
          borderLeft:
            "var(--accent-line-width) solid var(--semantic-warning)",
        }}
      >
        <Stack direction="row" alignItems="flex-start" spacing={2}>
          <Box
            sx={{
              width: 52,
              height: 52,
              borderRadius: "var(--radius-chip)",
              flexShrink: 0,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              backgroundColor:
                "color-mix(in srgb, var(--semantic-warning) 16%, transparent)",
            }}
          >
            <Box sx={{ width: 40, height: 40 }}>
              <Os3dIcon src={OS_ICON_DASHBOARD.connection} />
            </Box>
          </Box>
          <Box sx={{ minWidth: 0, flex: 1, pt: 0.25 }}>
            <Typography
              component="h2"
              sx={{
                fontFamily: "var(--font-sans)",
                fontSize: "var(--font-size-body-lg)",
                fontWeight: 700,
                letterSpacing: "var(--letter-spacing-tight)",
                lineHeight: "var(--line-height-snug)",
                color: "var(--foreground)",
                mb: 0.75,
              }}
            >
              {title}
            </Typography>
            <Typography
              sx={{
                fontSize: "var(--font-size-body-sm)",
                lineHeight: "var(--line-height-relaxed)",
                color: "var(--text-tertiary)",
              }}
            >
              {subtitle}
            </Typography>
          </Box>
        </Stack>

        <Box
          sx={{
            display: "flex",
            flexDirection: { xs: "column", sm: "row" },
            flexWrap: "wrap",
            gap: 1,
            alignItems: { xs: "stretch", sm: "center" },
            justifyContent: { sm: "space-between" },
            rowGap: 1.5,
          }}
        >
          <Stack
            direction={{ xs: "column", sm: "row" }}
            spacing={1}
            sx={{ flex: { sm: "1 1 auto" }, minWidth: 0 }}
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
          <Button
            size="medium"
            variant="text"
            onClick={onClearCache}
            sx={{
              alignSelf: { xs: "flex-start", sm: "center" },
              borderRadius: "var(--radius-control)",
              textTransform: "none",
              fontWeight: 600,
              color: "var(--semantic-warning)",
            }}
          >
            {clearCacheLabel}
          </Button>
        </Box>
      </Box>
    </>
  );
}
