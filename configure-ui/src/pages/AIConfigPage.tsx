import { useEffect, useMemo, useState } from "react";
import type { DragEvent } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import FormControl from "@mui/material/FormControl";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Tooltip from "@mui/material/Tooltip";
import type { SelectChangeEvent } from "@mui/material/Select";
import AddRounded from "@mui/icons-material/AddRounded";
import DeleteOutlined from "@mui/icons-material/DeleteOutlined";
import DragIndicatorRounded from "@mui/icons-material/DragIndicatorRounded";
import SaveRounded from "@mui/icons-material/SaveRounded";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Os3dIcon } from "../components/Os3dIcon";
import { OS_ICON_NAV } from "../config/osIcons";
import {
  FormGrid,
  FormLoadingSkeleton,
  PanelStateBlock,
  PanelStateLoading,
  FormSectionSubCollapsible,
  InlineAlert,
  PageLoadErrorState,
  SaveFeedback,
  splitPageErrorState,
} from "../components/form";
import { SettingsSection } from "../components/SettingsSection";
import {
  PAGE_COLUMN_FILL_SX,
  PAGE_STACK_OUTER_SX,
  TEXT_COLOR,
} from "../theme/panelStyles";
import { useConfig } from "../hooks/useConfig";
import { useConfigPageLoad } from "../hooks/useConfigPageLoad";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useSaveFeedback } from "../hooks/useSaveFeedback";
import { useSyncedState } from "../hooks/useSyncedState";
import { useUnsaved } from "../hooks/useUnsaved";
import { useRevealedPasswordFields } from "../hooks/useRevealedPassword";
import type { LlmCustomHeader, LlmModelKind, LlmSource } from "../types/appConfig";
import {
  apiUrlAfterProviderChange,
  DEFAULT_LLM_PROVIDER,
  defaultApiUrlForProvider,
  defaultModelForProvider,
  LLM_PROVIDER_VALUES,
  modelAfterProviderChange,
  type LlmProviderValue,
} from "../constants/llmProviders";

const MAX_LEN = 64;
const MAX_API_URL = 256;
const MAX_HEADER_VALUE = 256;
const AI_CONFIG_DIRTY_OWNER = "ai-config";

/** 与 `LLM_PROVIDER_VALUES` 顺序无关；下拉项与文案一一对应，避免只改数组忘改 MenuItem。 */
const LLM_PROVIDER_LABEL_KEY: Record<LlmProviderValue, string> = {
  anthropic: "config.providerAnthropic",
  openai: "config.providerOpenai",
  openai_compatible: "config.providerOpenaiCompatible",
  gemini: "config.providerGemini",
  glm: "config.providerGlm",
  qwen: "config.providerQwen",
  deepseek: "config.providerDeepseek",
  moonshot: "config.providerMoonshot",
  ollama: "config.providerOllama",
};

const LLM_MODEL_KIND_LABEL_KEY: Record<LlmModelKind, string> = {
  text: "config.llmModelKindText",
  multimodal: "config.llmModelKindMultimodal",
  image_generation: "config.llmModelKindImageGeneration",
  video_generation: "config.llmModelKindVideoGeneration",
};

const LLM_MODEL_KIND_VALUES: LlmModelKind[] = [
  "text",
  "multimodal",
  "image_generation",
  "video_generation",
];

function normalizeProvider(provider: string): LlmProviderValue {
  return (LLM_PROVIDER_VALUES as readonly string[]).includes(provider)
    ? (provider as LlmProviderValue)
    : DEFAULT_LLM_PROVIDER;
}

function normalizeModelKind(modelKind: string): LlmModelKind {
  return (LLM_MODEL_KIND_VALUES as readonly string[]).includes(modelKind)
    ? (modelKind as LlmModelKind)
    : "text";
}

function llmSourceTitle(row: SourceFormRow, t: (k: string) => string): string {
  const provider = t(LLM_PROVIDER_LABEL_KEY[normalizeProvider(row.provider)]).trim();
  const model = row.model.trim();
  return model ? `${provider} ${model}` : provider;
}

function generateSourceId(): string {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return `llm_${globalThis.crypto.randomUUID()}`;
  }
  return `llm_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 10)}`;
}

