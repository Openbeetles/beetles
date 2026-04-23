import { useContext, useState } from "react";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";
import { BeetleIcon } from "./BeetleIcon";
import { Os3dIcon } from "./Os3dIcon";
import { PageHeader } from "./PageHeader";
import { NavBlockerContext } from "../contexts/NavBlockerContext";
import { TOP_BAR_MIN_HEIGHT } from "../config/layout";
import { NAV_ITEMS } from "../config/navItems";
import { SHELL_TITLEBAR_CHROME_SX } from "../theme/shellChromeSurface";
import {
  isMacTauriWindow,
  isTauriRuntime,
} from "../runtime/desktopEnvironment";

const NAV_ICON_BY_PATH = Object.fromEntries(
  NAV_ITEMS.map((item) => [item.path, item.iconSrc]),
) as Record<string, string>;

const PATH_TO_META: Record<string, { titleKey: string; iconSrc: string }> = {
  "/device": {
    titleKey: "device.pageTitle",
    iconSrc: NAV_ICON_BY_PATH["/device"],
  },
  "/device-config": {
    titleKey: "deviceConfig.pageTitle",
    iconSrc: NAV_ICON_BY_PATH["/device-config"],
  },
  "/ai-config": {
    titleKey: "aiConfig.pageTitle",
    iconSrc: NAV_ICON_BY_PATH["/ai-config"],
  },
  "/channels-config": {
    titleKey: "channelsConfig.pageTitle",
    iconSrc: NAV_ICON_BY_PATH["/channels-config"],
  },
  "/system-config": {
    titleKey: "systemConfig.pageTitle",
    iconSrc: NAV_ICON_BY_PATH["/system-config"],
  },
  "/system-logs": {
    titleKey: "systemLogs.pageTitle",
    iconSrc: NAV_ICON_BY_PATH["/system-logs"],
  },
  "/skills": {
    titleKey: "skills.pageTitle",
    iconSrc: NAV_ICON_BY_PATH["/skills"],
  },
  "/tools": {
    titleKey: "tools.pageTitle",
    iconSrc: NAV_ICON_BY_PATH["/tools"],
  },
  "/accounts": {
    titleKey: "accounts.pageTitle",
    iconSrc: NAV_ICON_BY_PATH["/accounts"],
  },
};

function metaForPathname(pathname: string) {
  if (pathname.startsWith("/device-config")) {
    return PATH_TO_META["/device-config"];
  }
  return PATH_TO_META[pathname];
}

