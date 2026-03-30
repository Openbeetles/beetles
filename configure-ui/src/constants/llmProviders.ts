/**
 * LLM 服务商枚举与「api_url 为空时」的默认端点。
 * Keep in sync with firmware:
 * - `src/llm/openai_compatible.rs` (`from_source` empty `api_url` branches)
 * - `src/llm/anthropic.rs` (`API_BASE` when `api_url` empty)
 */

export const LLM_PROVIDER_VALUES = [
  "anthropic",
  "openai",
  "openai_compatible",
  "gemini",
  "glm",
  "qwen",
  "deepseek",
  "moonshot",
  "ollama",
] as const;

export type LlmProviderValue = (typeof LLM_PROVIDER_VALUES)[number];

/** 新建源时的默认下拉项（与历史 UI 一致）。 */
export const DEFAULT_LLM_PROVIDER: LlmProviderValue = "openai_compatible";

/** 与固件默认 base / Anthropic 完整 Messages URL 一致，供配置页预填与切换服务商时提示。 */
export const LLM_DEFAULT_API_URL: Record<LlmProviderValue, string> = {
  anthropic: "https://api.anthropic.com/v1/messages",
  openai: "https://api.openai.com/v1",
  openai_compatible: "https://api.openai.com/v1",
  gemini: "https://generativelanguage.googleapis.com/v1beta",
  glm: "https://open.bigmodel.cn/api/paas/v4",
  qwen: "https://dashscope.aliyuncs.com/compatible-mode/v1",
  deepseek: "https://api.deepseek.com/v1",
  moonshot: "https://api.moonshot.cn/v1",
  ollama: "http://localhost:11434/v1",
};

export function normalizeLlmApiUrl(url: string): string {
  return url.trim().replace(/\/+$/, "");
}

export function defaultApiUrlForProvider(provider: LlmProviderValue): string {
  return LLM_DEFAULT_API_URL[provider];
}

/**
 * 切换 provider 后写入的 api_url：
 * - 当前为空 → 填入新服务商默认地址；
 * - 当前与「旧服务商默认地址」等价（忽略尾部 `/`）→ 同步为新服务商默认，避免误留上一家端点；
 * - 否则保留用户自定义 URL。
 */
export function apiUrlAfterProviderChange(
  currentUrl: string,
  oldProvider: LlmProviderValue,
  newProvider: LlmProviderValue,
): string {
  const trimmed = currentUrl.trim();
  if (!trimmed) {
    return defaultApiUrlForProvider(newProvider);
  }
  const oldDefault = defaultApiUrlForProvider(oldProvider);
  if (
    oldDefault.length > 0 &&
    normalizeLlmApiUrl(trimmed) === normalizeLlmApiUrl(oldDefault)
  ) {
    return defaultApiUrlForProvider(newProvider);
  }
  return currentUrl;
}
