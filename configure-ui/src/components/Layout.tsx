import {
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Drawer from "@mui/material/Drawer";
import Typography from "@mui/material/Typography";
import { useTheme } from "@mui/material/styles";
import useMediaQuery from "@mui/material/useMediaQuery";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import {
  SIDEBAR_DRAWER_BREAKPOINT,
  SIDEBAR_DRAWER_WIDTH,
} from "../config/layout";
import { ConfirmDialog } from "./ConfirmDialog";
import { DeviceBanner } from "./DeviceBanner";
import { Sidebar } from "./Sidebar";
import { TopBar } from "./TopBar";
import { NavBlockerContext } from "../contexts/NavBlockerContext";
import { PcbDecorOverlay } from "./PcbDecorOverlay";
import { MAIN_SURFACE_PCB_SX } from "../theme/pcbSurface";
import { UnsavedContext } from "../contexts/UnsavedContext";
import { useConfig } from "../hooks/useConfig";
import { useToast } from "../hooks/useToast";
import {
  useDeviceConnected,
  useRestartPhase,
  consumeReconnectedAfterRestart,
  consumeRestartTimeout,
} from "../store/deviceStatusStore";
import WarningAmberRounded from "@mui/icons-material/WarningAmberRounded";

interface LayoutProps {
  onOpenSettings?: () => void;
}

function MainSurface({ children }: { children: ReactNode }) {
  return (
    <Box
      component="main"
      sx={{
        ...MAIN_SURFACE_PCB_SX,
        position: "relative",
        flex: 1,
        minHeight: 0,
        overflow: "auto",
        pt: 3,
        pb: 5,
        px: { xs: 2, md: 3 },
        width: "100%",
      }}
    >
      <PcbDecorOverlay />
      <Box sx={{ position: "relative", zIndex: 1 }}>{children}</Box>
    </Box>
  );
}

export function Layout({ onOpenSettings }: LayoutProps) {
  const AUTO_REFRESH_SECONDS = 5;
  const theme = useTheme();
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
  const showRestartBanner = restartPhase !== "idle";
  const showDisconnectedCacheBanner =
    !deviceConnected && config != null && !showRestartBanner;
  const sidebarAsDrawer = useMediaQuery(
    theme.breakpoints.down(SIDEBAR_DRAWER_BREAKPOINT),
  );
  const [drawerOpen, setDrawerOpen] = useState(false);

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
    border: "none",
    backgroundColor: "var(--card)",
    boxShadow: "none",
  };

  return (
    <NavBlockerContext.Provider value={navBlockerValue}>
      <Box
        sx={{
          display: "flex",
          height: "100vh",
          overflow: "hidden",
          backgroundColor: "var(--background)",
        }}
      >
        <ConfirmDialog
          open={showUnsavedDialog}
          onClose={() => setPendingPath(null)}
          title={t("common.unsavedLeaveTitle")}
          description={t("common.unsavedLeaveDesc")}
          icon={<WarningAmberRounded />}
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
                borderLeft: "var(--accent-line-width) solid var(--muted)",
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
          <>
            <Box aria-hidden sx={statusOverlayBackdropSx} />
            <Box
              role="status"
              sx={{
                ...statusOverlayCardSx,
                borderLeft: "var(--accent-line-width) solid var(--semantic-warning)",
              }}
            >
              <Typography
                variant="body2"
                sx={{
                  color: "var(--semantic-warning)",
                  fontWeight: 600,
                  fontSize: "var(--font-size-body-sm)",
                  flex: 1,
                  minWidth: 0,
                }}
              >
                {t("config.deviceDisconnectedCache")}
              </Typography>
              <Button
                size="small"
                variant="contained"
                onClick={() => {
                  void handleRefreshCachedConfig();
                }}
                disabled={refreshingCache}
                sx={{
                  flexShrink: 0,
                  borderRadius: "var(--radius-control)",
                  backgroundColor: "var(--primary)",
                  color: "var(--primary-fg)",
                  "&:hover:not(:disabled)": {
                    backgroundColor:
                      "color-mix(in srgb, var(--primary) 86%, black)",
                  },
                }}
              >
                {refreshingCache
                  ? `${t("common.loading")}…`
                  : `${t("common.retry")} (${refreshCountdown}s)`}
              </Button>
              <Button
                size="small"
                variant="outlined"
                onClick={clearCachedConfig}
                sx={{
                  flexShrink: 0,
                  borderRadius: "var(--radius-control)",
                  borderColor: "var(--semantic-warning)",
                  color: "var(--semantic-warning)",
                }}
              >
                {t("config.clearCache")}
              </Button>
            </Box>
          </>
        )}
        {sidebarAsDrawer ? (
          <>
            <Drawer
              anchor="left"
              open={drawerOpen}
              onClose={() => setDrawerOpen(false)}
              slotProps={{
                backdrop: {
                  sx: { backgroundColor: "var(--backdrop-overlay)" },
                },
              }}
              sx={{
                "& .MuiDrawer-paper": {
                  width: SIDEBAR_DRAWER_WIDTH,
                  maxWidth: "85vw",
                  boxSizing: "border-box",
                  backgroundColor: "var(--surface)",
                  boxShadow: "none",
                },
              }}
            >
              <Sidebar drawer />
            </Drawer>
            <Box
              sx={{
                display: "flex",
                flexDirection: "column",
                flex: 1,
                minWidth: 0,
              }}
            >
              <TopBar
                onMenuClick={() => setDrawerOpen(true)}
                onOpenSettings={() => {
                  setDrawerOpen(false);
                  onOpenSettings?.();
                }}
              />
              <DeviceBanner />
              <MainSurface>
                <Outlet />
              </MainSurface>
            </Box>
          </>
        ) : (
          <>
            <Sidebar />
            <Box
              sx={{
                display: "flex",
                flexDirection: "column",
                flex: 1,
                minWidth: 0,
              }}
            >
              <TopBar onOpenSettings={onOpenSettings} />
              <DeviceBanner />
              <MainSurface>
                <Outlet />
              </MainSurface>
            </Box>
          </>
        )}
      </Box>
    </NavBlockerContext.Provider>
  );
}
