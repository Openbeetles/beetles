import { useContext, useState } from "react";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";
import { BeetleIcon } from "./BeetleIcon";
import { PageHeader } from "./PageHeader";
import { ShellBreadcrumb } from "./ShellChromeTrail";
import { NavBlockerContext } from "../contexts/NavBlockerContext";
import { TOP_BAR_MIN_HEIGHT } from "../config/layout";
import { SHELL_TITLEBAR_CHROME_SX } from "../theme/shellChromeSurface";
import { OS_ICON_SHELL } from "../config/osIcons";
import { Os3dIcon } from "./Os3dIcon";

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
    width: 52,
    height: TOP_BAR_MIN_HEIGHT,
    maxHeight: TOP_BAR_MIN_HEIGHT,
    borderRadius: 0,
    border: "none",
    color: "var(--foreground-soft)",
    transition:
      "background-color var(--transition-duration) var(--ease-out-smooth), color var(--transition-duration) var(--ease-out-smooth)",
    "&:hover:not(:disabled)": {
      backgroundColor: "color-mix(in srgb, var(--foreground) 8%, transparent)",
      color: "var(--foreground)",
    },
  } as const;

  return (
    <Box
      component="header"
      sx={{
        flexShrink: 0,
        minHeight: TOP_BAR_MIN_HEIGHT,
        display: "flex",
        alignItems: "stretch",
        justifyContent: "space-between",
        pl: { xs: 2, sm: 3 },
        pr: 0,
        position: "relative",
        ...SHELL_TITLEBAR_CHROME_SX,
        /** 与主内容区分；不外投阴影，符合扁平壳层约定 */
        borderBottom: "1px solid var(--border-subtle)",
        gap: 0,
      }}
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
              /** 与 `PageHeader` 标题字阶、右侧 caption 按钮视觉重量对齐 */
              p: 0.625,
              borderRadius: "var(--radius-control)",
              border: "1px solid var(--border-subtle)",
              backgroundColor:
                "color-mix(in srgb, var(--surface) 65%, var(--card))",
              boxShadow:
                "inset 0 1px 0 color-mix(in srgb, var(--foreground) 10%, transparent)",
              transition:
                "background-color var(--transition-duration) var(--ease-out-smooth), box-shadow var(--transition-duration) var(--ease-out-smooth), transform var(--transition-duration) var(--ease-emphasized)",
              "&:hover": {
                backgroundColor:
                  "color-mix(in srgb, var(--foreground) 7%, transparent)",
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
        <Stack spacing={0.25} sx={{ minWidth: 0, flex: 1 }}>
          <PageHeader title={title} />
          <ShellBreadcrumb />
        </Stack>
      </Stack>
      <Stack
        direction="row"
        alignItems="stretch"
        sx={{
          flexShrink: 0,
          borderLeft: "1px solid var(--border-subtle)",
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
