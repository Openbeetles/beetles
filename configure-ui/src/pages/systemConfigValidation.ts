import type { SystemConfigSegment } from "../types/appConfig.ts";

/** 简单校验：非空时须含 :// 且 scheme 后为非空（后端会做完整校验）。 */
export function isValidProxyUrl(value: string): boolean {
  const trimmed = value.trim();
  if (!trimmed) return true;
  const separator = trimmed.indexOf("://");
  return separator !== -1 && separator + 3 < trimmed.length;
}

export function validateSystemConfig(
  form: SystemConfigSegment,
  t: (key: string) => string,
): string | null {
  if (!isValidProxyUrl(form.proxy_url ?? "")) {
    return t("config.validation.proxyUrlInvalid");
  }
  const wifiPassSet = form.wifi_pass.trim().length > 0;
  if (wifiPassSet && !form.wifi_ssid.trim()) {
    return t("config.validation.wifiSsidRequired");
  }
  return null;
}