/** 顶栏：与底部任务栏配套的「窗口标题栏」——左侧窗口图标、右侧标题栏按钮区。 */
export function TopBar() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const navBlocker = useContext(NavBlockerContext);
  const location = useLocation();
  const [brandIconHovered, setBrandIconHovered] = useState(false);
  const desktopShellWindow = isTauriRuntime();
  const macTauriWindow = isMacTauriWindow();
  const titlebarMinHeight = macTauriWindow
    ? 40
    : desktopShellWindow
      ? 48
      : TOP_BAR_MIN_HEIGHT;
  const titlebarPaddingTop = 0;
  const chromeControlPadding = desktopShellWindow ? 0.25 : 0.625;
  const chromeLabelPaddingY = desktopShellWindow ? 0 : 0.625;
  const chromeRowPaddingY = desktopShellWindow ? 0 : 0.5;
  const chromeRowSpacing = desktopShellWindow ? 0.75 : 1.25;
  const brandIconSize = macTauriWindow
    ? 16
    : desktopShellWindow
      ? 18
      : "var(--icon-size-lg)";
  const desktopTitleRailHeight = macTauriWindow ? 28 : titlebarMinHeight;
  const desktopTitleRailOffsetTop = macTauriWindow ? 4 : 0;
  const desktopTitleIconObjectPosition = macTauriWindow
    ? "50% 43%"
    : undefined;

  const pathname = location.pathname;
  const meta = metaForPathname(pathname);
  const title = meta ? t(meta.titleKey) : pathname;

  const handleWindowIconClick = () => {
    if (navBlocker?.attemptNavigate) {
      navBlocker.attemptNavigate("/");
    } else {
      navigate("/");
    }
  };

  const chromeCapsuleSx = {
    border: "1px solid color-mix(in srgb, var(--border) 18%, transparent)",
    backgroundColor: "color-mix(in srgb, var(--card) 72%, transparent)",
    backgroundImage:
      "linear-gradient(180deg, color-mix(in srgb, #fff 12%, transparent) 0%, transparent 100%)",
    boxShadow: "var(--os3d-breadcrumb-lift-stack)",
  } as const;

  if (macTauriWindow) {
    return (
      <Box
        component="header"
        sx={{
          flexShrink: 0,
          minHeight: titlebarMinHeight,
          backgroundColor: "transparent",
          backgroundImage: "none",
          borderBottom: "none",
        }}
        data-tauri-drag-region=""
      />
    );
  }

  return (
    <Box
      component="header"
      sx={{
        flexShrink: 0,
        minHeight: titlebarMinHeight,
        display: "flex",
        alignItems: "stretch",
        justifyContent: "flex-start",
        pl: desktopShellWindow ? "16px" : { xs: 2, sm: 3 },
        pr: desktopShellWindow ? "16px" : { xs: 2, sm: 3 },
        pt: titlebarPaddingTop,
        position: "relative",
        ...SHELL_TITLEBAR_CHROME_SX,
        borderBottom: "1px solid color-mix(in srgb, var(--border) 14%, transparent)",
        gap: 0,
      }}
    >
      <Stack
        direction="row"
        alignItems="center"
        spacing={chromeRowSpacing}
        sx={{
          minWidth: 0,
          flex: 1,
          py: chromeRowPaddingY,
          pr: 1,
          ...(desktopShellWindow
            ? {
                alignSelf: "flex-start",
                minHeight: desktopTitleRailHeight,
                mt: `${desktopTitleRailOffsetTop}px`,
              }
            : null),
        }}
      >
        {desktopShellWindow ? (
          <Tooltip title={title}>
            <Box
              sx={{
                flexShrink: 0,
                width: brandIconSize,
                height: brandIconSize,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
              }}
            >
              {meta?.iconSrc ? (
                <Os3dIcon
                  src={meta.iconSrc}
                  variant="titlebar"
                  sx={{
                    width: brandIconSize,
                    height: brandIconSize,
                    objectPosition: desktopTitleIconObjectPosition,
                  }}
                />
              ) : null}
            </Box>
          </Tooltip>
        ) : (
          <Tooltip title={t("nav.brandHome")}>
            <IconButton
              size="small"
              onClick={handleWindowIconClick}
              onMouseEnter={() => setBrandIconHovered(true)}
              onMouseLeave={() => setBrandIconHovered(false)}
              aria-label={t("nav.brandHome")}
              sx={{
                flexShrink: 0,
                p: chromeControlPadding,
                borderRadius: "var(--radius-control)",
                ...chromeCapsuleSx,
                color: "var(--foreground-soft)",
                transition:
                  "background-color var(--transition-duration) var(--ease-out-smooth), box-shadow var(--transition-duration) var(--ease-out-smooth), transform var(--transition-duration) var(--ease-emphasized)",
                "&:hover": {
                  backgroundColor:
                    "color-mix(in srgb, var(--primary) 8%, var(--card))",
                  boxShadow: "var(--os3d-selection-pill-stack)",
                  transform: "translateY(-0.5px)",
                },
                "&:active": {
                  transform: "translateY(0)",
                },
                "@media (prefers-reduced-motion: reduce)": {
                  "&:hover": { transform: "none" },
                  transition: "none",
                },
              }}
            >
              <BeetleIcon
                aria-hidden
                animationActive={brandIconHovered}
                sx={{
                  width: "var(--icon-size-lg)",
                  height: "var(--icon-size-lg)",
                  borderRadius: "calc(var(--radius-control) - 2px)",
                }}
              />
            </IconButton>
          </Tooltip>
        )}
        {desktopShellWindow ? (
          <Box sx={{ flex: 1 }} />
        ) : (
          <Box
            sx={{ minWidth: 0, flex: 1, display: "flex", alignItems: "center" }}
          >
            <Box
              sx={{
                display: "flex",
                alignItems: "center",
                minWidth: 0,
                width: "fit-content",
                maxWidth: "min(100%, 420px)",
                px: { xs: 0.875, sm: 1 },
                py: desktopShellWindow ? chromeLabelPaddingY : 0.625,
                borderRadius: "calc(var(--radius-control) + 2px)",
                ...chromeCapsuleSx,
                border:
                  "1px solid color-mix(in srgb, var(--border) 16%, transparent)",
                backgroundColor:
                  "color-mix(in srgb, var(--surface) 78%, var(--card))",
                backgroundImage: [
                  "linear-gradient(180deg, color-mix(in srgb, #fff 14%, transparent) 0%, transparent 100%)",
                  "linear-gradient(90deg, color-mix(in srgb, var(--primary) 4%, transparent) 0%, transparent 28%)",
                ].join(", "),
                boxShadow: "var(--os3d-control-soft-lift-stack)",
              }}
            >
              <PageHeader title={title} brandLabel={t("app.name")} />
            </Box>
          </Box>
        )}
      </Stack>
    </Box>
  );
}
