import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import SaveRounded from "@mui/icons-material/SaveRounded";
import {
  FormFieldStack,
  FormLoadingSkeleton,
  FormGrid,
  FormSectionSubCollapsible,
  InlineAlert,
  PanelStateBlock,
  PanelStateLoading,
  PageLoadErrorState,
  SaveFeedback,
  splitPageErrorState,
} from "../components/form";
import { Os3dIcon } from "../components/Os3dIcon";
import { SettingsSection } from "../components/SettingsSection";
import { OS_ICON_DEVICE_CONFIG } from "../config/osIcons";
import { useConfig } from "../hooks/useConfig";
import { useConfigEditorController } from "../hooks/useConfigEditorController";
import { PAGE_COLUMN_FILL_SX, PAGE_STACK_OUTER_SX } from "../theme/panelStyles";
import { defaultAudioConfig } from "../types/audioConfig";
import type { HardwareSegment, I2cBusConfig, I2sBusConfig } from "../types/hardwareConfig";
import {
  defaultHardwareSegment,
  HARDWARE_PIN_MAX,
  HARDWARE_PIN_MIN,
  I2C_BUS_DEFAULT_FREQ_HZ,
  I2C_BUS_DEFAULT_SCL_PIN,
  I2C_BUS_DEFAULT_SDA_PIN,
  I2C_BUS_FREQ_MAX,
  I2C_BUS_FREQ_MIN,
  I2S_BUS_DEFAULT_BCLK_PIN,
  I2S_BUS_DEFAULT_DIN_PIN,
  I2S_BUS_DEFAULT_DOUT_PIN,
  I2S_BUS_DEFAULT_MCLK_PIN,
  I2S_BUS_DEFAULT_WS_PIN,
} from "../types/hardwareConfig";
import { validateHardwareSegment } from "./hardwareConfigValidation";

function asNumber(v: string): number | null {
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
}

function defaultI2cBus(): I2cBusConfig {
  return {
    sda_pin: I2C_BUS_DEFAULT_SDA_PIN,
    scl_pin: I2C_BUS_DEFAULT_SCL_PIN,
    freq_hz: I2C_BUS_DEFAULT_FREQ_HZ,
  };
}

function defaultI2sBus(): I2sBusConfig {
  return {
    mclk_pin: I2S_BUS_DEFAULT_MCLK_PIN,
    ws_pin: I2S_BUS_DEFAULT_WS_PIN,
    bclk_pin: I2S_BUS_DEFAULT_BCLK_PIN,
    din_pin: I2S_BUS_DEFAULT_DIN_PIN,
    dout_pin: I2S_BUS_DEFAULT_DOUT_PIN,
  };
}

function mergeBusConfig(
  base: HardwareSegment | null,
  i2cBus: I2cBusConfig | null,
  i2sBus: I2sBusConfig | null,
): HardwareSegment {
  return {
    ...(base ?? defaultHardwareSegment()),
    i2c_bus: i2cBus,
    i2s_bus: i2sBus,
  };
}

