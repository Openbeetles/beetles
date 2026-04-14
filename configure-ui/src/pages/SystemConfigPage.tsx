import { useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Slider from "@mui/material/Slider";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import MenuItem from "@mui/material/MenuItem";
import SaveRounded from "@mui/icons-material/SaveRounded";
import WifiFind from "@mui/icons-material/WifiFind";
import {
  FormLoadingSkeleton,
  PanelStateLoading,
  FormSectionSub,
  InlineAlert,
  SaveFeedback,
} from "../components/form";
import { Os3dIcon } from "../components/Os3dIcon";
import { SettingsSection } from "../components/SettingsSection";
import { OS_ICON_NAV } from "../config/osIcons";
import {
  PAGE_COLUMN_FILL_SX,
  PAGE_STACK_OUTER_SX,
  TEXT_BODY_TERTIARY_SX,
} from "../theme/panelStyles";
import { LAYOUT_TOKENS } from "../config/themeTokens";
import { useConfig } from "../hooks/useConfig";
import { useConfigPageLoad } from "../hooks/useConfigPageLoad";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useSaveFeedback } from "../hooks/useSaveFeedback";
import { useDevice } from "../hooks/useDevice";
import { useRevealedPassword } from "../hooks/useRevealedPassword";
import { useUnsaved } from "../hooks/useUnsaved";
import { useSyncedNullableState } from "../hooks/useSyncedNullableState";
import type { WifiApEntry } from "../api/endpoints/system";
import type { AppConfig } from "../types/appConfig";

const WIFI_MANUAL = "__manual__";

const MAX_LEN = 64;
const SESSION_MIN = 1;
const SESSION_MAX = 128;

/** 简单校验：非空时须含 :// 且 scheme 后为非空（后端会做完整校验）。 */
function isValidProxyUrl(v: string): boolean {
  const s = v.trim();
  if (!s) return true;
  const i = s.indexOf("://");
  return i !== -1 && i + 3 < s.length;
}

function validateSystem(
  form: AppConfig,
  t: (k: string) => string,
): string | null {
  if (!isValidProxyUrl(form.proxy_url ?? ""))
    return t("config.validation.proxyUrlInvalid");
  const wifiPassSet = !!form.wifi_pass.trim();
  if (wifiPassSet && !form.wifi_ssid.trim())
    return t("config.validation.wifiSsidRequired");
  const n = form.session_max_messages;
  if (n < SESSION_MIN || n > SESSION_MAX)
    return t("config.validation.sessionMaxMessages");
  return null;
}

