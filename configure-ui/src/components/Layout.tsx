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
import { ChatDialog } from "./ChatDialog";
import { ConfirmDialog } from "./ConfirmDialog";
import { DeviceAccessCard } from "./DeviceAccessCard";
import { FloatingChatButton } from "./FloatingChatButton";
import { ShellPageTransition } from "./ShellPageTransition";
import { Taskbar } from "./Taskbar";
import { TopBar } from "./TopBar";
import { shouldShowProtectedRouteBlocker } from "./layoutAccessGate";
import { NavBlockerContext } from "../contexts/NavBlockerContext";
import { MAIN_CONTENT_INNER_SX } from "../theme/panelStyles";
import { UnsavedContext } from "../contexts/UnsavedContext";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useToast } from "../hooks/useToast";
import {
  useDeviceConnected,
  useRestartPhase,
  consumeReconnectedAfterRestart,
  consumeRestartTimeout,
} from "../store/deviceStatusStore";

interface LayoutProps {
  onOpenSettings?: () => void;
}

function MainSurface({
  children,
  immersive = false,
}: {
  children: ReactNode;
  immersive?: boolean;
}) {
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
      }}
    >
      <Box
        sx={{
          position: "relative",
          zIndex: 1,
          ...MAIN_CONTENT_INNER_SX,
          py: immersive ? 0 : { xs: 1.5, sm: 2 },
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
  const { t } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const { dirty, clearAllDirty } = useContext(UnsavedContext);
  const { appMode } = useDeviceApi();
  const { showToast } = useToast();
  const deviceConnected = useDeviceConnected();
  const restartPhase = useRestartPhase();
  const [pendingPath, setPendingPath] = useState<string | null>(null);
  const [chatOpen, setChatOpen] = useState(false);
  const showRestartBanner = restartPhase !== "idle";
  const showProtectedRouteBlocker = shouldShowProtectedRouteBlocker({
    pathname: location.pathname,
    appMode,
    deviceConnected,
    restartPhase,
  });

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
  const useImmersiveMainSurface =
    location.pathname === "/device" || showProtectedRouteBlocker;

  const handleUnsavedConfirm = useCallback(() => {
    clearAllDirty();
    if (pendingPath) navigate(pendingPath);
    setPendingPath(null);
  }, [clearAllDirty, navigate, pendingPath]);

  const protectedRouteBlocker = showProtectedRouteBlocker ? (
    <DeviceAccessCard />
  ) : null;
  const showFloatingChatButton = appMode === "ready";

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
    border: "1px solid var(--card-glass-border)",
    backgroundColor: "var(--card-glass)",
    backgroundImage:
      "linear-gradient(135deg, color-mix(in srgb, #fff 14%, transparent) 0%, transparent 55%)",
    boxShadow: "var(--card-glass-shadow)",
    backdropFilter: "blur(40px) saturate(1.6)",
    WebkitBackdropFilter: "blur(40px) saturate(1.6)",
  };

  return (
    <NavBlockerContext.Provider value={navBlockerValue}>
      <Box
        sx={{
          position: "relative",
          isolation: "isolate",
          display: "flex",
          height: "100vh",
          overflow: "hidden",
          /** 透明以便 body 纯色底透出；顶栏/任务栏用 shellChromeSurface 变体 */
          backgroundColor: "transparent",
          "&::before": {
            content: '""',
            position: "absolute",
            inset: 0,
            zIndex: 0,
            pointerEvents: "none",
            backgroundImage: [
              "radial-gradient(circle at 12% 0%, color-mix(in srgb, var(--primary) 18%, transparent) 0%, transparent 34%)",
              "radial-gradient(circle at 88% 8%, color-mix(in srgb, var(--accent) 16%, transparent) 0%, transparent 28%)",
              "radial-gradient(circle at 54% 92%, color-mix(in srgb, var(--primary) 8%, transparent) 0%, transparent 22%)",
              "linear-gradient(180deg, color-mix(in srgb, var(--surface) 38%, transparent) 0%, transparent 28%, transparent 70%, color-mix(in srgb, var(--foreground) 2%, transparent) 100%)",
            ].join(", "),
          },
        }}
      >
        <ConfirmDialog
          open={showUnsavedDialog}
          onClose={() => setPendingPath(null)}
          title={t("common.unsavedLeaveTitle")}
          description={t("common.unsavedLeaveDesc")}
          dialogIcon="unsavedChanges"
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
              boxShadow: "none",
            }}
          >
            <TopBar />
          </Box>
          <MainSurface immersive={useImmersiveMainSurface}>
            {protectedRouteBlocker ?? <ShellPageTransition />}
          </MainSurface>
          {showFloatingChatButton ? (
            <>
              <FloatingChatButton onClick={() => setChatOpen(true)} />
              <ChatDialog
                open={chatOpen}
                onClose={() => setChatOpen(false)}
                onMinimize={() => setChatOpen(false)}
              />
            </>
          ) : null}
          <Taskbar onOpenSettings={onOpenSettings} />
        </Box>
      </Box>
    </NavBlockerContext.Provider>
  );
}