export function BusConfigPanel() {
  const { t } = useTranslation();
  const {
    audioConfig,
    audioLoading,
    loadAudioConfig,
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
  const [draftI2cBus, setDraftI2cBus] = useState<I2cBusConfig | null>();
  const [draftI2sBus, setDraftI2sBus] = useState<I2sBusConfig | null>();
  const hasSegmentSource =
    hardwareSegment !== null ||
    draftI2cBus !== undefined ||
    draftI2sBus !== undefined;
  const loadErrorState = splitPageErrorState({
    hasData: hasSegmentSource,
    loading: hardwareLoading,
    error: hardwareError,
  });

  useEffect(() => {
    if (!audioLoading && audioConfig == null) {
      void loadAudioConfig();
    }
  }, [audioConfig, audioLoading, loadAudioConfig]);

  const topology = audioConfig?.topology ?? defaultAudioConfig().topology;
  const codecTopology = topology === "i2s_codec";
  const i2cBus = useMemo(
    () =>
      draftI2cBus !== undefined ? draftI2cBus : hardwareSegment?.i2c_bus ?? null,
    [draftI2cBus, hardwareSegment],
  );
  const i2sBus = useMemo(
    () =>
      draftI2sBus !== undefined ? draftI2sBus : hardwareSegment?.i2s_bus ?? null,
    [draftI2sBus, hardwareSegment],
  );
  const segmentToSave = useMemo(
    () => mergeBusConfig(hardwareSegment, i2cBus, i2sBus),
    [hardwareSegment, i2cBus, i2sBus],
  );
  const saveDisabled = editor.saveDisabled || (audioLoading && audioConfig == null);
  const busFieldHelp = codecTopology
    ? t("busConfig.requiredWhenCodec")
    : t("busConfig.optionalWhenDiscrete");

  const updateI2cBus = (patch: Partial<I2cBusConfig>) => {
    editor.markDirty();
    setDraftI2cBus((prev) => ({
      ...(prev ?? hardwareSegment?.i2c_bus ?? defaultI2cBus()),
      ...patch,
    }));
  };

  const updateI2sBus = (patch: Partial<I2sBusConfig>) => {
    editor.markDirty();
    setDraftI2sBus((prev) => ({
      ...(prev ?? hardwareSegment?.i2s_bus ?? defaultI2sBus()),
      ...patch,
    }));
  };

  const save = async () => {
    await editor.runSave({
      validate: () => {
        if (audioConfig == null) {
          return t("busConfig.validation.audioConfigUnavailable");
        }
        return validateHardwareSegment(segmentToSave, t, audioConfig);
      },
      onBeforeSave: () => setSaveRestartRequired(false),
      performSave: () => saveHardwareConfig(segmentToSave),
      onSuccess: (result) => {
        setSaveRestartRequired(Boolean(result.restartRequired));
        setDraftI2cBus(undefined);
        setDraftI2sBus(undefined);
      },
    });
  };

  if (hardwareLoading && !hardwareSegment && draftI2cBus === undefined && draftI2sBus === undefined) {
    return (
      <Box sx={PAGE_COLUMN_FILL_SX}>
        <SettingsSection
          pinHeader
          surfaceTone="loading"
          sx={{ flex: 1, minHeight: 0 }}
          icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.buses} />}
          label={t("busConfig.sectionMain")}
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
          icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.buses} />}
          label={t("busConfig.sectionMain")}
        >
          {loadErrorState.blockingError ? (
            <PageLoadErrorState
              message={loadErrorState.blockingError}
              onRetry={loadHardwareConfig}
            />
          ) : (
            <PanelStateBlock
              tone="neutral"
              icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.buses} variant="inline" />}
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
        icon={<Os3dIcon src={OS_ICON_DEVICE_CONFIG.buses} />}
        label={t("busConfig.sectionMain")}
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
          <FormSectionSubCollapsible
            title={t("busConfig.sectionI2c")}
            defaultOpen
            action={
              <Button
                size="small"
                variant="text"
                onClick={(event) => {
                  event.stopPropagation();
                  editor.markDirty();
                  setDraftI2cBus(null);
                }}
              >
                {t("busConfig.clearBus")}
              </Button>
            }
          >
            <FormGrid>
              <TextField
                type="number"
                required={codecTopology}
                label={t("busConfig.sdaPin")}
                helperText={busFieldHelp}
                value={i2cBus?.sda_pin ?? ""}
                onChange={(event) => {
                  const n = asNumber(event.target.value);
                  updateI2cBus({ sda_pin: n ?? I2C_BUS_DEFAULT_SDA_PIN });
                }}
                slotProps={{
                  htmlInput: { min: HARDWARE_PIN_MIN, max: HARDWARE_PIN_MAX },
                }}
              />
              <TextField
                type="number"
                required={codecTopology}
                label={t("busConfig.sclPin")}
                helperText={busFieldHelp}
                value={i2cBus?.scl_pin ?? ""}
                onChange={(event) => {
                  const n = asNumber(event.target.value);
                  updateI2cBus({ scl_pin: n ?? I2C_BUS_DEFAULT_SCL_PIN });
                }}
                slotProps={{
                  htmlInput: { min: HARDWARE_PIN_MIN, max: HARDWARE_PIN_MAX },
                }}
              />
              <TextField
                type="number"
                required={codecTopology}
                label={t("busConfig.freqHz")}
                helperText={codecTopology ? t("busConfig.freqRequiredHelp") : t("busConfig.freqOptionalHelp")}
                value={i2cBus?.freq_hz ?? ""}
                onChange={(event) => {
                  const raw = event.target.value.trim();
                  const n = raw === "" ? undefined : asNumber(raw);
                  updateI2cBus({
                    freq_hz: n == null ? undefined : Math.round(n),
                  });
                }}
                slotProps={{
                  htmlInput: { min: I2C_BUS_FREQ_MIN, max: I2C_BUS_FREQ_MAX },
                }}
              />
            </FormGrid>
          </FormSectionSubCollapsible>

          <FormSectionSubCollapsible
            title={t("busConfig.sectionI2s")}
            defaultOpen
            action={
              <Button
                size="small"
                variant="text"
                onClick={(event) => {
                  event.stopPropagation();
                  editor.markDirty();
                  setDraftI2sBus(null);
                }}
              >
                {t("busConfig.clearBus")}
              </Button>
            }
          >
            <FormGrid>
              <TextField
                type="number"
                required={codecTopology}
                label={t("busConfig.mclkPin")}
                helperText={busFieldHelp}
                value={i2sBus?.mclk_pin ?? ""}
                onChange={(event) => {
                  const n = asNumber(event.target.value);
                  updateI2sBus({ mclk_pin: n ?? I2S_BUS_DEFAULT_MCLK_PIN });
                }}
                slotProps={{
                  htmlInput: { min: HARDWARE_PIN_MIN, max: HARDWARE_PIN_MAX },
                }}
              />
              <TextField
                type="number"
                required={codecTopology}
                label={t("busConfig.wsPin")}
                helperText={busFieldHelp}
                value={i2sBus?.ws_pin ?? ""}
                onChange={(event) => {
                  const n = asNumber(event.target.value);
                  updateI2sBus({ ws_pin: n ?? I2S_BUS_DEFAULT_WS_PIN });
                }}
                slotProps={{
                  htmlInput: { min: HARDWARE_PIN_MIN, max: HARDWARE_PIN_MAX },
                }}
              />
              <TextField
                type="number"
                required={codecTopology}
                label={t("busConfig.bclkPin")}
                helperText={busFieldHelp}
                value={i2sBus?.bclk_pin ?? ""}
                onChange={(event) => {
                  const n = asNumber(event.target.value);
                  updateI2sBus({ bclk_pin: n ?? I2S_BUS_DEFAULT_BCLK_PIN });
                }}
                slotProps={{
                  htmlInput: { min: HARDWARE_PIN_MIN, max: HARDWARE_PIN_MAX },
                }}
              />
              <TextField
                type="number"
                required={codecTopology}
                label={t("busConfig.dinPin")}
                helperText={busFieldHelp}
                value={i2sBus?.din_pin ?? ""}
                onChange={(event) => {
                  const n = asNumber(event.target.value);
                  updateI2sBus({ din_pin: n ?? I2S_BUS_DEFAULT_DIN_PIN });
                }}
                slotProps={{
                  htmlInput: { min: HARDWARE_PIN_MIN, max: HARDWARE_PIN_MAX },
                }}
              />
              <TextField
                type="number"
                required={codecTopology}
                label={t("busConfig.doutPin")}
                helperText={busFieldHelp}
                value={i2sBus?.dout_pin ?? ""}
                onChange={(event) => {
                  const n = asNumber(event.target.value);
                  updateI2sBus({ dout_pin: n ?? I2S_BUS_DEFAULT_DOUT_PIN });
                }}
                slotProps={{
                  htmlInput: { min: HARDWARE_PIN_MIN, max: HARDWARE_PIN_MAX },
                }}
              />
            </FormGrid>
          </FormSectionSubCollapsible>
        </FormFieldStack>
      </SettingsSection>
    </Box>
  );
}