function normalizeInternalSourceId(rawId: string, usedIds: Set<string>): string {
  let id = rawId.trim();
  while (id.length === 0 || id.length > MAX_LEN || usedIds.has(id)) {
    id = generateSourceId();
  }
  usedIds.add(id);
  return id;
}

type SourceFormRow = LlmSource & { provider: LlmProviderValue };
type LlmDraftState = {
  sources: SourceFormRow[];
};

function toSourceRows(sources: LlmSource[]): SourceFormRow[] {
  const usedIds = new Set<string>();
  return sources.map((s) => ({
    ...s,
    id: normalizeInternalSourceId(s.id, usedIds),
    provider: normalizeProvider(s.provider),
    model_kind: normalizeModelKind(s.model_kind),
    custom_headers: s.custom_headers.map((header) => ({ ...header })),
  }));
}

function validateSources(
  rows: SourceFormRow[],
  t: (k: string) => string,
): string | null {
  if (rows.length === 0) return t("config.validation.llmSourcesNonEmpty");
  for (let i = 0; i < rows.length; i++) {
    const r = rows[i];
    if (r.provider.length > MAX_LEN) return t("config.validation.fieldMax64");
    if (r.api_key.trim().length === 0) return t("config.validation.llmApiKeyRequired");
    if (r.api_key.length > MAX_LEN) return t("config.validation.fieldMax64");
    if (r.model.length > MAX_LEN) return t("config.validation.fieldMax64");
    if (r.api_url.length > MAX_API_URL)
      return t("config.validation.apiUrlMax256");
    if (!LLM_MODEL_KIND_VALUES.includes(r.model_kind))
      return t("config.validation.llmModelKindInvalid");
    const headerNames = new Set<string>();
    for (const header of r.custom_headers) {
      const name = header.name.trim();
      const value = header.value.trim();
      if (!name && value) return t("config.validation.llmHeaderNameRequired");
      if (name.length > MAX_LEN) return t("config.validation.fieldMax64");
      if (value.length > MAX_HEADER_VALUE)
        return t("config.validation.llmHeaderValueMax256");
      if (!name) continue;
      const normalizedName = name.toLowerCase();
      if (headerNames.has(normalizedName))
        return t("config.validation.llmHeaderDuplicate");
      headerNames.add(normalizedName);
    }
  }
  return null;
}

