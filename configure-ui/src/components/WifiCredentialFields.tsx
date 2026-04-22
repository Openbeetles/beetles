import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import MenuItem from "@mui/material/MenuItem";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import WifiFind from "@mui/icons-material/WifiFind";
import { LAYOUT_TOKENS } from "../config/themeTokens";
import { useRevealedPassword } from "../hooks/useRevealedPassword";
import type { WifiApEntry } from "../api/endpoints/system";

const WIFI_MANUAL = "__manual__";
const MAX_LEN = 64;

interface WifiCredentialFieldsProps {
  ssid: string;
  password: string;
  onSsidChange: (value: string) => void;
  onPasswordChange: (value: string) => void;
  canScan: boolean;
  onScan: () => void;
  scanLoading: boolean;
  scanError: string;
  scanList: WifiApEntry[] | null;
}

export function WifiCredentialFields({
  ssid,
  password,
  onSsidChange,
  onPasswordChange,
  canScan,
  onScan,
  scanLoading,
  scanError,
  scanList,
}: WifiCredentialFieldsProps) {
  const { t } = useTranslation();
  const { type: wifiPassType, inputProps: wifiPassInputProps } =
    useRevealedPassword();

  return (
    <>
      <Box
        sx={{
          display: "flex",
          gap: LAYOUT_TOKENS.spacingInlineTight,
          alignItems: "flex-start",
          flexWrap: "wrap",
        }}
      >
        <Button
          variant="outlined"
          size="small"
          startIcon={<WifiFind sx={{ fontSize: "var(--icon-size-sm)" }} />}
          onClick={onScan}
          disabled={!canScan || scanLoading}
        >
          {scanLoading ? t("config.wifiScanning") : t("config.wifiScan")}
        </Button>
        {scanError ? (
          <Button
            variant="text"
            size="small"
            onClick={onScan}
            disabled={!canScan || scanLoading}
            sx={{ borderRadius: "var(--radius-control)" }}
          >
            {t("common.retry")}
          </Button>
        ) : null}
      </Box>
      {scanError ? (
        <Typography
          variant="caption"
          sx={{
            display: "block",
            mt: 0.5,
            color: "var(--semantic-danger)",
            fontWeight: 500,
          }}
        >
          {scanError}
        </Typography>
      ) : null}
      {scanList && scanList.length > 0 ? (
        <>
          <TextField
            select
            label={t("config.wifiSsid")}
            value={scanList.some((ap) => ap.ssid === ssid) ? ssid : WIFI_MANUAL}
            onChange={(e) => {
              const value = e.target.value;
              if (value !== WIFI_MANUAL) onSsidChange(value);
            }}
            fullWidth
            slotProps={{
              inputLabel: { shrink: true },
            }}
          >
            {scanList.map((ap) => (
              <MenuItem key={ap.ssid} value={ap.ssid}>
                {ap.ssid} ({ap.rssi} dBm)
              </MenuItem>
            ))}
            <MenuItem value={WIFI_MANUAL}>{t("config.wifiSsidManual")}</MenuItem>
          </TextField>
          {(ssid === "" || !scanList.some((ap) => ap.ssid === ssid)) && (
            <TextField
              label={t("config.wifiSsidManual")}
              value={ssid}
              onChange={(e) => onSsidChange(e.target.value)}
              fullWidth
              placeholder={t("config.wifiSsidHelp")}
              slotProps={{ htmlInput: { maxLength: MAX_LEN } }}
            />
          )}
        </>
      ) : (
        <TextField
          label={t("config.wifiSsid")}
          value={ssid}
          onChange={(e) => onSsidChange(e.target.value)}
          fullWidth
          helperText={t("config.wifiSsidHelp")}
          slotProps={{ htmlInput: { maxLength: MAX_LEN } }}
        />
      )}
      <TextField
        label={t("config.wifiPass")}
        value={password}
        onChange={(e) => onPasswordChange(e.target.value)}
        type={wifiPassType}
        fullWidth
        slotProps={{
          htmlInput: {
            maxLength: MAX_LEN,
            style: { fontFamily: "var(--font-mono)" },
            ...wifiPassInputProps,
          },
        }}
      />
    </>
  );
}
