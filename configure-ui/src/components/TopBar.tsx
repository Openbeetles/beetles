import { useContext, useState } from "react";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import Stack from "@mui/material/Stack";
import Tooltip from "@mui/material/Tooltip";
import RestartAltRounded from "@mui/icons-material/RestartAltRounded";
import SettingsRounded from "@mui/icons-material/SettingsRounded";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";
import * as systemApi from "../api/endpoints/system";
import { setRestartPending } from "../store/deviceStatusStore";
import { useDevice } from "../hooks/useDevice";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useToast } from "../hooks/useToast";
import { ConfirmDialog } from "./ConfirmDialog";
import { BeetleIcon } from "./BeetleIcon";
import { PageHeader } from "./PageHeader";
import { NavBlockerContext } from "../contexts/NavBlockerContext";
import { TOP_BAR_MIN_HEIGHT } from "../config/layout";
import { SHELL_TITLEBAR_CHROME_SX } from "../theme/shellChromeSurface";

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
  const { baseUrl, pairingCode } = useDevice();
  const { deviceConnected } = useDeviceApi();
  const { showToast } = useToast();
  const [restarting, setRestarting] = useState(false);
  const [restartConfirmOpen, setRestartConfirmOpen] = useState(false);
  const [brandIconHovered, setBrandIconHovered] = useState(false);

  const pathname = location.pathname;
  const meta = metaForPathname(pathname);
  const title = meta ? t(meta.titleKey) : pathname;

  const doRestart = async () => {
    if (!baseUrl?.trim() || !pairingCode?.trim()) return;
    setRestarting(true);
    const res = await systemApi.postRestart(baseUrl, pairingCode);
    setRestarting(false);
    setRestartConfirmOpen(false);
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
        pl: 1.5,
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
              p: 0.5,
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
              },
            }}
          >
            <BeetleIcon
              aria-hidden
              animationActive={brandIconHovered}
              sx={{
                width: 24,
                height: 24,
                borderRadius: "calc(var(--radius-control) - 2px)",
              }}
            />
          </IconButton>
        </Tooltip>
        <PageHeader title={title} />
      </Stack>
      <Stack
        direction="row"
        alignItems="stretch"
        sx={{
          flexShrink: 0,
          borderLeft: "1px solid var(--border-subtle)",
        }}
      >
        {deviceConnected && (
          <IconButton
            size="small"
            onClick={handleRestartClick}
            disabled={restarting}
            sx={{
              ...captionBtnSx,
              color: "var(--semantic-danger)",
              "&:hover:not(:disabled)": {
                backgroundColor:
                  "color-mix(in srgb, var(--semantic-danger) 14%, transparent)",
                color: "var(--semantic-danger)",
              },
            }}
            aria-label={t("device.restart")}
            title={t("device.restart")}
          >
            <RestartAltRounded sx={{ fontSize: "var(--icon-size-sm)" }} />
          </IconButton>
        )}
        {onOpenSettings && (
          <IconButton
            size="small"
            onClick={onOpenSettings}
            sx={captionBtnSx}
            aria-label={t("settings.open")}
          >
            <SettingsRounded sx={{ fontSize: "var(--icon-size-md)" }} />
          </IconButton>
        )}
      </Stack>
      <ConfirmDialog
        open={restartConfirmOpen}
        onClose={() => setRestartConfirmOpen(false)}
        title={t("device.restartConfirmTitle")}
        description={t("device.restartConfirmDesc")}
        icon={<RestartAltRounded sx={{ fontSize: "var(--icon-size-md)" }} />}
        confirmLabel={t("device.restart")}
        onConfirm={doRestart}
        confirmDisabled={restarting}
        confirmColor="primary"
      />
    </Box>
  );
}
