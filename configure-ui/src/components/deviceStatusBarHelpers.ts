import type { TFunction } from "i18next";

export function wifiStaLabel(staConnected: boolean | undefined, t: TFunction): string {
  if (staConnected === true) return t("device.wifiStaConnected");
  if (staConnected === false) return t("device.wifiStaDisconnected");
  return t("common.na");
}

export function yesNo(value: boolean | undefined, t: TFunction): string {
  if (value === true) return t("common.yes");
  if (value === false) return t("common.no");
  return t("common.na");
}

/** 与固件 `PressureLevel` 序列化字符串一致：Normal / Cautious / Critical */
export function pressureColor(pressure: string | undefined): string {
  switch (pressure) {
    case "Critical":
      return "var(--semantic-danger)";
    case "Cautious":
      return "var(--semantic-warning)";
    case "Normal":
    default:
      return "var(--semantic-success)";
  }
}
