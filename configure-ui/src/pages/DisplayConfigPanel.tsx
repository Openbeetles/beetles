import MenuItem from "@mui/material/MenuItem";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import FormControlLabel from "@mui/material/FormControlLabel";
import Switch from "@mui/material/Switch";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import SaveRounded from "@mui/icons-material/SaveRounded";
import {
  FormFieldStack,
  FormLoadingSkeleton,
  PanelStateLoading,
  FormSectionSub,
  InlineAlert,
  SaveFeedback,
} from "../components/form";
import { Os3dIcon } from "../components/Os3dIcon";
import { SettingsSection } from "../components/SettingsSection";
import { OS_ICON_DEVICE_CONFIG } from "../config/osIcons";
import { PAGE_COLUMN_FILL_SX, PAGE_STACK_OUTER_SX } from "../theme/panelStyles";
import { useConfig } from "../hooks/useConfig";
import { useSaveFeedback } from "../hooks/useSaveFeedback";
import { useUnsaved } from "../hooks/useUnsaved";
import type { DisplayConfig } from "../types/displayConfig";
import { defaultDisplayConfig } from "../types/displayConfig";
import {
  useDeviceRuntimeKind,
  type DeviceRuntimeKind,
} from "../store/deviceStatusStore";

const PIN_MIN = 1;
const PIN_MAX = 48;
const DIM_MIN = 1;
const DIM_MAX = 480;
const OFFSET_MIN = -480;
const OFFSET_MAX = 480;
const FREQ_MIN = 1_000_000;
const FREQ_MAX = 80_000_000;

function asNumber(v: string): number | null {
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
}

function pathNoControlChars(s: string): boolean {
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c < 0x20 || c === 0) return false;
  }
  return true;
}

function validateEsp(
  form: DisplayConfig,
  t: (k: string) => string,
): string | null {
  if (!form.enabled) return null;
  const dimOk =
    form.width >= DIM_MIN &&
    form.width <= DIM_MAX &&
    form.height >= DIM_MIN &&
    form.height <= DIM_MAX;
  if (!dimOk) return t("displayConfig.validation.dimension");
  if (![0, 90, 180, 270].includes(form.rotation))
    return t("displayConfig.validation.rotation");
  const offsetOk =
    form.offset_x >= OFFSET_MIN &&
    form.offset_x <= OFFSET_MAX &&
    form.offset_y >= OFFSET_MIN &&
    form.offset_y <= OFFSET_MAX;
  if (!offsetOk) return t("displayConfig.validation.offset");
  if (form.spi.freq_hz < FREQ_MIN || form.spi.freq_hz > FREQ_MAX) {
    return t("displayConfig.validation.freq");
  }
  const pins = [form.spi.sclk, form.spi.mosi, form.spi.cs, form.spi.dc];
  if (pins.some((p) => p < PIN_MIN || p > PIN_MAX))
    return t("displayConfig.validation.pin");
  if (
    form.spi.rst != null &&
    (form.spi.rst < PIN_MIN || form.spi.rst > PIN_MAX)
  )
    return t("displayConfig.validation.pin");
  if (form.spi.bl != null && (form.spi.bl < PIN_MIN || form.spi.bl > PIN_MAX))
    return t("displayConfig.validation.pin");
  return null;
}

function validateLinuxFb(
  form: DisplayConfig,
  t: (k: string) => string,
): string | null {
  if (!form.enabled) return null;
  const dimOk =
    form.width >= DIM_MIN &&
    form.width <= DIM_MAX &&
    form.height >= DIM_MIN &&
    form.height <= DIM_MAX;
  if (!dimOk) return t("displayConfig.validation.dimension");
  const fb = form.fb_device.trim();
  if (!fb) return t("displayConfig.validation.fbDeviceRequired");
  if (!pathNoControlChars(fb))
    return t("displayConfig.validation.pathInvalid");
  const bl = form.backlight_sysfs?.trim() ?? "";
  if (bl && !pathNoControlChars(bl))
    return t("displayConfig.validation.pathInvalid");
  return null;
}

