import { useState } from "react";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import Stack from "@mui/material/Stack";
import MenuRounded from "@mui/icons-material/MenuRounded";
import RestartAltRounded from "@mui/icons-material/RestartAltRounded";
import SettingsRounded from "@mui/icons-material/SettingsRounded";
import { useTranslation } from "react-i18next";
import { useLocation } from "react-router-dom";
import * as systemApi from "../api/endpoints/system";
import { setRestartPending } from "../store/deviceStatusStore";
import { useDevice } from "../hooks/useDevice";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useToast } from "../hooks/useToast";
import { ConfirmDialog } from "./ConfirmDialog";
import { PageHeader } from "./PageHeader";

const PATH_TO_META: Record<string, { titleKey: string; descKey: string }> = {
  "/device": { titleKey: "device.pageTitle", descKey: "device.pageDesc" },
  "/device-config": {
    titleKey: "deviceConfig.pageTitle",
    descKey: "deviceConfig.pageDesc",
  },
  "/ai-config": {
    titleKey: "aiConfig.pageTitle",
    descKey: "aiConfig.pageDesc",
  },
  "/channels-config": {
    titleKey: "channelsConfig.pageTitle",
    descKey: "channelsConfig.pageDesc",
  },
  "/system-config": {
    titleKey: "systemConfig.pageTitle",
    descKey: "systemConfig.pageDesc",
  },
  "/system-logs": {
    titleKey: "systemLogs.pageTitle",
    descKey: "systemLogs.pageDesc",
  },
  "/soul-user": {
    titleKey: "soulUser.pageTitle",
    descKey: "soulUser.pageDesc",
  },
  "/skills": { titleKey: "skills.pageTitle", descKey: "skills.pageDesc" },
  "/tools": { titleKey: "tools.pageTitle", descKey: "tools.pageDesc" },
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
  onMenuClick?: () => void;
  onOpenSettings?: () => void;
}

export function TopBar({ onMenuClick, onOpenSettings }: TopBarProps) {
  const { t } = useTranslation();
  const location = useLocation();
  const { baseUrl, pairingCode } = useDevice();
  const { deviceConnected } = useDeviceApi();
  const { showToast } = useToast();
  const [restarting, setRestarting] = useState(false);
  const [restartConfirmOpen, setRestartConfirmOpen] = useState(false);

  const pathname = location.pathname;
  const meta = metaForPathname(pathname);
  const title = meta ? t(meta.titleKey) : pathname;
  const description = meta ? t(meta.descKey) : undefined;

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

  return (
    <Box
      component="header"
      sx={{
        flexShrink: 0,
        minHeight: 48,
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        px: 2,
        backgroundColor: "var(--surface)",
        gap: 2,
      }}
    >
      <Stack
        direction="row"
        alignItems="center"
        spacing={1}
        sx={{ minWidth: 0, flex: 1 }}
      >
        {onMenuClick && (
          <IconButton
            size="small"
            onClick={onMenuClick}
            sx={{
              color: "var(--foreground)",
              flexShrink: 0,
              borderRadius: "var(--radius-control)",
              "&:hover": {
                backgroundColor:
                  "color-mix(in srgb, var(--foreground) 6%, transparent)",
              },
            }}
            aria-label="Open menu"
          >
            <MenuRounded />
          </IconButton>
        )}
        <PageHeader title={title} description={description} variant="bar" />
      </Stack>
      <Stack
        direction="row"
        alignItems="center"
        spacing={1}
        sx={{ flexShrink: 0 }}
      >
        {deviceConnected && (
          <IconButton
            size="small"
            onClick={handleRestartClick}
            disabled={restarting}
            sx={{
              color: "var(--semantic-danger)",
              borderRadius: "var(--radius-control)",
              backgroundColor: "color-mix(in srgb, var(--semantic-danger) 6%, transparent)",
              "&:hover:not(:disabled)": {
                backgroundColor: "color-mix(in srgb, var(--semantic-danger) 12%, transparent)",
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
            sx={{
              color: "var(--muted)",
              borderRadius: "var(--radius-control)",
              "&:hover": {
                backgroundColor:
                  "color-mix(in srgb, var(--foreground) 6%, transparent)",
                color: "var(--foreground)",
              },
            }}
            aria-label={t("settings.open")}
          >
            <SettingsRounded />
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
