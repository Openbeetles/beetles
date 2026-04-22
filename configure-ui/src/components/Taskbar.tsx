import {
  type KeyboardEvent as ReactKeyboardEvent,
  useCallback,
  type MouseEvent,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import Popover from "@mui/material/Popover";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import { useTranslation } from "react-i18next";
import { Link, useLocation, useNavigate } from "react-router-dom";
import * as systemApi from "../api/endpoints/system";
import { BeetleIcon } from "./BeetleIcon";
import { ConfirmDialog } from "./ConfirmDialog";
import { Os3dIcon } from "./Os3dIcon";
import { NavBlockerContext } from "../contexts/NavBlockerContext";
import { NAV_ITEMS } from "../config/navItems";
import { OS_ICON_SHELL } from "../config/osIcons";
import { TASKBAR_HEIGHT } from "../config/layout";
import { PANEL_SECTION_PADDING } from "../theme/panelStyles";
import { useDevice } from "../hooks/useDevice";
import { useDeviceApi, type DeviceHintReason } from "../hooks/useDeviceApi";
import { translateApiError } from "../i18n/apiErrors";
import { useToast } from "../hooks/useToast";
import { setRestartPending } from "../store/deviceStatusStore";
import { SHELL_TASKBAR_CHROME_SX } from "../theme/shellChromeSurface";

/** 顶栏已有「连接设备」宽幅磁贴，网格内不再重复 `/device` */
const START_MENU_NAV_ITEMS = NAV_ITEMS.filter(
  (item) => item.path !== "/device",
);
const START_MENU_GAP_PX = 14;
const DOCK_MAGNIFY_SCALE = [1.12, 1.06, 1.02];
const DOCK_MAGNIFY_LIFT_PX = [8, 4, 1];
const START_MENU_TILE_MIN_HEIGHT = 112;
const TASKBAR_TRAY_CONTROL_SIZE = 36;

function getDockMotion(index: number, hoveredIndex: number | null) {
  if (hoveredIndex === null) {
    return { scale: 1, translateY: 0, zIndex: 1 };
  }

  const distance = Math.abs(index - hoveredIndex);
  if (distance >= DOCK_MAGNIFY_SCALE.length) {
    return { scale: 1, translateY: 0, zIndex: 1 };
  }

  return {
    scale: DOCK_MAGNIFY_SCALE[distance],
    translateY: DOCK_MAGNIFY_LIFT_PX[distance],
    zIndex: DOCK_MAGNIFY_SCALE.length - distance + 1,
  };
}

function getNavBlockedMessageKey(reason: DeviceHintReason): string {
  switch (reason) {
    case "no_device":
      return "device.bannerNeedDevice";
    case "device_not_activated":
      return "device.bannerDeviceNotActivated";
    case "no_pairing":
      return "device.bannerNeedPairing";
    default:
      return "device.connectFirst";
  }
}

function displayHost(baseUrl: string): string {
  try {
    return new URL(baseUrl).host;
  } catch {
    return baseUrl;
  }
}

const NAV_BLOCK_TOAST_COOLDOWN_MS = 2500;

function collectStartMenuFocusables(root: HTMLElement | null): HTMLElement[] {
  if (!root) return [];
  return Array.from(
    root.querySelectorAll<HTMLElement>('a[href], button, [role="button"]'),
  ).filter((el) => {
    if (el.getAttribute("aria-hidden") === "true") return false;
    if (el.getAttribute("aria-disabled") === "true") return false;
    if ("disabled" in el && (el as HTMLButtonElement).disabled) return false;
    if (el.tabIndex < 0) return false;
    const style = window.getComputedStyle(el);
    if (style.visibility === "hidden" || style.display === "none") return false;
    return true;
  });
}

/** Windows 风格任务栏：Beetle OS 徽标为「开始」、中部固定快捷方式、右侧托盘（连接状态）。 */
interface TaskbarProps {
  onOpenSettings?: () => void;
}

/** Windows 风格任务栏：Beetle OS 徽标为「开始」、中部固定快捷方式、右侧托盘（连接状态）。 */
export function Taskbar({ onOpenSettings }: TaskbarProps) {
  const { t } = useTranslation();
  const location = useLocation();
  const navigate = useNavigate();
  const navBlocker = useContext(NavBlockerContext);
  const { baseUrl, pairingCode } = useDevice();
  const {
    deviceConnected,
    connectionChecking,
    needDeviceHint,
    deviceHintReason,
  } = useDeviceApi();
  const { showToast } = useToast();
  const lastNavBlockToastRef = useRef<{ key: string; at: number } | null>(null);
  const [startAnchor, setStartAnchor] = useState<HTMLElement | null>(null);
  const [taskbarAnchorEl, setTaskbarAnchorEl] = useState<HTMLDivElement | null>(
    null,
  );
  const [startHovered, setStartHovered] = useState(false);
  const [dockHoveredIndex, setDockHoveredIndex] = useState<number | null>(null);
  const [restarting, setRestarting] = useState(false);
  const [restartConfirmOpen, setRestartConfirmOpen] = useState(false);
  const startMenuPanelRef = useRef<HTMLDivElement | null>(null);
  const pathname = location.pathname;
  const canNavigate = (path: string) =>
    path === "/device" || (deviceConnected && !needDeviceHint);
  const startOpen = Boolean(startAnchor);
  const trayControlSurfaceSx = {
    border: "1px solid color-mix(in srgb, var(--border) 18%, transparent)",
    boxShadow: "var(--os3d-pedestal-lift-stack)",
    borderRadius: "calc(var(--radius-chip) + 2px)",
    backgroundColor: "color-mix(in srgb, var(--card) 72%, var(--surface))",
    backgroundImage:
      "linear-gradient(180deg, color-mix(in srgb, #fff 12%, transparent) 0%, transparent 100%)",
    transition:
      "background-color var(--transition-duration) var(--ease-emphasized), border-color var(--transition-duration) ease, box-shadow var(--transition-duration) var(--ease-emphasized), transform var(--transition-duration) var(--ease-emphasized)",
    "&:hover": {
      backgroundColor: "color-mix(in srgb, var(--primary) 8%, var(--surface))",
      borderColor: "color-mix(in srgb, var(--primary) 26%, var(--border))",
      boxShadow: "var(--os3d-selection-pill-stack)",
      transform: "translateY(-0.5px)",
    },
    "&:active": {
      transform: "translateY(0)",
    },
    "@media (prefers-reduced-motion: reduce)": {
      transition: "none",
      "&:hover": { transform: "none" },
    },
  } as const;

  const closeStart = () => setStartAnchor(null);

  const doRestart = async () => {
    if (!baseUrl?.trim() || !pairingCode?.trim()) return;
    setRestarting(true);
    const res = await systemApi.postRestart(baseUrl, pairingCode);
    setRestarting(false);
    setRestartConfirmOpen(false);
    closeStart();
    if (res.ok) {
      setRestartPending();
      showToast(t("device.restartSent"), { variant: "success" });
    } else {
      showToast(translateApiError(t, res.error, "device.restartFail"), { variant: "error" });
    }
  };

  const handleRestartClick = () => {
    if (!baseUrl?.trim() || !pairingCode?.trim() || restarting) return;
    setRestartConfirmOpen(true);
  };

  const handleStartClick = (e: MouseEvent<HTMLElement>) => {
    setStartAnchor(startOpen ? null : e.currentTarget);
  };

  const handleTaskbarRef = useCallback((node: HTMLDivElement | null) => {
    setTaskbarAnchorEl(node);
  }, []);

  useEffect(() => {
    if (!startOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      setStartAnchor(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [startOpen]);

  useEffect(() => {
    if (!startOpen || !startMenuPanelRef.current) return;
    const id = window.requestAnimationFrame(() => {
      const list = collectStartMenuFocusables(startMenuPanelRef.current);
      list[0]?.focus();
    });
    return () => window.cancelAnimationFrame(id);
  }, [startOpen]);

  const handleStartMenuKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (!startOpen) return;
    const keys = ["ArrowDown", "ArrowUp", "ArrowRight", "ArrowLeft"];
    if (!keys.includes(e.key)) return;
    const root = startMenuPanelRef.current;
    if (!root) return;
    const list = collectStartMenuFocusables(root);
    if (list.length === 0) return;
    const active = document.activeElement as HTMLElement | null;
    const i = list.indexOf(active as HTMLElement);
    if (i < 0) {
      if (e.key === "ArrowDown" || e.key === "ArrowRight") {
        list[0]?.focus();
        e.preventDefault();
      }
      return;
    }
    const next =
      e.key === "ArrowDown" || e.key === "ArrowRight"
        ? (i + 1) % list.length
        : (i - 1 + list.length) % list.length;
    list[next]?.focus();
    e.preventDefault();
  };

  return (
    <Box
      component="nav"
      aria-label={t("nav.taskbar")}
      ref={handleTaskbarRef}
      sx={{
        flexShrink: 0,
        height: TASKBAR_HEIGHT,
        minHeight: TASKBAR_HEIGHT,
        display: "flex",
        alignItems: "stretch",
        justifyContent: "space-between",
        px: { xs: 1.5, sm: 2 },
        gap: { xs: 1, sm: 1.25 },
        ...SHELL_TASKBAR_CHROME_SX,
        borderTop: "1px solid color-mix(in srgb, var(--border) 14%, transparent)",
        position: "relative",
        zIndex: 2,
        overflow: "visible",
      }}
    >
      <Tooltip title={t("nav.startMenu")} placement="top">
        <Box
          component="button"
          type="button"
          onClick={handleStartClick}
          onMouseEnter={() => setStartHovered(true)}
          onMouseLeave={() => setStartHovered(false)}
          aria-expanded={startOpen}
          aria-haspopup="menu"
          aria-label={t("nav.startMenu")}
          sx={{
            alignSelf: "center",
            flexShrink: 0,
            width: 44,
            height: 44,
            p: 0,
            borderRadius: "calc(var(--radius-chip) + 2px)",
            margin: 0,
            cursor: "pointer",
            font: "inherit",
            display: "grid",
            placeItems: "center",
            border:
              "1px solid color-mix(in srgb, var(--border) 20%, transparent)",
            backgroundColor:
              startOpen
                ? "color-mix(in srgb, var(--primary) 10%, var(--card))"
                : "color-mix(in srgb, var(--card) 76%, var(--surface))",
            backgroundImage: [
              "linear-gradient(180deg, color-mix(in srgb, #fff 14%, transparent) 0%, transparent 100%)",
              "linear-gradient(135deg, color-mix(in srgb, var(--primary) 8%, transparent) 0%, transparent 100%)",
            ].join(", "),
            boxShadow: startOpen
              ? "var(--os3d-selection-pill-stack)"
              : "var(--os3d-control-soft-lift-stack)",
            transition:
              "background-color var(--transition-duration) var(--ease-emphasized), border-color var(--transition-duration) ease, transform var(--transition-duration) var(--ease-emphasized), box-shadow var(--transition-duration) var(--ease-emphasized)",
            "&:hover": {
              backgroundColor:
                "color-mix(in srgb, var(--primary) 9%, var(--card))",
              borderColor:
                "color-mix(in srgb, var(--primary) 28%, var(--border))",
              transform: "translateY(-1px)",
            },
            "&:active": {
              transform: "translateY(0)",
            },
            "@media (prefers-reduced-motion: reduce)": {
              "&:hover": { transform: "none" },
            },
            "&:focus-visible": {
              outline: "var(--focus-ring-width) solid var(--primary)",
              outlineOffset: "var(--focus-ring-offset)",
            },
          }}
        >
          <BeetleIcon
            aria-hidden
            animationActive={startOpen || startHovered}
            sx={{
              width: 32,
              height: 32,
              borderRadius: "calc(var(--radius-control) - 2px)",
            }}
          />
        </Box>
      </Tooltip>

      <Popover
        open={startOpen}
        anchorEl={taskbarAnchorEl}
        onClose={closeStart}
        anchorOrigin={{ vertical: "top", horizontal: "left" }}
        transformOrigin={{ vertical: "bottom", horizontal: "left" }}
        /** 默认 16 会在贴边时把纸面往里推；与视口左缘对齐时需关闭 */
        marginThreshold={8}
        slotProps={{
          paper: {
            elevation: 0,
            sx: {
              width: "min(540px, 100vw - 32px)",
              maxHeight: "min(72vh, 540px)",
              position: "relative",
              display: "flex",
              flexDirection: "column",
              overflow: "hidden",
              borderRadius: "var(--radius-card)",
              border:
                "1px solid color-mix(in srgb, var(--border) 16%, transparent)",
              boxShadow: "var(--os3d-start-panel-stack)",
              backgroundColor:
                "color-mix(in srgb, var(--card) 96%, transparent)",
              backgroundImage: [
                "linear-gradient(180deg, color-mix(in srgb, #fff 18%, transparent) 0%, transparent 24%)",
                "linear-gradient(135deg, color-mix(in srgb, var(--primary) 8%, transparent) 0%, transparent 38%, color-mix(in srgb, var(--accent) 7%, transparent) 100%)",
                "linear-gradient(180deg, color-mix(in srgb, var(--surface) 58%, transparent) 0%, transparent 56%)",
              ].join(", "),
              backdropFilter: "saturate(1.12) blur(var(--shell-chrome-blur))",
              WebkitBackdropFilter:
                "saturate(1.12) blur(var(--shell-chrome-blur))",
              // Anchor to the taskbar instead of the start button so the menu
              // sits above the dock like a native start panel.
              ml: { xs: 1.5, sm: 2 },
              // MUI Popover's Grow transition owns `transform`, so use layout
              // offset here; this keeps the air gap working in both web and Tauri.
              mt: `-${START_MENU_GAP_PX}px`,
              "&::before": {
                content: '""',
                position: "absolute",
                inset: 0,
                pointerEvents: "none",
                backgroundImage:
                  "linear-gradient(180deg, color-mix(in srgb, #fff 20%, transparent) 0%, transparent 18%, transparent 82%, color-mix(in srgb, var(--foreground) 4%, transparent) 100%)",
              },
              "@media (prefers-reduced-motion: reduce)": {
                backdropFilter: "none",
                WebkitBackdropFilter: "none",
                backgroundColor: "var(--card)",
                backgroundImage: "none",
                "&::before, &::after": {
                  display: "none",
                },
              },
            },
          },
        }}
      >
        <Box
          ref={startMenuPanelRef}
          onKeyDown={handleStartMenuKeyDown}
          sx={{
            display: "flex",
            flexDirection: "column",
            flex: 1,
            minHeight: 0,
            outline: "none",
          }}
        >
          <Box
            sx={{
              flexShrink: 0,
              px: PANEL_SECTION_PADDING,
              py: 1.6,
              borderBottom: "1px solid color-mix(in srgb, var(--border) 16%, transparent)",
              background: [
                "linear-gradient(180deg, color-mix(in srgb, #fff 14%, var(--surface)) 0%, color-mix(in srgb, var(--surface) 66%, var(--card)) 100%)",
                "linear-gradient(90deg, color-mix(in srgb, var(--primary) 7%, transparent) 0%, transparent 62%, color-mix(in srgb, var(--accent) 6%, transparent) 100%)",
              ].join(", "),
              boxShadow:
                "inset 0 1px 0 color-mix(in srgb, #fff 42%, transparent)",
            }}
          >
            <Stack
              direction="row"
              alignItems="center"
              justifyContent="space-between"
              spacing={1}
              sx={{ gap: 1 }}
            >
              <Stack
                direction="row"
                alignItems="center"
                spacing={1.25}
                sx={{ minWidth: 0, flex: 1 }}
              >
                <BeetleIcon
                  aria-hidden
                  animationActive={startOpen || startHovered}
                  sx={{
                    width: "var(--icon-container-md)",
                    height: "var(--icon-container-md)",
                    borderRadius: 0,
                  }}
                />
                <Typography
                  variant="subtitle2"
                  sx={{
                    minWidth: 0,
                    fontFamily: "var(--font-display)",
                    fontWeight: 400,
                    letterSpacing: "0.04em",
                    fontSize: "var(--font-size-body-lg)",
                    lineHeight: 1.2,
                  }}
                >
                  {t("app.name")}
                </Typography>
              </Stack>
              {deviceConnected && (
                <Tooltip title={t("device.restart")} placement="left">
                  <span>
                    <IconButton
                      size="small"
                      disabled={restarting}
                      onClick={handleRestartClick}
                      aria-label={t("device.restart")}
                      sx={{
                        flexShrink: 0,
                        p: 0.5,
                        borderRadius: "var(--radius-control)",
                        color: "var(--semantic-danger)",
                        border: "1px solid color-mix(in srgb, var(--border) 14%, transparent)",
                        backgroundColor:
                          "color-mix(in srgb, var(--card) 70%, var(--surface))",
                        boxShadow: "var(--os3d-control-soft-lift-stack)",
                        "&:hover:not(:disabled)": {
                          backgroundColor:
                            "color-mix(in srgb, var(--semantic-danger) 8%, var(--card))",
                          boxShadow: "var(--os3d-selection-pill-stack)",
                        },
                        "&:disabled": { opacity: 0.55 },
                      }}
                    >
                      <Box
                        sx={{
                          width: 36,
                          height: 36,
                          display: "flex",
                          alignItems: "center",
                          justifyContent: "center",
                        }}
                      >
                        <Os3dIcon src={OS_ICON_SHELL.power} variant="inline" />
                      </Box>
                    </IconButton>
                  </span>
                </Tooltip>
              )}
            </Stack>
          </Box>

          <Box
            sx={{
              flex: 1,
              minHeight: 0,
              overflow: "auto",
              px: 1.5,
              pt: 1.5,
              pb: 1.5,
              display: "flex",
              flexDirection: "column",
              gap: 1.5,
              background:
                "linear-gradient(180deg, color-mix(in srgb, var(--surface) 22%, transparent) 0%, transparent 100%)",
            }}
          >
            {/* 宽幅「连接」磁贴 */}
            <Box
              component="button"
              type="button"
              onClick={(e) => {
                e.preventDefault();
                closeStart();
                if (navBlocker?.attemptNavigate) {
                  navBlocker.attemptNavigate("/device");
                } else {
                  navigate("/device");
                }
              }}
              aria-label={t("device.pageTitle")}
              sx={{
                width: "100%",
                border: "1px solid color-mix(in srgb, var(--border) 18%, transparent)",
                borderRadius: "var(--radius-card)",
                cursor: "pointer",
                font: "inherit",
                textAlign: "left",
                color: "inherit",
                p: 1.5,
                display: "grid",
                gridTemplateColumns: "auto 1fr",
                alignItems: "center",
                gap: 1.25,
                backgroundColor: deviceConnected
                  ? "color-mix(in srgb, var(--semantic-success) 7%, var(--card))"
                  : "color-mix(in srgb, var(--semantic-danger) 7%, var(--card))",
                backgroundImage:
                  "linear-gradient(180deg, color-mix(in srgb, #fff 14%, transparent) 0%, transparent 100%)",
                boxShadow: "var(--os3d-selection-pill-stack)",
                transition:
                  "background-color var(--transition-duration) var(--ease-out-smooth), transform var(--transition-duration) var(--ease-emphasized), box-shadow var(--transition-duration) var(--ease-emphasized)",
                "&:hover": {
                  backgroundColor: deviceConnected
                    ? "color-mix(in srgb, var(--semantic-success) 9%, var(--card))"
                    : "color-mix(in srgb, var(--semantic-danger) 9%, var(--card))",
                  transform: "translateY(-0.5px)",
                  boxShadow: "var(--os3d-chip-lift-stack)",
                },
                "&:active": {
                  transform: "translateY(0)",
                },
                "@media (prefers-reduced-motion: reduce)": {
                  "&:active": { transform: "none" },
                },
              }}
            >
              <Box
                sx={{
                  width: 12,
                  height: 12,
                  borderRadius: "var(--radius-chip)",
                  backgroundColor: deviceConnected
                    ? "var(--semantic-success)"
                    : connectionChecking
                      ? "var(--text-tertiary)"
                      : "var(--semantic-danger)",
                  boxShadow:
                    "inset 0 1px 0 color-mix(in srgb, var(--foreground) 25%, transparent)",
                }}
              />
              <Stack spacing={0.35} sx={{ minWidth: 0 }}>
                <Typography
                  variant="caption"
                  sx={{
                    fontWeight: 700,
                    fontSize: "var(--font-size-label)",
                    letterSpacing: "var(--letter-spacing-label)",
                    color: "var(--foreground)",
                    textTransform: "uppercase",
                  }}
                >
                  {t("device.pageTitle")}
                </Typography>
                <Typography
                  component="span"
                  sx={{
                    fontSize: "var(--font-size-caption)",
                    fontWeight: 600,
                    color: deviceConnected
                      ? "color-mix(in srgb, var(--semantic-success) 92%, var(--foreground))"
                      : connectionChecking
                        ? "var(--text-tertiary)"
                        : "color-mix(in srgb, var(--semantic-danger) 90%, var(--foreground))",
                    lineHeight: 1.25,
                  }}
                >
                  {deviceConnected
                    ? t("device.connected")
                    : connectionChecking
                      ? t("device.connecting")
                      : t("device.notConnected")}
                </Typography>
                {deviceConnected && baseUrl && (
                  <Typography
                    component="span"
                    sx={{
                      fontFamily: "var(--font-mono)",
                      fontSize: "var(--font-size-data-value)",
                      color: "var(--foreground-soft)",
                      lineHeight: 1.2,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {displayHost(baseUrl)}
                  </Typography>
                )}
              </Stack>
            </Box>

            <Box
              role="group"
              aria-label={t("nav.startMenu")}
              sx={{
                display: "grid",
                gridTemplateColumns: {
                  xs: "repeat(2, minmax(0, 1fr))",
                  sm: "repeat(3, minmax(0, 1fr))",
                },
                gap: 1.25,
              }}
            >
              {START_MENU_NAV_ITEMS.map(({ path, labelKey, iconSrc }) => {
                const active =
                  path === "/device-config"
                    ? pathname === "/device-config" ||
                      pathname.startsWith("/device-config/")
                    : path === "/soul-user"
                      ? pathname === "/soul-user" ||
                        pathname.startsWith("/soul-user/")
                      : pathname === path;
                const allowNav = canNavigate(path);
                const handleNavClick = (e: MouseEvent<HTMLElement>) => {
                  if (!allowNav) {
                    e.preventDefault();
                    const key = deviceHintReason ?? "unknown";
                    const now = Date.now();
                    const prev = lastNavBlockToastRef.current;
                    if (
                      prev?.key === key &&
                      now - prev.at < NAV_BLOCK_TOAST_COOLDOWN_MS
                    ) {
                      return;
                    }
                    lastNavBlockToastRef.current = { key, at: now };
                    showToast(t(getNavBlockedMessageKey(deviceHintReason)), {
                      variant: "warning",
                    });
                    return;
                  }
                  closeStart();
                  if (navBlocker?.attemptNavigate) {
                    e.preventDefault();
                    navBlocker.attemptNavigate(path);
                  }
                };
                const idleBg = "color-mix(in srgb, var(--card) 84%, var(--surface))";
                const activeBg = "color-mix(in srgb, var(--primary) 10%, var(--card))";

                return (
                  <Box
                    key={path}
                    component={
                      allowNav && !navBlocker?.attemptNavigate ? Link : "button"
                    }
                    {...(allowNav && !navBlocker?.attemptNavigate
                      ? { to: path }
                      : { type: "button" as const })}
                    role={allowNav ? "link" : "button"}
                    tabIndex={!allowNav ? -1 : 0}
                    aria-disabled={!allowNav ? true : undefined}
                    aria-current={active && allowNav ? "page" : undefined}
                    onClick={handleNavClick}
                    sx={{
                      minHeight: START_MENU_TILE_MIN_HEIGHT,
                      borderRadius: "calc(var(--radius-card) - 2px)",
                      border: "1px solid",
                      borderColor:
                        active && allowNav
                          ? "color-mix(in srgb, var(--primary) 28%, transparent)"
                          : "color-mix(in srgb, var(--border) 16%, transparent)",
                      p: 1.5,
                      display: "flex",
                      flexDirection: "column",
                      alignItems: "stretch",
                      justifyContent: "flex-start",
                      textAlign: "left",
                      gap: 1.125,
                      textDecoration: "none",
                      color: "inherit",
                      backgroundColor: active && allowNav ? activeBg : idleBg,
                      backgroundImage: [
                        "linear-gradient(180deg, color-mix(in srgb, #fff 14%, transparent) 0%, transparent 100%)",
                        "linear-gradient(135deg, color-mix(in srgb, var(--primary) 6%, transparent) 0%, transparent 100%)",
                      ].join(", "),
                      cursor: allowNav ? "pointer" : "default",
                      opacity: allowNav ? 1 : 0.72,
                      boxShadow: active && allowNav
                        ? "var(--os3d-selection-pill-stack)"
                        : "var(--os3d-control-soft-lift-stack)",
                      transition:
                        "background-color var(--transition-duration) var(--ease-out-smooth), transform var(--transition-duration) var(--ease-emphasized), border-color var(--transition-duration) ease, box-shadow var(--transition-duration) var(--ease-emphasized)",
                      "&:hover": allowNav
                        ? {
                            backgroundColor:
                              active && allowNav
                                ? "color-mix(in srgb, var(--primary) 12%, var(--card))"
                                : "color-mix(in srgb, var(--foreground) 4%, var(--card))",
                            transform: "translateY(-1px)",
                            boxShadow: "var(--os3d-chip-lift-stack)",
                          }
                        : {},
                      "&:active": allowNav
                        ? { transform: "translateY(0)" }
                        : {},
                      "@media (prefers-reduced-motion: reduce)": {
                        "&:hover": { transform: "none" },
                      },
                    }}
                  >
                    <Box
                      sx={{
                        color:
                          active && allowNav
                            ? "var(--primary)"
                            : "var(--foreground)",
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        width: 58,
                        height: 58,
                        borderRadius: "calc(var(--radius-card) - 6px)",
                        alignSelf: "flex-start",
                        bgcolor:
                          active && allowNav
                            ? "color-mix(in srgb, var(--primary) 10%, var(--card))"
                            : "color-mix(in srgb, var(--card) 74%, var(--surface))",
                        boxShadow:
                          active && allowNav
                            ? "var(--os3d-selection-pill-stack)"
                            : "var(--os3d-control-soft-lift-stack)",
                        "& svg, & img": {
                          width: "100%",
                          height: "100%",
                          objectFit: "contain",
                        },
                      }}
                    >
                      <Os3dIcon src={iconSrc} variant="tile" />
                    </Box>
                    <Typography
                      variant="caption"
                      sx={{
                        fontWeight: active ? 700 : 600,
                        lineHeight: 1.3,
                        fontSize: "var(--font-size-body-sm)",
                        mt: 0.25,
                        display: "-webkit-box",
                        WebkitLineClamp: 2,
                        WebkitBoxOrient: "vertical",
                        overflow: "hidden",
                        color: "var(--foreground)",
                      }}
                    >
                      {t(labelKey)}
                    </Typography>
                  </Box>
                );
              })}
            </Box>
          </Box>
        </Box>
      </Popover>

      <ConfirmDialog
        open={restartConfirmOpen}
        onClose={() => setRestartConfirmOpen(false)}
        title={t("device.restartConfirmTitle")}
        description={t("device.restartConfirmDesc")}
        icon={
          <Box
            sx={{
              width: 44,
              height: 44,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <Os3dIcon src={OS_ICON_SHELL.power} />
          </Box>
        }
        confirmLabel={t("device.restart")}
        onConfirm={doRestart}
        confirmDisabled={restarting}
        confirmColor="primary"
      />

      <Box
        sx={{
          position: "absolute",
          left: { sm: 92, md: 104 },
          right: { sm: 136, md: 220 },
          top: 0,
          bottom: 0,
          display: { xs: "none", sm: "flex" },
          justifyContent: "center",
          alignItems: "center",
          overflow: "visible",
          pointerEvents: "none",
          zIndex: 4,
        }}
      >
        <Stack
          direction="row"
          alignItems="center"
          spacing={1.75}
          onMouseLeave={() => setDockHoveredIndex(null)}
          sx={{
            width: "fit-content",
            maxWidth: "100%",
            overflow: "visible",
            px: { sm: 1, md: 2 },
            pointerEvents: "auto",
          }}
        >
          {NAV_ITEMS.map(({ path, labelKey, iconSrc }, index) => {
            const active =
              path === "/device-config"
                ? pathname === "/device-config" ||
                  pathname.startsWith("/device-config/")
                : path === "/soul-user"
                  ? pathname === "/soul-user" ||
                    pathname.startsWith("/soul-user/")
                  : pathname === path;
            const allowNav = canNavigate(path);
            const handlePinClick = (e: MouseEvent<HTMLElement>) => {
              if (!allowNav) {
                e.preventDefault();
                const key = deviceHintReason ?? "unknown";
                const now = Date.now();
                const prev = lastNavBlockToastRef.current;
                if (
                  prev?.key === key &&
                  now - prev.at < NAV_BLOCK_TOAST_COOLDOWN_MS
                ) {
                  return;
                }
                lastNavBlockToastRef.current = { key, at: now };
                showToast(t(getNavBlockedMessageKey(deviceHintReason)), {
                  variant: "warning",
                });
                return;
              }
              if (navBlocker?.attemptNavigate) {
                e.preventDefault();
                navBlocker.attemptNavigate(path);
              }
            };
            const dockMotion = getDockMotion(index, dockHoveredIndex);
            const dockTransform = allowNav
              ? `translateY(-${dockMotion.translateY}px) scale(${dockMotion.scale})`
              : "none";

            const button = (
              <IconButton
                component={
                  allowNav && !navBlocker?.attemptNavigate ? Link : "button"
                }
                to={allowNav && !navBlocker?.attemptNavigate ? path : undefined}
                size="small"
                onClick={handlePinClick}
                onMouseEnter={() => setDockHoveredIndex(index)}
                onFocus={() => setDockHoveredIndex(index)}
                onBlur={() =>
                  setDockHoveredIndex((current) =>
                    current === index ? null : current,
                  )
                }
                aria-label={t(labelKey)}
                aria-current={active ? "page" : undefined}
                sx={{
                  flexShrink: 0,
                  width: 46,
                  height: 46,
                  borderRadius: "calc(var(--radius-card) - 2px)",
                  color: active ? "var(--primary)" : "var(--foreground)",
                  position: "relative",
                  border:
                    "1px solid color-mix(in srgb, var(--border) 16%, transparent)",
                  backgroundColor:
                    active && allowNav
                      ? "color-mix(in srgb, var(--primary) 10%, var(--card))"
                      : "color-mix(in srgb, var(--card) 68%, transparent)",
                  transition:
                    "background-color 0.2s ease, border-color 0.2s ease, transform 0.24s cubic-bezier(0.22, 1, 0.36, 1), box-shadow 0.22s ease",
                  boxShadow:
                    active && allowNav
                      ? "var(--os3d-chip-lift-stack)"
                      : "var(--os3d-pedestal-lift-stack)",
                  transform: dockTransform,
                  zIndex: dockMotion.zIndex,
                  transformOrigin: "center bottom",
                  "&:hover": {
                    backgroundColor: allowNav
                      ? "color-mix(in srgb, var(--card) 82%, transparent)"
                      : "transparent",
                    borderColor: allowNav
                      ? "color-mix(in srgb, var(--primary) 26%, var(--border))"
                      : undefined,
                    boxShadow: allowNav
                      ? "var(--os3d-chip-lift-stack)"
                      : undefined,
                  },
                  "&:active": {
                    transform: allowNav ? "translateY(0) scale(0.98)" : "none",
                  },
                  "& svg, & img": {
                    width: "34px",
                    height: "34px",
                  },
                  "@media (prefers-reduced-motion: reduce)": {
                    transform: "none",
                    "&:hover": { transform: "none" },
                  },
                }}
              >
                <Os3dIcon src={iconSrc} variant="dock" />
                {active && allowNav ? (
                  <Box
                    aria-hidden
                    sx={{
                      position: "absolute",
                      bottom: -6,
                      left: "50%",
                      transform: "translateX(-50%)",
                      width: 4,
                      height: 4,
                      borderRadius: "50%",
                      backgroundColor: "var(--primary)",
                      boxShadow: "0 0 4px var(--primary)",
                    }}
                  />
                ) : null}
              </IconButton>
            );

            return (
              <Tooltip key={path} title={t(labelKey)} placement="top">
                {button}
              </Tooltip>
            );
          })}
        </Stack>
      </Box>

      <Stack
        direction="row"
        alignItems="center"
        spacing={0.75}
        sx={{
          alignSelf: "center",
          flexShrink: 0,
          pl: 0.5,
          maxWidth: { xs: 120, sm: 200 },
        }}
      >
        {onOpenSettings ? (
          <Tooltip title={t("settings.open")} placement="top">
            <IconButton
              size="small"
              onClick={onOpenSettings}
              aria-label={t("settings.open")}
              sx={{
                ...trayControlSurfaceSx,
                flexShrink: 0,
                width: TASKBAR_TRAY_CONTROL_SIZE,
                height: TASKBAR_TRAY_CONTROL_SIZE,
                color: "var(--foreground)",
                p: 0.375,
              }}
            >
              <Os3dIcon
                src={OS_ICON_SHELL.preferences}
                variant="inline"
                sx={{ width: 24, height: 24 }}
              />
            </IconButton>
          </Tooltip>
        ) : null}
        <Tooltip
          title={
            deviceConnected && baseUrl
              ? displayHost(baseUrl)
              : t("device.pageTitle")
          }
        >
          <Stack
            component="button"
            type="button"
            direction="row"
            alignItems="center"
            spacing={0.75}
            onClick={(e) => {
              e.preventDefault();
              if (navBlocker?.attemptNavigate) {
                navBlocker.attemptNavigate("/device");
              } else {
                navigate("/device");
              }
            }}
            aria-label={t("device.pageTitle")}
            sx={{
              ...trayControlSurfaceSx,
              px: 1.1,
              py: 0,
              minHeight: TASKBAR_TRAY_CONTROL_SIZE,
              minWidth: 0,
              maxWidth: "100%",
              cursor: "pointer",
              font: "inherit",
              color: "inherit",
            }}
          >
            <Box
              sx={{
                width: "var(--dot-size)",
                height: "var(--dot-size)",
                borderRadius: "50%",
                flexShrink: 0,
                backgroundColor: deviceConnected
                  ? "var(--semantic-success)"
                  : connectionChecking
                    ? "var(--text-tertiary)"
                    : "var(--semantic-danger)",
              }}
            />
            <Typography
              variant="caption"
              noWrap
              sx={{
                display: { xs: "none", sm: "block" },
                fontFamily: "var(--font-mono)",
                fontSize: "var(--font-size-caption)",
                color: "var(--text-tertiary)",
                lineHeight: 1,
              }}
            >
              {deviceConnected && baseUrl
                ? displayHost(baseUrl)
                : connectionChecking
                  ? t("device.connecting")
                  : t("device.notConnected")}
            </Typography>
          </Stack>
        </Tooltip>
      </Stack>
    </Box>
  );
}