function validateLinuxSpi(
  form: DisplayConfig,
  t: (k: string) => string,
): string | null {
  if (!form.enabled) return null;
  const dimOk =
    form.width >= DIM_MIN &&
    form.width <= DIM_MAX &&
    form.height >= DIM_MIN &&
    form.height <= DIM_MAX;
  if (!dimOk) return t("displayConfig.validation.dimension");
  if (![0, 90, 180, 270].includes(form.rotation))
    return t("displayConfig.validation.rotation");
  const offsetOk =
    form.offset_x >= OFFSET_MIN &&
    form.offset_x <= OFFSET_MAX &&
    form.offset_y >= OFFSET_MIN &&
    form.offset_y <= OFFSET_MAX;
  if (!offsetOk) return t("displayConfig.validation.offset");
  if (form.spi.freq_hz < FREQ_MIN || form.spi.freq_hz > FREQ_MAX) {
    return t("displayConfig.validation.freq");
  }
  const gpioPins = [form.spi.dc, form.spi.rst, form.spi.bl].filter(
    (pin): pin is number => pin != null,
  );
  if (gpioPins.some((p) => p < PIN_MIN || p > PIN_MAX))
    return t("displayConfig.validation.pin");
  const devicePath = form.fb_device.trim();
  if (devicePath && !pathNoControlChars(devicePath))
    return t("displayConfig.validation.pathInvalid");
  return null;
}

function validateForRuntime(
  form: DisplayConfig,
  kind: DeviceRuntimeKind,
  t: (k: string) => string,
): string | null {
  if (kind === "linux") {
    if (form.driver === "framebuffer") {
      return validateLinuxFb(form, t);
    }
    return validateLinuxSpi(form, t);
  }
  return validateEsp(form, t);
}

