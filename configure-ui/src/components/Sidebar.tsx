import {
  type MouseEvent,
  type ReactElement,
  useContext,
  useRef,
} from "react";
import Box from "@mui/material/Box";
import List from "@mui/material/List";
import ListItemButton from "@mui/material/ListItemButton";
import ListItemIcon from "@mui/material/ListItemIcon";
import ListItemText from "@mui/material/ListItemText";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import ChatBubbleOutlineRounded from "@mui/icons-material/ChatBubbleOutlineRounded";
import ExtensionOutlined from "@mui/icons-material/ExtensionOutlined";
import HandymanOutlined from "@mui/icons-material/HandymanOutlined";
import HistoryOutlined from "@mui/icons-material/HistoryOutlined";
import LinkRounded from "@mui/icons-material/LinkRounded";
import PaletteOutlined from "@mui/icons-material/PaletteOutlined";
import SettingsOutlined from "@mui/icons-material/SettingsOutlined";
import DevicesOtherOutlined from "@mui/icons-material/DevicesOtherOutlined";
import SmartToyOutlined from "@mui/icons-material/SmartToyOutlined";
import { BeetleIcon } from "./BeetleIcon";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";
import { NavBlockerContext } from "../contexts/NavBlockerContext";
import { SIDEBAR_WIDTH_EXPANDED, TOP_BAR_MIN_HEIGHT } from "../config/layout";
import { useDevice } from "../hooks/useDevice";
import { useDeviceApi, type DeviceHintReason } from "../hooks/useDeviceApi";
import { useToast } from "../hooks/useToast";
import { sidebarNavSelectedPcbOverlaySx } from "../theme/pcbSurface";
import { SHELL_CHROME_SURFACE_SX } from "../theme/shellChromeSurface";

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

/** 同一阻断原因下避免连点导航重复 Toast */
const NAV_BLOCK_TOAST_COOLDOWN_MS = 2500;

const NAV_ITEMS: { path: string; labelKey: string; icon: ReactElement }[] = [
  { path: "/device", labelKey: "nav.device", icon: <LinkRounded /> },
  { path: "/ai-config", labelKey: "nav.aiConfig", icon: <SmartToyOutlined /> },
  {
    path: "/channels-config",
    labelKey: "nav.channelsConfig",
    icon: <ChatBubbleOutlineRounded />,
  },
  { path: "/soul-user", labelKey: "nav.soulUser", icon: <PaletteOutlined /> },
  { path: "/skills", labelKey: "nav.skills", icon: <ExtensionOutlined /> },
  { path: "/tools", labelKey: "nav.tools", icon: <HandymanOutlined /> },
  {
    path: "/device-config",
    labelKey: "nav.deviceConfig",
    icon: <DevicesOtherOutlined />,
  },
  {
    path: "/system-logs",
    labelKey: "nav.systemLogs",
    icon: <HistoryOutlined />,
  },
  {
    path: "/system-config",
    labelKey: "nav.systemConfig",
    icon: <SettingsOutlined />,
  },
];

interface SidebarProps {
  /** 抽屉模式时宽度 100% */
  drawer?: boolean;
}

