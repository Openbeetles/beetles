import type {
  AccountAssessment,
  AccountFieldState,
  AccountReadiness,
} from "../types/accountConfig.ts";

export type AccountSummaryStatusTone = "success" | "warning" | "info" | "default";

export interface AccountDetailSummaryStatusItem {
  key: "readiness" | "nextAction";
  labelKey: string;
  tone: AccountSummaryStatusTone;
}

export interface AccountDetailSummaryModel {
  statusItems: AccountDetailSummaryStatusItem[];
  missingFieldsText: string | null;
  probeMessage: string | null;
}

function readinessTone(readiness: AccountReadiness): AccountSummaryStatusTone {
  switch (readiness) {
    case "ready":
      return "success";
    case "needs_configuration":
      return "warning";
    case "ready_for_probe":
      return "info";
    case "probe_unavailable":
      return "default";
  }
}

export function buildAccountDetailSummaryModel(args: {
  assessment: AccountAssessment;
  fields: AccountFieldState[];
  labelFor: (field: AccountFieldState) => string;
  probeMessage: string | null;
}): AccountDetailSummaryModel {
  const { assessment, fields, labelFor, probeMessage } = args;
  const statusItems: AccountDetailSummaryStatusItem[] = [
    {
      key: "readiness",
      labelKey: `accounts.readiness.${assessment.readiness}`,
      tone: readinessTone(assessment.readiness),
    },
  ];
  if (assessment.next_action !== "none") {
    statusItems.push({
      key: "nextAction",
      labelKey: `accounts.nextAction.${assessment.next_action}`,
      tone: "default",
    });
  }

  const missingFieldsText =
    assessment.missing_fields.length > 0
      ? assessment.missing_fields
          .map((fieldKey) => {
            const matchingField = fields.find((field) => field.key === fieldKey);
            return matchingField ? labelFor(matchingField) : fieldKey;
          })
          .join(", ")
      : null;

  return {
    statusItems,
    missingFieldsText,
    probeMessage,
  };
}
