import {
  type MouseEvent,
  useContext,
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
import { BeetleIcon } from "./BeetleIcon";
import { NavBlockerContext } from "../contexts/NavBlockerContext";
import { NAV_ITEMS } from "../config/navItems";

/** 顶栏已有「连接设备」宽幅磁贴，网格内不再重复 `/device` */
const START_MENU_NAV_ITEMS = NAV_ITEMS.filter((item) => item.path !== "/device");

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
import { TASKBAR_HEIGHT } from "../config/layout";
import { useDevice } from "../hooks/useDevice";
import { useDeviceApi, type DeviceHintReason } from "../hooks/useDeviceApi";
import { useToast } from "../hooks/useToast";
import { SHELL_TASKBAR_CHROME_SX } from "../theme/shellChromeSurface";

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

/** Windows 风格任务栏：甲壳虫徽标为「开始」、中部固定快捷方式、右侧托盘（连接状态）。 */
export function Taskbar() {
  const { t } = useTranslation();
  const location = useLocation();
  const navigate = useNavigate();
  const navBlocker = useContext(NavBlockerContext);
  const { baseUrl } = useDevice();
  const {
    deviceConnected,
    connectionChecking,
    needDeviceHint,
    deviceHintReason,
  } = useDeviceApi();
  const { showToast } = useToast();
  const lastNavBlockToastRef = useRef<{ key: string; at: number } | null>(
    null,
  );
  const [startAnchor, setStartAnchor] = useState<HTMLElement | null>(null);
  const pathname = location.pathname;
  const canNavigate = (path: string) =>
    path === "/device" || (deviceConnected && !needDeviceHint);
  const startOpen = Boolean(startAnchor);

  const closeStart = () => setStartAnchor(null);

  const handleStartClick = (e: MouseEvent<HTMLElement>) => {
    setStartAnchor(startOpen ? null : e.currentTarget);
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
            border: "1px solid color-mix(in srgb, var(--primary) 38%, transparent)",
            backgroundColor: "color-mix(in srgb, var(--primary) 12%, var(--card))",
            boxShadow: startOpen
              ? "0 0 0 2px color-mix(in srgb, var(--primary) 35%, transparent)"
              : "none",
            transition:
              "background-color var(--transition-duration) var(--ease-emphasized), border-color var(--transition-duration) ease, transform var(--transition-duration) var(--ease-emphasized), box-shadow var(--transition-duration) var(--ease-emphasized)",
            "&:hover": {
              backgroundColor: "color-mix(in srgb, var(--primary) 18%, var(--card))",
              borderColor: "color-mix(in srgb, var(--primary) 48%, transparent)",
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
        anchorOrigin={{ vertical: "top", horizontal: "left" }}
        transformOrigin={{ vertical: "bottom", horizontal: "left" }}
        /** 默认 16 会在贴边时把纸面往里推；与视口左缘对齐时需关闭 */
        marginThreshold={0}
        slotProps={{
          paper: {
            elevation: 0,
            sx: {
              /**
               * 任务栏容器有 px（与按钮左缘锚点错开）；负 margin 让纸面左缘与视口左缘对齐（类 Windows 开始菜单）。
               * Negate taskbar horizontal padding so flyout left aligns with viewport, not padded rail.
               */
              ml: { xs: -1.5, sm: -2 },
              width: "min(480px, 100vw)",
              maxHeight: "min(72vh, 560px)",
              display: "flex",
              flexDirection: "column",
              overflow: "hidden",
              borderRadius: 0,
              border: "none",
              boxShadow: "none",
              backgroundColor: "color-mix(in srgb, var(--card) 88%, transparent)",
              backgroundImage: [
                "linear-gradient(180deg, color-mix(in srgb, var(--foreground) 5%, transparent) 0%, transparent 36%)",
                "linear-gradient(0deg, color-mix(in srgb, var(--foreground) 3%, transparent) 0%, transparent 28%)",
              ].join(", "),
              backdropFilter: "saturate(1.12) blur(var(--shell-chrome-blur))",
              WebkitBackdropFilter: "saturate(1.12) blur(var(--shell-chrome-blur))",
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
          sx={{
            flexShrink: 0,
            px: 2,
            py: 1.5,
            borderBottom: "1px solid var(--border-subtle)",
            background:
              "linear-gradient(180deg, color-mix(in srgb, var(--surface) 82%, var(--card)) 0%, color-mix(in srgb, var(--surface) 58%, var(--card)) 100%)",
            boxShadow:
              "inset 0 1px 0 color-mix(in srgb, var(--foreground) 8%, transparent)",
          }}
        >
          <Stack direction="row" alignItems="center" spacing={1.25}>
            <BeetleIcon
              aria-hidden
              sx={{
                width: "var(--icon-container-md)",
                height: "var(--icon-container-md)",
                borderRadius: 0,
              }}
            />
            <Stack spacing={0.25} sx={{ minWidth: 0 }}>
              <Typography
                variant="subtitle2"
                sx={{
                  fontFamily: "var(--font-display)",
                  fontWeight: 700,
                  letterSpacing: "-0.02em",
                  textTransform: "uppercase",
                  fontSize: "var(--font-size-body-sm)",
                }}
              >
                {t("app.name")}
              </Typography>
              <Typography
                variant="caption"
                sx={{ color: "var(--muted)", fontSize: "var(--font-size-caption)" }}
              >
                {t("app.tagline")}
              </Typography>
            </Stack>
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
                    ? "var(--muted)"
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
                      ? "var(--muted)"
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
            {START_MENU_NAV_ITEMS.map(({ path, labelKey, icon }, index) => {
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
                      sm:
                        colSpan >= 6 ? 96 : colSpan >= 4 ? 88 : 84,
                    },
                    borderRadius: 0,
                    border: "1px solid",
                    borderColor:
                      active && allowNav
                        ? "color-mix(in srgb, var(--primary) 42%, transparent)"
                        : "var(--border-subtle)",
                    p: 1.25,
                    display: "flex",
                    flexDirection: "column",
                    alignItems: "flex-start",
                    justifyContent: "space-between",
                    gap: 0.75,
                    textDecoration: "none",
                    color: "inherit",
                    backgroundColor:
                      active && allowNav ? activeBg : idleBg,
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
                          transform: "translateY(-1px)",
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
                          : "var(--foreground-soft)",
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                      "& svg": {
                        fontSize: "var(--icon-size-lg)",
                      },
                    }}
                  >
                    {icon}
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
      </Popover>

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
        {NAV_ITEMS.map(({ path, labelKey, icon }) => {
          const active =
            path === "/device-config"
              ? pathname === "/device-config" ||
                pathname.startsWith("/device-config/")
              : path === "/soul-user"
                ? pathname === "/soul-user" || pathname.startsWith("/soul-user/")
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
                width: 40,
                height: 40,
                borderRadius: "var(--radius-control)",
                color: active ? "var(--primary)" : "var(--foreground-soft)",
                position: "relative",
                border:
                  active && allowNav
                    ? "1px solid color-mix(in srgb, var(--primary) 35%, transparent)"
                    : "1px solid transparent",
                backgroundColor:
                  active && allowNav
                    ? "color-mix(in srgb, var(--primary) 12%, transparent)"
                    : "transparent",
                transition:
                  "background-color var(--transition-duration) var(--ease-out-smooth), border-color var(--transition-duration) var(--ease-out-smooth), transform var(--transition-duration) var(--ease-emphasized), box-shadow var(--transition-duration) var(--ease-out-smooth)",
                boxShadow:
                  active && allowNav
                    ? "inset 0 1px 0 color-mix(in srgb, var(--foreground) 6%, transparent)"
                    : "none",
                "&:hover": {
                  backgroundColor: allowNav
                    ? "color-mix(in srgb, var(--foreground) 8%, transparent)"
                    : "transparent",
                  transform: allowNav ? "translateY(-2px)" : "none",
                },
                "&:active": {
                  transform: "translateY(0)",
                },
                "@media (prefers-reduced-motion: reduce)": {
                  "&:hover": { transform: "none" },
                },
              }}
            >
              {icon}
              {active && allowNav ? (
                <Box
                  aria-hidden
                  sx={{
                    position: "absolute",
                    bottom: 4,
                    left: "50%",
                    transform: "translateX(-50%)",
                    width: 14,
                    height: 2,
                    borderRadius: 1,
                    backgroundColor: "var(--primary)",
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
                    ? "var(--muted)"
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
                color: "var(--muted)",
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
