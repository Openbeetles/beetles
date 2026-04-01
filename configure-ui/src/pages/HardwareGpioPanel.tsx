import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import IconButton from "@mui/material/IconButton";
import TextField from "@mui/material/TextField";
import AddRounded from "@mui/icons-material/AddRounded";
import DeleteOutlineRounded from "@mui/icons-material/DeleteOutlineRounded";
import MemoryOutlined from "@mui/icons-material/MemoryOutlined";
import SaveRounded from "@mui/icons-material/SaveRounded";
import {
  FormFieldStack,
  FormLoadingSkeleton,
  FormSectionSubCollapsible,
  InlineAlert,
  SaveFeedback,
} from "../components/form";
import { SettingsSection } from "../components/SettingsSection";
import { useConfig } from "../hooks/useConfig";
import { useSaveFeedback } from "../hooks/useSaveFeedback";
import { useUnsaved } from "../hooks/useUnsaved";
import type {
  DeviceEntry,
  HardwareSegment,
  I2cSensorEntry,
} from "../types/hardwareConfig";
import {
  defaultHardwareSegment,
  HARDWARE_ADC1_MAX_PIN,
  HARDWARE_DEVICE_TYPES,
  HARDWARE_FORBIDDEN_PINS,
  HARDWARE_PIN_MAX,
  HARDWARE_PIN_MIN,
  HARDWARE_PWM_FREQ_MAX,
  HARDWARE_PWM_FREQ_MIN,
  I2C_MAX_READ_LEN_UI,
  I2C_SENSOR_ADDR_MAX,
  I2C_SENSOR_ADDR_MIN,
  I2C_SENSOR_ID_MAX_LEN,
  I2C_SENSOR_MAX_CMD_LEN,
  I2C_SENSOR_MODELS,
  type I2cSensorModel,
  MAX_HARDWARE_DEVICES,
  MAX_I2C_SENSORS,
  MAX_PWM_DEVICES,
} from "../types/hardwareConfig";
import { generateDeviceId } from "../util/hardwareDeviceId";

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

