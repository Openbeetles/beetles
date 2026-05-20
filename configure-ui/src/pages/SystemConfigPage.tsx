import { useTranslation } from "react-i18next";
import { useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import SaveRounded from "@mui/icons-material/SaveRounded";
import {
  FormLoadingSkeleton,
  PanelStateBlock,
  PanelStateLoading,
  FormSectionSub,
  InlineAlert,
  PageLoadErrorState,
  SaveFeedback,
  splitPageErrorState,
} from "../components/form";
import { Os3dIcon } from "../components/Os3dIcon";
import { SettingsSection } from "../components/SettingsSection";
import { OS_ICON_NAV } from "../config/osIcons";
import {
  PAGE_COLUMN_FILL_SX,
  PAGE_STACK_OUTER_SX,
} from "../theme/panelStyles";
import { useConfig } from "../hooks/useConfig";
import { useConfigEditorController } from "../hooks/useConfigEditorController";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useDevice } from "../hooks/useDevice";
import { useSyncedNullableState } from "../hooks/useSyncedNullableState";
import type { SystemConfigSegment } from "../types/appConfig";
import { WifiCredentialFields } from "../components/WifiCredentialFields";
import { useWifiScanController } from "../hooks/useWifiScanController";
import {
  isValidProxyUrl,
  validateSystemConfig,
} from "./systemConfigValidation";

export function SystemConfigPage() {
  const { t } = useTranslation();
  const { baseUrl } = useDevice();
  const { api, ready, deviceConnected, canAccessProtectedApis, connectionChecking } = useDeviceApi();
  const {
    systemConfig,
    loadSystemConfig,
    saveSystem,
    systemLoading,
    systemError,
  } = useConfig();
  const editor = useConfigEditorController({
    t,
    hasData: systemConfig !== null,
    loading: systemLoading,
    load: loadSystemConfig,
    canLoad: ready && deviceConnected,
  });
  const [form, setForm] = useSyncedNullableState<SystemConfigSegment>(systemConfig);
  const [saveRestartRequired, setSaveRestartRequired] = useState(false);
  const {
    wifiScanList,
    wifiScanLoading,
    wifiScanError,
    handleWifiScan,
  } = useWifiScanController({
    canScan: Boolean(baseUrl?.trim()),
    scan: api.system.wifiScan,
    t,
  });

  const update = (key: keyof SystemConfigSegment, value: string | number) => {
    editor.markDirty();
    setForm((prev) => (prev ? { ...prev, [key]: value } : null));
  };

  const handleSave = async () => {
    if (!form) return;
    await editor.runSave({
      validate: () => validateSystemConfig(form, t),
      performSave: () => saveSystem(form),
      onBeforeSave: () => setSaveRestartRequired(false),
      onSuccess: (result) => {
        setSaveRestartRequired(Boolean(result.restartRequired));
      },
    });
  };

  if (systemLoading && !systemConfig) {
    return (
      <Box sx={PAGE_COLUMN_FILL_SX}>
        <SettingsSection
          pinHeader
          surfaceTone="loading"
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

  const saveDisabled = editor.saveDisabled || !form;
  const proxyUrlError =
    form && !isValidProxyUrl(form.proxy_url ?? "")
      ? t("config.validation.proxyUrlInvalid")
      : "";
  const showConnectionLoading =
    !form && !systemLoading && ready && connectionChecking && !deviceConnected;
  const showConnectState =
    !form && !systemLoading && !showConnectionLoading && (!ready || !deviceConnected);
  const showPairingState =
    !form && !systemLoading && ready && deviceConnected && !canAccessProtectedApis;
  const loadErrorState = splitPageErrorState({
    hasData: Boolean(form),
    loading: systemLoading,
    error: systemError,
    suppress: showConnectState || showPairingState || showConnectionLoading,
  });

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert message={loadErrorState.inlineError} onRetry={loadSystemConfig} />
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
            {editor.saveFeedback.status === "saving" ? t("common.saving") : t("common.save")}
          </Button>
        }
        belowTitleRow={
          editor.saveFeedback.status === "ok" || editor.saveFeedback.status === "fail" ? (
            <SaveFeedback
              placement="belowTitle"
              status={editor.saveFeedback.status}
              message={
                editor.saveFeedback.status === "ok"
                  ? saveRestartRequired
                    ? t("config.restartRequired")
                    : t("common.saveOk")
                  : editor.saveFeedback.error
              }
              autoDismissMs={3000}
              onDismiss={editor.saveFeedback.dismiss}
            />
          ) : null
        }
      >
        {showConnectionLoading ? (
          <PanelStateLoading>
            <FormLoadingSkeleton />
          </PanelStateLoading>
        ) : showConnectState ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/system-config"]} variant="inline" />}
            title={ready ? t("device.connectFirst") : t("device.bannerNeedDevice")}
            description={t("config.connectDesc")}
          />
        ) : showPairingState ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/system-config"]} variant="inline" />}
            title={t("device.pairingCodeRequired")}
            description={t("config.needPairingDesc")}
          />
        ) : loadErrorState.blockingError ? (
          <PageLoadErrorState
            message={loadErrorState.blockingError}
            onRetry={loadSystemConfig}
          />
        ) : !form ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/system-config"]} variant="inline" />}
            title={t("config.unavailableTitle")}
            description={t("config.unavailableDesc")}
          />
        ) : (
          <>
        <FormSectionSub title={t("config.wifi")}>
          <WifiCredentialFields
            ssid={form.wifi_ssid}
            password={form.wifi_pass}
            onSsidChange={(value) => update("wifi_ssid", value)}
            onPasswordChange={(value) => update("wifi_pass", value)}
            canScan={Boolean(baseUrl?.trim())}
            onScan={() => {
              void handleWifiScan();
            }}
            scanLoading={wifiScanLoading}
            scanError={wifiScanError}
            scanList={wifiScanList}
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
            helperText={proxyUrlError || undefined}
            slotProps={{
              htmlInput: {
                maxLength: 256,
                style: { fontFamily: "var(--font-mono)" },
              },
            }}
          />
        </FormSectionSub>

          </>
        )}
      </SettingsSection>
    </Box>
  );
}
