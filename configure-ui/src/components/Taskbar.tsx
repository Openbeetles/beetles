import {
  type KeyboardEvent as ReactKeyboardEvent,
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
import { useToast } from "../hooks/useToast";
import { setRestartPending } from "../store/deviceStatusStore";
import { SHELL_TASKBAR_CHROME_SX } from "../theme/shellChromeSurface";

/** 顶栏已有「连接设备」宽幅磁贴，网格内不再重复 `/device` */
const START_MENU_NAV_ITEMS = NAV_ITEMS.filter(
  (item) => item.path !== "/device",
);

/**
 * 12 列栅格占列数（Metro 式大小不一）；未列出新路由时默认 4 列。
 * Column spans for 12-col grid; default 4 for unlisted routes.
 */
const START_MENU_TILE_SPAN_BY_PATH: Partial<Record<string, number>> = {
  "/ai-config": 8,
  "/channels-config": 4,
  "/soul-user": 3,
  "/skills": 3,
  "/tools": 3,
  "/device-config": 3,
  "/system-logs": 6,
  "/system-config": 6,
};

function startMenuTileColSpan(path: string): number {
  return START_MENU_TILE_SPAN_BY_PATH[path] ?? 4;
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
export function Taskbar() {
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
  const [startHovered, setStartHovered] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [restartConfirmOpen, setRestartConfirmOpen] = useState(false);
  const startMenuPanelRef = useRef<HTMLDivElement | null>(null);
  const pathname = location.pathname;
  const canNavigate = (path: string) =>
    path === "/device" || (deviceConnected && !needDeviceHint);
  const startOpen = Boolean(startAnchor);

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
      showToast(res.error ?? t("device.restartFail"), { variant: "error" });
    }
  };

  const handleRestartClick = () => {
    if (!baseUrl?.trim() || !pairingCode?.trim() || restarting) return;
    setRestartConfirmOpen(true);
  };

  const handleStartClick = (e: MouseEvent<HTMLElement>) => {
    setStartAnchor(startOpen ? null : e.currentTarget);
  };

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
      sx={{
        flexShrink: 0,
        height: TASKBAR_HEIGHT,
        minHeight: TASKBAR_HEIGHT,
        display: "flex",
        alignItems: "center",
        px: { xs: 1.5, sm: 2 },
        gap: { xs: 1, sm: 1.25 },
        ...SHELL_TASKBAR_CHROME_SX,
        borderTop: "1px solid var(--border-subtle)",
        boxShadow: "none",
        position: "relative",
        zIndex: 2,
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
            flexShrink: 0,
            width: 44,
            height: 44,
            p: 0,
            borderRadius: "var(--radius-control)",
            margin: 0,
            cursor: "pointer",
            font: "inherit",
            display: "grid",
            placeItems: "center",
            border:
              "1px solid color-mix(in srgb, var(--primary) 38%, transparent)",
            backgroundColor:
              "color-mix(in srgb, var(--primary) 12%, var(--card))",
            boxShadow: startOpen
              ? "0 0 0 2px color-mix(in srgb, var(--primary) 35%, transparent)"
              : "none",
            transition:
              "background-color var(--transition-duration) var(--ease-emphasized), border-color var(--transition-duration) ease, transform var(--transition-duration) var(--ease-emphasized), box-shadow var(--transition-duration) var(--ease-emphasized)",
            "&:hover": {
              backgroundColor:
                "color-mix(in srgb, var(--primary) 18%, var(--card))",
              borderColor:
                "color-mix(in srgb, var(--primary) 48%, transparent)",
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
        anchorEl={startAnchor}
        onClose={closeStart}
        anchorOrigin={{ vertical: "top", horizontal: "center" }}
        transformOrigin={{ vertical: "bottom", horizontal: "center" }}
        /** 默认 16 会在贴边时把纸面往里推；与视口左缘对齐时需关闭 */
        marginThreshold={8}
        slotProps={{
          paper: {
            elevation: 0,
            sx: {
              width: "min(480px, 100vw - 32px)",
              maxHeight: "min(72vh, 560px)",
              display: "flex",
              flexDirection: "column",
              overflow: "hidden",
              borderRadius: "var(--radius-card)",
              border:
                "1px solid color-mix(in srgb, var(--border) 15%, transparent)",
              boxShadow:
                "0 16px 40px -10px color-mix(in srgb, var(--foreground) 20%, transparent), 0 0 0 1px color-mix(in srgb, var(--border) 10%, transparent)",
              backgroundColor:
                "color-mix(in srgb, var(--card) 88%, transparent)",
              backgroundImage: [
                "linear-gradient(180deg, color-mix(in srgb, var(--foreground) 5%, transparent) 0%, transparent 36%)",
                "linear-gradient(0deg, color-mix(in srgb, var(--foreground) 3%, transparent) 0%, transparent 28%)",
              ].join(", "),
              backdropFilter: "saturate(1.12) blur(var(--shell-chrome-blur))",
              WebkitBackdropFilter:
                "saturate(1.12) blur(var(--shell-chrome-blur))",
              mb: 0.5,
              "@media (prefers-reduced-motion: reduce)": {
                backdropFilter: "none",
                WebkitBackdropFilter: "none",
                backgroundColor: "var(--card)",
                backgroundImage: "none",
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
              py: 1.5,
              borderBottom: "1px solid var(--border-subtle)",
              background:
                "linear-gradient(180deg, color-mix(in srgb, var(--surface) 82%, var(--card)) 0%, color-mix(in srgb, var(--surface) 58%, var(--card)) 100%)",
              boxShadow:
                "inset 0 1px 0 color-mix(in srgb, var(--foreground) 8%, transparent)",
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
                    fontFamily: "var(--font-sans)",
                    fontWeight: 700,
                    letterSpacing: "-0.02em",
                    textTransform: "uppercase",
                    fontSize: "var(--font-size-h4)",
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
                        "&:hover:not(:disabled)": {
                          backgroundColor:
                            "color-mix(in srgb, var(--semantic-danger) 12%, transparent)",
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
                        <Os3dIcon src={OS_ICON_SHELL.power} />
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
                border: "1px solid var(--border-subtle)",
                borderRadius: 0,
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
                  ? "color-mix(in srgb, var(--semantic-success) 9%, var(--card))"
                  : "color-mix(in srgb, var(--semantic-danger) 9%, var(--card))",
                borderLeftWidth: "var(--accent-line-width)",
                borderLeftColor: deviceConnected
                  ? "var(--semantic-success)"
                  : "var(--semantic-danger)",
                transition:
                  "background-color var(--transition-duration) var(--ease-out-smooth), transform var(--transition-duration) var(--ease-emphasized)",
                "&:hover": {
                  backgroundColor: deviceConnected
                    ? "color-mix(in srgb, var(--semantic-success) 14%, var(--card))"
                    : "color-mix(in srgb, var(--semantic-danger) 14%, var(--card))",
                },
                "&:active": {
                  transform: "scale(0.992)",
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
                  xs: "1fr",
                  sm: "repeat(12, minmax(0, 1fr))",
                },
                gap: 1,
              }}
            >
              {START_MENU_NAV_ITEMS.map(({ path, labelKey, iconSrc }, index) => {
                const colSpan = startMenuTileColSpan(path);
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

                const idleBg =
                  index % 2 === 0
                    ? "var(--card)"
                    : "color-mix(in srgb, var(--foreground) 3.5%, var(--card))";
                const activeBg =
                  "color-mix(in srgb, var(--primary) 12%, var(--card))";

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
                      gridColumn: { xs: "1 / -1", sm: `span ${colSpan}` },
                      minHeight: {
                        xs: 80,
                        sm: colSpan >= 6 ? 96 : colSpan >= 4 ? 88 : 84,
                      },
                      borderRadius: "var(--radius-chip)",
                      border: "1px solid",
                      borderColor:
                        active && allowNav
                          ? "color-mix(in srgb, var(--primary) 42%, transparent)"
                          : "color-mix(in srgb, var(--border) 25%, transparent)",
                      p: 1.5,
                      display: "flex",
                      flexDirection: "column",
                      alignItems: "flex-start",
                      justifyContent: "space-between",
                      gap: 1,
                      textDecoration: "none",
                      color: "inherit",
                      backgroundColor: active && allowNav ? activeBg : idleBg,
                      cursor: allowNav ? "pointer" : "default",
                      opacity: allowNav ? 1 : 0.72,
                      transition:
                        "background-color var(--transition-duration) var(--ease-out-smooth), transform var(--transition-duration) var(--ease-emphasized), border-color var(--transition-duration) ease",
                      "&:hover": allowNav
                        ? {
                            backgroundColor:
                              active && allowNav
                                ? "color-mix(in srgb, var(--primary) 16%, var(--card))"
                                : "color-mix(in srgb, var(--foreground) 6%, var(--card))",
                            transform: "translateY(-2px)",
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
                            : "var(--foreground)", // 取消 mute 柔化，图标自身有色彩
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        width: 48,
                        height: 48,
                        "& svg, & img": {
                          width: "100%",
                          height: "100%",
                          objectFit: "contain",
                        },
                      }}
                    >
                        <Os3dIcon src={iconSrc} />
                    </Box>
                    <Typography
                      variant="caption"
                      sx={{
                        fontWeight: active ? 700 : 600,
                        lineHeight: 1.25,
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

      <Stack
        direction="row"
        alignItems="center"
        spacing={1.25}
        sx={{
          flex: 1,
          minWidth: 0,
          justifyContent: "center",
          overflowX: "auto",
          overflowY: "hidden",
          py: 0.5,
          /** 超窄屏不铺 9 枚快捷方式，避免挤作一团；用「开始」菜单导航 */
          display: { xs: "none", sm: "flex" },
          px: { sm: 1, md: 2 },
          scrollbarWidth: "thin",
          scrollPaddingInline: { sm: 8, md: 12 },
          "&::-webkit-scrollbar": { height: 6 },
        }}
      >
        {NAV_ITEMS.map(({ path, labelKey, iconSrc }) => {
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

          const button = (
            <IconButton
              component={
                allowNav && !navBlocker?.attemptNavigate ? Link : "button"
              }
              to={allowNav && !navBlocker?.attemptNavigate ? path : undefined}
              size="small"
              onClick={handlePinClick}
              aria-label={t(labelKey)}
              aria-current={active ? "page" : undefined}
              sx={{
                flexShrink: 0,
                width: 48, // 放大到 48x48，更符合 macOS Dock 图标的默认尺寸感
                height: 48,
                borderRadius: "var(--radius-card)",
                color: active ? "var(--primary)" : "var(--foreground)", // 图标默认全彩，不用 muted 降低存在感
                position: "relative",
                border: "1px solid transparent",
                backgroundColor:
                  active && allowNav
                    ? "color-mix(in srgb, var(--primary) 15%, transparent)"
                    : "transparent",
                transition:
                  "background-color 0.2s ease, border-color 0.2s ease, transform 0.3s cubic-bezier(0.34, 1.56, 0.64, 1), box-shadow 0.2s ease",
                boxShadow:
                  active && allowNav
                    ? "inset 0 1px 0 color-mix(in srgb, var(--foreground) 10%, transparent), 0 4px 8px color-mix(in srgb, var(--primary) 20%, transparent)" // 选中时带立体投影
                    : "none",
                "&:hover": {
                  backgroundColor: allowNav
                    ? "color-mix(in srgb, var(--foreground) 6%, transparent)"
                    : "transparent",
                  transform: allowNav ? "scale(1.15) translateY(-4px)" : "none", // 类似 macOS Hover 放大的动效
                  zIndex: 10,
                },
                "&:active": {
                  transform: "scale(0.95)",
                },
                "& svg, & img": {
                  width: "36px",
                  height: "36px",
                },
                "@media (prefers-reduced-motion: reduce)": {
                  "&:hover": { transform: "none" },
                },
              }}
            >
              <Os3dIcon src={iconSrc} />
              {active && allowNav ? (
                <Box
                  aria-hidden
                  sx={{
                    position: "absolute",
                    bottom: -6, // 小圆点在图标外下方
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

      <Stack
        direction="row"
        alignItems="center"
        spacing={0.75}
        sx={{
          flexShrink: 0,
          pl: 0.5,
          maxWidth: { xs: 120, sm: 200 },
        }}
      >
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
              border: "1px solid var(--border-subtle)",
              borderRadius: "var(--radius-chip)",
              px: 1,
              py: 0.5,
              minWidth: 0,
              maxWidth: "100%",
              backgroundColor: "var(--surface)",
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