export function AIConfigPage() {
  const { t } = useTranslation();
  const { ready, deviceConnected, canAccessProtectedApis, connectionChecking } = useDeviceApi();
  const { llmConfig, loadLlmConfig, saveLlm, llmLoading, llmError } = useConfig();
  const { setDirtyFor, clearDirtyFor } = useUnsaved();
  const [removeSourceIndex, setRemoveSourceIndex] = useState<number | null>(null);
  const [draggedSourceIndex, setDraggedSourceIndex] = useState<number | null>(null);
  const saveFeedback = useSaveFeedback(t);
  const { isRevealed, getRevealHandlers } = useRevealedPasswordFields();

  const markDirty = () => setDirtyFor(AI_CONFIG_DIRTY_OWNER, true);
  const markClean = () => setDirtyFor(AI_CONFIG_DIRTY_OWNER, false);

  useEffect(() => {
    return () => clearDirtyFor(AI_CONFIG_DIRTY_OWNER);
  }, [clearDirtyFor]);

  useConfigPageLoad({
    hasConfig: llmConfig !== null,
    loading: llmLoading,
    loadConfig: loadLlmConfig,
    canLoad: ready && deviceConnected,
  });
  const syncedDraft = useMemo<LlmDraftState>(() => {
    if (!llmConfig) {
      return {
        sources: [],
      };
    }
    return {
      sources: toSourceRows(llmConfig.llm_sources),
    };
  }, [llmConfig]);
  const [draft, setDraft] = useSyncedState(syncedDraft);
  const { sources } = draft;

  const addSource = () => {
    markDirty();
    setDraft((prev) => ({
      ...prev,
      sources: [
        ...prev.sources,
        {
          id: normalizeInternalSourceId(
            "",
            new Set(prev.sources.map((source) => source.id.trim())),
          ),
          provider: DEFAULT_LLM_PROVIDER,
          api_key: "",
          model: defaultModelForProvider(DEFAULT_LLM_PROVIDER),
          api_url: defaultApiUrlForProvider(DEFAULT_LLM_PROVIDER),
          model_kind: "text",
          custom_headers: [],
        },
      ],
    }));
  };

  const requestRemoveSource = (i: number) => {
    if (sources.length <= 1) return;
    setRemoveSourceIndex(i);
  };
  const confirmRemoveSource = () => {
    if (removeSourceIndex == null) return;
    markDirty();
    setDraft((prev) => ({
      ...prev,
      sources: prev.sources.filter((_, j) => j !== removeSourceIndex),
    }));
    setRemoveSourceIndex(null);
  };

  const updateSource = (i: number, field: keyof LlmSource, value: string) => {
    markDirty();
    setDraft((prev) => {
      const next = [...prev.sources];
      next[i] = { ...next[i], [field]: value };
      return { ...prev, sources: next };
    });
  };

  const updateCustomHeader = (
    sourceIndex: number,
    headerIndex: number,
    field: keyof LlmCustomHeader,
    value: string,
  ) => {
    markDirty();
    setDraft((prev) => {
      const next = [...prev.sources];
      const source = next[sourceIndex];
      const customHeaders = [...source.custom_headers];
      customHeaders[headerIndex] = {
        ...customHeaders[headerIndex],
        [field]: value,
      };
      next[sourceIndex] = { ...source, custom_headers: customHeaders };
      return { ...prev, sources: next };
    });
  };

  const addCustomHeader = (sourceIndex: number) => {
    markDirty();
    setDraft((prev) => {
      const next = [...prev.sources];
      const source = next[sourceIndex];
      next[sourceIndex] = {
        ...source,
        custom_headers: [...source.custom_headers, { name: "", value: "" }],
      };
      return { ...prev, sources: next };
    });
  };

  const removeCustomHeader = (sourceIndex: number, headerIndex: number) => {
    markDirty();
    setDraft((prev) => {
      const next = [...prev.sources];
      const source = next[sourceIndex];
      next[sourceIndex] = {
        ...source,
        custom_headers: source.custom_headers.filter((_, i) => i !== headerIndex),
      };
      return { ...prev, sources: next };
    });
  };

  const moveSource = (fromIndex: number, toIndex: number) => {
    if (fromIndex === toIndex) return;
    markDirty();
    setDraft((prev) => {
      if (
        fromIndex < 0 ||
        toIndex < 0 ||
        fromIndex >= prev.sources.length ||
        toIndex >= prev.sources.length
      ) {
        return prev;
      }
      const next = [...prev.sources];
      const [moved] = next.splice(fromIndex, 1);
      next.splice(toIndex, 0, moved);
      return { ...prev, sources: next };
    });
  };

  const handleSourceDragStart = (event: DragEvent, index: number) => {
    setDraggedSourceIndex(index);
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", String(index));
  };

  const handleSourceDragOver = (event: DragEvent) => {
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
  };

  const handleSourceDrop = (event: DragEvent, toIndex: number) => {
    event.preventDefault();
    const fromRaw = event.dataTransfer.getData("text/plain");
    const fromIndex = fromRaw ? Number.parseInt(fromRaw, 10) : draggedSourceIndex;
    if (typeof fromIndex === "number" && Number.isInteger(fromIndex)) {
      moveSource(fromIndex, toIndex);
    }
    setDraggedSourceIndex(null);
  };

  const changeProvider = (i: number, newProviderRaw: string) => {
    const newP = normalizeProvider(newProviderRaw);
    markDirty();
    setDraft((prev) => {
      const next = [...prev.sources];
      const cur = next[i];
      next[i] = {
        ...cur,
        provider: newP,
        model: modelAfterProviderChange(cur.model, cur.provider, newP),
        api_url: apiUrlAfterProviderChange(cur.api_url, cur.provider, newP),
      };
      return { ...prev, sources: next };
    });
  };

  const handleSave = async () => {
    if (!llmConfig) return;
    const err = validateSources(sources, t);
    if (err) {
      saveFeedback.fail(err);
      return;
    }
    const usedIds = new Set<string>();
    const llm_sources: LlmSource[] = sources.map((r) => ({
      id: normalizeInternalSourceId(r.id, usedIds),
      provider: r.provider.trim(),
      api_key: r.api_key.trim(),
      model: r.model.trim(),
      api_url: r.api_url.trim(),
      max_tokens: r.max_tokens ?? null,
      model_kind: normalizeModelKind(r.model_kind),
      custom_headers: r.custom_headers
        .map((header) => ({
          name: header.name.trim(),
          value: header.value.trim(),
        }))
        .filter((header) => header.name.length > 0 || header.value.length > 0),
    }));
    saveFeedback.begin();
    const result = await saveLlm({
      llm_sources,
    });
    saveFeedback.finishFromResult(result);
    if (result.ok) markClean();
  };

  if (llmLoading && !llmConfig) {
    return (
      <Box sx={PAGE_COLUMN_FILL_SX}>
        <SettingsSection
          pinHeader
          surfaceTone="loading"
          sx={{ flex: 1, minHeight: 0 }}
          icon={<Os3dIcon src={OS_ICON_NAV["/ai-config"]} />}
          label={t("config.sectionLlm")}
        >
          <PanelStateLoading>
            <FormLoadingSkeleton />
          </PanelStateLoading>
        </SettingsSection>
      </Box>
    );
  }

  const saveDisabled = !llmConfig || saveFeedback.status === "saving";
  const showConnectionLoading =
    !llmConfig && !llmLoading && ready && connectionChecking && !deviceConnected;
  const showConnectState =
    !llmConfig && !llmLoading && !showConnectionLoading && (!ready || !deviceConnected);
  const showPairingState =
    !llmConfig && !llmLoading && ready && deviceConnected && !canAccessProtectedApis;
  const loadErrorState = splitPageErrorState({
    hasData: Boolean(llmConfig),
    loading: llmLoading,
    error: llmError,
    suppress: showConnectState || showPairingState || showConnectionLoading,
  });

  return (
    <Box sx={PAGE_STACK_OUTER_SX}>
      <InlineAlert message={loadErrorState.inlineError} onRetry={loadLlmConfig} />
      <ConfirmDialog
        open={removeSourceIndex != null}
        onClose={() => setRemoveSourceIndex(null)}
        title={t("config.llmRemoveSourceConfirmTitle")}
        description={t("config.llmRemoveSourceConfirmDesc")}
        icon={<DeleteOutlined />}
        confirmColor="error"
        confirmLabel={t("common.remove")}
        onConfirm={confirmRemoveSource}
      />
      <SettingsSection
        pinHeader
        sx={{ flex: 1, minHeight: 0 }}
        icon={<Os3dIcon src={OS_ICON_NAV["/ai-config"]} />}
        label={t("config.sectionLlm")}
        description={t("config.sectionLlmDesc")}
        accessory={
          <Button
            size="small"
            variant="contained"
            startIcon={<SaveRounded />}
            onClick={handleSave}
            disabled={saveDisabled}
            title={!llmConfig ? t("config.hintSaveNeedDevice") : undefined}
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
        {showConnectionLoading ? (
          <PanelStateLoading>
            <FormLoadingSkeleton />
          </PanelStateLoading>
        ) : showConnectState ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/ai-config"]} variant="inline" />}
            title={ready ? t("device.connectFirst") : t("device.bannerNeedDevice")}
            description={t("config.connectDesc")}
          />
        ) : showPairingState ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/ai-config"]} variant="inline" />}
            title={t("device.pairingCodeRequired")}
            description={t("config.needPairingDesc")}
          />
        ) : loadErrorState.blockingError ? (
          <PageLoadErrorState
            message={loadErrorState.blockingError}
            onRetry={loadLlmConfig}
          />
        ) : !llmConfig ? (
          <PanelStateBlock
            tone="neutral"
            icon={<Os3dIcon src={OS_ICON_NAV["/ai-config"]} variant="inline" />}
            title={t("config.unavailableTitle")}
            description={t("config.unavailableDesc")}
          />
        ) : (
          <Stack spacing={2.5}>
            <Stack spacing={1.5}>
              {sources.map((row, i) => (
                <Box
                  key={`${row.id}-${i}`}
                  onDragOver={handleSourceDragOver}
                  onDrop={(event) => handleSourceDrop(event, i)}
                  sx={{
                    opacity: draggedSourceIndex === i ? 0.56 : 1,
                    transition: "opacity var(--ease-out-smooth)",
                  }}
                >
                  <FormSectionSubCollapsible
                    title={llmSourceTitle(row, t)}
                    idBase={`llm-source-${row.id}`}
                    defaultOpen={i === 0}
                    action={
                      <Stack
                        component="span"
                        direction="row"
                        spacing={0.25}
                        alignItems="center"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <Tooltip title={t("config.dragLlmSource")}>
                          <Box
                            component="span"
                            role="button"
                            tabIndex={0}
                            draggable
                            onDragStart={(event) => handleSourceDragStart(event, i)}
                            onDragEnd={() => setDraggedSourceIndex(null)}
                            sx={{
                              display: "inline-flex",
                              alignItems: "center",
                              justifyContent: "center",
                              p: 0.5,
                              color: TEXT_COLOR.tertiary,
                              cursor: "grab",
                              borderRadius: "var(--radius-control)",
                              "&:focus-visible": {
                                outline:
                                  "var(--focus-ring-width) solid color-mix(in srgb, var(--primary) 55%, transparent)",
                                outlineOffset: "var(--focus-ring-offset)",
                              },
                            }}
                            aria-label={t("config.dragLlmSource")}
                          >
                            <DragIndicatorRounded fontSize="small" />
                          </Box>
                        </Tooltip>
                        <Tooltip title={t("common.remove")}>
                          <Box
                            component="span"
                            role="button"
                            tabIndex={0}
                            onClick={() => requestRemoveSource(i)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter" || e.key === " ") {
                                e.preventDefault();
                                requestRemoveSource(i);
                              }
                            }}
                            sx={{
                              display: "inline-flex",
                              alignItems: "center",
                              justifyContent: "center",
                              p: 0.5,
                              color: TEXT_COLOR.tertiary,
                              cursor: sources.length <= 1 ? "default" : "pointer",
                              opacity: sources.length <= 1 ? 0.5 : 1,
                              borderRadius: "var(--radius-control)",
                              "&:focus-visible": {
                                outline:
                                  "var(--focus-ring-width) solid color-mix(in srgb, var(--primary) 55%, transparent)",
                                outlineOffset: "var(--focus-ring-offset)",
                              },
                            }}
                            aria-label={t("common.remove")}
                            aria-disabled={sources.length <= 1}
                          >
                            <DeleteOutlined fontSize="small" />
                          </Box>
                        </Tooltip>
                      </Stack>
                    }
                  >
                    <Stack spacing={2}>
                      <FormGrid>
                        <FormControl fullWidth>
                          <InputLabel id={`llm-provider-${i}`}>
                            {t("config.llmProvider")}
                          </InputLabel>
                          <Select
                            labelId={`llm-provider-${i}`}
                            label={t("config.llmProvider")}
                            value={normalizeProvider(row.provider)}
                            onChange={(e: SelectChangeEvent<string>) =>
                              changeProvider(i, e.target.value)
                            }
                          >
                            {LLM_PROVIDER_VALUES.map((p) => (
                              <MenuItem key={p} value={p}>
                                {t(LLM_PROVIDER_LABEL_KEY[p])}
                              </MenuItem>
                            ))}
                          </Select>
                        </FormControl>
                        <FormControl fullWidth>
                          <InputLabel id={`llm-model-kind-${i}`}>
                            {t("config.llmModelKind")}
                          </InputLabel>
                          <Select
                            labelId={`llm-model-kind-${i}`}
                            label={t("config.llmModelKind")}
                            value={normalizeModelKind(row.model_kind)}
                            onChange={(e: SelectChangeEvent<string>) =>
                              updateSource(i, "model_kind", normalizeModelKind(e.target.value))
                            }
                          >
                            {LLM_MODEL_KIND_VALUES.map((kind) => (
                              <MenuItem key={kind} value={kind}>
                                {t(LLM_MODEL_KIND_LABEL_KEY[kind])}
                              </MenuItem>
                            ))}
                          </Select>
                        </FormControl>
                        <TextField
                          label={t("config.llmApiKey")}
                          value={row.api_key}
                          onChange={(e) => updateSource(i, "api_key", e.target.value)}
                          type={isRevealed(`api_key_${i}`) ? "text" : "password"}
                          fullWidth
                          slotProps={{
                            htmlInput: {
                              maxLength: MAX_LEN,
                              style: { fontFamily: "var(--font-mono)" },
                              ...getRevealHandlers(`api_key_${i}`),
                            },
                          }}
                        />
                        <TextField
                          label={t("config.llmModel")}
                          value={row.model}
                          onChange={(e) => updateSource(i, "model", e.target.value)}
                          fullWidth
                          placeholder={defaultModelForProvider(row.provider)}
                          slotProps={{ htmlInput: { maxLength: MAX_LEN } }}
                        />
                        <TextField
                          label={t("config.llmApiUrl")}
                          value={row.api_url}
                          onChange={(e) => updateSource(i, "api_url", e.target.value)}
                          fullWidth
                          placeholder={defaultApiUrlForProvider(row.provider)}
                          slotProps={{
                            htmlInput: {
                              maxLength: MAX_API_URL,
                              style: { fontFamily: "var(--font-mono)" },
                            },
                          }}
                        />
                      </FormGrid>
                      <Stack spacing={1.25}>
                        <Box
                          component="span"
                          sx={{
                            color: TEXT_COLOR.tertiary,
                            fontSize: "var(--font-size-caption)",
                            fontWeight: 600,
                          }}
                        >
                          {t("config.llmCustomHeaders")}
                        </Box>
                        {row.custom_headers.map((header, headerIndex) => (
                          <FormGrid key={headerIndex}>
                            <TextField
                              label={t("config.llmHeaderName")}
                              value={header.name}
                              onChange={(e) =>
                                updateCustomHeader(i, headerIndex, "name", e.target.value)
                              }
                              fullWidth
                              slotProps={{ htmlInput: { maxLength: MAX_LEN } }}
                            />
                            <TextField
                              label={t("config.llmHeaderValue")}
                              value={header.value}
                              onChange={(e) =>
                                updateCustomHeader(i, headerIndex, "value", e.target.value)
                              }
                              fullWidth
                              slotProps={{
                                htmlInput: {
                                  maxLength: MAX_HEADER_VALUE,
                                  style: { fontFamily: "var(--font-mono)" },
                                },
                              }}
                            />
                            <Button
                              variant="text"
                              color="inherit"
                              size="small"
                              startIcon={<DeleteOutlined />}
                              onClick={() => removeCustomHeader(i, headerIndex)}
                              sx={{
                                alignSelf: "center",
                                justifySelf: "start",
                                borderRadius: "var(--radius-control)",
                              }}
                            >
                              {t("common.remove")}
                            </Button>
                          </FormGrid>
                        ))}
                        <Button
                          startIcon={<AddRounded />}
                          onClick={() => addCustomHeader(i)}
                          variant="text"
                          color="inherit"
                          size="small"
                          sx={{
                            alignSelf: "flex-start",
                            borderRadius: "var(--radius-control)",
                            color: "var(--primary)",
                            fontSize: "var(--font-size-body-sm)",
                            fontWeight: 600,
                            minHeight: 30,
                            px: 0.5,
                          }}
                        >
                          {t("config.addCustomHeader")}
                        </Button>
                      </Stack>
                    </Stack>
                  </FormSectionSubCollapsible>
                </Box>
              ))}
            </Stack>
            <Button
              startIcon={<AddRounded />}
              onClick={addSource}
              variant="outlined"
              size="small"
              sx={{
                alignSelf: "flex-start",
                borderRadius: "var(--radius-control)",
              }}
            >
              {t("config.addLlmSource")}
            </Button>
          </Stack>
        )}
      </SettingsSection>
    </Box>
  );
}