function linuxBusForDriver(
  driver: DisplayConfig["driver"],
): DisplayConfig["bus"] {
  return driver === "framebuffer" ? "framebuffer" : "spi";
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
  const saveFeedback = useSaveFeedback(t);
  const { setDirty } = useUnsaved();
  const [draft, setDraft] = useState<DisplayConfig | null>(null);
  const [saveRestartRequired, setSaveRestartRequired] = useState(false);
  const form = draft ?? displayConfig ?? defaultDisplayConfig();
  const isLinuxRuntime = runtimeKind === "linux";
  const showLinuxFramebuffer = isLinuxRuntime && form.driver === "framebuffer";
  const showLinuxSpiByteSwap = isLinuxRuntime && !showLinuxFramebuffer;

  const sectionDesc = useMemo(() => {
    if (isLinuxRuntime) return t("displayConfig.sectionMainDescLinux");
    return t("displayConfig.sectionMainDesc");
  }, [isLinuxRuntime, t]);

  useEffect(() => {
    void loadDisplayConfig();
  }, [loadDisplayConfig]);

  const saveDisabled = saveFeedback.status === "saving";
  const setField = <K extends keyof DisplayConfig>(
    key: K,
    value: DisplayConfig[K],
  ) => {
    setDirty(true);
    setDraft((prev) => ({ ...(prev ?? form), [key]: value }));
  };

  if (displayLoading && !displayConfig && !draft) {
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

  const save = async () => {
    const err = validateForRuntime(form, runtimeKind, t);
    if (err) {
      saveFeedback.fail(err);
      return;
    }
    saveFeedback.begin();
    setSaveRestartRequired(false);
    const body = {
      ...form,
      ...(isLinuxRuntime ? { bus: linuxBusForDriver(form.driver) } : {}),
      fb_device: form.fb_device.trim(),
      backlight_sysfs: form.backlight_sysfs?.trim() || null,
    };
    const result = await saveDisplayConfig(body);
    saveFeedback.finishFromResult(result);
    if (result.ok) {
      setDirty(false);
      setSaveRestartRequired(Boolean(result.restartRequired));
    }
  };

  const fieldGridSx = {
    display: "grid",
    gap: 2,
    gridTemplateColumns: {
      xs: "minmax(0, 1fr)",
      md: "repeat(2, minmax(0, 1fr))",
    },
    "& .MuiFormControl-root": {
      minWidth: 0,
    },
  } as const;

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert message={displayError} onRetry={loadDisplayConfig} />
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
            {saveFeedback.status === "saving"
              ? t("common.saving")
              : t("common.save")}
          </Button>
        }
        belowTitleRow={
          saveFeedback.status === "ok" || saveFeedback.status === "fail" ? (
            <SaveFeedback
              placement="belowTitle"
              status={saveFeedback.status}
              message={
                saveFeedback.status === "ok"
                  ? saveRestartRequired
                    ? t("displayConfig.restartRequired")
                    : t("common.saveOk")
                  : saveFeedback.error
              }
              autoDismissMs={3000}
              onDismiss={saveFeedback.dismiss}
            />
          ) : null
        }
      >
        <FormFieldStack>
          <FormSectionSub title={t("displayConfig.sectionBasic")}>
            <FormControlLabel
              control={
                <Switch
                  checked={form.enabled}
                  onChange={(_, checked) => setField("enabled", checked)}
                />
              }
              label={t("displayConfig.enabled")}
            />
            <Box sx={fieldGridSx}>
              {isLinuxRuntime ? (
                <TextField
                  select
                  fullWidth
                  disabled={!form.enabled}
                  value={form.driver}
                  label={t("displayConfig.driver")}
                  onChange={(e) => {
                    const driver = e.target.value as DisplayConfig["driver"];
                    setDirty(true);
                    setDraft((prev) => ({
                      ...(prev ?? form),
                      driver,
                      bus: linuxBusForDriver(driver),
                    }));
                  }}
                >
                  <MenuItem value="st7789">ST7789</MenuItem>
                  <MenuItem value="ili9341">ILI9341</MenuItem>
                  <MenuItem value="st7735">
                    ST7735 (1.8&quot; / 1.44&quot; / 0.96&quot;)
                  </MenuItem>
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
                  <MenuItem value="st7735">
                    ST7735 (1.8&quot; / 1.44&quot; / 0.96&quot;)
                  </MenuItem>
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
                  <FormControlLabel
                    control={
                      <Switch
                        checked={form.invert_colors}
                        onChange={(_, checked) =>
                          setField("invert_colors", checked)
                        }
                        disabled={!form.enabled}
                      />
                    }
                    label={t("displayConfig.invertColors")}
                  />
                  {showLinuxSpiByteSwap ? (
                    <Box sx={{ display: "flex", flexDirection: "column", gap: 0.5 }}>
                      <FormControlLabel
                        control={
                          <Switch
                            checked={form.linux_spi_swap_bytes}
                            onChange={(_, checked) =>
                              setField("linux_spi_swap_bytes", checked)
                            }
                            disabled={!form.enabled}
                          />
                        }
                        label={t("displayConfig.linuxSpiSwapBytes")}
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
            </Box>
          </FormSectionSub>

          <FormSectionSub title={t("displayConfig.sectionGeometry")}>
            <Box sx={fieldGridSx}>
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
            </Box>
          </FormSectionSub>

          {isLinuxRuntime ? (
            <FormSectionSub title={t("displayConfig.sectionFramebuffer")}>
              <Box sx={fieldGridSx}>
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
              </Box>
            </FormSectionSub>
          ) : null}

          {!showLinuxFramebuffer ? (
            <FormSectionSub title={t("displayConfig.sectionSpi")}>
              <Box sx={fieldGridSx}>
                <TextField
                  select
                  fullWidth
                  disabled={!form.enabled}
                  value={form.spi.host}
                  label={t("displayConfig.spiHost")}
                  onChange={(e) => {
                    const host = Number(e.target.value) as 1 | 2;
                    setDraft((prev) => ({
                      ...(prev ?? form),
                      spi: { ...(prev ?? form).spi, host },
                    }));
                    setDirty(true);
                  }}
                >
                  <MenuItem value={1}>{t("displayConfig.spiHostSpi2")}</MenuItem>
                  <MenuItem value={2}>{t("displayConfig.spiHostSpi3")}</MenuItem>
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
                        setDirty(true);
                      }}
                    />
                  ),
                )}
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
                    setDirty(true);
                  }}
                />
              </Box>
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
