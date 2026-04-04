import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import OpenInNewRounded from "@mui/icons-material/OpenInNewRounded";
import { useTranslation } from "react-i18next";
import { useDevice } from "../hooks/useDevice";
import { useDeviceApi } from "../hooks/useDeviceApi";

/**
 * 与 Rust API 一致：无设备地址 / 设备未激活 / 未填配对码 时在 TopBar 下展示横幅；未激活时提供跳转固件配对页按钮。
 */
export function DeviceBanner() {
  const { t } = useTranslation();
  const { baseUrl } = useDevice();
  const { needDeviceHint, deviceHintReason } = useDeviceApi();

  if (!needDeviceHint || !deviceHintReason) return null;

  const messageKey =
    deviceHintReason === "no_device"
      ? "device.bannerNeedDevice"
      : deviceHintReason === "device_not_activated"
        ? "device.bannerDeviceNotActivated"
        : "device.bannerNeedPairing";

  const pairingUrl = baseUrl?.trim()
    ? `${baseUrl.replace(/\/$/, "")}/pairing`
    : "";

  return (
    <Box
      role="status"
      aria-live="polite"
      sx={{
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 2,
        px: 2,
        py: 1.25,
        borderBottom: "none",
        borderLeft: "var(--accent-line-width, 3px) solid var(--semantic-warning)",
        backgroundColor:
          "color-mix(in srgb, var(--semantic-warning) 4%, var(--surface))",
      }}
    >
      <Typography
        variant="body2"
        sx={{
          color: "var(--semantic-warning)",
          fontWeight: 600,
          fontSize: "var(--font-size-body-sm)",
        }}
      >
        {t(messageKey)}
      </Typography>
      {deviceHintReason === "device_not_activated" && pairingUrl && (
        <Tooltip title={t("device.bannerGoToPairingTooltip")} placement="left">
          <Button
            size="small"
            variant="outlined"
            href={pairingUrl}
            target="_blank"
            rel="noopener noreferrer"
            endIcon={<OpenInNewRounded sx={{ fontSize: "var(--icon-size-sm)" }} />}
            sx={{
              flexShrink: 0,
              borderRadius: "var(--radius-control)",
              borderColor: "var(--semantic-warning)",
              color: "var(--semantic-warning)",
              "&:hover": {
                borderColor: "var(--semantic-warning)",
                backgroundColor: "color-mix(in srgb, var(--semantic-warning) 8%, var(--surface))",
              },
            }}
          >
            {t("device.bannerGoToPairing")}
          </Button>
        </Tooltip>
      )}
    </Box>
  );
}
