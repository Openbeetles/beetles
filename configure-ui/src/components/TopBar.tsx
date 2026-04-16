import { useContext, useState } from "react";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";
import { BeetleIcon } from "./BeetleIcon";
import { PageHeader } from "./PageHeader";
import { NavBlockerContext } from "../contexts/NavBlockerContext";
import { TOP_BAR_MIN_HEIGHT } from "../config/layout";
import { SHELL_TITLEBAR_CHROME_SX } from "../theme/shellChromeSurface";
import { OS_ICON_SHELL } from "../config/osIcons";
import { Os3dIcon } from "./Os3dIcon";
import { isMacTauriWindow } from "../runtime/desktopEnvironment";

const PATH_TO_META: Record<string, { titleKey: string }> = {
  "/device": { titleKey: "device.pageTitle" },
  "/device-config": {
    titleKey: "deviceConfig.pageTitle",
  },
  "/ai-config": {
    titleKey: "aiConfig.pageTitle",
  },
  "/channels-config": {
    titleKey: "channelsConfig.pageTitle",
  },
  "/system-config": {
    titleKey: "systemConfig.pageTitle",
  },
  "/system-logs": {
    titleKey: "systemLogs.pageTitle",
  },
  "/soul-user": {
    titleKey: "soulUser.pageTitle",
  },
  "/skills": { titleKey: "skills.pageTitle" },
  "/tools": { titleKey: "tools.pageTitle" },
  "/accounts": { titleKey: "accounts.pageTitle" },
};

function metaForPathname(pathname: string) {
  if (pathname.startsWith("/device-config")) {
    return PATH_TO_META["/device-config"];
  }
  if (pathname.startsWith("/soul-user")) {
    return PATH_TO_META["/soul-user"];
  }
  return PATH_TO_META[pathname];
}

interface TopBarProps {
  onOpenSettings?: () => void;
}

/** 顶栏：与底部任务栏配套的「窗口标题栏」——左侧窗口图标、右侧标题栏按钮区。 */
export function TopBar({ onOpenSettings }: TopBarProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const navBlocker = useContext(NavBlockerContext);
  const location = useLocation();
  const [brandIconHovered, setBrandIconHovered] = useState(false);
  const macTauriWindow = isMacTauriWindow();

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

  const captionBtnSx = {
    flexShrink: 0,
    width: 48,
    height: TOP_BAR_MIN_HEIGHT,
    maxHeight: TOP_BAR_MIN_HEIGHT,
    borderRadius: 0,
    border: "none",
    color: "var(--foreground-soft)",
    backgroundColor: "transparent",
    transition:
      "background-color var(--transition-duration) var(--ease-out-smooth), color var(--transition-duration) var(--ease-out-smooth), box-shadow var(--transition-duration) var(--ease-emphasized)",
    "&:hover:not(:disabled)": {
      backgroundColor: "color-mix(in srgb, var(--card) 54%, transparent)",
      color: "var(--foreground)",
      boxShadow: "none",
    },
  } as const;

  return (
    <Box
      component="header"
      sx={{
        flexShrink: 0,
        minHeight: macTauriWindow ? TOP_BAR_MIN_HEIGHT + 28 : TOP_BAR_MIN_HEIGHT,
        display: "flex",
        alignItems: "stretch",
        justifyContent: "space-between",
        pl: { xs: 2, sm: 3 },
        pr: 0,
        pt: macTauriWindow ? 3.5 : 0,
        position: "relative",
        ...SHELL_TITLEBAR_CHROME_SX,
        borderBottom: "1px solid color-mix(in srgb, var(--border) 10%, transparent)",
        gap: 0,
      }}
      data-tauri-drag-region={macTauriWindow ? "" : undefined}
    >
      <Stack
        direction="row"
        alignItems="center"
        spacing={1.25}
        sx={{ minWidth: 0, flex: 1, py: 0.5, pr: 1 }}
      >
        <Tooltip title={t("nav.brandHome")}>
          <IconButton
            size="small"
            onClick={handleWindowIconClick}
            onMouseEnter={() => setBrandIconHovered(true)}
            onMouseLeave={() => setBrandIconHovered(false)}
            aria-label={t("nav.brandHome")}
            sx={{
              flexShrink: 0,
              p: 0.625,
              borderRadius: "var(--radius-control)",
              border: "1px solid color-mix(in srgb, var(--border) 12%, transparent)",
              backgroundColor:
                "color-mix(in srgb, var(--card) 62%, var(--surface))",
              boxShadow: "none",
              transition:
                "background-color var(--transition-duration) var(--ease-out-smooth), box-shadow var(--transition-duration) var(--ease-out-smooth), transform var(--transition-duration) var(--ease-emphasized)",
              "&:hover": {
                backgroundColor:
                  "color-mix(in srgb, var(--card) 74%, var(--surface))",
                boxShadow: "var(--os3d-pedestal-lift-stack)",
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
        <Box
          sx={{ minWidth: 0, flex: 1, display: "flex", alignItems: "center" }}
          data-tauri-drag-region={macTauriWindow ? "" : undefined}
        >
          <PageHeader title={title} />
        </Box>
      </Stack>
      <Stack
        direction="row"
        alignItems="stretch"
        sx={{
          flexShrink: 0,
          borderLeft: "1px solid color-mix(in srgb, var(--border) 8%, transparent)",
        }}
      >
        {onOpenSettings && (
          <IconButton
            size="small"
            onClick={onOpenSettings}
            sx={captionBtnSx}
            aria-label={t("settings.open")}
          >
            <Box
              sx={{
                width: "var(--icon-size-md)",
                height: "var(--icon-size-md)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
              }}
            >
              <Os3dIcon src={OS_ICON_SHELL.preferences} variant="inline" />
            </Box>
          </IconButton>
        )}
      </Stack>
    </Box>
  );
}
