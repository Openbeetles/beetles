export type ApiErrorTranslator = (
  key: string,
  options?: { defaultValue?: string },
) => string;

const DEVICE_OR_PAIRING_ERROR_KEYS = new Set([
  "auth.pairing_required",
  "auth.pairing_invalid",
  "device.bannerNeedDevice",
]);

export function isI18nLikeKey(value: string | undefined): boolean {
  const trimmed = value?.trim() ?? "";
  return /^[a-z]+[a-z0-9_]*(\.[a-zA-Z0-9_]+)+$/.test(trimmed);
}

export function isDeviceOrPairingErrorKey(value: string | undefined): boolean {
  const trimmed = value?.trim() ?? "";
  return DEVICE_OR_PAIRING_ERROR_KEYS.has(trimmed);
}

export function translateApiError(
  t: ApiErrorTranslator,
  message: string | undefined | null,
  fallbackKey = "common.error",
): string {
  const trimmed = message?.trim() ?? "";
  if (!trimmed) return t(fallbackKey);
  if (!isI18nLikeKey(trimmed)) return trimmed;
  const translated = t(trimmed, { defaultValue: trimmed });
  if (translated !== trimmed) return translated;
  return trimmed;
}

export function normalizeThrownErrorMessage(message: string | undefined): string | null {
  const trimmed = message?.trim() ?? "";
  if (!trimmed) return null;
  const normalized = trimmed.toLowerCase();
  if (
    normalized === "failed to fetch" ||
    normalized.endsWith("failed to fetch") ||
    normalized.includes("fetch failed") ||
    normalized.includes("network request failed") ||
    normalized.includes("networkerror")
  ) {
    return "network.request_failed";
  }
  return trimmed;
}
