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
  HardwareSegment,
  I2cSensorEntry,
} from "../types/hardwareConfig";
import {
  defaultHardwareSegment,
  HARDWARE_DEVICE_TYPES,
  HARDWARE_PIN_MAX,
  HARDWARE_PIN_MIN,
  HARDWARE_PWM_FREQ_MAX,
  HARDWARE_PWM_FREQ_MIN,
  I2C_MAX_READ_LEN_UI,
  I2C_SENSOR_ADDR_MAX,
  I2C_SENSOR_ADDR_MIN,
  I2C_SENSOR_MAX_CMD_LEN,
  I2C_SENSOR_MODELS,
  MAX_HARDWARE_DEVICES,
  MAX_I2C_SENSORS,
} from "../types/hardwareConfig";
import { generateDeviceId } from "../util/hardwareDeviceId";
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

function createNewI2cSensor(taken: Set<string>): I2cSensorEntry {
  const id = generateDeviceId(taken);
  taken.add(id);
  return {
    id,
    addr: 0x44,
    model: "sht3x",
    watch_field: "temperature",
    what: "",
    how: "",
    options: {},
  };
}

/** 合并编辑中的 hardware_devices / i2c_sensors，保留 i2c_bus 等与 GET 一致 */
function mergeSegment(
  base: HardwareSegment | null,
  devices: DeviceEntry[],
  i2cSensors: I2cSensorEntry[],
): HardwareSegment {
  const b = base ?? defaultHardwareSegment();
  return {
    ...b,
    hardware_devices: devices,
    i2c_sensors: i2cSensors,
  };
}

const fieldRisk = "hardwareConfig.fieldRiskHint" as const;