export function SystemConfigPage() {
  const { t } = useTranslation();
  const { baseUrl } = useDevice();
  const { api } = useDeviceApi();
  const { config, loadConfig, saveSystem, loading, error } = useConfig();
  const { setDirty } = useUnsaved();
  const [form, setForm] = useSyncedNullableState<AppConfig>(config);
  const [wifiScanList, setWifiScanList] = useState<WifiApEntry[] | null>(null);
  const [wifiScanLoading, setWifiScanLoading] = useState(false);
  const [wifiScanError, setWifiScanError] = useState("");
  const saveFeedback = useSaveFeedback(t);
  const { type: wifiPassType, inputProps: wifiPassInputProps } =
    useRevealedPassword();

  const handleWifiScan = async () => {
    if (!baseUrl?.trim()) return;
    setWifiScanLoading(true);
    setWifiScanList(null);
    setWifiScanError("");
    const res = await api.system.wifiScan();
    setWifiScanLoading(false);
    if (res.ok && Array.isArray(res.data)) {
      setWifiScanList(res.data);
      setWifiScanError("");
    } else {
      setWifiScanList([]);
      setWifiScanError(res.error ?? t("config.wifiScanFailed"));
    }
  };

  useConfigPageLoad({ hasConfig: config !== null, loading, loadConfig });

  const update = (key: keyof AppConfig, value: string | number) => {
    setDirty(true);
    setForm((prev) => (prev ? { ...prev, [key]: value } : null));
  };

  const handleSave = async () => {
    if (!config || !form) return;
    const err = validateSystem(form, t);
    if (err) {
      saveFeedback.fail(err);
      return;
    }
    const segment = {
      wifi_ssid: form.wifi_ssid,
      wifi_pass: form.wifi_pass,
      proxy_url: form.proxy_url ?? "",
      session_max_messages: form.session_max_messages,
      tg_group_activation: form.tg_group_activation,
    };
    saveFeedback.begin();
    const result = await saveSystem(segment);
    saveFeedback.finishFromResult(result);
    if (result.ok) setDirty(false);
  };

  if (loading && !config) {
    return (
      <Box sx={PAGE_COLUMN_FILL_SX}>
        <SettingsSection
          pinHeader
          sx={{ flex: 1, minHeight: 0 }}
          icon={<Os3dIcon src={OS_ICON_NAV["/system-config"]} />}
          label={t("config.sectionSystem")}
        >
          <PanelStateLoading>
            <FormLoadingSkeleton />
          </PanelStateLoading>
        </SettingsSection>
      </Box>
    );
  }

  const saveDisabled = saveFeedback.status === "saving" || !form;
  const proxyUrlError =
    form && !isValidProxyUrl(form.proxy_url ?? "")
      ? t("config.validation.proxyUrlInvalid")
      : "";
  const sessionError =
    form &&
    (form.session_max_messages < SESSION_MIN ||
      form.session_max_messages > SESSION_MAX)
      ? t("config.validation.sessionMaxMessages")
      : "";

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert message={error} onRetry={loadConfig} />
      <SettingsSection
        pinHeader
        sx={{ flex: 1, minHeight: 0 }}
        icon={<Os3dIcon src={OS_ICON_NAV["/system-config"]} />}
        label={t("config.sectionSystem")}
        description={t("config.sectionSystemDesc")}
        accessory={
          <Button
            size="small"
            variant="contained"
            startIcon={<SaveRounded />}
            onClick={handleSave}
            disabled={saveDisabled}
            title={!form ? t("config.hintSaveNeedDevice") : undefined}
            sx={{ borderRadius: "var(--radius-control)" }}
          >
            {saveFeedback.status === "saving" ? t("common.saving") : t("common.save")}
          </Button>
        }
        belowTitleRow={
          saveFeedback.status === "ok" || saveFeedback.status === "fail" ? (
            <SaveFeedback
              placement="belowTitle"
              status={saveFeedback.status}
              message={saveFeedback.status === "ok" ? t("common.saveOk") : saveFeedback.error}
              autoDismissMs={3000}
              onDismiss={saveFeedback.dismiss}
            />
          ) : null
        }
      >
        {!form ? (
          <Typography variant="body2" sx={{ ...TEXT_BODY_TERTIARY_SX, py: 2 }}>
            {t("config.hintSaveNeedDevice")}
          </Typography>
        ) : (
          <>
        <FormSectionSub title={t("config.wifi")}>
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
              onClick={handleWifiScan}
              disabled={!baseUrl?.trim() || wifiScanLoading}
            >
              {wifiScanLoading ? t("config.wifiScanning") : t("config.wifiScan")}
            </Button>
            {wifiScanError && (
              <Button
                variant="text"
                size="small"
                onClick={handleWifiScan}
                disabled={!baseUrl?.trim() || wifiScanLoading}
                sx={{ borderRadius: "var(--radius-control)" }}
              >
                {t("common.retry")}
              </Button>
            )}
          </Box>
          {wifiScanError && (
            <Typography
              variant="caption"
              sx={{
                display: "block",
                mt: 0.5,
                color: "var(--semantic-danger)",
                fontWeight: 500,
              }}
            >
              {wifiScanError}
            </Typography>
          )}
          {wifiScanList && wifiScanList.length > 0 ? (
            <>
              <TextField
                select
                label={t("config.wifiSsid")}
                value={
                  wifiScanList.some((ap) => ap.ssid === form.wifi_ssid)
                    ? form.wifi_ssid
                    : WIFI_MANUAL
                }
                onChange={(e) => {
                  const v = e.target.value;
                  if (v !== WIFI_MANUAL) update("wifi_ssid", v);
                }}
                fullWidth
                slotProps={{
                  inputLabel: { shrink: true },
                }}
              >
                {wifiScanList.map((ap) => (
                  <MenuItem key={ap.ssid} value={ap.ssid}>
                    {ap.ssid} ({ap.rssi} dBm)
                  </MenuItem>
                ))}
                <MenuItem value={WIFI_MANUAL}>{t("config.wifiSsidManual")}</MenuItem>
              </TextField>
              {(form.wifi_ssid === "" ||
                !wifiScanList.some((ap) => ap.ssid === form.wifi_ssid)) && (
                <TextField
                  label={t("config.wifiSsidManual")}
                  value={form.wifi_ssid}
                  onChange={(e) => update("wifi_ssid", e.target.value)}
                  fullWidth
                  placeholder={t("config.wifiSsidHelp")}
                  slotProps={{ htmlInput: { maxLength: MAX_LEN } }}
                />
              )}
            </>
          ) : (
            <TextField
              label={t("config.wifiSsid")}
              value={form.wifi_ssid}
              onChange={(e) => update("wifi_ssid", e.target.value)}
              fullWidth
              helperText={t("config.wifiSsidHelp")}
              slotProps={{ htmlInput: { maxLength: MAX_LEN } }}
            />
          )}
          <TextField
            label={t("config.wifiPass")}
            value={form.wifi_pass}
            onChange={(e) => update("wifi_pass", e.target.value)}
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
        </FormSectionSub>

        <FormSectionSub title={t("config.proxy")}>
          <TextField
            label={t("config.proxyUrl")}
            value={form.proxy_url ?? ""}
            onChange={(e) => update("proxy_url", e.target.value)}
            placeholder={t("config.placeholderProxyUrl")}
            fullWidth
            error={!!proxyUrlError}
            helperText={proxyUrlError || t("config.proxyUrlHint")}
            slotProps={{
              htmlInput: {
                maxLength: 256,
                style: { fontFamily: "var(--font-mono)" },
              },
            }}
          />
        </FormSectionSub>

        <FormSectionSub title={t("config.session")}>
          <Typography variant="body2" sx={{ mb: 1, ...TEXT_BODY_TERTIARY_SX }}>
            {t("config.sessionMaxMessages")}: {form.session_max_messages}
          </Typography>
          <Slider
            value={form.session_max_messages}
            min={SESSION_MIN}
            max={SESSION_MAX}
            valueLabelDisplay="auto"
            onChange={(_, value) =>
              update(
                "session_max_messages",
                Array.isArray(value) ? value[0] : value,
              )
            }
            sx={{ maxWidth: 320, mt: 0.5 }}
          />
          {(sessionError || t("config.sessionMaxMessagesHelp")) && (
            <Typography
              variant="caption"
              sx={{
                display: "block",
                mt: 0.5,
                color: sessionError
                  ? "var(--semantic-danger)"
                  : "var(--text-tertiary)",
              }}
            >
              {sessionError || t("config.sessionMaxMessagesHelp")}
            </Typography>
          )}
        </FormSectionSub>
          </>
        )}
      </SettingsSection>
    </Box>
  );
}
