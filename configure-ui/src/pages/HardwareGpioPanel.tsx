import MenuItem from "@mui/material/MenuItem";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import IconButton from "@mui/material/IconButton";
import TextField from "@mui/material/TextField";
import AddRounded from "@mui/icons-material/AddRounded";
import DeleteOutlineRounded from "@mui/icons-material/DeleteOutlineRounded";
import SaveRounded from "@mui/icons-material/SaveRounded";
import {
  FormFieldStack,
  FormLoadingSkeleton,
  PanelStateBlock,
  PanelStateLoading,
  FormSectionSubCollapsible,
  FormGrid,
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
import type {
  DeviceEntry,
} from "../types/hardwareConfig";
import {
  HARDWARE_DEVICE_TYPES,
  HARDWARE_PIN_MAX,
  HARDWARE_PIN_MIN,
  HARDWARE_PWM_FREQ_MAX,
  HARDWARE_PWM_FREQ_MIN,
  MAX_HARDWARE_DEVICES,
} from "../types/hardwareConfig";
import { generateDeviceId } from "../util/hardwareDeviceId";
import { mergeGpioHardwareDevices } from "./hardwareSegmentDraft";
import { validateHardwareSegment } from "./hardwareConfigValidation";

function asNumber(v: string): number | null {
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
}

function createNewDevice(taken: Set<string>): DeviceEntry {
  const id = generateDeviceId(taken);
  taken.add(id);
  return {
    id,
    device_type: "gpio_out",
    pins: { pin: 2 },
    what: "",
    how: "",
    options: {},
  };
}

export function HardwareGpioPanel() {
  const { t } = useTranslation();
  const {
    hardwareSegment,
    hardwareLoading,
    hardwareError,
    loadHardwareConfig,
    saveHardwareConfig,
  } = useConfig();
  const editor = useConfigEditorController({
    t,
    hasData: hardwareSegment !== null,
    loading: hardwareLoading,
    load: loadHardwareConfig,
  });
  const [saveRestartRequired, setSaveRestartRequired] = useState(false);
  const [draftDevices, setDraftDevices] = useState<DeviceEntry[] | null>(null);
  const hasSegmentSource = hardwareSegment !== null || draftDevices !== null;
  const loadErrorState = splitPageErrorState({
    hasData: hasSegmentSource,
    loading: hardwareLoading,
    error: hardwareError,
  });

  const devices = useMemo(
    () => draftDevices ?? hardwareSegment?.hardware_devices ?? [],
    [draftDevices, hardwareSegment],
  );

  const segmentToSave = useMemo(
    () => mergeGpioHardwareDevices(hardwareSegment, devices),
    [hardwareSegment, devices],
  );

  const saveDisabled = editor.saveDisabled;

  const updateDevice = (index: number, next: DeviceEntry) => {
    editor.markDirty();
    setDraftDevices((prev) => {
      const base = prev ?? hardwareSegment?.hardware_devices ?? [];
      const copy = [...base];
      copy[index] = next;
      return copy;
    });
  };

  const removeDevice = (index: number) => {
    editor.markDirty();
    setDraftDevices((prev) => {
      const base = prev ?? hardwareSegment?.hardware_devices ?? [];
      return base.filter((_, i) => i !== index);
    });
  };

  const addDevice = () => {
    if (devices.length >= MAX_HARDWARE_DEVICES) return;
    editor.markDirty();
    setDraftDevices((prev) => {
      const base = prev ?? hardwareSegment?.hardware_devices ?? [];
      const taken = new Set(base.map((d) => d.id).filter(Boolean));
      return [...base, createNewDevice(taken)];
    });
  };

  const save = async () => {
    await editor.runSave({
      validate: () => validateHardwareSegment(segmentToSave, t),
      onBeforeSave: () => setSaveRestartRequired(false),
      performSave: () => saveHardwareConfig(segmentToSave),
      onSuccess: (result) => {
        setSaveRestartRequired(Boolean(result.restartRequired));
        setDraftDevices(null);
      },
    });
  };

  if (
    hardwareLoading &&
    !hardwareSegment &&
    draftDevices == null
  ) {
    return (
      <Box sx={PAGE_COLUMN_FILL_SX}>
        <SettingsSection
          pinHeader
          surfaceTone="loading"
          sx={{ flex: 1, minHeight: 0 }}
          icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.hardware} />}
          label={t("hardwareConfig.sectionMain")}
        >
          <PanelStateLoading>
            <FormLoadingSkeleton />
          </PanelStateLoading>
        </SettingsSection>
      </Box>
    );
  }

  if (!hasSegmentSource) {
    return (
      <Box sx={PAGE_STACK_OUTER_SX}>
        <SettingsSection
          pinHeader
          sx={{ flex: 1, minHeight: 0 }}
          icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.hardware} />}
          label={t("hardwareConfig.sectionMain")}
          description={t("hardwareConfig.sectionMainDesc")}
        >
          {loadErrorState.blockingError ? (
            <PageLoadErrorState
              message={loadErrorState.blockingError}
              onRetry={loadHardwareConfig}
            />
          ) : (
            <PanelStateBlock
              tone="neutral"
              icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.hardware} variant="inline" />}
              title={t("config.unavailableTitle")}
              description={t("config.unavailableDesc")}
            />
          )}
        </SettingsSection>
      </Box>
    );
  }

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert
        message={loadErrorState.inlineError}
        onRetry={loadHardwareConfig}
      />
      <SettingsSection
        pinHeader
        sx={{ flex: 1, minHeight: 0 }}
        icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.hardware} />}
        label={t("hardwareConfig.sectionMain")}
        description={t("hardwareConfig.sectionMainDesc")}
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
                    ? t("hardwareConfig.restartRequired")
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
          {devices.map((dev, i) => (
            <FormSectionSubCollapsible
              key={`${dev.id}-${i}`}
              title={t("hardwareConfig.collapseTitle", {
                index: i + 1,
                id: dev.id || "—",
              })}
              defaultOpen={i === 0}
              action={
                <IconButton
                  size="small"
                  aria-label={t("hardwareConfig.removeDevice")}
                  onClick={(e) => {
                    e.stopPropagation();
                    removeDevice(i);
                  }}
                  sx={{ color: "var(--semantic-danger)" }}
                >
                  <DeleteOutlineRounded fontSize="small" />
                </IconButton>
              }
            >
              <FormGrid>
                <TextField
                  fullWidth
                  required
                  disabled
                  label={t("hardwareConfig.id")}
                  value={dev.id}
                  slotProps={{ htmlInput: { readOnly: true } }}
                />
                <TextField
                  select
                  fullWidth
                  label={t("hardwareConfig.deviceType")}
                  value={dev.device_type}
                  onChange={(e) => {
                    const nextType = e.target.value;
                    const prev = dev.options ?? {};
                    let nextOpts: Record<string, unknown> = { ...prev };
                    if (
                      nextType === "pwm_out" &&
                      dev.device_type !== "pwm_out"
                    ) {
                      nextOpts = {
                        ...nextOpts,
                        frequency_hz:
                          typeof nextOpts.frequency_hz === "number"
                            ? nextOpts.frequency_hz
                            : 1000,
                      };
                    }
                    if (
                      dev.device_type === "pwm_out" &&
                      nextType !== "pwm_out"
                    ) {
                      const o = { ...nextOpts };
                      delete o.frequency_hz;
                      nextOpts = o;
                    }

                    if (nextType === "dht" && dev.device_type !== "dht") {
                      const o = { ...nextOpts };
                      delete o.frequency_hz;
                      nextOpts = {
                        ...o,
                        model: "dht11",
                      };
                    }
                    if (dev.device_type === "dht" && nextType !== "dht") {
                      const o = { ...nextOpts };
                      delete o.model;
                      delete o.pull;
                      nextOpts = o;
                    }
                    updateDevice(i, {
                      ...dev,
                      device_type: nextType,
                      options: nextOpts,
                    });
                  }}
                >
                  {HARDWARE_DEVICE_TYPES.map((ty) => (
                    <MenuItem key={ty} value={ty}>
                      {ty}
                    </MenuItem>
                  ))}
                </TextField>
                <TextField
                  type="number"
                  required
                  label={t("hardwareConfig.pin")}
                  value={dev.pins.pin ?? ""}
                  onChange={(e) => {
                    const n = asNumber(e.target.value);
                    updateDevice(i, {
                      ...dev,
                      pins: { ...dev.pins, pin: n ?? 0 },
                    });
                  }}
                  slotProps={{
                    htmlInput: {
                      min: HARDWARE_PIN_MIN,
                      max: HARDWARE_PIN_MAX,
                    },
                  }}
                />
                {dev.device_type === "pwm_out" && (
                  <TextField
                    type="number"
                    label={t("hardwareConfig.pwmFreqHz")}
                    helperText={t("hardwareConfig.pwmFreqHelp")}
                    value={
                      dev.options?.frequency_hz != null
                        ? String(dev.options.frequency_hz)
                        : ""
                    }
                    onChange={(e) => {
                      const raw = e.target.value.trim();
                      const n = raw === "" ? undefined : asNumber(raw);
                      const o: Record<string, unknown> = { ...dev.options };
                      if (n != null) o.frequency_hz = Math.round(n);
                      else delete o.frequency_hz;
                      updateDevice(i, { ...dev, options: o });
                    }}
                    slotProps={{
                      htmlInput: {
                        min: HARDWARE_PWM_FREQ_MIN,
                        max: HARDWARE_PWM_FREQ_MAX,
                      },
                    }}
                  />
                )}
                {dev.device_type === "dht" && (
                  <>
                    <TextField
                      select
                      fullWidth
                      label={t("hardwareConfig.dhtModel")}
                      value={
                        typeof dev.options?.model === "string"
                          ? dev.options.model
                          : "dht11"
                      }
                      onChange={(e) => {
                        const o: Record<string, unknown> = { ...dev.options };
                        o.model = e.target.value;
                        updateDevice(i, { ...dev, options: o });
                      }}
                      slotProps={{ inputLabel: { shrink: true } }}
                    >
                      {(["dht11", "dht22", "dht21"] as const).map((m) => (
                        <MenuItem key={m} value={m}>
                          {m}
                        </MenuItem>
                      ))}
                    </TextField>
                    <TextField
                      select
                      fullWidth
                      label={t("hardwareConfig.dhtPull")}
                      value={
                        typeof dev.options?.pull === "string"
                          ? dev.options.pull
                          : "up"
                      }
                      onChange={(e) => {
                        const o: Record<string, unknown> = { ...dev.options };
                        o.pull = e.target.value;
                        updateDevice(i, { ...dev, options: o });
                      }}
                      slotProps={{ inputLabel: { shrink: true } }}
                    >
                      <MenuItem value="up">up</MenuItem>
                      <MenuItem value="down">down</MenuItem>
                      <MenuItem value="none">none</MenuItem>
                    </TextField>
                  </>
                )}
                <TextField
                  fullWidth
                  label={t("hardwareConfig.what")}
                  value={dev.what}
                  onChange={(e) =>
                    updateDevice(i, { ...dev, what: e.target.value })
                  }
                  slotProps={{ htmlInput: { maxLength: 128 } }}
                  sx={{ gridColumn: { xs: "1 / -1", md: "1 / -1" } }}
                />
                <TextField
                  fullWidth
                  multiline
                  minRows={2}
                  label={t("hardwareConfig.how")}
                  value={dev.how}
                  onChange={(e) =>
                    updateDevice(i, { ...dev, how: e.target.value })
                  }
                  slotProps={{ htmlInput: { maxLength: 256 } }}
                  sx={{ gridColumn: { xs: "1 / -1", md: "1 / -1" } }}
                />
              </FormGrid>
            </FormSectionSubCollapsible>
          ))}
          <Box>
            <Button
              size="small"
              variant="outlined"
              startIcon={<AddRounded />}
              onClick={addDevice}
              disabled={devices.length >= MAX_HARDWARE_DEVICES}
            >
              {t("hardwareConfig.addDevice")}
            </Button>
          </Box>
        </FormFieldStack>
      </SettingsSection>
    </Box>
  );
}