function validateSegment(
  seg: HardwareSegment,
  t: (k: string) => string,
): string | null {
  const devs = seg.hardware_devices;
  if (devs.length > MAX_HARDWARE_DEVICES) {
    return t("hardwareConfig.validation.maxDevices");
  }
  const seenIds = new Set<string>();
  const seenPins = new Set<number>();
  let pwmCount = 0;
  for (let i = 0; i < devs.length; i++) {
    const d = devs[i];
    if (!d.id.trim() || d.id.length > 32) {
      return t("hardwareConfig.validation.idLen");
    }
    if (seenIds.has(d.id)) {
      return t("hardwareConfig.validation.idDup");
    }
    seenIds.add(d.id);
    if (!HARDWARE_DEVICE_TYPES.includes(d.device_type as never)) {
      return t("hardwareConfig.validation.badType");
    }
    const pin = d.pins.pin;
    if (pin == null || !Number.isFinite(pin)) {
      return t("hardwareConfig.validation.pinRequired");
    }
    if (pin < HARDWARE_PIN_MIN || pin > HARDWARE_PIN_MAX) {
      return t("hardwareConfig.validation.pinRange");
    }
    if ((HARDWARE_FORBIDDEN_PINS as readonly number[]).includes(pin)) {
      return t("hardwareConfig.validation.pinForbidden");
    }
    if (seenPins.has(pin)) {
      return t("hardwareConfig.validation.pinDup");
    }
    seenPins.add(pin);
    if (d.device_type === "adc_in" && pin > HARDWARE_ADC1_MAX_PIN) {
      return t("hardwareConfig.validation.adcPin");
    }
    if (d.device_type === "pwm_out") {
      pwmCount += 1;
      const hz = d.options?.frequency_hz;
      if (hz != null) {
        const n = typeof hz === "number" ? hz : Number(hz);
        if (
          !Number.isFinite(n) ||
          n < HARDWARE_PWM_FREQ_MIN ||
          n > HARDWARE_PWM_FREQ_MAX
        ) {
          return t("hardwareConfig.validation.pwmFreq");
        }
      }
    }
    if (d.device_type === "dht") {
      const model = d.options?.model;
      if (model != null && typeof model === "string") {
        if (!["dht11", "dht22", "dht21"].includes(model)) {
          return t("hardwareConfig.validation.dhtModel");
        }
      }
      const wf = d.options?.watch_field;
      if (wf != null && typeof wf === "string") {
        if (wf !== "temperature" && wf !== "humidity") {
          return t("hardwareConfig.validation.dhtWatchField");
        }
      }
      const pull = d.options?.pull;
      if (pull != null && typeof pull === "string") {
        if (!["up", "down", "none"].includes(pull)) {
          return t("hardwareConfig.validation.dhtPull");
        }
      }
    }
  }
  if (pwmCount > MAX_PWM_DEVICES) {
    return t("hardwareConfig.validation.maxPwm");
  }

  const i2cList = seg.i2c_sensors ?? [];
  if (i2cList.length > MAX_I2C_SENSORS) {
    return t("hardwareConfig.validation.i2cSensorMax");
  }
  const seenI2cIds = new Set<string>();
  for (const s of i2cList) {
    if (!s.id.trim() || s.id.length > I2C_SENSOR_ID_MAX_LEN) {
      return t("hardwareConfig.validation.i2cSensorIdLen");
    }
    if (seenI2cIds.has(s.id)) {
      return t("hardwareConfig.validation.i2cSensorIdDup");
    }
    seenI2cIds.add(s.id);
    if (devs.some((d) => d.id === s.id)) {
      return t("hardwareConfig.validation.i2cSensorIdConflict");
    }
    if (
      !Number.isFinite(s.addr) ||
      s.addr < I2C_SENSOR_ADDR_MIN ||
      s.addr > I2C_SENSOR_ADDR_MAX
    ) {
      return t("hardwareConfig.validation.i2cSensorAddr");
    }
    if (!I2C_SENSOR_MODELS.includes(s.model as I2cSensorModel)) {
      return t("hardwareConfig.validation.i2cSensorModel");
    }
    if (s.watch_field !== "temperature" && s.watch_field !== "humidity") {
      return t("hardwareConfig.validation.i2cSensorWatchField");
    }
    if (s.what.length > 128) {
      return t("hardwareConfig.validation.i2cSensorWhatLen");
    }
    if (s.how.length > 256) {
      return t("hardwareConfig.validation.i2cSensorHowLen");
    }
    if (s.model === "raw") {
      const init = s.options?.init_cmd;
      if (
        !Array.isArray(init) ||
        init.length === 0 ||
        init.length > I2C_SENSOR_MAX_CMD_LEN
      ) {
        return t("hardwareConfig.validation.i2cSensorRawInit");
      }
      for (let k = 0; k < init.length; k++) {
        const el = init[k];
        const n = typeof el === "number" ? el : Number(el);
        if (!Number.isFinite(n) || n < 0 || n > 255) {
          return t("hardwareConfig.validation.i2cSensorRawInitByte");
        }
      }
      const rl = s.options?.read_len;
      const readLen = typeof rl === "number" ? rl : Number(rl);
      if (
        !Number.isFinite(readLen) ||
        readLen < 1 ||
        readLen > I2C_MAX_READ_LEN_UI
      ) {
        return t("hardwareConfig.validation.i2cSensorRawReadLen");
      }
      const cw = s.options?.conversion_wait_ms;
      if (cw != null) {
        const m = typeof cw === "number" ? cw : Number(cw);
        if (!Number.isFinite(m) || m > 2000) {
          return t("hardwareConfig.validation.i2cSensorRawWait");
        }
      }
    }
  }
  return null;
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
  const saveFeedback = useSaveFeedback(t);
  const { setDirty } = useUnsaved();
  const [saveRestartRequired, setSaveRestartRequired] = useState(false);
  const [draftDevices, setDraftDevices] = useState<DeviceEntry[] | null>(null);
  const [draftI2cSensors, setDraftI2cSensors] = useState<
    I2cSensorEntry[] | null
  >(null);
  /** raw 模型 init_cmd 文本编辑草稿（按列表下标） */
  const [i2cRawInitDraft, setI2cRawInitDraft] = useState<Record<number, string>>(
    {},
  );

  const devices = useMemo(
    () => draftDevices ?? hardwareSegment?.hardware_devices ?? [],
    [draftDevices, hardwareSegment],
  );
  const i2cSensors = useMemo(
    () => draftI2cSensors ?? hardwareSegment?.i2c_sensors ?? [],
    [draftI2cSensors, hardwareSegment],
  );

  useEffect(() => {
    void loadHardwareConfig();
  }, [loadHardwareConfig]);

  const segmentToSave = useMemo(
    () => mergeSegment(hardwareSegment, devices, i2cSensors),
    [hardwareSegment, devices, i2cSensors],
  );

  const saveDisabled = saveFeedback.status === "saving";

  const updateDevice = (index: number, next: DeviceEntry) => {
    setDirty(true);
    setDraftDevices((prev) => {
      const base = prev ?? hardwareSegment?.hardware_devices ?? [];
      const copy = [...base];
      copy[index] = next;
      return copy;
    });
  };

  const removeDevice = (index: number) => {
    setDirty(true);
    setDraftDevices((prev) => {
      const base = prev ?? hardwareSegment?.hardware_devices ?? [];
      return base.filter((_, i) => i !== index);
    });
  };

  const addDevice = () => {
    if (devices.length >= MAX_HARDWARE_DEVICES) return;
    setDirty(true);
    setDraftDevices((prev) => {
      const base = prev ?? hardwareSegment?.hardware_devices ?? [];
      const taken = new Set(base.map((d) => d.id).filter(Boolean));
      return [...base, createNewDevice(taken)];
    });
  };

  const updateI2cSensor = (index: number, next: I2cSensorEntry) => {
    setDirty(true);
    setDraftI2cSensors((prev) => {
      const base = prev ?? hardwareSegment?.i2c_sensors ?? [];
      const copy = [...base];
      copy[index] = next;
      return copy;
    });
  };

  const removeI2cSensor = (index: number) => {
    setDirty(true);
    setDraftI2cSensors((prev) => {
      const base = prev ?? hardwareSegment?.i2c_sensors ?? [];
      return base.filter((_, i) => i !== index);
    });
  };

  const addI2cSensor = () => {
    if (i2cSensors.length >= MAX_I2C_SENSORS) return;
    setDirty(true);
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
    const err = validateSegment(segmentToSave, t);
    if (err) {
      saveFeedback.fail(err);
      return;
    }
    saveFeedback.begin();
    setSaveRestartRequired(false);
    const result = await saveHardwareConfig(segmentToSave);
    saveFeedback.finishFromResult(result);
    if (result.ok) {
      setDirty(false);
      setSaveRestartRequired(Boolean(result.restartRequired));
      setDraftDevices(null);
      setDraftI2cSensors(null);
    }
  };

  if (
    hardwareLoading &&
    !hardwareSegment &&
    draftDevices == null &&
    draftI2cSensors == null
  ) {
    return (
      <SettingsSection
        icon={<MemoryOutlined />}
        label={t("hardwareConfig.sectionMain")}
      >
        <FormLoadingSkeleton />
      </SettingsSection>
    );
  }

  const fieldGridSx = {
    display: "grid",
    gap: 2,
    gridTemplateColumns: {
      xs: "minmax(0, 1fr)",
      md: "repeat(2, minmax(0, 1fr))",
    },
    "& .MuiFormControl-root": { minWidth: 0 },
  } as const;

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 4 }}>
      <InlineAlert message={hardwareError} onRetry={loadHardwareConfig} />
      <SettingsSection
        icon={<MemoryOutlined sx={{ fontSize: "var(--icon-size-md)" }} />}
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
                    ? t("hardwareConfig.restartRequired")
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
              <Box sx={fieldGridSx}>
                <TextField
                  size="small"
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
                  size="small"
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
                  slotProps={{ select: { native: true } }}
                >
                  {HARDWARE_DEVICE_TYPES.map((ty) => (
                    <option key={ty} value={ty}>
                      {ty}
                    </option>
                  ))}
                </TextField>
                <TextField
                  type="number"
                  size="small"
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
                    size="small"
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
                      size="small"
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
                      slotProps={{ select: { native: true } }}
                    >
                      {(["dht11", "dht22", "dht21"] as const).map((m) => (
                        <option key={m} value={m}>
                          {m}
                        </option>
                      ))}
                    </TextField>
                    <TextField
                      select
                      size="small"
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
                      slotProps={{ select: { native: true } }}
                    >
                      <option value="temperature">temperature</option>
                      <option value="humidity">humidity</option>
                    </TextField>
                    <TextField
                      select
                      size="small"
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
                      slotProps={{ select: { native: true } }}
                    >
                      <option value="up">up</option>
                      <option value="down">down</option>
                      <option value="none">none</option>
                    </TextField>
                  </>
                )}
                <TextField
                  size="small"
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
                  size="small"
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
              </Box>
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
                color: "text.secondary",
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
                <Box sx={fieldGridSx}>
                  <TextField
                    size="small"
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
                    size="small"
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
                    size="small"
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
                    slotProps={{ select: { native: true } }}
                  >
                    {I2C_SENSOR_MODELS.map((m) => (
                      <option key={m} value={m}>
                        {m}
                      </option>
                    ))}
                  </TextField>
                  <TextField
                    select
                    size="small"
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
                    slotProps={{ select: { native: true } }}
                  >
                    <option value="temperature">temperature</option>
                    <option value="humidity">humidity</option>
                  </TextField>
                  {sens.model === "raw" && (
                    <>
                      <TextField
                        size="small"
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
                        size="small"
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
                        size="small"
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
                    size="small"
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
                    size="small"
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
                </Box>
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