export function Sidebar({ drawer }: SidebarProps) {
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
  const pathname = location.pathname;
  /** 仅在设备已连接且无需提示时允许跳转；设备页始终可点。checking/unreachable/未激活/无配对码均禁用 */
  const canNavigate = (path: string) =>
    path === "/device" || (deviceConnected && !needDeviceHint);
  const width = drawer ? "100%" : SIDEBAR_WIDTH_EXPANDED;

  return (
    <Box
      component="nav"
      sx={{
        width,
        flexShrink: 0,
        height: "100vh",
        display: "flex",
        flexDirection: "column",
        ...SHELL_CHROME_SURFACE_SX,
        ...(!drawer && {
          borderRight:
            "1px solid color-mix(in srgb, var(--border) 38%, transparent)",
        }),
        transition:
          "width var(--transition-duration-emphasized) var(--ease-emphasized)",
      }}
    >
      <Stack
        component="button"
        type="button"
        direction="row"
        alignItems="center"
        spacing={1.5}
        onClick={(e) => {
          e.preventDefault();
          if (navBlocker?.attemptNavigate) {
            navBlocker.attemptNavigate("/");
          } else {
            navigate("/");
          }
        }}
        aria-label={t("nav.brandHome")}
        sx={{
          minHeight: TOP_BAR_MIN_HEIGHT,
          px: drawer ? 2 : 2,
          border: 0,
          margin: 0,
          background: "none",
          cursor: "pointer",
          font: "inherit",
          textAlign: "inherit",
          textDecoration: "none",
          color: "inherit",
          width: "100%",
          transition:
            "padding var(--transition-duration) ease, opacity var(--transition-duration) ease",
          "&:hover": { opacity: 0.9 },
        }}
      >
        <BeetleIcon
          aria-hidden
          sx={{
            width: "var(--icon-container-md)",
            height: "var(--icon-container-md)",
            borderRadius: "var(--radius-control)",
          }}
        />
        <Typography
          component="span"
          variant="h6"
          sx={{
            fontFamily: "var(--font-display)",
            fontSize: "var(--font-size-body)",
            fontWeight: 600,
            letterSpacing: "-0.02em",
            color: "var(--foreground)",
            lineHeight: "var(--line-height-tight)",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            textTransform: "uppercase",
          }}
        >
          {t("app.name")}
        </Typography>
      </Stack>

      <Box
        sx={{
          mx: drawer ? 1.5 : 1.5,
          px: drawer ? 1.5 : 1.5,
          py: 1.25,
          borderRadius: "var(--radius-control)",
          border: "1px solid var(--border-subtle)",
          backgroundColor: baseUrl
            ? "var(--surface)"
            : "color-mix(in srgb, var(--semantic-danger) 5%, var(--card))",
          borderLeftWidth: "var(--accent-line-width)",
          borderLeftColor: baseUrl
            ? "var(--semantic-success)"
            : "var(--semantic-danger)",
          display: "flex",
          alignItems: "center",
          gap: 0.75,
          transition:
            "border-color var(--transition-duration) ease, background-color var(--transition-duration) ease",
        }}
      >
        <Stack
          component="button"
          type="button"
          direction="row"
          alignItems="center"
          spacing={1.25}
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
            flex: 1,
            minWidth: 0,
            border: 0,
            margin: 0,
            background: "none",
            cursor: "pointer",
            font: "inherit",
            textAlign: "left",
            textDecoration: "none",
            color: "inherit",
            "&:hover": {
              backgroundColor: "transparent",
            },
            "&:focus-visible": {
              outline:
                "var(--focus-ring-width) solid color-mix(in srgb, var(--primary) 55%, transparent)",
              outlineOffset: "var(--focus-ring-offset)",
              borderRadius: "var(--radius-control)",
            },
          }}
        >
          <Box
            sx={{
              width: "var(--dot-size)",
              height: "var(--dot-size)",
              borderRadius: "50%",
              backgroundColor: baseUrl
                ? "var(--semantic-success)"
                : "var(--semantic-danger)",
              flexShrink: 0,
            }}
          />
          <Stack
            direction="column"
            spacing={0.25}
            sx={{ minWidth: 0, flex: 1 }}
          >
            <Typography
              component="span"
              sx={{
                fontSize: "var(--font-size-caption)",
                fontWeight: 600,
                color: deviceConnected
                  ? "var(--semantic-success)"
                  : connectionChecking
                    ? "var(--muted)"
                    : "var(--semantic-danger)",
                lineHeight: 1.2,
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
                  color: "var(--muted)",
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
        </Stack>
      </Box>

      <List sx={{ flex: 1, py: 1, px: drawer ? 1 : 1 }}>
        {NAV_ITEMS.map(({ path, labelKey, icon }) => {
          const active =
            path === "/device-config"
              ? pathname === "/device-config" ||
                pathname.startsWith("/device-config/")
              : path === "/soul-user"
                ? pathname === "/soul-user" ||
                  pathname.startsWith("/soul-user/")
                : pathname === path;
          const allowNav = canNavigate(path);
          const navSelectedBaseGrad =
            "linear-gradient(165deg, color-mix(in srgb, var(--primary) 9%, var(--surface)) 0%, color-mix(in srgb, var(--primary) 16%, var(--card)) 100%)";
          const navSelectedHoverGrad =
            "linear-gradient(165deg, color-mix(in srgb, var(--primary) 12%, var(--surface)) 0%, color-mix(in srgb, var(--primary) 22%, var(--card)) 100%)";
          const navSelectedPcbShared = {
            borderRadius: "var(--radius-control)",
            color: "var(--primary)",
            position: "relative" as const,
            overflow: "hidden" as const,
            minHeight: 56,
            backgroundColor: "transparent",
            border:
              "1px solid color-mix(in srgb, var(--primary) 26%, transparent)",
            boxShadow: [
              "inset 0 1px 0 color-mix(in srgb, var(--foreground) 10%, transparent)",
              "inset 0 -1px 0 color-mix(in srgb, var(--foreground) 7%, transparent)",
            ].join(", "),
            "& .MuiListItemIcon-root": {
              color: "var(--primary)",
              position: "relative",
              zIndex: 1,
            },
            "& .MuiListItemText-root": { position: "relative", zIndex: 1 },
          };
          const navSelectedHardware = {
            ...navSelectedPcbShared,
            backgroundImage: navSelectedBaseGrad,
            backgroundSize: "100% 100%",
            backgroundRepeat: "no-repeat",
          };
          const navSelectedHardwareHover = {
            ...navSelectedPcbShared,
            backgroundImage: navSelectedHoverGrad,
            backgroundSize: "100% 100%",
            backgroundRepeat: "no-repeat",
            borderColor: "color-mix(in srgb, var(--primary) 36%, transparent)",
          };
          const listItemSx = {
            borderRadius: "var(--radius-control)",
            mx: drawer ? 0.5 : 1,
            mb: 0.5,
            ...(active && allowNav
              ? {
                  "&.Mui-selected": navSelectedHardware,
                  "&.Mui-selected:hover": navSelectedHardwareHover,
                }
              : {}),
            ...(!allowNav && {
              opacity: 0.75,
              cursor: "default",
            }),
            transition:
              "background var(--transition-duration) ease, background-color var(--transition-duration) ease, background-image var(--transition-duration) ease, color var(--transition-duration) ease, box-shadow var(--transition-duration) ease, border-color var(--transition-duration) ease",
            "&:hover":
              allowNav && !(active && allowNav)
                ? { backgroundColor: "var(--card)" }
                : !allowNav
                  ? { backgroundColor: "transparent" }
                  : {},
          };
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
            if (navBlocker?.attemptNavigate) {
              e.preventDefault();
              navBlocker.attemptNavigate(path);
            }
          };

          return (
            <ListItemButton
              key={path}
              component={
                allowNav && !navBlocker?.attemptNavigate ? Link : "div"
              }
              to={allowNav && !navBlocker?.attemptNavigate ? path : undefined}
              selected={active && allowNav}
              role={allowNav ? "link" : "button"}
              tabIndex={!allowNav ? -1 : 0}
              aria-disabled={!allowNav ? true : undefined}
              onClick={handleNavClick}
              sx={listItemSx}
            >
              {active && allowNav ? (
                <Box
                  aria-hidden
                  sx={{
                    position: "absolute",
                    inset: 0,
                    borderRadius: "inherit",
                    zIndex: 0,
                    pointerEvents: "none",
                    overflow: "hidden",
                    ...sidebarNavSelectedPcbOverlaySx(),
                  }}
                />
              ) : null}
              <ListItemIcon
                sx={{
                  minWidth: 40,
                  color: "inherit",
                }}
              >
                {icon}
              </ListItemIcon>
              <ListItemText
                primary={t(labelKey)}
                slotProps={{
                  primary: {
                    sx: {
                      fontSize: "var(--font-size-body-sm)",
                      fontWeight: active ? 700 : 600,
                    },
                  },
                }}
              />
            </ListItemButton>
          );
        })}
      </List>
    </Box>
  );
}
