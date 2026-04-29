import MenuItem from "@mui/material/MenuItem";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Switch from "@mui/material/Switch";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import SaveRounded from "@mui/icons-material/SaveRounded";
import {
  FormFieldStack,
  FormLoadingSkeleton,
  PanelStateBlock,
  PanelStateLoading,
  FormSectionSub,
  FormGrid,
  FormSwitchRow,
  InlineAlert,
  PageLoadErrorState,
  SaveFeedback,
  splitPageErrorState,
} from "../components/form";
import { Os3dIcon } from "../components/Os3dIcon";
import { SettingsSection } from "../components/SettingsSection";
import { OS_ICON_DEVICE_CONFIG } from "../config/osIcons";
import { PAGE_COLUMN_FILL_SX, PAGE_STACK_OUTER_SX } from "../theme/panelStyles";
import { useConfig } from "../hooks/useConfig";
import { useConfigEditorController } from "../hooks/useConfigEditorController";
import type { DisplayConfig } from "../types/displayConfig";
import { defaultDisplayConfig } from "../types/displayConfig";
import {
  useDeviceRuntimeKind,
} from "../store/deviceStatusStore";
import {
  linuxBusForDriver,
  validateDisplayConfigForRuntime,
} from "./displayConfigValidation";

function asNumber(v: string): number | null {
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
}

