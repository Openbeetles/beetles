import type { TFunction } from "i18next";
import type { ProbeDisposition } from "../types/accountConfig";

function translateProbeReason(reason: string | undefined, t: TFunction): string {
  const trimmed = reason?.trim() ?? "";
  if (!trimmed) return "";
  const key = `accounts.probeReason.${trimmed}`;
  const translated = t(key);
  return translated === key ? trimmed : translated;
}

export function formatProbeMessage(
  disposition: ProbeDisposition,
  reason: string | undefined,
  t: TFunction,
): string {
  const dispositionLabel = t(`accounts.probeDisposition.${disposition}`);
  const reasonLabel = translateProbeReason(reason, t);
  return reasonLabel ? `${dispositionLabel} · ${reasonLabel}` : dispositionLabel;
}