export function HardwareGpioPanel() {
  const { t } = useTranslation();
  const riskHint = t(fieldRisk);
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
  const [draftI2cSensors, setDraftI2cSensors] = useState<
    I2cSensorEntry[] | null
  >(null);
  /** raw 模型 init_cmd 文本编辑草稿（按列表下标） */
  const [i2cRawInitDraft, setI2cRawInitDraft] = useState<Record<number, string>>(
    {},
  );
  const hasSegmentSource =
    hardwareSegment !== null || draftDevices !== null || draftI2cSensors !== null;
  const loadErrorState = splitPageErrorState({
    hasData: hasSegmentSource,
    loading: hardwareLoading,
    error: hardwareError,
  });

  const devices = useMemo(
    () => draftDevices ?? hardwareSegment?.hardware_devices ?? [],
    [draftDevices, hardwareSegment],
  );
  const i2cSensors = useMemo(
    () => draftI2cSensors ?? hardwareSegment?.i2c_sensors ?? [],
    [draftI2cSensors, hardwareSegment],
  );

  const segmentToSave = useMemo(
    () => mergeSegment(hardwareSegment, devices, i2cSensors),
    [hardwareSegment, devices, i2cSensors],
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

  const updateI2cSensor = (index: number, next: I2cSensorEntry) => {
    editor.markDirty();
    setDraftI2cSensors((prev) => {
      const base = prev ?? hardwareSegment?.i2c_sensors ?? [];
      const copy = [...base];
      copy[index] = next;
      return copy;
    });
  };

  const removeI2cSensor = (index: number) => {
    editor.markDirty();
    setDraftI2cSensors((prev) => {
      const base = prev ?? hardwareSegment?.i2c_sensors ?? [];
      return base.filter((_, i) => i !== index);
    });
  };

  const addI2cSensor = () => {
    if (i2cSensors.length >= MAX_I2C_SENSORS) return;
    editor.markDirty();
    setDraftI2cSensors((prev) => {
      const base = prev ?? hardwareSegment?.i2c_sensors ?? [];
      const taken = new Set<string>();
      for (const d of devices) {
        if (d.id) taken.add(d.id);
      }
      for (const x of base) {
        if (x.id) taken.add(x.id);
      }
      return [...base, createNewI2cSensor(taken)];
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
        setDraftI2cSensors(null);
      },
    });
  };

  if (
    hardwareLoading &&
    !hardwareSegment &&
    draftDevices == null &&
    draftI2cSensors == null
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
                  helperText={t("hardwareConfig.idReadonlyHint")}
                  slotProps={{ htmlInput: { readOnly: true } }}
                />
                <TextField
                  select
                  fullWidth
                  label={t("hardwareConfig.deviceType")}
                  helperText={riskHint}
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
                        watch_field: "temperature",
                      };
                    }
                    if (dev.device_type === "dht" && nextType !== "dht") {
                      const o = { ...nextOpts };
                      delete o.model;
                      delete o.watch_field;
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
                  helperText={riskHint}
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
                    helperText={`${t("hardwareConfig.pwmFreqHelp")} ${riskHint}`}
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
                      helperText={`${t("hardwareConfig.dhtModelHelp")} ${riskHint}`}
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
                      label={t("hardwareConfig.dhtWatchField")}
                      helperText={`${t("hardwareConfig.dhtWatchFieldHelp")} ${riskHint}`}
                      value={
                        typeof dev.options?.watch_field === "string"
                          ? dev.options.watch_field
                          : "temperature"
                      }
                      onChange={(e) => {
                        const o: Record<string, unknown> = { ...dev.options };
                        o.watch_field = e.target.value;
                        updateDevice(i, { ...dev, options: o });
                      }}
                      slotProps={{ inputLabel: { shrink: true } }}
                    >
                      <MenuItem value="temperature">temperature</MenuItem>
                      <MenuItem value="humidity">humidity</MenuItem>
                    </TextField>
                    <TextField
                      select
                      fullWidth
                      label={t("hardwareConfig.dhtPull")}
                      helperText={`${t("hardwareConfig.dhtPullHelp")} ${riskHint}`}
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
                  helperText={riskHint}
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
                  helperText={riskHint}
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

          <Box sx={{ mt: 3 }}>
            <Box
              component="h3"
              sx={{ typography: "subtitle1", mb: 1, fontWeight: 600 }}
            >
              {t("hardwareConfig.sectionI2cSensors")}
            </Box>
            <Box
              sx={{
                typography: "body2",
                color: "var(--text-tertiary)",
                mb: 2,
              }}
            >
              {t("hardwareConfig.sectionI2cSensorsDesc")}
            </Box>
            {i2cSensors.map((sens, i) => (
              <FormSectionSubCollapsible
                key={`${sens.id}-${i}`}
                title={t("hardwareConfig.i2cSensorCollapseTitle", {
                  index: i + 1,
                  id: sens.id || "—",
                })}
                defaultOpen={false}
                action={
                  <IconButton
                    size="small"
                    aria-label={t("hardwareConfig.removeI2cSensor")}
                    onClick={(e) => {
                      e.stopPropagation();
                      removeI2cSensor(i);
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
                    value={sens.id}
                    helperText={t("hardwareConfig.idReadonlyHint")}
                    slotProps={{ htmlInput: { readOnly: true } }}
                  />
                  <TextField
                    type="number"
                    required
                    label={t("hardwareConfig.i2cSensorAddr")}
                    helperText={t("hardwareConfig.i2cSensorAddrHelp")}
                    value={sens.addr ?? ""}
                    onChange={(e) => {
                      const n = asNumber(e.target.value);
                      updateI2cSensor(i, {
                        ...sens,
                        addr: n ?? I2C_SENSOR_ADDR_MIN,
                      });
                    }}
                    slotProps={{
                      htmlInput: {
                        min: I2C_SENSOR_ADDR_MIN,
                        max: I2C_SENSOR_ADDR_MAX,
                      },
                    }}
                  />
                  <TextField
                    select
                    fullWidth
                    label={t("hardwareConfig.i2cSensorModel")}
                    helperText={t("hardwareConfig.i2cSensorModelHelp")}
                    value={sens.model}
                    onChange={(e) => {
                      const nextModel = e.target.value;
                      let opts: Record<string, unknown> = {
                        ...(sens.options ?? {}),
                      };
                      if (nextModel === "raw" && sens.model !== "raw") {
                        opts = {
                          init_cmd: [0xac, 0x33, 0],
                          read_len: 6,
                          conversion_wait_ms: 80,
                        };
                        setI2cRawInitDraft((d) => {
                          const next = { ...d };
                          delete next[i];
                          return next;
                        });
                      }
                      if (sens.model === "raw" && nextModel !== "raw") {
                        const o = { ...opts };
                        delete o.init_cmd;
                        delete o.read_len;
                        delete o.conversion_wait_ms;
                        opts = o;
                        setI2cRawInitDraft((d) => {
                          const next = { ...d };
                          delete next[i];
                          return next;
                        });
                      }
                      updateI2cSensor(i, {
                        ...sens,
                        model: nextModel,
                        options: opts,
                      });
                    }}
                    slotProps={{ inputLabel: { shrink: true } }}
                  >
                    {I2C_SENSOR_MODELS.map((m) => (
                      <MenuItem key={m} value={m}>
                        {m}
                      </MenuItem>
                    ))}
                  </TextField>
                  <TextField
                    select
                    fullWidth
                    label={t("hardwareConfig.i2cSensorWatchField")}
                    helperText={t("hardwareConfig.i2cSensorWatchFieldHelp")}
                    value={sens.watch_field}
                    onChange={(e) =>
                      updateI2cSensor(i, {
                        ...sens,
                        watch_field: e.target.value,
                      })
                    }
                    slotProps={{ inputLabel: { shrink: true } }}
                  >
                    <MenuItem value="temperature">temperature</MenuItem>
                    <MenuItem value="humidity">humidity</MenuItem>
                  </TextField>
                  {sens.model === "raw" && (
                    <>
                      <TextField
                        fullWidth
                        label={t("hardwareConfig.i2cSensorRawInit")}
                        helperText={t("hardwareConfig.i2cSensorRawInitHelp")}
                        value={
                          i2cRawInitDraft[i] ??
                          (Array.isArray(sens.options?.init_cmd)
                            ? (sens.options.init_cmd as number[]).join(",")
                            : "")
                        }
                        onChange={(e) => {
                          setI2cRawInitDraft((d) => ({
                            ...d,
                            [i]: e.target.value,
                          }));
                        }}
                        onBlur={() => {
                          const raw =
                            i2cRawInitDraft[i] ??
                            (Array.isArray(sens.options?.init_cmd)
                              ? (sens.options.init_cmd as number[]).join(",")
                              : "");
                          const parts = raw
                            .split(",")
                            .map((x) => x.trim())
                            .filter(Boolean);
                          const arr: number[] = [];
                          for (const p of parts.slice(0, I2C_SENSOR_MAX_CMD_LEN)) {
                            const n = Number(p);
                            if (!Number.isFinite(n) || n < 0 || n > 255) break;
                            arr.push(Math.round(n));
                          }
                          const o: Record<string, unknown> = {
                            ...(sens.options ?? {}),
                          };
                          if (arr.length > 0) o.init_cmd = arr;
                          else delete o.init_cmd;
                          setI2cRawInitDraft((d) => {
                            const next = { ...d };
                            delete next[i];
                            return next;
                          });
                          updateI2cSensor(i, { ...sens, options: o });
                        }}
                        sx={{ gridColumn: { xs: "1 / -1", md: "1 / -1" } }}
                      />
                      <TextField
                        type="number"
                        label={t("hardwareConfig.i2cSensorRawReadLen")}
                        helperText={t("hardwareConfig.i2cSensorRawReadLenHelp")}
                        value={
                          sens.options?.read_len != null
                            ? String(sens.options.read_len)
                            : ""
                        }
                        onChange={(e) => {
                          const n = asNumber(e.target.value);
                          const o: Record<string, unknown> = {
                            ...(sens.options ?? {}),
                          };
                          if (n != null) o.read_len = Math.round(n);
                          else delete o.read_len;
                          updateI2cSensor(i, { ...sens, options: o });
                        }}
                        slotProps={{
                          htmlInput: { min: 1, max: I2C_MAX_READ_LEN_UI },
                        }}
                      />
                      <TextField
                        type="number"
                        label={t("hardwareConfig.i2cSensorRawWait")}
                        helperText={t("hardwareConfig.i2cSensorRawWaitHelp")}
                        value={
                          sens.options?.conversion_wait_ms != null
                            ? String(sens.options.conversion_wait_ms)
                            : ""
                        }
                        onChange={(e) => {
                          const raw = e.target.value.trim();
                          const o: Record<string, unknown> = {
                            ...(sens.options ?? {}),
                          };
                          if (raw === "") {
                            delete o.conversion_wait_ms;
                          } else {
                            const n = asNumber(raw);
                            if (n != null) o.conversion_wait_ms = Math.round(n);
                          }
                          updateI2cSensor(i, { ...sens, options: o });
                        }}
                        slotProps={{
                          htmlInput: { min: 0, max: 2000 },
                        }}
                      />
                    </>
                  )}
                  <TextField
                    fullWidth
                    label={t("hardwareConfig.what")}
                    helperText={riskHint}
                    value={sens.what}
                    onChange={(e) =>
                      updateI2cSensor(i, { ...sens, what: e.target.value })
                    }
                    slotProps={{ htmlInput: { maxLength: 128 } }}
                    sx={{ gridColumn: { xs: "1 / -1", md: "1 / -1" } }}
                  />
                  <TextField
                    fullWidth
                    multiline
                    minRows={2}
                    label={t("hardwareConfig.how")}
                    helperText={riskHint}
                    value={sens.how}
                    onChange={(e) =>
                      updateI2cSensor(i, { ...sens, how: e.target.value })
                    }
                    slotProps={{ htmlInput: { maxLength: 256 } }}
                    sx={{ gridColumn: { xs: "1 / -1", md: "1 / -1" } }}
                  />
                </FormGrid>
              </FormSectionSubCollapsible>
            ))}
            <Box sx={{ mt: 1 }}>
              <Button
                size="small"
                variant="outlined"
                startIcon={<AddRounded />}
                onClick={addI2cSensor}
                disabled={i2cSensors.length >= MAX_I2C_SENSORS}
              >
                {t("hardwareConfig.addI2cSensor")}
              </Button>
            </Box>
          </Box>
        </FormFieldStack>
      </SettingsSection>
    </Box>
  );
}
