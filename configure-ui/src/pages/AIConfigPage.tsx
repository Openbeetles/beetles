import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import FormControl from "@mui/material/FormControl";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import Button from "@mui/material/Button";
import Switch from "@mui/material/Switch";
import FormControlLabel from "@mui/material/FormControlLabel";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import type { SelectChangeEvent } from "@mui/material/Select";
import AddRounded from "@mui/icons-material/AddRounded";
import DeleteOutlined from "@mui/icons-material/DeleteOutlined";
import SaveRounded from "@mui/icons-material/SaveRounded";
import SmartToyOutlined from "@mui/icons-material/SmartToyOutlined";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  FormFieldStack,
  FormLoadingSkeleton,
  FormSectionSubCollapsible,
  InlineAlert,
  SaveFeedback,
} from "../components/form";
import { SettingsSection } from "../components/SettingsSection";
import { useConfig } from "../hooks/useConfig";
import { useConfigPageLoad } from "../hooks/useConfigPageLoad";
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
  llmStream: boolean;
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
  const { config, loadConfig, saveLlm, loading, error } = useConfig();
  const { setDirty } = useUnsaved();
  const [removeSourceIndex, setRemoveSourceIndex] = useState<number | null>(null);
  const saveFeedback = useSaveFeedback(t);
  const { isRevealed, getRevealHandlers } = useRevealedPasswordFields();

  useConfigPageLoad({ hasConfig: config !== null, loading, loadConfig });
  const syncedDraft = useMemo<LlmDraftState>(() => {
    if (!config) {
      return {
        sources: [],
        routerIndex: null,
        workerIndex: null,
        llmStream: false,
      };
    }
    const list =
      config.llm_sources?.length > 0
        ? config.llm_sources
        : [
            {
              provider: config.model_provider || "",
              api_key: config.api_key || "",
              model: config.model || "",
              api_url: config.api_url || "",
            },
          ];
    return {
      sources: toSourceRows(list),
      routerIndex: config.llm_router_source_index ?? null,
      workerIndex: config.llm_worker_source_index ?? null,
      llmStream: config.llm_stream ?? false,
    };
  }, [config]);
  const [draft, setDraft] = useSyncedState(syncedDraft);
  const { sources, routerIndex, workerIndex, llmStream } = draft;

  const addSource = () => {
    setDirty(true);
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
    setDirty(true);
    setDraft((prev) => ({
      ...prev,
      sources: prev.sources.filter((_, j) => j !== removeSourceIndex),
    }));
    setRemoveSourceIndex(null);
  };

  const updateSource = (i: number, field: keyof LlmSource, value: string) => {
    setDirty(true);
    setDraft((prev) => {
      const next = [...prev.sources];
      next[i] = { ...next[i], [field]: value };
      return { ...prev, sources: next };
    });
  };

  const changeProvider = (i: number, newProviderRaw: string) => {
    const newP = normalizeProvider(newProviderRaw);
    setDirty(true);
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
    if (!config) return;
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
      llm_stream: llmStream,
    });
    saveFeedback.finishFromResult(result);
    if (result.ok) setDirty(false);
  };

  if (loading && !config) {
    return (
      <Box sx={{ display: "flex", flexDirection: "column", gap: 4 }}>
        <SettingsSection
          icon={<SmartToyOutlined sx={{ fontSize: "var(--icon-size-md)" }} />}
          label={t("config.sectionLlm")}
        >
          <FormLoadingSkeleton />
        </SettingsSection>
      </Box>
    );
  }

  const saveDisabled = !config || saveFeedback.status === "saving";

  return (
    <Box sx={{ display: "flex", flexDirection: "column", gap: 4 }}>
      <InlineAlert message={error} onRetry={loadConfig} />
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
        icon={<SmartToyOutlined sx={{ fontSize: "var(--icon-size-md)" }} />}
        label={t("config.sectionLlm")}
        description={t("config.sectionLlmDesc")}
        accessory={
          <Button
            size="small"
            variant="contained"
            startIcon={<SaveRounded />}
            onClick={handleSave}
            disabled={saveDisabled}
            title={!config ? t("config.hintSaveNeedDevice") : undefined}
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
        {!config && !loading && (
          <Typography variant="body2" color="text.secondary" sx={{ pb: 2 }}>
            {t("config.hintSaveNeedDevice")}
          </Typography>
        )}
        <Stack spacing={0}>
          <FormControlLabel
            control={
              <Switch
                checked={llmStream}
                onChange={(e) => {
                  setDirty(true);
                  setDraft((prev) => ({
                    ...prev,
                    llmStream: e.target.checked,
                  }));
                }}
              />
            }
            label={t("config.llmStream")}
            sx={{ mb: 1 }}
          />
          <Typography variant="caption" color="text.secondary" sx={{ ml: 7, mt: -1, mb: 2 }}>
            {t("config.llmStreamHelp")}
          </Typography>
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
                    color: "var(--muted)",
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
                <FormControl size="small" fullWidth>
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
                  size="small"
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
                  size="small"
                  fullWidth
                  placeholder={defaultModelForProvider(row.provider)}
                  slotProps={{ htmlInput: { maxLength: MAX_LEN } }}
                />
                <TextField
                  label={t("config.llmApiUrl")}
                  value={row.api_url}
                  onChange={(e) => updateSource(i, "api_url", e.target.value)}
                  size="small"
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
            <Box sx={{ display: "flex", gap: 2, flexWrap: "wrap" }}>
              <TextField
                select
                label={t("config.llmRouterIndex")}
                value={routerIndex === null ? "" : String(routerIndex)}
                onChange={(e) => {
                  setDirty(true);
                  const v = e.target.value;
                  setDraft((prev) => ({
                    ...prev,
                    routerIndex:
                      v === "" ? null : Math.max(0, parseInt(v, 10) || 0),
                  }));
                }}
                size="small"
                sx={{ minWidth: 280, flex: 1 }}
                slotProps={{
                  inputLabel: { shrink: true },
                  select: { native: true },
                }}
                helperText={t("config.llmRouterIndexHelp")}
              >
                <option value="">{t("config.llmIndexNone")}</option>
                {sources.map((s, idx) => (
                  <option key={idx} value={idx}>
                    {formatLlmSourceSelectLabel(idx, s, t)}
                  </option>
                ))}
              </TextField>
              <TextField
                select
                label={t("config.llmWorkerIndex")}
                value={workerIndex === null ? "" : String(workerIndex)}
                onChange={(e) => {
                  setDirty(true);
                  const v = e.target.value;
                  setDraft((prev) => ({
                    ...prev,
                    workerIndex:
                      v === "" ? null : Math.max(0, parseInt(v, 10) || 0),
                  }));
                }}
                size="small"
                sx={{ minWidth: 280, flex: 1 }}
                slotProps={{
                  inputLabel: { shrink: true },
                  select: { native: true },
                }}
                helperText={t("config.llmWorkerIndexHelp")}
              >
                <option value="">{t("config.llmIndexNone")}</option>
                {sources.map((s, idx) => (
                  <option key={idx} value={idx}>
                    {formatLlmSourceSelectLabel(idx, s, t)}
                  </option>
                ))}
              </TextField>
            </Box>
          </FormSectionSubCollapsible>
        </Stack>
      </SettingsSection>
    </Box>
  );
}