/** 设备配置 →「显示」Tab 内容（路由子页） */
export function DisplayConfigPanel() {
  const { t } = useTranslation();
  const runtimeKind = useDeviceRuntimeKind();
  const {
    displayConfig,
    displayLoading,
    displayError,
    loadDisplayConfig,
    saveDisplayConfig,
  } = useConfig();
  const editor = useConfigEditorController({
    t,
    hasData: displayConfig !== null,
    loading: displayLoading,
    load: loadDisplayConfig,
  });
  const [draft, setDraft] = useState<DisplayConfig | null>(null);
  const [saveRestartRequired, setSaveRestartRequired] = useState(false);
  const formSource = draft ?? displayConfig;
  const form = formSource ?? defaultDisplayConfig();
  const isLinuxRuntime = runtimeKind === "linux";
  const showLinuxFramebuffer = isLinuxRuntime && form.driver === "framebuffer";
  const showLinuxSpiByteSwap = isLinuxRuntime && !showLinuxFramebuffer;
  const loadErrorState = splitPageErrorState({
    hasData: Boolean(formSource),
    loading: displayLoading,
    error: displayError,
  });

  const sectionDesc = useMemo(() => {
    if (isLinuxRuntime) return t("displayConfig.sectionMainDescLinux");
    return t("displayConfig.sectionMainDesc");
  }, [isLinuxRuntime, t]);

  const saveDisabled = editor.saveDisabled;
  const setField = <K extends keyof DisplayConfig>(
    key: K,
    value: DisplayConfig[K],
  ) => {
    editor.markDirty();
    setDraft((prev) => ({ ...(prev ?? form), [key]: value }));
  };

  if (displayLoading && !formSource) {
    return (
      <Box sx={PAGE_COLUMN_FILL_SX}>
        <SettingsSection
          pinHeader
          surfaceTone="loading"
          sx={{ flex: 1, minHeight: 0 }}
          icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.display} />}
          label={t("displayConfig.sectionMain")}
        >
          <PanelStateLoading>
            <FormLoadingSkeleton />
          </PanelStateLoading>
        </SettingsSection>
      </Box>
    );
  }

  if (!formSource) {
    return (
      <Box sx={PAGE_STACK_OUTER_SX}>
        <SettingsSection
          pinHeader
          sx={{ flex: 1, minHeight: 0 }}
          icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.display} />}
          label={t("displayConfig.sectionMain")}
          description={sectionDesc}
        >
          {loadErrorState.blockingError ? (
            <PageLoadErrorState
              message={loadErrorState.blockingError}
              onRetry={loadDisplayConfig}
            />
          ) : (
            <PanelStateBlock
              tone="neutral"
              icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.display} variant="inline" />}
              title={t("config.unavailableTitle")}
              description={t("config.unavailableDesc")}
            />
          )}
        </SettingsSection>
      </Box>
    );
  }

  const save = async () => {
    await editor.runSave({
      validate: () => validateDisplayConfigForRuntime(form, runtimeKind, t),
      onBeforeSave: () => setSaveRestartRequired(false),
      performSave: () =>
        saveDisplayConfig({
          ...form,
          ...(isLinuxRuntime ? { bus: linuxBusForDriver(form.driver) } : {}),
          fb_device: form.fb_device.trim(),
          backlight_sysfs: form.backlight_sysfs?.trim() || null,
        }),
      onSuccess: (result) => {
        setSaveRestartRequired(Boolean(result.restartRequired));
      },
    });
  };

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert
        message={loadErrorState.inlineError}
        onRetry={loadDisplayConfig}
      />
      <SettingsSection
        pinHeader
        sx={{ flex: 1, minHeight: 0 }}
        icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.display} />}
        label={t("displayConfig.sectionMain")}
        description={sectionDesc}
        accessory={
          <Button
            size="small"
            variant="contained"
            startIcon={<SaveRounded />}
            onClick={save}
            disabled={saveDisabled}
          >
            {editor.saveFeedback.status === "saving"
              ? t("common.saving")
              : t("common.save")}
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
                    ? t("displayConfig.restartRequired")
                    : t("common.saveOk")
                  : editor.saveFeedback.error
              }
              autoDismissMs={3000}
              onDismiss={editor.saveFeedback.dismiss}
            />
          ) : null
        }
      >
        <FormFieldStack>
          <FormSectionSub title={t("displayConfig.sectionBasic")}>
            <FormSwitchRow
              title={t("displayConfig.enabled")}
              divider={false}
              control={
                <Switch
                  checked={form.enabled}
                  onChange={(_, checked) => setField("enabled", checked)}
                />
              }
            />
            <FormGrid>
              {isLinuxRuntime ? (
                <TextField
                  select
                  fullWidth
                  disabled={!form.enabled}
                  value={form.driver}
                  label={t("displayConfig.driver")}
                  onChange={(e) => {
                    const driver = e.target.value as DisplayConfig["driver"];
                    editor.markDirty();
                    setDraft((prev) => ({
                      ...(prev ?? form),
                      driver,
                      bus: linuxBusForDriver(driver),
                    }));
                  }}
                >
                  <MenuItem value="st7789">ST7789</MenuItem>
                  <MenuItem value="ili9341">ILI9341</MenuItem>
                  <MenuItem value="framebuffer">
                    {t("displayConfig.driverFramebuffer")}
                  </MenuItem>
                </TextField>
              ) : (
                <TextField
                  select
                  fullWidth
                  disabled={!form.enabled}
                  value={form.driver}
                  label={t("displayConfig.driver")}
                  onChange={(e) =>
                    setField(
                      "driver",
                      e.target.value as DisplayConfig["driver"],
                    )
                  }
                >
                  <MenuItem value="st7789">ST7789</MenuItem>
                  <MenuItem value="ili9341">ILI9341</MenuItem>
                </TextField>
              )}
              {showLinuxFramebuffer ? (
                <TextField
                  fullWidth
                  disabled
                  label={t("displayConfig.rotation")}
                  value="0°"
                  helperText={t("displayConfig.rotationFramebufferHelp")}
                />
              ) : (
                <TextField
                  select
                  fullWidth
                  disabled={!form.enabled}
                  value={form.rotation}
                  label={t("displayConfig.rotation")}
                  onChange={(e) =>
                    setField(
                      "rotation",
                      Number(e.target.value) as DisplayConfig["rotation"],
                    )
                  }
                >
                  <MenuItem value={0}>0</MenuItem>
                  <MenuItem value={90}>90</MenuItem>
                  <MenuItem value={180}>180</MenuItem>
                  <MenuItem value={270}>270</MenuItem>
                </TextField>
              )}
              {!showLinuxFramebuffer ? (
                <>
                  <TextField
                    select
                    fullWidth
                    disabled={!form.enabled}
                    value={form.color_order}
                    label={t("displayConfig.colorOrder")}
                    onChange={(e) =>
                      setField(
                        "color_order",
                        e.target.value as DisplayConfig["color_order"],
                      )
                    }
                  >
                    <MenuItem value="rgb">RGB</MenuItem>
                    <MenuItem value="bgr">BGR</MenuItem>
                  </TextField>
                  <FormSwitchRow
                    title={t("displayConfig.invertColors")}
                    divider={false}
                    control={
                      <Switch
                        checked={form.invert_colors}
                        onChange={(_, checked) =>
                          setField("invert_colors", checked)
                        }
                        disabled={!form.enabled}
                      />
                    }
                  />
                  {showLinuxSpiByteSwap ? (
                    <Box sx={{ display: "flex", flexDirection: "column", gap: 0.5 }}>
                      <FormSwitchRow
                        title={t("displayConfig.linuxSpiSwapBytes")}
                        divider={false}
                        control={
                          <Switch
                            checked={form.linux_spi_swap_bytes}
                            onChange={(_, checked) =>
                              setField("linux_spi_swap_bytes", checked)
                            }
                            disabled={!form.enabled}
                          />
                        }
                      />
                      <Typography
                        variant="body2"
                        sx={{ color: "var(--text-tertiary)", pr: 1 }}
                      >
                        {t("displayConfig.linuxSpiSwapBytesHelp")}
                      </Typography>
                    </Box>
                  ) : null}
                </>
              ) : null}
            </FormGrid>
          </FormSectionSub>

          <FormSectionSub title={t("displayConfig.sectionGeometry")}>
            <FormGrid>
              <TextField
                type="number"
                label={t("displayConfig.width")}
                disabled={!form.enabled}
                value={form.width}
                onChange={(e) => {
                  const n = asNumber(e.target.value);
                  if (n != null) setField("width", n);
                }}
              />
              <TextField
                type="number"
                label={t("displayConfig.height")}
                disabled={!form.enabled}
                value={form.height}
                onChange={(e) => {
                  const n = asNumber(e.target.value);
                  if (n != null) setField("height", n);
                }}
              />
              {!showLinuxFramebuffer ? (
                <>
                  <TextField
                    type="number"
                    label={t("displayConfig.offsetX")}
                    disabled={!form.enabled}
                    value={form.offset_x}
                    onChange={(e) => {
                      const n = asNumber(e.target.value);
                      if (n != null) setField("offset_x", n);
                    }}
                  />
                  <TextField
                    type="number"
                    label={t("displayConfig.offsetY")}
                    disabled={!form.enabled}
                    value={form.offset_y}
                    onChange={(e) => {
                      const n = asNumber(e.target.value);
                      if (n != null) setField("offset_y", n);
                    }}
                  />
                </>
              ) : null}
            </FormGrid>
          </FormSectionSub>

          {isLinuxRuntime ? (
            <FormSectionSub title={t("displayConfig.sectionFramebuffer")}>
              <FormGrid>
                <TextField
                  fullWidth
                  disabled={!form.enabled}
                  label={t("displayConfig.fbDevice")}
                  value={form.fb_device}
                  helperText={t("displayConfig.fbDeviceHelp")}
                  onChange={(e) => setField("fb_device", e.target.value)}
                />
                <TextField
                  fullWidth
                  disabled={!form.enabled}
                  label={t("displayConfig.backlightSysfs")}
                  value={form.backlight_sysfs ?? ""}
                  helperText={t("displayConfig.backlightSysfsHelp")}
                  onChange={(e) => {
                    const v = e.target.value.trim();
                    setField("backlight_sysfs", v === "" ? null : e.target.value);
                  }}
                />
              </FormGrid>
            </FormSectionSub>
          ) : null}

          {!showLinuxFramebuffer ? (
            <FormSectionSub title={t("displayConfig.sectionSpi")}>
              <FormGrid>
                <TextField
                  select
                  fullWidth
                  disabled={!form.enabled}
                  value={form.spi.host}
                  label={t("displayConfig.spiHost")}
                  onChange={(e) => {
                    const host = Number(e.target.value) as 2 | 3;
                    setDraft((prev) => ({
                      ...(prev ?? form),
                      spi: { ...(prev ?? form).spi, host },
                    }));
                    editor.markDirty();
                  }}
                >
                  <MenuItem value={2}>{t("displayConfig.spiHostSpi2")}</MenuItem>
                  <MenuItem value={3}>{t("displayConfig.spiHostSpi3")}</MenuItem>
                </TextField>
                {(["sclk", "mosi", "cs", "dc", "rst", "bl"] as const).map(
                  (k) => (
                    <TextField
                      key={k}
                      type="number"
                      disabled={!form.enabled}
                      label={t(
                        `displayConfig.spi${k[0].toUpperCase()}${k.slice(1)}`,
                      )}
                      value={form.spi[k] ?? ""}
                      onChange={(e) => {
                        const raw = e.target.value.trim();
                        setDraft((prev) => {
                          const base = prev ?? form;
                          const next = { ...base.spi };
                          if (raw === "" && (k === "rst" || k === "bl")) {
                            next[k] = null;
                          } else {
                            const n = asNumber(raw);
                            if (n == null) return base;
                            next[k] = n as never;
                          }
                          return { ...base, spi: next };
                        });
                        editor.markDirty();
                      }}
                    />
                  ),
                )}
                <FormSwitchRow
                  title={t("displayConfig.spiRstActiveHigh")}
                  divider={false}
                  control={
                    <Switch
                      checked={form.spi.rst_active_high}
                      onChange={(_, checked) => {
                        setDraft((prev) => ({
                          ...(prev ?? form),
                          spi: {
                            ...(prev ?? form).spi,
                            rst_active_high: checked,
                          },
                        }));
                        editor.markDirty();
                      }}
                      disabled={!form.enabled || form.spi.rst == null}
                    />
                  }
                />
                <TextField
                  type="number"
                  label={t("displayConfig.spiFreqHz")}
                  disabled={!form.enabled}
                  value={form.spi.freq_hz}
                  onChange={(e) => {
                    const n = asNumber(e.target.value);
                    if (n == null) return;
                    setDraft((prev) => ({
                      ...(prev ?? form),
                      spi: { ...(prev ?? form).spi, freq_hz: n },
                    }));
                    editor.markDirty();
                  }}
                />
              </FormGrid>
            </FormSectionSub>
          ) : null}

          <FormSectionSub title={t("displayConfig.sleepTimeoutSecs")}>
            <TextField
              type="number"
              label={t("displayConfig.sleepTimeoutSecs")}
              helperText={
                isLinuxRuntime
                  ? t("displayConfig.sleepTimeoutSecsHelpLinux")
                  : t("displayConfig.sleepTimeoutSecsHelp")
              }
              disabled={!form.enabled}
              value={form.sleep_timeout_secs}
              onChange={(e) => {
                const n = asNumber(e.target.value);
                if (n != null && n >= 0)
                  setField("sleep_timeout_secs", Math.round(n));
              }}
              slotProps={{ htmlInput: { min: 0, max: 65535 } }}
            />
          </FormSectionSub>
        </FormFieldStack>
      </SettingsSection>
    </Box>
  );
}
