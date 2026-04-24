import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import FormControl from "@mui/material/FormControl";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import type { SelectChangeEvent } from "@mui/material/Select";
import AddRounded from "@mui/icons-material/AddRounded";
import DeleteOutlined from "@mui/icons-material/DeleteOutlined";
import SaveRounded from "@mui/icons-material/SaveRounded";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Os3dIcon } from "../components/Os3dIcon";
import { OS_ICON_NAV } from "../config/osIcons";
import {
  FormFieldStack,
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
import { LAYOUT_TOKENS } from "../config/themeTokens";
import { useConfig } from "../hooks/useConfig";
import { useConfigPageLoad } from "../hooks/useConfigPageLoad";
import { useDeviceApi } from "../hooks/useDeviceApi";
import { useSaveFeedback } from "../hooks/useSaveFeedback";
import { useSyncedState } from "../hooks/useSyncedState";
import { useUnsaved } from "../hooks/useUnsaved";
import { useRevealedPasswordFields } from "../hooks/useRevealedPassword";
import type { LlmSource } from "../types/appConfig";
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

function normalizeProvider(provider: string): LlmProviderValue {
  return (LLM_PROVIDER_VALUES as readonly string[]).includes(provider)
    ? (provider as LlmProviderValue)
    : DEFAULT_LLM_PROVIDER;
}

type SourceFormRow = LlmSource & { provider: LlmProviderValue };
type LlmDraftState = {
  sources: SourceFormRow[];
  routerIndex: number | null;
  workerIndex: number | null;
};

function toSourceRows(sources: LlmSource[]): SourceFormRow[] {
  return sources.map((s) => ({
    ...s,
    provider: normalizeProvider(s.provider),
  }));
}

const LLM_SOURCE_LABEL_MAX_MODEL = 40;
const LLM_SOURCE_LABEL_MAX_HOST = 28;

function hostFromApiUrl(url: string): string {
  const u = url.trim();
  if (!u) return "";
  try {
    const parsed = new URL(u);
    return parsed.host || u.slice(0, LLM_SOURCE_LABEL_MAX_HOST);
  } catch {
    return u.slice(0, LLM_SOURCE_LABEL_MAX_HOST);
  }
}

function truncateMiddle(s: string, max: number): string {
  if (s.length <= max) return s;
  if (max <= 3) return "…";
  const head = Math.ceil((max - 1) / 2);
  const tail = max - 1 - head;
  return `${s.slice(0, head)}…${s.slice(s.length - tail)}`;
}

/** 主用/备用下拉里区分「同 provider、不同模型/端点」的源。 */
function formatLlmSourceSelectLabel(
  index: number,
  row: SourceFormRow,
  t: (key: string) => string,
): string {
  const raw = row.provider.trim();
  const provDisplay =
    (LLM_PROVIDER_VALUES as readonly string[]).includes(raw)
      ? t(LLM_PROVIDER_LABEL_KEY[raw as LlmProviderValue])
      : raw || t("config.llmSource");
  const model = row.model.trim();
  const host = hostFromApiUrl(row.api_url);
  let detail: string;
  if (model) {
    detail = truncateMiddle(model, LLM_SOURCE_LABEL_MAX_MODEL);
  } else if (host) {
    detail = truncateMiddle(host, LLM_SOURCE_LABEL_MAX_HOST);
  } else {
    detail = t("config.llmOptionNoModel");
  }
  return `#${index} · ${provDisplay} · ${detail}`;
}

function validateSources(
  rows: SourceFormRow[],
  routerIndex: number | null,
  workerIndex: number | null,
  t: (k: string) => string,
): string | null {
  if (rows.length === 0) return t("config.validation.llmSourcesNonEmpty");
  const n = rows.length;
  for (let i = 0; i < rows.length; i++) {
    const r = rows[i];
    if (r.provider.length > MAX_LEN) return t("config.validation.fieldMax64");
    if (r.api_key.length > MAX_LEN) return t("config.validation.fieldMax64");
    if (r.model.length > MAX_LEN) return t("config.validation.fieldMax64");
    if (r.api_url.length > MAX_API_URL)
      return t("config.validation.apiUrlMax256");
  }
  if (routerIndex != null && (routerIndex < 0 || routerIndex >= n))
    return t("config.validation.routerIndexInRange");
  if (workerIndex != null && (workerIndex < 0 || workerIndex >= n))
    return t("config.validation.workerIndexInRange");
  return null;
}

export function AIConfigPage() {
  const { t } = useTranslation();
  const { ready, deviceConnected, canAccessProtectedApis, connectionChecking } = useDeviceApi();
  const { llmConfig, loadLlmConfig, saveLlm, llmLoading, llmError } = useConfig();
  const { setDirtyFor, clearDirtyFor } = useUnsaved();
  const [removeSourceIndex, setRemoveSourceIndex] = useState<number | null>(null);
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
        routerIndex: null,
        workerIndex: null,
      };
    }
    return {
      sources: toSourceRows(llmConfig.llm_sources),
      routerIndex: llmConfig.llm_router_source_index ?? null,
      workerIndex: llmConfig.llm_worker_source_index ?? null,
    };
  }, [llmConfig]);
  const [draft, setDraft] = useSyncedState(syncedDraft);
  const { sources, routerIndex, workerIndex } = draft;

  const addSource = () => {
    markDirty();
    setDraft((prev) => ({
      ...prev,
      sources: [
        ...prev.sources,
        {
          provider: DEFAULT_LLM_PROVIDER,
          api_key: "",
          model: defaultModelForProvider(DEFAULT_LLM_PROVIDER),
          api_url: defaultApiUrlForProvider(DEFAULT_LLM_PROVIDER),
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
    const err = validateSources(sources, routerIndex, workerIndex, t);
    if (err) {
      saveFeedback.fail(err);
      return;
    }
    const llm_sources: LlmSource[] = sources.map((r) => ({
      provider: r.provider.trim(),
      api_key: r.api_key.trim(),
      model: r.model.trim(),
      api_url: r.api_url.trim(),
    }));
    saveFeedback.begin();
    const result = await saveLlm({
      llm_sources,
      llm_router_source_index: routerIndex,
      llm_worker_source_index: workerIndex,
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
        <Stack spacing={0}>
          {sources.map((row, i) => (
            <FormSectionSubCollapsible
              key={i}
              title={`${t("config.llmSource")} ${i + 1}`}
              defaultOpen={
                routerIndex !== null && workerIndex !== null
                  ? i === routerIndex || i === workerIndex
                  : i === 0
              }
              action={
                <Box
                  component="span"
                  role="button"
                  tabIndex={0}
                  onClick={(e) => {
                    e.stopPropagation();
                    requestRemoveSource(i);
                  }}
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
              }
            >
              <FormFieldStack>
                <FormControl fullWidth>
                  <InputLabel id={`llm-provider-${i}`}>{t("config.llmProvider")}</InputLabel>
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
              </FormFieldStack>
            </FormSectionSubCollapsible>
          ))}
          <Button
            startIcon={<AddRounded />}
            onClick={addSource}
            variant="outlined"
            size="small"
            sx={{
              mt: 2,
              alignSelf: "flex-start",
              borderRadius: "var(--radius-control)",
            }}
          >
            {t("config.addLlmSource")}
          </Button>
          <FormSectionSubCollapsible
            title={t("config.llmRouterWorkerTitle")}
            defaultOpen={true}
          >
            <Box
              sx={{
                display: "flex",
                gap: LAYOUT_TOKENS.spacingSectionStack,
                flexWrap: "wrap",
              }}
            >
              <TextField
                select
                label={t("config.llmRouterIndex")}
                value={routerIndex === null ? "" : String(routerIndex)}
                onChange={(e) => {
                  markDirty();
                  const v = e.target.value;
                  setDraft((prev) => ({
                    ...prev,
                    routerIndex:
                      v === "" ? null : Math.max(0, parseInt(v, 10) || 0),
                  }));
                }}
                sx={{ minWidth: 280, flex: 1 }}
                slotProps={{
                  inputLabel: { shrink: true }
                }}
                helperText={t("config.llmRouterIndexHelp")}
              >
                <MenuItem value="">{t("config.llmIndexNone")}</MenuItem>
                {sources.map((s, idx) => (
                  <MenuItem key={idx} value={idx}>
                    {formatLlmSourceSelectLabel(idx, s, t)}
                  </MenuItem>
                ))}
              </TextField>
              <TextField
                select
                label={t("config.llmWorkerIndex")}
                value={workerIndex === null ? "" : String(workerIndex)}
                onChange={(e) => {
                  markDirty();
                  const v = e.target.value;
                  setDraft((prev) => ({
                    ...prev,
                    workerIndex:
                      v === "" ? null : Math.max(0, parseInt(v, 10) || 0),
                  }));
                }}
                sx={{ minWidth: 280, flex: 1 }}
                slotProps={{
                  inputLabel: { shrink: true }
                }}
                helperText={t("config.llmWorkerIndexHelp")}
              >
                <MenuItem value="">{t("config.llmIndexNone")}</MenuItem>
                {sources.map((s, idx) => (
                  <MenuItem key={idx} value={idx}>
                    {formatLlmSourceSelectLabel(idx, s, t)}
                  </MenuItem>
                ))}
              </TextField>
            </Box>
          </FormSectionSubCollapsible>
        </Stack>
        )}
      </SettingsSection>
    </Box>
  );
}
