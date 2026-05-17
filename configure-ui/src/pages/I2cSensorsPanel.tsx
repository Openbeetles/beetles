import MenuItem from "@mui/material/MenuItem";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import IconButton from "@mui/material/IconButton";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
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
import type { I2cSensorEntry } from "../types/hardwareConfig";
import {
  I2C_MAX_READ_LEN_UI,
  I2C_SENSOR_ADDR_MAX,
  I2C_SENSOR_ADDR_MIN,
  I2C_SENSOR_MAX_CMD_LEN,
  I2C_SENSOR_MODELS,
  MAX_I2C_SENSORS,
} from "../types/hardwareConfig";
import { generateDeviceId } from "../util/hardwareDeviceId";
import { mergeI2cSensorConfig } from "./hardwareSegmentDraft";
import { validateHardwareSegment } from "./hardwareConfigValidation";

function asNumber(v: string): number | null {
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
}

function createNewI2cSensor(taken: Set<string>): I2cSensorEntry {
  const id = generateDeviceId(taken);
  taken.add(id);
  return {
    id,
    addr: 0x38,
    model: "aht20",
    what: "",
    how: "",
    options: {},
  };
}

export function I2cSensorsPanel() {
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
  const [draftI2cSensors, setDraftI2cSensors] = useState<
    I2cSensorEntry[] | null
  >(null);
  const [i2cRawInitDraft, setI2cRawInitDraft] = useState<Record<number, string>>(
    {},
  );
  const hasSegmentSource = hardwareSegment !== null || draftI2cSensors !== null;
  const loadErrorState = splitPageErrorState({
    hasData: hasSegmentSource,
    loading: hardwareLoading,
    error: hardwareError,
  });

  const i2cBus = hardwareSegment?.i2c_bus ?? null;
  const i2cSensors = useMemo(
    () => draftI2cSensors ?? hardwareSegment?.i2c_sensors ?? [],
    [draftI2cSensors, hardwareSegment],
  );
  const segmentToSave = useMemo(
    () => mergeI2cSensorConfig(hardwareSegment, i2cBus, i2cSensors),
    [hardwareSegment, i2cBus, i2cSensors],
  );
  const saveDisabled = editor.saveDisabled;

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
      for (const d of hardwareSegment?.hardware_devices ?? []) {
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
        setDraftI2cSensors(null);
      },
    });
  };

  if (hardwareLoading && !hardwareSegment && draftI2cSensors == null) {
    return (
      <Box sx={PAGE_COLUMN_FILL_SX}>
        <SettingsSection
          pinHeader
          surfaceTone="loading"
          sx={{ flex: 1, minHeight: 0 }}
          icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.i2cSensors} />}
          label={t("i2cSensorsConfig.sectionMain")}
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
          icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.i2cSensors} />}
          label={t("i2cSensorsConfig.sectionMain")}
          description={t("i2cSensorsConfig.sectionMainDesc")}
        >
          {loadErrorState.blockingError ? (
            <PageLoadErrorState
              message={loadErrorState.blockingError}
              onRetry={loadHardwareConfig}
            />
          ) : (
            <PanelStateBlock
              tone="neutral"
              icon={
                <Os3dIcon
                  src={OS_ICON_DEVICE_CONFIG.i2cSensors}
                  variant="inline"
                />
              }
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
        icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.i2cSensors} />}
        label={t("i2cSensorsConfig.sectionMain")}
        description={t("i2cSensorsConfig.sectionMainDesc")}
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
          {i2cSensors.map((sens, i) => (
            <FormSectionSubCollapsible
              key={`${sens.id}-${i}`}
              title={t("hardwareConfig.i2cSensorCollapseTitle", {
                index: i + 1,
                id: sens.id || "—",
              })}
              defaultOpen={i === 0}
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
          <Box>
            <Button
              size="small"
              variant="outlined"
              startIcon={<AddRounded />}
              onClick={addI2cSensor}
              disabled={i2cBus == null || i2cSensors.length >= MAX_I2C_SENSORS}
            >
              {t("hardwareConfig.addI2cSensor")}
            </Button>
            {i2cBus == null ? (
              <Typography
                variant="caption"
                sx={{
                  mt: 0.75,
                  display: "block",
                  color: "var(--semantic-danger)",
                  fontSize: "var(--font-size-caption)",
                }}
              >
                {t("i2cSensorsConfig.busRequiredDesc")}
              </Typography>
            ) : null}
          </Box>
        </FormFieldStack>
      </SettingsSection>
    </Box>
  );
}
