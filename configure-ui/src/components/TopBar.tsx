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

/** 顶栏：与底部任务栏配套的「窗口标题栏」——左侧窗口图标、右侧标题栏按钮区。 */
export function TopBar() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const navBlocker = useContext(NavBlockerContext);
  const location = useLocation();
  const [brandIconHovered, setBrandIconHovered] = useState(false);
  const macTauriWindow = isMacTauriWindow();
  const titlebarPaddingTop = macTauriWindow ? 1.5 : 0;
  const chromeControlPadding = macTauriWindow ? 0.5 : 0.625;
  const chromeLabelPaddingY = macTauriWindow ? 0.5 : 0.625;
  const chromeRowPaddingY = macTauriWindow ? 0.25 : 0.5;

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
    border: "1px solid color-mix(in srgb, var(--border) 14%, transparent)",
    backgroundColor: "color-mix(in srgb, var(--card) 62%, transparent)",
    boxShadow: "var(--os3d-control-soft-lift-stack)",
  } as const;

  return (
    <Box
      component="header"
      sx={{
        flexShrink: 0,
        minHeight: TOP_BAR_MIN_HEIGHT,
        display: "flex",
        alignItems: "stretch",
        justifyContent: "flex-start",
        pl: { xs: 2, sm: 3 },
        pr: { xs: 2, sm: 3 },
        pt: titlebarPaddingTop,
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
        sx={{ minWidth: 0, flex: 1, py: chromeRowPaddingY, pr: 1 }}
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
              p: chromeControlPadding,
              borderRadius: "var(--radius-control)",
              ...chromeCapsuleSx,
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
        <Box
          sx={{ minWidth: 0, flex: 1, display: "flex", alignItems: "center" }}
          data-tauri-drag-region={macTauriWindow ? "" : undefined}
        >
          <Box
            sx={{
              minWidth: 0,
              maxWidth: "100%",
              px: { xs: 1.125, sm: 1.25 },
              py: chromeLabelPaddingY,
              borderRadius: "var(--radius-search-pill)",
              ...chromeCapsuleSx,
              backgroundImage:
                "linear-gradient(180deg, color-mix(in srgb, #fff 14%, transparent) 0%, transparent 100%)",
            }}
          >
            <PageHeader title={title} />
          </Box>
        </Box>
      </Stack>
    </Box>
  );
}
