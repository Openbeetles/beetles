import {
  translateApiError,
  type ApiErrorTranslator,
} from "../i18n/apiErrors.ts";

const CHAT_ERROR_OVERRIDES: Record<string, string> = {
  "http.route_worker_memory_low": "chat.resource_pressure",
};

export function translateChatApiError(
  t: ApiErrorTranslator,
  message: string | undefined | null,
  fallbackKey = "chat.sendFailed",
): string {
  const key = message?.trim() ?? "";
  return translateApiError(t, CHAT_ERROR_OVERRIDES[key] ?? message, fallbackKey);
}
