import type { AccountSummary } from "../types/accountConfig";

type AccountCardTranslator = (key: string) => string;

export type AccountCardStatusColor =
  | "default"
  | "success"
  | "warning"
  | "error";

export interface AccountCardModel {
  eyebrow: string;
  title: string;
  identityLabel: string;
  capabilityLabels: string[];
  readinessLabel: string;
  readinessColor: AccountCardStatusColor;
  showRuntimeFlag: boolean;
}

function accountTitle(row: AccountSummary): string {
  return row.account_label.trim() || row.account_key;
}

function readinessColor(
  readiness: AccountSummary["readiness"],
): AccountCardStatusColor {
  switch (readiness) {
    case "ready":
      return "success";
    case "needs_configuration":
    case "ready_for_probe":
      return "warning";
    default:
      return "default";
  }
}

export function buildAccountCardModel(
  row: AccountSummary,
  {
    t,
    providerLabel,
  }: {
    t: AccountCardTranslator;
    providerLabel: string;
  },
): AccountCardModel {
  const title = accountTitle(row);

  return {
    eyebrow: providerLabel,
    title,
    identityLabel: t(`accounts.identity.${row.identity_class}`),
    capabilityLabels: row.enabled_capabilities.map((capability) =>
      t(`accounts.capabilityShort.${capability}`),
    ),
    readinessLabel: t(`accounts.readiness.${row.readiness}`),
    readinessColor: readinessColor(row.readiness),
    showRuntimeFlag: row.has_runtime_error,
  };
}
