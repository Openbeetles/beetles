import {
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import { useLocation, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "./ConfirmDialog";
import { DisconnectedCacheOverlay } from "./DisconnectedCacheOverlay";
import { DeviceBanner } from "./DeviceBanner";
import { ShellPageTransition } from "./ShellPageTransition";
import { Taskbar } from "./Taskbar";
import { TopBar } from "./TopBar";
import { NavBlockerContext } from "../contexts/NavBlockerContext";
import { MAIN_CONTENT_INNER_SX } from "../theme/panelStyles";
import { UnsavedContext } from "../contexts/UnsavedContext";
import { useConfig } from "../hooks/useConfig";
import { useToast } from "../hooks/useToast";
import {
  useDeviceConnected,
  useRestartPhase,
  consumeReconnectedAfterRestart,
  consumeRestartTimeout,
} from "../store/deviceStatusStore";
import { OS_ICON_DIALOG } from "../config/osIcons";
import { Os3dIcon } from "./Os3dIcon";

interface LayoutProps {
  onOpenSettings?: () => void;
}

function MainSurface({ children }: { children: ReactNode }) {
  return (
    <Box
      component="main"
      data-main-surface
      sx={{
        position: "relative",
        /** `1 1 0`：中间列占满顶栏与任务栏之间槽位，避免 basis 随内容撑开导致滚动高度不对 */
        flex: "1 1 0",
        minHeight: 0,
        /** 由子路由在「主表单区」内滚动，避免顶栏/任务栏与标题栏跟内容一起滚 */
        overflow: "hidden",
        display: "flex",
        flexDirection: "column",
        /** 水平不设 padding：内层与 `MAIN_CONTENT_INNER_SX` / 顶栏 pl 对齐 */
        px: 0,
        width: "100%",
        /** 与顶栏接缝处内凹高光，强化「桌面工作区」层次 */
        boxShadow: "var(--shell-main-inset-top)",
      }}
    >
      <Box
        sx={{
          position: "relative",
          zIndex: 1,
          ...MAIN_CONTENT_INNER_SX,
          alignItems: "stretch",
          alignSelf: "stretch",
          flex: "1 1 0",
          flexBasis: 0,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        {children}
      </Box>
    </Box>
  );
}

export function Layout({ onOpenSettings }: LayoutProps) {
  const AUTO_REFRESH_SECONDS = 5;
  const { t } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const { dirty, setDirty } = useContext(UnsavedContext);
  const { config, clearCachedConfig, refreshCachedConfig } = useConfig();
  const { showToast } = useToast();
  const deviceConnected = useDeviceConnected();
  const restartPhase = useRestartPhase();
  const [pendingPath, setPendingPath] = useState<string | null>(null);
  const [refreshingCache, setRefreshingCache] = useState(false);
  const [refreshCountdown, setRefreshCountdown] =
    useState(AUTO_REFRESH_SECONDS);
  /** 离线缓存蒙层下允许进入「连接设备」修改地址；离开该页或重连后恢复提示 */
  const [
    suppressDisconnectedCacheOverlay,
    setSuppressDisconnectedCacheOverlay,
  ] = useState(false);
  const showRestartBanner = restartPhase !== "idle";
  const showDisconnectedCacheBanner =
    !deviceConnected &&
    config != null &&
    !showRestartBanner &&
    !suppressDisconnectedCacheOverlay;
  useEffect(() => {
    if (!deviceConnected) return;
    queueMicrotask(() => setSuppressDisconnectedCacheOverlay(false));
  }, [deviceConnected]);

  useEffect(() => {
    if (location.pathname === "/device") return;
    queueMicrotask(() => setSuppressDisconnectedCacheOverlay(false));
  }, [location.pathname]);

  const attemptNavigate = useCallback(
    (path: string) => {
      if (location.pathname === path) return;
      if (!dirty) {
        navigate(path);
        return;
      }
      setPendingPath(path);
    },
    [dirty, location.pathname, navigate],
  );

  const navBlockerValue = useMemo(
    () => ({ attemptNavigate }),
    [attemptNavigate],
  );

  useEffect(() => {
    if (!dirty) return;
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      e.preventDefault();
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, [dirty]);

  useEffect(() => {
    if (consumeReconnectedAfterRestart()) {
      showToast(t("device.restartComplete"), { variant: "success" });
    }
    if (consumeRestartTimeout()) {
      showToast(t("device.restartTimeout"), { variant: "error" });
    }
  }, [restartPhase, showToast, t]);

  const showUnsavedDialog = dirty && pendingPath != null;

  const handleUnsavedConfirm = useCallback(() => {
    setDirty(false);
    if (pendingPath) navigate(pendingPath);
    setPendingPath(null);
  }, [navigate, pendingPath, setDirty]);

  const handleRefreshCachedConfig = useCallback(async () => {
    if (refreshingCache) return;
    setRefreshingCache(true);
    const result = await refreshCachedConfig();
    if (!result.ok && result.error) {
      showToast(result.error, { variant: "warning" });
    }
    setRefreshingCache(false);
    setRefreshCountdown(AUTO_REFRESH_SECONDS);
  }, [refreshingCache, refreshCachedConfig, showToast]);

  useEffect(() => {
    if (!showDisconnectedCacheBanner) {
      queueMicrotask(() => {
        setRefreshingCache(false);
        setRefreshCountdown(AUTO_REFRESH_SECONDS);
      });
      return;
    }
    if (refreshingCache) return;
    const timer = window.setInterval(() => {
      setRefreshCountdown((prev) => {
        if (prev <= 1) {
          void handleRefreshCachedConfig();
          return AUTO_REFRESH_SECONDS;
        }
        return prev - 1;
      });
    }, 1000);
    return () => window.clearInterval(timer);
  }, [showDisconnectedCacheBanner, refreshingCache, handleRefreshCachedConfig]);

  /** 全屏磨砂底：拦截底层交互，避免可视蒙层下仍可点击 */
  const statusOverlayBackdropSx = {
    position: "fixed" as const,
    inset: 0,
    zIndex: 1100,
    pointerEvents: "auto" as const,
    backgroundColor: "color-mix(in srgb, var(--foreground) 10%, transparent)",
    backdropFilter: "blur(var(--overlay-backdrop-blur))",
    WebkitBackdropFilter: "blur(var(--overlay-backdrop-blur))",
  };

  const statusOverlayCardSx = {
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
    alignItems: "center",
    justifyContent: "space-between",
    gap: 2,
    px: 2,
    py: 1.5,
    borderRadius: "var(--radius-card)",
    border: "1px solid var(--form-outline-rest)",
    backgroundColor: "var(--card)",
    boxShadow:
      "0 8px 32px color-mix(in srgb, var(--foreground) 10%, transparent)",
  };

  return (
    <NavBlockerContext.Provider value={navBlockerValue}>
      <Box
        sx={{
          display: "flex",
          height: "100vh",
          overflow: "hidden",
          /** 透明以便 body 纯色底透出；顶栏/任务栏用 shellChromeSurface 变体 */
          backgroundColor: "transparent",
        }}
      >
        <ConfirmDialog
          open={showUnsavedDialog}
          onClose={() => setPendingPath(null)}
          title={t("common.unsavedLeaveTitle")}
          description={t("common.unsavedLeaveDesc")}
          icon={<Os3dIcon src={OS_ICON_DIALOG.unsavedChanges} />}
          confirmColor="error"
          confirmLabel={t("common.discardChanges")}
          onConfirm={handleUnsavedConfirm}
        />
        {showRestartBanner && (
          <>
            <Box aria-hidden sx={statusOverlayBackdropSx} />
            <Box
              role="status"
              sx={{
                ...statusOverlayCardSx,
                justifyContent: "center",
                borderLeft: "var(--accent-line-width) solid var(--form-outline-rest)",
              }}
            >
              <Typography
                variant="body2"
                textAlign="center"
                sx={{
                  color: "var(--foreground-soft)",
                  fontWeight: 600,
                  fontSize: "var(--font-size-body-sm)",
                }}
              >
                {restartPhase === "pending"
                  ? t("device.restartPhasePending")
                  : t("device.restartPhaseRestarting")}
              </Typography>
            </Box>
          </>
        )}
        {showDisconnectedCacheBanner && (
          <DisconnectedCacheOverlay
            title={t("config.deviceDisconnectedCacheTitle")}
            subtitle={t("config.deviceDisconnectedCacheSubtitle")}
            editConnectionLabel={t("config.editDeviceConnection")}
            retryLabel={
              refreshingCache
                ? `${t("common.loading")}…`
                : `${t("common.retry")} (${refreshCountdown}s)`
            }
            onEditConnection={() => {
              clearCachedConfig();
              navigate("/device");
            }}
            onRetry={() => {
              void handleRefreshCachedConfig();
            }}
            retryDisabled={refreshingCache}
          />
        )}
        <Box
          sx={{
            display: "flex",
            flexDirection: "column",
            flex: 1,
            minWidth: 0,
            minHeight: 0,
          }}
        >
          <Box
            sx={{
              position: "relative",
              zIndex: 10,
              boxShadow:
                "0 4px 20px color-mix(in srgb, var(--foreground) 5%, transparent)",
            }}
          >
            <TopBar onOpenSettings={onOpenSettings} />
            <DeviceBanner />
          </Box>
          {/** 与 main 平级的占位条：上下呼吸感；主区内滚动仍占满 main（见 panelStyles / MainSurface flex 链） */}
          <Box
            aria-hidden
            sx={(theme) => ({
              flexShrink: 0,
              height: theme.spacing(5),
              minHeight: theme.spacing(5),
            })}
          />
          <MainSurface>
            <ShellPageTransition />
          </MainSurface>
          <Box
            aria-hidden
            sx={(theme) => ({
              flexShrink: 0,
              height: theme.spacing(6),
              minHeight: theme.spacing(6),
            })}
          />
          <Taskbar />
        </Box>
      </Box>
    </NavBlockerContext.Provider>
  );
}
